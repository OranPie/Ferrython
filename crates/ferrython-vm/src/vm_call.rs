//! Function/method call dispatch, class instantiation, super().

use crate::builtins;
use crate::frame::{Frame, ScopeKind};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject};
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    AsyncGenAction, CompareOp, IteratorData, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef, is_data_descriptor, lookup_in_class_mro,
};
use ferrython_core::types::{HashableKey, SharedGlobals};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

impl VirtualMachine {
    pub(crate) fn call_function(
        &mut self,
        code: &CodeObject,
        args: Vec<PyObjectRef>,
        defaults: &[PyObjectRef],
        kw_defaults: &IndexMap<CompactString, PyObjectRef>,
        globals: SharedGlobals,
        closure: &[Arc<RwLock<Option<PyObjectRef>>>],
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new(code.clone(), globals, self.builtins.clone());
        let nparams = code.arg_count as usize;
        let nkwonly = code.kwonlyarg_count as usize;
        let has_varargs = code.flags.contains(CodeFlags::VARARGS);
        let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);

        // Assign positional parameters
        let positional_count = args.len().min(nparams);
        for i in 0..positional_count {
            frame.set_local(i, args[i].clone());
        }

        // Fill in defaults for missing positional args
        if args.len() < nparams && !defaults.is_empty() {
            let ndefaults = defaults.len();
            let first_default_param = nparams - ndefaults;
            for i in args.len()..nparams {
                if i >= first_default_param {
                    let default_idx = i - first_default_param;
                    frame.set_local(i, defaults[default_idx].clone());
                }
            }
        }

