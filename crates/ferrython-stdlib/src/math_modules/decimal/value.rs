use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::{
    float_as_integer_ratio, py_hash_float, py_hash_rational, PyInt, PY_HASH_INF,
};
use num_bigint::BigInt;
use num_traits::{One, ToPrimitive, Zero};

pub(super) fn get_decimal_str(obj: &PyObjectRef) -> Option<String> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        if let Some(v) = attrs.get("_value") {
            return v.as_str().map(|s| s.to_string());
        }
    }
    if let PyObjectPayload::Int(n) = &obj.payload {
        return Some(format!("{}", n));
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

fn decimal_ratio(s: &str) -> Option<(BigInt, BigInt)> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("nan") {
        return None;
    }
    if s.eq_ignore_ascii_case("infinity") || s.eq_ignore_ascii_case("+infinity") {
        return Some((BigInt::one(), BigInt::zero()));
    }
    if s.eq_ignore_ascii_case("-infinity") {
        return Some((-BigInt::one(), BigInt::zero()));
    }
    let (negative, body) = if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else {
        (false, s)
    };
    let (mantissa, exp) =
        if let Some((m, e)) = body.split_once('e').or_else(|| body.split_once('E')) {
            (m, e.parse::<i64>().ok()?)
        } else {
            (body, 0)
        };
    let mut digits = String::new();
    let mut scale = 0i64;
    if let Some((int_part, frac_part)) = mantissa.split_once('.') {
        digits.push_str(int_part);
        digits.push_str(frac_part);
        scale = frac_part.len() as i64;
    } else {
        digits.push_str(mantissa);
    }
    if digits.is_empty() {
        return Some((BigInt::zero(), BigInt::one()));
    }
    let mut numerator = digits.parse::<BigInt>().ok()?;
    if negative {
        numerator = -numerator;
    }
    let power = scale - exp;
    if power.abs() > 10_000 {
        if numerator.is_zero() {
            return Some((BigInt::zero(), BigInt::one()));
        }
        return None;
    }
    if power >= 0 {
        Some((numerator, BigInt::from(10u8).pow(power as u32)))
    } else {
        Some((
            numerator * BigInt::from(10u8).pow((-power) as u32),
            BigInt::one(),
        ))
    }
}

fn decimal_extreme_parts(s: &str) -> Option<(bool, i64, usize)> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("nan")
        || s.eq_ignore_ascii_case("infinity")
        || s.eq_ignore_ascii_case("+infinity")
        || s.eq_ignore_ascii_case("-infinity")
        || s.eq_ignore_ascii_case("inf")
        || s.eq_ignore_ascii_case("+inf")
        || s.eq_ignore_ascii_case("-inf")
    {
        return None;
    }
    let (negative, body) = if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else {
        (false, s)
    };
    let (mantissa, exp) =
        if let Some((m, e)) = body.split_once('e').or_else(|| body.split_once('E')) {
            (m, e.parse::<i64>().ok()?)
        } else {
            (body, 0)
        };
    let digit_count = mantissa.chars().filter(|c| c.is_ascii_digit()).count();
    let frac_count = mantissa
        .split_once('.')
        .map(|(_, frac)| frac.chars().filter(|c| c.is_ascii_digit()).count())
        .unwrap_or(0);
    if digit_count == 0 {
        return Some((false, 0, 1));
    }
    let magnitude = exp - frac_count as i64 + digit_count as i64;
    Some((negative, magnitude, digit_count))
}

fn object_attr_bigint(obj: &PyObjectRef) -> Option<BigInt> {
    match &obj.payload {
        PyObjectPayload::Bool(flag) => Some(BigInt::from(if *flag { 1 } else { 0 })),
        PyObjectPayload::Int(PyInt::Small(n)) => Some(BigInt::from(*n)),
        PyObjectPayload::Int(PyInt::Big(n)) => Some(n.as_ref().clone()),
        _ => None,
    }
}

