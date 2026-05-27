use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    new_fx_hashkey_map, PartialData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::frame::ScopeKind;
use crate::vm_call::exception_build::build_builtin_exception_instance;
use crate::VirtualMachine;

impl VirtualMachine {
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
}
