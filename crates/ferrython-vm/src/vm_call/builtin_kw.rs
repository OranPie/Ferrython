use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    new_fx_hashkey_map, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::frame::ScopeKind;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_kw(
        &mut self,
        func: PyObjectRef,
        name: &CompactString,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        match name.as_str() {
            "__build_class__" => {
                return self.build_class_kw(pos_args, kwargs);
            }
            "sorted" => {
                if !pos_args.is_empty() {
                    // Steal contents if list is temporary (refcount==1) — avoids clone
                    let mut items_vec = if let PyObjectPayload::List(ref cell) = pos_args[0].payload
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
                if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "base") {
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
                        compact_str::CompactString::from("bool() takes no keyword arguments"),
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
                if let ferrython_core::object::PyObjectPayload::Instance(inst) = &obj.payload {
                    if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                        let referent = self.call_object(target_fn, vec![])?;
                        return Ok(PyObject::bool_val(self.vm_is_truthy(&referent)?));
                    }
                }
                // Instance with __bool__: call it and enforce return type == bool
                if let ferrython_core::object::PyObjectPayload::Instance(_) = &obj.payload {
                    if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__bool__") {
                        let method = self.resolve_descriptor(&raw_method, obj)?;
                        let result = self.call_object(method, vec![])?;
                        if !matches!(
                            &result.payload,
                            ferrython_core::object::PyObjectPayload::Bool(_)
                        ) {
                            let tn = result.type_name();
                            return Err(ferrython_core::error::PyException::type_error(
                                compact_str::CompactString::from(format!(
                                    "__bool__ should return bool, returned {}",
                                    tn
                                )),
                            ));
                        }
                        return Ok(result);
                    }
                    if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__len__") {
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
                                    return Err(ferrython_core::error::PyException::value_error(
                                        compact_str::CompactString::from(
                                            "__len__() should return >= 0",
                                        ),
                                    ));
                                }
                                return Ok(PyObject::bool_val(!n.is_zero()));
                            }
                            ferrython_core::object::PyObjectPayload::Bool(b) => {
                                return Ok(PyObject::bool_val(*b));
                            }
                            _ => {
                                let tn = result.type_name();
                                return Err(ferrython_core::error::PyException::type_error(
                                    compact_str::CompactString::from(format!(
                                        "__len__() should return >= 0, returned {}",
                                        tn
                                    )),
                                ));
                            }
                        }
                    }
                }
                return self.call_object(func, pos_args);
            }
            "float" | "str" | "bytes" | "bytearray" | "list" | "tuple" | "set" | "frozenset" => {
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
                        return Err(PyException::type_error(
                            "argument for complex() given by name ('real') and position (1)",
                        ));
                    }
                }
                if let Some(i) = imag_arg {
                    while all_args.len() < 1 {
                        all_args.push(PyObject::int(0));
                    }
                    if all_args.len() == 1 {
                        all_args.push(i);
                    } else {
                        return Err(PyException::type_error(
                            "argument for complex() given by name ('imag') and position (2)",
                        ));
                    }
                }
                return self.call_object(func, all_args);
            }
            "open" => {
                // open(file, mode='r', buffering=-1, encoding=None, ...)
                let mut all_args = pos_args;
                if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "mode") {
                    while all_args.len() < 2 {
                        all_args.push(PyObject::str_val(CompactString::from("r")));
                    }
                    all_args[1] = v.clone();
                }
                if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "encoding") {
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
                    if let Some((_, value)) = kwargs.iter().find(|(k, _)| k.as_str() == *key) {
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
                            HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__")),
                            PyObject::bool_val(true),
                        );
                    }
                    all_args.push(PyObject::dict(kw_map));
                    return self.call_object(func, all_args);
                }
                return self.call_object(func, pos_args);
            }
        }
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
