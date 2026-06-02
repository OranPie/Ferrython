//! Numeric scalar type method dispatch.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::{
    float_as_integer_ratio, py_hash_bigint, py_hash_float, HashableKey, PyInt,
};
use num_bigint::{BigInt, Sign};
use num_traits::{One, Signed};

pub(crate) fn call_int_method(
    _receiver: &PyObjectRef,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "bit_length" => {
            let n = match &_receiver.payload {
                PyObjectPayload::Bool(flag) => BigInt::from(if *flag { 1 } else { 0 }),
                PyObjectPayload::Int(PyInt::Small(value)) => BigInt::from(*value),
                PyObjectPayload::Int(PyInt::Big(value)) => value.as_ref().clone(),
                _ => BigInt::from(_receiver.to_int()?),
            };
            if n == BigInt::from(0u8) {
                Ok(PyObject::int(0))
            } else {
                Ok(PyObject::int(n.abs().to_str_radix(2).len() as i64))
            }
        }
        "bit_count" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::int(n.abs().count_ones() as i64))
        }
        "to_bytes" => {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "to_bytes() requires at least 1 argument",
                ));
            }
            let length_int = args[0].to_int()?;
            if length_int < 0 {
                return Err(PyException::value_error(
                    "length argument must be non-negative",
                ));
            }
            let length = length_int as usize;
            // Extract byteorder and signed from positional or kwargs dict
            let mut byteorder = "big".to_string();
            let mut signed = false;
            let mut _kwarg_start = 1;
            // Check if last arg is a kwargs dict
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(map) = &last.payload {
                    let map_r = map.read();
                    if let Some(bo) =
                        map_r.get(&HashableKey::str_key(CompactString::from("byteorder")))
                    {
                        byteorder = bo.py_to_string();
                    }
                    if let Some(s) = map_r.get(&HashableKey::str_key(CompactString::from("signed")))
                    {
                        signed = s.is_truthy();
                    }
                    _kwarg_start = args.len(); // skip kwargs dict for positional scan
                }
            }
            if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::Dict(_)) {
                byteorder = args[1].py_to_string();
            }
            if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::Dict(_)) {
                signed = args[2].is_truthy();
            }

            let n = match &_receiver.payload {
                PyObjectPayload::Bool(flag) => BigInt::from(if *flag { 1 } else { 0 }),
                PyObjectPayload::Int(PyInt::Small(value)) => BigInt::from(*value),
                PyObjectPayload::Int(PyInt::Big(value)) => value.as_ref().clone(),
                _ => BigInt::from(_receiver.to_int()?),
            };
            if byteorder != "big" && byteorder != "little" {
                return Err(PyException::value_error(
                    "byteorder must be 'big' or 'little'",
                ));
            }
            if n.is_negative() && !signed {
                return Err(PyException::overflow_error(
                    "can't convert negative int to unsigned",
                ));
            }

            let bits = length
                .checked_mul(8)
                .ok_or_else(|| PyException::overflow_error("int too big to convert"))?;
            let unsigned_limit = BigInt::one() << bits;
            let signed_min = if signed && bits > 0 {
                -(BigInt::one() << (bits - 1))
            } else {
                BigInt::from(0u8)
            };
            let signed_max = if signed && bits > 0 {
                (BigInt::one() << (bits - 1)) - BigInt::one()
            } else {
                &unsigned_limit - BigInt::one()
            };
            if signed {
                if n < signed_min || n > signed_max {
                    return Err(PyException::overflow_error("int too big to convert"));
                }
            } else if n >= unsigned_limit {
                return Err(PyException::overflow_error("int too big to convert"));
            }

            let val_to_encode = if n.is_negative() {
                unsigned_limit + n
            } else {
                n
            };
            let (_, mut raw) = val_to_encode.to_bytes_be();
            if raw.len() > length {
                return Err(PyException::overflow_error("int too big to convert"));
            }
            if raw.len() < length {
                let mut padded = vec![0u8; length - raw.len()];
                padded.extend(raw);
                raw = padded;
            }
            if matches!(val_to_encode.sign(), Sign::NoSign) && length == 0 {
                raw.clear();
            }
            let bytes: Vec<u8> = match byteorder.as_str() {
                "big" => raw,
                "little" => {
                    raw.reverse();
                    raw
                }
                _ => unreachable!(),
            };
            Ok(PyObject::bytes(bytes))
        }
        "as_integer_ratio" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::tuple(vec![PyObject::int(n), PyObject::int(1)]))
        }
        "conjugate" => Ok(_receiver.clone()),
        "real" => Ok(_receiver.clone()),
        "imag" => Ok(PyObject::int(0)),
        "numerator" => Ok(_receiver.clone()),
        "denominator" => Ok(PyObject::int(1)),
        "__index__" | "__int__" | "__trunc__" | "__ceil__" | "__floor__" => Ok(_receiver.clone()),
        "__abs__" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::int(n.abs()))
        }
        "__neg__" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::int(-n))
        }
        "__pos__" => Ok(_receiver.clone()),
        "__invert__" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::int(!n))
        }
        "__format__" => {
            let n = _receiver.to_int()?;
            let spec = if !args.is_empty() {
                args[0].as_str().unwrap_or("").to_string()
            } else {
                String::new()
            };
            if spec.is_empty() {
                return Ok(PyObject::str_val(CompactString::from(n.to_string())));
            }
            Ok(PyObject::str_val(CompactString::from(
                super::super::apply_format_spec_int(n, &spec),
            )))
        }
        "__str__" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::str_val(CompactString::from(n.to_string())))
        }
        "__repr__" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::str_val(CompactString::from(n.to_string())))
        }
        "__hash__" => {
            let n = match &_receiver.payload {
                PyObjectPayload::Bool(flag) => BigInt::from(if *flag { 1 } else { 0 }),
                PyObjectPayload::Int(PyInt::Small(value)) => BigInt::from(*value),
                PyObjectPayload::Int(PyInt::Big(value)) => value.as_ref().clone(),
                _ => BigInt::from(_receiver.to_int()?),
            };
            Ok(PyObject::int(py_hash_bigint(&n)))
        }
        "__bool__" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::bool_val(n != 0))
        }
        "__eq__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::bool_val(n == m));
                }
                if let Ok(f) = args[0].to_float() {
                    return Ok(PyObject::bool_val(n as f64 == f));
                }
            }
            Ok(PyObject::bool_val(false))
        }
        "__ne__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::bool_val(n != m));
                }
                if let Ok(f) = args[0].to_float() {
                    return Ok(PyObject::bool_val(n as f64 != f));
                }
            }
            Ok(PyObject::bool_val(true))
        }
        "__lt__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::bool_val(n < m));
                }
                if let Ok(f) = args[0].to_float() {
                    return Ok(PyObject::bool_val((n as f64) < f));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__le__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::bool_val(n <= m));
                }
                if let Ok(f) = args[0].to_float() {
                    return Ok(PyObject::bool_val(n as f64 <= f));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__gt__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::bool_val(n > m));
                }
                if let Ok(f) = args[0].to_float() {
                    return Ok(PyObject::bool_val(n as f64 > f));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__ge__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::bool_val(n >= m));
                }
                if let Ok(f) = args[0].to_float() {
                    return Ok(PyObject::bool_val(n as f64 >= f));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__add__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::int(n.wrapping_add(m)));
                }
                if let Ok(f) = args[0].to_float() {
                    return Ok(PyObject::float(n as f64 + f));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__sub__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::int(n.wrapping_sub(m)));
                }
                if let Ok(f) = args[0].to_float() {
                    return Ok(PyObject::float(n as f64 - f));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__mul__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::int(n.wrapping_mul(m)));
                }
                if let Ok(f) = args[0].to_float() {
                    return Ok(PyObject::float(n as f64 * f));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__floordiv__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    if m != 0 {
                        return Ok(PyObject::int(n.div_euclid(m)));
                    } else {
                        return Err(PyException::new(
                            ExceptionKind::ZeroDivisionError,
                            "integer division or modulo by zero".to_string(),
                        ));
                    }
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__mod__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    if m != 0 {
                        return Ok(PyObject::int(n.rem_euclid(m)));
                    } else {
                        return Err(PyException::new(
                            ExceptionKind::ZeroDivisionError,
                            "integer division or modulo by zero".to_string(),
                        ));
                    }
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__pow__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(if m >= 0 {
                        PyObject::int(n.wrapping_pow(m as u32))
                    } else {
                        PyObject::float((n as f64).powi(m as i32))
                    });
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__truediv__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                let d = if let Ok(m) = args[0].to_int() {
                    m as f64
                } else if let Ok(f) = args[0].to_float() {
                    f
                } else {
                    return Ok(PyObject::not_implemented());
                };
                if d == 0.0 {
                    return Err(PyException::new(
                        ExceptionKind::ZeroDivisionError,
                        "division by zero".to_string(),
                    ));
                }
                return Ok(PyObject::float(n as f64 / d));
            }
            Ok(PyObject::not_implemented())
        }
        "__float__" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::float(n as f64))
        }
        "__round__" => {
            let n = _receiver.to_int()?;
            // int.__round__(ndigits) — for ints, just returns self (unless ndigits is negative)
            if !args.is_empty() {
                if let Ok(nd) = args[0].to_int() {
                    if nd < 0 {
                        let factor = 10i64.pow((-nd) as u32);
                        return Ok(PyObject::int((n + factor / 2) / factor * factor));
                    }
                }
            }
            Ok(PyObject::int(n))
        }
        "__divmod__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    if m == 0 {
                        return Err(PyException::new(
                            ExceptionKind::ZeroDivisionError,
                            "integer division or modulo by zero".to_string(),
                        ));
                    }
                    return Ok(PyObject::tuple(vec![
                        PyObject::int(n.div_euclid(m)),
                        PyObject::int(n.rem_euclid(m)),
                    ]));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__lshift__" => {
            if !args.is_empty() {
                return _receiver.lshift(&args[0]);
            }
            Ok(PyObject::not_implemented())
        }
        "__rshift__" => {
            if !args.is_empty() {
                return _receiver.rshift(&args[0]);
            }
            Ok(PyObject::not_implemented())
        }
        "__and__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::int(n & m));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__or__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::int(n | m));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__xor__" => {
            if !args.is_empty() {
                let n = _receiver.to_int()?;
                if let Ok(m) = args[0].to_int() {
                    return Ok(PyObject::int(n ^ m));
                }
            }
            Ok(PyObject::not_implemented())
        }
        _ => Err(PyException::attribute_error(format!(
            "'int' object has no attribute '{}'",
            method
        ))),
    }
}