fn object_ratio(obj: &PyObjectRef) -> Option<(BigInt, BigInt)> {
    match &obj.payload {
        PyObjectPayload::Bool(flag) => {
            Some((BigInt::from(if *flag { 1 } else { 0 }), BigInt::one()))
        }
        PyObjectPayload::Int(PyInt::Small(n)) => Some((BigInt::from(*n), BigInt::one())),
        PyObjectPayload::Int(PyInt::Big(n)) => Some((n.as_ref().clone(), BigInt::one())),
        PyObjectPayload::Float(f) if f.is_finite() => Some(float_as_integer_ratio(*f)),
        PyObjectPayload::Float(f) if f.is_infinite() => Some((
            if f.is_sign_negative() {
                -BigInt::one()
            } else {
                BigInt::one()
            },
            BigInt::zero(),
        )),
        PyObjectPayload::Complex { real, imag } if *imag == 0.0 && real.is_finite() => {
            Some(float_as_integer_ratio(*real))
        }
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__decimal__") {
                if let (Some(n), Some(d)) = (
                    attrs.get("_ratio_num").and_then(object_attr_bigint),
                    attrs.get("_ratio_den").and_then(object_attr_bigint),
                ) {
                    return Some((n, d));
                }
                attrs
                    .get("_value")
                    .and_then(|v| v.as_str())
                    .and_then(decimal_ratio)
            } else if attrs.contains_key("__fraction__") {
                let n = attrs.get("numerator").and_then(|v| match &v.payload {
                    PyObjectPayload::Int(PyInt::Small(n)) => Some(BigInt::from(*n)),
                    PyObjectPayload::Int(PyInt::Big(n)) => Some(n.as_ref().clone()),
                    PyObjectPayload::Bool(flag) => Some(BigInt::from(if *flag { 1 } else { 0 })),
                    _ => None,
                })?;
                let d = attrs.get("denominator").and_then(|v| match &v.payload {
                    PyObjectPayload::Int(PyInt::Small(n)) => Some(BigInt::from(*n)),
                    PyObjectPayload::Int(PyInt::Big(n)) => Some(n.as_ref().clone()),
                    PyObjectPayload::Bool(flag) => Some(BigInt::from(if *flag { 1 } else { 0 })),
                    _ => None,
                })?;
                Some((n, d))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn numeric_approx(obj: &PyObjectRef) -> Option<f64> {
    match &obj.payload {
        PyObjectPayload::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        PyObjectPayload::Int(n) => Some(n.to_f64()),
        PyObjectPayload::Float(f) => Some(*f),
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__decimal__") {
                attrs
                    .get("_value")
                    .and_then(|v| v.as_str())
                    .and_then(|s| match s.parse::<f64>() {
                        Ok(f) if f.is_infinite() && decimal_extreme_parts(s).is_some() => None,
                        Ok(f) => Some(f),
                        Err(_) => None,
                    })
            } else if attrs.contains_key("__fraction__") {
                let n = attrs.get("numerator").and_then(|v| match &v.payload {
                    PyObjectPayload::Int(PyInt::Small(n)) => Some(*n as f64),
                    PyObjectPayload::Int(PyInt::Big(n)) => n.to_f64(),
                    PyObjectPayload::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
                    _ => None,
                })?;
                let d = attrs.get("denominator").and_then(|v| match &v.payload {
                    PyObjectPayload::Int(PyInt::Small(n)) => Some(*n as f64),
                    PyObjectPayload::Int(PyInt::Big(n)) => n.to_f64(),
                    PyObjectPayload::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
                    _ => None,
                })?;
                Some(n / d)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn compare_extreme_decimal_to_float(decimal: &str, float: f64) -> Option<std::cmp::Ordering> {
    if !float.is_finite() {
        return None;
    }
    let (negative, magnitude, _) = decimal_extreme_parts(decimal)?;
    if magnitude > 309 {
        return Some(if negative {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        });
    }
    None
}

fn compare_extreme_decimal_to_numeric(
    decimal: &str,
    other: &PyObjectRef,
) -> Option<std::cmp::Ordering> {
    let (negative, magnitude, _) = decimal_extreme_parts(decimal)?;
    if magnitude <= 309 {
        return None;
    }
    match &other.payload {
        PyObjectPayload::Bool(_)
        | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_)
        | PyObjectPayload::Instance(_) => Some(if negative {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }),
        _ => None,
    }
}

fn compare_ratios(a: &PyObjectRef, b: &PyObjectRef) -> Option<std::cmp::Ordering> {
    let left = object_ratio(a);
    let right = object_ratio(b);
    let (an, ad, bn, bd) = match (left, right) {
        (Some((an, ad)), Some((bn, bd))) => (an, ad, bn, bd),
        _ => {
            if let (PyObjectPayload::Float(af), Some(ds)) = (&a.payload, get_decimal_str(b)) {
                if af.is_infinite() && decimal_extreme_parts(&ds).is_some() {
                    return if af.is_sign_negative() {
                        Some(std::cmp::Ordering::Less)
                    } else {
                        Some(std::cmp::Ordering::Greater)
                    };
                }
            }
            if let (Some(ds), PyObjectPayload::Float(bf)) = (get_decimal_str(a), &b.payload) {
                if bf.is_infinite() && decimal_extreme_parts(&ds).is_some() {
                    return if bf.is_sign_negative() {
                        Some(std::cmp::Ordering::Greater)
                    } else {
                        Some(std::cmp::Ordering::Less)
                    };
                }
                if let Some(ordering) = compare_extreme_decimal_to_float(&ds, *bf) {
                    return Some(ordering);
                }
            }
            if let (PyObjectPayload::Float(af), Some(ds)) = (&a.payload, get_decimal_str(b)) {
                if let Some(ordering) = compare_extreme_decimal_to_float(&ds, *af) {
                    return Some(ordering.reverse());
                }
            }
            if let Some(ds) = get_decimal_str(a) {
                if let Some(ordering) = compare_extreme_decimal_to_numeric(&ds, b) {
                    return Some(ordering);
                }
            }
            if let Some(ds) = get_decimal_str(b) {
                if let Some(ordering) = compare_extreme_decimal_to_numeric(&ds, a) {
                    return Some(ordering.reverse());
                }
            }
            let af = numeric_approx(a)?;
            let bf = numeric_approx(b)?;
            return af.partial_cmp(&bf);
        }
    };
    if ad.is_zero() || bd.is_zero() {
        return if ad.is_zero() && bd.is_zero() {
            an.sign().partial_cmp(&bn.sign())
        } else if ad.is_zero() {
            if an.sign() == num_bigint::Sign::Minus {
                Some(std::cmp::Ordering::Less)
            } else {
                Some(std::cmp::Ordering::Greater)
            }
        } else if bn.sign() == num_bigint::Sign::Minus {
            Some(std::cmp::Ordering::Greater)
        } else {
            Some(std::cmp::Ordering::Less)
        };
    }
    Some((an * bd).cmp(&(bn * ad)))
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
    Ok(PyObject::bool_val(
        compare_ratios(&args[0], &args[1]) == Some(std::cmp::Ordering::Equal),
    ))
}

pub(super) fn decimal_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        compare_ratios(&args[0], &args[1]) == Some(std::cmp::Ordering::Less),
    ))
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
    Ok(PyObject::bool_val(matches!(
        compare_ratios(&args[0], &args[1]),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    )))
}

