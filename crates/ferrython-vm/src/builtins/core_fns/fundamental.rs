use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, FxAttrMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{float_as_integer_ratio, HashableKey};
use num_bigint::BigInt;
use rustc_hash::FxHashMap;
use std::cell::Cell;
use std::rc::Rc;

pub(crate) fn builtin_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    // str(bytes_or_bytearray, encoding[, errors])
    if args.len() >= 2 {
        match &args[0].payload {
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                // encoding arg is present (ignore it for now, treat as utf-8)
                let s = String::from_utf8_lossy(b);
                return Ok(PyObject::str_val(CompactString::from(s.as_ref())));
            }
            _ => {}
        }
    }
    Ok(PyObject::str_val(CompactString::from(
        args[0].py_to_string(),
    )))
}

pub(crate) fn builtin_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::int(0));
    }
    if args.len() >= 2 {
        // int(string, base)
        let s = args[0].as_str().ok_or_else(|| {
            PyException::type_error("int() can't convert non-string with explicit base")
        })?;
        let base_int = args[1].to_int()?;
        if base_int != 0 && !(2..=36).contains(&base_int) {
            return Err(PyException::value_error(format!(
                "int() base must be >= 2 and <= 36, or 0, got {}",
                base_int
            )));
        }
        let mut base = base_int as u32;
        let s = s.trim();
        // Handle base 0: auto-detect from prefix
        let s = if base == 0 {
            if s.starts_with("0x") || s.starts_with("0X") {
                base = 16;
                &s[2..]
            } else if s.starts_with("0o") || s.starts_with("0O") {
                base = 8;
                &s[2..]
            } else if s.starts_with("0b") || s.starts_with("0B") {
                base = 2;
                &s[2..]
            } else {
                base = 10;
                s
            }
        } else if base == 16 && (s.starts_with("0x") || s.starts_with("0X")) {
            &s[2..]
        } else if base == 8 && (s.starts_with("0o") || s.starts_with("0O")) {
            &s[2..]
        } else if base == 2 && (s.starts_with("0b") || s.starts_with("0B")) {
            &s[2..]
        } else {
            s
        };
        let val = BigInt::parse_bytes(s.as_bytes(), base).ok_or_else(|| {
            PyException::value_error(format!(
                "invalid literal for int() with base {}: '{}'",
                base,
                args[0].as_str().unwrap()
            ))
        })?;
        return Ok(PyObject::big_int(val));
    }
    if let Some(text) = args[0].as_str() {
        let value = text.trim().parse::<BigInt>().map_err(|_| {
            PyException::value_error(format!("invalid literal for int(): '{}'", text))
        })?;
        return Ok(PyObject::big_int(value));
    }
    if let PyObjectPayload::Float(f) = &args[0].payload {
        if f.is_nan() {
            return Err(PyException::value_error(
                "cannot convert float NaN to integer",
            ));
        }
        if f.is_infinite() {
            return Err(PyException::overflow_error(
                "cannot convert float infinity to integer",
            ));
        }
        let truncated = f.trunc();
        if truncated >= -9_007_199_254_740_992.0 && truncated <= 9_007_199_254_740_992.0 {
            return Ok(PyObject::int(truncated as i64));
        }
        let (n, d) = float_as_integer_ratio(truncated);
        return Ok(PyObject::big_int(n / d));
    }
    Ok(PyObject::int(args[0].to_int()?))
}

pub(crate) fn builtin_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::float(0.0));
    }
    Ok(PyObject::float(args[0].to_float()?))
}

pub(crate) fn builtin_bool(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Reject kwargs: check if last arg looks like a kwargs dict (from call path).
    // Since this is NativeFunction, kwargs from call sites would be delivered as
    // extra trailing dict (they aren't here — if caller used x=10, it goes through
    // a different path). We must also reject >1 positional args.
    if args.len() > 1 {
        return Err(PyException::type_error(CompactString::from(format!(
            "bool() takes at most 1 argument ({} given)",
            args.len()
        ))));
    }
    if args.is_empty() {
        return Ok(PyObject::bool_val(false));
    }
    if let PyObjectPayload::Instance(inst) = &args[0].payload {
        if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
            if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                let referent = (nc.func)(&[])?;
                return Ok(PyObject::bool_val(referent.is_truthy()));
            }
        }
    }
    Ok(PyObject::bool_val(args[0].is_truthy()))
}

