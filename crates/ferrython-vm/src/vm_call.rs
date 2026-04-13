//! Function/method call dispatch, class instantiation, super().

use crate::builtins;
use crate::frame::{Frame, ScopeKind};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject};
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{ FxHashKeyMap, new_fx_hashkey_map, PyCell, 
    AsyncGenAction, CompareOp, IteratorData, PartialData, PropertyData, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef, is_data_descriptor, has_descriptor_get, lookup_in_class_mro,
    get_builtin_base_type_name,
};
use ferrython_core::types::{HashableKey, SharedConstantCache, SharedGlobals};
use indexmap::IndexMap;
use std::rc::Rc;

/// Attach `split` and `subgroup` methods to an ExceptionGroup instance.
/// Reads `message` and `exceptions` from the instance attrs.
fn attach_eg_methods(eg: &PyObjectRef) {
    if let PyObjectPayload::ExceptionInstance(ei) = &eg.payload {
        let (msg, exc_list) = {
            let a = ei.ensure_attrs().read();
            let msg = a.get(&CompactString::from("message"))
                .cloned()
                .unwrap_or_else(|| PyObject::str_val(CompactString::from("")));
            let exc_list = a.get(&CompactString::from("exceptions"))
                .cloned()
                .unwrap_or_else(|| PyObject::list(vec![]));
            (msg, exc_list)
        };
        let msg_sg = msg.clone();
        let exc_sg = exc_list.clone();
        let msg_sp = msg;
        let exc_sp = exc_list;
        let mut a = ei.ensure_attrs().write();
        a.insert(CompactString::from("subgroup"), PyObject::native_closure(
            "ExceptionGroup.subgroup",
            move |sg_args| {
                let filter_type = if sg_args.len() > 1 { &sg_args[1] } else if !sg_args.is_empty() { &sg_args[0] } else {
                    return Ok(PyObject::none());
                };
                let filter_kind = match &filter_type.payload {
                    PyObjectPayload::ExceptionType(k) => Some(k.clone()),
                    _ => ExceptionKind::from_name(&filter_type.py_to_string()),
                };
                let items = exc_sg.to_list().unwrap_or_default();
                let matched: Vec<PyObjectRef> = items.into_iter().filter(|exc| {
                    if let Some(ref fk) = filter_kind {
                        if let PyObjectPayload::ExceptionInstance(ei) = &exc.payload {
                            return ei.kind.is_subclass_of(fk);
                        }
                    }
                    false
                }).collect();
                if matched.is_empty() { return Ok(PyObject::none()); }
                let new_eg = PyObject::exception_instance(ExceptionKind::ExceptionGroup, msg_sg.py_to_string());
                if let PyObjectPayload::ExceptionInstance(ei) = &new_eg.payload {
                    let mut ew = ei.ensure_attrs().write();
                    ew.insert(CompactString::from("message"), msg_sg.clone());
                    ew.insert(CompactString::from("exceptions"), PyObject::list(matched));
                }
                attach_eg_methods(&new_eg);
                Ok(new_eg)
            }
        ));
        a.insert(CompactString::from("split"), PyObject::native_closure(
            "ExceptionGroup.split",
            move |sp_args| {
                let filter_type = if sp_args.len() > 1 { &sp_args[1] } else if !sp_args.is_empty() { &sp_args[0] } else {
                    return Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none()]));
                };
                let filter_kind = match &filter_type.payload {
                    PyObjectPayload::ExceptionType(k) => Some(k.clone()),
                    _ => ExceptionKind::from_name(&filter_type.py_to_string()),
                };
                let items = exc_sp.to_list().unwrap_or_default();
                let mut matched = Vec::new();
                let mut rest = Vec::new();
                for exc in items {
                    let matches = if let Some(ref fk) = filter_kind {
                        if let PyObjectPayload::ExceptionInstance(ei) = &exc.payload {
                            ei.kind.is_subclass_of(fk)
                        } else { false }
                    } else { false };
                    if matches { matched.push(exc); } else { rest.push(exc); }
                }
                let make_eg = |msg: &PyObjectRef, items: Vec<PyObjectRef>| -> PyObjectRef {
                    if items.is_empty() { return PyObject::none(); }
                    let eg = PyObject::exception_instance(ExceptionKind::ExceptionGroup, msg.py_to_string());
                    if let PyObjectPayload::ExceptionInstance(ei) = &eg.payload {
                        let mut ew = ei.ensure_attrs().write();
                        ew.insert(CompactString::from("message"), msg.clone());
                        ew.insert(CompactString::from("exceptions"), PyObject::list(items));
                    }
                    attach_eg_methods(&eg);
                    eg
                };
                Ok(PyObject::tuple(vec![make_eg(&msg_sp, matched), make_eg(&msg_sp, rest)]))
            }
        ));
    }
}

impl VirtualMachine {
    /// Write text to a file-like object, handling both BoundMethod (e.g. StringIO)
    /// and NativeFunction (e.g. default sys.stdout) cases.
    fn write_to_file_object(&mut self, target: &PyObjectRef, text: &str) -> PyResult<()> {
        if let Some(write_fn) = target.get_attr("write") {
            let text_obj = PyObject::str_val(CompactString::from(text));
            match &write_fn.payload {
                // Bound methods already include self in dispatch
                PyObjectPayload::BoundMethod { .. }
                | PyObjectPayload::BuiltinBoundMethod(_) => {
                    self.call_object(write_fn, vec![text_obj])?;
                }
                // NativeClosure (e.g. StringIO.write): instance method stored on instance dict
                PyObjectPayload::NativeClosure(_) => {
                    self.call_object(write_fn, vec![text_obj])?;
                }
                // Raw NativeFunction (e.g. default stdio): prepend self
                _ => {
                    self.call_object(write_fn, vec![target.clone(), text_obj])?;
                }
            }
            Ok(())
        } else {
            print!("{}", text);
            Ok(())
        }
    }

    /// Resolve the output target for print(): file= kwarg > sys.stdout > native stdout.
    fn resolve_print_target(&self, explicit_file: Option<PyObjectRef>) -> Option<PyObjectRef> {
        explicit_file
            .or_else(|| ferrython_stdlib::get_stdout_override())
            .or_else(|| self.modules.get("sys").and_then(|s| s.get_attr("stdout")))
    }