pub(super) fn decimal_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        compare_ratios(&args[0], &args[1]) == Some(std::cmp::Ordering::Greater),
    ))
}

pub(super) fn decimal_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(matches!(
        compare_ratios(&args[0], &args[1]),
        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
    )))
}

pub(super) fn decimal_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let s = args
        .first()
        .and_then(get_decimal_str)
        .unwrap_or_else(|| "0".to_string());
    Ok(PyObject::str_val(CompactString::from(s)))
}

pub(super) fn decimal_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some((n, d)) = args.first().and_then(object_ratio) {
        return Ok(PyObject::int(py_hash_rational(&n, &d)));
    }
    let s = args
        .first()
        .and_then(get_decimal_str)
        .unwrap_or_else(|| "0".to_string());
    if s.eq_ignore_ascii_case("nan")
        || s.eq_ignore_ascii_case("+nan")
        || s.eq_ignore_ascii_case("-nan")
    {
        return Ok(PyObject::int(0));
    }
    if s.eq_ignore_ascii_case("infinity") || s.eq_ignore_ascii_case("+infinity") {
        return Ok(PyObject::int(PY_HASH_INF));
    }
    if s.eq_ignore_ascii_case("-infinity") {
        return Ok(PyObject::int(-PY_HASH_INF));
    }
    if let Some((n, d)) = decimal_ratio(&s) {
        return Ok(PyObject::int(py_hash_rational(&n, &d)));
    }
    let f: f64 = s.parse().unwrap_or(0.0);
    Ok(PyObject::int(py_hash_float(f)))
}
