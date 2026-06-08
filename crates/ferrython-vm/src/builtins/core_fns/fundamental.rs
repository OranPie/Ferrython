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
        let text = int_explicit_base_text_arg(&args[0]).ok_or_else(|| {
            PyException::type_error("int() can't convert non-string with explicit base")
        })?;
        let base_index = args[1].to_index()?;
        let base_int = base_index
            .to_i64()
            .ok_or_else(|| PyException::value_error("int() base must be >= 2 and <= 36, or 0"))?;
        if base_int != 0 && !(2..=36).contains(&base_int) {
            return Err(PyException::value_error(format!(
                "int() base must be >= 2 and <= 36, or 0, got {}",
                base_int
            )));
        }
        return parse_int_text(&text, Some(base_int as u32), &args[0]);
    }
    if let Some(text) = int_text_arg(&args[0]) {
        return parse_int_text(&text, None, &args[0]);
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

fn int_explicit_base_text_arg(obj: &PyObjectRef) -> Option<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Str(s) => Some(s.as_str().as_bytes().to_vec()),
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Some((**b).clone()),
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__builtin_value__")
            .and_then(int_explicit_base_text_arg),
        _ => None,
    }
}

fn int_text_arg(obj: &PyObjectRef) -> Option<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => {
            int_explicit_base_text_arg(obj)
        }
        PyObjectPayload::Instance(_) if obj.get_attr("__memoryview__").is_some() => {
            obj.get_attr("obj").and_then(|base| int_text_arg(&base))
        }
        PyObjectPayload::Instance(inst) if obj.get_attr("__array__").is_some() => {
            let typecode = obj.get_attr("typecode")?.py_to_string();
            if typecode != "B" && typecode != "b" {
                return None;
            }
            let data = inst.attrs.read().get("_data").cloned()?;
            let PyObjectPayload::List(items) = &data.payload else {
                return None;
            };
            items
                .read()
                .iter()
                .map(|item| item.to_int().ok().map(|value| value as u8))
                .collect()
        }
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__builtin_value__")
            .and_then(int_text_arg),
        _ => None,
    }
}

fn parse_int_text(
    text: &[u8],
    explicit_base: Option<u32>,
    original: &PyObjectRef,
) -> PyResult<PyObjectRef> {
    let trimmed = trim_ascii_whitespace(text);
    let (negative, unsigned) = match trimmed.first().copied() {
        Some(b'+') => (false, &trimmed[1..]),
        Some(b'-') => (true, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let mut base = explicit_base.unwrap_or(10);
    let mut digits = unsigned;
    let mut prefixed = false;
    if explicit_base == Some(0) {
        if ascii_starts_with_any(unsigned, b"0x", b"0X") {
            base = 16;
            digits = &unsigned[2..];
            prefixed = true;
        } else if ascii_starts_with_any(unsigned, b"0o", b"0O") {
            base = 8;
            digits = &unsigned[2..];
            prefixed = true;
        } else if ascii_starts_with_any(unsigned, b"0b", b"0B") {
            base = 2;
            digits = &unsigned[2..];
            prefixed = true;
        } else {
            base = 10;
            digits = unsigned;
        }
    } else {
        match base {
            16 if ascii_starts_with_any(unsigned, b"0x", b"0X") => {
                digits = &unsigned[2..];
                prefixed = true;
            }
            8 if ascii_starts_with_any(unsigned, b"0o", b"0O") => {
                digits = &unsigned[2..];
                prefixed = true;
            }
            2 if ascii_starts_with_any(unsigned, b"0b", b"0B") => {
                digits = &unsigned[2..];
                prefixed = true;
            }
            _ => {}
        }
    }
    let Some(cleaned) = clean_int_digits(digits, base, prefixed) else {
        let shown_base = if explicit_base.is_none() { 10 } else { base };
        return Err(PyException::value_error(format!(
            "invalid literal for int() with base {}: {}",
            shown_base,
            int_literal_repr(original)
        )));
    };
    if explicit_base == Some(0)
        && base == 10
        && !decimal_base_zero_allows_leading_zero(unsigned, &cleaned)
    {
        return Err(PyException::value_error(format!(
            "invalid literal for int() with base 0: {}",
            int_literal_repr(original)
        )));
    }
    let mut value = BigInt::parse_bytes(&cleaned, base).ok_or_else(|| {
        let shown_base = if explicit_base.is_none() { 10 } else { base };
        PyException::value_error(format!(
            "invalid literal for int() with base {}: {}",
            shown_base,
            int_literal_repr(original)
        ))
    })?;
    if negative {
        value = -value;
    }
    Ok(PyObject::big_int(value))
}

fn trim_ascii_whitespace(text: &[u8]) -> &[u8] {
    let start = text
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map(|idx| idx + 1)
        .unwrap_or(start);
    &text[start..end]
}

fn ascii_starts_with_any(text: &[u8], first: &[u8], second: &[u8]) -> bool {
    text.starts_with(first) || text.starts_with(second)
}

fn decimal_base_zero_allows_leading_zero(unsigned: &[u8], cleaned: &[u8]) -> bool {
    if !unsigned.starts_with(b"0") {
        return true;
    }
    cleaned.iter().all(|ch| *ch == b'0')
}

fn clean_int_digits(digits: &[u8], base: u32, allow_prefix_underscore: bool) -> Option<Vec<u8>> {
    if digits.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(digits.len());
    let mut prev_digit = false;
    let mut saw_digit = false;
    for (i, byte) in digits.iter().copied().enumerate() {
        if byte == b'_' {
            if !prev_digit && !(allow_prefix_underscore && i == 0) {
                return None;
            }
            prev_digit = false;
            continue;
        }
        let Some(value) = ascii_digit_value(byte) else {
            return None;
        };
        if value >= base {
            return None;
        }
        out.push(byte);
        prev_digit = true;
        saw_digit = true;
    }
    if !saw_digit || !prev_digit {
        return None;
    }
    Some(out)
}

fn ascii_digit_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as u32),
        b'a'..=b'z' => Some((byte - b'a' + 10) as u32),
        b'A'..=b'Z' => Some((byte - b'A' + 10) as u32),
        _ => None,
    }
}

fn int_literal_repr(obj: &PyObjectRef) -> String {
    match &obj.payload {
        PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => {
            obj.repr()
        }
        _ => ferrython_core::object::py_ascii_repr(obj),
    }
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
            PyObjectPayload::BuiltinFunction(name) if name.as_str() == "enumerate" => {}
            PyObjectPayload::NativeFunction(nf) if nf.name.as_str() == "datetime.time" => {}
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