    /// str.format_map() with dict subclass mapping, supporting __missing__ via VM call dispatch.
    fn vm_format_map(
        &mut self,
        template: &str,
        mapping: &PyObjectRef,
        dict_storage: &Rc<PyCell<FxHashKeyMap>>,
        mapping_class: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let mut result = String::new();
        let mut chars = template.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    result.push('{');
                } else {
                    let mut field = String::new();
                    for c in chars.by_ref() {
                        if c == '}' { break; }
                        field.push(c);
                    }
                    let key = HashableKey::str_key(CompactString::from(&field));
                    if let Some(val) = dict_storage.read().get(&key) {
                        result.push_str(&val.py_to_string());
                    } else if let Some(missing_fn) = lookup_in_class_mro(mapping_class, "__missing__") {
                        // Call __missing__(self, key) via VM dispatch
                        let key_obj = PyObject::str_val(CompactString::from(&field));
                        let val = self.call_object(missing_fn, vec![mapping.clone(), key_obj])?;
                        result.push_str(&val.py_to_string());
                    } else {
                        return Err(PyException::key_error(field));
                    }
                }
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                result.push('}');
            } else {
                result.push(c);
            }
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }

    /// str.format_map() for defaultdict (Dict payload with __defaultdict_factory__).
    fn vm_format_map_dict(
        &mut self,
        template: &str,
        _mapping: &PyObjectRef,
        dict: &Rc<PyCell<FxHashKeyMap>>,
    ) -> PyResult<PyObjectRef> {
        let factory_key = HashableKey::str_key(CompactString::from("__defaultdict_factory__"));
        let mut result = String::new();
        let mut chars = template.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    result.push('{');
                } else {
                    let mut field = String::new();
                    for c in chars.by_ref() {
                        if c == '}' { break; }
                        field.push(c);
                    }
                    let key = HashableKey::str_key(CompactString::from(&field));
                    let guard = dict.read();
                    if let Some(val) = guard.get(&key) {
                        result.push_str(&val.py_to_string());
                    } else if let Some(factory) = guard.get(&factory_key).cloned() {
                        drop(guard);
                        let val = self.call_object(factory, vec![])?;
                        dict.write().insert(key, val.clone());
                        result.push_str(&val.py_to_string());
                    } else {
                        return Err(PyException::key_error(field));
                    }
                }
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                result.push('}');
            } else {
                result.push(c);
            }
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }

    /// Collect the current frame's local variables into a dict.
    /// At module scope, locals() == globals().
    fn collect_locals_dict(&self) -> PyResult<PyObjectRef> {
        let frame = self.call_stack.last().unwrap();
        if matches!(frame.scope_kind, ScopeKind::Module) {
            // At module level, locals() == globals()
            let g = frame.globals.read();
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = g.iter()
                .map(|(k, v)| (PyObject::str_val(CompactString::from(k.as_str())), v.clone()))
                .collect();
            drop(g);
            return Ok(PyObject::dict_from_pairs(pairs));
        }
        let mut pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
        // Fast locals (function parameters and local variables)
        for (i, name) in frame.code.varnames.iter().enumerate() {
            if let Some(Some(val)) = frame.locals.get(i) {
                pairs.push((PyObject::str_val(name.clone()), val.clone()));
            }
        }
        // local_names (class scope, etc.)
        for (k, v) in frame.local_names_iter() {
            pairs.push((PyObject::str_val(k.clone()), v.clone()));
        }
        // Cell and free variables
        for (i, name) in frame.code.cellvars.iter().chain(frame.code.freevars.iter()).enumerate() {
            if let Some(cell) = frame.cells.get(i) {
                let cell_val = cell.read();
                if let Some(val) = cell_val.as_ref() {
                    pairs.push((PyObject::str_val(name.clone()), val.clone()));
                }
            }
        }
        Ok(PyObject::dict_from_pairs(pairs))
    }

    pub(crate) fn call_function(
        &mut self,
        code: &Rc<CodeObject>,
        mut args: Vec<PyObjectRef>,
        defaults: &[PyObjectRef],
        kw_defaults: &IndexMap<CompactString, PyObjectRef>,
        globals: SharedGlobals,
        closure: &[Rc<PyCell<Option<PyObjectRef>>>],
        constant_cache: &SharedConstantCache,
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new_from_pool(Rc::clone(code), globals, self.builtins.clone(), Rc::clone(constant_cache), &mut self.frame_pool);
        let nparams = code.arg_count as usize;
        let nkwonly = code.kwonlyarg_count as usize;
        let has_varargs = code.flags.contains(CodeFlags::VARARGS);
        let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);

        let nargs = args.len();
        let positional_count = nargs.min(nparams);

        // Move positional args into locals (zero-clone via drain)
        {
            let mut drain = args.drain(..positional_count);
            for i in 0..positional_count {
                frame.set_local(i, drain.next().unwrap());
            }
        }
        // `args` now contains only surplus positional args (if any)

        // Fill in defaults for missing positional args
        if nargs < nparams && !defaults.is_empty() {
            let ndefaults = defaults.len();
            let first_default_param = nparams - ndefaults;
            for i in nargs..nparams {
                if i >= first_default_param {
                    let default_idx = i - first_default_param;
                    frame.set_local(i, defaults[default_idx].clone());
                }
            }
        }

        // Check for missing required positional args
        if nargs < nparams {
            let ndefaults = defaults.len();
            let required = nparams - ndefaults;
            if nargs < required {
                let missing = required - nargs;
                let fname = code.name.as_str();
                let missing_names: Vec<&str> = (nargs..required)
                    .filter_map(|i| code.varnames.get(i).map(|s| s.as_str()))
                    .collect();
                return Err(PyException::type_error(format!(
                    "{}() missing {} required positional argument{}: {}",
                    fname, missing, if missing == 1 { "" } else { "s" },
                    missing_names.iter().map(|n| format!("'{}'", n)).collect::<Vec<_>>().join(", ")
                )));
            }
        }

        // Pack extra positional args into *args tuple, or raise TypeError
        if has_varargs {
            let extra: Vec<PyObjectRef> = if nargs > nparams {
                args // already drained to only surplus args
            } else {
                Vec::new()
            };
            frame.set_local(nparams, PyObject::tuple(extra));
        } else if nargs > nparams {
            let fname = code.name.as_str();
            return Err(PyException::type_error(format!(
                "{}() takes {} positional argument{} but {} {} given",
                fname, nparams, if nparams == 1 { "" } else { "s" },
                nargs, if nargs == 1 { "was" } else { "were" }
            )));
        }

        // Fill in kw_defaults for keyword-only args
        let kwonly_start = if has_varargs { nparams + 1 } else { nparams };
        for i in 0..nkwonly {
            let slot = kwonly_start + i;
            if frame.locals.get(slot).map_or(true, |v| v.is_none()) {
                if let Some(varname) = code.varnames.get(slot) {
                    if let Some(default_val) = kw_defaults.get(varname.as_str()) {
                        frame.set_local(slot, default_val.clone());
                    }
                }
            }
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            if frame.locals.get(kwargs_idx).map_or(true, |v| v.is_none()) {
                frame.set_local(kwargs_idx, PyObject::dict(new_fx_hashkey_map()));
            }
        }

        self.install_closure_and_run(frame, code, closure)
    }

    pub(crate) fn call_function_kw(
        &mut self,
        code: &Rc<CodeObject>,
        mut pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
        defaults: &[PyObjectRef],
        kw_defaults: &IndexMap<CompactString, PyObjectRef>,
        globals: SharedGlobals,
        closure: &[Rc<PyCell<Option<PyObjectRef>>>],
        constant_cache: &SharedConstantCache,
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new_from_pool(Rc::clone(code), globals, self.builtins.clone(), Rc::clone(constant_cache), &mut self.frame_pool);
        let nparams = code.arg_count as usize;
        let nkwonly = code.kwonlyarg_count as usize;
        let has_varargs = code.flags.contains(CodeFlags::VARARGS);
        let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);

        // Total named parameters (positional + keyword-only)
        let _total_named = nparams + nkwonly;
        // Varargs slot comes after positional params
        let varargs_slot = nparams;
        // Keyword-only params start after *args slot (if present)
        let kwonly_start = if has_varargs { nparams + 1 } else { nparams };

        let npos = pos_args.len();
        let positional_count = npos.min(nparams);

        // Move positional args into locals (zero-clone via drain)
        {
            let mut drain = pos_args.drain(..positional_count);
            for i in 0..positional_count {
                frame.set_local(i, drain.next().unwrap());
            }
        }
        // `pos_args` now contains only surplus positional args (if any)

        // Place keyword args at their correct parameter positions
        // Build a name→index lookup for O(1) kwarg matching
        let posonlyarg_count = code.posonlyarg_count as usize;
        let mut extra_kwargs: FxHashKeyMap = new_fx_hashkey_map();
        // Pre-build varname→index map for fast lookup when kwargs > 2
        let varname_map: Option<std::collections::HashMap<&str, usize>> = if kwargs.len() > 2 {
            Some(code.varnames.iter().enumerate().map(|(i, v)| (v.as_str(), i)).collect())
        } else {
            None
        };
        for (name, val) in kwargs {
            let found_idx = if let Some(ref map) = varname_map {
                map.get(name.as_str()).copied()
            } else {
                code.varnames.iter().position(|v| v.as_str() == name.as_str())
            };
            if let Some(idx) = found_idx {
                // Reject positional-only parameters passed as keyword arguments
                if idx < posonlyarg_count {
                    return Err(PyException::type_error(format!(
                        "{}() got some positional-only arguments passed as keyword arguments: '{}'",
                        code.name, name
                    )));
                }
                // Accept both positional params (< nparams) and kwonly params
                let is_positional = idx < nparams;
                let is_kwonly = idx >= kwonly_start && idx < kwonly_start + nkwonly;
                if is_positional || is_kwonly {
                    frame.set_local(idx, val);
                    continue;
                }
            }
            // Not a known parameter — goes into **kwargs
            extra_kwargs.insert(
                HashableKey::str_key(name),
                val,
            );
        }

        // Fill in defaults for missing positional args
        if !defaults.is_empty() {
            let ndefaults = defaults.len();
            let first_default_param = nparams - ndefaults;
            for i in 0..nparams {
                if frame.locals[i].is_none() && i >= first_default_param {
                    let default_idx = i - first_default_param;
                    frame.set_local(i, defaults[default_idx].clone());
                }
            }
        }

        // Fill in kw_defaults for missing keyword-only args
        for i in 0..nkwonly {
            let slot = kwonly_start + i;
            if frame.locals.get(slot).map_or(true, |v| v.is_none()) {
                if let Some(varname) = code.varnames.get(slot) {
                    if let Some(default_val) = kw_defaults.get(varname.as_str()) {
                        frame.set_local(slot, default_val.clone());
                    }
                }
            }
        }

        // Pack extra positional args into *args tuple, or raise TypeError
        if has_varargs {
            let extra: Vec<PyObjectRef> = if npos > nparams {
                pos_args // already drained to only surplus args
            } else {
                Vec::new()
            };
            frame.set_local(varargs_slot, PyObject::tuple(extra));
        } else if npos > nparams {
            let fname = code.name.as_str();
            return Err(PyException::type_error(format!(
                "{}() takes {} positional argument{} but {} {} given",
                fname, nparams, if nparams == 1 { "" } else { "s" },
                npos, if npos == 1 { "was" } else { "were" }
            )));
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            frame.set_local(kwargs_idx, PyObject::dict(extra_kwargs));
        }

        self.install_closure_and_run(frame, code, closure)
    }

    /// Unified class instantiation: __new__, dataclass/namedtuple auto-init, __init__, exception attrs.
    pub(crate) fn instantiate_class(
        &mut self,
        cls: &PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        // ── ABC check (must run before any fast path) ──
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let is_abstract_marker = |val: &PyObjectRef| -> bool {
                if let PyObjectPayload::Tuple(items) = &val.payload {
                    items.len() == 2 && items[0].as_str() == Some("__abstract__")
                } else if let PyObjectPayload::Property(pd) = &val.payload {
                    if let Some(fg) = &pd.fget {
                        if let PyObjectPayload::Tuple(items) = &fg.payload {
                            return items.len() == 2 && items[0].as_str() == Some("__abstract__");
                        }
                    }
                    false
                } else {
                    false
                }
            };
            let mut abstract_names: Vec<String> = Vec::new();
            {
                let ns = cd.namespace.read();
                for (name, val) in ns.iter() {
                    if is_abstract_marker(val) {
                        abstract_names.push(name.to_string());
                    }
                }
            }
            for ancestor in &cd.mro {
                if let PyObjectPayload::Class(ancestor_cd) = &ancestor.payload {
                    let ancestor_ns = ancestor_cd.namespace.read();
                    for (name, val) in ancestor_ns.iter() {
                        if !is_abstract_marker(val) { continue; }
                        let overridden_in_own = cd.namespace.read().get(name.as_str())
                            .map(|v| !is_abstract_marker(v)).unwrap_or(false);
                        let overridden_in_mro = cd.mro.iter().any(|m| {
                            if PyObjectRef::ptr_eq(m, ancestor) { return false; }
                            if let PyObjectPayload::Class(mcd) = &m.payload {
                                mcd.namespace.read().get(name.as_str())
                                    .map(|v| !is_abstract_marker(v)).unwrap_or(false)
                            } else { false }
                        });
                        if !overridden_in_own && !overridden_in_mro && !abstract_names.contains(&name.to_string()) {
                            abstract_names.push(name.to_string());
                        }
                    }
                }
            }
            if !abstract_names.is_empty() {
                abstract_names.sort();
                return Err(PyException::type_error(format!(
                    "Can't instantiate abstract class {} with abstract method{}{}",
                    cd.name,
                    if abstract_names.len() > 1 { "s " } else { " " },
                    abstract_names.join(", ")
                )));
            }
        }
        // ── FAST PATH: simple class with no enum/abstract/__new__/dataclass/namedtuple ──
        if let PyObjectPayload::Class(cd) = &cls.payload {
            // Also check for __new__ added after class creation (is_simple_class is stale)
            if cd.is_simple_class && kwargs.is_empty() && !cd.namespace.read().contains_key("__new__") {
                let instance = PyObject::instance(cls.clone());
                // Look up __init__ via MRO (may be inherited from a base class).
                // Use namespace first (own class), then fall back to MRO.
                let init_fn = cd.namespace.read().get("__init__").cloned()
                    .or_else(|| lookup_in_class_mro(cls, "__init__"));
                if let Some(init_fn) = init_fn {
                    let mut init_args = Vec::with_capacity(1 + pos_args.len());
                    init_args.push(instance.clone());
                    init_args.extend(pos_args.clone());
                    let init_result = self.call_object(init_fn, init_args)?;
                    if !matches!(&init_result.payload, PyObjectPayload::None) {
                        return Err(PyException::type_error(
                            "__init__() should return None, not '".to_string()
                                + init_result.type_name() + "'"
                        ));
                    }
                }
                // Exception subclass: set args tuple
                if Self::is_exception_class(cls) {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        let mut attrs = inst.attrs.write();
                        if !attrs.contains_key("args") {
                            if pos_args.len() == 1 {
                                attrs.insert(CompactString::from("message"), pos_args[0].clone());
                            }
                            attrs.insert(CompactString::from("args"), PyObject::tuple(pos_args));
                        }
                    }
                }
                return Ok(instance);
            }
        }

        // ── STANDARD PATH ──
        // Enum lookup: Color(2) returns the member with that value
        // Also handle Enum functional API: Enum("Name", "mem1 mem2") or Enum("Name", ["mem1", "mem2"])
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let is_enum_base = cd.name.as_str() == "Enum" || cd.name.as_str() == "Flag"
                || cd.name.as_str() == "IntEnum" || cd.name.as_str() == "IntFlag"
                || cd.name.as_str() == "StrEnum";
            // Functional API: Enum("Name", "member1 member2") or Enum("Name", [...])
            if is_enum_base && pos_args.len() >= 2 {
                if let PyObjectPayload::Str(ref name_str) = pos_args[0].payload {
                    // Collect (name, value) pairs from different input formats
                    let members: Vec<(String, PyObjectRef)> = match &pos_args[1].payload {
                        PyObjectPayload::Str(s) => {
                            // "member1 member2" or "member1,member2"
                            s.replace(',', " ").split_whitespace().enumerate()
                                .map(|(i, n)| (n.to_string(), PyObject::int((i + 1) as i64)))
                                .collect()
                        }
                        PyObjectPayload::List(items) => {
                            items.read().iter().enumerate()
                                .map(|(i, item)| (item.py_to_string(), PyObject::int((i + 1) as i64)))
                                .collect()
                        }
                        PyObjectPayload::Tuple(items) => {
                            items.iter().enumerate()
                                .map(|(i, item)| (item.py_to_string(), PyObject::int((i + 1) as i64)))
                                .collect()
                        }
                        PyObjectPayload::Dict(map) => {
                            map.read().iter().map(|(k, v)| {
                                let name = match k {
                                    HashableKey::Str(s) => s.to_string(),
                                    _ => format!("{:?}", k),
                                };
                                (name, v.clone())
                            }).collect()
                        }
                        _ => vec![],
                    };
                    if !members.is_empty() {
                        let mut ns = IndexMap::new();
                        ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
                        let new_cls = PyObject::class(name_str.clone(), vec![cls.clone()], ns);
                        if let PyObjectPayload::Class(ref new_cd) = new_cls.payload {
                            let mut new_ns = new_cd.namespace.write();
                            for (member_name, member_value) in &members {
                                let member = PyObject::instance_with_attrs(
                                    new_cls.clone(),
                                    {
                                        let mut m = IndexMap::new();
                                        m.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(member_name.as_str())));
                                        m.insert(CompactString::from("value"), member_value.clone());
                                        m.insert(CompactString::from("_name_"), PyObject::str_val(CompactString::from(member_name.as_str())));
                                        m.insert(CompactString::from("_value_"), member_value.clone());
                                        m
                                    },
                                );
                                new_ns.insert(CompactString::from(member_name.as_str()), member);
                            }
                        }
                        return Ok(new_cls);
                    }
                }
            }
            if cd.namespace.read().contains_key("__enum__") && pos_args.len() == 1 && kwargs.is_empty() {
                let target_val = &pos_args[0];
                let ns = cd.namespace.read();
                for (_, member) in ns.iter() {
                    if let PyObjectPayload::Instance(inst) = &member.payload {
                        if let Some(val) = inst.attrs.read().get("value") {
                            if val.compare(target_val, CompareOp::Eq)
                                .map(|r| r.is_truthy())
                                .unwrap_or(false)
                            {
                                return Ok(member.clone());
                            }
                        }
                    }
                }
                return Err(PyException::value_error(format!(
                    "{} is not a valid {}", target_val.repr(), cd.name
                )));
            }
        }
        // __new__
        let instance = if let Some(new_method) = cls.get_attr("__new__") {
            // If __new__ is from a BuiltinType base (dict, list, etc.), just create instance
            let is_builtin_new = matches!(&new_method.payload,
                PyObjectPayload::BuiltinBoundMethod(bbm)
                    if matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(_))
            );
            // Also recognize builtin __new__ NativeFunctions (tuple.__new__, list.__new__, etc.)
            let is_native_builtin_new = matches!(&new_method.payload,
                PyObjectPayload::NativeFunction(nf)
                    if nf.name.ends_with(".__new__") && matches!(nf.name.as_str(),
                        "tuple.__new__" | "list.__new__" | "str.__new__" | "int.__new__"
                        | "float.__new__" | "object.__new__")
            );
            if is_builtin_new || is_native_builtin_new {
                let inst = PyObject::instance(cls.clone());
                // For builtin type subclasses (int, str, float), store the constructor
                // argument as __builtin_value__ so arithmetic/methods work correctly.
                if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                    if let Some(base_type) = get_builtin_base_type_name(cls) {
                        let value = if pos_args.is_empty() {
                            // No-arg defaults for builtin type subclasses
                            match base_type.as_str() {
                                "list" => Some(PyObject::list(vec![])),
                                "dict" => Some(PyObject::dict(new_fx_hashkey_map())),
                                "set" => Some(PyObject::set(new_fx_hashkey_map())),
                                "tuple" => Some(PyObject::tuple(vec![])),
                                "int" => Some(PyObject::int(0)),
                                "float" => Some(PyObject::float(0.0)),
                                "str" => Some(PyObject::str_val(CompactString::from(""))),
                                "bytes" => Some(PyObject::bytes(vec![])),
                                "bytearray" => Some(PyObject::bytes(vec![])),
                                _ => None,
                            }
                        } else {
                            match base_type.as_str() {
                                "int" => {
                                    let arg = &pos_args[0];
                                    match &arg.payload {
                                        PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => Some(arg.clone()),
                                        PyObjectPayload::Float(f) => Some(PyObject::int(*f as i64)),
                                        PyObjectPayload::Str(s) => s.trim().parse::<i64>().ok().map(PyObject::int),
                                        _ => None,
                                    }
                                }
                                "float" => {
                                    let arg = &pos_args[0];
                                    match &arg.payload {
                                        PyObjectPayload::Float(_) => Some(arg.clone()),
                                        PyObjectPayload::Int(n) => Some(PyObject::float(n.to_f64())),
                                        PyObjectPayload::Bool(b) => Some(PyObject::float(if *b { 1.0 } else { 0.0 })),
                                        PyObjectPayload::Str(s) => s.trim().parse::<f64>().ok().map(PyObject::float),
                                        _ => None,
                                    }
                                }
                                "str" => {
                                    // str(bytes, encoding) → decode
                                    if pos_args.len() >= 2 {
                                        match &pos_args[0].payload {
                                            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                                                let s = String::from_utf8_lossy(b);
                                                return Ok(PyObject::str_val(CompactString::from(s.as_ref())));
                                            }
                                            _ => {}
                                        }
                                    }
                                    // Use vm_str for VM-aware conversion (calls __str__/__repr__)
                                    match self.vm_str(&pos_args[0]) {
                                        Ok(s) => Some(PyObject::str_val(CompactString::from(s))),
                                        Err(_) => {
                                            let s = pos_args[0].py_to_string();
                                            Some(PyObject::str_val(CompactString::from(s)))
                                        }
                                    }
                                }
                                "list" => {
                                    Some(PyObject::list(pos_args[0].to_list().unwrap_or_default()))
                                }
                                "tuple" => {
                                    // Namedtuple: multiple positional args → store as tuple
                                    // Regular tuple subclass: single iterable arg → expand to tuple
                                    if pos_args.len() > 1 {
                                        Some(PyObject::tuple(pos_args.clone()))
                                    } else {
                                        let items = pos_args[0].to_list().unwrap_or_default();
                                        Some(PyObject::tuple(items))
                                    }
                                }
                                "set" => {
                                    Some(pos_args[0].clone())
                                }
                                "bytes" => {
                                    Some(pos_args[0].clone())
                                }
                                "bytearray" => {
                                    Some(pos_args[0].clone())
                                }
                                _ => None,
                            }
                        };
                        if let Some(val) = value {
                            inst_data.attrs.write().insert(
                                intern_or_new("__builtin_value__"), val,
                            );
                        }
                    }
                }
                inst
            } else {
                let new_fn = match &new_method.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => new_method.clone(),
                };
                let mut new_args = vec![cls.clone()];
                new_args.extend(pos_args.clone());
                // Forward kwargs to __new__
                if kwargs.is_empty() {
                    self.call_object(new_fn, new_args)?
                } else {
                    self.call_object_kw(new_fn, new_args, kwargs.clone())?
                }
            }
        } else {
            PyObject::instance(cls.clone())
        };

        // Check markers in class namespace directly, not via get_attr,
        // because BuiltinType get_attr can return false positives.
        let class_has_key = |obj: &PyObjectRef, key: &str| -> bool {
            // Check the class itself and its MRO (base classes)
            if let PyObjectPayload::Class(cd) = &obj.payload {
                if cd.namespace.read().contains_key(key) {
                    return true;
                }
                for base in &cd.bases {
                    if let PyObjectPayload::Class(bcd) = &base.payload {
                        if bcd.namespace.read().contains_key(key) {
                            return true;
                        }
                    }
                }
            }
            false
        };
        let is_dataclass = class_has_key(cls, "__dataclass__");
        let has_user_init = cls.get_attr("__init__").is_some();

        if is_dataclass && !has_user_init {
            // Dataclass auto-init: populate fields from args/kwargs
            let is_frozen = class_has_key(cls, "__dataclass_frozen__");
            if let Some(fields) = cls.get_attr("__dataclass_fields__") {
                // __dataclass_fields__ can be either:
                // - Tuple of (name, has_default, default_val, init_flag) tuples (legacy VM format)
                // - Dict mapping field_name → Field instance (Python dataclasses format)
                let field_entries: Vec<(String, bool, PyObjectRef, bool)> = match &fields.payload {
                    PyObjectPayload::Tuple(field_tuples) => {
                        field_tuples.iter().filter_map(|ft| {
                            if let PyObjectPayload::Tuple(info) = &ft.payload {
                                let name = info[0].py_to_string();
                                let has_default = info[1].is_truthy();
                                let default_val = info[2].clone();
                                let field_init = if info.len() > 3 { info[3].is_truthy() } else { true };
                                Some((name, has_default, default_val, field_init))
                            } else { None }
                        }).collect()
                    }
                    PyObjectPayload::Dict(map) => {
                        // Dict of {name: Field} — extract field info from Field instances
                        let r = map.read();
                        r.iter().map(|(k, field_obj)| {
                            let name = match k {
                                HashableKey::Str(s) => s.to_string(),
                                _ => field_obj.get_attr("name")
                                    .map(|n| n.py_to_string())
                                    .unwrap_or_default(),
                            };
                            let field_init = field_obj.get_attr("init")
                                .map(|v| v.is_truthy())
                                .unwrap_or(true);
                            // Use __has_default__ flag (set by our Rust dataclass_apply)
                            // to reliably distinguish "no default" from "default is None"
                            let has_default_flag = field_obj.get_attr("__has_default__")
                                .map(|v| v.is_truthy())
                                .unwrap_or(false);
                            let default_factory = field_obj.get_attr("default_factory");
                            let has_factory = default_factory.as_ref()
                                .map(|f| f.is_callable())
                                .unwrap_or(false);
                            let (has_default, default_val) = if has_factory {
                                (true, default_factory.unwrap_or_else(PyObject::none))
                            } else if has_default_flag {
                                let default = field_obj.get_attr("default").unwrap_or_else(PyObject::none);
                                (true, default)
                            } else {
                                (false, PyObject::none())
                            };
                            (name, has_default, default_val, field_init)
                        }).collect()
                    }
                    _ => Vec::new(),
                };

                let mut arg_idx = 0;
                for (name, has_default, default_val, field_init) in &field_entries {
                    let value = if !field_init {
                        // init=False: use default if available, else skip (post_init sets it)
                        if *has_default {
                            if default_val.is_callable() {
                                self.call_object(default_val.clone(), vec![])?
                            } else {
                                default_val.clone()
                            }
                        } else {
                            continue; // Will be set by __post_init__
                        }
                    } else if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == name.as_str()) {
                        v.clone()
                    } else if arg_idx < pos_args.len() {
                        let v = pos_args[arg_idx].clone();
                        arg_idx += 1;
                        v
                    } else if *has_default {
                        if default_val.is_callable() {
                            self.call_object(default_val.clone(), vec![])?
                        } else {
                            default_val.clone()
                        }
                    } else {
                        return Err(PyException::type_error(format!(
                            "__init__() missing required argument: '{}'", name
                        )));
                    };

                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        inst.attrs.write().insert(CompactString::from(name.as_str()), value);
                    }
                }
            }
            // Call __post_init__ if defined
            if let Some(post_init) = cls.get_attr("__post_init__") {
                let pi_fn = match &post_init.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => post_init.clone(),
                };
                self.call_object(pi_fn, vec![instance.clone()])?;
            }
            // For frozen dataclasses, install __setattr__/__delattr__ that raise
            if is_frozen {
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    let ns = cd.namespace.read();
                    if !ns.contains_key("__setattr__") {
                        drop(ns);
                        let mut ns = cd.namespace.write();
                        ns.insert(intern_or_new("__setattr__"), PyObject::native_function("__setattr__", |_args| {
                            Err(PyException::attribute_error(String::from("cannot assign to field of frozen dataclass")))
                        }));
                        ns.insert(intern_or_new("__delattr__"), PyObject::native_function("__delattr__", |_args| {
                            Err(PyException::attribute_error(String::from("cannot delete field of frozen dataclass")))
                        }));
                    }
                }
            }
        } else if class_has_key(cls, "__namedtuple__") {
            // Namedtuple: populate named fields
            if let Some(fields) = cls.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        // Get defaults dict if available
                        let defaults_map = cls.get_attr("_field_defaults").and_then(|d| {
                            if let PyObjectPayload::Dict(map) = &d.payload {
                                Some(map.read().clone())
                            } else { None }
                        });
                        let mut attrs = inst.attrs.write();
                        let mut tuple_values = Vec::with_capacity(field_names.len());
                        for (i, field) in field_names.iter().enumerate() {
                            let name = field.py_to_string();
                            let value = if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == name.as_str()) {
                                v.clone()
                            } else if i < pos_args.len() {
                                pos_args[i].clone()
                            } else if let Some(ref dmap) = defaults_map {
                                let key = HashableKey::str_key(CompactString::from(name.as_str()));
                                dmap.get(&key).cloned().unwrap_or_else(PyObject::none)
                            } else {
                                PyObject::none()
                            };
                            attrs.insert(CompactString::from(name.as_str()), value.clone());
                            tuple_values.push(value);
                        }
                        attrs.insert(CompactString::from("_tuple"), PyObject::tuple(tuple_values));
                    }
                }
            }
        } else if let Some(init) = cls.get_attr("__init__") {
            // Skip builtin __init__ — instance already created, no user code to run
            let is_builtin_init = matches!(&init.payload,
                PyObjectPayload::BuiltinBoundMethod(bbm)
                    if matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(_)));
            if !is_builtin_init {
                let init_fn = match &init.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => init.clone(),
                };
                let mut init_args = vec![instance.clone()];
                init_args.extend(pos_args.clone());
                let init_result = if kwargs.is_empty() {
                    self.call_object(init_fn, init_args)?
                } else {
                    self.call_object_kw(init_fn, init_args, kwargs.clone())?
                };
                // CPython: __init__ must return None
                if !matches!(&init_result.payload, PyObjectPayload::None) {
                    return Err(PyException::type_error(
                        "__init__() should return None, not '".to_string()
                            + init_result.type_name() + "'"
                    ));
                }
            }
            // Dict subclass: populate dict_storage from pos_args/kwargs
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                if let Some(ref ds) = inst.dict_storage {
                    let mut storage = ds.write();
                    // If first positional arg is a dict, copy its entries
                    if !pos_args.is_empty() {
                        if let PyObjectPayload::Dict(src) = &pos_args[0].payload {
                            for (k, v) in src.read().iter() {
                                storage.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    // Populate kwargs into dict_storage
                    for (k, v) in &kwargs {
                        storage.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                }
            }
        }

        // Store kwargs as instance attrs when no user __init__ consumed them.
        // This supports AST nodes, simple data classes, and similar patterns
        // where Class(field=value) stores field as an attribute.
        if !kwargs.is_empty() {
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                let mut attrs = inst.attrs.write();
                for (k, v) in &kwargs {
                    if !attrs.contains_key(k.as_str()) {
                        attrs.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        // Map positional args to _fields for AST-like node classes.
        // When a class defines _fields (tuple of field name strings) and has no
        // user __init__, positional constructor args are stored as named attrs.
        if !pos_args.is_empty() {
            if let Some(fields_obj) = cls.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields_obj.payload {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        let mut attrs = inst.attrs.write();
                        for (i, field) in field_names.iter().enumerate() {
                            if i < pos_args.len() {
                                let fname = field.py_to_string();
                                if !attrs.contains_key(fname.as_str()) {
                                    attrs.insert(CompactString::from(fname.as_str()), pos_args[i].clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        // Exception subclass attrs
        if Self::is_exception_class(cls) {
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                let mut attrs = inst.attrs.write();
                if !attrs.contains_key("args") {
                    attrs.insert(CompactString::from("args"), PyObject::tuple(pos_args));
                }
            }
        }

        Ok(instance)
    }

    /// Build a super() proxy from current call frame or explicit args.
    pub(crate) fn make_super(&self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            let frame = self.call_stack.last().unwrap();
            // First check locals[0] for self; if moved to a cell (e.g. captured by
            // a comprehension), fall back to cellvars to find it (PEP 3135 compat).
            let self_obj = frame.locals.first().cloned().flatten().or_else(|| {
                // If self is in cellvars (common when method body has comprehensions
                // that reference self), look it up from cells
                for (i, cv) in frame.code.cellvars.iter().enumerate() {
                    if cv.as_str() == "self" || cv.as_str() == "cls" {
                        if let Some(cell) = frame.cells.get(i) {
                            if let Some(val) = cell.read().as_ref() {
                                return Some(val.clone());
                            }
                        }
                    }
                }
                None
            });
            if let Some(self_obj) = self_obj {
                let qualname = frame.code.qualname.as_str();
                let defining_class_name = qualname.rsplit_once('.')
                    .map(|(cls_part, _)| {
                        cls_part.rsplit_once('.').map(|(_, c)| c).unwrap_or(cls_part)
                    });

                let (runtime_cls, instance_for_super) = match &self_obj.payload {
                    PyObjectPayload::Instance(inst) => (inst.class.clone(), self_obj.clone()),
                    PyObjectPayload::Class(cd) => {
                        // For metaclass methods: if defining_class_name matches the metaclass,
                        // use the metaclass as runtime_cls (so super walks metaclass MRO)
                        if let Some(meta) = &cd.metaclass {
                            (meta.clone(), self_obj.clone())
                        } else {
                            (self_obj.clone(), self_obj.clone())
                        }
                    }
                    // Unwrap Super proxy — can happen if property getter receives
                    // a super proxy as self (shouldn't normally, but be defensive)
                    PyObjectPayload::Super { instance, .. } => {
                        match &instance.payload {
                            PyObjectPayload::Instance(inst) => (inst.class.clone(), instance.clone()),
                            _ => (instance.clone(), instance.clone()),
                        }
                    }
                    _ => return Err(PyException::runtime_error("super(): no current class")),
                };

                let mut cls = runtime_cls.clone();
                if let Some(def_name) = defining_class_name {
                    if let PyObjectPayload::Class(cd) = &runtime_cls.payload {
                        // Build full MRO including the runtime class itself
                        let mut full_mro = vec![runtime_cls.clone()];
                        full_mro.extend(cd.mro.iter().cloned());

                        // Strategy: find the class whose namespace contains the
                        // currently executing function (by matching Rc<CodeObject>
                        // pointers).  This is robust even when multiple classes
                        // share the same name (e.g. Flask Request vs werkzeug
                        // Request, or same-named EnvironBuilder subclasses).
                        let code_ptr = Rc::as_ptr(&frame.code);
                        let mut found_by_code = false;
                        for m in &full_mro {
                            if let PyObjectPayload::Class(mc) = &m.payload {
                                let ns = mc.namespace.read();
                                // Check method name from qualname (last segment)
                                let method_name = qualname.rsplit_once('.')
                                    .map(|(_, m)| m).unwrap_or(qualname);
                                if let Some(val) = ns.get(method_name) {
                                    let matches = match &val.payload {
                                        PyObjectPayload::Function(f) =>
                                            Rc::as_ptr(&f.code) == code_ptr,
                                        PyObjectPayload::BoundMethod { method, .. } => {
                                            if let PyObjectPayload::Function(f) = &method.payload {
                                                Rc::as_ptr(&f.code) == code_ptr
                                            } else { false }
                                        }
                                        _ => false,
                                    };
                                    if matches {
                                        cls = m.clone();
                                        found_by_code = true;
                                        break;
                                    }
                                }
                            }
                        }

                        // Fallback: match by class name if code-pointer match failed
                        // (can happen with NativeFunction or wrapped methods)
                        if !found_by_code {
                            for m in &full_mro {
                                if let PyObjectPayload::Class(mc) = &m.payload {
                                    if mc.name.as_str() == def_name {
                                        cls = m.clone();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                return Ok(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::Super { cls, instance: instance_for_super }
                }));
            }
            Err(PyException::runtime_error("super(): no current class"))
        } else if args.len() == 2 {
            Ok(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::Super { cls: args[0].clone(), instance: args[1].clone() }
            }))
        } else {
            Err(PyException::type_error("super() takes 0 or 2 arguments"))
        }
    }

    pub(crate) fn call_object_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        match &func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let globals = pyfunc.globals.clone();
                self.call_function_kw(&pyfunc.code, pos_args, kwargs, &pyfunc.defaults, &pyfunc.kw_defaults, globals, &pyfunc.closure, &pyfunc.constant_cache)
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(pos_args);
                self.call_object_kw(method.clone(), bound_args, kwargs)
            }
            PyObjectPayload::Class(cd) => {
                // If the metaclass defines its own __call__ (not just type.__call__),
                // dispatch through it.
                if let Some(meta) = &cd.metaclass {
                    if let Some(call_method) = meta.get_attr("__call__") {
                        let is_inherited_type_call = matches!(
                            &call_method.payload,
                            PyObjectPayload::BuiltinBoundMethod(bbm)
                                if bbm.method_name.as_str() == "__call__"
                                && matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(t) if t.as_str() == "type")
                        );
                        if !is_inherited_type_call {
                            let mut call_args = vec![func.clone()];
                            call_args.extend(pos_args);
                            if kwargs.is_empty() {
                                return self.call_object(call_method, call_args);
                            } else {
                                return self.call_object_kw(call_method, call_args, kwargs);
                            }
                        }
                    }
                }
                self.instantiate_class(&func, pos_args, kwargs)
            }
            _ => {
                // For BuiltinBoundMethod on str.format, pass kwargs as a dict
                if let PyObjectPayload::BuiltinBoundMethod(bbm) = &func.payload {
                    // Handle list.sort(key=..., reverse=...)
                    if bbm.method_name.as_str() == "sort" {
                        if let PyObjectPayload::List(items_arc) = &bbm.receiver.payload {
                            let mut items_vec = items_arc.read().clone();
                            let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone());
                            let reverse = kwargs.iter().find(|(k, _)| k.as_str() == "reverse")
                                .map(|(_, v)| v.is_truthy()).unwrap_or(false);
                            self.sort_with_key(&mut items_vec, key_fn, reverse)?;
                            *items_arc.write() = items_vec;
                            return Ok(PyObject::none());
                        }
                    }
                    // Handle dict.update(key=val, ...)
                    if bbm.method_name.as_str() == "update" && !kwargs.is_empty() {
                        if let PyObjectPayload::Dict(map) = &bbm.receiver.payload {
                            // First process positional arg (another dict or iterable)
                            if !pos_args.is_empty() {
                                if let PyObjectPayload::Dict(other) = &pos_args[0].payload {
                                    let other_items = other.read().clone();
                                    let mut w = map.write();
                                    for (k, v) in other_items {
                                        w.insert(k, v);
                                    }
                                }
                            }
                            // Then add kwargs
                            let mut w = map.write();
                            for (k, v) in &kwargs {
                                w.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                            return Ok(PyObject::none());
                        }
                    }
                    if bbm.method_name.as_str() == "format" && !kwargs.is_empty() {
                        if let PyObjectPayload::Str(s) = &bbm.receiver.payload {
                            // Handle str.format() with named args via VM-aware formatter
                            return self.vm_str_format_kw(s, &pos_args, &kwargs);
                        }
                    }
                }
                // BuiltinBoundMethod kwargs: resolve known kwargs to positional args
                if let PyObjectPayload::BuiltinBoundMethod(bbm) = &func.payload {
                    if !kwargs.is_empty() {
                        match bbm.method_name.as_str() {
                            // str.encode(encoding=, errors=) / bytes.decode(encoding=, errors=)
                            "encode" | "decode" => {
                                let mut resolved = pos_args;
                                if resolved.is_empty() {
                                    // encoding kwarg or default
                                    let enc = kwargs.iter().find(|(k, _)| k.as_str() == "encoding")
                                        .map(|(_, v)| v.clone())
                                        .unwrap_or_else(|| PyObject::str_val(CompactString::from("utf-8")));
                                    resolved.push(enc);
                                }
                                if resolved.len() < 2 {
                                    if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "errors") {
                                        resolved.push(v.clone());
                                    }
                                }
                                return self.call_object(func, resolved);
                            }
                            _ => {
                                // Generic fallback: pass kwargs as trailing dict
                                let mut all_args = pos_args;
                                let mut kw_map = IndexMap::new();
                                for (k, v) in kwargs {
                                    kw_map.insert(HashableKey::str_key(k), v);
                                }
                                all_args.push(PyObject::dict(kw_map));
                                return self.call_object(func, all_args);
                            }
                        }
                    }
                }
                // Fall back to call_object for builtins etc
                // Handle builtins with keyword args
                let builtin_name = match &func.payload {
                    PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => Some(name.clone()),
                    _ => None,
                };
                if let Some(name) = builtin_name {
                    match name.as_str() {
                        "__build_class__" => {
                            return self.build_class_kw(pos_args, kwargs);
                        }
                        "sorted" => {
                            if !pos_args.is_empty() {
                                let items = self.collect_iterable(&pos_args[0])?;
                                let mut items_vec = items;
                                let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone());
                                let reverse = kwargs.iter().find(|(k, _)| k.as_str() == "reverse")
                                    .map(|(_, v)| v.is_truthy()).unwrap_or(false);
                                self.sort_with_key(&mut items_vec, key_fn, reverse)?;
                                return Ok(PyObject::list(items_vec));
                            }
                        }
                        "globals" => {
                            if let Some(frame) = self.call_stack.last() {
                                let globals_arc = frame.globals.clone();
                                return Ok(PyObject::wrap(PyObjectPayload::InstanceDict(globals_arc)));
                            }
                            return Ok(PyObject::dict(new_fx_hashkey_map()));
                        }
                        "locals" => {
                            if let Some(frame) = self.call_stack.last() {
                                let mut map = IndexMap::new();
                                for (i, name) in frame.code.varnames.iter().enumerate() {
                                    if let Some(Some(val)) = frame.locals.get(i) {
                                        map.insert(HashableKey::str_key(name.clone()), val.clone());
                                    }
                                }
                                if frame.code.varnames.is_empty() {
                                    let g = frame.globals.read();
                                    for (k, v) in g.iter() {
                                        map.insert(HashableKey::str_key(k.clone()), v.clone());
                                    }
                                    drop(g);
                                    for (k, v) in frame.local_names_iter() {
                                        map.insert(HashableKey::str_key(k.clone()), v.clone());
                                    }
                                }
                                return Ok(PyObject::dict(map));
                            }
                            return Ok(PyObject::dict(new_fx_hashkey_map()));
                        }
                        "print" => {
                            let sep = kwargs.iter().find(|(k, _)| k.as_str() == "sep")
                                .map(|(_, v)| v.py_to_string()).unwrap_or_else(|| " ".to_string());
                            let end = kwargs.iter().find(|(k, _)| k.as_str() == "end")
                                .map(|(_, v)| v.py_to_string()).unwrap_or_else(|| "\n".to_string());
                            let file_obj = kwargs.iter().find(|(k, _)| k.as_str() == "file").map(|(_, v)| v.clone());
                            let flush = kwargs.iter().find(|(k, _)| k.as_str() == "flush")
                                .map(|(_,v)| v.is_truthy()).unwrap_or(false);
                            let mut parts = Vec::new();
                            for a in &pos_args {
                                parts.push(self.vm_str(a)?);
                            }
                            let output = format!("{}{}", parts.join(&sep), end);
                            if let Some(f) = self.resolve_print_target(file_obj) {
                                self.write_to_file_object(&f, &output)?;
                                if flush {
                                    if let Some(flush_fn) = f.get_attr("flush") {
                                        let _ = self.call_object(flush_fn, vec![]);
                                    }
                                }
                            } else {
                                print!("{}", output);
                                if flush {
                                    use std::io::Write;
                                    let _ = std::io::stdout().flush();
                                }
                            }
                            return Ok(PyObject::none());
                        }
                        "max" | "min" => {
                            let is_max = name.as_str() == "max";
                            let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone());
                            let default = kwargs.iter().find(|(k, _)| k.as_str() == "default").map(|(_, v)| v.clone());
                            let items = if pos_args.len() == 1 {
                                self.collect_iterable(&pos_args[0])?
                            } else {
                                pos_args.clone()
                            };
                            return self.compute_min_max(items, is_max, key_fn, default, name.as_str());
                        }
                        "super" => {
                            return self.make_super(&pos_args);
                        }
                        "dict" => {
                            let mut map = IndexMap::new();
                            // dict(mapping_or_iterable, **kwargs) or dict(**kwargs)
                            if !pos_args.is_empty() {
                                let mut handled = false;
                                // Check for Dict payload
                                if let PyObjectPayload::Dict(src) = &pos_args[0].payload {
                                    for (k, v) in src.read().iter() {
                                        map.insert(k.clone(), v.clone());
                                    }
                                    handled = true;
                                }
                                // Check for MappingProxy payload
                                if !handled {
                                    if let PyObjectPayload::MappingProxy(src) = &pos_args[0].payload {
                                        for (k, v) in src.read().iter() {
                                            map.insert(k.clone(), v.clone());
                                        }
                                        handled = true;
                                    }
                                }
                                // Check for InstanceDict payload
                                if !handled {
                                    if let PyObjectPayload::InstanceDict(src) = &pos_args[0].payload {
                                        let read = src.read();
                                        for (k, v) in read.iter() {
                                            map.insert(HashableKey::str_key(k.clone()), v.clone());
                                        }
                                        handled = true;
                                    }
                                }
                                // Check for Instance with dict_storage (e.g., defaultdict, OrderedDict)
                                if !handled {
                                    if let PyObjectPayload::Instance(inst) = &pos_args[0].payload {
                                        if let Some(ref ds) = inst.dict_storage {
                                            for (k, v) in ds.read().iter() {
                                                map.insert(k.clone(), v.clone());
                                            }
                                            handled = true;
                                        }
                                    }
                                }
                                if !handled {
                                    // dict(iterable_of_pairs, **kwargs)
                                    let items = self.collect_iterable(&pos_args[0])?;
                                    for item in &items {
                                        let pair = item.to_list()?;
                                        if pair.len() == 2 {
                                            let hk = pair[0].to_hashable_key()?;
                                            map.insert(hk, pair[1].clone());
                                        }
                                    }
                                }
                            }
                            for (k, v) in &kwargs {
                                map.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                            return Ok(PyObject::dict(map));
                        }
                        "enumerate" => {
                            let start = kwargs.iter().find(|(k, _)| k.as_str() == "start")
                                .map(|(_, v)| v.clone())
                                .unwrap_or_else(|| PyObject::int(0));
                            let mut all_args = pos_args;
                            all_args.push(start);
                            return self.call_object(func, all_args);
                        }
                        "int" => {
                            // int(x, base=N)
                            let mut all_args = pos_args;
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "base") {
                                while all_args.len() < 1 { all_args.push(PyObject::int(0)); }
                                all_args.push(v.clone());
                            }
                            return self.call_object(func, all_args);
                        }
                        "float" | "str" | "bool" | "bytes" | "bytearray" | "list" | "tuple" | "set" | "frozenset" => {
                            // These builtins don't use kwargs meaningfully — just pass positional
                            return self.call_object(func, pos_args);
                        }
                        "open" => {
                            // open(file, mode='r', buffering=-1, encoding=None, ...)
                            let mut all_args = pos_args;
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "mode") {
                                while all_args.len() < 2 { all_args.push(PyObject::str_val(CompactString::from("r"))); }
                                all_args[1] = v.clone();
                            }
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "encoding") {
                                while all_args.len() < 4 { all_args.push(PyObject::none()); }
                                all_args[3] = v.clone();
                            }
                            return self.call_object(func, all_args);
                        }
                        "type" => {
                            // type(name, bases, dict) — 3-arg form with kwargs
                            if !kwargs.is_empty() && pos_args.len() >= 3 {
                                return self.call_object(func, pos_args);
                            }
                            let mut all_args = pos_args;
                            let mut kw_map = IndexMap::new();
                            for (k, v) in kwargs {
                                kw_map.insert(HashableKey::str_key(k), v);
                            }
                            if !kw_map.is_empty() {
                                all_args.push(PyObject::dict(kw_map));
                            }
                            return self.call_object(func, all_args);
                        }
                        _ => {
                            // Generic BuiltinFunction kwargs: pass as trailing dict
                            if !kwargs.is_empty() {
                                let mut all_args = pos_args;
                                let mut kw_map = IndexMap::new();
                                for (k, v) in kwargs {
                                    kw_map.insert(HashableKey::str_key(k), v);
                                }
                                all_args.push(PyObject::dict(kw_map));
                                return self.call_object(func, all_args);
                            }
                            return self.call_object(func, pos_args);
                        }
                    }
                }
                // Handle other payload types that support kwargs
                match &func.payload {
                    PyObjectPayload::NativeFunction(nf_data) => {
                        // property.__init__(self, fget=None, fset=None, fdel=None, doc=None)
                        if nf_data.name.as_str() == "property.__init__" {
                            if pos_args.is_empty() { return Ok(PyObject::none()); }
                            let fget = kwargs.iter().find(|(k, _)| k.as_str() == "fget").map(|(_, v)| v.clone())
                                .or_else(|| pos_args.get(1).cloned());
                            let fset = kwargs.iter().find(|(k, _)| k.as_str() == "fset").map(|(_, v)| v.clone())
                                .or_else(|| pos_args.get(2).cloned());
                            let fdel = kwargs.iter().find(|(k, _)| k.as_str() == "fdel").map(|(_, v)| v.clone())
                                .or_else(|| pos_args.get(3).cloned());
                            if let PyObjectPayload::Instance(ref inst) = pos_args[0].payload {
                                let mut w = inst.attrs.write();
                                if let Some(f) = &fget { w.insert(CompactString::from("fget"), f.clone()); }
                                if let Some(f) = &fset { w.insert(CompactString::from("fset"), f.clone()); }
                                if let Some(f) = &fdel { w.insert(CompactString::from("fdel"), f.clone()); }
                            }
                            return Ok(PyObject::none());
                        }
                        // OrderedDict(**kwargs) / Counter(**kwargs) / defaultdict(factory, **kwargs) — dict-like init
                        if nf_data.name.as_str() == "collections.OrderedDict" || nf_data.name.as_str() == "collections.Counter" {
                            let mut map = IndexMap::new();
                            if !pos_args.is_empty() {
                                if let PyObjectPayload::Dict(src) = &pos_args[0].payload {
                                    for (k, v) in src.read().iter() { map.insert(k.clone(), v.clone()); }
                                } else {
                                    let items = self.collect_iterable(&pos_args[0])?;
                                    for item in &items {
                                        let pair = item.to_list()?;
                                        if pair.len() == 2 {
                                            let hk = pair[0].to_hashable_key()?;
                                            map.insert(hk, pair[1].clone());
                                        }
                                    }
                                }
                            }
                            for (k, v) in &kwargs {
                                map.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                            if nf_data.name.as_str() == "collections.Counter" {
                                return (nf_data.func)(&[PyObject::dict(map)]);
                            }
                            return Ok(PyObject::dict(map));
                        }
                        if nf_data.name.as_str() == "collections.defaultdict" {
                            // defaultdict(factory, mapping_or_iterable, **kwargs) or defaultdict(factory, **kwargs)
                            let mut all = pos_args.clone();
                            if !kwargs.is_empty() {
                                let mut map = IndexMap::new();
                                // If there's a second positional arg (mapping), merge it first
                                if all.len() >= 2 {
                                    if let PyObjectPayload::Dict(src) = &all[1].payload {
                                        for (k, v) in src.read().iter() { map.insert(k.clone(), v.clone()); }
                                    }
                                }
                                for (k, v) in &kwargs {
                                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                                }
                                if all.len() >= 2 { all[1] = PyObject::dict(map); } else {
                                    while all.len() < 1 { all.push(PyObject::none()); }
                                    all.push(PyObject::dict(map));
                                }
                            }
                            return (nf_data.func)(&all);
                        }
                        if nf_data.name.as_str() == "collections.deque" {
                            // deque(iterable, maxlen=N)
                            let mut all = pos_args.clone();
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "maxlen") {
                                while all.len() < 1 { all.push(PyObject::list(vec![])); }
                                if all.len() < 2 { all.push(v.clone()); } else { all[1] = v.clone(); }
                            }
                            return (nf_data.func)(&all);
                        }
                        if nf_data.name.as_str() == "functools.partial" {
                            // functools.partial(func, *args, **kwargs)
                            if pos_args.is_empty() {
                                return Err(PyException::type_error("partial() requires at least 1 argument"));
                            }
                            let pf = pos_args[0].clone();
                            let pa = if pos_args.len() > 1 { pos_args[1..].to_vec() } else { vec![] };
                            return Ok(PyObject::wrap(PyObjectPayload::Partial(Box::new(PartialData {
                                func: pf, args: pa, kwargs,
                            }))));
                        }
                        // re.sub / re.subn with callable replacement
                        if (nf_data.name.as_str() == "re.sub" || nf_data.name.as_str() == "re.subn") && pos_args.len() >= 3 {
                            let repl = &pos_args[1];
                            let is_callable = matches!(&repl.payload,
                                PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
                                | PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_)
                                | PyObjectPayload::Partial(_));
                            if is_callable {
                                // Merge kwargs into args as a trailing dict
                                let mut merged = pos_args.clone();
                                if !kwargs.is_empty() {
                                    let mut kw_map = IndexMap::new();
                                    for (k, v) in &kwargs {
                                        kw_map.insert(HashableKey::str_key(k.clone()), v.clone());
                                    }
                                    merged.push(PyObject::dict(kw_map));
                                }
                                return self.re_sub_with_callable(&merged, nf_data.name.as_str() == "re.subn");
                            }
                        }
                        // re.compile(pattern, flags=...) / re.match/search/findall/sub with flags kwarg
                        if nf_data.name.starts_with("re.") {
                            if let Some((_, flags_val)) = kwargs.iter().find(|(k, _)| k.as_str() == "flags") {
                                let mut all = pos_args.clone();
                                // Insert flags as second positional arg
                                if all.len() < 2 {
                                    all.push(flags_val.clone());
                                } else {
                                    all[1] = flags_val.clone();
                                }
                                return (nf_data.func)(&all);
                            }
                        }
                        // itertools.groupby with key function
                        if nf_data.name.as_str() == "itertools.groupby" {
                            let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone())
                                .or_else(|| if pos_args.len() >= 2 { Some(pos_args[1].clone()) } else { None });
                            let iterable = vec![pos_args[0].clone()];
                            return self.vm_itertools_groupby(&iterable, key_fn);
                        }
                        // itertools.accumulate with initial kwarg
                        if nf_data.name.as_str() == "itertools.accumulate" && !kwargs.is_empty() {
                            let initial = kwargs.iter().find(|(k, _)| k.as_str() == "initial").map(|(_, v)| v.clone());
                            let func_arg = if pos_args.len() >= 2 && !matches!(&pos_args[1].payload, PyObjectPayload::None) {
                                Some(pos_args[1].clone())
                            } else {
                                None
                            };
                            let mut all = vec![pos_args[0].clone()];
                            all.push(func_arg.unwrap_or_else(PyObject::none));
                            all.push(initial.unwrap_or_else(PyObject::none));
                            return (nf_data.func)(&all);
                        }
                        // re.split with maxsplit kwarg
                        if nf_data.name.as_str() == "re.split" && !kwargs.is_empty() {
                            let mut all = pos_args.clone();
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "maxsplit") {
                                while all.len() < 3 { all.push(PyObject::int(0)); }
                                all[2] = v.clone();
                            }
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "flags") {
                                while all.len() < 4 { all.push(PyObject::int(0)); }
                                all[3] = v.clone();
                            }
                            return (nf_data.func)(&all);
                        }
                        // re.sub with count kwarg
                        if nf_data.name.as_str() == "re.sub" && !kwargs.is_empty() {
                            let mut all = pos_args.clone();
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "count") {
                                while all.len() < 4 { all.push(PyObject::int(0)); }
                                all[3] = v.clone();
                            }
                            return (nf_data.func)(&all);
                        }
                        // type.__call__(cls, *args, **kwargs) — standard class instantiation
                        if nf_data.name.as_str() == "__type_call__" {
                            if pos_args.is_empty() {
                                return Err(PyException::type_error("type.__call__ requires cls"));
                            }
                            let cls = pos_args[0].clone();
                            let rest = pos_args[1..].to_vec();
                            return self.instantiate_class(&cls, rest, kwargs);
                        }
                        // json.loads with object_hook/parse_float/parse_int Python callables
                        if nf_data.name.as_str() == "json.loads" && !kwargs.is_empty() {
                            let has_py_hook = kwargs.iter().any(|(k, v)| {
                                matches!(k.as_str(), "object_hook" | "parse_float" | "parse_int" | "object_pairs_hook")
                                && matches!(&v.payload, PyObjectPayload::Function(_) | PyObjectPayload::Class(_))
                            });
                            if has_py_hook {
                                // Call native json.loads without hooks to get parsed data
                                let filtered_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs.iter()
                                    .filter(|(k, _)| !matches!(k.as_str(), "object_hook" | "parse_float" | "parse_int" | "object_pairs_hook"))
                                    .cloned()
                                    .collect();
                                let mut load_args = pos_args.clone();
                                if !filtered_kwargs.is_empty() {
                                    let mut kw_map = IndexMap::new();
                                    for (k, v) in filtered_kwargs {
                                        kw_map.insert(HashableKey::str_key(k), v);
                                    }
                                    load_args.push(PyObject::dict(kw_map));
                                }
                                let parsed = (nf_data.func)(&load_args)?;
                                // Apply hooks via VM (can call Python functions)
                                let object_hook = kwargs.iter().find(|(k, _)| k.as_str() == "object_hook").map(|(_, v)| v.clone());
                                let parse_float = kwargs.iter().find(|(k, _)| k.as_str() == "parse_float").map(|(_, v)| v.clone());
                                let parse_int = kwargs.iter().find(|(k, _)| k.as_str() == "parse_int").map(|(_, v)| v.clone());
                                return self.json_apply_hooks(&parsed, &object_hook, &parse_float, &parse_int);
                            }
                        }
                        // json.dumps / json.dump with `default` kwarg that may be a Python function
                        if (nf_data.name.as_str() == "json.dumps" || nf_data.name.as_str() == "json.dump") && !kwargs.is_empty() {
                            let default_fn = kwargs.iter()
                                .find(|(k, _)| k.as_str() == "default")
                                .map(|(_, v)| v.clone());
                            let cls_default = if default_fn.is_none() {
                                kwargs.iter()
                                    .find(|(k, _)| k.as_str() == "cls")
                                    .and_then(|(_, cls_val)| {
                                        // Create an encoder instance and bind its default method
                                        let encoder_inst = PyObject::instance(cls_val.clone());
                                        cls_val.get_attr("default").map(|method| {
                                            PyObject::wrap(PyObjectPayload::BoundMethod {
                                                receiver: encoder_inst,
                                                method,
                                            })
                                        })
                                    })
                            } else { None };
                            let effective_default = default_fn.or(cls_default);
                            if let Some(ref def) = effective_default {
                                let needs_vm_prepare = match &def.payload {
                                    PyObjectPayload::Function(_) => true,
                                    PyObjectPayload::BoundMethod { method, .. } => {
                                        matches!(&method.payload, PyObjectPayload::Function(_))
                                    }
                                    PyObjectPayload::NativeFunction(_)
                                    | PyObjectPayload::NativeClosure(_)
                                    | PyObjectPayload::Class(_)
                                    | PyObjectPayload::BuiltinFunction(_)
                                    | PyObjectPayload::BuiltinType(_) => true,
                                    _ => false,
                                };
                                if needs_vm_prepare {
                                    // Pre-process object tree: call `default` on non-serializable values
                                    let prepared = self.json_prepare_with_default(&pos_args[0], def)?;
                                    // Rebuild kwargs without `default` and `cls`
                                    let filtered_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs.into_iter()
                                        .filter(|(k, _)| k.as_str() != "default" && k.as_str() != "cls")
                                        .collect();
                                    if nf_data.name.as_str() == "json.dump" {
                                        // json.dump(obj, fp, **kwargs) → dump prepared obj to fp
                                        let mut dump_args = vec![prepared];
                                        if pos_args.len() > 1 { dump_args.push(pos_args[1].clone()); }
                                        if !filtered_kwargs.is_empty() {
                                            let mut kw_map = IndexMap::new();
                                            for (k, v) in filtered_kwargs {
                                                kw_map.insert(HashableKey::str_key(k), v);
                                            }
                                            dump_args.push(PyObject::dict(kw_map));
                                        }
                                        return (nf_data.func)(&dump_args);
                                    }
                                    // json.dumps(prepared, **remaining_kwargs)
                                    let mut dump_args = vec![prepared];
                                    if !filtered_kwargs.is_empty() {
                                        let mut kw_map = IndexMap::new();
                                        for (k, v) in filtered_kwargs {
                                            kw_map.insert(HashableKey::str_key(k), v);
                                        }
                                        dump_args.push(PyObject::dict(kw_map));
                                    }
                                    return (nf_data.func)(&dump_args);
                                }
                            }
                        }
                        // Pass kwargs as trailing dict if present
                        if !kwargs.is_empty() {
                            let mut all_args = pos_args;
                            let mut kw_map = IndexMap::new();
                            for (k, v) in kwargs {
                                kw_map.insert(HashableKey::str_key(k), v);
                            }
                            all_args.push(PyObject::dict(kw_map));
                            return (nf_data.func)(&all_args);
                        }
                        return (nf_data.func)(&pos_args);
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        let result = if !kwargs.is_empty() {
                            let mut all_args = pos_args;
                            let mut kw_map = IndexMap::new();
                            for (k, v) in kwargs {
                                kw_map.insert(HashableKey::str_key(k), v);
                            }
                            all_args.push(PyObject::dict(kw_map));
                            (nc.func)(&all_args)?
                        } else {
                            (nc.func)(&pos_args)?
                        };
                        // Check if asyncio.run() was invoked
                        if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
                            return self.maybe_await_result(coro);
                        }
                        return Ok(result);
                    }
                    PyObjectPayload::Partial(pd) => {
                        let partial_func = pd.func.clone();
                        let mut combined_args = pd.args.clone();
                        combined_args.extend(pos_args);
                        let mut combined_kw = pd.kwargs.clone();
                        combined_kw.extend(kwargs);
                        if combined_kw.is_empty() {
                            return self.call_object(partial_func, combined_args);
                        } else {
                            return self.call_object_kw(partial_func, combined_args, combined_kw);
                        }
                    }
                    PyObjectPayload::ExceptionType(kind) => {
                        let msg = if pos_args.is_empty() { String::new() } else { pos_args[0].py_to_string() };
                        let inst = PyObject::exception_instance_with_args(kind.clone(), msg, pos_args.clone());
                        // ExceptionGroup/BaseExceptionGroup: store .message and .exceptions attrs
                        if matches!(kind, ExceptionKind::ExceptionGroup | ExceptionKind::BaseExceptionGroup) {
                            if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
                                {
                                    let mut a = ei.ensure_attrs().write();
                                    if !pos_args.is_empty() {
                                        a.insert(CompactString::from("message"), pos_args[0].clone());
                                    }
                                    if pos_args.len() >= 2 {
                                        let exc_list = match &pos_args[1].payload {
                                            PyObjectPayload::List(_) => pos_args[1].clone(),
                                            PyObjectPayload::Tuple(items) => PyObject::list(items.clone()),
                                            _ => PyObject::list(vec![pos_args[1].clone()]),
                                        };
                                        a.insert(CompactString::from("exceptions"), exc_list);
                                    }
                                }
                                if pos_args.len() >= 2 {
                                    attach_eg_methods(&inst);
                                }
                            }
                        }
                        // OSError and subclasses: OSError(errno, strerror[, filename])
                        if kind.is_subclass_of(&ExceptionKind::OSError) && pos_args.len() >= 2 {
                            if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
                                let mut a = ei.ensure_attrs().write();
                                a.insert(CompactString::from("errno"), pos_args[0].clone());
                                a.insert(CompactString::from("strerror"), pos_args[1].clone());
                                if pos_args.len() >= 3 {
                                    a.insert(CompactString::from("filename"), pos_args[2].clone());
                                } else {
                                    a.insert(CompactString::from("filename"), PyObject::none());
                                }
                            }
                        }
                        // SystemExit: store .code attribute
                        if *kind == ExceptionKind::SystemExit && !pos_args.is_empty() {
                            if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
                                ei.ensure_attrs().write().insert(CompactString::from("code"), pos_args[0].clone());
                            }
                        }
                        return Ok(inst);
                    }
                    PyObjectPayload::Instance(_) => {
                        if func.get_attr("__singledispatch__").is_some() {
                            return self.vm_singledispatch_call_instance(&func, &pos_args);
                        }
                        if let Some(method) = func.get_attr("__call__") {
                            return self.call_object_kw(method, pos_args, kwargs);
                        }
                        return Err(PyException::type_error(format!(
                            "'{}' object is not callable", func.type_name()
                        )));
                    }
                    _ => {}
                }
                // Final fallback: pass kwargs as trailing dict to preserve key names
                if !kwargs.is_empty() {
                    let mut all_args = pos_args;
                    let mut kw_map = IndexMap::new();
                    for (k, v) in kwargs {
                        kw_map.insert(HashableKey::str_key(k), v);
                    }
                    all_args.push(PyObject::dict(kw_map));
                    self.call_object(func, all_args)
                } else {
                    self.call_object(func, pos_args)
                }
            }
        }
    }

    pub(crate) fn call_object(
        &mut self,
        func: PyObjectRef,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match &func.payload {
            PyObjectPayload::Function(pyfunc) => {
                // Borrow fields directly from the Arc-backed func instead of cloning
                // expensive Vec/IndexMap payloads. Only globals needs cloning (moved into frame).
                let globals = pyfunc.globals.clone();
                self.call_function(&pyfunc.code, args, &pyfunc.defaults, &pyfunc.kw_defaults, globals, &pyfunc.closure, &pyfunc.constant_cache)
            }
            PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
                if name.as_str() == "__build_class__" {
                    return self.build_class(args);
                }
                // VM-aware builtins that need to call user-defined methods
                match name.as_str() {
                    "globals" => {
                        // Return an InstanceDict that shares the frame's globals Arc.
                        // This means mutations via globals()['key'] = value propagate directly.
                        if let Some(frame) = self.call_stack.last() {
                            let globals_arc = frame.globals.clone();
                            return Ok(PyObject::wrap(PyObjectPayload::InstanceDict(globals_arc)));
                        }
                        return Ok(PyObject::dict(new_fx_hashkey_map()));
                    }
                    "locals" => {
                        if let Some(frame) = self.call_stack.last() {
                            let mut map = IndexMap::new();
                            // Include function-scope locals (varnames → locals array)
                            for (i, name) in frame.code.varnames.iter().enumerate() {
                                if let Some(Some(val)) = frame.locals.get(i) {
                                    map.insert(HashableKey::str_key(name.clone()), val.clone());
                                }
                            }
                            // If no varnames (module scope), include globals + local_names
                            if frame.code.varnames.is_empty() {
                                let g = frame.globals.read();
                                for (k, v) in g.iter() {
                                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                                }
                                drop(g);
                                for (k, v) in frame.local_names_iter() {
                                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                                }
                            }
                            return Ok(PyObject::dict(map));
                        }
                        return Ok(PyObject::dict(new_fx_hashkey_map()));
                    }
                    "print" => {
                        let mut parts = Vec::new();
                        for a in &args {
                            parts.push(self.vm_str(a)?);
                        }
                        let output = format!("{}\n", parts.join(" "));
                        if let Some(f) = self.resolve_print_target(None) {
                            self.write_to_file_object(&f, &output)?;
                        } else {
                            print!("{}", output);
                        }
                        return Ok(PyObject::none());
                    }
                    "str" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        // str(bytes, encoding[, errors]) — decode bytes
                        if args.len() >= 2 {
                            match &args[0].payload {
                                PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                                    let s = String::from_utf8_lossy(b);
                                    return Ok(PyObject::str_val(CompactString::from(s.as_ref())));
                                }
                                _ => {}
                            }
                        }
                        return self.vm_str(&args[0]).map(|s| PyObject::str_val(CompactString::from(s)));
                    }
                    "repr" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        return self.vm_repr(&args[0]).map(|s| PyObject::str_val(CompactString::from(s)));
                    }
                    "map" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("map() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        if args.len() == 2 {
                            let source = self.resolve_iterable(&args[1])?;
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                Rc::new(PyCell::new(IteratorData::Map { func: func_obj, source }))
                            )));
                        } else {
                            let mut iters: Vec<Vec<PyObjectRef>> = Vec::new();
                            for a in &args[1..] { iters.push(self.collect_iterable(a)?); }
                            let min_len = iters.iter().map(|v| v.len()).min().unwrap_or(0);
                            let mut result = Vec::new();
                            for i in 0..min_len {
                                let call_args: Vec<PyObjectRef> = iters.iter().map(|v| v[i].clone()).collect();
                                result.push(self.call_object(func_obj.clone(), call_args)?);
                            }
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                Rc::new(PyCell::new(IteratorData::List { items: result, index: 0 }))
                            )));
                        }
                    }
                    "filter" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("filter() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        let source = self.resolve_iterable(&args[1])?;
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                            Rc::new(PyCell::new(IteratorData::Filter { func: func_obj, source }))
                        )));
                    }
                    "iter" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(iter_method) = Self::resolve_instance_dunder(&args[0], "__iter__") {
                                    return self.call_object(iter_method, vec![]);
                                }
                                // Builtin base type subclass: delegate to __builtin_value__
                                if let Some(bv) = Self::get_builtin_value(&args[0]) {
                                    let resolved = self.resolve_iterable(&bv)?;
                                    return Ok(resolved);
                                }
                            }
                            // Fall through to builtin dispatch for non-instances
                        }
                    }
                    "next" => {
                        if args.is_empty() {
                            return Err(PyException::type_error("next() requires at least 1 argument"));
                        }
                        // For generators, resume directly so StopIteration return value propagates
                        if let PyObjectPayload::Generator(gen_arc) = &args[0].payload {
                            match self.resume_generator(gen_arc, PyObject::none()) {
                                Ok(value) => return Ok(value),
                                Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                                    return Ok(args[1].clone());
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        // Use vm_iter_next which handles instances and lazy iterators
                        match self.vm_iter_next(&args[0]) {
                            Ok(Some(value)) => return Ok(value),
                            Ok(None) => {
                                if args.len() > 1 {
                                    return Ok(args[1].clone()); // default value
                                }
                                return Err(PyException::new(ExceptionKind::StopIteration, ""));
                            }
                            Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                                return Ok(args[1].clone());
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    "list" => {
                        if args.is_empty() {
                            return Ok(PyObject::list(vec![]));
                        }
                        let items = self.collect_iterable(&args[0])?;
                        return Ok(PyObject::list(items));
                    }
                    "tuple" => {
                        if args.is_empty() {
                            return Ok(PyObject::tuple(vec![]));
                        }
                        let items = self.collect_iterable(&args[0])?;
                        return Ok(PyObject::tuple(items));
                    }
                    "sum" => {
                        if args.is_empty() {
                            return Err(PyException::type_error("sum() requires at least 1 argument"));
                        }
                        let items = self.collect_iterable(&args[0])?;
                        let start = if args.len() > 1 { args[1].clone() } else { PyObject::int(0) };
                        let mut total = start;
                        for item in items {
                            // Use VM-level add to support __add__/__radd__
                            if let Some(r) = self.try_binary_dunder(&total, &item, "__add__", Some("__radd__"))? {
                                total = r;
                            } else {
                                total = total.add(&item)?;
                            }
                        }
                        return Ok(total);
                    }
                    "sorted" => {
                        if !args.is_empty() {
                            let mut items = self.collect_iterable(&args[0])?;
                            self.vm_sort(&mut items)?;
                            return Ok(PyObject::list(items));
                        }
                    }
                    "set" => {
                        if args.is_empty() {
                            return builtins::dispatch("set", &[]);
                        }
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::dispatch("set", &[PyObject::list(items)]);
                    }
                    "frozenset" => {
                        if args.is_empty() {
                            return builtins::dispatch("frozenset", &[]);
                        }
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::dispatch("frozenset", &[PyObject::list(items)]);
                    }
                    "dict" => {
                        if args.is_empty() {
                            return Ok(PyObject::dict(new_fx_hashkey_map()));
                        }
                        // dict(mapping) — handle Dict payload
                        if let PyObjectPayload::Dict(_) = &args[0].payload {
                            return builtins::dispatch("dict", &args);
                        }
                        // dict(MappingProxy) — e.g., cls.__dict__
                        if let PyObjectPayload::MappingProxy(src) = &args[0].payload {
                            return Ok(PyObject::dict(src.read().clone()));
                        }
                        // dict(InstanceDict) — e.g., obj.__dict__
                        if let PyObjectPayload::InstanceDict(src) = &args[0].payload {
                            let read = src.read();
                            let mut map = IndexMap::new();
                            for (k, v) in read.iter() {
                                map.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                            return Ok(PyObject::dict(map));
                        }
                        // dict(instance_with_dict_storage) — e.g., defaultdict, OrderedDict
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(ref ds) = inst.dict_storage {
                                let mut map = IndexMap::new();
                                for (k, v) in ds.read().iter() {
                                    map.insert(k.clone(), v.clone());
                                }
                                return Ok(PyObject::dict(map));
                            }
                        }
                        // dict(iterable_of_pairs)
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::dispatch("dict", &[PyObject::list(items)]);
                    }
                    "any" => {
                        if !args.is_empty() {
                            let iter_obj = builtins::get_iter_from_obj_pub(&args[0])?;
                            loop {
                                match self.vm_iter_next(&iter_obj)? {
                                    Some(item) => if item.is_truthy() { return Ok(PyObject::bool_val(true)); },
                                    None => return Ok(PyObject::bool_val(false)),
                                }
                            }
                        }
                    }
                    "all" => {
                        if !args.is_empty() {
                            let iter_obj = builtins::get_iter_from_obj_pub(&args[0])?;
                            loop {
                                match self.vm_iter_next(&iter_obj)? {
                                    Some(item) => if !item.is_truthy() { return Ok(PyObject::bool_val(false)); },
                                    None => return Ok(PyObject::bool_val(true)),
                                }
                            }
                        }
                    }
                    "isinstance" => {
                        if args.len() == 2 {
                            let cls = &args[1];
                            // Check for metaclass __instancecheck__ on user-defined classes
                            if let PyObjectPayload::Class(cd) = &cls.payload {
                                if let Some(ref metaclass) = cd.metaclass {
                                    if let Some(ic) = metaclass.get_attr("__instancecheck__") {
                                        let result = self.call_object(ic, vec![cls.clone(), args[0].clone()])?;
                                        return Ok(PyObject::bool_val(result.is_truthy()));
                                    }
                                }
                                // Check __subclasshook__ on the class (ABC protocol)
                                if let Some(hook) = cls.get_attr("__subclasshook__") {
                                    // Pass the type of the object being checked
                                    let obj = &args[0];
                                    let obj_type = match &obj.payload {
                                        PyObjectPayload::Instance(inst) => inst.class.clone(),
                                        _ => PyObject::builtin_type(CompactString::from(obj.type_name())),
                                    };
                                    if let Ok(result) = self.call_object(hook, vec![obj_type]) {
                                        if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                            return Ok(PyObject::bool_val(result.is_truthy()));
                                        }
                                    }
                                }
                                // Check for runtime_checkable Protocol — structural subtyping
                                let ns = cd.namespace.read();
                                if ns.get("_is_runtime_checkable").map_or(false, |v| v.is_truthy()) {
                                    if let Some(protocol_attrs) = ns.get("__protocol_attrs__") {
                                        if let PyObjectPayload::Tuple(required) = &protocol_attrs.payload {
                                            let obj = &args[0];
                                            let has_all = required.iter().all(|attr_name| {
                                                let name = attr_name.py_to_string();
                                                obj.get_attr(&name).is_some()
                                            });
                                            return Ok(PyObject::bool_val(has_all));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "issubclass" => {
                        if args.len() == 2 {
                            let sup = &args[1];
                            if let PyObjectPayload::Class(cd) = &sup.payload {
                                // Check metaclass __subclasscheck__ first
                                if let Some(ref metaclass) = cd.metaclass {
                                    if let Some(sc) = metaclass.get_attr("__subclasscheck__") {
                                        let result = self.call_object(sc, vec![sup.clone(), args[0].clone()])?;
                                        return Ok(PyObject::bool_val(result.is_truthy()));
                                    }
                                }
                                // Check __subclasshook__ on the superclass (ABC protocol)
                                if let Some(hook) = sup.get_attr("__subclasshook__") {
                                    if let Ok(result) = self.call_object(hook, vec![args[0].clone()]) {
                                        if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                            return Ok(PyObject::bool_val(result.is_truthy()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "min" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            return self.compute_min_max(items, false, None, None, "min");
                        }
                    }
                    "max" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            return self.compute_min_max(items, true, None, None, "max");
                        }
                    }
                    "reversed" => {
                        if !args.is_empty() {
                            // Check for __reversed__ dunder on instances
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(rev_method) = Self::resolve_instance_dunder(&args[0], "__reversed__") {
                                    return self.call_object(rev_method, vec![]);
                                }
                            }
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("reversed", &[PyObject::list(items)]);
                        }
                    }
                    "enumerate" => {
                        if !args.is_empty() {
                            let mut resolved = Vec::with_capacity(args.len());
                            resolved.push(self.resolve_iterable(&args[0])?);
                            resolved.extend_from_slice(&args[1..]);
                            return builtins::dispatch("enumerate", &resolved);
                        }
                        return builtins::dispatch("enumerate", &args);
                    }
                    "zip" => {
                        // Check for trailing kwargs dict (e.g. strict=True)
                        let mut strict = false;
                        let iter_end = if let Some(last) = args.last() {
                            if let PyObjectPayload::Dict(kw) = &last.payload {
                                let r = kw.read();
                                if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("strict"))) {
                                    strict = v.is_truthy();
                                }
                                drop(r);
                                args.len() - 1
                            } else { args.len() }
                        } else { args.len() };
                        let resolved = self.resolve_iterables(&args[..iter_end])?;
                        let mut full_args = resolved;
                        if strict {
                            // Re-add kwargs dict so builtin_zip can pick it up
                            let kw = PyObject::dict(indexmap::IndexMap::from([(
                                HashableKey::str_key(CompactString::from("strict")),
                                PyObject::bool_val(true),
                            )]));
                            full_args.push(kw);
                        }
                        return builtins::dispatch("zip", &full_args);
                    }
                    "len" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Dict subclass: use dict_storage length
                                if let Some(ref ds) = inst.dict_storage {
                                    return Ok(PyObject::int(ds.read().len() as i64));
                                }
                                // Namedtuple: delegate to call_namedtuple_method
                                if inst.class.get_attr("__namedtuple__").is_some() {
                                    return builtins::call_method(&args[0], "__len__", &[]);
                                }
                                // Check for custom __len__ (skip BuiltinBoundMethod from BuiltinType base)
                                if let Some(method) = args[0].get_attr("__len__") {
                                    if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                                        let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                                        return self.call_object(method, ca);
                                    }
                                }
                                // Builtin base type subclass (list, tuple, etc.)
                                if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                                    if let Ok(n) = bv.py_len() {
                                        return Ok(PyObject::int(n as i64));
                                    }
                                }
                            }
                        }
                    }
                    "abs" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__abs__") {
                                    let call_args = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) { vec![] } else { vec![args[0].clone()] };
                                    return self.call_object(method, call_args);
                                }
                            }
                        }
                    }
                    "hash" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__hash__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "bin" | "oct" | "hex" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__index__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                                    let idx_val = self.call_object(method, ca)?;
                                    // Re-call bin/oct/hex with the resolved int
                                    return self.call_object(func.clone(), vec![idx_val]);
                                }
                            }
                        }
                    }
                    "format" => {
                        if !args.is_empty() {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__format__") {
                                    let spec = if args.len() > 1 {
                                        args[1].clone()
                                    } else {
                                        PyObject::str_val(CompactString::from(""))
                                    };
                                    let mut ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; ca.push(spec); return self.call_object(method, ca);
                                }
                                // No __format__: use __str__ for empty/no spec (CPython default __format__)
                                let has_spec = args.len() > 1 && !args[1].py_to_string().is_empty();
                                if !has_spec {
                                    let s = self.vm_str(&args[0])?;
                                    return Ok(PyObject::str_val(CompactString::from(s)));
                                }
                            }
                            // Fall through to native format
                        }
                    }
                    "int" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Check for __builtin_value__ first (int subclass)
                                if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                                    if matches!(&val.payload, PyObjectPayload::Int(_)) {
                                        return Ok(val);
                                    }
                                }
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__int__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "float" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Check for __builtin_value__ first (float subclass)
                                if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                                    if matches!(&val.payload, PyObjectPayload::Float(_)) {
                                        return Ok(val);
                                    }
                                    // int subclass → convert to float
                                    if let PyObjectPayload::Int(n) = &val.payload {
                                        return Ok(PyObject::float(n.to_f64()));
                                    }
                                }
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "round" => {
                        if !args.is_empty() {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__round__") {
                                    let mut ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                                    if args.len() >= 2 { ca.push(args[1].clone()); }
                                    return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "bytes" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__bytes__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "bool" => {
                        if args.len() == 1 {
                            return Ok(PyObject::bool_val(self.vm_is_truthy(&args[0])?));
                        }
                    }
                    "mappingproxy" => {
                        // types.MappingProxyType(dict) — read-only view of a dict
                        if args.len() == 1 {
                            let src = &args[0];
                            let map = match &src.payload {
                                PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => {
                                    m.read().clone()
                                }
                                PyObjectPayload::InstanceDict(attrs) => {
                                    let rd = attrs.read();
                                    let mut m = new_fx_hashkey_map();
                                    for (k, v) in rd.iter() {
                                        m.insert(HashableKey::str_key(k.clone()), v.clone());
                                    }
                                    m
                                }
                                _ => {
                                    return Err(PyException::type_error(
                                        "mappingproxy() argument must be a mapping, not a non-mapping type"
                                    ));
                                }
                            };
                            return Ok(PyObject::wrap(PyObjectPayload::MappingProxy(
                                Rc::new(PyCell::new(map)),
                            )));
                        }
                        if args.is_empty() {
                            return Err(PyException::type_error(
                                "mappingproxy() missing required argument: 'mapping'"
                            ));
                        }
                    }
                    "dir" => {
                        if args.is_empty() {
                            // dir() with no args: return sorted local variable names
                            let locals = self.collect_locals_dict()?;
                            if let PyObjectPayload::Dict(map) = &locals.payload {
                                let mut names: Vec<String> = map.read().keys()
                                    .map(|k| k.to_object().py_to_string())
                                    .collect();
                                names.sort();
                                let items = names.into_iter()
                                    .map(|n| PyObject::str_val(CompactString::from(n)))
                                    .collect();
                                return Ok(PyObject::list(items));
                            }
                        }
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__dir__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                                    return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "super" => {
                        return self.make_super(&args);
                    }
                    "exec" => {
                        return self.builtin_exec(&args);
                    }
                    "eval" => {
                        return self.builtin_eval(&args);
                    }
                    "compile" => {
                        return self.builtin_compile(&args);
                    }
                    "__import__" => {
                        if args.is_empty() {
                            return Err(PyException::type_error("__import__() requires at least 1 argument"));
                        }
                        let name = args[0].py_to_string();
                        let level = if args.len() >= 5 {
                            args[4].as_int().unwrap_or(0) as usize
                        } else {
                            0
                        };
                        return self.import_module_simple(&name, level);
                    }
                    "vars" => {
                        if args.is_empty() {
                            return self.collect_locals_dict();
                        }
                        // vars(obj) — fall through to static builtin_vars
                    }
                    "getattr" => {
                        if args.len() < 2 || args.len() > 3 {
                            return Err(PyException::type_error("getattr expected 2 or 3 arguments"));
                        }
                        let attr_name = args[1].as_str().ok_or_else(||
                            PyException::type_error("getattr(): attribute name must be string"))?;
                        // Use get_attr which handles MRO + data descriptors
                        match args[0].get_attr(attr_name) {
                            Some(v) => {
                                // Invoke descriptor protocol (Property, custom __get__)
                                if let PyObjectPayload::Property(pd) = &v.payload {
                                    if let Some(getter) = pd.fget.as_ref() {
                                        let getter = crate::builtins::unwrap_abstract_fget(getter);
                                        return self.call_object(getter, vec![args[0].clone()]);
                                    }
                                    return Err(PyException::attribute_error(
                                        format!("unreadable attribute '{}'", attr_name)));
                                }
                                if has_descriptor_get(&v) {
                                    if let Some(get_method) = v.get_attr("__get__") {
                                        let (inst_arg, owner_arg) = match &args[0].payload {
                                            PyObjectPayload::Instance(inst) =>
                                                (args[0].clone(), inst.class.clone()),
                                            PyObjectPayload::Class(_) =>
                                                (PyObject::none(), args[0].clone()),
                                            _ => (args[0].clone(), PyObject::none()),
                                        };
                                        // get_method is already a BoundMethod if from class MRO
                                        return self.call_object(get_method, vec![inst_arg, owner_arg]);
                                    }
                                }
                                return Ok(v);
                            }
                            None => {
                                // Try __getattr__ fallback
                                if let PyObjectPayload::Instance(_) = &args[0].payload {
                                    if let Some(ga) = args[0].get_attr("__getattr__") {
                                        let name_arg = PyObject::str_val(CompactString::from(attr_name));
                                        return self.call_object(ga, vec![name_arg]);
                                    }
                                }
                                if args.len() > 2 {
                                    return Ok(args[2].clone());
                                }
                                return Err(PyException::attribute_error(format!(
                                    "'{}' object has no attribute '{}'",
                                    args[0].type_name(), attr_name)));
                            }
                        }
                    }
                    "setattr" => {
                        if args.len() != 3 {
                            return Err(PyException::type_error("setattr() takes exactly 3 arguments"));
                        }
                        let attr_name = args[1].py_to_string();
                        let value = args[2].clone();
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                                if let PyObjectPayload::Property(pd) = &desc.payload {
                                    if let Some(setter) = pd.fset.as_ref() {
                                        self.call_object(setter.clone(), vec![args[0].clone(), value])?;
                                        return Ok(PyObject::none());
                                    } else {
                                        return Err(PyException::attribute_error(format!(
                                            "can't set attribute '{}'", attr_name
                                        )));
                                    }
                                }
                                if is_data_descriptor(&desc) {
                                    if let Some(set_method) = desc.get_attr("__set__") {
                                        // set_method is already bound to desc
                                        self.call_object(set_method, vec![args[0].clone(), value])?;
                                        return Ok(PyObject::none());
                                    }
                                }
                            }
                            if let Some(sa) = lookup_in_class_mro(&inst.class, "__setattr__") {
                                if matches!(&sa.payload, PyObjectPayload::Function(_)) {
                                    let method = PyObjectRef::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: args[0].clone(),
                                            method: sa,
                                        }
                                    });
                                    self.call_object(method, vec![PyObject::str_val(CompactString::from(&attr_name)), value])?;
                                    return Ok(PyObject::none());
                                }
                            }
                        }
                        return builtins::dispatch("setattr", &args);
                    }
                    "delattr" => {
                        if args.len() != 2 {
                            return Err(PyException::type_error("delattr() takes exactly 2 arguments"));
                        }
                        let attr_name = args[1].py_to_string();
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                                if let PyObjectPayload::Property(pd) = &desc.payload {
                                    if let Some(deleter) = pd.fdel.as_ref() {
                                        self.call_object(deleter.clone(), vec![args[0].clone()])?;
                                        return Ok(PyObject::none());
                                    }
                                }
                            }
                        }
                        return builtins::dispatch("delattr", &args);
                    }
                    "NamedTuple" => {
                        // typing.NamedTuple('Point', [('x', int), ('y', int)]) or NamedTuple('Point', x=int, y=int)
                        if !args.is_empty() {
                            let typename = args[0].py_to_string();
                            let mut field_names: Vec<CompactString> = Vec::new();

                            // Check for kwargs dict as last arg
                            let kwargs_dict: Option<FxHashKeyMap> = if args.len() >= 2 {
                                if let PyObjectPayload::Dict(d) = &args[args.len() - 1].payload {
                                    Some(d.read().clone())
                                } else { None }
                            } else { None };

                            let has_kwargs = kwargs_dict.is_some();
                            let positional_end = if has_kwargs { args.len() - 1 } else { args.len() };

                            if positional_end >= 2 {
                                match &args[1].payload {
                                    PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
                                        if let Ok(items) = args[1].to_list() {
                                            for item in &items {
                                                if let PyObjectPayload::Tuple(pair) = &item.payload {
                                                    if !pair.is_empty() {
                                                        field_names.push(CompactString::from(pair[0].py_to_string()));
                                                    }
                                                } else {
                                                    field_names.push(CompactString::from(item.py_to_string()));
                                                }
                                            }
                                        }
                                    }
                                    PyObjectPayload::Str(s) => {
                                        for n in s.replace(',', " ").split_whitespace() {
                                            field_names.push(CompactString::from(n));
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            // kwargs form: NamedTuple('Point', x=int, y=int)
                            if let Some(ref kw) = kwargs_dict {
                                for (k, _v) in kw {
                                    if let HashableKey::Str(fname) = k {
                                        if fname.as_str() != "defaults" && fname.as_str() != "module" && fname.as_str() != "rename" {
                                            if !field_names.contains(&fname.as_ref().clone()) {
                                                field_names.push(fname.as_ref().clone());
                                            }
                                        }
                                    }
                                }
                            }

                            // Build namedtuple class with __namedtuple__ marker and _fields
                            let fields_tuple = PyObject::tuple(
                                field_names.iter().map(|n| PyObject::str_val(n.clone())).collect()
                            );
                            let mut ns = IndexMap::new();
                            ns.insert(CompactString::from("__namedtuple__"), PyObject::bool_val(true));
                            ns.insert(CompactString::from("_fields"), fields_tuple);
                            ns.insert(CompactString::from("_field_defaults"), PyObject::dict(new_fx_hashkey_map()));
                            return Ok(PyObject::class(CompactString::from(typename), vec![], ns));
                        }
                    }
                    _ => {}
                }
                match builtins::get_builtin_fn(name.as_str()) {
                    Some(f) => {
                        let result = f(&args);
                        // Check if breakpoint() was called
                        if crate::builtins::core_fns::BREAKPOINT_TRIGGERED
                            .swap(false, std::sync::atomic::Ordering::Relaxed)
                        {
                            self.breakpoints.builtin_breakpoint_pending = true;
                            self.handle_breakpoint_hit();
                        }
                        result
                    }
                    None => Err(PyException::type_error(format!(
                        "'{}' is not callable", name
                    ))),
                }
            }
            PyObjectPayload::Class(cd) => {
                // If the metaclass defines its own __call__ (not just type.__call__),
                // dispatch through it.
                if let Some(meta) = &cd.metaclass {
                    if let Some(call_method) = meta.get_attr("__call__") {
                        // Skip if this is just the inherited type.__call__ builtin
                        let is_inherited_type_call = matches!(
                            &call_method.payload,
                            PyObjectPayload::BuiltinBoundMethod(bbm)
                                if bbm.method_name.as_str() == "__call__"
                                && matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(t) if t.as_str() == "type")
                        );
                        if !is_inherited_type_call {
                            let mut call_args = vec![func.clone()];
                            call_args.extend(args);
                            return self.call_object(call_method, call_args);
                        }
                    }
                }
                self.instantiate_class(&func, args, vec![])
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                // VM intercept: RawIOBase.read(size=-1) → calls self.readinto()
                if let PyObjectPayload::NativeFunction(nf) = &method.payload {
                    if nf.name.as_str() == "RawIOBase.read" {
                        let size: i64 = args.first().and_then(|a| a.as_int()).unwrap_or(-1);
                        return self.rawiobase_read(receiver, size);
                    }
                    if nf.name.as_str() == "RawIOBase.readall" {
                        return self.rawiobase_readall(receiver);
                    }
                }
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(args);
                self.call_object(method.clone(), bound_args)
            }
            PyObjectPayload::BuiltinBoundMethod(bbm) => {
                // ── Generator / Coroutine / AsyncGenerator dispatch ──
                // Extract gen_arc and discriminate the bbm.receiver kind for proper protocol.
                let gen_kind = match &bbm.receiver.payload {
                    PyObjectPayload::Generator(g) => Some(("generator", g.clone())),
                    PyObjectPayload::Coroutine(g) => Some(("coroutine", g.clone())),
                    PyObjectPayload::AsyncGenerator(g) => Some(("async_generator", g.clone())),
                    _ => None,
                };
                if let Some((kind, ref gen_arc)) = gen_kind {
                    match bbm.method_name.as_str() {
                        "send" => {
                            let val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return self.resume_generator(gen_arc, val);
                        }
                        "throw" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            // Preserve original exception value for identity
                            let original_value = if args.len() >= 2 {
                                let v = &args[1];
                                if matches!(v.payload, PyObjectPayload::ExceptionInstance(_)
                                    | PyObjectPayload::Instance(_)) {
                                    Some(v.clone())
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            return self.gen_throw_with_value(gen_arc, exc_kind, msg, original_value);
                        }
                        "close" => {
                            // CPython: throw GeneratorExit into the frame so finally blocks run.
                            // If generator yields during cleanup → RuntimeError.
                            let gen = gen_arc.read();
                            if gen.finished || gen.frame.is_none() {
                                // Already finished — nothing to clean up
                                drop(gen);
                                return Ok(PyObject::none());
                            }
                            drop(gen);
                            match self.gen_throw(gen_arc, ExceptionKind::GeneratorExit, CompactString::new("")) {
                                Ok(_yielded) => {
                                    // Generator yielded during close → RuntimeError
                                    return Err(PyException::runtime_error(
                                        "generator ignored GeneratorExit"
                                    ));
                                }
                                Err(e) if e.kind == ExceptionKind::GeneratorExit
                                       || e.kind == ExceptionKind::StopIteration => {
                                    // Expected: GeneratorExit propagated out or StopIteration
                                    let mut gen = gen_arc.write();
                                    gen.finished = true;
                                    gen.frame = None;
                                    return Ok(PyObject::none());
                                }
                                Err(e) => {
                                    // Other exception from finally block — propagate
                                    let mut gen = gen_arc.write();
                                    gen.finished = true;
                                    gen.frame = None;
                                    return Err(e);
                                }
                            }
                        }
                        "__next__" if kind != "async_generator" => {
                            return self.resume_generator(gen_arc, PyObject::none());
                        }
                        // Context manager protocol for generators (@contextmanager)
                        "__enter__" if kind == "generator" => {
                            // __enter__ = next(gen) — advance to first yield
                            return self.resume_generator(gen_arc, PyObject::none());
                        }
                        "__exit__" if kind == "generator" => {
                            // args: exc_type, exc_val, exc_tb
                            let has_exc = !args.is_empty()
                                && !matches!(&args[0].payload, PyObjectPayload::None);
                            if has_exc {
                                // Exception in with block — throw into generator
                                let (exc_kind, msg) = Self::parse_throw_args(&args);
                                match self.gen_throw(gen_arc, exc_kind, msg) {
                                    Ok(_) => {
                                        // Generator caught the exception and yielded or returned
                                        return Ok(PyObject::bool_val(true)); // suppress exception
                                    }
                                    Err(e) if e.kind == ExceptionKind::StopIteration => {
                                        return Ok(PyObject::bool_val(true));
                                    }
                                    Err(e) => return Err(e),
                                }
                            } else {
                                // Normal exit — advance generator past yield
                                match self.resume_generator(gen_arc, PyObject::none()) {
                                    Ok(_) => {
                                        // Generator yielded again — it should have stopped
                                        return Err(PyException::runtime_error(
                                            "generator didn't stop"
                                        ));
                                    }
                                    Err(e) if e.kind == ExceptionKind::StopIteration => {
                                        return Ok(PyObject::bool_val(false));
                                    }
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                        // ── Async generator protocol methods ──
                        // __aiter__ returns self (async generator is its own async iterator)
                        "__aiter__" if kind == "async_generator" => {
                            return Ok(bbm.receiver.clone());
                        }
                        // These return AsyncGenAwaitable objects, not direct results.
                        "__anext__" if kind == "async_generator" => {
                            return Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: Box::new(AsyncGenAction::Next),
                                }
                            }));
                        }
                        "asend" if kind == "async_generator" => {
                            let val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: Box::new(AsyncGenAction::Send(val)),
                                }
                            }));
                        }
                        "athrow" if kind == "async_generator" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            return Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: Box::new(AsyncGenAction::Throw(exc_kind, CompactString::from(msg))),
                                }
                            }));
                        }
                        "aclose" if kind == "async_generator" => {
                            return Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: Box::new(AsyncGenAction::Close),
                                }
                            }));
                        }
                        _ => {}
                    }
                }

                // ── Iterator protocol dispatch ──
                if let PyObjectPayload::Iterator(_) | PyObjectPayload::RangeIter { .. } = &bbm.receiver.payload {
                    match bbm.method_name.as_str() {
                        "__next__" => {
                            match crate::builtins::iter_advance(&bbm.receiver)? {
                                Some((_new_iter, value)) => return Ok(value),
                                None => return Err(ferrython_core::error::PyException::stop_iteration()),
                            }
                        }
                        "__iter__" => {
                            return Ok(bbm.receiver.clone());
                        }
                        _ => {}
                    }
                }

                // ── AsyncGenAwaitable dispatch (driving the awaitable) ──
                if let PyObjectPayload::AsyncGenAwaitable { gen, action } = &bbm.receiver.payload {
                    match bbm.method_name.as_str() {
                        "send" => {
                            let send_val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return self.drive_async_gen_awaitable(gen, action, send_val);
                        }
                        "throw" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            let original_value = if args.len() >= 2 {
                                let v = &args[1];
                                if matches!(v.payload, PyObjectPayload::ExceptionInstance(_)
                                    | PyObjectPayload::Instance(_)) {
                                    Some(v.clone())
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            return self.gen_throw_with_value(gen, exc_kind, msg, original_value);
                        }
                        "close" => {
                            return Ok(PyObject::none());
                        }
                        _ => {}
                    }
                }
                // VM-level methods that need iterable collection
                if bbm.method_name.as_str() == "join" {
                    if let PyObjectPayload::Str(sep) = &bbm.receiver.payload {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            let strs: Result<Vec<String>, _> = items.iter()
                                .map(|x| x.as_str().map(String::from).ok_or_else(||
                                    ferrython_core::error::PyException::type_error("sequence item: expected str")))
                                .collect();
                            return Ok(PyObject::str_val(CompactString::from(strs?.join(sep.as_str()))));
                        }
                    }
                    if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) = &bbm.receiver.payload {
                        if !args.is_empty() {
                            let sep = sep.clone();
                            let items = self.collect_iterable(&args[0])?;
                            let mut result = Vec::new();
                            for (i, item) in items.iter().enumerate() {
                                if i > 0 { result.extend_from_slice(&sep); }
                                match &item.payload {
                                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => result.extend_from_slice(b),
                                    _ => return Err(PyException::type_error("sequence item: expected a bytes-like object")),
                                }
                            }
                            return Ok(PyObject::bytes(result));
                        }
                    }
                }
                // VM-level list.sort with key function
                if bbm.method_name.as_str() == "sort" {
                    if let PyObjectPayload::List(items_arc) = &bbm.receiver.payload {
                        let items_arc = items_arc.clone();
                        let mut items_vec = items_arc.read().clone();
                        self.vm_sort(&mut items_vec)?;
                        *items_arc.write() = items_vec;
                        return Ok(PyObject::none());
                    }
                }
                // Range methods
                if let PyObjectPayload::Range { start, stop, step } = &bbm.receiver.payload {
                    let (rs, re, rst) = (*start, *stop, *step);
                    match bbm.method_name.as_str() {
                        "count" => {
                            if args.is_empty() { return Err(PyException::type_error("count() takes exactly one argument")); }
                            let val = args[0].to_int().unwrap_or(i64::MIN);
                            let found = if rst > 0 { val >= rs && val < re && (val - rs) % rst == 0 }
                                       else if rst < 0 { val <= rs && val > re && (rs - val) % (-rst) == 0 }
                                       else { false };
                            return Ok(PyObject::int(if found { 1 } else { 0 }));
                        }
                        "index" => {
                            if args.is_empty() { return Err(PyException::type_error("index() takes exactly one argument")); }
                            let val = args[0].to_int().unwrap_or(i64::MIN);
                            let in_range = if rst > 0 { val >= rs && val < re && (val - rs) % rst == 0 }
                                          else if rst < 0 { val <= rs && val > re && (rs - val) % (-rst) == 0 }
                                          else { false };
                            if in_range {
                                return Ok(PyObject::int((val - rs) / rst));
                            }
                            return Err(PyException::value_error(format!("{} is not in range", val)));
                        }
                        _ => {}
                    }
                }
                // Class introspection methods
                if let PyObjectPayload::Class(cd) = &bbm.receiver.payload {
                    match bbm.method_name.as_str() {
                        "__subclasses__" => {
                            let subs = cd.subclasses.read();
                            let alive: Vec<PyObjectRef> = subs.iter()
                                .filter_map(|w| w.upgrade())
                                .collect();
                            drop(subs);
                            // Prune dead weak refs periodically
                            cd.subclasses.write().retain(|w| w.strong_count() > 0);
                            return Ok(PyObject::list(alive));
                        }
                        "mro" => {
                            let mut mro_list = vec![bbm.receiver.clone()];
                            mro_list.extend(cd.mro.iter().cloned());
                            return Ok(PyObject::list(mro_list));
                        }
                        _ => {}
                    }
                }
                // Property descriptor methods: setter/getter/deleter
                if let PyObjectPayload::Property(pd) = &bbm.receiver.payload {
                    if args.len() == 1 {
                        let func = args[0].clone();
                        let new_prop = match bbm.method_name.as_str() {
                            "setter" => PyObjectPayload::Property(Box::new(PropertyData { fget: pd.fget.clone(), fset: Some(func), fdel: pd.fdel.clone() })),
                            "getter" => PyObjectPayload::Property(Box::new(PropertyData { fget: Some(func), fset: pd.fset.clone(), fdel: pd.fdel.clone() })),
                            "deleter" => PyObjectPayload::Property(Box::new(PropertyData { fget: pd.fget.clone(), fset: pd.fset.clone(), fdel: Some(func) })),
                            _ => return Err(PyException::attribute_error(format!("property has no attribute '{}'", bbm.method_name))),
                        };
                        return Ok(PyObjectRef::new(PyObject { payload: new_prop }));
                    }
                }
                // namedtuple methods — delegated to builtins
                if let PyObjectPayload::Instance(inst) = &bbm.receiver.payload {
                    if matches!(&inst.class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__namedtuple__"))
                        || inst.attrs.read().contains_key("__deque__")
                    {
                        // deque extend/extendleft need iterable collection via VM
                        if inst.attrs.read().contains_key("__deque__") && matches!(bbm.method_name.as_str(), "extend" | "extendleft") {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), &[PyObject::list(items)]);
                        }
                        return builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), &args);
                    }
                    // Hashlib methods — delegated to builtins
                    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.to_string() } else { String::new() };
                    if matches!(class_name.as_str(), "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512") {
                        return builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), &args);
                    }
                }
                // Unbound method call: str.upper("hello") → call_method("hello", "upper", [])
                if let PyObjectPayload::BuiltinType(tn) = &bbm.receiver.payload {
                    // type.__call__(cls, *args) → instantiate the class
                    if tn.as_str() == "type" && bbm.method_name.as_str() == "__call__" && !args.is_empty() {
                        if matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                            let cls = args[0].clone();
                            let mut rest = args[1..].to_vec();
                            // Unpack trailing kwargs dict (produced by call_object_kw fallback)
                            let kw = {
                                let mut extracted = vec![];
                                let should_pop = if let Some(last) = rest.last() {
                                    if let PyObjectPayload::Dict(map) = &last.payload {
                                        let rd = map.read();
                                        let all_str = rd.keys().all(|k| matches!(k, HashableKey::Str(_)));
                                        if all_str && !rd.is_empty() {
                                            for (k, v) in rd.iter() {
                                                if let HashableKey::Str(s) = k {
                                                    extracted.push((s.as_ref().clone(), v.clone()));
                                                }
                                            }
                                            true
                                        } else { false }
                                    } else { false }
                                } else { false };
                                if should_pop { rest.pop(); }
                                extracted
                            };
                            return self.instantiate_class(&cls, rest, kw);
                        }
                    }
                    // Class methods (e.g., int.from_bytes, dict.fromkeys)
                    if let Some(class_method) = builtins::resolve_type_class_method(tn, bbm.method_name.as_str()) {
                        if let PyObjectPayload::NativeFunction(nf) = &class_method.payload {
                            return (nf.func)(&args);
                        }
                    }
                    if !args.is_empty() {
                        let instance = args[0].clone();
                        let rest_args = if args.len() > 1 { args[1..].to_vec() } else { vec![] };
                        return builtins::call_method(&instance, bbm.method_name.as_str(), &rest_args);
                    }
                }
                // list.extend with generator/lazy iterator/instance needs VM-level collection
                if bbm.method_name.as_str() == "extend" && !args.is_empty() {
                    if matches!(bbm.receiver.payload, PyObjectPayload::List(_)) {
                        if matches!(args[0].payload, PyObjectPayload::Generator(_) | PyObjectPayload::Instance(_)) ||
                           (matches!(&args[0].payload, PyObjectPayload::Iterator(ref d) if {
                               let data = d.read();
                               matches!(&*data, IteratorData::Enumerate { .. } | IteratorData::Zip { .. }
                                   | IteratorData::Map { .. } | IteratorData::Filter { .. }
                                   | IteratorData::Sentinel { .. })
                           }))
                        {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::call_method(&bbm.receiver, "extend", &[PyObject::list(items)]);
                        }
                    }
                }
                // list.sort(key=, reverse=) needs VM for key function calls
                if bbm.method_name.as_str() == "sort" {
                    if let PyObjectPayload::List(items) = &bbm.receiver.payload {
                        // Extract key and reverse from trailing kwargs dict
                        let mut key_fn: Option<PyObjectRef> = None;
                        let mut reverse = false;
                        for arg in &args {
                            if let PyObjectPayload::Dict(d) = &arg.payload {
                                let rd = d.read();
                                if let Some(v) = rd.get(&HashableKey::str_key(CompactString::from("reverse"))) {
                                    reverse = v.is_truthy();
                                }
                                if let Some(v) = rd.get(&HashableKey::str_key(CompactString::from("key"))) {
                                    if !matches!(v.payload, PyObjectPayload::None) {
                                        key_fn = Some(v.clone());
                                    }
                                }
                            }
                        }
                        if let Some(key) = key_fn {
                            // Decorate-sort-undecorate (Schwartzian transform)
                            let mut w = items.write();
                            let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
                            for item in w.iter() {
                                let k = self.call_object(key.clone(), vec![item.clone()])?;
                                decorated.push((k, item.clone()));
                            }
                            let keys: Vec<PyObjectRef> = decorated.iter().map(|(k, _)| k.clone()).collect();
                            let mut indices: Vec<usize> = (0..decorated.len()).collect();
                            for i in 1..indices.len() {
                                let mut j = i;
                                while j > 0 {
                                    if self.vm_lt(&keys[indices[j]], &keys[indices[j - 1]])? {
                                        indices.swap(j, j - 1);
                                        j -= 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                            w.clear();
                            for i in indices {
                                w.push(decorated[i].1.clone());
                            }
                            if reverse {
                                w.reverse();
                            }
                            return Ok(PyObject::none());
                        } else if reverse {
                            let mut w = items.write();
                            let mut v: Vec<_> = w.drain(..).collect();
                            self.vm_sort(&mut v)?;
                            v.reverse();
                            w.extend(v);
                            return Ok(PyObject::none());
                        }
                        // No key or reverse — fall through to basic sort
                    }
                }
                // str.format with positional args: needs VM for __str__ on instances
                if bbm.method_name.as_str() == "format" {
                    if let PyObjectPayload::Str(s) = &bbm.receiver.payload {
                        return self.vm_str_format(s, &args);
                    }
                }
                // str.format_map with dict subclass: needs VM for __missing__ calls
                if bbm.method_name.as_str() == "format_map" && !args.is_empty() {
                    if let PyObjectPayload::Str(s) = &bbm.receiver.payload {
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(ref ds) = inst.dict_storage {
                                return self.vm_format_map(s, &args[0], ds, &inst.class);
                            }
                        }
                        // Handle defaultdict (Dict payload with __defaultdict_factory__)
                        if let PyObjectPayload::Dict(m) = &args[0].payload {
                            let factory_key = ferrython_core::types::HashableKey::str_key(CompactString::from("__defaultdict_factory__"));
                            if m.read().contains_key(&factory_key) {
                                return self.vm_format_map_dict(s, &args[0], m);
                            }
                        }
                    }
                }
                builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), &args)
            }
            PyObjectPayload::ExceptionType(kind) => {
                // Calling an exception type creates an exception instance
                let msg: CompactString = if args.is_empty() {
                    CompactString::default()
                } else if let PyObjectPayload::Str(s) = &args[0].payload {
                    s.clone()
                } else {
                    CompactString::from(args[0].py_to_string())
                };
                // Only clone args for exception types that need post-processing;
                // common types (ValueError, TypeError, etc.) just move args in.
                let needs_post = matches!(kind, ExceptionKind::ExceptionGroup | ExceptionKind::BaseExceptionGroup)
                    || (kind.is_subclass_of(&ExceptionKind::OSError) && args.len() >= 2)
                    || (*kind == ExceptionKind::SystemExit && !args.is_empty());
                if needs_post {
                    let inst = PyObject::exception_instance_with_args(*kind, msg, args.clone());
                    // ExceptionGroup/BaseExceptionGroup: store .message and .exceptions attrs
                    if matches!(kind, ExceptionKind::ExceptionGroup | ExceptionKind::BaseExceptionGroup) {
                        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
                            let mut a = ei.ensure_attrs().write();
                            if !args.is_empty() {
                                a.insert(CompactString::from("message"), args[0].clone());
                            }
                            if args.len() >= 2 {
                                let exc_list = match &args[1].payload {
                                    PyObjectPayload::List(_) => args[1].clone(),
                                    PyObjectPayload::Tuple(items) => PyObject::list(items.clone()),
                                    _ => PyObject::list(vec![args[1].clone()]),
                                };
                                a.insert(CompactString::from("exceptions"), exc_list);
                                drop(a);
                                attach_eg_methods(&inst);
                            }
                        }
                    }
                    // OSError and subclasses: OSError(errno, strerror[, filename])
                    if kind.is_subclass_of(&ExceptionKind::OSError) && args.len() >= 2 {
                        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
                            let mut a = ei.ensure_attrs().write();
                            a.insert(CompactString::from("errno"), args[0].clone());
                            a.insert(CompactString::from("strerror"), args[1].clone());
                            if args.len() >= 3 {
                                a.insert(CompactString::from("filename"), args[2].clone());
                            } else {
                                a.insert(CompactString::from("filename"), PyObject::none());
                            }
                        }
                    }
                    // SystemExit: store .code attribute
                    if *kind == ExceptionKind::SystemExit && !args.is_empty() {
                        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
                            ei.ensure_attrs().write().insert(CompactString::from("code"), args[0].clone());
                        }
                    }
                    Ok(inst)
                } else {
                    // Common case: no post-processing, move args directly (zero-clone)
                    Ok(PyObject::exception_instance_with_args(*kind, msg, args))
                }
            }
            PyObjectPayload::NativeFunction(nf_data) => {
                // Intercept functions that need VM access to call Python callables
                // property.__get__(self, obj, objtype) — must call fget(obj) and return result
                if nf_data.name.as_str() == "property.__get__" {
                    if args.is_empty() {
                        return Err(PyException::type_error("descriptor '__get__' requires a property object"));
                    }
                    let prop = &args[0];
                    let obj = args.get(1);
                    let is_none_obj = match obj {
                        Some(o) => matches!(&o.payload, PyObjectPayload::None),
                        None => true,
                    };
                    if is_none_obj {
                        return Ok(prop.clone());
                    }
                    let obj = obj.unwrap();
                    // Try native Property payload first
                    if let PyObjectPayload::Property(pd) = &prop.payload {
                        if let Some(getter) = pd.fget.as_ref() {
                            let getter = crate::builtins::unwrap_abstract_fget(getter);
                            return self.call_object(getter, vec![obj.clone()]);
                        }
                        return Err(PyException::attribute_error("unreadable attribute"));
                    }
                    // Instance subclass of property — look for fget in instance attrs
                    if let PyObjectPayload::Instance(inst) = &prop.payload {
                        if let Some(fget) = inst.attrs.read().get("fget").cloned() {
                            if !matches!(&fget.payload, PyObjectPayload::None) {
                                return self.call_object(fget, vec![obj.clone()]);
                            }
                        }
                    }
                    return Ok(prop.clone());
                }
                if nf_data.name.as_str() == "functools.reduce" {
                    return self.vm_functools_reduce(&args);
                }
                if nf_data.name.as_str() == "itertools.islice" {
                    return self.vm_itertools_islice(&args);
                }
                // singledispatch.register: register(type) → decorator
                if nf_data.name.as_str() == "singledispatch.register" {
                    return self.vm_singledispatch_register(&args);
                }
                // type.__call__(cls, *args) — standard class instantiation protocol
                if nf_data.name.as_str() == "__type_call__" {
                    if args.is_empty() {
                        return Err(PyException::type_error("type.__call__ requires cls"));
                    }
                    let cls = args[0].clone();
                    let rest = args[1..].to_vec();
                    return self.instantiate_class(&cls, rest, vec![]);
                }
                // re.sub / re.subn with callable replacement
                if (nf_data.name.as_str() == "re.sub" || nf_data.name.as_str() == "re.subn") && args.len() >= 3 {
                    let repl = &args[1];
                    let is_callable = matches!(&repl.payload,
                        PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
                        | PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_)
                        | PyObjectPayload::Partial(_));
                    if is_callable {
                        return self.re_sub_with_callable(&args, nf_data.name.as_str() == "re.subn");
                    }
                }
                if nf_data.name.as_str() == "itertools.groupby" {
                    let mut key_fn = None;
                    let mut iterable_end = args.len();
                    // Check last arg for kwargs dict with "key"
                    if let Some(last) = args.last() {
                        if let PyObjectPayload::Dict(map) = &last.payload {
                            let map_r = map.read();
                            key_fn = map_r.get(&HashableKey::str_key(CompactString::from("key"))).cloned();
                            if key_fn.is_some() {
                                iterable_end = args.len() - 1;
                            }
                        }
                    }
                    // Check for positional key arg (2nd arg, not a dict)
                    if key_fn.is_none() && iterable_end >= 2 {
                        key_fn = Some(args[1].clone());
                        iterable_end = 1;
                    }
                    return self.vm_itertools_groupby(&args[..iterable_end], key_fn);
                }
                if nf_data.name.as_str() == "itertools.filterfalse" && args.len() >= 2 {
                    return self.vm_itertools_filterfalse(&args);
                }
                if nf_data.name.as_str() == "itertools.starmap" && args.len() >= 2 {
                    return self.vm_itertools_starmap(&args);
                }
                if nf_data.name.as_str() == "itertools.accumulate" && args.len() >= 2 {
                    return self.vm_itertools_accumulate(&args);
                }
                // math.trunc / math.floor / math.ceil — dispatch to __trunc__ / __floor__ / __ceil__
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        let dunder = match nf_data.name.as_str() {
                            "math.trunc" => Some("__trunc__"),
                            "math.floor" => Some("__floor__"),
                            "math.ceil" => Some("__ceil__"),
                            _ => None,
                        };
                        if let Some(dunder_name) = dunder {
                            if let Some(method) = args[0].get_attr(dunder_name) {
                                let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                                return self.call_object(method, ca);
                            }
                        }
                    }
                }
                // os.fspath — dispatch to __fspath__
                if nf_data.name.as_str() == "os.fspath" && args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = args[0].get_attr("__fspath__") {
                            let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                            return self.call_object(method, ca);
                        }
                    }
                }
                // Resolve generators to lists for stdlib NativeFunctions
                // that expect iterables (e.g. Counter, deque, OrderedDict, set)
                if !args.is_empty()
                    && matches!(&args[0].payload, PyObjectPayload::Generator(_))
                {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                    resolved.extend_from_slice(&args[1..]);
                    return (nf_data.func)(&resolved);
                }
                let result = (nf_data.func)(&args)?;
                // Check if native function requested VM method calls
                let collect_mode = ferrython_core::error::take_collect_vm_call_results();
                if collect_mode {
                    let mut collected = Vec::new();
                    while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                        collected.push(self.call_object(method, margs)?);
                    }
                    if !collected.is_empty() {
                        return Ok(PyObject::list(collected));
                    }
                }
                let mut last_result = None;
                while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                    last_result = Some(self.call_object(method, margs)?);
                }
                if let Some(r) = last_result {
                    return Ok(r);
                }
                // Execute any deferred calls (e.g., HTMLParser.feed() callbacks)
                let deferred = ferrython_stdlib::drain_deferred_calls();
                for (dfunc, dargs) in deferred {
                    self.call_object(dfunc, dargs)?;
                }
                Ok(result)
            }
            PyObjectPayload::NativeClosure(nc) => {
                // Resolve generators to lists for NativeClosure functions
                let args = if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Generator(_)) {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                    resolved.extend_from_slice(&args[1..]);
                    resolved
                } else {
                    args
                };
                let result = (nc.func)(&args)?;
                // Check if stdlib requested VM method calls (loop for multiple)
                let collect_mode = ferrython_core::error::take_collect_vm_call_results();
                if collect_mode {
                    let mut collected = Vec::new();
                    while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                        collected.push(self.call_object(method, margs)?);
                    }
                    if !collected.is_empty() {
                        return Ok(PyObject::list(collected));
                    }
                }
                let mut last_result = None;
                while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                    last_result = Some(self.call_object(method, margs)?);
                }
                if let Some(r) = last_result {
                    return Ok(r);
                }
                // Execute any deferred calls (e.g., Thread.start() calling Python functions)
                let deferred = ferrython_stdlib::drain_deferred_calls();
                for (dfunc, dargs) in deferred {
                    self.call_object(dfunc, dargs)?;
                }
                // Check if asyncio.run() was invoked — drive the coroutine to completion
                if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
                    return self.maybe_await_result(coro);
                }
                Ok(result)
            }
            PyObjectPayload::Partial(pd) => {
                let partial_func = pd.func.clone();
                let mut combined_args = pd.args.clone();
                combined_args.extend(args);
                if !pd.kwargs.is_empty() {
                    let kw: Vec<(CompactString, PyObjectRef)> = pd.kwargs.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    self.call_object_kw(partial_func, combined_args, kw)
                } else {
                    self.call_object(partial_func, combined_args)
                }
            }
            PyObjectPayload::Instance(_inst) => {
                // lru_cache wrapper: check _cache + __wrapped__
                if let Some(cache_obj) = func.get_attr("_cache") {
                    if let Some(wrapped) = func.get_attr("__wrapped__") {
                        if let PyObjectPayload::Dict(cache_map) = &cache_obj.payload {
                            // Build cache key from stringified args
                            let key_str = args.iter().map(|a| a.repr()).collect::<Vec<_>>().join(",");
                            let cache_key = HashableKey::str_key(CompactString::from(&key_str));
                            // Check cache
                            let cached_val = cache_map.read().get(&cache_key).cloned();
                            if let Some(cached) = cached_val {
                                // Cache hit: move to MRU position (re-insert at end) for LRU eviction
                                {
                                    let mut cw = cache_map.write();
                                    cw.shift_remove(&cache_key);
                                    cw.insert(cache_key, cached.clone());
                                }
                                // Increment _hits counter
                                if let PyObjectPayload::Instance(ref d) = func.payload {
                                    let mut w = d.attrs.write();
                                    let hits = w.get(&intern_or_new("_hits"))
                                        .and_then(|v| v.as_int()).unwrap_or(0);
                                    w.insert(intern_or_new("_hits"), PyObject::int(hits + 1));
                                }
                                return Ok(cached);
                            }
                            // Cache miss: call the wrapped function, increment _misses
                            if let PyObjectPayload::Instance(ref d) = func.payload {
                                let mut w = d.attrs.write();
                                let misses = w.get(&intern_or_new("_misses"))
                                    .and_then(|v| v.as_int()).unwrap_or(0);
                                w.insert(intern_or_new("_misses"), PyObject::int(misses + 1));
                            }
                            let result = self.call_object(wrapped, args)?;
                            // Enforce maxsize: evict LRU entry (first in insertion order) when cache is full
                            {
                                let mut cache_w = cache_map.write();
                                if let PyObjectPayload::Instance(ref d) = func.payload {
                                    let maxsize = d.attrs.read()
                                        .get(&intern_or_new("_maxsize"))
                                        .and_then(|v| v.as_int());
                                    if let Some(max) = maxsize {
                                        if max >= 0 {
                                            while cache_w.len() >= max as usize {
                                                cache_w.shift_remove_index(0);
                                            }
                                        }
                                    }
                                }
                                cache_w.insert(cache_key, result.clone());
                            }
                            return Ok(result);
                        }
                    }
                }
                // Callable instances: check for __call__
                if func.get_attr("__singledispatch__").is_some() {
                    // singledispatch: dispatch based on first arg type
                    return self.vm_singledispatch_call_instance(&func, &args);
                }
                if let Some(method) = func.get_attr("__call__") {
                    self.call_object(method, args)
                } else {
                    Err(PyException::type_error(format!(
                        "'{}' object is not callable", func.type_name()
                    )))
                }
            }
            _ => Err(PyException::type_error(format!(
                "'{}' object is not callable", func.type_name()
            ))),
        }
    }

    /// Install closure cells, set scope, and either return generator/coroutine or execute frame.
    fn install_closure_and_run(
        &mut self,
        mut frame: Frame,
        code: &CodeObject,
        closure: &[Rc<PyCell<Option<PyObjectRef>>>],
    ) -> PyResult<PyObjectRef> {
        let n_cell = code.cellvars.len();
        for (i, cell) in closure.iter().enumerate() {
            if n_cell + i < frame.cells.len() {
                frame.cells[n_cell + i] = cell.clone();
            }
        }
        for (cell_idx, cell_name) in code.cellvars.iter().enumerate() {
            for (var_idx, var_name) in code.varnames.iter().enumerate() {
                if cell_name == var_name {
                    if let Some(val) = frame.locals[var_idx].take() {
                        *frame.cells[cell_idx].write() = Some(val);
                    }
                    break;
                }
            }
        }
        frame.scope_kind = ScopeKind::Function;

        if code.flags.contains(CodeFlags::GENERATOR) && code.flags.contains(CodeFlags::COROUTINE) {
            return Ok(PyObject::async_generator(CompactString::from(code.name.as_str()), Box::new(frame)));
        }
        if code.flags.contains(CodeFlags::COROUTINE) {
            return Ok(PyObject::coroutine(CompactString::from(code.name.as_str()), Box::new(frame)));
        }
        if code.flags.contains(CodeFlags::GENERATOR) {
            return Ok(PyObject::generator(CompactString::from(code.name.as_str()), Box::new(frame)));
        }

        self.call_stack.push(frame);
        // Check recursion limit
        if self.call_stack.len() > self.recursion_limit {
            if let Some(frame) = self.call_stack.pop() {
                frame.recycle(&mut self.frame_pool);
            }
            return Err(PyException::recursion_error(
                "maximum recursion depth exceeded"
            ));
        }
        let result = self.run_frame();
        if let Some(frame) = self.call_stack.pop() {
            frame.recycle(&mut self.frame_pool);
        }
        result
    }

    /// Schwartzian transform: sort items by key function, optionally reversed.
    fn sort_with_key(
        &mut self,
        items: &mut Vec<PyObjectRef>,
        key_fn: Option<PyObjectRef>,
        reverse: bool,
    ) -> PyResult<()> {
        if let Some(key) = key_fn {
            // Check if key is a cmp_to_key class — use comparison function directly
            if let PyObjectPayload::Class(cd) = &key.payload {
                if let Some(cmp_func) = cd.namespace.read().get("__cmp_to_key_func__").cloned() {
                    // Sort using comparison function: cmp(a, b) < 0 means a < b
                    let mut indices: Vec<usize> = (0..items.len()).collect();
                    for i in 1..indices.len() {
                        let mut j = i;
                        while j > 0 {
                            let a = &items[indices[j]];
                            let b = &items[indices[j - 1]];
                            let result = self.call_object(cmp_func.clone(), vec![a.clone(), b.clone()])?;
                            let cmp_val = result.to_int().unwrap_or(0);
                            if cmp_val < 0 {
                                indices.swap(j, j - 1);
                                j -= 1;
                            } else {
                                break;
                            }
                        }
                    }
                    *items = indices.into_iter().map(|i| items[i].clone()).collect();
                    if reverse { items.reverse(); }
                    return Ok(());
                }
            }
            // Normal key function sort
            let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
            for item in items.iter() {
                let k = self.call_object(key.clone(), vec![item.clone()])?;
                decorated.push((k, item.clone()));
            }
            let mut indices: Vec<usize> = (0..decorated.len()).collect();
            for i in 1..indices.len() {
                let mut j = i;
                while j > 0 {
                    let cmp = if reverse {
                        // Sort descending directly for stable reverse
                        self.vm_lt(&decorated[indices[j - 1]].0, &decorated[indices[j]].0)?
                    } else {
                        self.vm_lt(&decorated[indices[j]].0, &decorated[indices[j - 1]].0)?
                    };
                    if cmp {
                        indices.swap(j, j - 1);
                        j -= 1;
                    } else {
                        break;
                    }
                }
            }
            *items = indices.into_iter().map(|i| decorated[i].1.clone()).collect();
        } else {
            self.vm_sort(items)?;
            if reverse {
                items.reverse();
            }
        }
        Ok(())
    }

    /// Compute min or max from a collection, with optional key function and default value.
    fn compute_min_max(
        &mut self,
        items: Vec<PyObjectRef>,
        is_max: bool,
        key_fn: Option<PyObjectRef>,
        default: Option<PyObjectRef>,
        func_name: &str,
    ) -> PyResult<PyObjectRef> {
        if items.is_empty() {
            return if let Some(d) = default {
                Ok(d)
            } else {
                Err(PyException::value_error(format!("{}() arg is an empty sequence", func_name)))
            };
        }
        let mut best = items[0].clone();
        let mut best_key = if let Some(ref kf) = key_fn {
            self.call_object(kf.clone(), vec![best.clone()])?
        } else {
            best.clone()
        };
        for item in &items[1..] {
            let item_key = if let Some(ref kf) = key_fn {
                self.call_object(kf.clone(), vec![item.clone()])?
            } else {
                item.clone()
            };
            let better = if is_max {
                self.vm_lt(&best_key, &item_key)?
            } else {
                self.vm_lt(&item_key, &best_key)?
            };
            if better {
                best = item.clone();
                best_key = item_key;
            }
        }
        Ok(best)
    }

    /// Post-process parsed JSON: apply object_hook, parse_float, parse_int
    /// by calling Python functions via the VM.
    fn json_apply_hooks(
        &mut self,
        value: &PyObjectRef,
        object_hook: &Option<PyObjectRef>,
        parse_float: &Option<PyObjectRef>,
        parse_int: &Option<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match &value.payload {
            PyObjectPayload::Dict(map) => {
                // Recursively apply hooks to values first
                let entries: Vec<_> = map.read().iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut new_map = IndexMap::new();
                for (k, v) in entries {
                    new_map.insert(k, self.json_apply_hooks(&v, object_hook, parse_float, parse_int)?);
                }
                let new_dict = PyObject::dict(new_map);
                // Apply object_hook to the dict
                if let Some(hook) = object_hook {
                    self.call_object(hook.clone(), vec![new_dict])
                } else {
                    Ok(new_dict)
                }
            }
            PyObjectPayload::List(items) => {
                let items: Vec<_> = items.read().clone();
                let mut result = Vec::with_capacity(items.len());
                for item in &items {
                    result.push(self.json_apply_hooks(item, object_hook, parse_float, parse_int)?);
                }
                Ok(PyObject::list(result))
            }
            PyObjectPayload::Float(_) => {
                if let Some(pf) = parse_float {
                    let s = PyObject::str_val(CompactString::from(value.py_to_string()));
                    self.call_object(pf.clone(), vec![s])
                } else {
                    Ok(value.clone())
                }
            }
            PyObjectPayload::Int(_) => {
                if let Some(pi) = parse_int {
                    let s = PyObject::str_val(CompactString::from(value.py_to_string()));
                    self.call_object(pi.clone(), vec![s])
                } else {
                    Ok(value.clone())
                }
            }
            _ => Ok(value.clone()),
        }
    }

    /// Pre-process an object tree for json.dumps: replace non-JSON-serializable
    /// values by calling `default(obj)` (a user Python function). Basic types
    /// (dict, list, tuple, str, int, float, bool, None) are passed through.
    fn json_prepare_with_default(
        &mut self,
        obj: &PyObjectRef,
        default_fn: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        match &obj.payload {
            PyObjectPayload::Dict(map) => {
                let entries: Vec<_> = map.read().iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut new_map = IndexMap::new();
                for (k, v) in entries {
                    new_map.insert(k, self.json_prepare_with_default(&v, default_fn)?);
                }
                Ok(PyObject::dict(new_map))
            }
            PyObjectPayload::InstanceDict(map) => {
                // Instance __dict__ uses CompactString keys, convert to HashableKey
                let entries: Vec<_> = map.read().iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut new_map = IndexMap::new();
                for (k, v) in entries {
                    let prepared = self.json_prepare_with_default(&v, default_fn)?;
                    new_map.insert(HashableKey::str_key(k), prepared);
                }
                Ok(PyObject::dict(new_map))
            }
            PyObjectPayload::List(items) => {
                let items: Vec<_> = items.read().clone();
                let mut prepared = Vec::with_capacity(items.len());
                for item in &items {
                    prepared.push(self.json_prepare_with_default(item, default_fn)?);
                }
                Ok(PyObject::list(prepared))
            }
            PyObjectPayload::Tuple(items) => {
                let mut prepared = Vec::with_capacity(items.len());
                for item in items.iter() {
                    prepared.push(self.json_prepare_with_default(item, default_fn)?);
                }
                Ok(PyObject::tuple(prepared))
            }
            PyObjectPayload::Str(_)
            | PyObjectPayload::Int(_)
            | PyObjectPayload::Float(_)
            | PyObjectPayload::Bool(_)
            | PyObjectPayload::None => Ok(obj.clone()),
            _ => {
                // Call default(obj) and recursively prepare the result
                let result = self.call_object(default_fn.clone(), vec![obj.clone()])?;
                self.json_prepare_with_default(&result, default_fn)
            }
        }
    }
}