pub(crate) fn call_bool_method(
    value: bool,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "__repr__" | "__str__" => Ok(PyObject::str_val(CompactString::from(if value {
            "True"
        } else {
            "False"
        }))),
        "__format__"
            if args
                .first()
                .and_then(|arg| arg.as_str())
                .unwrap_or("")
                .is_empty() =>
        {
            Ok(PyObject::str_val(CompactString::from(if value {
                "True"
            } else {
                "False"
            })))
        }
        "__bool__" => Ok(PyObject::bool_val(value)),
        "__index__" | "__int__" | "__trunc__" | "__ceil__" | "__floor__" => {
            Ok(PyObject::int(if value { 1 } else { 0 }))
        }
        "__and__"
            if matches!(
                args.first().map(|arg| &arg.payload),
                Some(PyObjectPayload::Bool(_))
            ) =>
        {
            let rhs = matches!(args[0].payload, PyObjectPayload::Bool(true));
            Ok(PyObject::bool_val(value & rhs))
        }
        "__or__"
            if matches!(
                args.first().map(|arg| &arg.payload),
                Some(PyObjectPayload::Bool(_))
            ) =>
        {
            let rhs = matches!(args[0].payload, PyObjectPayload::Bool(true));
            Ok(PyObject::bool_val(value | rhs))
        }
        "__xor__"
            if matches!(
                args.first().map(|arg| &arg.payload),
                Some(PyObjectPayload::Bool(_))
            ) =>
        {
            let rhs = matches!(args[0].payload, PyObjectPayload::Bool(true));
            Ok(PyObject::bool_val(value ^ rhs))
        }
        _ => call_int_method(&PyObject::int(if value { 1 } else { 0 }), method, args),
    }
}