        // Check for missing required positional args
        if args.len() < nparams {
            let ndefaults = defaults.len();
            let required = nparams - ndefaults;
            if args.len() < required {
                let missing = required - args.len();
                let fname = code.name.as_str();
                let missing_names: Vec<&str> = (args.len()..required)
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
            let extra: Vec<PyObjectRef> = if args.len() > nparams {
                args[nparams..].to_vec()
            } else {
                Vec::new()
            };
            frame.set_local(nparams, PyObject::tuple(extra));
        } else if args.len() > nparams {
            let fname = code.name.as_str();
            return Err(PyException::type_error(format!(
                "{}() takes {} positional argument{} but {} {} given",
                fname, nparams, if nparams == 1 { "" } else { "s" },
                args.len(), if args.len() == 1 { "was" } else { "were" }
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
                frame.set_local(kwargs_idx, PyObject::dict(IndexMap::new()));
            }
        }

        // Install closure cells as free vars in this frame.
        let n_cell = code.cellvars.len();
        for (i, cell) in closure.iter().enumerate() {
            if n_cell + i < frame.cells.len() {
                frame.cells[n_cell + i] = cell.clone();
            }
        }
        // For cell vars that are also parameters, copy the parameter value into the cell
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

        // If the function is a generator/coroutine, return suspended object without executing
        if code.flags.contains(CodeFlags::GENERATOR) && code.flags.contains(CodeFlags::COROUTINE) {
            let name = CompactString::from(code.name.as_str());
            return Ok(PyObject::async_generator(name, Box::new(frame)));
        }
        if code.flags.contains(CodeFlags::COROUTINE) {
            let name = CompactString::from(code.name.as_str());
            return Ok(PyObject::coroutine(name, Box::new(frame)));
        }
        if code.flags.contains(CodeFlags::GENERATOR) {
            let name = CompactString::from(code.name.as_str());
            return Ok(PyObject::generator(name, Box::new(frame)));
        }

        self.call_stack.push(frame);
        let result = self.run_frame();
        self.call_stack.pop();
        result
    }

    pub(crate) fn call_function_kw(
        &mut self,
        code: &CodeObject,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
        defaults: &[PyObjectRef],
        kw_defaults: &IndexMap<CompactString, PyObjectRef>,
        globals: SharedGlobals,
        closure: &[Arc<RwLock<Option<PyObjectRef>>>],
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new(code.clone(), globals, self.builtins.clone());
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

        // Assign positional parameters
        let positional_count = pos_args.len().min(nparams);
        for i in 0..positional_count {
            frame.set_local(i, pos_args[i].clone());
        }

        // Place keyword args at their correct parameter positions
        let posonlyarg_count = code.posonlyarg_count as usize;
        let mut extra_kwargs: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        for (name, val) in &kwargs {
            if let Some(idx) = code.varnames.iter().position(|v| v.as_str() == name.as_str()) {
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
                    frame.set_local(idx, val.clone());
                    continue;
                }
            }
            // Not a known parameter — goes into **kwargs
            extra_kwargs.insert(
                HashableKey::Str(name.clone()),
                val.clone(),
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
            let extra: Vec<PyObjectRef> = if pos_args.len() > nparams {
                pos_args[nparams..].to_vec()
            } else {
                Vec::new()
            };
            frame.set_local(varargs_slot, PyObject::tuple(extra));
        } else if pos_args.len() > nparams {
            let fname = code.name.as_str();
            return Err(PyException::type_error(format!(
                "{}() takes {} positional argument{} but {} {} given",
                fname, nparams, if nparams == 1 { "" } else { "s" },
                pos_args.len(), if pos_args.len() == 1 { "was" } else { "were" }
            )));
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            frame.set_local(kwargs_idx, PyObject::dict(extra_kwargs));
        }

        // Install closure cells
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

        // If the function is a generator/coroutine, return suspended object without executing
        if code.flags.contains(CodeFlags::GENERATOR) && code.flags.contains(CodeFlags::COROUTINE) {
            let name = CompactString::from(code.name.as_str());
            return Ok(PyObject::async_generator(name, Box::new(frame)));
        }
        if code.flags.contains(CodeFlags::COROUTINE) {
            let name = CompactString::from(code.name.as_str());
            return Ok(PyObject::coroutine(name, Box::new(frame)));
        }
        if code.flags.contains(CodeFlags::GENERATOR) {
            let name = CompactString::from(code.name.as_str());
            return Ok(PyObject::generator(name, Box::new(frame)));
        }

        self.call_stack.push(frame);
        let result = self.run_frame();
        self.call_stack.pop();
        result
    }

    /// Unified class instantiation: __new__, dataclass/namedtuple auto-init, __init__, exception attrs.
    pub(crate) fn instantiate_class(
        &mut self,
        cls: &PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        // Enum lookup: Color(2) returns the member with that value
        if let PyObjectPayload::Class(cd) = &cls.payload {
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
        // Check for abstract methods (ABC support)
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let mut abstract_names: Vec<String> = Vec::new();
            // Check this class's own namespace for abstract markers
            {
                let ns = cd.namespace.read();
                for (name, val) in ns.iter() {
                    if let PyObjectPayload::Tuple(items) = &val.payload {
                        if items.len() == 2 {
                            if let Some(s) = items[0].as_str() {
                                if s == "__abstract__" {
                                    abstract_names.push(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
            // Also check base classes for abstract methods not overridden
            for base in &cd.bases {
                if let PyObjectPayload::Class(base_cd) = &base.payload {
                    let base_ns = base_cd.namespace.read();
                    for (name, val) in base_ns.iter() {
                        if let PyObjectPayload::Tuple(items) = &val.payload {
                            if items.len() == 2 {
                                if let Some(s) = items[0].as_str() {
                                    if s == "__abstract__" {
                                        let overridden = cd.namespace.read().get(name.as_str())
                                            .map(|v| !matches!(&v.payload, PyObjectPayload::Tuple(t) if t.len() == 2 && t[0].as_str() == Some("__abstract__")))
                                            .unwrap_or(false);
                                        if !overridden && !abstract_names.contains(&name.to_string()) {
                                            abstract_names.push(name.to_string());
                                        }
                                    }
                                }
                            }
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
        // __new__
        let instance = if let Some(new_method) = cls.get_attr("__new__") {
            // If __new__ is from a BuiltinType base (dict, list, etc.), just create instance
            let is_builtin_new = matches!(&new_method.payload,
                PyObjectPayload::BuiltinBoundMethod { receiver, .. }
                    if matches!(&receiver.payload, PyObjectPayload::BuiltinType(_))
            );
            if is_builtin_new {
                PyObject::instance(cls.clone())
            } else {
                let new_fn = match &new_method.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => new_method.clone(),
                };
                let mut new_args = vec![cls.clone()];
                new_args.extend(pos_args.clone());
                self.call_object(new_fn, new_args)?
            }
        } else {
            PyObject::instance(cls.clone())
        };

        // Check markers in class namespace directly, not via get_attr,
        // because BuiltinType get_attr can return false positives.
        let class_has_key = |obj: &PyObjectRef, key: &str| -> bool {
            match &obj.payload {
                PyObjectPayload::Class(cd) => cd.namespace.read().contains_key(key),
                _ => false,
            }
        };
        let is_dataclass = class_has_key(cls, "__dataclass__");
        let has_user_init = cls.get_attr("__init__").is_some();

        if is_dataclass && !has_user_init {
            // Dataclass auto-init: populate fields from args/kwargs
            if let Some(fields) = cls.get_attr("__dataclass_fields__") {
                if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                    let mut arg_idx = 0;
                    for ft in field_tuples {
                        if let PyObjectPayload::Tuple(info) = &ft.payload {
                            let name = info[0].py_to_string();
                            let has_default = info[1].is_truthy();
                            let default_val = &info[2];

                            let value = if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == name.as_str()) {
                                v.clone()
                            } else if arg_idx < pos_args.len() {
                                let v = pos_args[arg_idx].clone();
                                arg_idx += 1;
                                v
                            } else if has_default {
                                // If default is callable (factory), call it
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
                }
            }
        } else if class_has_key(cls, "__namedtuple__") {
            // Namedtuple: populate named fields
            if let Some(fields) = cls.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        let mut attrs = inst.attrs.write();
                        for (i, field) in field_names.iter().enumerate() {
                            let name = field.py_to_string();
                            let value = if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == name.as_str()) {
                                v.clone()
                            } else if i < pos_args.len() {
                                pos_args[i].clone()
                            } else {
                                PyObject::none()
                            };
                            attrs.insert(CompactString::from(name.as_str()), value);
                        }
                        attrs.insert(CompactString::from("_tuple"), PyObject::tuple(pos_args.clone()));
                    }
                }
            }
        } else if let Some(init) = cls.get_attr("__init__") {
            // Skip builtin __init__ — instance already created, no user code to run
            let is_builtin_init = matches!(&init.payload,
                PyObjectPayload::BuiltinBoundMethod { receiver, .. }
                    if matches!(&receiver.payload, PyObjectPayload::BuiltinType(_)));
            if !is_builtin_init {
                let init_fn = match &init.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => init.clone(),
                };
                let mut init_args = vec![instance.clone()];
                init_args.extend(pos_args.clone());
                if kwargs.is_empty() {
                    self.call_object(init_fn, init_args)?;
                } else {
                    self.call_object_kw(init_fn, init_args, kwargs.clone())?;
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
                        storage.insert(HashableKey::Str(k.clone()), v.clone());
                    }
                }
            }
        }

        // Exception subclass attrs
        if Self::is_exception_class(cls) {
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                let mut attrs = inst.attrs.write();
                if !attrs.contains_key("args") {
                    attrs.insert(CompactString::from("args"), PyObject::tuple(pos_args.clone()));
                }
                if !attrs.contains_key("message") && !pos_args.is_empty() {
                    attrs.insert(CompactString::from("message"), pos_args[0].clone());
                }
            }
        }

        Ok(instance)
    }

    /// Build a super() proxy from current call frame or explicit args.
    pub(crate) fn make_super(&self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            let frame = self.call_stack.last().unwrap();
            if let Some(self_obj) = frame.locals.first().cloned().flatten() {
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
                    _ => return Err(PyException::runtime_error("super(): no current class")),
                };

                let mut cls = runtime_cls.clone();
                if let Some(def_name) = defining_class_name {
                    if let PyObjectPayload::Class(cd) = &runtime_cls.payload {
                        let mro = if cd.mro.is_empty() {
                            vec![runtime_cls.clone()]
                        } else {
                            cd.mro.clone()
                        };
                        for m in &mro {
                            if let PyObjectPayload::Class(mc) = &m.payload {
                                if mc.name.as_str() == def_name {
                                    cls = m.clone();
                                    break;
                                }
                            }
                        }
                    }
                }
                return Ok(Arc::new(PyObject {
                    payload: PyObjectPayload::Super { cls, instance: instance_for_super }
                }));
            }
            Err(PyException::runtime_error("super(): no current class"))
        } else if args.len() == 2 {
            Ok(Arc::new(PyObject {
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
                let code = pyfunc.code.clone();
                let globals = pyfunc.globals.clone();
                let defaults = pyfunc.defaults.clone();
                let kw_defaults = pyfunc.kw_defaults.clone();
                let closure = pyfunc.closure.clone();
                self.call_function_kw(&code, pos_args, kwargs, &defaults, &kw_defaults, globals, &closure)
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(pos_args);
                self.call_object_kw(method.clone(), bound_args, kwargs)
            }
            PyObjectPayload::Class(cd) => {
                // If class has a metaclass with __call__, dispatch through it
                if let Some(meta) = &cd.metaclass {
                    if let Some(call_method) = meta.get_attr("__call__") {
                        let mut call_args = vec![func.clone()];
                        call_args.extend(pos_args);
                        if kwargs.is_empty() {
                            return self.call_object(call_method, call_args);
                        } else {
                            return self.call_object_kw(call_method, call_args, kwargs);
                        }
                    }
                }
                self.instantiate_class(&func, pos_args, kwargs)
            }
            _ => {
                // For BuiltinBoundMethod on str.format, pass kwargs as a dict
                if let PyObjectPayload::BuiltinBoundMethod { receiver, method_name } = &func.payload {
                    // Handle list.sort(key=..., reverse=...)
                    if method_name.as_str() == "sort" {
                        if let PyObjectPayload::List(items_arc) = &receiver.payload {
                            let mut items_vec = items_arc.read().clone();
                            let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone());
                            let reverse = kwargs.iter().find(|(k, _)| k.as_str() == "reverse")
                                .map(|(_, v)| v.is_truthy()).unwrap_or(false);
                            if let Some(key) = key_fn {
                                let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
                                for item in &items_vec {
                                    let k = self.call_object(key.clone(), vec![item.clone()])?;
                                    decorated.push((k, item.clone()));
                                }
                                let mut indices: Vec<usize> = (0..decorated.len()).collect();
                                for i in 1..indices.len() {
                                    let mut j = i;
                                    while j > 0 {
                                        let should_swap = if reverse {
                                            self.vm_lt(&decorated[indices[j - 1]].0, &decorated[indices[j]].0)?
                                        } else {
                                            self.vm_lt(&decorated[indices[j]].0, &decorated[indices[j - 1]].0)?
                                        };
                                        if should_swap {
                                            indices.swap(j, j - 1);
                                            j -= 1;
                                        } else { break; }
                                    }
                                }
                                items_vec = indices.into_iter().map(|i| decorated[i].1.clone()).collect();
                            } else {
                                self.vm_sort(&mut items_vec)?;
                                if reverse { items_vec.reverse(); }
                            }
                            *items_arc.write() = items_vec;
                            return Ok(PyObject::none());
                        }
                    }
                    // Handle dict.update(key=val, ...)
                    if method_name.as_str() == "update" && !kwargs.is_empty() {
                        if let PyObjectPayload::Dict(map) = &receiver.payload {
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
                                w.insert(HashableKey::Str(k.clone()), v.clone());
                            }
                            return Ok(PyObject::none());
                        }
                    }
                    if method_name.as_str() == "format" && !kwargs.is_empty() {
                        if let PyObjectPayload::Str(s) = &receiver.payload {
                            // Handle str.format() with named args
                            let mut result = String::new();
                            let mut chars = s.chars().peekable();
                            let mut arg_idx = 0usize;
                            while let Some(c) = chars.next() {
                                if c == '{' {
                                    if chars.peek() == Some(&'{') {
                                        chars.next();
                                        result.push('{');
                                    } else if chars.peek() == Some(&'}') {
                                        chars.next();
                                        if arg_idx < pos_args.len() {
                                            result.push_str(&pos_args[arg_idx].py_to_string());
                                            arg_idx += 1;
                                        }
                                    } else {
                                        let mut field = String::new();
                                        for c in chars.by_ref() {
                                            if c == '}' { break; }
                                            field.push(c);
                                        }
                                        // Try numeric index first
                                        if let Ok(idx) = field.parse::<usize>() {
                                            if idx < pos_args.len() {
                                                result.push_str(&pos_args[idx].py_to_string());
                                            }
                                        } else {
                                            // Named arg lookup
                                            let found = kwargs.iter().find(|(k, _)| k.as_str() == field);
                                            if let Some((_, v)) = found {
                                                result.push_str(&v.py_to_string());
                                            }
                                        }
                                    }
                                } else if c == '}' && chars.peek() == Some(&'}') {
                                    chars.next();
                                    result.push('}');
                                } else {
                                    result.push(c);
                                }
                            }
                            return Ok(PyObject::str_val(CompactString::from(result)));
                        }
                    }
                }
                // Generic BuiltinBoundMethod kwargs: pass as trailing dict
                if let PyObjectPayload::BuiltinBoundMethod { .. } = &func.payload {
                    if !kwargs.is_empty() {
                        let mut all_args = pos_args;
                        let mut kw_map = IndexMap::new();
                        for (k, v) in kwargs {
                            kw_map.insert(HashableKey::Str(k), v);
                        }
                        all_args.push(PyObject::dict(kw_map));
                        return self.call_object(func, all_args);
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
                                if let Some(key) = key_fn {
                                    // Decorate-sort-undecorate (Schwartzian transform)
                                    let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
                                    for item in &items_vec {
                                        let k = self.call_object(key.clone(), vec![item.clone()])?;
                                        decorated.push((k, item.clone()));
                                    }
                                    // Sort ascending by key, then reverse if needed (matches CPython)
                                    let keys: Vec<PyObjectRef> = decorated.iter().map(|(k, _)| k.clone()).collect();
                                    let mut indices: Vec<usize> = (0..decorated.len()).collect();
                                    // Insertion sort on indices by key (stable, ascending)
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
                                    items_vec = indices.into_iter().map(|i| decorated[i].1.clone()).collect();
                                    if reverse {
                                        items_vec.reverse();
                                    }
                                } else {
                                    self.vm_sort(&mut items_vec)?;
                                    if reverse {
                                        items_vec.reverse();
                                    }
                                }
                                return Ok(PyObject::list(items_vec));
                            }
                        }
                        "print" => {
                            let sep = kwargs.iter().find(|(k, _)| k.as_str() == "sep")
                                .map(|(_, v)| v.py_to_string()).unwrap_or_else(|| " ".to_string());
                            let end = kwargs.iter().find(|(k, _)| k.as_str() == "end")
                                .map(|(_, v)| v.py_to_string()).unwrap_or_else(|| "\n".to_string());
                            let mut parts = Vec::new();
                            for a in &pos_args {
                                parts.push(self.vm_str(a)?);
                            }
                            print!("{}{}", parts.join(&sep), end);
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
                            if items.is_empty() {
                                return if let Some(d) = default {
                                    Ok(d)
                                } else {
                                    Err(PyException::value_error(format!("{}() arg is an empty sequence", name)))
                                };
                            }
                            let mut best = items[0].clone();
                            let mut best_key = if let Some(ref kf) = key_fn {
                                self.call_object(kf.clone(), vec![best.clone()])?
                            } else { best.clone() };
                            for item in &items[1..] {
                                let item_key = if let Some(ref kf) = key_fn {
                                    self.call_object(kf.clone(), vec![item.clone()])?
                                } else { item.clone() };
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
                            return Ok(best);
                        }
                        "super" => {
                            return self.make_super(&pos_args);
                        }
                        "dict" => {
                            let mut map = IndexMap::new();
                            // dict(iterable, **kwargs) or dict(**kwargs)
                            if !pos_args.is_empty() {
                                let items = self.collect_iterable(&pos_args[0])?;
                                for item in &items {
                                    let pair = item.to_list()?;
                                    if pair.len() == 2 {
                                        let hk = pair[0].to_hashable_key()?;
                                        map.insert(hk, pair[1].clone());
                                    }
                                }
                            }
                            for (k, v) in &kwargs {
                                map.insert(HashableKey::Str(k.clone()), v.clone());
                            }
                            return Ok(PyObject::dict(map));
                        }
                        _ => {}
                    }
                }
                // Handle other payload types that support kwargs
                match &func.payload {
                    PyObjectPayload::NativeFunction { func: nf, name } => {
                        if name.as_str() == "functools.partial" {
                            // functools.partial(func, *args, **kwargs)
                            if pos_args.is_empty() {
                                return Err(PyException::type_error("partial() requires at least 1 argument"));
                            }
                            let pf = pos_args[0].clone();
                            let pa = if pos_args.len() > 1 { pos_args[1..].to_vec() } else { vec![] };
                            return Ok(PyObject::wrap(PyObjectPayload::Partial {
                                func: pf, args: pa, kwargs,
                            }));
                        }
                        // re.sub / re.subn with callable replacement
                        if (name.as_str() == "re.sub" || name.as_str() == "re.subn") && pos_args.len() >= 3 {
                            let repl = &pos_args[1];
                            let is_callable = matches!(&repl.payload,
                                PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
                                | PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. }
                                | PyObjectPayload::Partial { .. });
                            if is_callable {
                                return self.re_sub_with_callable(&pos_args, name.as_str() == "re.subn");
                            }
                        }
                        // itertools.groupby with key function
                        if name.as_str() == "itertools.groupby" {
                            let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone());
                            return self.vm_itertools_groupby(&pos_args, key_fn);
                        }
                        // itertools.accumulate with initial kwarg
                        if name.as_str() == "itertools.accumulate" && !kwargs.is_empty() {
                            let initial = kwargs.iter().find(|(k, _)| k.as_str() == "initial").map(|(_, v)| v.clone());
                            let func_arg = if pos_args.len() >= 2 && !matches!(&pos_args[1].payload, PyObjectPayload::None) {
                                Some(pos_args[1].clone())
                            } else {
                                None
                            };
                            let mut all = vec![pos_args[0].clone()];
                            all.push(func_arg.unwrap_or_else(PyObject::none));
                            all.push(initial.unwrap_or_else(PyObject::none));
                            return nf(&all);
                        }
                        // re.split with maxsplit kwarg
                        if name.as_str() == "re.split" && !kwargs.is_empty() {
                            let mut all = pos_args.clone();
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "maxsplit") {
                                while all.len() < 3 { all.push(PyObject::int(0)); }
                                all[2] = v.clone();
                            }
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "flags") {
                                while all.len() < 4 { all.push(PyObject::int(0)); }
                                all[3] = v.clone();
                            }
                            return nf(&all);
                        }
                        // re.sub with count kwarg
                        if name.as_str() == "re.sub" && !kwargs.is_empty() {
                            let mut all = pos_args.clone();
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "count") {
                                while all.len() < 4 { all.push(PyObject::int(0)); }
                                all[3] = v.clone();
                            }
                            return nf(&all);
                        }
                        // type.__call__(cls, *args, **kwargs) — standard class instantiation
                        if name.as_str() == "__type_call__" {
                            if pos_args.is_empty() {
                                return Err(PyException::type_error("type.__call__ requires cls"));
                            }
                            let cls = pos_args[0].clone();
                            let rest = pos_args[1..].to_vec();
                            return self.instantiate_class(&cls, rest, kwargs);
                        }
                        // Pass kwargs as trailing dict if present
                        if !kwargs.is_empty() {
                            let mut all_args = pos_args;
                            let mut kw_map = IndexMap::new();
                            for (k, v) in kwargs {
                                kw_map.insert(HashableKey::Str(k), v);
                            }
                            all_args.push(PyObject::dict(kw_map));
                            return nf(&all_args);
                        }
                        return nf(&pos_args);
                    }
                    PyObjectPayload::NativeClosure { func, .. } => {
                        let result = if !kwargs.is_empty() {
                            let mut all_args = pos_args;
                            let mut kw_map = IndexMap::new();
                            for (k, v) in kwargs {
                                kw_map.insert(HashableKey::Str(k), v);
                            }
                            all_args.push(PyObject::dict(kw_map));
                            func(&all_args)?
                        } else {
                            func(&pos_args)?
                        };
                        // Check if asyncio.run() was invoked
                        if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
                            return self.maybe_await_result(coro);
                        }
                        return Ok(result);
                    }
                    PyObjectPayload::Partial { func: partial_func, args: partial_args, kwargs: partial_kwargs } => {
                        let partial_func = partial_func.clone();
                        let mut combined_args = partial_args.clone();
                        combined_args.extend(pos_args);
                        let mut combined_kw = partial_kwargs.clone();
                        combined_kw.extend(kwargs);
                        if combined_kw.is_empty() {
                            return self.call_object(partial_func, combined_args);
                        } else {
                            return self.call_object_kw(partial_func, combined_args, combined_kw);
                        }
                    }
                    PyObjectPayload::ExceptionType(kind) => {
                        let msg = if pos_args.is_empty() { String::new() } else { pos_args[0].py_to_string() };
                        return Ok(PyObject::exception_instance_with_args(kind.clone(), msg, pos_args));
                    }
                    PyObjectPayload::Instance(_) => {
                        if let Some(method) = func.get_attr("__call__") {
                            return self.call_object_kw(method, pos_args, kwargs);
                        }
                        return Err(PyException::type_error(format!(
                            "'{}' object is not callable", func.type_name()
                        )));
                    }
                    _ => {}
                }
                // Final fallback: merge kwargs into positional (lossy but functional)
                let mut all_args = pos_args;
                for (_, v) in kwargs {
                    all_args.push(v);
                }
                self.call_object(func, all_args)
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
                let code = pyfunc.code.clone();
                let globals = pyfunc.globals.clone();
                let defaults = pyfunc.defaults.clone();
                let kw_defaults = pyfunc.kw_defaults.clone();
                let closure = pyfunc.closure.clone();
                self.call_function(&code, args, &defaults, &kw_defaults, globals, &closure)
            }
            PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
                if name.as_str() == "__build_class__" {
                    return self.build_class(args);
                }
                // VM-aware builtins that need to call user-defined methods
                match name.as_str() {
                    "print" => {
                        let mut parts = Vec::new();
                        for a in &args {
                            parts.push(self.vm_str(a)?);
                        }
                        println!("{}", parts.join(" "));
                        return Ok(PyObject::none());
                    }
                    "str" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
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
                            // Create lazy map iterator
                            let source = builtins::get_iter_from_obj_pub(&args[1])?;
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                Arc::new(std::sync::Mutex::new(IteratorData::Map { func: func_obj, source }))
                            )));
                        } else {
                            // Multi-arg map: collect eagerly (rare case)
                            let mut iters: Vec<Vec<PyObjectRef>> = Vec::new();
                            for a in &args[1..] { iters.push(self.collect_iterable(a)?); }
                            let min_len = iters.iter().map(|v| v.len()).min().unwrap_or(0);
                            let mut result = Vec::new();
                            for i in 0..min_len {
                                let call_args: Vec<PyObjectRef> = iters.iter().map(|v| v[i].clone()).collect();
                                result.push(self.call_object(func_obj.clone(), call_args)?);
                            }
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                Arc::new(std::sync::Mutex::new(IteratorData::List { items: result, index: 0 }))
                            )));
                        }
                    }
                    "filter" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("filter() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        let source = builtins::get_iter_from_obj_pub(&args[1])?;
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                            Arc::new(std::sync::Mutex::new(IteratorData::Filter { func: func_obj, source }))
                        )));
                    }
                    "iter" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(iter_method) = args[0].get_attr("__iter__") {
                                    return self.call_object(iter_method, vec![]);
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
                            return Ok(PyObject::dict(IndexMap::new()));
                        }
                        // dict(iterable_of_pairs) or dict(mapping)
                        if let PyObjectPayload::Dict(_) = &args[0].payload {
                            return builtins::dispatch("dict", &args);
                        }
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
                            }
                        }
                    }
                    "issubclass" => {
                        if args.len() == 2 {
                            let sup = &args[1];
                            if let PyObjectPayload::Class(cd) = &sup.payload {
                                if let Some(ref metaclass) = cd.metaclass {
                                    if let Some(sc) = metaclass.get_attr("__subclasscheck__") {
                                        let result = self.call_object(sc, vec![sup.clone(), args[0].clone()])?;
                                        return Ok(PyObject::bool_val(result.is_truthy()));
                                    }
                                }
                            }
                        }
                    }
                    "min" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            if items.is_empty() {
                                return Err(PyException::value_error("min() arg is an empty sequence"));
                            }
                            let mut best = items[0].clone();
                            for item in &items[1..] {
                                if self.vm_lt(item, &best)? {
                                    best = item.clone();
                                }
                            }
                            return Ok(best);
                        }
                    }
                    "max" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            if items.is_empty() {
                                return Err(PyException::value_error("max() arg is an empty sequence"));
                            }
                            let mut best = items[0].clone();
                            for item in &items[1..] {
                                if self.vm_lt(&best, item)? {
                                    best = item.clone();
                                }
                            }
                            return Ok(best);
                        }
                    }
                    "reversed" => {
                        if !args.is_empty() {
                            // Check for __reversed__ dunder on instances
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(rev_method) = args[0].get_attr("__reversed__") {
                                    return self.call_object(rev_method, vec![]);
                                }
                            }
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("reversed", &[PyObject::list(items)]);
                        }
                    }
                    "enumerate" => {
                        return builtins::dispatch("enumerate", &args);
                    }
                    "zip" => {
                        return builtins::dispatch("zip", &args);
                    }
                    "len" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Dict subclass: use dict_storage length
                                if let Some(ref ds) = inst.dict_storage {
                                    return Ok(PyObject::int(ds.read().len() as i64));
                                }
                                if let Some(method) = args[0].get_attr("__len__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "abs" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__abs__") {
                                    let call_args = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) { vec![] } else { vec![args[0].clone()] };
                                    return self.call_object(method, call_args);
                                }

                            }
                        }
                    }
                    "hash" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__hash__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "format" => {
                        if !args.is_empty() {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__format__") {
                                    let spec = if args.len() > 1 {
                                        args[1].clone()
                                    } else {
                                        PyObject::str_val(CompactString::from(""))
                                    };
                                    let mut ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; ca.push(spec); return self.call_object(method, ca);
                                }
                            }
                            // Fall through to native format
                        }
                    }
                    "int" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__int__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "float" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__float__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "round" => {
                        if !args.is_empty() {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__round__") {
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
                                if let Some(method) = args[0].get_attr("__bytes__") {
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
                    "dir" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__dir__") {
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
                    "globals" => {
                        let frame = self.call_stack.last().unwrap();
                        let g = frame.globals.read();
                        let pairs: Vec<(PyObjectRef, PyObjectRef)> = g.iter()
                            .map(|(k, v)| (PyObject::str_val(CompactString::from(k.as_str())), v.clone()))
                            .collect();
                        drop(g);
                        return Ok(PyObject::dict_from_pairs(pairs));
                    }
                    "locals" => {
                        let frame = self.call_stack.last().unwrap();
                        let mut pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
                        for (i, name) in frame.code.varnames.iter().enumerate() {
                            if let Some(Some(val)) = frame.locals.get(i) {
                                pairs.push((PyObject::str_val(name.clone()), val.clone()));
                            }
                        }
                        for (i, name) in frame.code.cellvars.iter().chain(frame.code.freevars.iter()).enumerate() {
                            if let Some(cell) = frame.cells.get(i) {
                                let cell_val = cell.read();
                                if let Some(val) = cell_val.as_ref() {
                                    pairs.push((PyObject::str_val(name.clone()), val.clone()));
                                }
                            }
                        }
                        return Ok(PyObject::dict_from_pairs(pairs));
                    }
                    "setattr" => {
                        if args.len() != 3 {
                            return Err(PyException::type_error("setattr() takes exactly 3 arguments"));
                        }
                        let attr_name = args[1].py_to_string();
                        let value = args[2].clone();
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                                if let PyObjectPayload::Property { fset, .. } = &desc.payload {
                                    if let Some(setter) = fset {
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
                                        self.call_object(set_method, vec![desc, args[0].clone(), value])?;
                                        return Ok(PyObject::none());
                                    }
                                }
                            }
                            if let Some(sa) = lookup_in_class_mro(&inst.class, "__setattr__") {
                                if matches!(&sa.payload, PyObjectPayload::Function(_)) {
                                    let method = Arc::new(PyObject {
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
                                if let PyObjectPayload::Property { fdel, .. } = &desc.payload {
                                    if let Some(deleter) = fdel {
                                        self.call_object(deleter.clone(), vec![args[0].clone()])?;
                                        return Ok(PyObject::none());
                                    }
                                }
                            }
                        }
                        return builtins::dispatch("delattr", &args);
                    }
                    _ => {}
                }
                match builtins::get_builtin_fn(name.as_str()) {
                    Some(f) => f(&args),
                    None => Err(PyException::type_error(format!(
                        "'{}' is not callable", name
                    ))),
                }
            }
            PyObjectPayload::Class(cd) => {
                // If class has a metaclass with __call__, dispatch through it
                if let Some(meta) = &cd.metaclass {
                    if let Some(call_method) = meta.get_attr("__call__") {
                        let mut call_args = vec![func.clone()];
                        call_args.extend(args);
                        return self.call_object(call_method, call_args);
                    }
                }
                self.instantiate_class(&func, args, vec![])
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(args);
                self.call_object(method.clone(), bound_args)
            }
            PyObjectPayload::BuiltinBoundMethod { receiver, method_name } => {
                // ── Generator / Coroutine / AsyncGenerator dispatch ──
                // Extract gen_arc and discriminate the receiver kind for proper protocol.
                let gen_kind = match &receiver.payload {
                    PyObjectPayload::Generator(g) => Some(("generator", g.clone())),
                    PyObjectPayload::Coroutine(g) => Some(("coroutine", g.clone())),
                    PyObjectPayload::AsyncGenerator(g) => Some(("async_generator", g.clone())),
                    _ => None,
                };
                if let Some((kind, ref gen_arc)) = gen_kind {
                    match method_name.as_str() {
                        "send" => {
                            let val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return self.resume_generator(gen_arc, val);
                        }
                        "throw" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            return self.gen_throw(gen_arc, exc_kind, msg);
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
                            match self.gen_throw(gen_arc, ExceptionKind::GeneratorExit, String::new()) {
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
                        // ── Async generator protocol methods ──
                        // __aiter__ returns self (async generator is its own async iterator)
                        "__aiter__" if kind == "async_generator" => {
                            return Ok(receiver.clone());
                        }
                        // These return AsyncGenAwaitable objects, not direct results.
                        "__anext__" if kind == "async_generator" => {
                            return Ok(Arc::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: AsyncGenAction::Next,
                                }
                            }));
                        }
                        "asend" if kind == "async_generator" => {
                            let val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return Ok(Arc::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: AsyncGenAction::Send(val),
                                }
                            }));
                        }
                        "athrow" if kind == "async_generator" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            return Ok(Arc::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: AsyncGenAction::Throw(exc_kind, CompactString::from(msg)),
                                }
                            }));
                        }
                        "aclose" if kind == "async_generator" => {
                            return Ok(Arc::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: AsyncGenAction::Close,
                                }
                            }));
                        }
                        _ => {}
                    }
                }

                // ── AsyncGenAwaitable dispatch (driving the awaitable) ──
                if let PyObjectPayload::AsyncGenAwaitable { gen, action } = &receiver.payload {
                    match method_name.as_str() {
                        "send" => {
                            let send_val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return self.drive_async_gen_awaitable(gen, action, send_val);
                        }
                        "throw" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            return self.gen_throw(gen, exc_kind, msg);
                        }
                        "close" => {
                            return Ok(PyObject::none());
                        }
                        _ => {}
                    }
                }
                // VM-level methods that need iterable collection
                if method_name.as_str() == "join" {
                    if let PyObjectPayload::Str(sep) = &receiver.payload {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            let strs: Result<Vec<String>, _> = items.iter()
                                .map(|x| x.as_str().map(String::from).ok_or_else(||
                                    ferrython_core::error::PyException::type_error("sequence item: expected str")))
                                .collect();
                            return Ok(PyObject::str_val(CompactString::from(strs?.join(sep.as_str()))));
                        }
                    }
                }
                // VM-level list.sort with key function
                if method_name.as_str() == "sort" {
                    if let PyObjectPayload::List(items_arc) = &receiver.payload {
                        let items_arc = items_arc.clone();
                        let mut items_vec = items_arc.read().clone();
                        self.vm_sort(&mut items_vec)?;
                        *items_arc.write() = items_vec;
                        return Ok(PyObject::none());
                    }
                }
                // Property descriptor methods: setter/getter/deleter
                if let PyObjectPayload::Property { fget, fset, fdel } = &receiver.payload {
                    if args.len() == 1 {
                        let func = args[0].clone();
                        let new_prop = match method_name.as_str() {
                            "setter" => PyObjectPayload::Property { fget: fget.clone(), fset: Some(func), fdel: fdel.clone() },
                            "getter" => PyObjectPayload::Property { fget: Some(func), fset: fset.clone(), fdel: fdel.clone() },
                            "deleter" => PyObjectPayload::Property { fget: fget.clone(), fset: fset.clone(), fdel: Some(func) },
                            _ => return Err(PyException::attribute_error(format!("property has no attribute '{}'", method_name))),
                        };
                        return Ok(Arc::new(PyObject { payload: new_prop }));
                    }
                }
                // namedtuple methods — delegated to builtins
                if let PyObjectPayload::Instance(inst) = &receiver.payload {
                    if matches!(&inst.class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__namedtuple__"))
                        || inst.attrs.read().contains_key("__deque__")
                    {
                        // deque extend/extendleft need iterable collection via VM
                        if inst.attrs.read().contains_key("__deque__") && matches!(method_name.as_str(), "extend" | "extendleft") {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::call_method(receiver, method_name.as_str(), &[PyObject::list(items)]);
                        }
                        return builtins::call_method(receiver, method_name.as_str(), &args);
                    }
                    // Hashlib methods — delegated to builtins
                    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.to_string() } else { String::new() };
                    if matches!(class_name.as_str(), "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512") {
                        return builtins::call_method(receiver, method_name.as_str(), &args);
                    }
                }
                // Unbound method call: str.upper("hello") → call_method("hello", "upper", [])
                if let PyObjectPayload::BuiltinType(tn) = &receiver.payload {
                    // Class methods (e.g., int.from_bytes, dict.fromkeys)
                    if let Some(class_method) = builtins::resolve_type_class_method(tn, method_name) {
                        if let PyObjectPayload::NativeFunction { func, .. } = &class_method.payload {
                            return func(&args);
                        }
                    }
                    if !args.is_empty() {
                        let instance = args[0].clone();
                        let rest_args = if args.len() > 1 { args[1..].to_vec() } else { vec![] };
                        return builtins::call_method(&instance, method_name.as_str(), &rest_args);
                    }
                }
                // list.extend with generator/lazy iterator needs VM-level collection
                if method_name.as_str() == "extend" && !args.is_empty() {
                    if matches!(receiver.payload, PyObjectPayload::List(_)) {
                        if matches!(args[0].payload, PyObjectPayload::Generator(_)) ||
                           (matches!(&args[0].payload, PyObjectPayload::Iterator(ref d) if {
                               let data = d.lock().unwrap();
                               matches!(&*data, IteratorData::Enumerate { .. } | IteratorData::Zip { .. }
                                   | IteratorData::Map { .. } | IteratorData::Filter { .. }
                                   | IteratorData::Sentinel { .. })
                           }))
                        {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::call_method(receiver, "extend", &[PyObject::list(items)]);
                        }
                    }
                }
                builtins::call_method(receiver, method_name.as_str(), &args)
            }
            PyObjectPayload::ExceptionType(kind) => {
                // Calling an exception type creates an exception instance
                let msg = if args.is_empty() {
                    String::new()
                } else {
                    args[0].py_to_string()
                };
                Ok(PyObject::exception_instance_with_args(kind.clone(), msg, args))
            }
            PyObjectPayload::NativeFunction { func, name } => {
                // Intercept functions that need VM access to call Python callables
                if name.as_str() == "functools.reduce" {
                    return self.vm_functools_reduce(&args);
                }
                if name.as_str() == "itertools.islice" {
                    return self.vm_itertools_islice(&args);
                }
                // type.__call__(cls, *args) — standard class instantiation protocol
                if name.as_str() == "__type_call__" {
                    if args.is_empty() {
                        return Err(PyException::type_error("type.__call__ requires cls"));
                    }
                    let cls = args[0].clone();
                    let rest = args[1..].to_vec();
                    return self.instantiate_class(&cls, rest, vec![]);
                }
                // re.sub / re.subn with callable replacement
                if (name.as_str() == "re.sub" || name.as_str() == "re.subn") && args.len() >= 3 {
                    let repl = &args[1];
                    let is_callable = matches!(&repl.payload,
                        PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
                        | PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. }
                        | PyObjectPayload::Partial { .. });
                    if is_callable {
                        return self.re_sub_with_callable(&args, name.as_str() == "re.subn");
                    }
                }
                if name.as_str() == "itertools.groupby" {
                    let key_fn = args.last().and_then(|last| {
                        if let PyObjectPayload::Dict(map) = &last.payload {
                            let map = map.read();
                            map.get(&HashableKey::Str(CompactString::from("key"))).cloned()
                        } else { None }
                    });
                    let pos_args: Vec<_> = if key_fn.is_some() { args[..args.len()-1].to_vec() } else { args.clone() };
                    return self.vm_itertools_groupby(&pos_args, key_fn);
                }
                if name.as_str() == "itertools.filterfalse" && args.len() >= 2 {
                    return self.vm_itertools_filterfalse(&args);
                }
                if name.as_str() == "itertools.starmap" && args.len() >= 2 {
                    return self.vm_itertools_starmap(&args);
                }
                if name.as_str() == "itertools.accumulate" && args.len() >= 2 {
                    return self.vm_itertools_accumulate(&args);
                }
                // math.trunc / math.floor / math.ceil — dispatch to __trunc__ / __floor__ / __ceil__
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        let dunder = match name.as_str() {
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
                if name.as_str() == "os.fspath" && args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = args[0].get_attr("__fspath__") {
                            let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                            return self.call_object(method, ca);
                        }
                    }
                }
                func(&args)
            }
            PyObjectPayload::NativeClosure { func, .. } => {
                let result = func(&args)?;
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
            PyObjectPayload::Partial { func: partial_func, args: partial_args, kwargs: partial_kwargs } => {
                let partial_func = partial_func.clone();
                let mut combined_args = partial_args.clone();
                combined_args.extend(args);
                if !partial_kwargs.is_empty() {
                    let kw: Vec<(CompactString, PyObjectRef)> = partial_kwargs.iter()
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
                            let cache_key = HashableKey::Str(CompactString::from(&key_str));
                            // Check cache
                            if let Some(cached) = cache_map.read().get(&cache_key) {
                                // Cache hit: increment _hits counter
                                if let PyObjectPayload::Instance(ref d) = func.payload {
                                    let mut w = d.attrs.write();
                                    let hits = w.get(&CompactString::from("_hits"))
                                        .and_then(|v| v.as_int()).unwrap_or(0);
                                    w.insert(CompactString::from("_hits"), PyObject::int(hits + 1));
                                }
                                return Ok(cached.clone());
                            }
                            // Cache miss: call the wrapped function, increment _misses
                            if let PyObjectPayload::Instance(ref d) = func.payload {
                                let mut w = d.attrs.write();
                                let misses = w.get(&CompactString::from("_misses"))
                                    .and_then(|v| v.as_int()).unwrap_or(0);
                                w.insert(CompactString::from("_misses"), PyObject::int(misses + 1));
                            }
                            let result = self.call_object(wrapped, args)?;
                            cache_map.write().insert(cache_key, result.clone());
                            return Ok(result);
                        }
                    }
                }
                // Callable instances: check for __call__
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
}
