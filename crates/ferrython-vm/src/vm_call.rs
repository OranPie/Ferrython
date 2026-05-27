//! Function/method call dispatch, class instantiation, super().

use crate::builtins;
use crate::frame::{Frame, ScopeKind};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject};
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    has_descriptor_get, is_data_descriptor, lookup_in_class_mro, new_fx_hashkey_flatmap,
    new_fx_hashkey_map, AsyncGenAction, FxHashKeyMap, IteratorData, PartialData, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef, CLASS_FLAG_HAS_DESCRIPTORS,
    CLASS_FLAG_HAS_SETATTR, CLASS_FLAG_HAS_SLOTS,
};
use ferrython_core::types::{HashableKey, PyFunction, SharedConstantCache, SharedGlobals};
use indexmap::IndexMap;
use std::rc::Rc;

mod ast_nodes;
mod bytes_constructor;
mod class_inline;
mod class_instantiate;
mod exception_build;
mod exception_group;
mod frame_run;
mod frameless;
mod inline_simple;
mod iterator_state;
mod json_hooks;
mod locals;
mod print_format;
mod property_helpers;
mod sort_helpers;
mod str_fast;
mod super_object;

use class_inline::analyze_trivial_init;
use exception_build::build_builtin_exception_instance;
use frameless::{CallObjectDepthGuard, FRAMELESS_CALL_RECURSION_LIMIT};
use iterator_state::set_iterator_state;
use str_fast::fast_exact_str;

pub use exception_group::attach_eg_methods_pub;

impl VirtualMachine {
    #[inline]
    fn enter_frameless_call_dispatch(&self) -> PyResult<CallObjectDepthGuard> {
        let depth = Rc::clone(&self.call_object_depth);
        let next = depth.get().saturating_add(1);
        let raw_limit = ferrython_stdlib::get_recursion_limit();
        let configured_limit = if raw_limit > 0 {
            raw_limit as usize
        } else {
            self.recursion_limit
        };
        let limit = configured_limit.min(FRAMELESS_CALL_RECURSION_LIMIT);
        if next > limit {
            return Err(PyException::recursion_error(
                "maximum recursion depth exceeded",
            ));
        }
        depth.set(next);
        Ok(CallObjectDepthGuard { depth })
    }