pub(crate) fn call_float_method(
    f: f64,
    method: &str,
    _args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "is_integer" => Ok(PyObject::bool_val(f.fract() == 0.0)),
        "hex" => {
            // Python's float.hex() format
            let (mantissa, exponent, sign) = if f == 0.0 {
                (0u64, 0i32, if f.is_sign_negative() { "-" } else { "" })
            } else {
                let bits = f.to_bits();
                let sign = if bits >> 63 != 0 { "-" } else { "" };
                let exp = ((bits >> 52) & 0x7ff) as i32 - 1023;
                let mant = bits & 0x000f_ffff_ffff_ffff;
                (mant, exp, sign)
            };
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}0x1.{:013x}p{:+}",
                sign, mantissa, exponent
            ))))
        }
        "as_integer_ratio" => {
            if f.is_infinite() || f.is_nan() {
                return Err(PyException::value_error(
                    "cannot convert Infinity or NaN to integer ratio",
                ));
            }
            let (numer, denom) = float_as_integer_ratio(f);
            Ok(PyObject::tuple(vec![
                PyObject::big_int(numer),
                PyObject::big_int(denom),
            ]))
        }
        "conjugate" => Ok(PyObject::float(f)),
        "real" => Ok(PyObject::float(f)),
        "imag" => Ok(PyObject::float(0.0)),
        "__format__" => {
            let spec = if !_args.is_empty() {
                _args[0].as_str().unwrap_or("").to_string()
            } else {
                String::new()
            };
            if spec.is_empty() {
                return Ok(PyObject::str_val(CompactString::from(
                    super::super::format_float_repr(f),
                )));
            }
            Ok(PyObject::str_val(CompactString::from(
                super::super::apply_format_spec_float(f, &spec),
            )))
        }
        "__str__" | "__repr__" => Ok(PyObject::str_val(CompactString::from(
            super::super::format_float_repr(f),
        ))),
        "__hash__" => Ok(PyObject::int(py_hash_float(f))),
        "__bool__" => Ok(PyObject::bool_val(f != 0.0)),
        "__int__" | "__trunc__" => Ok(PyObject::int(f as i64)),
        "__float__" => Ok(PyObject::float(f)),
        "__abs__" => Ok(PyObject::float(f.abs())),
        "__neg__" => Ok(PyObject::float(-f)),
        "__pos__" => Ok(PyObject::float(f)),
        "__round__" => {
            let ndigits = if !_args.is_empty() {
                _args[0].as_int().unwrap_or(0)
            } else {
                0
            };
            let factor = 10f64.powi(ndigits as i32);
            Ok(PyObject::float((f * factor).round() / factor))
        }
        "__eq__" => {
            if !_args.is_empty() {
                if let Ok(g) = _args[0].to_float() {
                    return Ok(PyObject::bool_val(f == g));
                }
                if let Ok(n) = _args[0].to_int() {
                    return Ok(PyObject::bool_val(f == n as f64));
                }
            }
            Ok(PyObject::bool_val(false))
        }
        "__ne__" => {
            if !_args.is_empty() {
                if let Ok(g) = _args[0].to_float() {
                    return Ok(PyObject::bool_val(f != g));
                }
                if let Ok(n) = _args[0].to_int() {
                    return Ok(PyObject::bool_val(f != n as f64));
                }
            }
            Ok(PyObject::bool_val(true))
        }
        "__lt__" => {
            if !_args.is_empty() {
                if let Ok(g) = _args[0].to_float() {
                    return Ok(PyObject::bool_val(f < g));
                }
                if let Ok(n) = _args[0].to_int() {
                    return Ok(PyObject::bool_val(f < n as f64));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__le__" => {
            if !_args.is_empty() {
                if let Ok(g) = _args[0].to_float() {
                    return Ok(PyObject::bool_val(f <= g));
                }
                if let Ok(n) = _args[0].to_int() {
                    return Ok(PyObject::bool_val(f <= n as f64));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__gt__" => {
            if !_args.is_empty() {
                if let Ok(g) = _args[0].to_float() {
                    return Ok(PyObject::bool_val(f > g));
                }
                if let Ok(n) = _args[0].to_int() {
                    return Ok(PyObject::bool_val(f > n as f64));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__ge__" => {
            if !_args.is_empty() {
                if let Ok(g) = _args[0].to_float() {
                    return Ok(PyObject::bool_val(f >= g));
                }
                if let Ok(n) = _args[0].to_int() {
                    return Ok(PyObject::bool_val(f >= n as f64));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__add__" => {
            if !_args.is_empty() {
                if let Ok(g) = _args[0].to_float() {
                    return Ok(PyObject::float(f + g));
                }
                if let Ok(n) = _args[0].to_int() {
                    return Ok(PyObject::float(f + n as f64));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__sub__" => {
            if !_args.is_empty() {
                if let Ok(g) = _args[0].to_float() {
                    return Ok(PyObject::float(f - g));
                }
                if let Ok(n) = _args[0].to_int() {
                    return Ok(PyObject::float(f - n as f64));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__mul__" => {
            if !_args.is_empty() {
                if let Ok(g) = _args[0].to_float() {
                    return Ok(PyObject::float(f * g));
                }
                if let Ok(n) = _args[0].to_int() {
                    return Ok(PyObject::float(f * n as f64));
                }
            }
            Ok(PyObject::not_implemented())
        }
        "__truediv__" => {
            if !_args.is_empty() {
                let g = if let Ok(g) = _args[0].to_float() {
                    g
                } else if let Ok(n) = _args[0].to_int() {
                    n as f64
                } else {
                    return Ok(PyObject::not_implemented());
                };
                if g == 0.0 {
                    return Err(PyException::new(
                        ExceptionKind::ZeroDivisionError,
                        "float division by zero".to_string(),
                    ));
                }
                return Ok(PyObject::float(f / g));
            }
            Ok(PyObject::not_implemented())
        }
        "__floordiv__" => {
            if !_args.is_empty() {
                let g = if let Ok(g) = _args[0].to_float() {
                    g
                } else if let Ok(n) = _args[0].to_int() {
                    n as f64
                } else {
                    return Ok(PyObject::not_implemented());
                };
                if g == 0.0 {
                    return Err(PyException::new(
                        ExceptionKind::ZeroDivisionError,
                        "float floor division by zero".to_string(),
                    ));
                }
                return Ok(PyObject::float((f / g).floor()));
            }
            Ok(PyObject::not_implemented())
        }
        "__mod__" => {
            if !_args.is_empty() {
                let g = if let Ok(g) = _args[0].to_float() {
                    g
                } else if let Ok(n) = _args[0].to_int() {
                    n as f64
                } else {
                    return Ok(PyObject::not_implemented());
                };
                if g == 0.0 {
                    return Err(PyException::new(
                        ExceptionKind::ZeroDivisionError,
                        "float modulo".to_string(),
                    ));
                }
                return Ok(PyObject::float(f - (f / g).floor() * g));
            }
            Ok(PyObject::not_implemented())
        }
        "__pow__" => {
            if !_args.is_empty() {
                let g = if let Ok(g) = _args[0].to_float() {
                    g
                } else if let Ok(n) = _args[0].to_int() {
                    n as f64
                } else {
                    return Ok(PyObject::not_implemented());
                };
                return Ok(PyObject::float(f.powf(g)));
            }
            Ok(PyObject::not_implemented())
        }
        "__divmod__" => {
            if !_args.is_empty() {
                let g = if let Ok(g) = _args[0].to_float() {
                    g
                } else if let Ok(n) = _args[0].to_int() {
                    n as f64
                } else {
                    return Ok(PyObject::not_implemented());
                };
                if g == 0.0 {
                    return Err(PyException::new(
                        ExceptionKind::ZeroDivisionError,
                        "float divmod()".to_string(),
                    ));
                }
                let q = (f / g).floor();
                return Ok(PyObject::tuple(vec![
                    PyObject::float(q),
                    PyObject::float(f - q * g),
                ]));
            }
            Ok(PyObject::not_implemented())
        }
        "__ceil__" => Ok(PyObject::int(f.ceil() as i64)),
        "__floor__" => Ok(PyObject::int(f.floor() as i64)),
        _ => Err(PyException::attribute_error(format!(
            "'float' object has no attribute '{}'",
            method
        ))),
    }
}
