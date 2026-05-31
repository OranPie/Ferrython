use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::helpers::{make_builtin, BuiltinFn};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::float_as_integer_ratio;
use indexmap::IndexMap;
use num_bigint::BigInt;

use super::value::get_decimal_str;

pub(super) fn add_extended_decimal_methods(
    dec_ns: &mut IndexMap<CompactString, PyObjectRef>,
    make_decimal: fn(&str) -> PyObjectRef,
    make_decimal_from_ratio: fn(&str, BigInt, BigInt) -> PyObjectRef,
    decimal_quantize: BuiltinFn,
    decimal_sqrt: BuiltinFn,
    decimal_ln: BuiltinFn,
    decimal_exp: BuiltinFn,
    decimal_is_zero: BuiltinFn,
    decimal_is_nan: BuiltinFn,
    decimal_is_infinite: BuiltinFn,
    decimal_to_eng_string: BuiltinFn,
) {
    dec_ns.insert(
        CompactString::from("quantize"),
        make_builtin(decimal_quantize),
    );
    dec_ns.insert(CompactString::from("sqrt"), make_builtin(decimal_sqrt));
    dec_ns.insert(CompactString::from("ln"), make_builtin(decimal_ln));
    dec_ns.insert(CompactString::from("exp"), make_builtin(decimal_exp));
    dec_ns.insert(
        CompactString::from("is_zero"),
        make_builtin(decimal_is_zero),
    );
    dec_ns.insert(CompactString::from("is_nan"), make_builtin(decimal_is_nan));
    dec_ns.insert(
        CompactString::from("is_infinite"),
        make_builtin(decimal_is_infinite),
    );
    dec_ns.insert(
        CompactString::from("is_finite"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(true));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let v: f64 = s.parse().unwrap_or(0.0);
            Ok(PyObject::bool_val(v.is_finite()))
        }),
    );
    dec_ns.insert(
        CompactString::from("is_signed"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            Ok(PyObject::bool_val(s.starts_with('-')))
        }),
    );
    dec_ns.insert(
        CompactString::from("is_normal"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let v: f64 = s.parse().unwrap_or(0.0);
            Ok(PyObject::bool_val(v.is_normal()))
        }),
    );
    dec_ns.insert(
        CompactString::from("is_subnormal"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let v: f64 = s.parse().unwrap_or(0.0);
            Ok(PyObject::bool_val(v.is_subnormal()))
        }),
    );
    dec_ns.insert(
        CompactString::from("copy_abs"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("copy_abs requires self"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let s = s.trim_start_matches('-');
            Ok(make_decimal(s))
        }),
    );
    dec_ns.insert(
        CompactString::from("copy_negate"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("copy_negate requires self"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let neg = if s.starts_with('-') {
                s[1..].to_string()
            } else {
                format!("-{}", s)
            };
            Ok(make_decimal(&neg))
        }),
    );
    dec_ns.insert(
        CompactString::from("normalize"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("normalize requires self"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            if s.contains('.') {
                let trimmed = s.trim_end_matches('0').trim_end_matches('.');
                Ok(make_decimal(trimmed))
            } else {
                Ok(make_decimal(&s))
            }
        }),
    );
    dec_ns.insert(
        CompactString::from("adjusted"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let s = s.trim_start_matches('-');
            if s.contains('.') {
                let parts: Vec<&str> = s.split('.').collect();
                let digits = parts[0].trim_start_matches('0');
                if digits.is_empty() {
                    let frac = parts.get(1).unwrap_or(&"");
                    let leading_zeros = frac.len() - frac.trim_start_matches('0').len();
                    Ok(PyObject::int(-(leading_zeros as i64 + 1)))
                } else {
                    Ok(PyObject::int((digits.len() as i64) - 1))
                }
            } else {
                let digits = s.trim_start_matches('0');
                Ok(PyObject::int((digits.len().max(1) as i64) - 1))
            }
        }),
    );
    dec_ns.insert(
        CompactString::from("to_eng_string"),
        make_builtin(decimal_to_eng_string),
    );
    dec_ns.insert(
        CompactString::from("from_float"),
        PyObject::native_closure("Decimal.from_float", move |args: &[PyObjectRef]| {
            let value = args
                .iter()
                .find(|arg| !matches!(arg.payload, PyObjectPayload::Class(_)))
                .ok_or_else(|| PyException::type_error("from_float requires an argument"))?;
            match &value.payload {
                PyObjectPayload::Float(f) => {
                    if f.is_nan() {
                        return Ok(make_decimal("NaN"));
                    }
                    if f.is_infinite() {
                        return Ok(if f.is_sign_negative() {
                            make_decimal("-Infinity")
                        } else {
                            make_decimal("Infinity")
                        });
                    }
                    let (n, d) = float_as_integer_ratio(*f);
                    let display = format!("{}", f);
                    Ok(make_decimal_from_ratio(&display, n, d))
                }
                PyObjectPayload::Int(n) => Ok(make_decimal(&format!("{}", n))),
                _ => Err(PyException::type_error("argument must be int or float")),
            }
        }),
    );
    // as_tuple() → DecimalTuple(sign, digits, exponent)
    dec_ns.insert(
        CompactString::from("as_tuple"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("as_tuple requires self"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let sign = if s.starts_with('-') { 1i64 } else { 0i64 };
            let abs_s = s.trim_start_matches('-').trim_start_matches('+');
            if abs_s == "NaN" {
                return Ok(PyObject::tuple(vec![
                    PyObject::int(0),
                    PyObject::tuple(vec![]),
                    PyObject::str_val(CompactString::from("n")),
                ]));
            }
            if abs_s == "Infinity" {
                return Ok(PyObject::tuple(vec![
                    PyObject::int(sign),
                    PyObject::tuple(vec![]),
                    PyObject::str_val(CompactString::from("F")),
                ]));
            }
            let (digits_str, exponent) = if abs_s.contains('.') {
                let parts: Vec<&str> = abs_s.splitn(2, '.').collect();
                let full = format!("{}{}", parts[0], parts.get(1).unwrap_or(&""));
                let exp = -(parts.get(1).map(|s| s.len()).unwrap_or(0) as i64);
                (full, exp)
            } else if abs_s.contains('E') || abs_s.contains('e') {
                let parts: Vec<&str> = abs_s.splitn(2, |c: char| c == 'E' || c == 'e').collect();
                let exp: i64 = parts.get(1).unwrap_or(&"0").parse().unwrap_or(0);
                (parts[0].replace('.', ""), exp)
            } else {
                (abs_s.to_string(), 0i64)
            };
            let digit_objs: Vec<PyObjectRef> = digits_str
                .chars()
                .filter(|c| c.is_ascii_digit())
                .map(|c| PyObject::int((c as u8 - b'0') as i64))
                .collect();
            Ok(PyObject::tuple(vec![
                PyObject::int(sign),
                PyObject::tuple(digit_objs),
                PyObject::int(exponent),
            ]))
        }),
    );
    // copy_sign(other) → Decimal with sign of other
    dec_ns.insert(
        CompactString::from("copy_sign"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("copy_sign requires self and other"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let other_s = get_decimal_str(&args[1]).unwrap_or_else(|| args[1].py_to_string());
            let abs_val = s.trim_start_matches('-').trim_start_matches('+');
            if other_s.starts_with('-') {
                Ok(make_decimal(&format!("-{}", abs_val)))
            } else {
                Ok(make_decimal(abs_val))
            }
        }),
    );
    // __pow__
    dec_ns.insert(
        CompactString::from("__pow__"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("__pow__ requires two arguments"));
            }
            let a = get_decimal_str(&args[0])
                .unwrap_or_default()
                .parse::<f64>()
                .unwrap_or(0.0);
            let b = get_decimal_str(&args[1])
                .unwrap_or_else(|| args[1].py_to_string())
                .parse::<f64>()
                .unwrap_or(0.0);
            Ok(make_decimal(&format!("{}", a.powf(b))))
        }),
    );
    // __mod__
    dec_ns.insert(
        CompactString::from("__mod__"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("__mod__ requires two arguments"));
            }
            let a = get_decimal_str(&args[0])
                .unwrap_or_default()
                .parse::<f64>()
                .unwrap_or(0.0);
            let b = get_decimal_str(&args[1])
                .unwrap_or_else(|| args[1].py_to_string())
                .parse::<f64>()
                .unwrap_or(1.0);
            if b == 0.0 {
                return Err(PyException::zero_division_error("decimal modulo by zero"));
            }
            let r = a % b;
            Ok(make_decimal(&format!("{}", r)))
        }),
    );
    // __floordiv__
    dec_ns.insert(
        CompactString::from("__floordiv__"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "__floordiv__ requires two arguments",
                ));
            }
            let a = get_decimal_str(&args[0])
                .unwrap_or_default()
                .parse::<f64>()
                .unwrap_or(0.0);
            let b = get_decimal_str(&args[1])
                .unwrap_or_else(|| args[1].py_to_string())
                .parse::<f64>()
                .unwrap_or(1.0);
            if b == 0.0 {
                return Err(PyException::zero_division_error(
                    "decimal floor division by zero",
                ));
            }
            Ok(make_decimal(&format!("{}", (a / b).floor())))
        }),
    );
    // __bool__
    dec_ns.insert(
        CompactString::from("__bool__"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let v: f64 = s.parse().unwrap_or(0.0);
            Ok(PyObject::bool_val(v != 0.0))
        }),
    );
    // __round__
    dec_ns.insert(
        CompactString::from("__round__"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(make_decimal("0"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let v: f64 = s.parse().unwrap_or(0.0);
            let ndigits = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
            let factor = 10f64.powi(ndigits as i32);
            let rounded = (v * factor).round() / factor;
            Ok(make_decimal(&format!("{}", rounded)))
        }),
    );
    // max / min
    dec_ns.insert(
        CompactString::from("max"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("max requires self and other"));
            }
            let a = get_decimal_str(&args[0])
                .unwrap_or_default()
                .parse::<f64>()
                .unwrap_or(0.0);
            let b = get_decimal_str(&args[1])
                .unwrap_or_else(|| args[1].py_to_string())
                .parse::<f64>()
                .unwrap_or(0.0);
            Ok(if a >= b {
                args[0].clone()
            } else {
                args[1].clone()
            })
        }),
    );
    dec_ns.insert(
        CompactString::from("min"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("min requires self and other"));
            }
            let a = get_decimal_str(&args[0])
                .unwrap_or_default()
                .parse::<f64>()
                .unwrap_or(0.0);
            let b = get_decimal_str(&args[1])
                .unwrap_or_else(|| args[1].py_to_string())
                .parse::<f64>()
                .unwrap_or(0.0);
            Ok(if a <= b {
                args[0].clone()
            } else {
                args[1].clone()
            })
        }),
    );
    // compare(other) → Decimal(-1, 0, or 1)
    dec_ns.insert(
        CompactString::from("compare"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("compare requires self and other"));
            }
            let a = get_decimal_str(&args[0])
                .unwrap_or_default()
                .parse::<f64>()
                .unwrap_or(0.0);
            let b = get_decimal_str(&args[1])
                .unwrap_or_else(|| args[1].py_to_string())
                .parse::<f64>()
                .unwrap_or(0.0);
            let result = if a < b {
                -1
            } else if a > b {
                1
            } else {
                0
            };
            Ok(make_decimal(&format!("{}", result)))
        }),
    );
    dec_ns.insert(
        CompactString::from("conjugate"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("conjugate requires self"));
            }
            Ok(args[0].clone())
        }),
    );
    dec_ns.insert(
        CompactString::from("radix"),
        PyObject::native_closure("Decimal.radix", move |_| Ok(make_decimal("10"))),
    );
    dec_ns.insert(
        CompactString::from("to_integral_value"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(make_decimal("0"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let v: f64 = s.parse().unwrap_or(0.0);
            Ok(make_decimal(&format!("{}", v.round() as i64)))
        }),
    );
    dec_ns.insert(
        CompactString::from("to_integral_exact"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(make_decimal("0"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let v: f64 = s.parse().unwrap_or(0.0);
            Ok(make_decimal(&format!("{}", v.round() as i64)))
        }),
    );
    dec_ns.insert(
        CompactString::from("log10"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("log10 requires self"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let v: f64 = s.parse().unwrap_or(1.0);
            Ok(make_decimal(&format!("{}", v.log10())))
        }),
    );
    dec_ns.insert(
        CompactString::from("logb"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("logb requires self"));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let v: f64 = s.parse().unwrap_or(1.0);
            let abs_v = v.abs();
            if abs_v == 0.0 {
                return Err(PyException::value_error("logarithm of zero"));
            }
            Ok(make_decimal(&format!("{}", abs_v.log10().floor() as i64)))
        }),
    );
    dec_ns.insert(
        CompactString::from("fma"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.len() < 3 {
                return Err(PyException::type_error("fma requires self, other, third"));
            }
            let a = get_decimal_str(&args[0])
                .unwrap_or_default()
                .parse::<f64>()
                .unwrap_or(0.0);
            let b = get_decimal_str(&args[1])
                .unwrap_or_else(|| args[1].py_to_string())
                .parse::<f64>()
                .unwrap_or(0.0);
            let c = get_decimal_str(&args[2])
                .unwrap_or_else(|| args[2].py_to_string())
                .parse::<f64>()
                .unwrap_or(0.0);
            Ok(make_decimal(&format!("{}", a * b + c)))
        }),
    );
    dec_ns.insert(
        CompactString::from("same_quantum"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "same_quantum requires self and other",
                ));
            }
            let a = get_decimal_str(&args[0]).unwrap_or_default();
            let b = get_decimal_str(&args[1]).unwrap_or_else(|| args[1].py_to_string());
            let exp_a = if a.contains('.') {
                a.split('.').nth(1).map(|s| s.len()).unwrap_or(0)
            } else {
                0
            };
            let exp_b = if b.contains('.') {
                b.split('.').nth(1).map(|s| s.len()).unwrap_or(0)
            } else {
                0
            };
            Ok(PyObject::bool_val(exp_a == exp_b))
        }),
    );
    dec_ns.insert(
        CompactString::from("number_class"),
        PyObject::native_closure("Decimal.method", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("+Zero")));
            }
            let s = get_decimal_str(&args[0]).unwrap_or_default();
            let lower = s.to_lowercase();
            let result = if lower.contains("nan") {
                "NaN"
            } else if lower.contains("infinity") || lower.contains("inf") {
                if s.starts_with('-') {
                    "-Infinity"
                } else {
                    "+Infinity"
                }
            } else {
                let v: f64 = s.parse().unwrap_or(0.0);
                if v == 0.0 {
                    if s.starts_with('-') {
                        "-Zero"
                    } else {
                        "+Zero"
                    }
                } else if v < 0.0 {
                    "-Normal"
                } else {
                    "+Normal"
                }
            };
            Ok(PyObject::str_val(CompactString::from(result)))
        }),
    );
    // __new__ enables Decimal("1.23") to work when called as class constructor
    dec_ns.insert(
        CompactString::from("__new__"),
        PyObject::native_closure("Decimal.__new__", move |args: &[PyObjectRef]| {
            // args[0] = cls, args[1..] = constructor args
            if args.len() < 2 {
                return Ok(make_decimal("0"));
            }
            let s = args[1].py_to_string();
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Ok(make_decimal("0"));
            }
            match &args[1].payload {
                PyObjectPayload::Int(n) => return Ok(make_decimal(&format!("{}", n))),
                PyObjectPayload::Float(f) => return Ok(make_decimal(&format!("{}", f))),
                _ => {}
            }
            if let PyObjectPayload::Instance(inst) = &args[1].payload {
                if let Some(v) = inst.attrs.read().get("_value") {
                    if let Some(sv) = v.as_str() {
                        return Ok(make_decimal(&sv.to_string()));
                    }
                }
            }
            let check = trimmed.trim_start_matches('+').trim_start_matches('-');
            let check_lower = check.to_lowercase();
            let mantissa = check
                .split_once('e')
                .or_else(|| check.split_once('E'))
                .map(|(m, _)| m)
                .unwrap_or(check);
            let parts: Vec<&str> = mantissa.splitn(2, '.').collect();
            let valid = parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
                && check
                    .split_once('e')
                    .or_else(|| check.split_once('E'))
                    .map(|(_, e)| e.parse::<i64>().is_ok())
                    .unwrap_or(true)
                || check == "Infinity"
                || check == "NaN"
                || check_lower == "inf"
                || check_lower == "infinity"
                || check_lower == "nan"
                || check_lower == "snan";
            if valid {
                // Normalize special values
                let normalized = if check_lower == "inf" || check_lower == "infinity" {
                    let sign = if trimmed.starts_with('-') { "-" } else { "" };
                    format!("{}Infinity", sign)
                } else if check_lower == "nan" || check_lower == "snan" {
                    let sign = if trimmed.starts_with('-') { "-" } else { "" };
                    format!("{}NaN", sign)
                } else {
                    trimmed.to_string()
                };
                Ok(make_decimal(&normalized))
            } else {
                Err(PyException::value_error(format!(
                    "Invalid literal for Decimal: '{}'",
                    s
                )))
            }
        }),
    );
}