    fn ast_class_name(cls: &PyObjectRef) -> Option<CompactString> {
        let PyObjectPayload::Class(cd) = &cls.payload else {
            return None;
        };
        if cd
            .namespace
            .read()
            .get("__ferrython_ast_node__")
            .map(|v| v.is_truthy())
            .unwrap_or(false)
        {
            return Some(cd.name.clone());
        }
        for base in &cd.mro {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                if bcd
                    .namespace
                    .read()
                    .get("__ferrython_ast_node__")
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    return Some(bcd.name.clone());
                }
            }
        }
        None
    }

    fn ast_class_fields(cls: &PyObjectRef) -> Vec<CompactString> {
        match cls.get_attr("_fields") {
            Some(fields) => match &fields.payload {
                PyObjectPayload::Tuple(items) => items
                    .iter()
                    .filter_map(|item| item.as_str().map(CompactString::from))
                    .collect(),
                _ => Vec::new(),
            },
            None => Vec::new(),
        }
    }

    fn ast_storage_name(name: &str) -> CompactString {
        if name == "n" || name == "s" {
            CompactString::from("value")
        } else {
            CompactString::from(name)
        }
    }

    pub(crate) fn property_isabstractmethod(
        &mut self,
        prop: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        for field in ["fget", "fset", "fdel"] {
            if let Some(func) = Self::property_callable_field(prop, field) {
                if let Some(flag) = func.get_attr("__isabstractmethod__") {
                    if self.vm_is_truthy(&flag)? {
                        return Ok(PyObject::bool_val(true));
                    }
                }
            }
        }
        Ok(PyObject::bool_val(false))
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
        let mut frame = Frame::new_from_pool(
            Rc::clone(code),
            globals,
            self.builtins.clone(),
            Rc::clone(constant_cache),
            &mut self.frame_pool,
        );
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
                    fname,
                    missing,
                    if missing == 1 { "" } else { "s" },
                    missing_names
                        .iter()
                        .map(|n| format!("'{}'", n))
                        .collect::<Vec<_>>()
                        .join(", ")
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
                fname,
                nparams,
                if nparams == 1 { "" } else { "s" },
                nargs,
                if nargs == 1 { "was" } else { "were" }
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
        let mut frame = Frame::new_from_pool(
            Rc::clone(code),
            globals,
            self.builtins.clone(),
            Rc::clone(constant_cache),
            &mut self.frame_pool,
        );
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
            Some(
                code.varnames
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (v.as_str(), i))
                    .collect(),
            )
        } else {
            None
        };
        for (name, val) in kwargs {
            let found_idx = if let Some(ref map) = varname_map {
                map.get(name.as_str()).copied()
            } else {
                code.varnames
                    .iter()
                    .position(|v| v.as_str() == name.as_str())
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
            extra_kwargs.insert(HashableKey::str_key(name), val);
        }
        if !has_varkw && !extra_kwargs.is_empty() {
            let unexpected = extra_kwargs
                .keys()
                .find_map(|key| match key {
                    HashableKey::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("?");
            return Err(PyException::type_error(format!(
                "{}() got an unexpected keyword argument '{}'",
                code.name, unexpected
            )));
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
        let ndefaults = defaults.len();
        let required = nparams - ndefaults;
        let missing_names: Vec<&str> = (0..required)
            .filter(|&i| frame.locals.get(i).map_or(true, |slot| slot.is_none()))
            .filter_map(|i| code.varnames.get(i).map(|s| s.as_str()))
            .collect();
        if !missing_names.is_empty() {
            return Err(PyException::type_error(format!(
                "{}() missing {} required positional argument{}: {}",
                code.name,
                missing_names.len(),
                if missing_names.len() == 1 { "" } else { "s" },
                missing_names
                    .iter()
                    .map(|n| format!("'{}'", n))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
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
                fname,
                nparams,
                if nparams == 1 { "" } else { "s" },
                npos,
                if npos == 1 { "was" } else { "were" }
            )));
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            frame.set_local(kwargs_idx, PyObject::dict(extra_kwargs));
        }

        self.install_closure_and_run(frame, code, closure)
    }

    pub(crate) fn call_object_one_arg_fast_or_fallback(
        &mut self,
        func: PyObjectRef,
        arg: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let pyfunc_ptr = match &func.payload {
            PyObjectPayload::Function(pyfunc)
                if pyfunc.is_simple
                    && pyfunc.code.arg_count == 1
                    && pyfunc.defaults.is_empty()
                    && pyfunc.kw_defaults.is_empty()
                    && pyfunc.closure.is_empty() =>
            {
                &**pyfunc as *const PyFunction
            }
            _ => return self.call_object(func, vec![arg]),
        };

        if ferrython_stdlib::is_trace_active() || ferrython_stdlib::is_profile_active() {
            return self.call_object(func, vec![arg]);
        }

        // `func` owns the payload behind this pointer and is moved into the borrowed
        // frame below, so the code/globals/constant cache stay alive while it runs.
        let pyfunc = unsafe { &*pyfunc_ptr };
        if let Some(result) = Self::try_inline_simple_function_one_arg(pyfunc, &arg) {
            return Ok(result);
        }

        let mut frame =
            unsafe { Frame::new_borrowed(pyfunc, func, &self.builtins, &mut self.frame_pool) };
        frame.locals[0] = Some(arg);
        frame.scope_kind = ScopeKind::Function;

        self.call_stack.push(frame);
        if self.call_stack.len() > self.recursion_limit {
            if let Some(frame) = self.call_stack.pop() {
                frame.recycle(&mut self.frame_pool);
            }
            return Err(PyException::recursion_error(
                "maximum recursion depth exceeded",
            ));
        }
        let result = self.run_frame();
        if let Some(frame) = self.call_stack.pop() {
            frame.recycle(&mut self.frame_pool);
        }
        result
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
                self.call_function_kw(
                    &pyfunc.code,
                    pos_args,
                    kwargs,
                    &pyfunc.defaults,
                    &pyfunc.kw_defaults,
                    globals,
                    &pyfunc.closure,
                    &pyfunc.constant_cache,
                )
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(pos_args);
                self.call_object_kw(method.clone(), bound_args, kwargs)
            }
            PyObjectPayload::Class(cd) => {
                if cd.name.as_str() == "weakref" && !kwargs.is_empty() {
                    return Err(PyException::type_error("ref() takes no keyword arguments"));
                }
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
                            let key_fn = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "key")
                                .map(|(_, v)| v.clone());
                            let reverse = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "reverse")
                                .map(|(_, v)| v.is_truthy())
                                .unwrap_or(false);
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
                                    let enc = kwargs
                                        .iter()
                                        .find(|(k, _)| k.as_str() == "encoding")
                                        .map(|(_, v)| v.clone())
                                        .unwrap_or_else(|| {
                                            PyObject::str_val(CompactString::from("utf-8"))
                                        });
                                    resolved.push(enc);
                                }
                                if resolved.len() < 2 {
                                    if let Some((_, v)) =
                                        kwargs.iter().find(|(k, _)| k.as_str() == "errors")
                                    {
                                        resolved.push(v.clone());
                                    }
                                }
                                return self.call_object(func, resolved);
                            }
                            _ => {
                                if matches!(
                                    &bbm.receiver.payload,
                                    PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                                ) && bbm.method_name.as_str() == "__init__"
                                {
                                    return Err(PyException::type_error(format!(
                                        "{}() takes no keyword arguments",
                                        bbm.method_name
                                    )));
                                }
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
                    PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
                        Some(name.clone())
                    }
                    _ => None,
                };
                if let Some(name) = builtin_name {
                    match name.as_str() {
                        "__build_class__" => {
                            return self.build_class_kw(pos_args, kwargs);
                        }
                        "sorted" => {
                            if !pos_args.is_empty() {
                                // Steal contents if list is temporary (refcount==1) — avoids clone
                                let mut items_vec = if let PyObjectPayload::List(ref cell) =
                                    pos_args[0].payload
                                {
                                    if PyObjectRef::strong_count(&pos_args[0]) == 1 {
                                        std::mem::take(&mut *cell.write())
                                    } else {
                                        cell.read().clone()
                                    }
                                } else if let PyObjectPayload::Tuple(ref t) = pos_args[0].payload {
                                    t.to_vec()
                                } else {
                                    self.collect_iterable(&pos_args[0])?
                                };
                                let key_fn = kwargs
                                    .iter()
                                    .find(|(k, _)| k.as_str() == "key")
                                    .map(|(_, v)| v.clone());
                                let reverse = kwargs
                                    .iter()
                                    .find(|(k, _)| k.as_str() == "reverse")
                                    .map(|(_, v)| v.is_truthy())
                                    .unwrap_or(false);
                                self.sort_with_key(&mut items_vec, key_fn, reverse)?;
                                return Ok(PyObject::list(items_vec));
                            }
                        }
                        "globals" => {
                            if let Some(frame) = self.call_stack.last() {
                                if let Some(globals_obj) = &frame.exec_globals {
                                    return Ok(globals_obj.clone());
                                }
                                let globals_arc = frame.globals.clone();
                                return Ok(PyObject::wrap(PyObjectPayload::InstanceDict(
                                    globals_arc,
                                )));
                            }
                            return Ok(PyObject::dict(new_fx_hashkey_map()));
                        }
                        "locals" => {
                            if let Some(frame) = self.call_stack.last() {
                                if let Some(locals) = &frame.exec_locals {
                                    return Ok(locals.clone());
                                }
                                if matches!(frame.scope_kind, ScopeKind::Module) {
                                    if let Some(globals_obj) = &frame.exec_globals {
                                        return Ok(globals_obj.clone());
                                    }
                                }
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
                            let sep = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "sep")
                                .map(|(_, v)| v.clone());
                            let end = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "end")
                                .map(|(_, v)| v.clone());
                            let file_obj = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "file")
                                .map(|(_, v)| v.clone());
                            let flush = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "flush")
                                .map(|(_, v)| v.is_truthy())
                                .unwrap_or(false);
                            return self.vm_print(&pos_args, sep, end, file_obj, flush);
                        }
                        "max" | "min" => {
                            let is_max = name.as_str() == "max";
                            let key_fn = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "key")
                                .map(|(_, v)| v.clone());
                            let default = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "default")
                                .map(|(_, v)| v.clone());
                            let items = if pos_args.len() == 1 {
                                self.collect_iterable(&pos_args[0])?
                            } else {
                                pos_args.clone()
                            };
                            return self.compute_min_max(
                                items,
                                is_max,
                                key_fn,
                                default,
                                name.as_str(),
                            );
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
                                    if let PyObjectPayload::MappingProxy(src) = &pos_args[0].payload
                                    {
                                        for (k, v) in src.read().iter() {
                                            map.insert(k.clone(), v.clone());
                                        }
                                        handled = true;
                                    }
                                }
                                // Check for InstanceDict payload
                                if !handled {
                                    if let PyObjectPayload::InstanceDict(src) = &pos_args[0].payload
                                    {
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
                            let start = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "start")
                                .map(|(_, v)| v.clone())
                                .unwrap_or_else(|| PyObject::int(0));
                            let mut all_args = pos_args;
                            all_args.push(start);
                            return self.call_object(func, all_args);
                        }
                        "int" => {
                            // int(x, base=N)
                            let mut all_args = pos_args;
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "base")
                            {
                                while all_args.len() < 1 {
                                    all_args.push(PyObject::int(0));
                                }
                                all_args.push(v.clone());
                            }
                            return self.call_object(func, all_args);
                        }
                        "bool" => {
                            // bool() doesn't accept kwargs
                            if !kwargs.is_empty() {
                                return Err(ferrython_core::error::PyException::type_error(
                                    compact_str::CompactString::from(
                                        "bool() takes no keyword arguments",
                                    ),
                                ));
                            }
                            if pos_args.len() > 1 {
                                return Err(ferrython_core::error::PyException::type_error(
                                    compact_str::CompactString::from(format!(
                                        "bool() takes at most 1 argument ({} given)",
                                        pos_args.len()
                                    )),
                                ));
                            }
                            if pos_args.is_empty() {
                                return Ok(PyObject::bool_val(false));
                            }
                            let obj = &pos_args[0];
                            if let ferrython_core::object::PyObjectPayload::Instance(inst) =
                                &obj.payload
                            {
                                if let Some(target_fn) =
                                    inst.attrs.read().get("__weakref_target__").cloned()
                                {
                                    let referent = self.call_object(target_fn, vec![])?;
                                    return Ok(PyObject::bool_val(self.vm_is_truthy(&referent)?));
                                }
                            }
                            // Instance with __bool__: call it and enforce return type == bool
                            if let ferrython_core::object::PyObjectPayload::Instance(_) =
                                &obj.payload
                            {
                                if let Some(raw_method) =
                                    Self::resolve_instance_dunder(obj, "__bool__")
                                {
                                    let method = self.resolve_descriptor(&raw_method, obj)?;
                                    let result = self.call_object(method, vec![])?;
                                    if !matches!(
                                        &result.payload,
                                        ferrython_core::object::PyObjectPayload::Bool(_)
                                    ) {
                                        let tn = result.type_name();
                                        return Err(
                                            ferrython_core::error::PyException::type_error(
                                                compact_str::CompactString::from(format!(
                                                    "__bool__ should return bool, returned {}",
                                                    tn
                                                )),
                                            ),
                                        );
                                    }
                                    return Ok(result);
                                }
                                if let Some(raw_method) =
                                    Self::resolve_instance_dunder(obj, "__len__")
                                {
                                    let method = self.resolve_descriptor(&raw_method, obj)?;
                                    let result = self.call_object(method, vec![])?;
                                    // __len__ must return non-negative int
                                    match &result.payload {
                                        ferrython_core::object::PyObjectPayload::Int(n) => {
                                            let is_neg = match n.to_i64() {
                                                Some(v) => v < 0,
                                                None => false, // bignum, rarely negative in practice
                                            };
                                            if is_neg {
                                                return Err(
                                                    ferrython_core::error::PyException::value_error(
                                                        compact_str::CompactString::from(
                                                            "__len__() should return >= 0",
                                                        ),
                                                    ),
                                                );
                                            }
                                            return Ok(PyObject::bool_val(!n.is_zero()));
                                        }
                                        ferrython_core::object::PyObjectPayload::Bool(b) => {
                                            return Ok(PyObject::bool_val(*b));
                                        }
                                        _ => {
                                            let tn = result.type_name();
                                            return Err(
                                                ferrython_core::error::PyException::type_error(
                                                    compact_str::CompactString::from(format!(
                                                        "__len__() should return >= 0, returned {}",
                                                        tn
                                                    )),
                                                ),
                                            );
                                        }
                                    }
                                }
                            }
                            return self.call_object(func, pos_args);
                        }
                        "float" | "str" | "bytes" | "bytearray" | "list" | "tuple" | "set"
                        | "frozenset" => {
                            // These builtins don't use kwargs meaningfully — just pass positional
                            return self.call_object(func, pos_args);
                        }
                        "complex" => {
                            // complex(real=, imag=) — resolve kwargs to positional
                            let mut real_arg: Option<PyObjectRef> = None;
                            let mut imag_arg: Option<PyObjectRef> = None;
                            for (k, v) in &kwargs {
                                match k.as_str() {
                                    "real" => real_arg = Some(v.clone()),
                                    "imag" => imag_arg = Some(v.clone()),
                                    _ => {
                                        return Err(PyException::type_error(format!(
                                            "'{}' is an invalid keyword argument for complex()",
                                            k
                                        )))
                                    }
                                }
                            }
                            let mut all_args = pos_args;
                            if let Some(r) = real_arg {
                                if all_args.is_empty() {
                                    all_args.push(r);
                                } else {
                                    return Err(PyException::type_error("argument for complex() given by name ('real') and position (1)"));
                                }
                            }
                            if let Some(i) = imag_arg {
                                while all_args.len() < 1 {
                                    all_args.push(PyObject::int(0));
                                }
                                if all_args.len() == 1 {
                                    all_args.push(i);
                                } else {
                                    return Err(PyException::type_error("argument for complex() given by name ('imag') and position (2)"));
                                }
                            }
                            return self.call_object(func, all_args);
                        }
                        "open" => {
                            // open(file, mode='r', buffering=-1, encoding=None, ...)
                            let mut all_args = pos_args;
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "mode")
                            {
                                while all_args.len() < 2 {
                                    all_args.push(PyObject::str_val(CompactString::from("r")));
                                }
                                all_args[1] = v.clone();
                            }
                            if let Some((_, v)) =
                                kwargs.iter().find(|(k, _)| k.as_str() == "encoding")
                            {
                                while all_args.len() < 4 {
                                    all_args.push(PyObject::none());
                                }
                                all_args[3] = v.clone();
                            }
                            return self.call_object(func, all_args);
                        }
                        "property" => {
                            let mut all_args = pos_args;
                            for (idx, key) in ["fget", "fset", "fdel", "doc"].iter().enumerate() {
                                if let Some((_, value)) =
                                    kwargs.iter().find(|(k, _)| k.as_str() == *key)
                                {
                                    while all_args.len() < idx {
                                        all_args.push(PyObject::none());
                                    }
                                    if all_args.len() == idx {
                                        all_args.push(value.clone());
                                    } else {
                                        all_args[idx] = value.clone();
                                    }
                                }
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
                                if matches!(&func.payload, PyObjectPayload::NativeFunction(nf)
                                    if nf.name.as_str() == "weakref.__new__")
                                {
                                    kw_map.insert(
                                        HashableKey::str_key(CompactString::from(
                                            "__weakref_ref_kwargs__",
                                        )),
                                        PyObject::bool_val(true),
                                    );
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
                        if nf_data.name.as_str() == "_ast.AST.__init__" {
                            if pos_args.is_empty() {
                                return Err(PyException::type_error("__init__ requires self"));
                            }
                            let instance = &pos_args[0];
                            let cls = match &instance.payload {
                                PyObjectPayload::Instance(inst) => inst.class.clone(),
                                _ => {
                                    return Err(PyException::type_error(
                                        "AST.__init__ requires an AST instance",
                                    ))
                                }
                            };
                            Self::populate_ast_node_attrs(instance, &cls, &pos_args[1..], &kwargs)?;
                            return Ok(PyObject::none());
                        }
                        if nf_data.name.as_str() == "_ast.AST.__new__" {
                            if pos_args.is_empty() {
                                return Err(PyException::type_error("__new__ requires cls"));
                            }
                            let cls = pos_args[0].clone();
                            let args = pos_args[1..].to_vec();
                            return Ok(self
                                .try_instantiate_ast_node(&cls, args, kwargs)?
                                .unwrap_or_else(|| PyObject::instance(cls)));
                        }
                        // property.__init__(self, fget=None, fset=None, fdel=None, doc=None)
                        if nf_data.name.as_str() == "property.__init__" {
                            if pos_args.is_empty() {
                                return Ok(PyObject::none());
                            }
                            Self::init_property_instance_attrs(
                                &pos_args[0],
                                &pos_args[1..],
                                &kwargs,
                            )?;
                            return Ok(PyObject::none());
                        }
                        // OrderedDict(**kwargs) / Counter(**kwargs) / defaultdict(factory, **kwargs) — dict-like init
                        if nf_data.name.as_str() == "collections.OrderedDict"
                            || nf_data.name.as_str() == "collections.Counter"
                        {
                            let mut map = IndexMap::new();
                            if !pos_args.is_empty() {
                                if let PyObjectPayload::Dict(src) = &pos_args[0].payload {
                                    for (k, v) in src.read().iter() {
                                        map.insert(k.clone(), v.clone());
                                    }
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
                                        for (k, v) in src.read().iter() {
                                            map.insert(k.clone(), v.clone());
                                        }
                                    }
                                }
                                for (k, v) in &kwargs {
                                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                                }
                                if all.len() >= 2 {
                                    all[1] = PyObject::dict(map);
                                } else {
                                    while all.len() < 1 {
                                        all.push(PyObject::none());
                                    }
                                    all.push(PyObject::dict(map));
                                }
                            }
                            return (nf_data.func)(&all);
                        }
                        if nf_data.name.as_str() == "collections.deque" {
                            // deque(iterable, maxlen=N)
                            let mut all = pos_args.clone();
                            if let Some((_, v)) =
                                kwargs.iter().find(|(k, _)| k.as_str() == "maxlen")
                            {
                                while all.len() < 1 {
                                    all.push(PyObject::list(vec![]));
                                }
                                if all.len() < 2 {
                                    all.push(v.clone());
                                } else {
                                    all[1] = v.clone();
                                }
                            }
                            return (nf_data.func)(&all);
                        }
                        if nf_data.name.as_str() == "WeakValueDictionary"
                            || nf_data.name.as_str() == "WeakKeyDictionary"
                        {
                            let instance = (nf_data.func)(&pos_args)?;
                            if !kwargs.is_empty() {
                                if let Some(update) = instance.get_attr("update") {
                                    self.call_object_kw(update, vec![], kwargs)?;
                                }
                            }
                            return Ok(instance);
                        }
                        if nf_data.name.as_str() == "functools.partial" {
                            // functools.partial(func, *args, **kwargs)
                            if pos_args.is_empty() {
                                return Err(PyException::type_error(
                                    "partial() requires at least 1 argument",
                                ));
                            }
                            let pf = pos_args[0].clone();
                            let pa = if pos_args.len() > 1 {
                                pos_args[1..].to_vec()
                            } else {
                                vec![]
                            };
                            return Ok(PyObject::wrap(PyObjectPayload::Partial(Box::new(
                                PartialData {
                                    func: pf,
                                    args: pa,
                                    kwargs,
                                },
                            ))));
                        }
                        // re.sub / re.subn with callable replacement
                        if (nf_data.name.as_str() == "re.sub" || nf_data.name.as_str() == "re.subn")
                            && pos_args.len() >= 3
                        {
                            let repl = &pos_args[1];
                            let is_callable = matches!(
                                &repl.payload,
                                PyObjectPayload::Function(_)
                                    | PyObjectPayload::BuiltinFunction(_)
                                    | PyObjectPayload::NativeFunction(_)
                                    | PyObjectPayload::NativeClosure(_)
                                    | PyObjectPayload::Partial(_)
                            );
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
                                return self.re_sub_with_callable(
                                    &merged,
                                    nf_data.name.as_str() == "re.subn",
                                );
                            }
                        }
                        // re.compile(pattern, flags=...) / re.match/search/findall/sub with flags kwarg
                        if nf_data.name.starts_with("re.") {
                            if let Some((_, flags_val)) =
                                kwargs.iter().find(|(k, _)| k.as_str() == "flags")
                            {
                                let mut all = pos_args.clone();
                                let flags_index = match nf_data.name.as_str() {
                                    "re.compile" => 1,
                                    "re.sub" | "re.subn" => 4,
                                    "re.split" => 3,
                                    _ => 2,
                                };
                                while all.len() <= flags_index {
                                    all.push(PyObject::int(0));
                                }
                                if matches!(nf_data.name.as_str(), "re.sub" | "re.subn") {
                                    if let Some((_, count_val)) =
                                        kwargs.iter().find(|(k, _)| k.as_str() == "count")
                                    {
                                        while all.len() <= 3 {
                                            all.push(PyObject::int(0));
                                        }
                                        all[3] = count_val.clone();
                                    }
                                } else if nf_data.name.as_str() == "re.split" {
                                    if let Some((_, maxsplit_val)) =
                                        kwargs.iter().find(|(k, _)| k.as_str() == "maxsplit")
                                    {
                                        while all.len() <= 2 {
                                            all.push(PyObject::int(0));
                                        }
                                        all[2] = maxsplit_val.clone();
                                    }
                                }
                                all[flags_index] = flags_val.clone();
                                return (nf_data.func)(&all);
                            }
                        }
                        // itertools.groupby with key function
                        if nf_data.name.as_str() == "itertools.groupby" && !pos_args.is_empty() {
                            let key_fn = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "key")
                                .map(|(_, v)| v.clone())
                                .or_else(|| {
                                    if pos_args.len() >= 2 {
                                        Some(pos_args[1].clone())
                                    } else {
                                        None
                                    }
                                });
                            let iterable = vec![pos_args[0].clone()];
                            return self.vm_itertools_groupby(&iterable, key_fn);
                        }
                        // itertools.accumulate with initial kwarg
                        if nf_data.name.as_str() == "itertools.accumulate"
                            && !kwargs.is_empty()
                            && !pos_args.is_empty()
                        {
                            let initial = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "initial")
                                .map(|(_, v)| v.clone());
                            let func_arg = if pos_args.len() >= 2
                                && !matches!(&pos_args[1].payload, PyObjectPayload::None)
                            {
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
                            if let Some((_, v)) =
                                kwargs.iter().find(|(k, _)| k.as_str() == "maxsplit")
                            {
                                while all.len() < 3 {
                                    all.push(PyObject::int(0));
                                }
                                all[2] = v.clone();
                            }
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "flags")
                            {
                                while all.len() < 4 {
                                    all.push(PyObject::int(0));
                                }
                                all[3] = v.clone();
                            }
                            return (nf_data.func)(&all);
                        }
                        // re.sub with count kwarg
                        if nf_data.name.as_str() == "re.sub" && !kwargs.is_empty() {
                            let mut all = pos_args.clone();
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "count")
                            {
                                while all.len() < 4 {
                                    all.push(PyObject::int(0));
                                }
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
                                matches!(
                                    k.as_str(),
                                    "object_hook"
                                        | "parse_float"
                                        | "parse_int"
                                        | "object_pairs_hook"
                                ) && matches!(
                                    &v.payload,
                                    PyObjectPayload::Function(_) | PyObjectPayload::Class(_)
                                )
                            });
                            if has_py_hook {
                                // Call native json.loads without hooks to get parsed data
                                let filtered_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs
                                    .iter()
                                    .filter(|(k, _)| {
                                        !matches!(
                                            k.as_str(),
                                            "object_hook"
                                                | "parse_float"
                                                | "parse_int"
                                                | "object_pairs_hook"
                                        )
                                    })
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
                                let object_hook = kwargs
                                    .iter()
                                    .find(|(k, _)| k.as_str() == "object_hook")
                                    .map(|(_, v)| v.clone());
                                let parse_float = kwargs
                                    .iter()
                                    .find(|(k, _)| k.as_str() == "parse_float")
                                    .map(|(_, v)| v.clone());
                                let parse_int = kwargs
                                    .iter()
                                    .find(|(k, _)| k.as_str() == "parse_int")
                                    .map(|(_, v)| v.clone());
                                return self.json_apply_hooks(
                                    &parsed,
                                    &object_hook,
                                    &parse_float,
                                    &parse_int,
                                );
                            }
                        }
                        // json.dumps / json.dump with `default` kwarg that may be a Python function
                        if (nf_data.name.as_str() == "json.dumps"
                            || nf_data.name.as_str() == "json.dump")
                            && !kwargs.is_empty()
                        {
                            let default_fn = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "default")
                                .map(|(_, v)| v.clone());
                            let cls_default = if default_fn.is_none() {
                                kwargs.iter().find(|(k, _)| k.as_str() == "cls").and_then(
                                    |(_, cls_val)| {
                                        // Create an encoder instance and bind its default method
                                        let encoder_inst = PyObject::instance(cls_val.clone());
                                        cls_val.get_attr("default").map(|method| {
                                            PyObject::wrap(PyObjectPayload::BoundMethod {
                                                receiver: encoder_inst,
                                                method,
                                            })
                                        })
                                    },
                                )
                            } else {
                                None
                            };
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
                                    let prepared =
                                        self.json_prepare_with_default(&pos_args[0], def)?;
                                    // Rebuild kwargs without `default` and `cls`
                                    let filtered_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs
                                        .into_iter()
                                        .filter(|(k, _)| {
                                            k.as_str() != "default" && k.as_str() != "cls"
                                        })
                                        .collect();
                                    if nf_data.name.as_str() == "json.dump" {
                                        // json.dump(obj, fp, **kwargs) → dump prepared obj to fp
                                        let mut dump_args = vec![prepared];
                                        if pos_args.len() > 1 {
                                            dump_args.push(pos_args[1].clone());
                                        }
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
                            if matches!(
                                nf_data.name.as_str(),
                                "weakref.__new__" | "weakref.__init__"
                            ) {
                                kw_map.insert(
                                    HashableKey::str_key(CompactString::from(
                                        "__weakref_ref_kwargs__",
                                    )),
                                    PyObject::bool_val(true),
                                );
                            }
                            all_args.push(PyObject::dict(kw_map));
                            return (nf_data.func)(&all_args);
                        }
                        return (nf_data.func)(&pos_args);
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        let mut counter_kw_marker = false;
                        let mut defaultdict_kw_marker = false;
                        let mut weakdict_kw_marker = false;
                        let mut finalize_kw_marker = false;
                        let mut adjusted_kwargs = kwargs;
                        if !adjusted_kwargs.is_empty() && nc.name.as_str().starts_with("Counter.") {
                            counter_kw_marker = true;
                            adjusted_kwargs.push((
                                CompactString::from("__counter_kwargs__"),
                                PyObject::bool_val(true),
                            ));
                        }
                        if !adjusted_kwargs.is_empty()
                            && nc.name.as_str().starts_with("defaultdict.")
                        {
                            defaultdict_kw_marker = true;
                            adjusted_kwargs.push((
                                CompactString::from("__defaultdict_kwargs__"),
                                PyObject::bool_val(true),
                            ));
                        }
                        if !adjusted_kwargs.is_empty()
                            && (nc.name.as_str() == "WeakValueDictionary.update"
                                || nc.name.as_str() == "WeakKeyDictionary.update")
                        {
                            weakdict_kw_marker = true;
                            adjusted_kwargs.push((
                                CompactString::from("__weakdict_kwargs__"),
                                PyObject::bool_val(true),
                            ));
                        }
                        if !adjusted_kwargs.is_empty()
                            && (nc.name.as_str() == "finalize"
                                || nc.name.as_str() == "finalize.__new__")
                        {
                            finalize_kw_marker = true;
                            adjusted_kwargs.push((
                                CompactString::from("__finalize_kwargs__"),
                                PyObject::bool_val(true),
                            ));
                        }
                        if !adjusted_kwargs.is_empty() && nc.name.as_str() == "weakref.__new__" {
                            adjusted_kwargs.push((
                                CompactString::from("__weakref_ref_kwargs__"),
                                PyObject::bool_val(true),
                            ));
                        }
                        let result = if !adjusted_kwargs.is_empty() {
                            let mut all_args = pos_args;
                            let mut kw_map = IndexMap::new();
                            for (k, v) in adjusted_kwargs {
                                kw_map.insert(HashableKey::str_key(k), v);
                            }
                            if counter_kw_marker {
                                kw_map.insert(
                                    HashableKey::str_key(CompactString::from("__counter_kwargs__")),
                                    PyObject::bool_val(true),
                                );
                            }
                            if defaultdict_kw_marker {
                                kw_map.insert(
                                    HashableKey::str_key(CompactString::from(
                                        "__defaultdict_kwargs__",
                                    )),
                                    PyObject::bool_val(true),
                                );
                            }
                            if weakdict_kw_marker {
                                kw_map.insert(
                                    HashableKey::str_key(CompactString::from(
                                        "__weakdict_kwargs__",
                                    )),
                                    PyObject::bool_val(true),
                                );
                            }
                            if finalize_kw_marker {
                                kw_map.insert(
                                    HashableKey::str_key(CompactString::from(
                                        "__finalize_kwargs__",
                                    )),
                                    PyObject::bool_val(true),
                                );
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
                        return build_builtin_exception_instance(*kind, pos_args, &kwargs);
                    }
                    PyObjectPayload::Instance(_) => {
                        if func.get_attr("__singledispatch__").is_some() {
                            return self.vm_singledispatch_call_instance(&func, &pos_args);
                        }
                        if let Some(method) = func.get_attr("__call__") {
                            let _dispatch_guard = self.enter_frameless_call_dispatch()?;
                            return self.call_object_kw(method, pos_args, kwargs);
                        }
                        return Err(PyException::type_error(format!(
                            "'{}' object is not callable",
                            func.type_name()
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
        let needs_current_frame = ferrython_stdlib::is_trace_active()
            || ferrython_stdlib::is_profile_active()
            || matches!(&func.payload, PyObjectPayload::NativeFunction(nf) if nf.name.as_str() == "sys._getframe");
        let prev_frame = if needs_current_frame {
            ferrython_stdlib::get_current_frame()
        } else {
            None
        };
        if needs_current_frame && !self.call_stack.is_empty() {
            ferrython_stdlib::set_current_frame(Some(self.make_trace_frame()));
        }
        let result = match &func.payload {
            PyObjectPayload::Function(pyfunc) => {
                // Borrow fields directly from the Arc-backed func instead of cloning
                // expensive Vec/IndexMap payloads. Only globals needs cloning (moved into frame).
                let globals = pyfunc.globals.clone();
                self.call_function(
                    &pyfunc.code,
                    args,
                    &pyfunc.defaults,
                    &pyfunc.kw_defaults,
                    globals,
                    &pyfunc.closure,
                    &pyfunc.constant_cache,
                )
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
                            if let Some(globals_obj) = &frame.exec_globals {
                                return Ok(globals_obj.clone());
                            }
                            let globals_arc = frame.globals.clone();
                            return Ok(PyObject::wrap(PyObjectPayload::InstanceDict(globals_arc)));
                        }
                        return Ok(PyObject::dict(new_fx_hashkey_map()));
                    }
                    "locals" => {
                        if let Some(frame) = self.call_stack.last() {
                            if let Some(locals) = &frame.exec_locals {
                                return Ok(locals.clone());
                            }
                            if matches!(frame.scope_kind, ScopeKind::Module) {
                                if let Some(globals_obj) = &frame.exec_globals {
                                    return Ok(globals_obj.clone());
                                }
                            }
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
                        return self.vm_print(&args, None, None, None, false);
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
                        if args.len() == 1 {
                            if let Some(result) = fast_exact_str(&args[0]) {
                                return Ok(result);
                            }
                        }
                        return self
                            .vm_str(&args[0])
                            .map(|s| PyObject::str_val(CompactString::from(s)));
                    }
                    "bytes" => {
                        return self.vm_bytes_constructor(&args, false);
                    }
                    "bytearray" => {
                        return self.vm_bytes_constructor(&args, true);
                    }
                    "repr" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        return self
                            .vm_repr(&args[0])
                            .map(|s| PyObject::str_val(CompactString::from(s)));
                    }
                    "map" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error(
                                "map() requires at least 2 arguments",
                            ));
                        }
                        let func_obj = args[0].clone();
                        let mut sources = Vec::with_capacity(args.len() - 1);
                        for a in &args[1..] {
                            sources.push(self.resolve_iterable(a)?);
                        }
                        if sources.len() == 1 {
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                PyCell::new(IteratorData::MapOne {
                                    func: func_obj,
                                    source: sources.pop().unwrap(),
                                }),
                            ))));
                        }
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                            PyCell::new(IteratorData::Map {
                                func: func_obj,
                                sources,
                            }),
                        ))));
                    }
                    "filter" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error(
                                "filter() requires at least 2 arguments",
                            ));
                        }
                        let func_obj = args[0].clone();
                        let source = self.resolve_iterable(&args[1])?;
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                            PyCell::new(IteratorData::Filter {
                                func: func_obj,
                                source,
                            }),
                        ))));
                    }
                    "iter" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                if let Some(raw_iter) =
                                    Self::resolve_instance_dunder(&args[0], "__iter__")
                                {
                                    let iter_method =
                                        self.resolve_descriptor(&raw_iter, &args[0])?;
                                    let r = self.call_object(iter_method, vec![])?;
                                    return Self::ensure_iterator_result(&args[0], r);
                                }
                                if inst.dict_storage.is_some() {
                                    return args[0].get_iter();
                                }
                                // Builtin base type subclass: delegate to __builtin_value__
                                if let Some(bv) = Self::get_builtin_value(&args[0]) {
                                    let iter = self.resolve_iterable(&bv)?;
                                    return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                        PyCell::new(IteratorData::HeldIter {
                                            iter,
                                            owner: Some(args[0].clone()),
                                        }),
                                    ))));
                                }
                                // Old-style sequence protocol: lazy SeqIter
                                if args[0].get_attr("__getitem__").is_some() {
                                    return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                        PyCell::new(IteratorData::SeqIter {
                                            obj: args[0].clone(),
                                            index: 0,
                                            exhausted: false,
                                        }),
                                    ))));
                                }
                                return Err(PyException::type_error(format!(
                                    "'{}' object is not iterable",
                                    args[0].type_name()
                                )));
                            }
                            // Fall through to builtin dispatch for non-instances
                        }
                    }
                    "next" => {
                        if args.is_empty() {
                            return Err(PyException::type_error(
                                "next() requires at least 1 argument",
                            ));
                        }
                        // For generators, resume directly so StopIteration return value propagates
                        if let PyObjectPayload::Generator(gen_arc) = &args[0].payload {
                            match self.resume_generator(gen_arc, PyObject::none()) {
                                Ok(value) => return Ok(value),
                                Err(e)
                                    if e.kind == ExceptionKind::StopIteration && args.len() > 1 =>
                                {
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
                            return Err(PyException::type_error(
                                "sum() requires at least 1 argument",
                            ));
                        }
                        let start = if args.len() > 1 {
                            args[1].clone()
                        } else {
                            PyObject::int(0)
                        };
                        let mut total = start;
                        // Inline lazy iteration — avoid materializing entire iterable
                        match &args[0].payload {
                            PyObjectPayload::List(cell) => {
                                let items = cell.read();
                                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(
                                    s,
                                )) = &total.payload
                                {
                                    let mut acc: i64 = *s;
                                    let mut fallback_idx = items.len();
                                    for (i, item) in items.iter().enumerate() {
                                        if let PyObjectPayload::Int(
                                            ferrython_core::types::PyInt::Small(n),
                                        ) = &item.payload
                                        {
                                            acc = acc.wrapping_add(*n);
                                        } else {
                                            total = PyObject::int(acc);
                                            total = self.vm_add(&total, item)?;
                                            fallback_idx = i + 1;
                                            break;
                                        }
                                    }
                                    if fallback_idx < items.len() {
                                        for item in &items[fallback_idx..] {
                                            total = self.vm_add(&total, item)?;
                                        }
                                    } else {
                                        total = PyObject::int(acc);
                                    }
                                } else {
                                    for item in items.iter() {
                                        total = self.vm_add(&total, item)?;
                                    }
                                }
                            }
                            PyObjectPayload::Tuple(items) => {
                                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(
                                    s,
                                )) = &total.payload
                                {
                                    let mut acc: i64 = *s;
                                    let mut fallback_idx = items.len();
                                    for (i, item) in items.iter().enumerate() {
                                        if let PyObjectPayload::Int(
                                            ferrython_core::types::PyInt::Small(n),
                                        ) = &item.payload
                                        {
                                            acc = acc.wrapping_add(*n);
                                        } else {
                                            total = PyObject::int(acc);
                                            total = self.vm_add(&total, item)?;
                                            fallback_idx = i + 1;
                                            break;
                                        }
                                    }
                                    if fallback_idx < items.len() {
                                        for item in &items[fallback_idx..] {
                                            total = self.vm_add(&total, item)?;
                                        }
                                    } else {
                                        total = PyObject::int(acc);
                                    }
                                } else {
                                    for item in items.iter() {
                                        total = self.vm_add(&total, item)?;
                                    }
                                }
                            }
                            PyObjectPayload::Range(rd) => {
                                // O(1) arithmetic sum for integer ranges
                                let (s, e, st) = (rd.start, rd.stop, rd.step);
                                let n = if st > 0 {
                                    if e > s {
                                        (e - s - 1) / st + 1
                                    } else {
                                        0
                                    }
                                } else if st < 0 {
                                    if s > e {
                                        (s - e - 1) / (-st) + 1
                                    } else {
                                        0
                                    }
                                } else {
                                    0
                                };
                                if n > 0 {
                                    // Gauss: sum = n*start + step*n*(n-1)/2
                                    let range_sum = n
                                        .wrapping_mul(s)
                                        .wrapping_add(st.wrapping_mul(n).wrapping_mul(n - 1) / 2);
                                    total = self.vm_add(&total, &PyObject::int(range_sum))?;
                                }
                            }
                            PyObjectPayload::RangeIter(ri) => {
                                // O(1) arithmetic sum for range iterators
                                let c = ri.current.get();
                                let s = ri.stop;
                                let st = ri.step;
                                let n = if st > 0 {
                                    if s > c {
                                        (s - c - 1) / st + 1
                                    } else {
                                        0
                                    }
                                } else if st < 0 {
                                    if c > s {
                                        (c - s - 1) / (-st) + 1
                                    } else {
                                        0
                                    }
                                } else {
                                    0
                                };
                                if n > 0 {
                                    let range_sum = n
                                        .wrapping_mul(c)
                                        .wrapping_add(st.wrapping_mul(n).wrapping_mul(n - 1) / 2);
                                    total = self.vm_add(&total, &PyObject::int(range_sum))?;
                                    ri.current.set(c + st * n); // advance iterator to exhaustion
                                }
                            }
                            PyObjectPayload::Iterator(_) => {
                                let items = self.collect_iterable(&args[0])?;
                                // Native i64 accumulation for homogeneous int iterators
                                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(
                                    s,
                                )) = &total.payload
                                {
                                    let mut acc: i64 = *s;
                                    let mut fallback_idx = items.len();
                                    for (i, item) in items.iter().enumerate() {
                                        if let PyObjectPayload::Int(
                                            ferrython_core::types::PyInt::Small(n),
                                        ) = &item.payload
                                        {
                                            acc = acc.wrapping_add(*n);
                                        } else {
                                            total = PyObject::int(acc);
                                            total = self.vm_add(&total, &item)?;
                                            fallback_idx = i + 1;
                                            break;
                                        }
                                    }
                                    if fallback_idx < items.len() {
                                        for item in &items[fallback_idx..] {
                                            total = self.vm_add(&total, &item)?;
                                        }
                                    } else {
                                        total = PyObject::int(acc);
                                    }
                                } else {
                                    for item in items {
                                        total = self.vm_add(&total, &item)?;
                                    }
                                }
                            }
                            PyObjectPayload::Generator(gen_arc) => {
                                let gen_arc = gen_arc.clone();
                                // Native i64 accumulation for homogeneous int generators
                                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(
                                    s,
                                )) = &total.payload
                                {
                                    let mut acc: i64 = *s;
                                    let mut use_native = true;
                                    loop {
                                        match self.resume_generator_for_iter(&gen_arc) {
                                            Ok(Some(item)) => {
                                                if let PyObjectPayload::Int(
                                                    ferrython_core::types::PyInt::Small(n),
                                                ) = &item.payload
                                                {
                                                    acc = acc.wrapping_add(*n);
                                                } else {
                                                    // Switch to generic accumulation
                                                    total = PyObject::int(acc);
                                                    total = self.vm_add(&total, &item)?;
                                                    use_native = false;
                                                    break;
                                                }
                                            }
                                            Ok(None) => break,
                                            Err(e) => return Err(e),
                                        }
                                    }
                                    if use_native {
                                        return Ok(PyObject::int(acc));
                                    }
                                    // Fell through — continue with generic total
                                    loop {
                                        match self.resume_generator_for_iter(&gen_arc) {
                                            Ok(Some(item)) => {
                                                total = self.vm_add(&total, &item)?;
                                            }
                                            Ok(None) => break,
                                            Err(e) => return Err(e),
                                        }
                                    }
                                } else {
                                    loop {
                                        match self.resume_generator_for_iter(&gen_arc) {
                                            Ok(Some(item)) => {
                                                total = self.vm_add(&total, &item)?;
                                            }
                                            Ok(None) => break,
                                            Err(e) => return Err(e),
                                        }
                                    }
                                }
                            }
                            _ => {
                                let items = self.collect_iterable(&args[0])?;
                                for item in items {
                                    total = self.vm_add(&total, &item)?;
                                }
                            }
                        }
                        return Ok(total);
                    }
                    "sorted" => {
                        if !args.is_empty() {
                            // Steal contents if list is temporary (refcount==1) — avoids clone
                            let mut items = if let PyObjectPayload::List(ref cell) = args[0].payload
                            {
                                if PyObjectRef::strong_count(&args[0]) == 1 {
                                    std::mem::take(&mut *cell.write())
                                } else {
                                    cell.read().clone()
                                }
                            } else if let PyObjectPayload::Tuple(ref t) = args[0].payload {
                                t.to_vec()
                            } else {
                                self.collect_iterable(&args[0])?
                            };
                            self.vm_sort(&mut items)?;
                            return Ok(PyObject::list(items));
                        }
                    }
                    "set" => {
                        if args.len() > 1 {
                            return builtins::dispatch("set", &args);
                        }
                        if args.is_empty() {
                            return builtins::dispatch("set", &[]);
                        }
                        if let PyObjectPayload::Dict(items) = &args[0].payload {
                            let read = items.read();
                            let mut map = new_fx_hashkey_flatmap();
                            map.reserve(read.len());
                            for key in read.keys() {
                                map.insert(key.clone(), key.to_object());
                            }
                            return Ok(PyObject::set_from_flatmap(map));
                        }
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::dispatch("set", &[PyObject::list(items)]);
                    }
                    "frozenset" => {
                        if args.len() > 1 {
                            return builtins::dispatch("frozenset", &args);
                        }
                        if args.is_empty() {
                            return builtins::dispatch("frozenset", &[]);
                        }
                        if let PyObjectPayload::Dict(items) = &args[0].payload {
                            let read = items.read();
                            let mut map = new_fx_hashkey_map();
                            for key in read.keys() {
                                map.insert(key.clone(), key.to_object());
                            }
                            return Ok(PyObject::frozenset(map));
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
                            if let Some(keys_method) = args[0].get_attr("keys") {
                                let keys_obj = self.call_object(keys_method, vec![])?;
                                let keys = self.collect_iterable(&keys_obj)?;
                                let mut map = IndexMap::new();
                                for key_obj in keys {
                                    let value = args[0].get_item(&key_obj)?;
                                    map.insert(key_obj.to_hashable_key()?, value);
                                }
                                return Ok(PyObject::dict(map));
                            }
                            if inst.attrs.read().contains_key("__chainmap__") {
                                if let Some(items_method) = args[0].get_attr("items") {
                                    let items_obj = self.call_object(items_method, vec![])?;
                                    let items = self.collect_iterable(&items_obj)?;
                                    let mut map = IndexMap::new();
                                    for item in &items {
                                        let kv = item.to_list()?;
                                        if kv.len() == 2 {
                                            let key = kv[0].to_hashable_key()?;
                                            map.insert(key, kv[1].clone());
                                        }
                                    }
                                    return Ok(PyObject::dict(map));
                                }
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
                                    Some(item) => {
                                        if item.is_truthy() {
                                            return Ok(PyObject::bool_val(true));
                                        }
                                    }
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
                                    Some(item) => {
                                        if !item.is_truthy() {
                                            return Ok(PyObject::bool_val(false));
                                        }
                                    }
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
                                        let result = self
                                            .call_object(ic, vec![cls.clone(), args[0].clone()])?;
                                        return Ok(PyObject::bool_val(result.is_truthy()));
                                    }
                                }
                                // Check __subclasshook__ on the class (ABC protocol)
                                if let Some(hook) = cls.get_attr("__subclasshook__") {
                                    // Pass the type of the object being checked
                                    let obj = &args[0];
                                    let obj_type = match &obj.payload {
                                        PyObjectPayload::Instance(inst) => inst.class.clone(),
                                        _ => PyObject::builtin_type(CompactString::from(
                                            obj.type_name(),
                                        )),
                                    };
                                    if let Ok(result) = self.call_object(hook, vec![obj_type]) {
                                        if !matches!(
                                            &result.payload,
                                            PyObjectPayload::NotImplemented
                                        ) {
                                            return Ok(PyObject::bool_val(result.is_truthy()));
                                        }
                                    }
                                }
                                // Check for runtime_checkable Protocol — structural subtyping
                                let ns = cd.namespace.read();
                                if ns
                                    .get("_is_runtime_checkable")
                                    .map_or(false, |v| v.is_truthy())
                                {
                                    if let Some(protocol_attrs) = ns.get("__protocol_attrs__") {
                                        if let PyObjectPayload::Tuple(required) =
                                            &protocol_attrs.payload
                                        {
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
                                        let result = self
                                            .call_object(sc, vec![sup.clone(), args[0].clone()])?;
                                        return Ok(PyObject::bool_val(result.is_truthy()));
                                    }
                                }
                                // Check __subclasshook__ on the superclass (ABC protocol)
                                if let Some(hook) = sup.get_attr("__subclasshook__") {
                                    if let Ok(result) =
                                        self.call_object(hook, vec![args[0].clone()])
                                    {
                                        if !matches!(
                                            &result.payload,
                                            PyObjectPayload::NotImplemented
                                        ) {
                                            return Ok(PyObject::bool_val(result.is_truthy()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "min" => {
                        if args.len() == 1 {
                            if let Some(r) = self.native_min_max_list(&args[0], false)? {
                                return Ok(r);
                            }
                            let items = self.collect_iterable(&args[0])?;
                            return self.compute_min_max(items, false, None, None, "min");
                        }
                    }
                    "max" => {
                        if args.len() == 1 {
                            if let Some(r) = self.native_min_max_list(&args[0], true)? {
                                return Ok(r);
                            }
                            let items = self.collect_iterable(&args[0])?;
                            return self.compute_min_max(items, true, None, None, "max");
                        }
                    }
                    "reversed" => {
                        if !args.is_empty() {
                            if matches!(&args[0].payload, PyObjectPayload::List(_)) {
                                return builtins::dispatch("reversed", &[args[0].clone()]);
                            }
                            // Check for __reversed__ dunder on instances
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(rev_method) =
                                    Self::resolve_instance_dunder(&args[0], "__reversed__")
                                {
                                    return self.call_object(rev_method, vec![]);
                                }
                                if let Some(bv) = Self::get_builtin_value(&args[0]) {
                                    let items = self.collect_iterable(&bv)?;
                                    let iter =
                                        builtins::dispatch("reversed", &[PyObject::list(items)])?;
                                    return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                        PyCell::new(IteratorData::HeldIter {
                                            iter,
                                            owner: Some(args[0].clone()),
                                        }),
                                    ))));
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
                                if let Some(v) =
                                    r.get(&HashableKey::str_key(CompactString::from("strict")))
                                {
                                    strict = v.is_truthy();
                                }
                                drop(r);
                                args.len() - 1
                            } else {
                                args.len()
                            }
                        } else {
                            args.len()
                        };
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
                                if inst.attrs.read().contains_key("__chainmap__") {
                                    if let Some(method) = args[0].get_attr("__len__") {
                                        let result = self.call_object(method, vec![])?;
                                        return Ok(result);
                                    }
                                }
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
                                    if !matches!(
                                        &method.payload,
                                        PyObjectPayload::BuiltinBoundMethod(_)
                                    ) {
                                        let ca = if matches!(
                                            &method.payload,
                                            PyObjectPayload::BoundMethod { .. }
                                        ) {
                                            vec![]
                                        } else {
                                            vec![args[0].clone()]
                                        };
                                        return self.call_object(method, ca);
                                    }
                                }
                                // Builtin base type subclass (list, tuple, etc.)
                                if let Some(bv) =
                                    inst.attrs.read().get("__builtin_value__").cloned()
                                {
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
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__abs__")
                                {
                                    let call_args = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
                                    return self.call_object(method, call_args);
                                }
                            }
                        }
                    }
                    "hash" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__hash__")
                                {
                                    let ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
                                    return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "bin" | "oct" | "hex" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__index__")
                                {
                                    let ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
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
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__format__")
                                {
                                    let spec = if args.len() > 1 {
                                        args[1].clone()
                                    } else {
                                        PyObject::str_val(CompactString::from(""))
                                    };
                                    let mut ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
                                    ca.push(spec);
                                    return self.call_object(method, ca);
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
                    "complex" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Check for user-defined __complex__ FIRST (takes priority over __builtin_value__)
                                let has_user_complex = inst.class.get_attr("__complex__").is_some()
                                    && {
                                        // Distinguish user-defined from inherited builtin
                                        let m =
                                            Self::resolve_instance_dunder(&args[0], "__complex__");
                                        matches!(
                                            m.as_ref().map(|o| &o.payload),
                                            Some(
                                                PyObjectPayload::BoundMethod { .. }
                                                    | PyObjectPayload::Function(_)
                                            )
                                        )
                                    };
                                if has_user_complex {
                                    if let Some(method) =
                                        Self::resolve_instance_dunder(&args[0], "__complex__")
                                    {
                                        let ca = if matches!(
                                            &method.payload,
                                            PyObjectPayload::BoundMethod { .. }
                                        ) {
                                            vec![]
                                        } else {
                                            vec![args[0].clone()]
                                        };
                                        let result = self.call_object(method, ca)?;
                                        match &result.payload {
                                            PyObjectPayload::Complex { .. } => return Ok(result),
                                            PyObjectPayload::Instance(i2) => {
                                                // subclass of complex — extract via __builtin_value__
                                                if let Some(v) = i2
                                                    .attrs
                                                    .read()
                                                    .get("__builtin_value__")
                                                    .cloned()
                                                {
                                                    if matches!(
                                                        &v.payload,
                                                        PyObjectPayload::Complex { .. }
                                                    ) {
                                                        return Ok(v);
                                                    }
                                                }
                                                return Err(PyException::type_error(format!(
                                                    "__complex__ returned non-complex (type {})",
                                                    result.type_name()
                                                )));
                                            }
                                            _ => {
                                                return Err(PyException::type_error(format!(
                                                    "__complex__ returned non-complex (type {})",
                                                    result.type_name()
                                                )))
                                            }
                                        }
                                    }
                                }
                                if let Some(val) =
                                    inst.attrs.read().get("__builtin_value__").cloned()
                                {
                                    if matches!(&val.payload, PyObjectPayload::Complex { .. }) {
                                        return Ok(val);
                                    }
                                }
                                // Fallback: __float__
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__float__")
                                {
                                    let ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
                                    let result = self.call_object(method, ca)?;
                                    match &result.payload {
                                        PyObjectPayload::Float(f) => {
                                            return Ok(PyObject::complex(*f, 0.0))
                                        }
                                        PyObjectPayload::Int(n) => {
                                            return Ok(PyObject::complex(n.to_f64(), 0.0))
                                        }
                                        PyObjectPayload::Bool(b) => {
                                            return Ok(PyObject::complex(
                                                if *b { 1.0 } else { 0.0 },
                                                0.0,
                                            ))
                                        }
                                        _ => {
                                            return Err(PyException::type_error(format!(
                                                "__float__ returned non-float (type {})",
                                                result.type_name()
                                            )))
                                        }
                                    }
                                }
                                // Fallback: __index__
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__index__")
                                {
                                    let ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
                                    let result = self.call_object(method, ca)?;
                                    match &result.payload {
                                        PyObjectPayload::Int(n) => {
                                            let f = n.to_f64();
                                            if f.is_infinite() {
                                                return Err(PyException::overflow_error(
                                                    "int too large to convert to float",
                                                ));
                                            }
                                            return Ok(PyObject::complex(f, 0.0));
                                        }
                                        PyObjectPayload::Bool(b) => {
                                            return Ok(PyObject::complex(
                                                if *b { 1.0 } else { 0.0 },
                                                0.0,
                                            ))
                                        }
                                        _ => {
                                            return Err(PyException::type_error(format!(
                                                "__index__ returned non-int (type {})",
                                                result.type_name()
                                            )))
                                        }
                                    }
                                }
                                return Err(PyException::type_error(
                                    format!("complex() first argument must be a string or a number, not '{}'", args[0].type_name())));
                            }
                        } else if args.len() == 2 {
                            // Handle instances as either arg via __float__/__index__/__complex__
                            let coerce_for_complex =
                                |vm: &mut Self,
                                 obj: &PyObjectRef,
                                 which: &str|
                                 -> PyResult<PyObjectRef> {
                                    if matches!(
                                        &obj.payload,
                                        PyObjectPayload::Complex { .. }
                                            | PyObjectPayload::Int(_)
                                            | PyObjectPayload::Float(_)
                                            | PyObjectPayload::Bool(_)
                                    ) {
                                        return Ok(obj.clone());
                                    }
                                    if let PyObjectPayload::Instance(inst) = &obj.payload {
                                        if let Some(val) =
                                            inst.attrs.read().get("__builtin_value__").cloned()
                                        {
                                            if matches!(
                                                &val.payload,
                                                PyObjectPayload::Complex { .. }
                                                    | PyObjectPayload::Int(_)
                                                    | PyObjectPayload::Float(_)
                                            ) {
                                                return Ok(val);
                                            }
                                        }
                                        for dunder in &["__complex__", "__float__", "__index__"] {
                                            if let Some(method) =
                                                Self::resolve_instance_dunder(obj, dunder)
                                            {
                                                let ca = if matches!(
                                                    &method.payload,
                                                    PyObjectPayload::BoundMethod { .. }
                                                ) {
                                                    vec![]
                                                } else {
                                                    vec![obj.clone()]
                                                };
                                                let res = vm.call_object(method, ca)?;
                                                if matches!(
                                                    &res.payload,
                                                    PyObjectPayload::Complex { .. }
                                                        | PyObjectPayload::Int(_)
                                                        | PyObjectPayload::Float(_)
                                                        | PyObjectPayload::Bool(_)
                                                ) {
                                                    return Ok(res);
                                                }
                                            }
                                        }
                                    }
                                    Err(PyException::type_error(format!(
                                        "complex() {} argument must be a number, not '{}'",
                                        which,
                                        obj.type_name()
                                    )))
                                };
                            let has_inst = matches!(&args[0].payload, PyObjectPayload::Instance(_))
                                || matches!(&args[1].payload, PyObjectPayload::Instance(_));
                            if has_inst {
                                let which_first =
                                    if matches!(&args[0].payload, PyObjectPayload::Str(_)) {
                                        ""
                                    } else {
                                        "first"
                                    };
                                let which_second = "second";
                                let a = coerce_for_complex(
                                    self,
                                    &args[0],
                                    if which_first.is_empty() {
                                        "first"
                                    } else {
                                        which_first
                                    },
                                )?;
                                let b = coerce_for_complex(self, &args[1], which_second)?;
                                return crate::builtins::core_fns::builtin_complex(&[a, b]);
                            }
                        }
                    }
                    "int" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Check for __builtin_value__ first (int subclass)
                                if let Some(val) =
                                    inst.attrs.read().get("__builtin_value__").cloned()
                                {
                                    if matches!(&val.payload, PyObjectPayload::Int(_)) {
                                        return Ok(val);
                                    }
                                }
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__int__")
                                {
                                    let ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
                                    return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "float" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Check for __builtin_value__ first (float subclass)
                                if let Some(val) =
                                    inst.attrs.read().get("__builtin_value__").cloned()
                                {
                                    if matches!(&val.payload, PyObjectPayload::Float(_)) {
                                        return Ok(val);
                                    }
                                    // int subclass → convert to float
                                    if let PyObjectPayload::Int(n) = &val.payload {
                                        return Ok(PyObject::float(n.to_f64()));
                                    }
                                }
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__float__")
                                {
                                    let ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
                                    return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "round" => {
                        if !args.is_empty() {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__round__")
                                {
                                    let mut ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
                                    if args.len() >= 2 {
                                        ca.push(args[1].clone());
                                    }
                                    return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "bool" => {
                        if args.len() == 1 {
                            let obj = &args[0];
                            if let ferrython_core::object::PyObjectPayload::Instance(inst) =
                                &obj.payload
                            {
                                if let Some(target_fn) =
                                    inst.attrs.read().get("__weakref_target__").cloned()
                                {
                                    let referent = self.call_object(target_fn, vec![])?;
                                    return Ok(PyObject::bool_val(self.vm_is_truthy(&referent)?));
                                }
                            }
                            // Instance with __bool__: call it and enforce return type == bool
                            if let ferrython_core::object::PyObjectPayload::Instance(_) =
                                &obj.payload
                            {
                                if let Some(raw_method) =
                                    Self::resolve_instance_dunder(obj, "__bool__")
                                {
                                    let method = self.resolve_descriptor(&raw_method, obj)?;
                                    let result = self.call_object(method, vec![])?;
                                    if !matches!(
                                        &result.payload,
                                        ferrython_core::object::PyObjectPayload::Bool(_)
                                    ) {
                                        let tn = result.type_name();
                                        return Err(
                                            ferrython_core::error::PyException::type_error(
                                                compact_str::CompactString::from(format!(
                                                    "__bool__ should return bool, returned {}",
                                                    tn
                                                )),
                                            ),
                                        );
                                    }
                                    return Ok(result);
                                }
                                if let Some(raw_method) =
                                    Self::resolve_instance_dunder(obj, "__len__")
                                {
                                    let method = self.resolve_descriptor(&raw_method, obj)?;
                                    let result = self.call_object(method, vec![])?;
                                    match &result.payload {
                                        ferrython_core::object::PyObjectPayload::Int(n) => {
                                            let is_neg = n.to_i64().map(|v| v < 0).unwrap_or(false);
                                            if is_neg {
                                                return Err(
                                                    ferrython_core::error::PyException::value_error(
                                                        compact_str::CompactString::from(
                                                            "__len__() should return >= 0",
                                                        ),
                                                    ),
                                                );
                                            }
                                            return Ok(PyObject::bool_val(!n.is_zero()));
                                        }
                                        ferrython_core::object::PyObjectPayload::Bool(b) => {
                                            return Ok(PyObject::bool_val(*b));
                                        }
                                        _ => {
                                            let tn = result.type_name();
                                            return Err(
                                                ferrython_core::error::PyException::type_error(
                                                    compact_str::CompactString::from(format!(
                                                        "__len__() should return >= 0, returned {}",
                                                        tn
                                                    )),
                                                ),
                                            );
                                        }
                                    }
                                }
                            }
                            return Ok(PyObject::bool_val(self.vm_is_truthy(obj)?));
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
                            return Ok(PyObject::wrap(PyObjectPayload::MappingProxy(Rc::new(
                                PyCell::new(map),
                            ))));
                        }
                        if args.is_empty() {
                            return Err(PyException::type_error(
                                "mappingproxy() missing required argument: 'mapping'",
                            ));
                        }
                    }
                    "dir" => {
                        if args.is_empty() {
                            if let Some(locals) =
                                self.call_stack.last().and_then(|f| f.exec_locals.clone())
                            {
                                let mut names: Vec<String> = self
                                    .exec_locals_keys(&locals)?
                                    .into_iter()
                                    .map(|key| key.py_to_string())
                                    .collect();
                                names.sort();
                                let items = names
                                    .into_iter()
                                    .map(|n| PyObject::str_val(CompactString::from(n)))
                                    .collect();
                                return Ok(PyObject::list(items));
                            }
                            // dir() with no args: return sorted local variable names
                            let locals = self.collect_locals_dict()?;
                            if let PyObjectPayload::Dict(map) = &locals.payload {
                                let mut names: Vec<String> = map
                                    .read()
                                    .keys()
                                    .map(|k| k.to_object().py_to_string())
                                    .collect();
                                names.sort();
                                let items = names
                                    .into_iter()
                                    .map(|n| PyObject::str_val(CompactString::from(n)))
                                    .collect();
                                return Ok(PyObject::list(items));
                            }
                        }
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) =
                                    Self::resolve_instance_dunder(&args[0], "__dir__")
                                {
                                    let ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![args[0].clone()]
                                    };
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
                            return Err(PyException::type_error(
                                "__import__() requires at least 1 argument",
                            ));
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
                            return Err(PyException::type_error(
                                "getattr expected 2 or 3 arguments",
                            ));
                        }
                        let attr_name = args[1].as_str().ok_or_else(|| {
                            PyException::type_error("getattr(): attribute name must be string")
                        })?;
                        if attr_name == "__isabstractmethod__"
                            && ferrython_core::object::is_property_like(&args[0])
                        {
                            return self.property_isabstractmethod(&args[0]);
                        }
                        // Use get_attr which handles MRO + data descriptors
                        match args[0].get_attr(attr_name) {
                            Some(v) => {
                                // Invoke descriptor protocol (Property, custom __get__)
                                if ferrython_core::object::is_property_like(&v) {
                                    if matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                                        return Ok(v);
                                    }
                                    if let Some(getter) =
                                        ferrython_core::object::property_field(&v, "fget")
                                    {
                                        if matches!(&getter.payload, PyObjectPayload::None) {
                                            return Err(PyException::attribute_error(format!(
                                                "unreadable attribute '{}'",
                                                attr_name
                                            )));
                                        }
                                        let getter = crate::builtins::unwrap_abstract_fget(&getter);
                                        return self.call_object(getter, vec![args[0].clone()]);
                                    }
                                    return Err(PyException::attribute_error(format!(
                                        "unreadable attribute '{}'",
                                        attr_name
                                    )));
                                }
                                if has_descriptor_get(&v) {
                                    if let Some(get_method) = v.get_attr("__get__") {
                                        let (inst_arg, owner_arg) = match &args[0].payload {
                                            PyObjectPayload::Instance(inst) => {
                                                (args[0].clone(), inst.class.clone())
                                            }
                                            PyObjectPayload::Class(_) => {
                                                (PyObject::none(), args[0].clone())
                                            }
                                            _ => (args[0].clone(), PyObject::none()),
                                        };
                                        // get_method is already a BoundMethod if from class MRO
                                        return self
                                            .call_object(get_method, vec![inst_arg, owner_arg]);
                                    }
                                }
                                return Ok(v);
                            }
                            None => {
                                // Try __getattr__ fallback
                                if let PyObjectPayload::Instance(_) = &args[0].payload {
                                    if let Some(ga) = args[0].get_attr("__getattr__") {
                                        let name_arg =
                                            PyObject::str_val(CompactString::from(attr_name));
                                        return self.call_object(ga, vec![name_arg]);
                                    }
                                }
                                if args.len() > 2 {
                                    return Ok(args[2].clone());
                                }
                                return Err(PyException::attribute_error(format!(
                                    "'{}' object has no attribute '{}'",
                                    args[0].type_name(),
                                    attr_name
                                )));
                            }
                        }
                    }
                    "setattr" => {
                        if args.len() != 3 {
                            return Err(PyException::type_error(
                                "setattr() takes exactly 3 arguments",
                            ));
                        }
                        let attr_name = args[1].py_to_string();
                        let value = args[2].clone();
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if inst.class_flags
                                & (CLASS_FLAG_HAS_SETATTR
                                    | CLASS_FLAG_HAS_DESCRIPTORS
                                    | CLASS_FLAG_HAS_SLOTS)
                                == 0
                            {
                                inst.attrs
                                    .write()
                                    .insert(CompactString::from(attr_name.as_str()), value);
                                return Ok(PyObject::none());
                            }
                            if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                                if ferrython_core::object::is_property_like(&desc) {
                                    if let Some(setter) =
                                        ferrython_core::object::property_field(&desc, "fset")
                                    {
                                        if matches!(&setter.payload, PyObjectPayload::None) {
                                            return Err(PyException::attribute_error(format!(
                                                "can't set attribute '{}'",
                                                attr_name
                                            )));
                                        }
                                        self.call_object(setter, vec![args[0].clone(), value])?;
                                        return Ok(PyObject::none());
                                    } else {
                                        return Err(PyException::attribute_error(format!(
                                            "can't set attribute '{}'",
                                            attr_name
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
                                        },
                                    });
                                    self.call_object(
                                        method,
                                        vec![
                                            PyObject::str_val(CompactString::from(&attr_name)),
                                            value,
                                        ],
                                    )?;
                                    return Ok(PyObject::none());
                                }
                            }
                        }
                        return builtins::dispatch("setattr", &args);
                    }
                    "delattr" => {
                        if args.len() != 2 {
                            return Err(PyException::type_error(
                                "delattr() takes exactly 2 arguments",
                            ));
                        }
                        let attr_name = args[1].py_to_string();
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                                if ferrython_core::object::is_property_like(&desc) {
                                    if let Some(deleter) =
                                        ferrython_core::object::property_field(&desc, "fdel")
                                    {
                                        if matches!(&deleter.payload, PyObjectPayload::None) {
                                            return Err(PyException::attribute_error(format!(
                                                "can't delete attribute '{}'",
                                                attr_name
                                            )));
                                        }
                                        self.call_object(deleter, vec![args[0].clone()])?;
                                        return Ok(PyObject::none());
                                    }
                                    return Err(PyException::attribute_error(format!(
                                        "can't delete attribute '{}'",
                                        attr_name
                                    )));
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
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            let has_kwargs = kwargs_dict.is_some();
                            let positional_end = if has_kwargs {
                                args.len() - 1
                            } else {
                                args.len()
                            };

                            if positional_end >= 2 {
                                match &args[1].payload {
                                    PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
                                        if let Ok(items) = args[1].to_list() {
                                            for item in &items {
                                                if let PyObjectPayload::Tuple(pair) = &item.payload
                                                {
                                                    if !pair.is_empty() {
                                                        field_names.push(CompactString::from(
                                                            pair[0].py_to_string(),
                                                        ));
                                                    }
                                                } else {
                                                    field_names.push(CompactString::from(
                                                        item.py_to_string(),
                                                    ));
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
                                        if fname.as_str() != "defaults"
                                            && fname.as_str() != "module"
                                            && fname.as_str() != "rename"
                                        {
                                            let fname = fname.to_compact_string();
                                            if !field_names.contains(&fname) {
                                                field_names.push(fname);
                                            }
                                        }
                                    }
                                }
                            }

                            // Build namedtuple class with __namedtuple__ marker and _fields
                            let fields_tuple = PyObject::tuple(
                                field_names
                                    .iter()
                                    .map(|n| PyObject::str_val(n.clone()))
                                    .collect(),
                            );
                            let mut ns = IndexMap::new();
                            ns.insert(
                                CompactString::from("__namedtuple__"),
                                PyObject::bool_val(true),
                            );
                            ns.insert(CompactString::from("_fields"), fields_tuple);
                            ns.insert(
                                CompactString::from("_field_defaults"),
                                PyObject::dict(new_fx_hashkey_map()),
                            );
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
                        "'{}' is not callable",
                        name
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
                // Fast path: common receiver types (Str, List, Dict, Tuple, Set, Int, Float, Bool, Bytes)
                // go directly to builtins::call_method, skipping 15+ special-case checks for
                // Generator, Iterator, Range, Class, Property, Instance, BuiltinType, etc.
                // Exception: list.extend with a generator/lazy iterator must go through
                // collect_iterable first (builtins::call_method can't drive a generator).
                match &bbm.receiver.payload {
                    PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                        if !args.is_empty()
                            && matches!(
                                bbm.method_name.as_str(),
                                "union"
                                    | "intersection"
                                    | "difference"
                                    | "symmetric_difference"
                                    | "update"
                                    | "intersection_update"
                                    | "difference_update"
                                    | "symmetric_difference_update"
                                    | "issubset"
                                    | "issuperset"
                                    | "isdisjoint"
                                    | "__or__"
                                    | "__and__"
                                    | "__sub__"
                                    | "__xor__"
                            )
                            && matches!(
                                args[0].payload,
                                PyObjectPayload::Generator(_)
                                    | PyObjectPayload::Instance(_)
                                    | PyObjectPayload::Iterator(_)
                            ) =>
                    {
                        let mut resolved = Vec::with_capacity(args.len());
                        resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                        resolved.extend_from_slice(&args[1..]);
                        return builtins::call_method(
                            &bbm.receiver,
                            bbm.method_name.as_str(),
                            &resolved,
                        );
                    }
                    PyObjectPayload::List(_)
                        if bbm.method_name.as_str() == "extend"
                            && !args.is_empty()
                            && (matches!(
                                args[0].payload,
                                PyObjectPayload::Generator(_) | PyObjectPayload::Instance(_)
                            ) || matches!(&args[0].payload, PyObjectPayload::Iterator(ref d) if {
                                let data = d.read();
                                matches!(&*data, IteratorData::Enumerate { .. } | IteratorData::Zip { .. }
                                    | IteratorData::MapOne { .. }
                                    | IteratorData::Map { .. } | IteratorData::Filter { .. }
                                    | IteratorData::FilterFalse { .. }
                                    | IteratorData::Sentinel { .. })
                            })) =>
                    {
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::call_method(
                            &bbm.receiver,
                            "extend",
                            &[PyObject::list(items)],
                        );
                    }
                    PyObjectPayload::Str(_)
                    | PyObjectPayload::List(_)
                    | PyObjectPayload::Dict(_)
                    | PyObjectPayload::Tuple(_)
                    | PyObjectPayload::Set(_)
                    | PyObjectPayload::Int(_)
                    | PyObjectPayload::Float(_)
                    | PyObjectPayload::Bool(_)
                    | PyObjectPayload::Range(_)
                    | PyObjectPayload::Bytes(_)
                    | PyObjectPayload::ByteArray(_)
                    | PyObjectPayload::FrozenSet(_)
                        if !(matches!(&bbm.receiver.payload, PyObjectPayload::List(_))
                            && bbm.method_name.as_str() == "sort")
                            && !(bbm.method_name.as_str() == "join"
                                && matches!(
                                    &bbm.receiver.payload,
                                    PyObjectPayload::Str(_)
                                        | PyObjectPayload::Bytes(_)
                                        | PyObjectPayload::ByteArray(_)
                                )) =>
                    {
                        return builtins::call_method(
                            &bbm.receiver,
                            bbm.method_name.as_str(),
                            &args,
                        );
                    }
                    _ => {}
                }
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
                            let val = if args.is_empty() {
                                PyObject::none()
                            } else {
                                args[0].clone()
                            };
                            return self.resume_generator(gen_arc, val);
                        }
                        "throw" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            let original_value = Self::parse_throw_original_value(&args);
                            return self.gen_throw_with_value(
                                gen_arc,
                                exc_kind,
                                msg,
                                original_value,
                            );
                        }
                        "close" => {
                            // CPython: throw GeneratorExit into the frame so finally blocks run.
                            // If generator yields during cleanup → RuntimeError.
                            let gen = gen_arc.read();
                            if gen.finished || !gen.has_frame() {
                                // Already finished — nothing to clean up
                                drop(gen);
                                return Ok(PyObject::none());
                            }
                            drop(gen);
                            match self.gen_throw(
                                gen_arc,
                                ExceptionKind::GeneratorExit,
                                CompactString::new(""),
                            ) {
                                Ok(_yielded) => {
                                    // Generator yielded during close → RuntimeError
                                    return Err(PyException::runtime_error(
                                        "generator ignored GeneratorExit",
                                    ));
                                }
                                Err(e)
                                    if e.kind == ExceptionKind::GeneratorExit
                                        || e.kind == ExceptionKind::StopIteration =>
                                {
                                    // Expected: GeneratorExit propagated out or StopIteration
                                    let mut gen = gen_arc.write();
                                    gen.finished = true;
                                    gen.clear_frame();
                                    return Ok(PyObject::none());
                                }
                                Err(e) => {
                                    // Other exception from finally block — propagate
                                    let mut gen = gen_arc.write();
                                    gen.finished = true;
                                    gen.clear_frame();
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
                                let original_value = Self::parse_throw_original_value(&args);
                                match self.gen_throw_with_value(
                                    gen_arc,
                                    exc_kind,
                                    msg,
                                    original_value,
                                ) {
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
                                            "generator didn't stop",
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
                                },
                            }));
                        }
                        "asend" if kind == "async_generator" => {
                            let val = if args.is_empty() {
                                PyObject::none()
                            } else {
                                args[0].clone()
                            };
                            return Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: Box::new(AsyncGenAction::Send(val)),
                                },
                            }));
                        }
                        "athrow" if kind == "async_generator" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            return Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: Box::new(AsyncGenAction::Throw(
                                        exc_kind,
                                        CompactString::from(msg),
                                    )),
                                },
                            }));
                        }
                        "aclose" if kind == "async_generator" => {
                            return Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: Box::new(AsyncGenAction::Close),
                                },
                            }));
                        }
                        _ => {}
                    }
                }

                // ── Iterator protocol dispatch ──
                if let PyObjectPayload::Iterator(_)
                | PyObjectPayload::RangeIter(..)
                | PyObjectPayload::VecIter(_)
                | PyObjectPayload::WeakValueIter(_)
                | PyObjectPayload::WeakKeyIter(_)
                | PyObjectPayload::RefIter { .. }
                | PyObjectPayload::RevRefIter { .. } = &bbm.receiver.payload
                {
                    match bbm.method_name.as_str() {
                        "__next__" => match self.vm_iter_next(&bbm.receiver)? {
                            Some(value) => return Ok(value),
                            None => {
                                return Err(ferrython_core::error::PyException::stop_iteration())
                            }
                        },
                        "__iter__" => {
                            return Ok(bbm.receiver.clone());
                        }
                        "__length_hint__" => {
                            let len = bbm.receiver.py_len().unwrap_or(0);
                            return Ok(PyObject::int(len as i64));
                        }
                        "__setstate__" => {
                            return set_iterator_state(&bbm.receiver, &args);
                        }
                        _ => {}
                    }
                }

                // ── AsyncGenAwaitable dispatch (driving the awaitable) ──
                if let PyObjectPayload::AsyncGenAwaitable { gen, action } = &bbm.receiver.payload {
                    match bbm.method_name.as_str() {
                        "send" => {
                            let send_val = if args.is_empty() {
                                PyObject::none()
                            } else {
                                args[0].clone()
                            };
                            return self.drive_async_gen_awaitable(gen, action, send_val);
                        }
                        "throw" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            let original_value = Self::parse_throw_original_value(&args);
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
                            let strs: Result<Vec<String>, _> = items
                                .iter()
                                .map(|x| {
                                    x.as_str().map(String::from).ok_or_else(|| {
                                        ferrython_core::error::PyException::type_error(
                                            "sequence item: expected str",
                                        )
                                    })
                                })
                                .collect();
                            return Ok(PyObject::str_val(CompactString::from(
                                strs?.join(sep.as_str()),
                            )));
                        }
                    }
                    if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) =
                        &bbm.receiver.payload
                    {
                        if !args.is_empty() {
                            let sep = sep.clone();
                            let mutable_result =
                                matches!(&bbm.receiver.payload, PyObjectPayload::ByteArray(_));
                            let items = self.collect_iterable(&args[0])?;
                            let mut result = Vec::new();
                            for (i, item) in items.iter().enumerate() {
                                if i > 0 {
                                    result.extend_from_slice(&sep);
                                }
                                if let Some(data) = Self::bytes_like_data(item) {
                                    result.extend_from_slice(&data);
                                } else {
                                    return Err(PyException::type_error(
                                        "sequence item: expected a bytes-like object",
                                    ));
                                }
                            }
                            return Ok(if mutable_result {
                                PyObject::bytearray(result)
                            } else {
                                PyObject::bytes(result)
                            });
                        }
                    }
                }
                // VM-level list.sort with key function
                if bbm.method_name.as_str() == "sort" {
                    if matches!(&bbm.receiver.payload, PyObjectPayload::List(_)) {
                        let mut items_vec =
                            if let PyObjectPayload::List(items) = &bbm.receiver.payload {
                                items.read().clone()
                            } else {
                                Vec::new()
                            };
                        self.vm_sort(&mut items_vec)?;
                        if let PyObjectPayload::List(items) = &bbm.receiver.payload {
                            *items.write() = items_vec;
                        }
                        return Ok(PyObject::none());
                    }
                }
                // Range methods
                if let PyObjectPayload::Range(rd) = &bbm.receiver.payload {
                    let (rs, re, rst) = (rd.start, rd.stop, rd.step);
                    match bbm.method_name.as_str() {
                        "count" => {
                            if args.is_empty() {
                                return Err(PyException::type_error(
                                    "count() takes exactly one argument",
                                ));
                            }
                            let val = args[0].to_int().unwrap_or(i64::MIN);
                            let found = if rst > 0 {
                                val >= rs && val < re && (val - rs) % rst == 0
                            } else if rst < 0 {
                                val <= rs && val > re && (rs - val) % (-rst) == 0
                            } else {
                                false
                            };
                            return Ok(PyObject::int(if found { 1 } else { 0 }));
                        }
                        "index" => {
                            if args.is_empty() {
                                return Err(PyException::type_error(
                                    "index() takes exactly one argument",
                                ));
                            }
                            let val = args[0].to_int().unwrap_or(i64::MIN);
                            let in_range = if rst > 0 {
                                val >= rs && val < re && (val - rs) % rst == 0
                            } else if rst < 0 {
                                val <= rs && val > re && (rs - val) % (-rst) == 0
                            } else {
                                false
                            };
                            if in_range {
                                return Ok(PyObject::int((val - rs) / rst));
                            }
                            return Err(PyException::value_error(format!(
                                "{} is not in range",
                                val
                            )));
                        }
                        _ => {}
                    }
                }
                // Class introspection methods
                if let PyObjectPayload::Class(cd) = &bbm.receiver.payload {
                    match bbm.method_name.as_str() {
                        "__subclasses__" => {
                            let subs = cd.subclasses.read();
                            let alive: Vec<PyObjectRef> =
                                subs.iter().filter_map(|w| w.upgrade()).collect();
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
                if ferrython_core::object::is_property_like(&bbm.receiver) {
                    if args.len() == 1 {
                        let func = args[0].clone();
                        let old_fget = Self::property_callable_field(&bbm.receiver, "fget");
                        let old_fset = Self::property_callable_field(&bbm.receiver, "fset");
                        let old_fdel = Self::property_callable_field(&bbm.receiver, "fdel");
                        let doc_from_getter = Self::property_doc_from_getter_flag(&bbm.receiver);
                        let (fget, fset, fdel, doc, new_doc_from_getter) = match bbm
                            .method_name
                            .as_str()
                        {
                            "setter" => {
                                let doc = if doc_from_getter {
                                    ferrython_core::object::property_doc_from_getter(
                                        old_fget.as_ref(),
                                    )
                                } else {
                                    ferrython_core::object::property_field(&bbm.receiver, "__doc__")
                                };
                                (old_fget, Some(func), old_fdel, doc, doc_from_getter)
                            }
                            "getter" => {
                                let doc = if doc_from_getter {
                                    ferrython_core::object::property_doc_from_getter(Some(&func))
                                } else {
                                    ferrython_core::object::property_field(&bbm.receiver, "__doc__")
                                };
                                (Some(func), old_fset, old_fdel, doc, doc_from_getter)
                            }
                            "deleter" => {
                                let doc = if doc_from_getter {
                                    ferrython_core::object::property_doc_from_getter(
                                        old_fget.as_ref(),
                                    )
                                } else {
                                    ferrython_core::object::property_field(&bbm.receiver, "__doc__")
                                };
                                (old_fget, old_fset, Some(func), doc, doc_from_getter)
                            }
                            _ => {
                                return Err(PyException::attribute_error(format!(
                                    "property has no attribute '{}'",
                                    bbm.method_name
                                )))
                            }
                        };
                        return Self::make_property_like(
                            &bbm.receiver,
                            fget,
                            fset,
                            fdel,
                            doc,
                            new_doc_from_getter,
                        );
                    }
                }
                // namedtuple methods — delegated to builtins
                if let PyObjectPayload::Instance(inst) = &bbm.receiver.payload {
                    if matches!(&inst.class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__namedtuple__"))
                        || inst.attrs.read().contains_key("__deque__")
                    {
                        // deque extend/extendleft need iterable collection via VM
                        if inst.attrs.read().contains_key("__deque__")
                            && matches!(bbm.method_name.as_str(), "extend" | "extendleft")
                        {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::call_method(
                                &bbm.receiver,
                                bbm.method_name.as_str(),
                                &[PyObject::list(items)],
                            );
                        }
                        return builtins::call_method(
                            &bbm.receiver,
                            bbm.method_name.as_str(),
                            &args,
                        );
                    }
                    // Hashlib methods — delegated to builtins
                    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        cd.name.to_string()
                    } else {
                        String::new()
                    };
                    if matches!(
                        class_name.as_str(),
                        "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512"
                    ) {
                        return builtins::call_method(
                            &bbm.receiver,
                            bbm.method_name.as_str(),
                            &args,
                        );
                    }
                }
                // Unbound method call: str.upper("hello") → call_method("hello", "upper", [])
                if let PyObjectPayload::BuiltinType(tn) = &bbm.receiver.payload {
                    // type.__call__(cls, *args) → instantiate the class
                    if tn.as_str() == "type"
                        && bbm.method_name.as_str() == "__call__"
                        && !args.is_empty()
                    {
                        if matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                            let cls = args[0].clone();
                            let mut rest = args[1..].to_vec();
                            // Unpack trailing kwargs dict (produced by call_object_kw fallback)
                            let kw = {
                                let mut extracted = vec![];
                                let should_pop = if let Some(last) = rest.last() {
                                    if let PyObjectPayload::Dict(map) = &last.payload {
                                        let rd = map.read();
                                        let all_str =
                                            rd.keys().all(|k| matches!(k, HashableKey::Str(_)));
                                        if all_str && !rd.is_empty() {
                                            for (k, v) in rd.iter() {
                                                if let HashableKey::Str(s) = k {
                                                    extracted
                                                        .push((s.to_compact_string(), v.clone()));
                                                }
                                            }
                                            true
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                };
                                if should_pop {
                                    rest.pop();
                                }
                                extracted
                            };
                            return self.instantiate_class(&cls, rest, kw);
                        }
                    }
                    // Class methods (e.g., int.from_bytes, dict.fromkeys)
                    if let Some(class_method) =
                        builtins::resolve_type_class_method(tn, bbm.method_name.as_str())
                    {
                        if let PyObjectPayload::NativeFunction(nf) = &class_method.payload {
                            if nf.name.as_str() == "dict.fromkeys"
                                && !args.is_empty()
                                && matches!(
                                    args[0].payload,
                                    PyObjectPayload::Generator(_)
                                        | PyObjectPayload::Instance(_)
                                        | PyObjectPayload::Iterator(_)
                                )
                            {
                                let mut resolved = Vec::with_capacity(args.len());
                                resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                                resolved.extend_from_slice(&args[1..]);
                                return (nf.func)(&resolved);
                            }
                            return (nf.func)(&args);
                        }
                    }
                    if matches!(tn.as_str(), "bytes" | "bytearray")
                        && bbm.method_name.as_str() == "hex"
                    {
                        let (instance, rest_args) = Self::builtin_type_instance_operand(
                            tn.as_str(),
                            bbm.method_name.as_str(),
                            &args,
                        )?;
                        return builtins::call_method(
                            &instance,
                            bbm.method_name.as_str(),
                            &rest_args,
                        );
                    }
                    if !args.is_empty() {
                        let instance = args[0].clone();
                        let rest_args = if args.len() > 1 {
                            args[1..].to_vec()
                        } else {
                            vec![]
                        };
                        return builtins::call_method(
                            &instance,
                            bbm.method_name.as_str(),
                            &rest_args,
                        );
                    }
                }
                // list.extend with generator/lazy iterator/instance needs VM-level collection
                if bbm.method_name.as_str() == "extend" && !args.is_empty() {
                    if matches!(bbm.receiver.payload, PyObjectPayload::List(_)) {
                        if matches!(
                            args[0].payload,
                            PyObjectPayload::Generator(_) | PyObjectPayload::Instance(_)
                        ) || (matches!(&args[0].payload, PyObjectPayload::Iterator(ref d) if {
                            let data = d.read();
                            matches!(&*data, IteratorData::Enumerate { .. } | IteratorData::Zip { .. }
                                | IteratorData::MapOne { .. }
                                | IteratorData::Map { .. } | IteratorData::Filter { .. }
                                | IteratorData::FilterFalse { .. }
                                | IteratorData::Sentinel { .. })
                        })) {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::call_method(
                                &bbm.receiver,
                                "extend",
                                &[PyObject::list(items)],
                            );
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
                                if let Some(v) =
                                    rd.get(&HashableKey::str_key(CompactString::from("reverse")))
                                {
                                    reverse = v.is_truthy();
                                }
                                if let Some(v) =
                                    rd.get(&HashableKey::str_key(CompactString::from("key")))
                                {
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
                            let keys: Vec<PyObjectRef> =
                                decorated.iter().map(|(k, _)| k.clone()).collect();
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
                        } else {
                            let mut w = items.write();
                            let mut v: Vec<_> = w.drain(..).collect();
                            self.vm_sort(&mut v)?;
                            if reverse {
                                v.reverse();
                            }
                            w.extend(v);
                            return Ok(PyObject::none());
                        }
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
                            let factory_key = ferrython_core::types::HashableKey::str_key(
                                CompactString::from("__defaultdict_factory__"),
                            );
                            if m.read().contains_key(&factory_key) {
                                return self.vm_format_map_dict(s, &args[0], m);
                            }
                        }
                    }
                }
                builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), &args)
            }
            PyObjectPayload::ExceptionType(kind) => {
                build_builtin_exception_instance(*kind, args, &[])
            }
            PyObjectPayload::NativeFunction(nf_data) => {
                // Intercept functions that need VM access to call Python callables
                if nf_data.name.as_str() == "_ast.AST.__init__" {
                    if args.is_empty() {
                        return Err(PyException::type_error("__init__ requires self"));
                    }
                    let (pos_args, kwargs) = Self::split_trailing_kwargs_dict(&args);
                    if pos_args.is_empty() {
                        return Err(PyException::type_error("__init__ requires self"));
                    }
                    let instance = &pos_args[0];
                    let cls = match &instance.payload {
                        PyObjectPayload::Instance(inst) => inst.class.clone(),
                        _ => {
                            return Err(PyException::type_error(
                                "AST.__init__ requires an AST instance",
                            ))
                        }
                    };
                    Self::populate_ast_node_attrs(instance, &cls, &pos_args[1..], &kwargs)?;
                    return Ok(PyObject::none());
                }
                if nf_data.name.as_str() == "_ast.AST.__new__" {
                    if args.is_empty() {
                        return Err(PyException::type_error("__new__ requires cls"));
                    }
                    let (pos_args, kwargs) = Self::split_trailing_kwargs_dict(&args);
                    if pos_args.is_empty() {
                        return Err(PyException::type_error("__new__ requires cls"));
                    }
                    let cls = pos_args[0].clone();
                    let pos_args = pos_args[1..].to_vec();
                    return Ok(self
                        .try_instantiate_ast_node(&cls, pos_args, kwargs)?
                        .unwrap_or_else(|| PyObject::instance(cls)));
                }
                // property.__get__(self, obj, objtype) — must call fget(obj) and return result
                if nf_data.name.as_str() == "property.__get__" {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "descriptor '__get__' requires a property object",
                        ));
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
                    return Err(PyException::attribute_error("unreadable attribute"));
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
                if (nf_data.name.as_str() == "re.sub" || nf_data.name.as_str() == "re.subn")
                    && args.len() >= 3
                {
                    let repl = &args[1];
                    let is_callable = matches!(
                        &repl.payload,
                        PyObjectPayload::Function(_)
                            | PyObjectPayload::BuiltinFunction(_)
                            | PyObjectPayload::NativeFunction(_)
                            | PyObjectPayload::NativeClosure(_)
                            | PyObjectPayload::Partial(_)
                    );
                    if is_callable {
                        return self
                            .re_sub_with_callable(&args, nf_data.name.as_str() == "re.subn");
                    }
                }
                if nf_data.name.as_str() == "itertools.groupby" {
                    let mut key_fn = None;
                    let mut iterable_end = args.len();
                    // Check last arg for kwargs dict with "key"
                    if let Some(last) = args.last() {
                        if let PyObjectPayload::Dict(map) = &last.payload {
                            let map_r = map.read();
                            key_fn = map_r
                                .get(&HashableKey::str_key(CompactString::from("key")))
                                .cloned();
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
                if nf_data.name.as_str() == "dict.fromkeys"
                    && !args.is_empty()
                    && matches!(
                        args[0].payload,
                        PyObjectPayload::Generator(_)
                            | PyObjectPayload::Instance(_)
                            | PyObjectPayload::Iterator(_)
                    )
                {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                    resolved.extend_from_slice(&args[1..]);
                    return (nf_data.func)(&resolved);
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
                                let ca = if matches!(
                                    &method.payload,
                                    PyObjectPayload::BoundMethod { .. }
                                ) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                                return self.call_object(method, ca);
                            }
                        }
                    }
                }
                // os.fspath — dispatch to __fspath__
                if nf_data.name.as_str() == "os.fspath" && args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = args[0].get_attr("__fspath__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca);
                        }
                    }
                }
                // Resolve generators to lists for stdlib NativeFunctions
                // that expect iterables (e.g. Counter, deque, OrderedDict, set)
                if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Generator(_)) {
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
                    while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call()
                    {
                        collected.push(self.call_object(method, margs)?);
                    }
                    if !collected.is_empty() {
                        return Ok(PyObject::list(collected));
                    }
                }
                while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                    self.call_object(method, margs)?;
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
                let args = if !args.is_empty()
                    && matches!(&args[0].payload, PyObjectPayload::Generator(_))
                {
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
                    while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call()
                    {
                        collected.push(self.call_object(method, margs)?);
                    }
                    if !collected.is_empty() {
                        return Ok(PyObject::list(collected));
                    }
                }
                while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                    self.call_object(method, margs)?;
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
                    let kw: Vec<(CompactString, PyObjectRef)> = pd
                        .kwargs
                        .iter()
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
                            let key_str =
                                args.iter().map(|a| a.repr()).collect::<Vec<_>>().join(",");
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
                                    let hits = w
                                        .get(&intern_or_new("_hits"))
                                        .and_then(|v| v.as_int())
                                        .unwrap_or(0);
                                    w.insert(intern_or_new("_hits"), PyObject::int(hits + 1));
                                }
                                return Ok(cached);
                            }
                            // Cache miss: call the wrapped function, increment _misses
                            if let PyObjectPayload::Instance(ref d) = func.payload {
                                let mut w = d.attrs.write();
                                let misses = w
                                    .get(&intern_or_new("_misses"))
                                    .and_then(|v| v.as_int())
                                    .unwrap_or(0);
                                w.insert(intern_or_new("_misses"), PyObject::int(misses + 1));
                            }
                            let result = self.call_object(wrapped, args)?;
                            // Enforce maxsize: evict LRU entry (first in insertion order) when cache is full
                            {
                                let mut cache_w = cache_map.write();
                                if let PyObjectPayload::Instance(ref d) = func.payload {
                                    let maxsize = d
                                        .attrs
                                        .read()
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
                    let _dispatch_guard = self.enter_frameless_call_dispatch()?;
                    let result = self.call_object(method, args);
                    drop(func);
                    result
                } else {
                    Err(PyException::type_error(format!(
                        "'{}' object is not callable",
                        func.type_name()
                    )))
                }
            }
            _ => Err(PyException::type_error(format!(
                "'{}' object is not callable",
                func.type_name()
            ))),
        };
        if needs_current_frame {
            ferrython_stdlib::set_current_frame(prev_frame);
        }
        result
    }
}
