use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    has_descriptor_get, is_data_descriptor, lookup_in_class_mro, new_fx_hashkey_map, FxHashKeyMap,
    PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, CLASS_FLAG_HAS_DESCRIPTORS,
    CLASS_FLAG_HAS_SETATTR, CLASS_FLAG_HAS_SLOTS,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

use crate::builtins;
use crate::frame::ScopeKind;
use crate::vm_call::str_fast::fast_exact_str;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_or_type(
        &mut self,
        func: &PyObjectRef,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if name.as_str() == "__build_class__" {
            return self.build_class(args);
        }
        if matches!(
            name.as_str(),
            "map"
                | "filter"
                | "iter"
                | "next"
                | "list"
                | "tuple"
                | "sum"
                | "sorted"
                | "set"
                | "frozenset"
                | "dict"
                | "any"
                | "all"
                | "isinstance"
                | "issubclass"
                | "min"
                | "max"
                | "reversed"
                | "enumerate"
                | "zip"
        ) {
            return self.call_iterable_builtin(name, args);
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
                            if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
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
                            let call_args =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
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
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__hash__") {
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
            }
            "bin" | "oct" | "hex" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__index__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
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
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__format__")
                        {
                            let spec = if args.len() > 1 {
                                args[1].clone()
                            } else {
                                PyObject::str_val(CompactString::from(""))
                            };
                            let mut ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
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
                        let has_user_complex = inst.class.get_attr("__complex__").is_some() && {
                            // Distinguish user-defined from inherited builtin
                            let m = Self::resolve_instance_dunder(&args[0], "__complex__");
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
                                        if let Some(v) =
                                            i2.attrs.read().get("__builtin_value__").cloned()
                                        {
                                            if matches!(&v.payload, PyObjectPayload::Complex { .. })
                                            {
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
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Complex { .. }) {
                                return Ok(val);
                            }
                        }
                        // Fallback: __float__
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            let result = self.call_object(method, ca)?;
                            match &result.payload {
                                PyObjectPayload::Float(f) => return Ok(PyObject::complex(*f, 0.0)),
                                PyObjectPayload::Int(n) => {
                                    return Ok(PyObject::complex(n.to_f64(), 0.0))
                                }
                                PyObjectPayload::Bool(b) => {
                                    return Ok(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0))
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
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__index__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
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
                                    return Ok(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0))
                                }
                                _ => {
                                    return Err(PyException::type_error(format!(
                                        "__index__ returned non-int (type {})",
                                        result.type_name()
                                    )))
                                }
                            }
                        }
                        return Err(PyException::type_error(format!(
                            "complex() first argument must be a string or a number, not '{}'",
                            args[0].type_name()
                        )));
                    }
                } else if args.len() == 2 {
                    // Handle instances as either arg via __float__/__index__/__complex__
                    let coerce_for_complex = |vm: &mut Self,
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
                            if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
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
                                if let Some(method) = Self::resolve_instance_dunder(obj, dunder) {
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
                        let which_first = if matches!(&args[0].payload, PyObjectPayload::Str(_)) {
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
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Int(_)) {
                                return Ok(val);
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__int__") {
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
            }
            "round" => {
                if !args.is_empty() {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__round__") {
                            let mut ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
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
                    if let ferrython_core::object::PyObjectPayload::Instance(inst) = &obj.payload {
                        if let Some(target_fn) =
                            inst.attrs.read().get("__weakref_target__").cloned()
                        {
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
                                "mappingproxy() argument must be a mapping, not a non-mapping type",
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
                    if let Some(locals) = self.call_stack.last().and_then(|f| f.exec_locals.clone())
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
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__dir__") {
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
                    return Err(PyException::type_error("getattr expected 2 or 3 arguments"));
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
                            if let Some(getter) = ferrython_core::object::property_field(&v, "fget")
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
                                vec![PyObject::str_val(CompactString::from(&attr_name)), value],
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
                                        if let PyObjectPayload::Tuple(pair) = &item.payload {
                                            if !pair.is_empty() {
                                                field_names.push(CompactString::from(
                                                    pair[0].py_to_string(),
                                                ));
                                            }
                                        } else {
                                            field_names
                                                .push(CompactString::from(item.py_to_string()));
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
}
