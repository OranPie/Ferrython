use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

pub(super) fn get_decimal_str(obj: &PyObjectRef) -> Option<String> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        if let Some(v) = attrs.get("_value") {
            return v.as_str().map(|s| s.to_string());
        }
    }
    if let PyObjectPayload::Int(n) = &obj.payload {
        return Some(format!("{}", n.to_i64().unwrap_or(0)));
    }
    if let PyObjectPayload::Float(f) = &obj.payload {
        return Some(format!("{}", f));
    }
    None
}

pub(super) fn decimal_parse(s: &str) -> (bool, i128, u32) {
    let s = s.trim();
    let (neg, s) = if s.starts_with('-') {
        (true, &s[1..])
    } else if s.starts_with('+') {
        (false, &s[1..])
    } else {
        (false, s)
    };
    if let Some(dot_pos) = s.find('.') {
        let int_part = &s[..dot_pos];
        let frac_part = &s[dot_pos + 1..];
        let scale = frac_part.len() as u32;
        let digits_str = format!("{}{}", int_part, frac_part);
        let digits: i128 = digits_str.parse().unwrap_or(0);
        (neg, digits, scale)
    } else {
        let digits: i128 = s.parse().unwrap_or(0);
        (neg, digits, 0)
    }
}

pub(super) fn decimal_format(neg: bool, digits: i128, scale: u32) -> String {
    // CPython Decimal preserves trailing zeros to maintain precision
    if scale == 0 {
        if neg && digits != 0 {
            format!("-{}", digits)
        } else {
            format!("{}", digits)
        }
    } else {
        let s = format!("{:0>width$}", digits, width = scale as usize + 1);
        let (int_part, frac_part) = s.split_at(s.len() - scale as usize);
        if neg && digits != 0 {
            format!("-{}.{}", int_part, frac_part)
        } else {
            format!("{}.{}", int_part, frac_part)
        }
    }
}

pub(super) fn align_scales(
    a: (bool, i128, u32),
    b: (bool, i128, u32),
) -> ((bool, i128, u32), (bool, i128, u32)) {
    let max_scale = a.2.max(b.2);
    let a_digits = a.1 * 10i128.pow(max_scale - a.2);
    let b_digits = b.1 * 10i128.pow(max_scale - b.2);
    ((a.0, a_digits, max_scale), (b.0, b_digits, max_scale))
}

pub(super) fn decimal_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    let a = get_decimal_str(&args[0]);
    let b = get_decimal_str(&args[1]);
    match (a, b) {
        (Some(a), Some(b)) => {
            let ap = decimal_parse(&a);
            let bp = decimal_parse(&b);
            let (ap, bp) = align_scales(ap, bp);
            let a_val = if ap.0 { -(ap.1) } else { ap.1 };
            let b_val = if bp.0 { -(bp.1) } else { bp.1 };
            Ok(PyObject::bool_val(a_val == b_val))
        }
        _ => Ok(PyObject::bool_val(false)),
    }
}

pub(super) fn decimal_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    let a = get_decimal_str(&args[0]);
    let b = get_decimal_str(&args[1]);
    match (a, b) {
        (Some(a), Some(b)) => {
            let ap = decimal_parse(&a);
            let bp = decimal_parse(&b);
            let (ap, bp) = align_scales(ap, bp);
            let a_val = if ap.0 { -(ap.1) } else { ap.1 };
            let b_val = if bp.0 { -(bp.1) } else { bp.1 };
            Ok(PyObject::bool_val(a_val < b_val))
        }
        _ => Ok(PyObject::bool_val(false)),
    }
}

pub(super) fn decimal_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let s = args
        .first()
        .and_then(get_decimal_str)
        .unwrap_or_else(|| "0".to_string());
    let f: f64 = s.parse().unwrap_or(0.0);
    Ok(PyObject::float(f))
}

pub(super) fn decimal_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let s = args
        .first()
        .and_then(get_decimal_str)
        .unwrap_or_else(|| "0".to_string());
    let (neg, digits, scale) = decimal_parse(&s);
    let int_val = digits / 10i128.pow(scale);
    Ok(PyObject::int(if neg {
        -(int_val as i64)
    } else {
        int_val as i64
    }))
}

pub(super) fn decimal_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    let (a, b) = (get_decimal_str(&args[0]), get_decimal_str(&args[1]));
    match (a, b) {
        (Some(a), Some(b)) => {
            let (ap, bp) = align_scales(decimal_parse(&a), decimal_parse(&b));
            let a_val = if ap.0 { -(ap.1) } else { ap.1 };
            let b_val = if bp.0 { -(bp.1) } else { bp.1 };
            Ok(PyObject::bool_val(a_val <= b_val))
        }
        _ => Ok(PyObject::bool_val(false)),
    }
}

pub(super) fn decimal_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    let (a, b) = (get_decimal_str(&args[0]), get_decimal_str(&args[1]));
    match (a, b) {
        (Some(a), Some(b)) => {
            let (ap, bp) = align_scales(decimal_parse(&a), decimal_parse(&b));
            let a_val = if ap.0 { -(ap.1) } else { ap.1 };
            let b_val = if bp.0 { -(bp.1) } else { bp.1 };
            Ok(PyObject::bool_val(a_val > b_val))
        }
        _ => Ok(PyObject::bool_val(false)),
    }
}

pub(super) fn decimal_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    let (a, b) = (get_decimal_str(&args[0]), get_decimal_str(&args[1]));
    match (a, b) {
        (Some(a), Some(b)) => {
            let (ap, bp) = align_scales(decimal_parse(&a), decimal_parse(&b));
            let a_val = if ap.0 { -(ap.1) } else { ap.1 };
            let b_val = if bp.0 { -(bp.1) } else { bp.1 };
            Ok(PyObject::bool_val(a_val >= b_val))
        }
        _ => Ok(PyObject::bool_val(false)),
    }
}

pub(super) fn decimal_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let s = args
        .first()
        .and_then(get_decimal_str)
        .unwrap_or_else(|| "0".to_string());
    Ok(PyObject::str_val(CompactString::from(s)))
}

pub(super) fn decimal_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let s = args
        .first()
        .and_then(get_decimal_str)
        .unwrap_or_else(|| "0".to_string());
    let f: f64 = s.parse().unwrap_or(0.0);
    Ok(PyObject::int(f.to_bits() as i64))
}