pub(crate) fn builtin_type(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // type.__new__(mcs, name, bases, dict) — called from metaclass __new__
    if args.len() == 4 {
        // First arg is the metaclass (mcs), use it; pass name, bases, dict
        let mcs = &args[0];
        let cls = builtin_type_create(&args[1], &args[2], &args[3])?;
        // Inject metaclass reference if mcs is a user-defined metaclass (not plain 'type')
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if cd.metaclass.is_none() {
                let is_plain_type =
                    matches!(&mcs.payload, PyObjectPayload::BuiltinType(n) if n.as_str() == "type");
                if !is_plain_type {
                    // Re-create with metaclass set
                    return Ok(PyObject::wrap(PyObjectPayload::Class(Box::new(
                        ferrython_core::object::ClassData {
                            name: cd.name.clone(),
                            bases: cd.bases.clone(),
                            namespace: cd.namespace.clone(),
                            mro: cd.mro.clone(),
                            metaclass: Some(mcs.clone()),
                            method_cache: Rc::new(PyCell::new(FxHashMap::default())),
                            subclasses: Rc::new(PyCell::new(Vec::new())),
                            slots: cd.slots.clone(),
                            has_getattribute: cd.has_getattribute,
                            has_getattr: cd.has_getattr,
                            has_setattr: cd.has_setattr,
                            has_descriptors: cd.has_descriptors,
                            method_vtable: cd.method_vtable.clone(),
                            attr_shape: cd.attr_shape.clone(),
                            class_version: cd.class_version,
                            is_dict_subclass: cd.is_dict_subclass,
                            expected_attrs: cd.expected_attrs,
                            is_simple_class: Cell::new(false), // has metaclass
                            is_exception_subclass: cd.is_exception_subclass,
                            instance_flags: cd.instance_flags,
                            cached_init: PyCell::new(None),
                            cached_init_inline: PyCell::new(None),
                            has_custom_new: Cell::new(cd.has_custom_new.get()),
                            builtin_base_name: cd.builtin_base_name.clone(),
                        },
                    ))));
                }
            }
        }
        return Ok(cls);
    }
    if args.len() == 3 {
        // type(name, bases, dict) -> dynamic class creation
        return builtin_type_create(&args[0], &args[1], &args[2]);
    }
    check_args("type", args, 1)?;
    let name = args[0].type_name();
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => Ok(inst.class.clone()),
        PyObjectPayload::ExceptionInstance(ei) => Ok(PyObject::exception_type(ei.kind)),
        PyObjectPayload::DictKeys { .. } => {
            Ok(PyObject::builtin_type(CompactString::from("dict_keys")))
        }
        PyObjectPayload::DictValues { .. } => {
            Ok(PyObject::builtin_type(CompactString::from("dict_values")))
        }
        PyObjectPayload::DictItems { .. } => {
            Ok(PyObject::builtin_type(CompactString::from("dict_items")))
        }
        // For classes with a custom metaclass, return the metaclass
        PyObjectPayload::Class(cd) => {
            if let Some(ref mcs) = cd.metaclass {
                Ok(mcs.clone())
            } else {
                Ok(PyObject::builtin_type(CompactString::from("type")))
            }
        }
        _ => Ok(PyObject::builtin_type(CompactString::from(name))),
    }
}

fn builtin_type_create(
    name_obj: &PyObjectRef,
    bases_obj: &PyObjectRef,
    dict_obj: &PyObjectRef,
) -> PyResult<PyObjectRef> {
    let name = name_obj
        .as_str()
        .ok_or_else(|| PyException::type_error("type() argument 1 must be str"))?;
    let bases = bases_obj.to_list()?;
    // Check for attempts to subclass final builtin types (bool)
    for base in &bases {
        match &base.payload {
            PyObjectPayload::BuiltinType(n) => {
                if n.as_str() == "bool" {
                    return Err(PyException::type_error(CompactString::from(
                        "type 'bool' is not an acceptable base type",
                    )));
                }
            }
            PyObjectPayload::Class(_) | PyObjectPayload::ExceptionType(_) => {}
            _ => {
                return Err(PyException::type_error(
                    "MRO entry resolution; use types.new_class()",
                ))
            }
        }
    }
    let namespace = match &dict_obj.payload {
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
            ns
        }
        _ => return Err(PyException::type_error("type() argument 3 must be dict")),
    };
    let class_cell = namespace.get("__classcell__").cloned();
    if let Some(cell) = &class_cell {
        if !matches!(&cell.payload, PyObjectPayload::Cell(_)) {
            return Err(PyException::type_error(
                "__classcell__ must be a nonlocal cell",
            ));
        }
    }
    let mut namespace = namespace;
    namespace.shift_remove("__classcell__");
    let mut mro = Vec::new();
    for base in &bases {
        if !matches!(&base.payload, PyObjectPayload::BuiltinType(n) if n.as_str() == "object") {
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
    let cls = PyObject::wrap(PyObjectPayload::Class(Box::new(
        ferrython_core::object::ClassData::new(
            CompactString::from(name),
            bases,
            namespace,
            mro,
            None,
        ),
    )));
    if let Some(cell_obj) = class_cell {
        if let PyObjectPayload::Cell(cell) = &cell_obj.payload {
            *cell.write() = Some(cls.clone());
        }
    }
    Ok(cls)
}
