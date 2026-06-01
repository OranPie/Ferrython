//! Builtin type method resolver helpers.

use super::super::methods::{CompareOp, PyObjectMethods};
use super::super::payload::*;
use super::{
    is_dynamic_class_attribute, mark_dict_storage_mutated, property_field, property_init_doc,
    property_set_doc, unwrap_builtin_subclass,
};
use crate::error::{PyException, PyResult};
use crate::intern::intern_or_new;
use crate::object::ClassData;
use crate::types::{float_as_integer_ratio, hash_key_like_python, HashableKey};
use compact_str::CompactString;
use std::cell::RefCell;

thread_local! {
    static BUILTIN_TYPE_METHOD_CACHE: RefCell<rustc_hash::FxHashMap<(String, String), PyObjectRef>> =
        RefCell::new(rustc_hash::FxHashMap::default());
}

fn cached_builtin_type_method(
    type_name: &str,
    method_name: &str,
    make: impl FnOnce() -> PyObjectRef,
) -> PyObjectRef {
    let key = (type_name.to_string(), method_name.to_string());
    BUILTIN_TYPE_METHOD_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(obj) = cache.get(&key) {
            return obj.clone();
        }
        let obj = make();
        cache.insert(key, obj.clone());
        obj
    })
}

/// Resolve known built-in type methods that can be defined without VM access.
/// This is used by super() resolution when a base is a BuiltinType.
pub fn resolve_builtin_type_method(type_name: &str, method_name: &str) -> Option<PyObjectRef> {
    match (type_name, method_name) {
        ("property", "__get__") => Some(PyObject::native_function("property.__get__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "descriptor '__get__' requires a property object",
                ));
            }
            let prop = &args[0];
            let obj = args.get(1);
            let obj = match obj {
                Some(o) if !matches!(&o.payload, PyObjectPayload::None) => o,
                _ if is_dynamic_class_attribute(prop) => {
                    return Err(PyException::attribute_error(""));
                }
                _ => return Ok(prop.clone()),
            };
            if let Some(getter) = property_field(prop, "fget") {
                if !matches!(&getter.payload, PyObjectPayload::None) {
                    return Ok(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: obj.clone(),
                            method: getter,
                        },
                    }));
                }
            }
            Err(PyException::attribute_error("unreadable attribute"))
        })),
        ("type", "__new__") => Some(PyObject::native_function("type.__new__", |args| {
            // type.__new__(mcs, name, bases, dict) or type(name, bases, dict)
            if args.len() == 4 {
                let mcs = &args[0];
                let name = args[1].as_str().ok_or_else(|| {
                    PyException::type_error("type.__new__ argument 2 must be str")
                })?;
                let bases = args[2].to_list()?;
                let (mut namespace, class_cell) = match &args[3].payload {
                    PyObjectPayload::Dict(m) => {
                        let r = m.read();
                        let mut ns = FxAttrMap::default();
                        for (k, v) in r.iter() {
                            let key_str = match k {
                                HashableKey::Str(s) => s.to_compact_string(),
                                _ => CompactString::from(k.to_object().py_to_string()),
                            };
                            ns.insert(key_str, v.clone());
                        }
                        let class_cell = ns.get("__classcell__").cloned();
                        (ns, class_cell)
                    }
                    _ => {
                        return Err(PyException::type_error(
                            "type.__new__ argument 4 must be dict",
                        ))
                    }
                };
                if let Some(cell) = &class_cell {
                    if !matches!(&cell.payload, PyObjectPayload::Cell(_)) {
                        return Err(PyException::type_error(
                            "__classcell__ must be a nonlocal cell",
                        ));
                    }
                }
                namespace.shift_remove("__classcell__");
                let mut mro = Vec::new();
                for base in &bases {
                    if !matches!(&base.payload, PyObjectPayload::BuiltinType(n) if n.as_str() == "object")
                    {
                        mro.push(base.clone());
                    }
                    if let PyObjectPayload::Class(cd) = &base.payload {
                        for m in &cd.mro {
                            if !mro.iter().any(|existing| PyObjectRef::ptr_eq(existing, m)) {
                                mro.push(m.clone());
                            }
                        }
                    }
                }
                let is_plain_type =
                    matches!(&mcs.payload, PyObjectPayload::BuiltinType(n) if n.as_str() == "type");
                let cls = PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData::new(
                    CompactString::from(name),
                    bases,
                    namespace,
                    mro,
                    if is_plain_type {
                        None
                    } else {
                        Some(mcs.clone())
                    },
                ))));
                if let Some(cell_obj) = class_cell {
                    if let PyObjectPayload::Cell(cell) = &cell_obj.payload {
                        *cell.write() = Some(cls.clone());
                    }
                }
                Ok(cls)
            } else if args.len() == 3 {
                // type(name, bases, dict) — no mcs
                let name = args[0]
                    .as_str()
                    .ok_or_else(|| PyException::type_error("type() argument 1 must be str"))?;
                let bases = args[1].to_list()?;
                let (mut namespace, class_cell) = match &args[2].payload {
                    PyObjectPayload::Dict(m) => {
                        let r = m.read();
                        let mut ns = FxAttrMap::default();
                        for (k, v) in r.iter() {
                            let key_str = match k {
                                HashableKey::Str(s) => s.to_compact_string(),
                                _ => CompactString::from(k.to_object().py_to_string()),
                            };
                            ns.insert(key_str, v.clone());
                        }
                        let class_cell = ns.get("__classcell__").cloned();
                        (ns, class_cell)
                    }
                    _ => return Err(PyException::type_error("type() argument 3 must be dict")),
                };
                if let Some(cell) = &class_cell {
                    if !matches!(&cell.payload, PyObjectPayload::Cell(_)) {
                        return Err(PyException::type_error(
                            "__classcell__ must be a nonlocal cell",
                        ));
                    }
                }
                namespace.shift_remove("__classcell__");
                let mut mro = Vec::new();
                for base in &bases {
                    if !matches!(&base.payload, PyObjectPayload::BuiltinType(n) if n.as_str() == "object")
                    {
                        mro.push(base.clone());
                    }
                    if let PyObjectPayload::Class(cd) = &base.payload {
                        for m in &cd.mro {
                            if !mro.iter().any(|existing| PyObjectRef::ptr_eq(existing, m)) {
                                mro.push(m.clone());
                            }
                        }
                    }
                }
                let cls = PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData::new(
                    CompactString::from(name),
                    bases,
                    namespace,
                    mro,
                    None,
                ))));
                if let Some(cell_obj) = class_cell {
                    if let PyObjectPayload::Cell(cell) = &cell_obj.payload {
                        *cell.write() = Some(cls.clone());
                    }
                }
                Ok(cls)
            } else {
                Err(PyException::type_error(
                    "type.__new__ requires 3 or 4 arguments",
                ))
            }
        })),
        ("type", "mro") => Some(PyObject::native_function("type.mro", |args| {
            let Some(cls) = args.first() else {
                return Err(PyException::type_error(
                    "unbound method type.mro() needs an argument",
                ));
            };
            match &cls.payload {
                PyObjectPayload::Class(cd) => {
                    let mut mro_list = vec![cls.clone()];
                    mro_list.extend(cd.mro.iter().cloned());
                    Ok(PyObject::list(mro_list))
                }
                PyObjectPayload::BuiltinType(n) => {
                    let mut mro_list = vec![cls.clone()];
                    if n.as_str() == "bool" {
                        mro_list.push(PyObject::builtin_type(CompactString::from("int")));
                    }
                    if n.as_str() != "object" {
                        mro_list.push(PyObject::builtin_type(CompactString::from("object")));
                    }
                    Ok(PyObject::list(mro_list))
                }
                _ => Err(PyException::type_error(
                    "descriptor 'mro' requires a type object",
                )),
            }
        })),
        // tuple.__new__(cls, iterable) — create tuple subclass instance with __builtin_value__
        ("tuple", "__new__") => Some(PyObject::native_function("tuple.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("tuple.__new__ requires cls"));
            }
            let cls = &args[0];
            let inst = PyObject::instance(cls.clone());
            let items = if args.len() > 2 {
                // Multiple positional args (namedtuple-style): use all as items
                args[1..].to_vec()
            } else if args.len() == 2 {
                // Single arg: try to expand as iterable, else wrap
                args[1].to_list().unwrap_or_else(|_| vec![args[1].clone()])
            } else {
                vec![]
            };
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), PyObject::tuple(items));
            }
            Ok(inst)
        })),
        // list.__new__(cls, iterable) — create list subclass instance with __builtin_value__
        ("list", "__new__") => Some(PyObject::native_function("list.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("list.__new__ requires cls"));
            }
            let cls = &args[0];
            let inst = PyObject::instance(cls.clone());
            let items = if args.len() > 1 {
                args[1].to_list().unwrap_or_default()
            } else {
                vec![]
            };
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), PyObject::list(items));
            }
            Ok(inst)
        })),
        ("str", "__new__") => Some(PyObject::native_function("str.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("str.__new__ requires cls"));
            }
            let cls = &args[0];
            let value = if args.len() > 1 {
                args[1].py_to_string()
            } else {
                String::new()
            };
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data.attrs.write().insert(
                    intern_or_new("__builtin_value__"),
                    PyObject::str_val(CompactString::from(value)),
                );
            }
            Ok(inst)
        })),
        ("int", "__new__") => Some(PyObject::native_function("int.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("int.__new__ requires cls"));
            }
            let cls = &args[0];
            // CPython: int.__new__(bool, ...) is not allowed
            if let PyObjectPayload::BuiltinType(name) = &cls.payload {
                if name.as_str() == "bool" {
                    return Err(PyException::type_error(
                        "int.__new__(bool) is not safe, use bool.__new__()",
                    ));
                }
            }
            let value = if args.len() > 1 {
                match &args[1].payload {
                    PyObjectPayload::Int(value) => value.to_object(),
                    PyObjectPayload::Bool(value) => PyObject::int(if *value { 1 } else { 0 }),
                    PyObjectPayload::Str(value) => {
                        let text = value.trim();
                        match text.parse::<i64>() {
                            Ok(value) => PyObject::int(value),
                            Err(_) => {
                                let value = text.parse::<num_bigint::BigInt>().map_err(|_| {
                                    PyException::value_error(format!(
                                        "invalid literal for int(): '{}'",
                                        value
                                    ))
                                })?;
                                PyObject::big_int(value)
                            }
                        }
                    }
                    PyObjectPayload::Float(value) => {
                        if value.is_nan() {
                            return Err(PyException::value_error(
                                "cannot convert float NaN to integer",
                            ));
                        }
                        if value.is_infinite() {
                            return Err(PyException::overflow_error(
                                "cannot convert float infinity to integer",
                            ));
                        }
                        let (numerator, denominator) = float_as_integer_ratio(value.trunc());
                        PyObject::big_int(numerator / denominator)
                    }
                    _ => PyObject::int(args[1].to_int()?),
                }
            } else {
                PyObject::int(0)
            };
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), value);
            }
            Ok(inst)
        })),
        ("float", "__new__") => Some(PyObject::native_function("float.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("float.__new__ requires cls"));
            }
            let cls = &args[0];
            let value = if args.len() > 1 {
                match &args[1].payload {
                    PyObjectPayload::Float(f) => *f,
                    PyObjectPayload::Int(n) => n.to_f64(),
                    PyObjectPayload::Bool(b) => {
                        if *b {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    PyObjectPayload::Str(s) => s.parse::<f64>().map_err(|_| {
                        PyException::value_error(format!(
                            "could not convert string to float: '{}'",
                            s
                        ))
                    })?,
                    _ => {
                        return Err(PyException::type_error(format!(
                            "float() argument must be a string or a number, not '{}'",
                            args[1].type_name()
                        )))
                    }
                }
            } else {
                0.0
            };
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), PyObject::float(value));
            }
            Ok(inst)
        })),
        ("complex", "__new__") => Some(PyObject::native_function("complex.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("complex.__new__ requires cls"));
            }
            let cls = &args[0];
            // Extract (real, imag) from up to 2 more args
            let to_ri = |o: &PyObjectRef| -> (f64, f64) {
                match &o.payload {
                    PyObjectPayload::Complex { real, imag } => (*real, *imag),
                    PyObjectPayload::Int(n) => (n.to_f64(), 0.0),
                    PyObjectPayload::Float(f) => (*f, 0.0),
                    PyObjectPayload::Bool(b) => (if *b { 1.0 } else { 0.0 }, 0.0),
                    _ => (0.0, 0.0),
                }
            };
            let is_complex =
                |o: &PyObjectRef| matches!(&o.payload, PyObjectPayload::Complex { .. });
            let (real, imag) = match (args.get(1), args.get(2)) {
                (None, _) => (0.0, 0.0),
                (Some(a), None) => to_ri(a),
                (Some(a), Some(b)) => {
                    let (ar, ai) = to_ri(a);
                    let (br, bi) = to_ri(b);
                    let r = if is_complex(b) { ar - bi } else { ar };
                    let i = if is_complex(a) { ai + br } else { br };
                    (r, i)
                }
            };
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data.attrs.write().insert(
                    intern_or_new("__builtin_value__"),
                    PyObject::complex(real, imag),
                );
            }
            Ok(inst)
        })),
        ("object", "__new__") => Some(PyObject::native_function("object.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("object.__new__ requires cls"));
            }
            if args.len() != 1 {
                if let Some(cls) = args.first() {
                    if let PyObjectPayload::Class(cd) = &cls.payload {
                        let ns = cd.namespace.read();
                        if !ns.contains_key("__new__") && !ns.contains_key("__init__") {
                            return Err(PyException::type_error(format!(
                                "{}() takes no arguments",
                                cd.name
                            )));
                        }
                    }
                }
                return Err(PyException::type_error(
                    "object.__new__() takes exactly one argument (the type to instantiate)",
                ));
            }
            Ok(PyObject::instance(args[0].clone()))
        })),
        ("object", "__init__") => Some(PyObject::native_function("object.__init__", |args| {
            if args.len() != 1 {
                if let Some(instance) = args.first() {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            if !cd.namespace.read().contains_key("__init__") {
                                return Err(PyException::type_error(format!(
                                    "{}.__init__() takes exactly one argument (the instance to initialize)",
                                    cd.name
                                )));
                            }
                        }
                    }
                }
                return Err(PyException::type_error(
                    "object.__init__() takes exactly one argument (the instance to initialize)",
                ));
            }
            Ok(PyObject::none())
        })),
        // property.__init__(self, fget=None, fset=None, fdel=None, doc=None)
        // Store fget/fset/fdel on Instance attrs so property subclasses work
        ("property", "__init__") => Some(PyObject::native_function("property.__init__", |args| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let property_arg = |idx: usize| {
                args.get(idx).and_then(|arg| {
                    if matches!(&arg.payload, PyObjectPayload::None) {
                        None
                    } else {
                        Some(arg.clone())
                    }
                })
            };
            let fget = property_arg(1);
            let fset = property_arg(2);
            let fdel = property_arg(3);
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                let mut w = inst.attrs.write();
                w.insert(
                    CompactString::from("fget"),
                    fget.clone().unwrap_or_else(PyObject::none),
                );
                w.insert(
                    CompactString::from("fset"),
                    fset.clone().unwrap_or_else(PyObject::none),
                );
                w.insert(
                    CompactString::from("fdel"),
                    fdel.clone().unwrap_or_else(PyObject::none),
                );
            }
            let (doc, doc_from_getter) = property_init_doc(fget.as_ref(), args.get(4).cloned());
            if let Some(doc) = doc {
                property_set_doc(&args[0], doc)?;
            }
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                inst.attrs.write().insert(
                    CompactString::from("__property_doc_from_getter__"),
                    PyObject::bool_val(doc_from_getter),
                );
            }
            Ok(PyObject::none())
        })),
        // dict.__init__(self, data=None, **kwargs) — populate dict_storage from positional/kw args
        ("dict", "__init__") => Some(PyObject::native_function("dict.__init__", |args| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                if let Some(ref ds) = inst.dict_storage {
                    let mut storage = ds.write();
                    // If there's a positional arg (a dict or iterable of pairs), copy entries
                    if args.len() >= 2 {
                        match &args[1].payload {
                            PyObjectPayload::Dict(src) => {
                                for (k, v) in src.read().iter() {
                                    if storage.insert(k.clone(), v.clone()).is_none() {
                                        mark_dict_storage_mutated(ds);
                                    }
                                }
                            }
                            PyObjectPayload::Instance(src_inst)
                                if src_inst.dict_storage.is_some() =>
                            {
                                if let Some(src_ds) = src_inst.dict_storage.as_ref() {
                                    for (k, v) in src_ds.read().iter() {
                                        if storage.insert(k.clone(), v.clone()).is_none() {
                                            mark_dict_storage_mutated(ds);
                                        }
                                    }
                                }
                            }
                            _ => {
                                // Try treating as iterable of (key, value) pairs
                                if let Ok(items) = args[1].to_list() {
                                    for item in &items {
                                        if let Ok(pair) = item.to_list() {
                                            if pair.len() == 2 {
                                                let hk = pair[0].to_hashable_key()?;
                                                if storage.insert(hk, pair[1].clone()).is_none() {
                                                    mark_dict_storage_mutated(ds);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(PyObject::none())
        })),
        // __init__ on any builtin type base is a no-op (instance already created)
        (_, "__init__") => Some(PyObject::native_function("builtin.__init__", |_args| {
            Ok(PyObject::none())
        })),
        ("dict", "keys") => Some(PyObject::native_function("dict.keys", |args| {
            dict_storage_view(args, "keys", "dict.keys")
        })),
        ("dict", "values") => Some(PyObject::native_function("dict.values", |args| {
            dict_storage_view(args, "values", "dict.values")
        })),
        ("dict", "items") => Some(PyObject::native_function("dict.items", |args| {
            dict_storage_view(args, "items", "dict.items")
        })),
        ("dict", "copy") => Some(PyObject::native_function("dict.copy", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "dict.copy() takes exactly one argument",
                ));
            }
            match &args[0].payload {
                PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => {
                    Ok(PyObject::dict(map.read().clone()))
                }
                PyObjectPayload::Instance(inst) => {
                    if let Some(ref ds) = inst.dict_storage {
                        Ok(PyObject::dict(ds.read().clone()))
                    } else {
                        Err(PyException::type_error(
                            "dict.copy requires a dict instance",
                        ))
                    }
                }
                _ => Err(PyException::type_error(
                    "dict.copy requires a dict instance",
                )),
            }
        })),
        // dict.__getitem__(self, key) — access dict_storage on dict subclass
        ("dict", "__getitem__") => Some(PyObject::native_function("dict.__getitem__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "dict.__getitem__() takes exactly 2 arguments",
                ));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    if let Some(val) = ds.read().get(&hk) {
                        return Ok(val.clone());
                    }
                    return Err(PyException::key_error(args[1].py_to_string()));
                }
            }
            Err(PyException::type_error(
                "dict.__getitem__ requires a dict instance",
            ))
        })),
        // dict.__setitem__(self, key, value)
        ("dict", "__setitem__") => Some(PyObject::native_function("dict.__setitem__", |args| {
            if args.len() != 3 {
                return Err(PyException::type_error(
                    "dict.__setitem__() takes exactly 3 arguments",
                ));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    if ds.write().insert(hk, args[2].clone()).is_none() {
                        mark_dict_storage_mutated(ds);
                    }
                    return Ok(PyObject::none());
                }
            }
            Err(PyException::type_error(
                "dict.__setitem__ requires a dict instance",
            ))
        })),
        // dict.__delitem__(self, key)
        ("dict", "__delitem__") => Some(PyObject::native_function("dict.__delitem__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "dict.__delitem__() takes exactly 2 arguments",
                ));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    if ds.write().shift_remove(&hk).is_some() {
                        mark_dict_storage_mutated(ds);
                        return Ok(PyObject::none());
                    }
                    return Err(PyException::key_error(args[1].py_to_string()));
                }
            }
            Err(PyException::type_error(
                "dict.__delitem__ requires a dict instance",
            ))
        })),
        // dict.__contains__(self, key)
        ("dict", "__contains__") => Some(PyObject::native_function("dict.__contains__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "dict.__contains__() takes exactly 2 arguments",
                ));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    return Ok(PyObject::bool_val(ds.read().contains_key(&hk)));
                }
            }
            Ok(PyObject::bool_val(false))
        })),
        // Arithmetic dunder wrappers for builtin types (unbound method access)
        (_, "__add__") => Some(PyObject::native_function("__add__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__add__ takes 2 arguments"));
            }
            args[0].add(&args[1])
        })),
        (_, "__sub__") => Some(PyObject::native_function("__sub__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__sub__ takes 2 arguments"));
            }
            args[0].sub(&args[1])
        })),
        (_, "__mul__") => Some(PyObject::native_function("__mul__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__mul__ takes 2 arguments"));
            }
            args[0].mul(&args[1])
        })),
        (_, "__truediv__") => Some(PyObject::native_function("__truediv__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__truediv__ takes 2 arguments"));
            }
            args[0].true_div(&args[1])
        })),
        (_, "__floordiv__") => Some(PyObject::native_function("__floordiv__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__floordiv__ takes 2 arguments"));
            }
            args[0].floor_div(&args[1])
        })),
        (_, "__mod__") => Some(PyObject::native_function("__mod__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__mod__ takes 2 arguments"));
            }
            args[0].modulo(&args[1])
        })),
        (_, "__eq__") => Some(PyObject::native_function("__eq__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__eq__ takes 2 arguments"));
            }
            args[0].compare(&args[1], CompareOp::Eq)
        })),
        (_, "__ne__") => Some(PyObject::native_function("__ne__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__ne__ takes 2 arguments"));
            }
            args[0].compare(&args[1], CompareOp::Ne)
        })),
        (_, "__lt__") => Some(PyObject::native_function("__lt__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__lt__ takes 2 arguments"));
            }
            if matches!(&args[0].payload, PyObjectPayload::Complex { .. }) {
                return Ok(PyObject::not_implemented());
            }
            args[0].compare(&args[1], CompareOp::Lt)
        })),
        (_, "__le__") => Some(PyObject::native_function("__le__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__le__ takes 2 arguments"));
            }
            if matches!(&args[0].payload, PyObjectPayload::Complex { .. }) {
                return Ok(PyObject::not_implemented());
            }
            args[0].compare(&args[1], CompareOp::Le)
        })),
        (_, "__gt__") => Some(PyObject::native_function("__gt__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__gt__ takes 2 arguments"));
            }
            if matches!(&args[0].payload, PyObjectPayload::Complex { .. }) {
                return Ok(PyObject::not_implemented());
            }
            args[0].compare(&args[1], CompareOp::Gt)
        })),
        (_, "__ge__") => Some(PyObject::native_function("__ge__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__ge__ takes 2 arguments"));
            }
            if matches!(&args[0].payload, PyObjectPayload::Complex { .. }) {
                return Ok(PyObject::not_implemented());
            }
            args[0].compare(&args[1], CompareOp::Ge)
        })),
        (_, "__neg__") => Some(PyObject::native_function("__neg__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__neg__ takes 1 argument"));
            }
            args[0].negate()
        })),
        (_, "__abs__") => Some(PyObject::native_function("__abs__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__abs__ takes 1 argument"));
            }
            args[0].py_abs()
        })),
        (_, "__len__") => Some(PyObject::native_function("__len__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__len__ takes 1 argument"));
            }
            Ok(PyObject::int(args[0].py_len()? as i64))
        })),
        (_, "__contains__") => Some(PyObject::native_function("__contains__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__contains__ takes 2 arguments"));
            }
            Ok(PyObject::bool_val(args[0].contains(&args[1])?))
        })),
        (type_name, "__repr__") => Some(cached_builtin_type_method(type_name, "__repr__", || {
            let fn_name = format!("{type_name}.__repr__");
            PyObject::native_closure(&fn_name, |args| {
                if args.len() != 1 {
                    return Err(PyException::type_error("__repr__ takes 1 argument"));
                }
                Ok(PyObject::str_val(CompactString::from(args[0].repr())))
            })
        })),
        (type_name, "__str__") => Some(cached_builtin_type_method(type_name, "__str__", || {
            let fn_name = format!("{type_name}.__str__");
            PyObject::native_closure(&fn_name, |args| {
                if args.len() != 1 {
                    return Err(PyException::type_error("__str__ takes 1 argument"));
                }
                Ok(PyObject::str_val(CompactString::from(
                    args[0].py_to_string(),
                )))
            })
        })),
        (_, "__hash__") => Some(PyObject::native_function("__hash__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__hash__ takes 1 argument"));
            }
            let value = unwrap_builtin_subclass(&args[0]);
            if let PyObjectPayload::Int(n) = &value.payload {
                return Ok(n.to_object());
            }
            if let PyObjectPayload::Bool(b) = &value.payload {
                return Ok(PyObject::int(*b as i64));
            }
            let hk = value.to_hashable_key()?;
            Ok(PyObject::int(hash_key_like_python(&hk)))
        })),
        (_, "__bool__") => Some(PyObject::native_function("__bool__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__bool__ takes 1 argument"));
            }
            Ok(PyObject::bool_val(args[0].is_truthy()))
        })),
        (_, "__pow__") => Some(PyObject::native_function("__pow__", |args| {
            if args.len() < 2 || args.len() > 3 {
                return Err(PyException::type_error("__pow__ takes 2-3 arguments"));
            }
            args[0].power(&args[1])
        })),
        (_, "__lshift__") => Some(PyObject::native_function("__lshift__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__lshift__ takes 2 arguments"));
            }
            args[0].lshift(&args[1])
        })),
        (_, "__rshift__") => Some(PyObject::native_function("__rshift__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__rshift__ takes 2 arguments"));
            }
            args[0].rshift(&args[1])
        })),
        (_, "__and__") => Some(PyObject::native_function("__and__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__and__ takes 2 arguments"));
            }
            args[0].bit_and(&args[1])
        })),
        (_, "__or__") => Some(PyObject::native_function("__or__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__or__ takes 2 arguments"));
            }
            args[0].bit_or(&args[1])
        })),
        (_, "__xor__") => Some(PyObject::native_function("__xor__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__xor__ takes 2 arguments"));
            }
            args[0].bit_xor(&args[1])
        })),
        (_, "__pos__") => Some(PyObject::native_function("__pos__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__pos__ takes 1 argument"));
            }
            args[0].positive()
        })),
        (_, "__invert__") => Some(PyObject::native_function("__invert__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__invert__ takes 1 argument"));
            }
            args[0].invert()
        })),
        (_, "__getitem__") => Some(PyObject::native_function("__getitem__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__getitem__ takes 2 arguments"));
            }
            args[0].get_item(&args[1])
        })),
        (_, "__int__") => Some(PyObject::native_function("__int__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__int__ takes 1 argument"));
            }
            Ok(PyObject::int(args[0].to_int()?))
        })),
        (_, "__float__") => Some(PyObject::native_function("__float__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__float__ takes 1 argument"));
            }
            Ok(PyObject::float(args[0].to_float()?))
        })),
        (_, "__index__") => Some(PyObject::native_function("__index__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__index__ takes 1 argument"));
            }
            Ok(PyObject::int(args[0].to_int()?))
        })),
        (_, "__iter__") => None, // handled by VM iter() builtin
        (_, "__sizeof__") => Some(PyObject::native_function("__sizeof__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("__sizeof__ takes 1 argument"));
            }
            let size = std::mem::size_of::<PyObject>() as i64
                + match &args[0].payload {
                    PyObjectPayload::Str(s) => s.len() as i64,
                    PyObjectPayload::Bytes(b) => b.len() as i64,
                    PyObjectPayload::List(items) => {
                        (items.read().len() * std::mem::size_of::<PyObjectRef>()) as i64
                    }
                    PyObjectPayload::Dict(map) => (map.read().len() * 64) as i64,
                    PyObjectPayload::Set(set) => (set.read().len() * 32) as i64,
                    PyObjectPayload::Tuple(items) => {
                        (items.len() * std::mem::size_of::<PyObjectRef>()) as i64
                    }
                    _ => 0,
                };
            Ok(PyObject::int(size))
        })),
        _ => None,
    }
}

