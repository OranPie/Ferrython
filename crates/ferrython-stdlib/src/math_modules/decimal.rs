use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── decimal module ──

pub fn create_decimal_module() -> PyObjectRef {
    use ferrython_core::object::{new_shared_fx, to_shared_fx, InstanceData};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::OnceLock;

    static DECIMAL_PREC: AtomicU32 = AtomicU32::new(28);
    static DECIMAL_CLASS: OnceLock<PyObjectRef> = OnceLock::new();

    // Signal names used by decimal module
    const SIGNAL_NAMES: &[&str] = &[
        "Clamped",
        "InvalidOperation",
        "DivisionByZero",
        "Inexact",
        "Rounded",
        "Subnormal",
        "Underflow",
        "Overflow",
        "FloatOperation",
    ];

    fn make_signal_types() -> Vec<(CompactString, PyObjectRef)> {
        SIGNAL_NAMES
            .iter()
            .map(|&name| {
                let kind = match name {
                    "DivisionByZero" => ferrython_core::error::ExceptionKind::ZeroDivisionError,
                    "Overflow" => ferrython_core::error::ExceptionKind::OverflowError,
                    _ => ferrython_core::error::ExceptionKind::ArithmeticError,
                };
                (CompactString::from(name), PyObject::exception_type(kind))
            })
            .collect()
    }

    fn make_decimal_flags_dict(signals: &[(CompactString, PyObjectRef)]) -> PyObjectRef {
        let mut map = IndexMap::new();
        for (_, sig_obj) in signals {
            let key = HashableKey::from_object(sig_obj).unwrap();
            map.insert(key, PyObject::bool_val(false));
        }
        PyObject::dict(map)
    }

    fn add_context_flags_and_methods(
        ctx_ns: &mut IndexMap<CompactString, PyObjectRef>,
        signals: &[(CompactString, PyObjectRef)],
    ) {
        ctx_ns.insert(
            CompactString::from("flags"),
            make_decimal_flags_dict(signals),
        );
        ctx_ns.insert(
            CompactString::from("traps"),
            make_decimal_flags_dict(signals),
        );
        let sigs_for_clear = signals.iter().map(|(_, o)| o.clone()).collect::<Vec<_>>();
        ctx_ns.insert(
            CompactString::from("clear_flags"),
            PyObject::native_closure("clear_flags", move |args: &[PyObjectRef]| {
                if let Some(self_obj) = args.first() {
                    if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
                        let mut new_flags = IndexMap::new();
                        for sig in &sigs_for_clear {
                            let key = HashableKey::from_object(sig).unwrap();
                            new_flags.insert(key, PyObject::bool_val(false));
                        }
                        inst.attrs
                            .write()
                            .insert(CompactString::from("flags"), PyObject::dict(new_flags));
                    }
                }
                Ok(PyObject::none())
            }),
        );
        let sigs_for_clear2 = signals.iter().map(|(_, o)| o.clone()).collect::<Vec<_>>();
        ctx_ns.insert(
            CompactString::from("clear_traps"),
            PyObject::native_closure("clear_traps", move |args: &[PyObjectRef]| {
                if let Some(self_obj) = args.first() {
                    if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
                        let mut new_traps = IndexMap::new();
                        for sig in &sigs_for_clear2 {
                            let key = HashableKey::from_object(sig).unwrap();
                            new_traps.insert(key, PyObject::bool_val(false));
                        }
                        inst.attrs
                            .write()
                            .insert(CompactString::from("traps"), PyObject::dict(new_traps));
                    }
                }
                Ok(PyObject::none())
            }),
        );
        ctx_ns.insert(
            CompactString::from("copy"),
            make_builtin(|args| {
                if let Some(self_obj) = args.first() {
                    if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
                        let attrs = inst.attrs.read().clone();
                        let new_inst = InstanceData {
                            class: inst.class.clone(),
                            attrs: to_shared_fx(attrs.into_iter().collect()),
                            is_special: true,
                            dict_storage: None,
                            class_flags: inst.class_flags,
                            finalizer_state: std::cell::Cell::new(0),
                        };
                        return Ok(PyObject::wrap(PyObjectPayload::Instance(
                            std::mem::ManuallyDrop::new(Box::new(new_inst)),
                        )));
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    fn get_prec() -> u32 {
        DECIMAL_PREC.load(Ordering::Relaxed)
    }

    fn get_decimal_class() -> PyObjectRef {
        DECIMAL_CLASS
            .get_or_init(|| {
                let mut dec_ns = IndexMap::new();
                dec_ns.insert(CompactString::from("__add__"), make_builtin(decimal_add));
                dec_ns.insert(CompactString::from("__radd__"), make_builtin(decimal_add));
                dec_ns.insert(CompactString::from("__sub__"), make_builtin(decimal_sub));
                dec_ns.insert(CompactString::from("__mul__"), make_builtin(decimal_mul));
                dec_ns.insert(
                    CompactString::from("__truediv__"),
                    make_builtin(decimal_div),
                );
                dec_ns.insert(CompactString::from("__eq__"), make_builtin(decimal_eq));
                dec_ns.insert(CompactString::from("__lt__"), make_builtin(decimal_lt));
                dec_ns.insert(
                    CompactString::from("__float__"),
                    make_builtin(decimal_float),
                );
                dec_ns.insert(CompactString::from("__int__"), make_builtin(decimal_int));
                dec_ns.insert(CompactString::from("__neg__"), make_builtin(decimal_neg));
                dec_ns.insert(CompactString::from("__abs__"), make_builtin(decimal_abs));
                dec_ns.insert(CompactString::from("__le__"), make_builtin(decimal_le));
                dec_ns.insert(CompactString::from("__gt__"), make_builtin(decimal_gt));
                dec_ns.insert(CompactString::from("__ge__"), make_builtin(decimal_ge));
                dec_ns.insert(CompactString::from("__str__"), make_builtin(decimal_str));
                dec_ns.insert(CompactString::from("__repr__"), make_builtin(decimal_str));
                dec_ns.insert(CompactString::from("__hash__"), make_builtin(decimal_hash));
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Ok(PyObject::bool_val(false));
                        }
                        let s = get_decimal_str(&args[0]).unwrap_or_default();
                        Ok(PyObject::bool_val(s.starts_with('-')))
                    }),
                );
                dec_ns.insert(
                    CompactString::from("is_normal"),
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                // as_tuple() → DecimalTuple(sign, digits, exponent)
                dec_ns.insert(
                    CompactString::from("as_tuple"),
                    make_builtin(|args: &[PyObjectRef]| {
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
                            let parts: Vec<&str> =
                                abs_s.splitn(2, |c: char| c == 'E' || c == 'e').collect();
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
                    make_builtin(|args: &[PyObjectRef]| {
                        if args.len() < 2 {
                            return Err(PyException::type_error(
                                "copy_sign requires self and other",
                            ));
                        }
                        let s = get_decimal_str(&args[0]).unwrap_or_default();
                        let other_s =
                            get_decimal_str(&args[1]).unwrap_or_else(|| args[1].py_to_string());
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Err(PyException::type_error("conjugate requires self"));
                        }
                        Ok(args[0].clone())
                    }),
                );
                dec_ns.insert(
                    CompactString::from("radix"),
                    make_builtin(|_| Ok(make_decimal("10"))),
                );
                dec_ns.insert(
                    CompactString::from("to_integral_value"),
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    make_builtin(|args: &[PyObjectRef]| {
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
                    PyObject::native_function("Decimal.__new__", |args: &[PyObjectRef]| {
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
                            PyObjectPayload::Int(n) => {
                                return Ok(make_decimal(&format!("{}", n.to_i64().unwrap_or(0))))
                            }
                            PyObjectPayload::Float(f) => {
                                return Ok(make_decimal(&format!("{}", f)))
                            }
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
                        let parts: Vec<&str> = check.splitn(2, '.').collect();
                        let valid = parts
                            .iter()
                            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
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
                        } else if check.contains('E') || check.contains('e') {
                            match trimmed.parse::<f64>() {
                                Ok(f) => Ok(make_decimal(&format!("{}", f))),
                                Err(_) => Err(PyException::value_error(format!(
                                    "Invalid literal for Decimal: '{}'",
                                    s
                                ))),
                            }
                        } else {
                            Err(PyException::value_error(format!(
                                "Invalid literal for Decimal: '{}'",
                                s
                            )))
                        }
                    }),
                );
                PyObject::class(CompactString::from("Decimal"), vec![], dec_ns)
            })
            .clone()
    }

    fn make_decimal(s: &str) -> PyObjectRef {
        let class = get_decimal_class();
        let class_flags = InstanceData::compute_flags(&class);
        let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
            Box::new(InstanceData {
                class,
                attrs: new_shared_fx(),
                is_special: true,
                dict_storage: None,
                class_flags,
                finalizer_state: std::cell::Cell::new(0),
            }),
        )));
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("__decimal__"), PyObject::bool_val(true));
            w.insert(
                CompactString::from("_value"),
                PyObject::str_val(CompactString::from(s)),
            );
        }
        inst
    }

    fn get_decimal_str(obj: &PyObjectRef) -> Option<String> {
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

    fn decimal_parse(s: &str) -> (bool, i128, u32) {
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

    fn decimal_format(neg: bool, digits: i128, scale: u32) -> String {
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

    fn align_scales(
        a: (bool, i128, u32),
        b: (bool, i128, u32),
    ) -> ((bool, i128, u32), (bool, i128, u32)) {
        let max_scale = a.2.max(b.2);
        let a_digits = a.1 * 10i128.pow(max_scale - a.2);
        let b_digits = b.1 * 10i128.pow(max_scale - b.2);
        ((a.0, a_digits, max_scale), (b.0, b_digits, max_scale))
    }

    // ── Arbitrary-precision bignum helpers for division ──

    fn i128_to_digits(mut n: i128) -> Vec<u8> {
        if n == 0 {
            return vec![0];
        }
        let mut digits = Vec::new();
        while n > 0 {
            digits.push((n % 10) as u8);
            n /= 10;
        }
        digits.reverse();
        digits
    }

    fn digits_to_string(digits: &[u8]) -> String {
        if digits.is_empty() {
            return "0".to_string();
        }
        digits.iter().map(|&d| (b'0' + d) as char).collect()
    }

    fn digits_compare(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
        let a_start = a.iter().position(|&d| d != 0).unwrap_or(a.len());
        let b_start = b.iter().position(|&d| d != 0).unwrap_or(b.len());
        let a_len = a.len() - a_start;
        let b_len = b.len() - b_start;
        if a_len != b_len {
            return a_len.cmp(&b_len);
        }
        a[a_start..].cmp(&b[b_start..])
    }

    fn digits_subtract(a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut result = vec![0u8; a.len()];
        let mut borrow: i8 = 0;
        let b_offset = a.len() as isize - b.len() as isize;
        for i in (0..a.len()).rev() {
            let bi = i as isize - b_offset;
            let b_digit = if bi >= 0 && (bi as usize) < b.len() {
                b[bi as usize] as i8
            } else {
                0
            };
            let diff = a[i] as i8 - b_digit - borrow;
            if diff < 0 {
                result[i] = (diff + 10) as u8;
                borrow = 1;
            } else {
                result[i] = diff as u8;
                borrow = 0;
            }
        }
        // Trim leading zeros
        let start = result
            .iter()
            .position(|&d| d != 0)
            .unwrap_or(result.len().saturating_sub(1));
        result[start..].to_vec()
    }

    fn digits_long_div(a: &[u8], b: &[u8]) -> Vec<u8> {
        let mut quotient = Vec::new();
        let mut remainder: Vec<u8> = vec![0];
        for &digit in a {
            // Shift remainder left and append digit
            if remainder.len() == 1 && remainder[0] == 0 {
                remainder = vec![digit];
            } else {
                remainder.push(digit);
            }
            // Binary search for the quotient digit (0..9)
            let mut lo: u8 = 0;
            let mut hi: u8 = 9;
            while lo < hi {
                let mid = (lo + hi + 1) / 2;
                let product = digits_mul_single(b, mid);
                if digits_compare(&product, &remainder) == std::cmp::Ordering::Greater {
                    hi = mid - 1;
                } else {
                    lo = mid;
                }
            }
            quotient.push(lo);
            if lo > 0 {
                let product = digits_mul_single(b, lo);
                remainder = digits_subtract(&remainder, &product);
            }
        }
        // Trim leading zeros from quotient
        let start = quotient
            .iter()
            .position(|&d| d != 0)
            .unwrap_or(quotient.len().saturating_sub(1));
        quotient[start..].to_vec()
    }

    fn digits_mul_single(a: &[u8], b: u8) -> Vec<u8> {
        if b == 0 {
            return vec![0];
        }
        let mut result = vec![0u8; a.len() + 1];
        let mut carry: u16 = 0;
        for i in (0..a.len()).rev() {
            let prod = a[i] as u16 * b as u16 + carry;
            result[i + 1] = (prod % 10) as u8;
            carry = prod / 10;
        }
        result[0] = carry as u8;
        let start = result
            .iter()
            .position(|&d| d != 0)
            .unwrap_or(result.len().saturating_sub(1));
        result[start..].to_vec()
    }

    /// Truncate a decimal string to `prec` significant digits (ROUND_HALF_EVEN)
    fn truncate_to_prec(s: &str, prec: u32) -> String {
        if prec == 0 {
            return s.to_string();
        }
        let (neg, rest) = if s.starts_with('-') {
            (true, &s[1..])
        } else {
            (false, s)
        };
        let (int_part, frac_part) = if let Some(dot) = rest.find('.') {
            (&rest[..dot], &rest[dot + 1..])
        } else {
            (rest, "")
        };
        let all_digits: Vec<char> = format!("{}{}", int_part, frac_part).chars().collect();
        let first_sig = match all_digits.iter().position(|&c| c != '0') {
            Some(i) => i,
            None => return s.to_string(),
        };
        let sig_count = all_digits.len() - first_sig;
        if sig_count <= prec as usize {
            return s.to_string();
        }
        let keep = first_sig + prec as usize;
        // Banker's rounding on the digit at position `keep`
        let round_digit = if keep < all_digits.len() {
            all_digits[keep].to_digit(10).unwrap_or(0)
        } else {
            0
        };
        let mut kept: Vec<u8> = all_digits[..keep]
            .iter()
            .map(|c| c.to_digit(10).unwrap_or(0) as u8)
            .collect();
        let round_up = if round_digit > 5 {
            true
        } else if round_digit == 5 {
            // Check if there are any nonzero digits after
            let has_trailing = if keep + 1 < all_digits.len() {
                all_digits[keep + 1..].iter().any(|&c| c != '0')
            } else {
                false
            };
            if has_trailing {
                true
            } else {
                kept.last().map_or(false, |&d| d % 2 != 0)
            }
        } else {
            false
        };
        if round_up {
            let mut i = kept.len();
            while i > 0 {
                i -= 1;
                if kept[i] < 9 {
                    kept[i] += 1;
                    break;
                }
                kept[i] = 0;
                if i == 0 {
                    kept.insert(0, 1);
                }
            }
        }
        // Reconstruct
        let int_len = int_part.len();
        let trunc_str: String = kept.iter().map(|&d| (b'0' + d) as char).collect();
        if frac_part.is_empty() || keep <= int_len {
            let int_digits = &trunc_str[..std::cmp::min(int_len, trunc_str.len())];
            let pad = if int_len > trunc_str.len() {
                int_len - trunc_str.len()
            } else {
                0
            };
            let padded = format!("{}{}", int_digits, "0".repeat(pad));
            if neg && padded != "0" {
                format!("-{}", padded)
            } else {
                padded
            }
        } else {
            let int_d = &trunc_str[..int_len];
            let frac_d = &trunc_str[int_len..];
            if neg {
                format!("-{}.{}", int_d, frac_d)
            } else {
                format!("{}.{}", int_d, frac_d)
            }
        }
    }

    fn decimal_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Decimal.__add__ requires 2 args"));
        }
        let a_str =
            get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let b_str =
            get_decimal_str(&args[1]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let a = decimal_parse(&a_str);
        let b = decimal_parse(&b_str);
        let (a, b) = align_scales(a, b);
        let a_val = if a.0 { -(a.1) } else { a.1 };
        let b_val = if b.0 { -(b.1) } else { b.1 };
        let result = a_val + b_val;
        let neg = result < 0;
        let digits = result.unsigned_abs();
        Ok(make_decimal(&decimal_format(neg, digits as i128, a.2)))
    }

    fn decimal_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Decimal.__sub__ requires 2 args"));
        }
        let a_str =
            get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let b_str =
            get_decimal_str(&args[1]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let a = decimal_parse(&a_str);
        let b = decimal_parse(&b_str);
        let (a, b) = align_scales(a, b);
        let a_val = if a.0 { -(a.1) } else { a.1 };
        let b_val = if b.0 { -(b.1) } else { b.1 };
        let result = a_val - b_val;
        let neg = result < 0;
        let digits = result.unsigned_abs();
        Ok(make_decimal(&decimal_format(neg, digits as i128, a.2)))
    }

    fn decimal_mul(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Decimal.__mul__ requires 2 args"));
        }
        let a_str =
            get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let b_str =
            get_decimal_str(&args[1]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let a = decimal_parse(&a_str);
        let b = decimal_parse(&b_str);
        let neg = a.0 != b.0;
        let digits = a.1 * b.1;
        let scale = a.2 + b.2;
        Ok(make_decimal(&decimal_format(neg, digits, scale)))
    }

    fn decimal_div(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "Decimal.__truediv__ requires 2 args",
            ));
        }
        let a_str =
            get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let b_str =
            get_decimal_str(&args[1]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let a = decimal_parse(&a_str);
        let b = decimal_parse(&b_str);
        if b.1 == 0 {
            return Err(PyException::zero_division_error("decimal division by zero"));
        }
        let neg = a.0 != b.0;
        let precision = get_prec();
        // Use bignum division: scale a by 10^(precision+2) for rounding headroom, then truncate
        let extra = 2u32;
        let mut a_digits = i128_to_digits(a.1);
        a_digits.extend(std::iter::repeat(0u8).take((precision + extra) as usize));
        let b_digits = i128_to_digits(b.1);
        let result_digits = digits_long_div(&a_digits, &b_digits);
        let result_str = digits_to_string(&result_digits);
        let total_scale = a.2 + precision + extra - b.2;
        // Format with full scale, then truncate to prec significant digits
        let formatted = if total_scale == 0 {
            if neg && result_str != "0" {
                format!("-{}", result_str)
            } else {
                result_str
            }
        } else {
            let padded = if result_str.len() <= total_scale as usize {
                format!("{:0>width$}", result_str, width = total_scale as usize + 1)
            } else {
                result_str
            };
            let split_pos = padded.len() - total_scale as usize;
            let int_part = &padded[..split_pos];
            let frac_part = &padded[split_pos..];
            if neg && (int_part != "0" || frac_part.chars().any(|c| c != '0')) {
                format!("-{}.{}", int_part, frac_part)
            } else {
                format!("{}.{}", int_part, frac_part)
            }
        };
        Ok(make_decimal(&truncate_to_prec(&formatted, precision)))
    }

    fn decimal_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

    fn decimal_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

    fn decimal_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        let f: f64 = s.parse().unwrap_or(0.0);
        Ok(PyObject::float(f))
    }

    fn decimal_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

    fn decimal_neg(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        let (neg, digits, scale) = decimal_parse(&s);
        Ok(make_decimal(&decimal_format(!neg, digits, scale)))
    }

    fn decimal_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        let (_, digits, scale) = decimal_parse(&s);
        Ok(make_decimal(&decimal_format(false, digits, scale)))
    }

    fn decimal_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

    fn decimal_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

    fn decimal_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

    fn decimal_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        Ok(PyObject::str_val(CompactString::from(s)))
    }

    fn decimal_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        let f: f64 = s.parse().unwrap_or(0.0);
        Ok(PyObject::int(f.to_bits() as i64))
    }

    fn decimal_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        if s == "NaN" {
            return Ok(make_decimal("NaN"));
        }
        let f: f64 = s.parse().unwrap_or(0.0);
        if f < 0.0 {
            return Err(PyException::value_error("Square root of negative number"));
        }
        let result = f.sqrt();
        Ok(make_decimal(&format!("{}", result)))
    }

    fn decimal_ln(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        if s == "NaN" {
            return Ok(make_decimal("NaN"));
        }
        let f: f64 = s.parse().unwrap_or(0.0);
        if f <= 0.0 {
            return Err(PyException::value_error("ln of non-positive number"));
        }
        let result = f.ln();
        Ok(make_decimal(&format!("{}", result)))
    }

    fn decimal_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        if s == "NaN" {
            return Ok(make_decimal("NaN"));
        }
        let f: f64 = s.parse().unwrap_or(0.0);
        let result = f.exp();
        Ok(make_decimal(&format!("{}", result)))
    }

    fn decimal_is_zero(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        let (_, digits, _) = decimal_parse(&s);
        Ok(PyObject::bool_val(digits == 0))
    }

    fn decimal_is_nan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        Ok(PyObject::bool_val(s == "NaN"))
    }

    fn decimal_is_infinite(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        Ok(PyObject::bool_val(s == "Infinity" || s == "-Infinity"))
    }

    fn decimal_to_eng_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = args
            .first()
            .and_then(get_decimal_str)
            .unwrap_or_else(|| "0".to_string());
        if s == "NaN" || s == "Infinity" || s == "-Infinity" {
            return Ok(PyObject::str_val(CompactString::from(&s)));
        }
        let f: f64 = s.parse().unwrap_or(0.0);
        if f == 0.0 {
            return Ok(PyObject::str_val(CompactString::from("0")));
        }
        let neg = f < 0.0;
        let abs_f = f.abs();
        let exp10 = abs_f.log10().floor() as i32;
        // Engineering notation: exponent is multiple of 3
        let eng_exp = (exp10.div_euclid(3)) * 3;
        let mantissa = abs_f / 10f64.powi(eng_exp);
        let result = if eng_exp == 0 {
            if neg {
                format!("-{}", mantissa)
            } else {
                format!("{}", mantissa)
            }
        } else {
            if neg {
                format!("-{}E+{}", mantissa, eng_exp)
            } else {
                format!("{}E+{}", mantissa, eng_exp)
            }
        };
        Ok(PyObject::str_val(CompactString::from(&result)))
    }

    /// quantize(self, exp, rounding=None) — round to the scale of exp
    fn decimal_quantize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("quantize requires 2 args"));
        }
        let a_str =
            get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let exp_str = get_decimal_str(&args[1])
            .ok_or_else(|| PyException::type_error("quantize exp must be Decimal"))?;
        let (neg, digits, scale) = decimal_parse(&a_str);
        let (_, _, target_scale) = decimal_parse(&exp_str);

        // Extract rounding mode from kwargs
        let rounding = if args.len() > 2 {
            if let Some(s) = args[2].as_str() {
                s.to_string()
            } else if let PyObjectPayload::Dict(d) = &args[args.len() - 1].payload {
                d.read()
                    .get(&HashableKey::str_key(CompactString::from("rounding")))
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let val = if neg {
            -(digits as i128)
        } else {
            digits as i128
        };
        let result = if target_scale < scale {
            // Reduce scale — need rounding
            let factor = 10i128.pow(scale - target_scale);
            let truncated = val / factor;
            let remainder = (val % factor).unsigned_abs();
            let half = factor.unsigned_abs() / 2;
            let rounded = match rounding.as_str() {
                "ROUND_HALF_UP" => {
                    if remainder >= half {
                        if val >= 0 {
                            truncated + 1
                        } else {
                            truncated - 1
                        }
                    } else {
                        truncated
                    }
                }
                "ROUND_CEILING" => {
                    if remainder > 0 && val > 0 {
                        truncated + 1
                    } else {
                        truncated
                    }
                }
                "ROUND_FLOOR" => {
                    if remainder > 0 && val < 0 {
                        truncated - 1
                    } else {
                        truncated
                    }
                }
                _ => {
                    // ROUND_HALF_EVEN (default banker's rounding)
                    if remainder > half {
                        if val >= 0 {
                            truncated + 1
                        } else {
                            truncated - 1
                        }
                    } else if remainder == half {
                        if truncated % 2 != 0 {
                            if val >= 0 {
                                truncated + 1
                            } else {
                                truncated - 1
                            }
                        } else {
                            truncated
                        }
                    } else {
                        truncated
                    }
                }
            };
            rounded
        } else {
            // Increase scale — multiply
            val * 10i128.pow(target_scale - scale)
        };
        let r_neg = result < 0;
        let r_digits = result.unsigned_abs();
        // Preserve exact target scale (don't trim trailing zeros)
        if target_scale == 0 {
            let s = if r_neg {
                format!("-{}", r_digits)
            } else {
                format!("{}", r_digits)
            };
            Ok(make_decimal(&s))
        } else {
            let s = format!("{:0>width$}", r_digits, width = target_scale as usize + 1);
            let (int_part, frac_part) = s.split_at(s.len() - target_scale as usize);
            let formatted = if r_neg {
                format!("-{}.{}", int_part, frac_part)
            } else {
                format!("{}.{}", int_part, frac_part)
            };
            Ok(make_decimal(&formatted))
        }
    }

    // Pre-create signal types so they're shared across module exports and context flags
    let signals = make_signal_types();
    let signals_for_getctx = signals.clone();
    let signals_for_basic = signals.clone();
    let signals_for_ext = signals.clone();
    let signals_for_ctor = signals.clone();

    let mut module_entries: Vec<(&str, PyObjectRef)> = vec![
        ("Decimal", get_decimal_class()),
        (
            "ROUND_HALF_UP",
            PyObject::str_val(CompactString::from("ROUND_HALF_UP")),
        ),
        (
            "ROUND_HALF_DOWN",
            PyObject::str_val(CompactString::from("ROUND_HALF_DOWN")),
        ),
        (
            "ROUND_HALF_EVEN",
            PyObject::str_val(CompactString::from("ROUND_HALF_EVEN")),
        ),
        (
            "ROUND_CEILING",
            PyObject::str_val(CompactString::from("ROUND_CEILING")),
        ),
        (
            "ROUND_FLOOR",
            PyObject::str_val(CompactString::from("ROUND_FLOOR")),
        ),
        (
            "ROUND_DOWN",
            PyObject::str_val(CompactString::from("ROUND_DOWN")),
        ),
        (
            "ROUND_UP",
            PyObject::str_val(CompactString::from("ROUND_UP")),
        ),
        (
            "ROUND_05UP",
            PyObject::str_val(CompactString::from("ROUND_05UP")),
        ),
        (
            "getcontext",
            PyObject::native_closure("getcontext", move |_| {
                use std::sync::atomic::Ordering;
                let current_prec = DECIMAL_PREC.load(Ordering::Relaxed);
                let mut ctx_ns = IndexMap::new();
                ctx_ns.insert(
                    CompactString::from("prec"),
                    PyObject::int(current_prec as i64),
                );
                ctx_ns.insert(
                    CompactString::from("rounding"),
                    PyObject::str_val(CompactString::from("ROUND_HALF_EVEN")),
                );
                ctx_ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
                ctx_ns.insert(CompactString::from("Emax"), PyObject::int(999999));
                ctx_ns.insert(CompactString::from("capitals"), PyObject::int(1));
                ctx_ns.insert(CompactString::from("clamp"), PyObject::int(0));
                add_context_flags_and_methods(&mut ctx_ns, &signals_for_getctx);
                // Add __setattr__ to intercept prec assignment
                let cls_ns = {
                    let mut ns = IndexMap::new();
                    ns.insert(
                        CompactString::from("__setattr__"),
                        make_builtin(|args| {
                            use std::sync::atomic::Ordering;
                            if args.len() < 3 {
                                return Ok(PyObject::none());
                            }
                            let attr_name = args[1].py_to_string();
                            if attr_name == "prec" {
                                let new_prec = args[2].to_int()? as u32;
                                DECIMAL_PREC.store(new_prec, Ordering::Relaxed);
                                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                                    inst.attrs.write().insert(
                                        CompactString::from("prec"),
                                        PyObject::int(new_prec as i64),
                                    );
                                }
                            } else if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                                inst.attrs
                                    .write()
                                    .insert(CompactString::from(attr_name), args[2].clone());
                            }
                            Ok(PyObject::none())
                        }),
                    );
                    ns
                };
                let cls = PyObject::class(CompactString::from("Context"), vec![], cls_ns);
                let class_flags = InstanceData::compute_flags(&cls);
                let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
                    Box::new(InstanceData {
                        class: cls,
                        attrs: to_shared_fx(ctx_ns),
                        is_special: true,
                        dict_storage: None,
                        class_flags,
                        finalizer_state: std::cell::Cell::new(0),
                    }),
                )));
                Ok(inst)
            }),
        ),
        (
            "setcontext",
            make_builtin(|args| {
                if args.is_empty() {
                    return Err(PyException::type_error("setcontext() requires 1 argument"));
                }
                // Extract prec from context and update global
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    if let Some(prec) = inst.attrs.read().get("prec") {
                        if let Some(n) = prec.as_int() {
                            DECIMAL_PREC.store(n as u32, Ordering::Relaxed);
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        ),
        (
            "localcontext",
            make_builtin(|args| {
                // localcontext(ctx=None) → context manager that saves/restores decimal context
                let saved_prec = DECIMAL_PREC.load(Ordering::Relaxed);
                // If a context is provided, apply its prec
                if let Some(ctx) = args.first() {
                    if let PyObjectPayload::Instance(ref inst) = ctx.payload {
                        if let Some(prec) = inst.attrs.read().get("prec") {
                            if let Some(n) = prec.as_int() {
                                DECIMAL_PREC.store(n as u32, Ordering::Relaxed);
                            }
                        }
                    }
                }
                // Build a context object as the __enter__ return value
                let mut ctx_ns = IndexMap::new();
                ctx_ns.insert(
                    CompactString::from("prec"),
                    PyObject::int(DECIMAL_PREC.load(Ordering::Relaxed) as i64),
                );
                ctx_ns.insert(
                    CompactString::from("rounding"),
                    PyObject::str_val(CompactString::from("ROUND_HALF_EVEN")),
                );
                ctx_ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
                ctx_ns.insert(CompactString::from("Emax"), PyObject::int(999999));

                // __setattr__ on the context
                let cls_ns = {
                    let mut ns = IndexMap::new();
                    ns.insert(
                        CompactString::from("__setattr__"),
                        make_builtin(|args| {
                            if args.len() < 3 {
                                return Ok(PyObject::none());
                            }
                            let attr_name = args[1].py_to_string();
                            if attr_name == "prec" {
                                let new_prec = args[2].to_int()? as u32;
                                DECIMAL_PREC.store(new_prec, Ordering::Relaxed);
                                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                                    inst.attrs.write().insert(
                                        CompactString::from("prec"),
                                        PyObject::int(new_prec as i64),
                                    );
                                }
                            } else if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                                inst.attrs
                                    .write()
                                    .insert(CompactString::from(attr_name), args[2].clone());
                            }
                            Ok(PyObject::none())
                        }),
                    );
                    ns
                };
                let cls = PyObject::class(CompactString::from("Context"), vec![], cls_ns);
                let class_flags = InstanceData::compute_flags(&cls);
                let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
                    Box::new(InstanceData {
                        class: cls,
                        attrs: to_shared_fx(ctx_ns),
                        is_special: true,
                        dict_storage: None,
                        class_flags,
                        finalizer_state: std::cell::Cell::new(0),
                    }),
                )));
                // Add __enter__ and __exit__ for context manager
                if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                    let mut attrs = inst_data.attrs.write();
                    let ctx_clone = inst.clone();
                    attrs.insert(
                        CompactString::from("__enter__"),
                        PyObject::native_closure("localcontext.__enter__", move |_| {
                            Ok(ctx_clone.clone())
                        }),
                    );
                    attrs.insert(
                        CompactString::from("__exit__"),
                        PyObject::native_closure("localcontext.__exit__", move |_| {
                            DECIMAL_PREC.store(saved_prec, Ordering::Relaxed);
                            Ok(PyObject::bool_val(false))
                        }),
                    );
                }
                Ok(inst)
            }),
        ),
    ];

    // Add signal types from the pre-created set (share same objects with flags dicts)
    for (name, obj) in &signals {
        let static_name = SIGNAL_NAMES.iter().find(|&&s| s == name.as_str()).unwrap();
        module_entries.push((static_name, obj.clone()));
    }
    module_entries.push((
        "DecimalException",
        PyObject::exception_type(ferrython_core::error::ExceptionKind::ArithmeticError),
    ));

    module_entries.extend(vec![
        ("BasicContext", {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("prec"), PyObject::int(9));
            ns.insert(
                CompactString::from("rounding"),
                PyObject::str_val(CompactString::from("ROUND_HALF_UP")),
            );
            ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
            ns.insert(CompactString::from("Emax"), PyObject::int(999999));
            add_context_flags_and_methods(&mut ns, &signals_for_basic);
            let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
            let class_flags = InstanceData::compute_flags(&cls);
            PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
                Box::new(InstanceData {
                    class: cls,
                    attrs: to_shared_fx(ns),
                    is_special: true,
                    dict_storage: None,
                    class_flags,
                    finalizer_state: std::cell::Cell::new(0),
                }),
            )))
        }),
        ("ExtendedContext", {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("prec"), PyObject::int(9));
            ns.insert(
                CompactString::from("rounding"),
                PyObject::str_val(CompactString::from("ROUND_HALF_EVEN")),
            );
            ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
            ns.insert(CompactString::from("Emax"), PyObject::int(999999));
            add_context_flags_and_methods(&mut ns, &signals_for_ext);
            let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
            let class_flags = InstanceData::compute_flags(&cls);
            PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
                Box::new(InstanceData {
                    class: cls,
                    attrs: to_shared_fx(ns),
                    is_special: true,
                    dict_storage: None,
                    class_flags,
                    finalizer_state: std::cell::Cell::new(0),
                }),
            )))
        }),
        (
            "Context",
            PyObject::native_closure("Context", move |args: &[PyObjectRef]| {
                // Context(prec=28, rounding=ROUND_HALF_EVEN, ...)
                let mut ctx_ns = IndexMap::new();
                let prec = args
                    .first()
                    .and_then(|a| {
                        if matches!(a.payload, PyObjectPayload::Dict(_)) {
                            if let PyObjectPayload::Dict(ref m) = a.payload {
                                m.read()
                                    .get(&HashableKey::str_key(CompactString::from("prec")))
                                    .and_then(|v| v.as_int())
                            } else {
                                None
                            }
                        } else {
                            a.as_int()
                        }
                    })
                    .unwrap_or(28) as i64;
                ctx_ns.insert(CompactString::from("prec"), PyObject::int(prec));
                ctx_ns.insert(
                    CompactString::from("rounding"),
                    PyObject::str_val(CompactString::from("ROUND_HALF_EVEN")),
                );
                ctx_ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
                ctx_ns.insert(CompactString::from("Emax"), PyObject::int(999999));
                ctx_ns.insert(CompactString::from("capitals"), PyObject::int(1));
                ctx_ns.insert(CompactString::from("clamp"), PyObject::int(0));
                add_context_flags_and_methods(&mut ctx_ns, &signals_for_ctor);
                let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
                let class_flags = InstanceData::compute_flags(&cls);
                Ok(PyObject::wrap(PyObjectPayload::Instance(
                    std::mem::ManuallyDrop::new(Box::new(InstanceData {
                        class: cls,
                        attrs: to_shared_fx(ctx_ns),
                        is_special: true,
                        dict_storage: None,
                        class_flags,
                        finalizer_state: std::cell::Cell::new(0),
                    })),
                )))
            }),
        ),
    ]);

    make_module("decimal", module_entries)
}
