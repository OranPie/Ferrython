use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

mod class_methods;
mod context;
mod digits;
mod value;

use context::{add_context_flags_and_methods, make_signal_types, SIGNAL_NAMES};
use digits::{digits_long_div, digits_to_string, i128_to_digits, truncate_to_prec};
use value::{
    align_scales, decimal_eq, decimal_float, decimal_format, decimal_ge, decimal_gt, decimal_hash,
    decimal_int, decimal_le, decimal_lt, decimal_parse, decimal_str, get_decimal_str,
};

// ── decimal module ──

pub fn create_decimal_module() -> PyObjectRef {
    use ferrython_core::object::{new_shared_fx, to_shared_fx, InstanceData};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::OnceLock;

    static DECIMAL_PREC: AtomicU32 = AtomicU32::new(28);
    static DECIMAL_ROUNDING: OnceLock<std::sync::RwLock<CompactString>> = OnceLock::new();
    static DECIMAL_CLASS: OnceLock<PyObjectRef> = OnceLock::new();

    fn get_prec() -> u32 {
        DECIMAL_PREC.load(Ordering::Relaxed)
    }

    fn rounding_cell() -> &'static std::sync::RwLock<CompactString> {
        DECIMAL_ROUNDING
            .get_or_init(|| std::sync::RwLock::new(CompactString::from("ROUND_HALF_EVEN")))
    }

    fn get_rounding() -> String {
        rounding_cell().read().unwrap().to_string()
    }

    fn set_rounding(value: &str) {
        *rounding_cell().write().unwrap() = CompactString::from(value);
    }

    fn context_set_attr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 {
            return Ok(PyObject::none());
        }
        let attr_name = args[1].py_to_string();
        if attr_name == "prec" {
            let new_prec = args[2].to_int()? as u32;
            DECIMAL_PREC.store(new_prec, Ordering::Relaxed);
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                inst.attrs
                    .write()
                    .insert(CompactString::from("prec"), PyObject::int(new_prec as i64));
            }
        } else {
            if attr_name == "rounding" {
                if let Some(rounding) = args[2].as_str() {
                    set_rounding(rounding);
                }
            }
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                inst.attrs
                    .write()
                    .insert(CompactString::from(attr_name), args[2].clone());
            }
        }
        Ok(PyObject::none())
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
                class_methods::add_extended_decimal_methods(
                    &mut dec_ns,
                    make_decimal,
                    decimal_quantize,
                    decimal_sqrt,
                    decimal_ln,
                    decimal_exp,
                    decimal_is_zero,
                    decimal_is_nan,
                    decimal_is_infinite,
                    decimal_to_eng_string,
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
            get_rounding()
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
                "ROUND_UP" => {
                    if remainder > 0 {
                        if val >= 0 {
                            truncated + 1
                        } else {
                            truncated - 1
                        }
                    } else {
                        truncated
                    }
                }
                "ROUND_DOWN" => truncated,
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
                    PyObject::str_val(CompactString::from(get_rounding())),
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
                        make_builtin(context_set_attr),
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
                let saved_rounding = get_rounding();
                // If a context is provided, apply its prec
                if let Some(ctx) = args.first() {
                    if let PyObjectPayload::Instance(ref inst) = ctx.payload {
                        if let Some(prec) = inst.attrs.read().get("prec") {
                            if let Some(n) = prec.as_int() {
                                DECIMAL_PREC.store(n as u32, Ordering::Relaxed);
                            }
                        }
                        if let Some(rounding) = inst
                            .attrs
                            .read()
                            .get("rounding")
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                        {
                            set_rounding(&rounding);
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
                    PyObject::str_val(CompactString::from(get_rounding())),
                );
                ctx_ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
                ctx_ns.insert(CompactString::from("Emax"), PyObject::int(999999));

                // __setattr__ on the context
                let cls_ns = {
                    let mut ns = IndexMap::new();
                    ns.insert(
                        CompactString::from("__setattr__"),
                        make_builtin(context_set_attr),
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
                            set_rounding(&saved_rounding);
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
