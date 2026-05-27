use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    get_builtin_base_type_name, new_fx_hashkey_flatmap, new_fx_hashkey_map, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    /// Unified class instantiation: __new__, dataclass/namedtuple auto-init, __init__, exception attrs.
    pub(crate) fn instantiate_class(
        &mut self,
        cls: &PyObjectRef,
        mut pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if let Some(instance) =
            self.try_instantiate_ast_node(cls, pos_args.clone(), kwargs.clone())?
        {
            return Ok(instance);
        }

        if ferrython_core::object::is_property_subclass_class(cls) {
            let instance = PyObject::instance(cls.clone());
            Self::init_property_instance_attrs(&instance, &pos_args, &kwargs)?;
            return Ok(instance);
        }

        if let Some(instance) = self.try_instantiate_simple_class(cls, &mut pos_args, &kwargs)? {
            return Ok(instance);
        }
        self.check_abstract_class_instantiation(cls)?;
        if let Some(instance) = self.try_instantiate_enum(cls, &pos_args, &kwargs)? {
            return Ok(instance);
        }
        // __new__
        let instance = if cls.get_attr("__namedtuple__").is_some() {
            PyObject::instance(cls.clone())
        } else if let Some(new_method) = cls.get_attr("__new__") {
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
                        | "float.__new__" | "complex.__new__" | "object.__new__")
            );
            if is_builtin_new || is_native_builtin_new {
                let inst = PyObject::instance(cls.clone());
                // For builtin value subclasses (int, str, float, etc.), store the constructor
                // argument as __builtin_value__ so arithmetic/methods work correctly.
                // Dict subclasses use dict_storage instead; a synthetic empty __builtin_value__
                // would hide their real storage from dict methods such as items().
                // Namedtuple uses _tuple instead and should not receive __builtin_value__.
                if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                    if cls.get_attr("__namedtuple__").is_none() {
                        if let Some(base_type) = get_builtin_base_type_name(cls) {
                            let value = if pos_args.is_empty() {
                                // No-arg defaults for builtin type subclasses
                                match base_type.as_str() {
                                    "list" => Some(PyObject::list(vec![])),
                                    "set" => {
                                        Some(PyObject::set_from_flatmap(new_fx_hashkey_flatmap()))
                                    }
                                    "frozenset" => Some(PyObject::frozenset(new_fx_hashkey_map())),
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
                                            PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => {
                                                Some(arg.clone())
                                            }
                                            PyObjectPayload::Float(f) => {
                                                Some(PyObject::int(*f as i64))
                                            }
                                            PyObjectPayload::Str(s) => {
                                                s.trim().parse::<i64>().ok().map(PyObject::int)
                                            }
                                            _ => None,
                                        }
                                    }
                                    "float" => {
                                        let arg = &pos_args[0];
                                        match &arg.payload {
                                            PyObjectPayload::Float(_) => Some(arg.clone()),
                                            PyObjectPayload::Int(n) => {
                                                Some(PyObject::float(n.to_f64()))
                                            }
                                            PyObjectPayload::Bool(b) => {
                                                Some(PyObject::float(if *b { 1.0 } else { 0.0 }))
                                            }
                                            PyObjectPayload::Str(s) => {
                                                s.trim().parse::<f64>().ok().map(PyObject::float)
                                            }
                                            _ => None,
                                        }
                                    }
                                    "str" => {
                                        // str(bytes, encoding) → decode
                                        if pos_args.len() >= 2 {
                                            match &pos_args[0].payload {
                                                PyObjectPayload::Bytes(b)
                                                | PyObjectPayload::ByteArray(b) => {
                                                    let s = String::from_utf8_lossy(b);
                                                    return Ok(PyObject::str_val(
                                                        CompactString::from(s.as_ref()),
                                                    ));
                                                }
                                                _ => {}
                                            }
                                        }
                                        // Use vm_str for VM-aware conversion (calls __str__/__repr__)
                                        match self.vm_str(&pos_args[0]) {
                                            Ok(s) => {
                                                Some(PyObject::str_val(CompactString::from(s)))
                                            }
                                            Err(_) => {
                                                let s = pos_args[0].py_to_string();
                                                Some(PyObject::str_val(CompactString::from(s)))
                                            }
                                        }
                                    }
                                    "complex" => {
                                        let to_ri = |obj: &PyObjectRef| -> Option<(f64, f64)> {
                                            match &obj.payload {
                                                PyObjectPayload::Complex { real, imag } => {
                                                    Some((*real, *imag))
                                                }
                                                PyObjectPayload::Int(n) => Some((n.to_f64(), 0.0)),
                                                PyObjectPayload::Float(f) => Some((*f, 0.0)),
                                                PyObjectPayload::Bool(b) => {
                                                    Some((if *b { 1.0 } else { 0.0 }, 0.0))
                                                }
                                                _ => None,
                                            }
                                        };
                                        if pos_args.len() >= 2 {
                                            match (to_ri(&pos_args[0]), to_ri(&pos_args[1])) {
                                                (Some((ar, ai)), Some((br, bi))) => {
                                                    let a_c = matches!(
                                                        &pos_args[0].payload,
                                                        PyObjectPayload::Complex { .. }
                                                    );
                                                    let b_c = matches!(
                                                        &pos_args[1].payload,
                                                        PyObjectPayload::Complex { .. }
                                                    );
                                                    let r = if b_c { ar - bi } else { ar };
                                                    let i = if a_c { ai + br } else { br };
                                                    Some(PyObject::complex(r, i))
                                                }
                                                _ => None,
                                            }
                                        } else {
                                            let arg = &pos_args[0];
                                            match &arg.payload {
                                                PyObjectPayload::Complex { .. } => {
                                                    Some(arg.clone())
                                                }
                                                PyObjectPayload::Int(n) => {
                                                    Some(PyObject::complex(n.to_f64(), 0.0))
                                                }
                                                PyObjectPayload::Float(f) => {
                                                    Some(PyObject::complex(*f, 0.0))
                                                }
                                                PyObjectPayload::Bool(b) => {
                                                    Some(PyObject::complex(
                                                        if *b { 1.0 } else { 0.0 },
                                                        0.0,
                                                    ))
                                                }
                                                _ => None,
                                            }
                                        }
                                    }
                                    "list" => Some(PyObject::list(
                                        self.collect_iterable(&pos_args[0]).unwrap_or_default(),
                                    )),
                                    "tuple" => {
                                        // Namedtuple: multiple positional args → store as tuple
                                        // Regular tuple subclass: single iterable arg → expand to tuple
                                        if pos_args.len() > 1 {
                                            Some(PyObject::tuple(pos_args.clone()))
                                        } else {
                                            let items = self
                                                .collect_iterable(&pos_args[0])
                                                .unwrap_or_default();
                                            Some(PyObject::tuple(items))
                                        }
                                    }
                                    "set" => {
                                        if let PyObjectPayload::Dict(items) = &pos_args[0].payload {
                                            let read = items.read();
                                            let mut map = new_fx_hashkey_flatmap();
                                            map.reserve(read.len());
                                            for key in read.keys() {
                                                map.insert(key.clone(), key.to_object());
                                            }
                                            Some(PyObject::set_from_flatmap(map))
                                        } else {
                                            let mut map = new_fx_hashkey_flatmap();
                                            for item in self
                                                .collect_iterable(&pos_args[0])
                                                .unwrap_or_default()
                                            {
                                                if let Ok(key) = item.to_hashable_key() {
                                                    map.insert(key, item);
                                                }
                                            }
                                            Some(PyObject::set_from_flatmap(map))
                                        }
                                    }
                                    "frozenset" => {
                                        if let PyObjectPayload::Dict(items) = &pos_args[0].payload {
                                            let read = items.read();
                                            let mut map = new_fx_hashkey_map();
                                            for key in read.keys() {
                                                map.insert(key.clone(), key.to_object());
                                            }
                                            Some(PyObject::frozenset(map))
                                        } else {
                                            let mut map = new_fx_hashkey_map();
                                            for item in self
                                                .collect_iterable(&pos_args[0])
                                                .unwrap_or_default()
                                            {
                                                if let Ok(key) = item.to_hashable_key() {
                                                    map.insert(key, item);
                                                }
                                            }
                                            Some(PyObject::frozenset(map))
                                        }
                                    }
                                    "bytes" => Some(pos_args[0].clone()),
                                    "bytearray" => Some(pos_args[0].clone()),
                                    _ => None,
                                }
                            };
                            if let Some(val) = value {
                                inst_data
                                    .attrs
                                    .write()
                                    .insert(intern_or_new("__builtin_value__"), val);
                            }
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

        // Ensure builtin type subclass instances have __builtin_value__ set.
        // The synthesized __new__ creates a plain Instance; we must store the builtin
        // value so len/iter/indexing work on subclasses of tuple, list, int, etc.
        if let PyObjectPayload::Instance(ref inst_data) = instance.payload {
            if !inst_data.attrs.read().contains_key("__builtin_value__")
                && cls.get_attr("__namedtuple__").is_none()
            {
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    if let Some(ref base_type) = cd.builtin_base_name {
                        let value = if pos_args.is_empty() {
                            match base_type.as_str() {
                                "list" => Some(PyObject::list(vec![])),
                                "set" => Some(PyObject::set_from_flatmap(new_fx_hashkey_flatmap())),
                                "frozenset" => Some(PyObject::frozenset(new_fx_hashkey_map())),
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
                                        PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => {
                                            Some(arg.clone())
                                        }
                                        PyObjectPayload::Float(f) => Some(PyObject::int(*f as i64)),
                                        PyObjectPayload::Str(s) => {
                                            s.trim().parse::<i64>().ok().map(PyObject::int)
                                        }
                                        _ => None,
                                    }
                                }
                                "float" => {
                                    let arg = &pos_args[0];
                                    match &arg.payload {
                                        PyObjectPayload::Float(_) => Some(arg.clone()),
                                        PyObjectPayload::Int(n) => {
                                            Some(PyObject::float(n.to_f64()))
                                        }
                                        PyObjectPayload::Bool(b) => {
                                            Some(PyObject::float(if *b { 1.0 } else { 0.0 }))
                                        }
                                        PyObjectPayload::Str(s) => {
                                            s.trim().parse::<f64>().ok().map(PyObject::float)
                                        }
                                        _ => None,
                                    }
                                }
                                "str" => {
                                    let s = pos_args[0].py_to_string();
                                    Some(PyObject::str_val(CompactString::from(s)))
                                }
                                "list" => Some(PyObject::list(
                                    self.collect_iterable(&pos_args[0]).unwrap_or_default(),
                                )),
                                "tuple" => {
                                    if pos_args.len() > 1 {
                                        Some(PyObject::tuple(pos_args.clone()))
                                    } else {
                                        let items =
                                            self.collect_iterable(&pos_args[0]).unwrap_or_default();
                                        Some(PyObject::tuple(items))
                                    }
                                }
                                "set" => {
                                    if let PyObjectPayload::Dict(items) = &pos_args[0].payload {
                                        let read = items.read();
                                        let mut map = new_fx_hashkey_flatmap();
                                        map.reserve(read.len());
                                        for key in read.keys() {
                                            map.insert(key.clone(), key.to_object());
                                        }
                                        Some(PyObject::set_from_flatmap(map))
                                    } else {
                                        let mut map = new_fx_hashkey_flatmap();
                                        for item in
                                            self.collect_iterable(&pos_args[0]).unwrap_or_default()
                                        {
                                            if let Ok(key) = item.to_hashable_key() {
                                                map.insert(key, item);
                                            }
                                        }
                                        Some(PyObject::set_from_flatmap(map))
                                    }
                                }
                                "frozenset" => {
                                    if let PyObjectPayload::Dict(items) = &pos_args[0].payload {
                                        let read = items.read();
                                        let mut map = new_fx_hashkey_map();
                                        for key in read.keys() {
                                            map.insert(key.clone(), key.to_object());
                                        }
                                        Some(PyObject::frozenset(map))
                                    } else {
                                        let mut map = new_fx_hashkey_map();
                                        for item in
                                            self.collect_iterable(&pos_args[0]).unwrap_or_default()
                                        {
                                            if let Ok(key) = item.to_hashable_key() {
                                                map.insert(key, item);
                                            }
                                        }
                                        Some(PyObject::frozenset(map))
                                    }
                                }
                                "bytes" | "bytearray" => Some(pos_args[0].clone()),
                                "complex" => {
                                    let to_ri = |obj: &PyObjectRef| -> Option<(f64, f64)> {
                                        match &obj.payload {
                                            PyObjectPayload::Complex { real, imag } => {
                                                Some((*real, *imag))
                                            }
                                            PyObjectPayload::Int(n) => Some((n.to_f64(), 0.0)),
                                            PyObjectPayload::Float(f) => Some((*f, 0.0)),
                                            PyObjectPayload::Bool(b) => {
                                                Some((if *b { 1.0 } else { 0.0 }, 0.0))
                                            }
                                            _ => None,
                                        }
                                    };
                                    if pos_args.len() >= 2 {
                                        match (to_ri(&pos_args[0]), to_ri(&pos_args[1])) {
                                            (Some((ar, ai)), Some((br, bi))) => {
                                                let a_c = matches!(
                                                    &pos_args[0].payload,
                                                    PyObjectPayload::Complex { .. }
                                                );
                                                let b_c = matches!(
                                                    &pos_args[1].payload,
                                                    PyObjectPayload::Complex { .. }
                                                );
                                                let r = if b_c { ar - bi } else { ar };
                                                let i = if a_c { ai + br } else { br };
                                                Some(PyObject::complex(r, i))
                                            }
                                            _ => None,
                                        }
                                    } else {
                                        let arg = &pos_args[0];
                                        match &arg.payload {
                                            PyObjectPayload::Complex { .. } => Some(arg.clone()),
                                            PyObjectPayload::Int(n) => {
                                                Some(PyObject::complex(n.to_f64(), 0.0))
                                            }
                                            PyObjectPayload::Float(f) => {
                                                Some(PyObject::complex(*f, 0.0))
                                            }
                                            PyObjectPayload::Bool(b) => Some(PyObject::complex(
                                                if *b { 1.0 } else { 0.0 },
                                                0.0,
                                            )),
                                            _ => None,
                                        }
                                    }
                                }
                                _ => None,
                            }
                        };
                        if let Some(val) = value {
                            inst_data
                                .attrs
                                .write()
                                .insert(intern_or_new("__builtin_value__"), val);
                        }
                    }
                }
            }
        }

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
        let default_dict_init = cls
            .get_attr("__init__")
            .map(|init| match &init.payload {
                PyObjectPayload::NativeFunction(nf) => nf.name.as_str() == "dict.__init__",
                PyObjectPayload::BuiltinBoundMethod(bbm) => {
                    bbm.method_name.as_str() == "__init__"
                        && matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(name) if name.as_str() == "dict")
                }
                _ => false,
            })
            .unwrap_or(false);

        if is_dataclass && !has_user_init {
            // Dataclass auto-init: populate fields from args/kwargs
            let is_frozen = class_has_key(cls, "__dataclass_frozen__");
            if let Some(fields) = cls.get_attr("__dataclass_fields__") {
                // __dataclass_fields__ can be either:
                // - Tuple of (name, has_default, default_val, init_flag) tuples (legacy VM format)
                // - Dict mapping field_name → Field instance (Python dataclasses format)
                let field_entries: Vec<(String, bool, PyObjectRef, bool)> = match &fields.payload {
                    PyObjectPayload::Tuple(field_tuples) => field_tuples
                        .iter()
                        .filter_map(|ft| {
                            if let PyObjectPayload::Tuple(info) = &ft.payload {
                                let name = info[0].py_to_string();
                                let has_default = info[1].is_truthy();
                                let default_val = info[2].clone();
                                let field_init = if info.len() > 3 {
                                    info[3].is_truthy()
                                } else {
                                    true
                                };
                                Some((name, has_default, default_val, field_init))
                            } else {
                                None
                            }
                        })
                        .collect(),
                    PyObjectPayload::Dict(map) => {
                        // Dict of {name: Field} — extract field info from Field instances
                        let r = map.read();
                        r.iter()
                            .map(|(k, field_obj)| {
                                let name = match k {
                                    HashableKey::Str(s) => s.to_string(),
                                    _ => field_obj
                                        .get_attr("name")
                                        .map(|n| n.py_to_string())
                                        .unwrap_or_default(),
                                };
                                let field_init = field_obj
                                    .get_attr("init")
                                    .map(|v| v.is_truthy())
                                    .unwrap_or(true);
                                // Use __has_default__ flag (set by our Rust dataclass_apply)
                                // to reliably distinguish "no default" from "default is None"
                                let has_default_flag = field_obj
                                    .get_attr("__has_default__")
                                    .map(|v| v.is_truthy())
                                    .unwrap_or(false);
                                let default_factory = field_obj.get_attr("default_factory");
                                let has_factory = default_factory
                                    .as_ref()
                                    .map(|f| f.is_callable())
                                    .unwrap_or(false);
                                let (has_default, default_val) = if has_factory {
                                    (true, default_factory.unwrap_or_else(PyObject::none))
                                } else if has_default_flag {
                                    let default = field_obj
                                        .get_attr("default")
                                        .unwrap_or_else(PyObject::none);
                                    (true, default)
                                } else {
                                    (false, PyObject::none())
                                };
                                (name, has_default, default_val, field_init)
                            })
                            .collect()
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
                    } else if let Some((_, v)) =
                        kwargs.iter().find(|(k, _)| k.as_str() == name.as_str())
                    {
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
                            "__init__() missing required argument: '{}'",
                            name
                        )));
                    };

                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        inst.attrs
                            .write()
                            .insert(CompactString::from(name.as_str()), value);
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
                        ns.insert(
                            intern_or_new("__setattr__"),
                            PyObject::native_function("__setattr__", |_args| {
                                Err(PyException::attribute_error(String::from(
                                    "cannot assign to field of frozen dataclass",
                                )))
                            }),
                        );
                        ns.insert(
                            intern_or_new("__delattr__"),
                            PyObject::native_function("__delattr__", |_args| {
                                Err(PyException::attribute_error(String::from(
                                    "cannot delete field of frozen dataclass",
                                )))
                            }),
                        );
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
                            } else {
                                None
                            }
                        });
                        // Validate: too many positional args
                        if pos_args.len() > field_names.len() {
                            return Err(PyException::type_error(format!(
                                "__new__() takes {} positional arguments but {} were given",
                                field_names.len() + 1,
                                pos_args.len() + 1
                            )));
                        }
                        // Validate: unknown kwargs
                        let field_name_strs: Vec<String> =
                            field_names.iter().map(|f| f.py_to_string()).collect();
                        for (k, _) in &kwargs {
                            if !field_name_strs.iter().any(|n| n.as_str() == k.as_str()) {
                                return Err(PyException::type_error(format!(
                                    "got an unexpected keyword argument '{}'",
                                    k
                                )));
                            }
                        }
                        let mut attrs = inst.attrs.write();
                        let mut tuple_values = Vec::with_capacity(field_names.len());
                        let mut missing: Vec<String> = Vec::new();
                        for (i, field) in field_names.iter().enumerate() {
                            let name = field.py_to_string();
                            let value = if let Some((_, v)) =
                                kwargs.iter().find(|(k, _)| k.as_str() == name.as_str())
                            {
                                // Also detect duplicate: positional + kwarg
                                if i < pos_args.len() {
                                    return Err(PyException::type_error(format!(
                                        "got multiple values for argument '{}'",
                                        name
                                    )));
                                }
                                v.clone()
                            } else if i < pos_args.len() {
                                pos_args[i].clone()
                            } else if let Some(ref dmap) = defaults_map {
                                let key = HashableKey::str_key(CompactString::from(name.as_str()));
                                if let Some(v) = dmap.get(&key) {
                                    v.clone()
                                } else {
                                    missing.push(name.clone());
                                    PyObject::none()
                                }
                            } else {
                                missing.push(name.clone());
                                PyObject::none()
                            };
                            tuple_values.push(value);
                        }
                        if !missing.is_empty() {
                            drop(attrs);
                            return Err(PyException::type_error(format!(
                                "__new__() missing {} required argument{}: {}",
                                missing.len(),
                                if missing.len() == 1 { "" } else { "s" },
                                missing
                                    .iter()
                                    .map(|n| format!("'{}'", n))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )));
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
                            + init_result.type_name()
                            + "'",
                    ));
                }
            }
            // Dict subclass: populate dict_storage from pos_args/kwargs
            self.populate_dict_subclass_storage(&instance, &pos_args, &kwargs)?;
        }

        if default_dict_init {
            self.populate_dict_subclass_storage(&instance, &pos_args, &kwargs)?;
        }

        // Store kwargs as instance attrs when no user __init__ consumed them.
        // This supports AST nodes, simple data classes, and similar patterns
        // where Class(field=value) stores field as an attribute.
        if !kwargs.is_empty() && cls.get_attr("__namedtuple__").is_none() {
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
        if !pos_args.is_empty() && cls.get_attr("__namedtuple__").is_none() {
            if let Some(fields_obj) = cls.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields_obj.payload {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        let mut attrs = inst.attrs.write();
                        for (i, field) in field_names.iter().enumerate() {
                            if i < pos_args.len() {
                                let fname = field.py_to_string();
                                if !attrs.contains_key(fname.as_str()) {
                                    attrs.insert(
                                        CompactString::from(fname.as_str()),
                                        pos_args[i].clone(),
                                    );
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
}