fn dict_storage_view(args: &[PyObjectRef], view: &str, name: &str) -> PyResult<PyObjectRef> {
    if args.len() != 1 {
        return Err(PyException::type_error(format!(
            "{}() takes exactly 1 argument",
            name
        )));
    }
    match &args[0].payload {
        PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => {
            Ok(dict_view_payload(map.clone(), Some(args[0].clone()), view))
        }
        PyObjectPayload::Instance(inst) => {
            if let Some(ref ds) = inst.dict_storage {
                Ok(dict_view_payload(ds.clone(), Some(args[0].clone()), view))
            } else {
                Err(PyException::type_error(format!(
                    "{}() requires a dict instance",
                    name
                )))
            }
        }
        _ => Err(PyException::type_error(format!(
            "{}() requires a dict instance",
            name
        ))),
    }
}

fn dict_view_payload(
    map: std::rc::Rc<PyCell<FxHashKeyMap>>,
    owner: Option<PyObjectRef>,
    view: &str,
) -> PyObjectRef {
    match view {
        "keys" => PyObject::wrap(PyObjectPayload::DictKeys { map, owner }),
        "values" => PyObject::wrap(PyObjectPayload::DictValues { map, owner }),
        _ => PyObject::wrap(PyObjectPayload::DictItems { map, owner }),
    }
}
