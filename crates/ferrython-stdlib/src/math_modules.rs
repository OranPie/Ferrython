//! Math and statistics stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    call_callable, check_args, make_builtin, make_module, CompareOp, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{FromPrimitive, One, Signed, ToPrimitive, Zero};

unsafe extern "C" {
    fn ldexp(x: libc::c_double, exp: libc::c_int) -> libc::c_double;
    #[link_name = "remainder"]
    fn c_remainder(x: libc::c_double, y: libc::c_double) -> libc::c_double;
}

pub fn create_math_module() -> PyObjectRef {
    make_module(
        "math",
        vec![
            ("pi", PyObject::float(std::f64::consts::PI)),
            ("e", PyObject::float(std::f64::consts::E)),
            ("tau", PyObject::float(std::f64::consts::TAU)),
            ("inf", PyObject::float(f64::INFINITY)),
            ("nan", PyObject::float(f64::NAN)),
            ("sqrt", make_builtin(math_sqrt)),
            ("ceil", PyObject::native_function("math.ceil", math_ceil)),
            ("floor", PyObject::native_function("math.floor", math_floor)),
            ("abs", make_builtin(math_fabs)),
            ("fabs", make_builtin(math_fabs)),
            ("pow", make_builtin(math_pow)),
            ("log", make_builtin(math_log)),
            ("log2", make_builtin(math_log2)),
            ("log10", make_builtin(math_log10)),
            ("exp", make_builtin(math_exp)),
            ("sin", make_builtin(math_sin)),
            ("cos", make_builtin(math_cos)),
            ("tan", make_builtin(math_tan)),
            ("asin", make_builtin(math_asin)),
            ("acos", make_builtin(math_acos)),
            ("atan", make_builtin(math_atan)),
            ("atan2", make_builtin(math_atan2)),
            ("degrees", make_builtin(math_degrees)),
            ("radians", make_builtin(math_radians)),
            ("isnan", make_builtin(math_isnan)),
            ("isinf", make_builtin(math_isinf)),
            ("isfinite", make_builtin(math_isfinite)),
            ("gcd", make_builtin(math_gcd)),
            ("factorial", make_builtin(math_factorial)),
            ("trunc", PyObject::native_function("math.trunc", math_trunc)),
            ("copysign", make_builtin(math_copysign)),
            ("hypot", make_builtin(math_hypot)),
            ("modf", make_builtin(math_modf)),
            ("fmod", make_builtin(math_fmod)),
            ("frexp", make_builtin(math_frexp)),
            ("ldexp", make_builtin(math_ldexp)),
            ("isclose", make_builtin(math_isclose)),
            ("comb", make_builtin(math_comb)),
            ("perm", make_builtin(math_perm)),
            ("prod", make_builtin(math_prod)),
            ("lcm", make_builtin(math_lcm)),
            ("isqrt", make_builtin(math_isqrt)),
            (
                "nextafter",
                make_builtin(|args| {
                    check_args("nextafter", args, 2)?;
                    let x = args[0].to_float()?;
                    let y = args[1].to_float()?;
                    // IEEE 754 nextafter: step x toward y
                    if x == y {
                        return Ok(PyObject::float(y));
                    }
                    if x.is_nan() || y.is_nan() {
                        return Ok(PyObject::float(f64::NAN));
                    }
                    let bits = x.to_bits();
                    let result = if (y > x) == (x >= 0.0) {
                        f64::from_bits(bits + 1)
                    } else {
                        f64::from_bits(bits - 1)
                    };
                    Ok(PyObject::float(result))
                }),
            ),
            (
                "ulp",
                make_builtin(|args| {
                    check_args("ulp", args, 1)?;
                    let x = args[0].to_float()?;
                    if x.is_nan() {
                        return Ok(PyObject::float(f64::NAN));
                    }
                    if x.is_infinite() {
                        return Ok(PyObject::float(f64::INFINITY));
                    }
                    let x = x.abs();
                    let next = f64::from_bits(x.to_bits() + 1);
                    Ok(PyObject::float(next - x))
                }),
            ),
            ("remainder", make_builtin(math_remainder)),
            ("expm1", make_builtin(math_expm1)),
            ("log1p", make_builtin(math_log1p)),
            ("sinh", make_builtin(math_sinh)),
            ("cosh", make_builtin(math_cosh)),
            ("tanh", make_builtin(math_tanh)),
            ("asinh", make_builtin(math_asinh)),
            ("acosh", make_builtin(math_acosh)),
            ("atanh", make_builtin(math_atanh)),
            ("erf", make_builtin(math_erf)),
            ("erfc", make_builtin(math_erfc)),
            ("gamma", make_builtin(math_gamma)),
            ("lgamma", make_builtin(math_lgamma)),
            ("fsum", make_builtin(math_fsum)),
            ("dist", make_builtin(math_dist)),
        ],
    )
}

fn math_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sqrt", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x < 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.sqrt()))
}

fn math_number_to_float(obj: &PyObjectRef) -> PyResult<f64> {
    match &obj.payload {
        PyObjectPayload::Float(f) => Ok(*f),
        PyObjectPayload::Int(PyInt::Small(n)) => Ok(*n as f64),
        PyObjectPayload::Int(PyInt::Big(n)) => {
            let value = n
                .to_f64()
                .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
            if value.is_finite() {
                Ok(value)
            } else {
                Err(PyException::overflow_error(
                    "int too large to convert to float",
                ))
            }
        }
        PyObjectPayload::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        PyObjectPayload::Str(_) => Err(PyException::type_error("must be real number, not str")),
        PyObjectPayload::Instance(inst) => {
            {
                let attrs = inst.attrs.read();
                if attrs.contains_key("__decimal__") {
                    if let Some(v) = attrs.get("_value") {
                        if let Some(s) = v.as_str() {
                            return s.parse::<f64>().map_err(|_| {
                                PyException::value_error(format!(
                                    "could not convert string to float: '{}'",
                                    s
                                ))
                            });
                        }
                    }
                }
                if let Some(v) = attrs.get("__builtin_value__") {
                    if matches!(
                        &v.payload,
                        PyObjectPayload::Float(_)
                            | PyObjectPayload::Int(_)
                            | PyObjectPayload::Bool(_)
                    ) {
                        return math_number_to_float(v);
                    }
                }
                if let Some(v) = attrs.get("_value") {
                    if let Some(s) = v.as_str() {
                        return s.parse::<f64>().map_err(|_| {
                            PyException::value_error(format!(
                                "could not convert string to float: '{}'",
                                s
                            ))
                        });
                    }
                }
                if attrs.contains_key("__fraction__") {
                    if let (Some(n), Some(d)) = (attrs.get("numerator"), attrs.get("denominator")) {
                        return Ok(math_number_to_float(n)? / math_number_to_float(d)?);
                    }
                }
            }
            if let Some(method) = obj.get_attr("__float__") {
                let result = ferrython_core::object::call_callable(&method, &[])?;
                if let PyObjectPayload::Float(f) = &result.payload {
                    return Ok(*f);
                }
                return Err(PyException::type_error("__float__ returned non-float"));
            }
            obj.to_float()
        }
        _ => Err(PyException::type_error(format!(
            "must be real number, not {}",
            obj.type_name()
        ))),
    }
}

fn index_bigint(obj: &PyObjectRef, func_name: &str) -> PyResult<BigInt> {
    match &obj.payload {
        PyObjectPayload::Int(n) => Ok(match n {
            PyInt::Small(v) => BigInt::from(*v),
            PyInt::Big(v) => v.as_ref().clone(),
        }),
        PyObjectPayload::Bool(b) => Ok(BigInt::from(if *b { 1 } else { 0 })),
        PyObjectPayload::Instance(inst) => {
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                if matches!(
                    &value.payload,
                    PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)
                ) {
                    return index_bigint(&value, func_name);
                }
            }
            if let Some(method) = obj.get_attr("__index__") {
                let result = ferrython_core::object::call_callable(&method, &[])?;
                return index_bigint(&result, func_name);
            }
            Err(PyException::type_error(format!(
                "{}() argument must be an integer",
                func_name
            )))
        }
        _ => Err(PyException::type_error(format!(
            "{}() argument must be an integer",
            func_name
        ))),
    }
}

fn bigint_to_object(value: BigInt) -> PyObjectRef {
    if let Some(v) = value.to_i64() {
        PyObject::int(v)
    } else {
        PyObject::big_int(value)
    }
}

fn isqrt_bigint(n: &BigInt) -> BigInt {
    if n.is_zero() {
        return BigInt::zero();
    }
    let mut x = BigInt::one() << ((n.bits() + 1) / 2);
    loop {
        let y = (&x + n / &x) >> 1usize;
        if y >= x {
            return x;
        }
        x = y;
    }
}

fn float_to_integral_object(x: f64, func_name: &str, op: fn(f64) -> f64) -> PyResult<PyObjectRef> {
    if x.is_nan() {
        return Err(PyException::value_error(format!(
            "cannot convert float NaN to integer in {}",
            func_name
        )));
    }
    if x.is_infinite() {
        return Err(PyException::overflow_error(format!(
            "cannot convert float infinity to integer in {}",
            func_name
        )));
    }
    let value = op(x);
    if value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        Ok(PyObject::int(value as i64))
    } else {
        BigInt::from_f64(value)
            .map(PyObject::big_int)
            .ok_or_else(|| PyException::overflow_error("float too large to convert to integer"))
    }
}

fn pyint_ln(n: &PyInt) -> PyResult<f64> {
    match n {
        PyInt::Small(v) => {
            if *v <= 0 {
                Err(PyException::value_error("math domain error"))
            } else {
                Ok((*v as f64).ln())
            }
        }
        PyInt::Big(v) => {
            if v.sign() != Sign::Plus {
                return Err(PyException::value_error("math domain error"));
            }
            let bits = v.bits();
            if bits <= 1023 {
                return v
                    .to_f64()
                    .map(|f| f.ln())
                    .ok_or_else(|| PyException::overflow_error("int too large to convert"));
            }
            let shift = bits.saturating_sub(53) as usize;
            let top = (v.as_ref() >> shift)
                .to_u64()
                .ok_or_else(|| PyException::overflow_error("int too large to convert"))?;
            Ok((top as f64).ln() + (shift as f64) * std::f64::consts::LN_2)
        }
    }
}

fn pyint_log2(n: &PyInt) -> PyResult<f64> {
    match n {
        PyInt::Small(v) => {
            if *v <= 0 {
                Err(PyException::value_error("math domain error"))
            } else if (*v & (*v - 1)) == 0 {
                Ok((63 - v.leading_zeros()) as f64)
            } else {
                Ok((*v as f64).log2())
            }
        }
        PyInt::Big(v) => {
            if v.sign() != Sign::Plus {
                return Err(PyException::value_error("math domain error"));
            }
            let bits = v.bits();
            let one = BigInt::one();
            if (v.as_ref() & (v.as_ref() - &one)).is_zero() {
                return Ok((bits - 1) as f64);
            }
            Ok(pyint_ln(n)? / std::f64::consts::LN_2)
        }
    }
}

fn float_log2_exact_power(x: f64) -> Option<f64> {
    if !(x > 0.0 && x.is_finite()) {
        return None;
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    if exp == 0 {
        if frac != 0 && (frac & (frac - 1)) == 0 {
            return Some((frac.trailing_zeros() as i32 - 1074) as f64);
        }
    } else if frac == 0 {
        return Some((exp - 1023) as f64);
    }
    None
}

fn is_odd_integer_float(x: f64) -> bool {
    x.is_finite() && x.fract() == 0.0 && x.abs() <= u64::MAX as f64 && ((x.abs() as u64) & 1) == 1
}

fn math_ln_arg(obj: &PyObjectRef) -> PyResult<f64> {
    match &obj.payload {
        PyObjectPayload::Int(n) => pyint_ln(n),
        _ => {
            let x = math_number_to_float(obj)?;
            if x <= 0.0 {
                return Err(PyException::value_error("math domain error"));
            }
            Ok(x.ln())
        }
    }
}

fn math_ceil(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.ceil", args, 1)?;
    float_to_integral_object(math_number_to_float(&args[0])?, "math.ceil", f64::ceil)
}
fn math_floor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.floor", args, 1)?;
    float_to_integral_object(math_number_to_float(&args[0])?, "math.floor", f64::floor)
}
fn math_fabs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fabs", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.abs()))
}
fn math_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.pow", args, 2)?;
    let x = math_number_to_float(&args[0])?;
    let y = math_number_to_float(&args[1])?;
    if y == 0.0 || x == 1.0 {
        return Ok(PyObject::float(1.0));
    }
    if x.is_infinite() {
        if y.is_nan() {
            return Ok(PyObject::float(f64::NAN));
        }
        if y > 0.0 {
            if x.is_sign_negative() && is_odd_integer_float(y) {
                return Ok(PyObject::float(f64::NEG_INFINITY));
            }
            return Ok(PyObject::float(f64::INFINITY));
        }
        if y < 0.0 {
            if x.is_sign_negative() && is_odd_integer_float(y) {
                return Ok(PyObject::float(-0.0));
            }
            return Ok(PyObject::float(0.0));
        }
    }
    if x == 0.0 && y < 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    if y.is_infinite() {
        let ax = x.abs();
        if ax == 1.0 {
            return Ok(PyObject::float(1.0));
        }
        let grows = (ax > 1.0) == y.is_sign_positive();
        return Ok(PyObject::float(if grows { f64::INFINITY } else { 0.0 }));
    }
    if x < 0.0 && y.is_finite() && y.fract() != 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    let result = x.powf(y);
    if result.is_nan() && !x.is_nan() && !y.is_nan() {
        return Err(PyException::value_error("math domain error"));
    }
    if result.is_infinite() && x.is_finite() && y.is_finite() {
        return Err(PyException::overflow_error("math range error"));
    }
    Ok(PyObject::float(result))
}
fn math_log(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "math.log requires at least 1 argument",
        ));
    }
    if args.len() > 2 {
        return Err(PyException::type_error(
            "math.log expected at most 2 arguments",
        ));
    }
    let ln_x = math_ln_arg(&args[0])?;
    if args.len() > 1 {
        let ln_base = math_ln_arg(&args[1])?;
        Ok(PyObject::float(ln_x / ln_base))
    } else {
        Ok(PyObject::float(ln_x))
    }
}
fn math_log2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log2", args, 1)?;
    if let PyObjectPayload::Int(n) = &args[0].payload {
        return Ok(PyObject::float(pyint_log2(n)?));
    }
    let x = math_number_to_float(&args[0])?;
    if x <= 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    if let Some(exact) = float_log2_exact_power(x) {
        return Ok(PyObject::float(exact));
    }
    Ok(PyObject::float(x.log2()))
}
fn math_log10(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log10", args, 1)?;
    Ok(PyObject::float(
        math_ln_arg(&args[0])? / std::f64::consts::LN_10,
    ))
}
fn math_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.exp", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x == f64::INFINITY {
        return Ok(PyObject::float(f64::INFINITY));
    }
    if x == f64::NEG_INFINITY {
        return Ok(PyObject::float(0.0));
    }
    let result = x.exp();
    if result.is_infinite() && x.is_finite() {
        return Err(PyException::overflow_error("math range error"));
    }
    Ok(PyObject::float(result))
}
fn math_sin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sin", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.sin()))
}
fn math_cos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.cos", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.cos()))
}
fn math_tan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.tan", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.tan()))
}
fn math_asin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.asin", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if !x.is_nan() && !(-1.0..=1.0).contains(&x) {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.asin()))
}
fn math_acos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.acos", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if !x.is_nan() && !(-1.0..=1.0).contains(&x) {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.acos()))
}
fn math_atan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.atan()))
}
fn math_atan2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan2", args, 2)?;
    Ok(PyObject::float(
        math_number_to_float(&args[0])?.atan2(math_number_to_float(&args[1])?),
    ))
}
fn math_degrees(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.degrees", args, 1)?;
    Ok(PyObject::float(
        math_number_to_float(&args[0])?.to_degrees(),
    ))
}
fn math_radians(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.radians", args, 1)?;
    Ok(PyObject::float(
        math_number_to_float(&args[0])?.to_radians(),
    ))
}
fn math_isnan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isnan", args, 1)?;
    Ok(PyObject::bool_val(math_number_to_float(&args[0])?.is_nan()))
}
fn math_isinf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isinf", args, 1)?;
    Ok(PyObject::bool_val(
        math_number_to_float(&args[0])?.is_infinite(),
    ))
}
fn math_isfinite(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isfinite", args, 1)?;
    Ok(PyObject::bool_val(
        math_number_to_float(&args[0])?.is_finite(),
    ))
}
fn math_gcd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::int(0));
    }
    if args.len() == 1 {
        return Ok(bigint_to_object(index_bigint(&args[0], "gcd")?.abs()));
    }
    let mut result = index_bigint(&args[0], "gcd")?.abs();
    for arg in &args[1..] {
        result = result.gcd(&index_bigint(arg, "gcd")?.abs());
    }
    Ok(bigint_to_object(result))
}
fn math_factorial(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.factorial", args, 1)?;
    let n = match &args[0].payload {
        PyObjectPayload::Int(PyInt::Small(v)) => *v,
        PyObjectPayload::Int(PyInt::Big(v)) => {
            if v.sign() == Sign::Minus {
                return Err(PyException::value_error(
                    "factorial() not defined for negative values",
                ));
            }
            v.to_i64()
                .ok_or_else(|| PyException::value_error("factorial() argument too large"))?
        }
        PyObjectPayload::Bool(b) => {
            if *b {
                1
            } else {
                0
            }
        }
        PyObjectPayload::Float(f) => {
            if *f < 0.0 {
                return Err(PyException::value_error(
                    "factorial() not defined for negative values",
                ));
            }
            if !f.is_finite() || f.fract() != 0.0 {
                return Err(PyException::value_error(
                    "factorial() only accepts integral values",
                ));
            }
            if *f > i64::MAX as f64 || *f < i64::MIN as f64 {
                return Err(PyException::overflow_error(
                    "factorial() argument too large",
                ));
            }
            *f as i64
        }
        _ => {
            return Err(PyException::type_error(
                "factorial() argument must be an integer",
            ))
        }
    };
    if n < 0 {
        return Err(PyException::value_error(
            "factorial() not defined for negative values",
        ));
    }
    let mut result = BigInt::one();
    for i in 2..=n {
        result *= i;
    }
    Ok(PyObject::big_int(result))
}

fn math_isqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("isqrt", args, 1)?;
    let n = index_bigint(&args[0], "isqrt")?;
    if n.sign() == Sign::Minus {
        return Err(PyException::value_error("isqrt() argument must be >= 0"));
    }
    Ok(bigint_to_object(isqrt_bigint(&n)))
}
fn math_trunc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.trunc", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.trunc() as i64))
}
fn math_copysign(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.copysign", args, 2)?;
    Ok(PyObject::float(
        math_number_to_float(&args[0])?.copysign(math_number_to_float(&args[1])?),
    ))
}
fn math_hypot(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mut values = Vec::with_capacity(args.len());
    let mut max = 0.0f64;
    let mut has_nan = false;
    for arg in args {
        let x = math_number_to_float(arg)?.abs();
        if x.is_infinite() {
            return Ok(PyObject::float(f64::INFINITY));
        }
        if x.is_nan() {
            has_nan = true;
        } else if x > max {
            max = x;
        }
        values.push(x);
    }
    if has_nan {
        return Ok(PyObject::float(f64::NAN));
    }
    if max == 0.0 {
        return Ok(PyObject::float(0.0));
    }
    let mut sum = 0.0;
    for value in values {
        let scaled = value / max;
        sum += scaled * scaled;
    }
    Ok(PyObject::float(max * sum.sqrt()))
}
fn math_modf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.modf", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x.is_infinite() {
        return Ok(PyObject::tuple(vec![
            PyObject::float(0.0f64.copysign(x)),
            PyObject::float(x),
        ]));
    }
    let fract = x.fract();
    let trunc = x.trunc();
    Ok(PyObject::tuple(vec![
        PyObject::float(fract),
        PyObject::float(trunc),
    ]))
}
fn math_fmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fmod", args, 2)?;
    let x = math_number_to_float(&args[0])?;
    let y = math_number_to_float(&args[1])?;
    if y == 0.0 || x.is_infinite() {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x % y))
}
fn math_frexp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.frexp", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x.is_nan() || x.is_infinite() || x == 0.0 {
        return Ok(PyObject::tuple(vec![PyObject::float(x), PyObject::int(0)]));
    }
    let (m, e) = frexp(x);
    Ok(PyObject::tuple(vec![
        PyObject::float(m),
        PyObject::int(e as i64),
    ]))
}
fn math_ldexp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.ldexp", args, 2)?;
    let x = math_number_to_float(&args[0])?;
    let exponent = index_bigint(&args[1], "ldexp")?;
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return Ok(PyObject::float(x));
    }
    let Some(i) = exponent.to_i32() else {
        if exponent.sign() == Sign::Minus {
            return Ok(PyObject::float(0.0f64.copysign(x)));
        }
        return Err(PyException::overflow_error("math range error"));
    };
    // libc ldexp/scalbn preserves subnormal results that powi-based scaling loses.
    let result = unsafe { ldexp(x, i as libc::c_int) };
    if result.is_infinite() {
        return Err(PyException::overflow_error("math range error"));
    }
    Ok(PyObject::float(result))
}

fn math_isclose(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "isclose() requires at least 2 arguments",
        ));
    }
    let a = math_number_to_float(&args[0])?;
    let b = math_number_to_float(&args[1])?;
    // Extract rel_tol and abs_tol from positional args or trailing kwargs dict
    let mut rel_tol = 1e-9;
    let mut abs_tol = 0.0;
    let remaining = &args[2..];
    for arg in remaining {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            let map = d.read();
            if let Some(v) = map.get(&HashableKey::str_key(CompactString::from("rel_tol"))) {
                rel_tol = math_number_to_float(v)?;
            }
            if let Some(v) = map.get(&HashableKey::str_key(CompactString::from("abs_tol"))) {
                abs_tol = math_number_to_float(v)?;
            }
        } else if rel_tol == 1e-9 && abs_tol == 0.0 {
            // First non-dict remaining arg = rel_tol
            rel_tol = math_number_to_float(arg)?;
        } else {
            abs_tol = math_number_to_float(arg)?;
        }
    }
    if rel_tol < 0.0 || abs_tol < 0.0 {
        return Err(PyException::value_error("tolerances must be non-negative"));
    }
    if a == b {
        return Ok(PyObject::bool_val(true));
    }
    if a.is_infinite() || b.is_infinite() {
        return Ok(PyObject::bool_val(false));
    }
    let diff = (a - b).abs();
    Ok(PyObject::bool_val(
        diff <= (rel_tol * a.abs().max(b.abs())).max(abs_tol),
    ))
}

fn math_comb(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.comb", args, 2)?;
    let n = index_bigint(&args[0], "comb")?;
    let k = index_bigint(&args[1], "comb")?;
    if n.sign() == Sign::Minus {
        return Err(PyException::value_error("n must be a non-negative integer"));
    }
    if k.sign() == Sign::Minus {
        return Err(PyException::value_error("k must be a non-negative integer"));
    }
    if k > n {
        return Ok(PyObject::int(0));
    }
    let n_minus_k = &n - &k;
    let k = if k > n_minus_k { n_minus_k } else { k };
    if k.is_zero() {
        return Ok(PyObject::int(1));
    }
    if k.is_one() {
        return Ok(bigint_to_object(n));
    }
    if k == BigInt::from(2) {
        return Ok(bigint_to_object((&n * (&n - 1u32)) / 2u32));
    }
    let Some(k_u64) = k.to_u64() else {
        return Err(PyException::overflow_error("comb() argument too large"));
    };
    if k_u64 > 1_000_000 {
        return Err(PyException::overflow_error("comb() argument too large"));
    }
    let mut result = BigInt::one();
    for i in 1..=k_u64 {
        let i_big = BigInt::from(i);
        result *= &n - &k + &i_big;
        result /= i_big;
    }
    Ok(bigint_to_object(result))
}

fn math_perm(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() || args.len() > 2 {
        return Err(PyException::type_error("perm() requires 1 or 2 arguments"));
    }
    let n = index_bigint(&args[0], "perm")?;
    let k = if args.len() == 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        index_bigint(&args[1], "perm")?
    } else {
        n.clone()
    };
    if n.sign() == Sign::Minus {
        return Err(PyException::value_error("n must be a non-negative integer"));
    }
    if k.sign() == Sign::Minus {
        return Err(PyException::value_error("k must be a non-negative integer"));
    }
    if k > n {
        return Ok(PyObject::int(0));
    }
    if k.is_zero() {
        return Ok(PyObject::int(1));
    }
    if k.is_one() {
        return Ok(bigint_to_object(n));
    }
    if k == BigInt::from(2) {
        return Ok(bigint_to_object(&n * (&n - 1u32)));
    }
    let Some(k_u64) = k.to_u64() else {
        return Err(PyException::overflow_error("perm() argument too large"));
    };
    if k_u64 > 1_000_000 {
        return Err(PyException::overflow_error("perm() argument too large"));
    }
    let mut result = BigInt::one();
    for i in 0..k_u64 {
        result *= &n - i;
    }
    Ok(bigint_to_object(result))
}

fn math_prod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "prod() requires at least 1 argument",
        ));
    }
    let mut positional_end = args.len();
    let mut start = PyObject::int(1);
    if args.len() > 1 {
        if let Some(PyObjectPayload::Dict(d)) = args.last().map(|a| &a.payload) {
            let map = d.read();
            if let Some(v) = map.get(&HashableKey::str_key(CompactString::from("start"))) {
                start = v.clone();
            }
            positional_end -= 1;
        }
    }
    if positional_end != 1 {
        return Err(PyException::type_error(
            "prod() takes exactly 1 positional argument",
        ));
    }
    let items = args[0].to_list()?;
    let mut int_product = match &start.payload {
        PyObjectPayload::Int(PyInt::Small(v)) => Some(BigInt::from(*v)),
        PyObjectPayload::Int(PyInt::Big(v)) => Some(v.as_ref().clone()),
        PyObjectPayload::Bool(b) => Some(BigInt::from(if *b { 1 } else { 0 })),
        _ => None,
    };
    let mut product = start;
    for item in &items {
        if let Some(acc) = int_product.as_mut() {
            match &item.payload {
                PyObjectPayload::Int(PyInt::Small(v)) => {
                    *acc *= *v;
                    continue;
                }
                PyObjectPayload::Int(PyInt::Big(v)) => {
                    *acc *= v.as_ref();
                    continue;
                }
                PyObjectPayload::Bool(b) => {
                    *acc *= if *b { 1 } else { 0 };
                    continue;
                }
                _ => {
                    product = bigint_to_object(acc.clone());
                    int_product = None;
                }
            }
        }
        product = prod_multiply(&product, item)?;
    }
    if let Some(product) = int_product {
        Ok(bigint_to_object(product))
    } else {
        Ok(product)
    }
}

fn prod_multiply(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
        if let Some(method) = a.get_attr("__mul__") {
            let result = ferrython_core::object::call_callable(&method, std::slice::from_ref(b))?;
            if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                return Ok(result);
            }
        }
    }
    if matches!(&b.payload, PyObjectPayload::Instance(_)) {
        if let Some(method) = b.get_attr("__rmul__").or_else(|| b.get_attr("__mul__")) {
            let result = ferrython_core::object::call_callable(&method, std::slice::from_ref(a))?;
            if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                return Ok(result);
            }
        }
    }
    let result = a.mul(b)?;
    if matches!(&result.payload, PyObjectPayload::NotImplemented) {
        Err(PyException::type_error(format!(
            "unsupported operand type(s) for *: '{}' and '{}'",
            a.type_name(),
            b.type_name()
        )))
    } else {
        Ok(result)
    }
}

fn math_lcm(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::int(0));
    }
    fn gcd(a: i64, b: i64) -> i64 {
        if b == 0 {
            a.abs()
        } else {
            gcd(b, a % b)
        }
    }
    let mut result = args[0].to_int()?.abs();
    for arg in &args[1..] {
        let b = arg.to_int()?.abs();
        if b == 0 {
            return Ok(PyObject::int(0));
        }
        result = result / gcd(result, b) * b;
    }
    Ok(PyObject::int(result))
}

fn math_remainder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.remainder", args, 2)?;
    let x = math_number_to_float(&args[0])?;
    let y = math_number_to_float(&args[1])?;
    if x.is_nan() || y.is_nan() {
        return Ok(PyObject::float(f64::NAN));
    }
    if y == 0.0 || x.is_infinite() {
        return Err(PyException::value_error("math domain error"));
    }
    if y.is_infinite() {
        return Ok(PyObject::float(x));
    }
    Ok(PyObject::float(unsafe { c_remainder(x, y) }))
}

fn math_expm1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.expm1", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.exp_m1()))
}

fn math_log1p(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log1p", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if !x.is_nan() && x <= -1.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.ln_1p()))
}

fn math_sinh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sinh", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.sinh()))
}
fn math_cosh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.cosh", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.cosh()))
}
fn math_tanh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.tanh", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.tanh()))
}
fn math_asinh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.asinh", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.asinh()))
}
fn math_acosh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.acosh", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if !x.is_nan() && x < 1.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.acosh()))
}
fn math_atanh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atanh", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if !x.is_nan() && x.abs() >= 1.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.atanh()))
}

fn math_erf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.erf", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    // Abramowitz and Stegun approximation (7.1.26)
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let erf = 1.0 - poly * (-x * x).exp();
    Ok(PyObject::float(if x < 0.0 { -erf } else { erf }))
}
fn math_erfc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.erfc", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let erf = 1.0 - poly * (-x * x).exp();
    Ok(PyObject::float(if x < 0.0 { 1.0 + erf } else { 1.0 - erf }))
}
fn math_gamma(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.gamma", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x <= 0.0 && x == x.floor() {
        return Err(PyException::value_error("math domain error"));
    }
    // Lanczos approximation
    Ok(PyObject::float(lanczos_gamma(x)))
}
fn math_lgamma(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.lgamma", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x <= 0.0 && x == x.floor() {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(lanczos_gamma(x).abs().ln()))
}

fn math_fsum(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fsum", args, 1)?;
    let items = args[0].to_list()?;
    let mut total = BigInt::zero();
    let mut total_exp = 0i32;
    let mut initialized = false;
    let mut pos_inf = false;
    let mut neg_inf = false;
    let mut has_nan = false;
    for item in &items {
        let x = math_number_to_float(item)?;
        if x.is_nan() {
            has_nan = true;
            continue;
        }
        if x.is_infinite() {
            if x.is_sign_positive() {
                pos_inf = true;
            } else {
                neg_inf = true;
            }
            continue;
        }
        if x == 0.0 {
            continue;
        }
        let bits = x.to_bits();
        let negative = (bits >> 63) != 0;
        let raw_exp = ((bits >> 52) & 0x7ff) as i32;
        let frac = bits & 0x000f_ffff_ffff_ffff;
        let (mantissa, exp) = if raw_exp == 0 {
            (frac, -1074)
        } else {
            ((1u64 << 52) | frac, raw_exp - 1075)
        };
        let mut mant = BigInt::from(mantissa);
        if negative {
            mant = -mant;
        }
        if !initialized {
            total = mant;
            total_exp = exp;
            initialized = true;
        } else if exp < total_exp {
            total <<= (total_exp - exp) as usize;
            total += mant;
            total_exp = exp;
        } else {
            mant <<= (exp - total_exp) as usize;
            total += mant;
        }
    }
    if pos_inf && neg_inf {
        return Err(PyException::value_error("-inf + inf in fsum"));
    }
    if pos_inf {
        return Ok(PyObject::float(f64::INFINITY));
    }
    if neg_inf {
        return Ok(PyObject::float(f64::NEG_INFINITY));
    }
    if has_nan {
        return Ok(PyObject::float(f64::NAN));
    }
    if total.is_zero() {
        return Ok(PyObject::float(0.0));
    }

    let sign = total.sign();
    let mut mant = total.abs();
    let bit_len = mant.bits() as i32;
    let tail = (bit_len - 53).max(-1074 - total_exp);
    if tail > 0 {
        let mask = (BigInt::one() << tail as usize) - 1u32;
        let remainder = &mant & &mask;
        mant >>= tail as usize;
        let half = BigInt::one() << (tail as usize - 1);
        if remainder > half || (remainder == half && (&mant & BigInt::one()).is_one()) {
            mant += 1u32;
        }
        total_exp += tail;
    }
    let mut rounded = mant
        .to_f64()
        .ok_or_else(|| PyException::overflow_error("intermediate overflow in fsum"))?;
    if sign == Sign::Minus {
        rounded = -rounded;
    }
    let result = unsafe { ldexp(rounded, total_exp as libc::c_int) };
    if result.is_infinite() {
        return Err(PyException::overflow_error("intermediate overflow in fsum"));
    }
    Ok(PyObject::float(result))
}

fn math_dist(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.dist", args, 2)?;
    let p = args[0].to_list()?;
    let q = args[1].to_list()?;
    if p.len() != q.len() {
        return Err(PyException::value_error(
            "both points must have the same number of dimensions",
        ));
    }
    let mut diffs = Vec::with_capacity(p.len());
    let mut max = 0.0f64;
    let mut has_nan = false;
    for (a, b) in p.iter().zip(q.iter()) {
        let diff = (math_number_to_float(a)? - math_number_to_float(b)?).abs();
        if diff.is_infinite() {
            return Ok(PyObject::float(f64::INFINITY));
        }
        if diff.is_nan() {
            has_nan = true;
        } else if diff > max {
            max = diff;
        }
        diffs.push(diff);
    }
    if has_nan {
        return Ok(PyObject::float(f64::NAN));
    }
    if max == 0.0 {
        return Ok(PyObject::float(0.0));
    }
    let mut sum = 0.0f64;
    for diff in diffs {
        let scaled = diff / max;
        sum += scaled * scaled;
    }
    Ok(PyObject::float(max * sum.sqrt()))
}

fn lanczos_gamma(x: f64) -> f64 {
    if x < 0.5 {
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * lanczos_gamma(1.0 - x))
    } else {
        let g = 7.0;
        let coefs = [
            0.99999999999980993,
            676.5203681218851,
            -1259.1392167224028,
            771.32342877765313,
            -176.61502916214059,
            12.507343278686905,
            -0.13857109526572012,
            9.9843695780195716e-6,
            1.5056327351493116e-7,
        ];
        let z = x - 1.0;
        let mut sum = coefs[0];
        for (i, &c) in coefs[1..].iter().enumerate() {
            sum += c / (z + i as f64 + 1.0);
        }
        let t = z + g + 0.5;
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(z + 0.5) * (-t).exp() * sum
    }
}

fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 {
        return (0.0, 0);
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1022;
    let mantissa = f64::from_bits((bits & 0x800FFFFFFFFFFFFF) | 0x3FE0000000000000);
    (mantissa, exp)
}

// ── sys module ──

fn stats_extract_floats(args: &[PyObjectRef]) -> PyResult<Vec<f64>> {
    if args.is_empty() {
        return Err(PyException::type_error("requires at least 1 argument"));
    }
    let items = args[0].to_list()?;
    if items.is_empty() {
        return Err(PyException::value_error("requires a non-empty dataset"));
    }
    Ok(items
        .iter()
        .map(|x| x.to_float().unwrap_or(x.as_int().unwrap_or(0) as f64))
        .collect())
}

pub fn create_statistics_module() -> PyObjectRef {
    make_module(
        "statistics",
        vec![
            (
                "mean",
                make_builtin(|args| {
                    let vals = stats_extract_floats(args)?;
                    Ok(PyObject::float(
                        vals.iter().sum::<f64>() / vals.len() as f64,
                    ))
                }),
            ),
            (
                "median",
                make_builtin(|args| {
                    let mut vals = stats_extract_floats(args)?;
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let n = vals.len();
                    if n % 2 == 1 {
                        Ok(PyObject::float(vals[n / 2]))
                    } else {
                        Ok(PyObject::float((vals[n / 2 - 1] + vals[n / 2]) / 2.0))
                    }
                }),
            ),
            (
                "median_low",
                make_builtin(|args| {
                    let mut vals = stats_extract_floats(args)?;
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let n = vals.len();
                    if n % 2 == 1 {
                        Ok(PyObject::float(vals[n / 2]))
                    } else {
                        Ok(PyObject::float(vals[n / 2 - 1]))
                    }
                }),
            ),
            (
                "median_high",
                make_builtin(|args| {
                    let mut vals = stats_extract_floats(args)?;
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let n = vals.len();
                    Ok(PyObject::float(vals[n / 2]))
                }),
            ),
            (
                "mode",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("mode requires 1 argument"));
                    }
                    let items = args[0].to_list()?;
                    if items.is_empty() {
                        return Err(PyException::value_error(
                            "mode requires a non-empty dataset",
                        ));
                    }
                    let mut counts: IndexMap<String, (PyObjectRef, usize)> = IndexMap::new();
                    for item in &items {
                        let key = item.py_to_string();
                        counts.entry(key).or_insert_with(|| (item.clone(), 0)).1 += 1;
                    }
                    let max_count = counts.values().map(|v| v.1).max().unwrap();
                    let modes: Vec<_> = counts.values().filter(|v| v.1 == max_count).collect();
                    if modes.len() > 1 {
                        return Err(PyException::runtime_error(
                            "no unique mode; found multiple equally common values",
                        ));
                    }
                    Ok(modes[0].0.clone())
                }),
            ),
            (
                "multimode",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("multimode requires 1 argument"));
                    }
                    let items = args[0].to_list()?;
                    if items.is_empty() {
                        return Ok(PyObject::list(vec![]));
                    }
                    let mut counts: IndexMap<String, (PyObjectRef, usize)> = IndexMap::new();
                    for item in &items {
                        let key = item.py_to_string();
                        counts.entry(key).or_insert_with(|| (item.clone(), 0)).1 += 1;
                    }
                    let max_count = counts.values().map(|v| v.1).max().unwrap();
                    let modes: Vec<PyObjectRef> = counts
                        .values()
                        .filter(|v| v.1 == max_count)
                        .map(|v| v.0.clone())
                        .collect();
                    Ok(PyObject::list(modes))
                }),
            ),
            (
                "stdev",
                make_builtin(|args| {
                    let vals = stats_extract_floats(args)?;
                    if vals.len() < 2 {
                        return Err(PyException::value_error(
                            "stdev requires at least 2 data points",
                        ));
                    }
                    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                    let var = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>()
                        / (vals.len() - 1) as f64;
                    Ok(PyObject::float(var.sqrt()))
                }),
            ),
            (
                "variance",
                make_builtin(|args| {
                    let vals = stats_extract_floats(args)?;
                    if vals.len() < 2 {
                        return Err(PyException::value_error(
                            "variance requires at least 2 data points",
                        ));
                    }
                    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                    let var = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>()
                        / (vals.len() - 1) as f64;
                    Ok(PyObject::float(var))
                }),
            ),
            (
                "pstdev",
                make_builtin(|args| {
                    let vals = stats_extract_floats(args)?;
                    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                    let var =
                        vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / vals.len() as f64;
                    Ok(PyObject::float(var.sqrt()))
                }),
            ),
            (
                "pvariance",
                make_builtin(|args| {
                    let vals = stats_extract_floats(args)?;
                    let mean = vals.iter().sum::<f64>() / vals.len() as f64;
                    let var =
                        vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / vals.len() as f64;
                    Ok(PyObject::float(var))
                }),
            ),
            (
                "harmonic_mean",
                make_builtin(|args| {
                    let vals = stats_extract_floats(args)?;
                    for v in &vals {
                        if *v <= 0.0 {
                            return Err(PyException::value_error(
                                "harmonic_mean requires positive data",
                            ));
                        }
                    }
                    let reciprocal_sum: f64 = vals.iter().map(|x| 1.0 / x).sum();
                    Ok(PyObject::float(vals.len() as f64 / reciprocal_sum))
                }),
            ),
            (
                "geometric_mean",
                make_builtin(|args| {
                    let vals = stats_extract_floats(args)?;
                    for v in &vals {
                        if *v <= 0.0 {
                            return Err(PyException::value_error(
                                "geometric_mean requires positive data",
                            ));
                        }
                    }
                    let log_mean = vals.iter().map(|x| x.ln()).sum::<f64>() / vals.len() as f64;
                    Ok(PyObject::float(log_mean.exp()))
                }),
            ),
            (
                "quantiles",
                make_builtin(|args| {
                    let vals = stats_extract_floats(args)?;
                    let n = if args.len() >= 2 {
                        args[1].to_int().unwrap_or(4) as usize
                    } else {
                        4
                    };
                    if n < 1 {
                        return Err(PyException::value_error("n must be at least 1"));
                    }
                    let mut sorted = vals.clone();
                    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let m = sorted.len();
                    let mut result = Vec::new();
                    for i in 1..n {
                        let idx = (i as f64 * m as f64) / n as f64;
                        let lo = (idx - 0.5).floor().max(0.0) as usize;
                        let hi = lo + 1;
                        if hi >= m {
                            result.push(PyObject::float(sorted[m - 1]));
                        } else {
                            let frac = idx - 0.5 - lo as f64;
                            let val = sorted[lo] + frac * (sorted[hi] - sorted[lo]);
                            result.push(PyObject::float(val));
                        }
                    }
                    Ok(PyObject::list(result))
                }),
            ),
            (
                "StatisticsError",
                PyObject::str_val(CompactString::from("StatisticsError")),
            ),
        ],
    )
}

// ── numbers module (stub) ──

pub fn create_numbers_module() -> PyObjectRef {
    // Abstract method that raises NotImplementedError
    fn make_abstract(name: &str) -> PyObjectRef {
        let n = CompactString::from(name);
        PyObject::native_closure(name, move |_args: &[PyObjectRef]| {
            Err(PyException::type_error(format!("{} is abstract", n)))
        })
    }

    // Number — root of the numeric tower
    let mut number_ns = IndexMap::new();
    number_ns.insert(
        CompactString::from("__hash__"),
        make_abstract("Number.__hash__"),
    );
    let number_class = PyObject::class(CompactString::from("Number"), vec![], number_ns);

    // Complex — adds complex arithmetic operations
    let mut complex_ns = IndexMap::new();
    for op in &[
        "__add__",
        "__radd__",
        "__sub__",
        "__rsub__",
        "__mul__",
        "__rmul__",
        "__truediv__",
        "__rtruediv__",
        "__pow__",
        "__rpow__",
        "__neg__",
        "__pos__",
        "__abs__",
        "__complex__",
        "__eq__",
        "__hash__",
        "real",
        "imag",
        "conjugate",
    ] {
        complex_ns.insert(
            CompactString::from(*op),
            make_abstract(&format!("Complex.{}", op)),
        );
    }
    complex_ns.insert(
        CompactString::from("__bool__"),
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(true))),
    );
    let complex_class = PyObject::class(
        CompactString::from("Complex"),
        vec![number_class.clone()],
        complex_ns,
    );

    // Real — adds ordering and real-valued operations
    let mut real_ns = IndexMap::new();
    for op in &[
        "__float__",
        "__trunc__",
        "__floor__",
        "__ceil__",
        "__round__",
        "__floordiv__",
        "__rfloordiv__",
        "__mod__",
        "__rmod__",
        "__lt__",
        "__le__",
    ] {
        real_ns.insert(
            CompactString::from(*op),
            make_abstract(&format!("Real.{}", op)),
        );
    }
    real_ns.insert(
        CompactString::from("real"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::float(0.0));
            }
            Ok(args[0].clone())
        }),
    );
    real_ns.insert(
        CompactString::from("imag"),
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::int(0))),
    );
    real_ns.insert(
        CompactString::from("conjugate"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::float(0.0));
            }
            Ok(args[0].clone())
        }),
    );
    let real_class = PyObject::class(
        CompactString::from("Real"),
        vec![complex_class.clone()],
        real_ns,
    );

    // Rational — adds numerator/denominator
    let mut rational_ns = IndexMap::new();
    rational_ns.insert(
        CompactString::from("numerator"),
        make_abstract("Rational.numerator"),
    );
    rational_ns.insert(
        CompactString::from("denominator"),
        make_abstract("Rational.denominator"),
    );
    rational_ns.insert(
        CompactString::from("__float__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::float(0.0));
            }
            let self_obj = &args[0];
            if let (Some(num), Some(den)) = (
                self_obj.get_attr("numerator"),
                self_obj.get_attr("denominator"),
            ) {
                let n = num.to_int().unwrap_or(0) as f64;
                let d = den.to_int().unwrap_or(1) as f64;
                return Ok(PyObject::float(if d != 0.0 { n / d } else { f64::NAN }));
            }
            Ok(PyObject::float(0.0))
        }),
    );
    let rational_class = PyObject::class(
        CompactString::from("Rational"),
        vec![real_class.clone()],
        rational_ns,
    );

    // Integral — adds integer-specific operations
    let mut integral_ns = IndexMap::new();
    for op in &[
        "__int__",
        "__index__",
        "__lshift__",
        "__rlshift__",
        "__rshift__",
        "__rrshift__",
        "__and__",
        "__rand__",
        "__xor__",
        "__rxor__",
        "__or__",
        "__ror__",
        "__invert__",
    ] {
        integral_ns.insert(
            CompactString::from(*op),
            make_abstract(&format!("Integral.{}", op)),
        );
    }
    integral_ns.insert(
        CompactString::from("__float__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::float(0.0));
            }
            let v = args[0].to_int().unwrap_or(0);
            Ok(PyObject::float(v as f64))
        }),
    );
    integral_ns.insert(
        CompactString::from("numerator"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            Ok(args[0].clone())
        }),
    );
    integral_ns.insert(
        CompactString::from("denominator"),
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::int(1))),
    );
    let integral_class = PyObject::class(
        CompactString::from("Integral"),
        vec![rational_class.clone()],
        integral_ns,
    );

    make_module(
        "numbers",
        vec![
            ("Number", number_class),
            ("Complex", complex_class),
            ("Real", real_class),
            ("Rational", rational_class),
            ("Integral", integral_class),
        ],
    )
}

// ── platform module ──

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
                    })),
                )))
            }),
        ),
    ]);

    make_module("decimal", module_entries)
}

// ── statistics module ──

pub fn create_random_module() -> PyObjectRef {
    make_module(
        "random",
        vec![
            ("random", make_builtin(random_random)),
            ("randint", make_builtin(random_randint)),
            ("choice", make_builtin(random_choice)),
            ("shuffle", make_builtin(random_shuffle)),
            ("seed", make_builtin(random_seed)),
            ("randrange", make_builtin(random_randrange)),
            (
                "uniform",
                make_builtin(|args| {
                    check_args("random.uniform", args, 2)?;
                    let a = args[0].to_float()?;
                    let b = args[1].to_float()?;
                    Ok(PyObject::float(a + simple_random() * (b - a)))
                }),
            ),
            (
                "sample",
                make_builtin(|args| {
                    check_args("random.sample", args, 2)?;
                    let items = args[0].to_list()?;
                    let k = args[1].to_int()? as usize;
                    if k > items.len() {
                        return Err(PyException::value_error("Sample larger than population"));
                    }
                    let mut result = Vec::with_capacity(k);
                    let mut pool = items.clone();
                    for _ in 0..k {
                        let idx = (simple_random() * pool.len() as f64) as usize;
                        let idx = idx.min(pool.len() - 1);
                        result.push(pool.remove(idx));
                    }
                    Ok(PyObject::list(result))
                }),
            ),
            (
                "choices",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "random.choices requires at least 1 argument",
                        ));
                    }
                    let items = args[0].to_list()?;
                    let mut k = 1usize;
                    let mut weights: Option<Vec<f64>> = None;
                    for arg in args.iter().skip(1) {
                        if let PyObjectPayload::Dict(d) = &arg.payload {
                            let d = d.read();
                            if let Some(kv) = d.get(&HashableKey::str_key(CompactString::from("k")))
                            {
                                k = kv.to_int()? as usize;
                            }
                            if let Some(wv) =
                                d.get(&HashableKey::str_key(CompactString::from("weights")))
                            {
                                let wl = wv.to_list()?;
                                weights =
                                    Some(wl.iter().map(|w| w.to_float().unwrap_or(1.0)).collect());
                            }
                            if let Some(cwv) =
                                d.get(&HashableKey::str_key(CompactString::from("cum_weights")))
                            {
                                let cwl = cwv.to_list()?;
                                let cw: Vec<f64> =
                                    cwl.iter().map(|w| w.to_float().unwrap_or(0.0)).collect();
                                // Convert cumulative weights back to regular weights
                                let mut w = Vec::with_capacity(cw.len());
                                for i in 0..cw.len() {
                                    w.push(if i == 0 { cw[0] } else { cw[i] - cw[i - 1] });
                                }
                                weights = Some(w);
                            }
                        }
                    }
                    if items.is_empty() {
                        return Err(PyException::value_error(
                            "Cannot choose from an empty population",
                        ));
                    }
                    let mut result = Vec::with_capacity(k);
                    if let Some(ref w) = weights {
                        let total: f64 = w.iter().sum();
                        for _ in 0..k {
                            let mut r = simple_random() * total;
                            let mut chosen = items.len() - 1;
                            for (i, &weight) in w.iter().enumerate() {
                                r -= weight;
                                if r <= 0.0 {
                                    chosen = i;
                                    break;
                                }
                            }
                            result.push(items[chosen.min(items.len() - 1)].clone());
                        }
                    } else {
                        for _ in 0..k {
                            let idx = (simple_random() * items.len() as f64) as usize;
                            result.push(items[idx.min(items.len() - 1)].clone());
                        }
                    }
                    Ok(PyObject::list(result))
                }),
            ),
            (
                "gauss",
                make_builtin(|args| {
                    let mu = if !args.is_empty() {
                        args[0].to_float()?
                    } else {
                        0.0
                    };
                    let sigma = if args.len() > 1 {
                        args[1].to_float()?
                    } else {
                        1.0
                    };
                    // Box-Muller transform
                    let u1 = simple_random().max(1e-10);
                    let u2 = simple_random();
                    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                    Ok(PyObject::float(mu + sigma * z))
                }),
            ),
            (
                "normalvariate",
                make_builtin(|args| {
                    let mu = if !args.is_empty() {
                        args[0].to_float()?
                    } else {
                        0.0
                    };
                    let sigma = if args.len() > 1 {
                        args[1].to_float()?
                    } else {
                        1.0
                    };
                    let u1 = simple_random().max(1e-10);
                    let u2 = simple_random();
                    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                    Ok(PyObject::float(mu + sigma * z))
                }),
            ),
            (
                "expovariate",
                make_builtin(|args| {
                    check_args("random.expovariate", args, 1)?;
                    let lambd = args[0].to_float()?;
                    if lambd == 0.0 {
                        return Err(PyException::value_error("expovariate: lambd must not be 0"));
                    }
                    let u = simple_random().max(1e-10);
                    Ok(PyObject::float(-u.ln() / lambd))
                }),
            ),
            (
                "triangular",
                make_builtin(|args| {
                    let low = if !args.is_empty() {
                        args[0].to_float()?
                    } else {
                        0.0
                    };
                    let high = if args.len() > 1 {
                        args[1].to_float()?
                    } else {
                        1.0
                    };
                    let mode = if args.len() > 2 {
                        args[2].to_float()?
                    } else {
                        (low + high) / 2.0
                    };
                    let u = simple_random();
                    let c = (mode - low) / (high - low);
                    if u < c {
                        Ok(PyObject::float(
                            low + (u * (high - low) * (mode - low)).sqrt(),
                        ))
                    } else {
                        Ok(PyObject::float(
                            high - ((1.0 - u) * (high - low) * (high - mode)).sqrt(),
                        ))
                    }
                }),
            ),
            (
                "getrandbits",
                make_builtin(|args| {
                    check_args("random.getrandbits", args, 1)?;
                    let k = args[0].to_int()? as u32;
                    if k == 0 {
                        return Ok(PyObject::int(0));
                    }
                    let mut result: i64 = 0;
                    for _ in 0..k.min(62) {
                        result = (result << 1) | (if simple_random() < 0.5 { 1 } else { 0 });
                    }
                    Ok(PyObject::int(result))
                }),
            ),
            (
                "getstate",
                make_builtin(|_| {
                    RNG.with(|rng| {
                        let r = rng.borrow();
                        Ok(PyObject::tuple(vec![
                            PyObject::int(r.s[0] as i64),
                            PyObject::int(r.s[1] as i64),
                            PyObject::int(r.s[2] as i64),
                            PyObject::int(r.s[3] as i64),
                        ]))
                    })
                }),
            ),
            (
                "setstate",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("setstate() requires 1 argument"));
                    }
                    let state = &args[0];
                    if let PyObjectPayload::Tuple(items) = &state.payload {
                        if items.len() >= 4 {
                            let s0 = items[0].to_int()? as u64;
                            let s1 = items[1].to_int()? as u64;
                            let s2 = items[2].to_int()? as u64;
                            let s3 = items[3].to_int()? as u64;
                            RNG.with(|rng| {
                                let mut r = rng.borrow_mut();
                                r.s = [s0, s1, s2, s3];
                            });
                            return Ok(PyObject::none());
                        }
                    }
                    Err(PyException::type_error(
                        "state must be a 4-tuple of integers",
                    ))
                }),
            ),
            (
                "Random",
                make_builtin(|_args| {
                    let mut attrs = IndexMap::new();
                    attrs.insert(CompactString::from("random"), make_builtin(random_random));
                    attrs.insert(CompactString::from("randint"), make_builtin(random_randint));
                    attrs.insert(CompactString::from("choice"), make_builtin(random_choice));
                    attrs.insert(CompactString::from("shuffle"), make_builtin(random_shuffle));
                    attrs.insert(CompactString::from("seed"), make_builtin(random_seed));
                    attrs.insert(
                        CompactString::from("randrange"),
                        make_builtin(random_randrange),
                    );
                    attrs.insert(
                        CompactString::from("uniform"),
                        make_builtin(|args| {
                            check_args("Random.uniform", args, 2)?;
                            let a = args[0].to_float()?;
                            let b = args[1].to_float()?;
                            Ok(PyObject::float(a + simple_random() * (b - a)))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("sample"),
                        make_builtin(|args| {
                            check_args("Random.sample", args, 2)?;
                            let items = args[0].to_list()?;
                            let k = args[1].to_int()? as usize;
                            if k > items.len() {
                                return Err(PyException::value_error(
                                    "Sample larger than population",
                                ));
                            }
                            let mut result = Vec::with_capacity(k);
                            let mut pool = items.clone();
                            for _ in 0..k {
                                let idx = (simple_random() * pool.len() as f64) as usize;
                                let idx = idx.min(pool.len() - 1);
                                result.push(pool.remove(idx));
                            }
                            Ok(PyObject::list(result))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("gauss"),
                        make_builtin(|args| {
                            check_args("Random.gauss", args, 2)?;
                            let mu = args[0].to_float()?;
                            let sigma = args[1].to_float()?;
                            let u1 = simple_random();
                            let u2 = simple_random();
                            let z =
                                (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                            Ok(PyObject::float(mu + sigma * z))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("getrandbits"),
                        make_builtin(|args| {
                            check_args("Random.getrandbits", args, 1)?;
                            let k = args[0].to_int()?;
                            if k <= 0 {
                                return Err(PyException::value_error(
                                    "number of bits must be greater than zero",
                                ));
                            }
                            let val = if k <= 64 {
                                (simple_random() * (1u64 << k.min(63)) as f64) as i64
                            } else {
                                (simple_random() * i64::MAX as f64) as i64
                            };
                            Ok(PyObject::int(val))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("getstate"),
                        make_builtin(|_| Ok(PyObject::tuple(vec![]))),
                    );
                    attrs.insert(
                        CompactString::from("setstate"),
                        make_builtin(|_| Ok(PyObject::none())),
                    );
                    Ok(PyObject::module_with_attrs(
                        CompactString::from("Random"),
                        attrs,
                    ))
                }),
            ),
            (
                "SystemRandom",
                make_builtin(|_args| {
                    let mut attrs = IndexMap::new();
                    attrs.insert(CompactString::from("random"), make_builtin(random_random));
                    attrs.insert(CompactString::from("randint"), make_builtin(random_randint));
                    attrs.insert(CompactString::from("choice"), make_builtin(random_choice));
                    attrs.insert(
                        CompactString::from("randrange"),
                        make_builtin(random_randrange),
                    );
                    attrs.insert(
                        CompactString::from("getrandbits"),
                        make_builtin(|args| {
                            check_args("SystemRandom.getrandbits", args, 1)?;
                            let k = args[0].to_int()?;
                            let val = (simple_random() * (1u64 << k.min(63)) as f64) as i64;
                            Ok(PyObject::int(val))
                        }),
                    );
                    Ok(PyObject::module_with_attrs(
                        CompactString::from("SystemRandom"),
                        attrs,
                    ))
                }),
            ),
        ],
    )
}

// ── Seeded PRNG (xoshiro256**) for reproducible random sequences ──

use std::cell::RefCell;

/// Xoshiro256** state — fast, high-quality PRNG with proper seeding support.
struct Xoshiro256 {
    s: [u64; 4],
}

impl Xoshiro256 {
    fn new(seed: u64) -> Self {
        // SplitMix64 to expand a single u64 seed into 4 state words
        let mut z = seed;
        let mut s = [0u64; 4];
        for item in &mut s {
            z = z.wrapping_add(0x9e3779b97f4a7c15);
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            *item = z ^ (z >> 31);
        }
        Self { s }
    }

    fn next_u64(&mut self) -> u64 {
        let result = (self.s[1].wrapping_mul(5)).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

thread_local! {
    static RNG: RefCell<Xoshiro256> = RefCell::new({
        // Default seed from system time + thread id
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default().as_nanos() as u64;
        let tid = format!("{:?}", std::thread::current().id()).len() as u64;
        Xoshiro256::new(nanos ^ tid.wrapping_mul(0x517cc1b727220a95))
    });
}

fn simple_random() -> f64 {
    RNG.with(|rng| rng.borrow_mut().next_f64())
}

fn random_random(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::float(simple_random()))
}
fn random_randint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.randint", args, 2)?;
    let a = args[0].to_int()?;
    let b = args[1].to_int()?;
    let range = (b - a + 1) as f64;
    Ok(PyObject::int(a + (simple_random() * range) as i64))
}
fn random_choice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.choice", args, 1)?;
    let items = args[0].to_list()?;
    if items.is_empty() {
        return Err(PyException::index_error(
            "Cannot choose from an empty sequence",
        ));
    }
    let idx = (simple_random() * items.len() as f64) as usize;
    Ok(items[idx.min(items.len() - 1)].clone())
}
fn random_shuffle(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.shuffle", args, 1)?;
    // Fisher-Yates in-place shuffle
    if let PyObjectPayload::List(list_arc) = &args[0].payload {
        let mut items = list_arc.write();
        let n = items.len();
        for i in (1..n).rev() {
            let j = (simple_random() * (i + 1) as f64) as usize;
            let j = j.min(i);
            items.swap(i, j);
        }
    }
    Ok(PyObject::none())
}
fn random_seed(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let seed_val: u64 = if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
        // No seed or None → use system time
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    } else {
        match &args[0].payload {
            PyObjectPayload::Int(n) => {
                let v = n.to_i64().unwrap_or(0);
                v as u64
            }
            PyObjectPayload::Float(f) => f.to_bits(),
            PyObjectPayload::Str(s) => {
                // Hash the string for seed
                let mut h: u64 = 0;
                for b in s.as_bytes() {
                    h = h.wrapping_mul(31).wrapping_add(*b as u64);
                }
                h
            }
            _ => args[0].py_to_string().len() as u64,
        }
    };
    RNG.with(|rng| *rng.borrow_mut() = Xoshiro256::new(seed_val));
    Ok(PyObject::none())
}
fn random_randrange(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "randrange requires at least 1 argument",
        ));
    }
    let start = if args.len() == 1 {
        0
    } else {
        args[0].to_int()?
    };
    let stop = if args.len() == 1 {
        args[0].to_int()?
    } else {
        args[1].to_int()?
    };
    let step = if args.len() > 2 { args[2].to_int()? } else { 1 };
    let range = ((stop - start) as f64 / step as f64).ceil() as i64;
    if range <= 0 {
        return Err(PyException::value_error("empty range for randrange()"));
    }
    let idx = (simple_random() * range as f64) as i64;
    Ok(PyObject::int(start + idx * step))
}

// ── Stub modules ──

// ── heapq module ──

pub fn create_heapq_module() -> PyObjectRef {
    create_heapq_module_named("heapq")
}

pub fn create_heapq_accel_module() -> PyObjectRef {
    create_heapq_module_named("_heapq")
}

fn heapq_function(
    module: &str,
    name: &str,
    func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>,
) -> PyObjectRef {
    PyObject::native_function(&format!("{module}.{name}"), func)
}

fn create_heapq_module_named(module: &str) -> PyObjectRef {
    make_module(
        module,
        vec![
            ("heappush", heapq_function(module, "heappush", heapq_push)),
            ("heappop", heapq_function(module, "heappop", heapq_pop)),
            ("heapify", heapq_function(module, "heapify", heapq_heapify)),
            (
                "heappushpop",
                heapq_function(module, "heappushpop", heapq_pushpop),
            ),
            (
                "heapreplace",
                heapq_function(module, "heapreplace", heapq_replace),
            ),
            (
                "_heappop_max",
                heapq_function(module, "_heappop_max", heapq_pop_max),
            ),
            (
                "_heapreplace_max",
                heapq_function(module, "_heapreplace_max", heapq_replace_max),
            ),
            (
                "_heapify_max",
                heapq_function(module, "_heapify_max", heapq_heapify_max),
            ),
            (
                "nlargest",
                heapq_function(module, "nlargest", heapq_nlargest),
            ),
            (
                "nsmallest",
                heapq_function(module, "nsmallest", heapq_nsmallest),
            ),
            ("merge", heapq_function(module, "merge", heapq_merge)),
        ],
    )
}

fn heap_cmp_lt(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
    if let PyObjectPayload::Instance(_) = &a.payload {
        if let Some(method) = a.get_attr("__lt__") {
            if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                let result = call_callable(&method, std::slice::from_ref(b))?;
                if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                    return Ok(result.is_truthy());
                }
            }
        }
    }
    Ok(a.compare(b, CompareOp::Lt)?.is_truthy())
}

fn heap_cmp_lt_checked(
    heap: &PyCell<Vec<PyObjectRef>>,
    a: &PyObjectRef,
    b: &PyObjectRef,
    expected_len: usize,
) -> PyResult<bool> {
    let result = heap_cmp_lt(a, b);
    if heap.read().len() != expected_len {
        return Err(PyException::index_error(
            "list changed size during iteration",
        ));
    }
    result
}

fn heap_pair(
    heap: &PyCell<Vec<PyObjectRef>>,
    left: usize,
    right: usize,
) -> PyResult<(PyObjectRef, PyObjectRef, usize)> {
    let items = heap.read();
    if left >= items.len() || right >= items.len() {
        return Err(PyException::index_error("index out of range"));
    }
    Ok((items[left].clone(), items[right].clone(), items.len()))
}

fn heap_swap(
    heap: &PyCell<Vec<PyObjectRef>>,
    left: usize,
    right: usize,
    expected_len: usize,
) -> PyResult<()> {
    let mut items = heap.write();
    if items.len() != expected_len || left >= items.len() || right >= items.len() {
        return Err(PyException::index_error(
            "list changed size during iteration",
        ));
    }
    items.swap(left, right);
    Ok(())
}

fn heap_sift_up(heap: &PyCell<Vec<PyObjectRef>>, mut pos: usize) -> PyResult<()> {
    while pos > 0 {
        let parent = (pos - 1) / 2;
        let (item, parent_item, expected_len) = heap_pair(heap, pos, parent)?;
        if heap_cmp_lt_checked(heap, &item, &parent_item, expected_len)? {
            heap_swap(heap, pos, parent, expected_len)?;
            pos = parent;
        } else {
            break;
        }
    }
    Ok(())
}

fn heap_sift_down(heap: &PyCell<Vec<PyObjectRef>>, mut pos: usize, end: usize) -> PyResult<()> {
    loop {
        let mut child = 2 * pos + 1;
        if child >= end {
            break;
        }
        let right = child + 1;
        if right < end {
            let (right_item, child_item, expected_len) = heap_pair(heap, right, child)?;
            if expected_len < end {
                return Err(PyException::index_error(
                    "list changed size during iteration",
                ));
            }
            if heap_cmp_lt_checked(heap, &right_item, &child_item, expected_len)? {
                child = right;
            }
        }
        let (child_item, item, expected_len) = heap_pair(heap, child, pos)?;
        if expected_len < end {
            return Err(PyException::index_error(
                "list changed size during iteration",
            ));
        }
        if heap_cmp_lt_checked(heap, &child_item, &item, expected_len)? {
            heap_swap(heap, pos, child, expected_len)?;
            pos = child;
        } else {
            break;
        }
    }
    Ok(())
}

fn heap_sift_up_max(heap: &PyCell<Vec<PyObjectRef>>, mut pos: usize) -> PyResult<()> {
    while pos > 0 {
        let parent = (pos - 1) / 2;
        let (parent_item, item, expected_len) = heap_pair(heap, parent, pos)?;
        if heap_cmp_lt_checked(heap, &parent_item, &item, expected_len)? {
            heap_swap(heap, pos, parent, expected_len)?;
            pos = parent;
        } else {
            break;
        }
    }
    Ok(())
}

fn heap_sift_down_max(heap: &PyCell<Vec<PyObjectRef>>, mut pos: usize, end: usize) -> PyResult<()> {
    loop {
        let mut child = 2 * pos + 1;
        if child >= end {
            break;
        }
        let right = child + 1;
        if right < end {
            let (child_item, right_item, expected_len) = heap_pair(heap, child, right)?;
            if expected_len < end {
                return Err(PyException::index_error(
                    "list changed size during iteration",
                ));
            }
            if heap_cmp_lt_checked(heap, &child_item, &right_item, expected_len)? {
                child = right;
            }
        }
        let (item, child_item, expected_len) = heap_pair(heap, pos, child)?;
        if expected_len < end {
            return Err(PyException::index_error(
                "list changed size during iteration",
            ));
        }
        if heap_cmp_lt_checked(heap, &item, &child_item, expected_len)? {
            heap_swap(heap, pos, child, expected_len)?;
            pos = child;
        } else {
            break;
        }
    }
    Ok(())
}

fn heap_item_precedes(a: &PyObjectRef, b: &PyObjectRef, reverse: bool) -> PyResult<bool> {
    if reverse {
        if heap_cmp_lt(b, a)? {
            return Ok(true);
        }
        if heap_cmp_lt(a, b)? {
            return Ok(false);
        }
    } else {
        if heap_cmp_lt(a, b)? {
            return Ok(true);
        }
        if heap_cmp_lt(b, a)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn heap_sort_items(items: &[PyObjectRef], reverse: bool) -> PyResult<Vec<PyObjectRef>> {
    if items.len() <= 1 {
        return Ok(items.to_vec());
    }
    let mid = items.len() / 2;
    let left = heap_sort_items(&items[..mid], reverse)?;
    let right = heap_sort_items(&items[mid..], reverse)?;
    let mut merged = Vec::with_capacity(items.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if heap_item_precedes(&left[i], &right[j], reverse)? {
            merged.push(left[i].clone());
            i += 1;
        } else {
            merged.push(right[j].clone());
            j += 1;
        }
    }
    merged.extend(left[i..].iter().cloned());
    merged.extend(right[j..].iter().cloned());
    Ok(merged)
}

fn heap_key_pair_precedes(
    a: &(PyObjectRef, PyObjectRef),
    b: &(PyObjectRef, PyObjectRef),
    reverse: bool,
) -> PyResult<bool> {
    heap_item_precedes(&a.0, &b.0, reverse)
}

fn heap_sort_key_pairs(
    pairs: &[(PyObjectRef, PyObjectRef)],
    reverse: bool,
) -> PyResult<Vec<(PyObjectRef, PyObjectRef)>> {
    if pairs.len() <= 1 {
        return Ok(pairs.to_vec());
    }
    let mid = pairs.len() / 2;
    let left = heap_sort_key_pairs(&pairs[..mid], reverse)?;
    let right = heap_sort_key_pairs(&pairs[mid..], reverse)?;
    let mut merged = Vec::with_capacity(pairs.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if heap_key_pair_precedes(&left[i], &right[j], reverse)? {
            merged.push(left[i].clone());
            i += 1;
        } else {
            merged.push(right[j].clone());
            j += 1;
        }
    }
    merged.extend(left[i..].iter().cloned());
    merged.extend(right[j..].iter().cloned());
    Ok(merged)
}

fn heap_sort_with_key(
    items: Vec<PyObjectRef>,
    key: Option<PyObjectRef>,
    reverse: bool,
) -> PyResult<Vec<PyObjectRef>> {
    let Some(key_fn) = key else {
        return heap_sort_items(&items, reverse);
    };
    if matches!(&key_fn.payload, PyObjectPayload::None) {
        return heap_sort_items(&items, reverse);
    }
    let mut pairs = Vec::with_capacity(items.len());
    for item in items {
        let key_obj = call_callable(&key_fn, std::slice::from_ref(&item))?;
        pairs.push((key_obj, item));
    }
    Ok(heap_sort_key_pairs(&pairs, reverse)?
        .into_iter()
        .map(|(_, item)| item)
        .collect())
}

fn heap_kwarg(kwargs: Option<&PyObjectRef>, name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &kwargs?.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(name)))
        .cloned()
}

fn heap_split_kwargs(args: &[PyObjectRef]) -> (&[PyObjectRef], Option<PyObjectRef>) {
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let read = map.read();
            if read.contains_key(&HashableKey::str_key(CompactString::from("key")))
                || read.contains_key(&HashableKey::str_key(CompactString::from("reverse")))
            {
                return (&args[..args.len() - 1], Some(last.clone()));
            }
        }
    }
    (args, None)
}

fn heap_collect_via_list(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    let list_type = PyObject::builtin_type(CompactString::from("list"));
    call_callable(&list_type, std::slice::from_ref(obj))?.to_list()
}

fn heap_collect_iterable(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    if let PyObjectPayload::Instance(_) = &obj.payload {
        if let Some(iter_method) = obj.get_attr("__iter__") {
            let iter = call_callable(&iter_method, &[])?;
            if iter.get_attr("__next__").is_none() {
                return Err(PyException::type_error(format!(
                    "iter() returned non-iterator of type '{}'",
                    iter.type_name()
                )));
            }
            return heap_collect_via_list(&iter);
        }
        if obj.get_attr("__next__").is_some() {
            return Err(PyException::type_error(format!(
                "'{}' object is not iterable",
                obj.type_name()
            )));
        }
    }
    if matches!(
        &obj.payload,
        PyObjectPayload::Instance(_)
            | PyObjectPayload::Generator(_)
            | PyObjectPayload::Iterator(_)
            | PyObjectPayload::RangeIter(..)
            | PyObjectPayload::VecIter(_)
            | PyObjectPayload::RefIter { .. }
    ) {
        return heap_collect_via_list(obj);
    }
    match obj.to_list() {
        Ok(items) => Ok(items),
        Err(_) => heap_collect_via_list(obj),
    }
}

fn heapq_push(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heappush", args, 2)?;
    let list_obj = &args[0];
    if let PyObjectPayload::List(lock) = &list_obj.payload {
        let pos = {
            let mut items = lock.write();
            items.push(args[1].clone());
            items.len() - 1
        };
        heap_sift_up(lock, pos)?;
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error(
            "heappush: first arg must be a list",
        ))
    }
}

fn heapq_pop(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heappop", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let (result, n) = {
            let mut items = lock.write();
            if items.is_empty() {
                return Err(PyException::index_error("index out of range"));
            }
            let len = items.len();
            if len == 1 {
                return Ok(items.pop().unwrap());
            }
            let result = items[0].clone();
            let last = items.pop().unwrap();
            items[0] = last;
            (result, items.len())
        };
        heap_sift_down(lock, 0, n)?;
        Ok(result)
    } else {
        Err(PyException::type_error("heappop: arg must be a list"))
    }
}

fn heapq_heapify(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heapify", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let n = lock.read().len();
        for i in (0..n / 2).rev() {
            heap_sift_down(lock, i, n)?;
        }
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error("heapify: arg must be a list"))
    }
}

fn heapq_pushpop(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heappushpop", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let root_and_len = {
            let items = lock.read();
            if items.is_empty() {
                return Ok(args[1].clone());
            }
            (items[0].clone(), items.len())
        };
        let (root, expected_len) = root_and_len;
        if !heap_cmp_lt_checked(lock, &root, &args[1], expected_len)? {
            return Ok(args[1].clone());
        }
        {
            let mut items = lock.write();
            if items.len() != expected_len || items.is_empty() {
                return Err(PyException::index_error(
                    "list changed size during iteration",
                ));
            }
            items[0] = args[1].clone();
        }
        heap_sift_down(lock, 0, expected_len)?;
        Ok(root)
    } else {
        Err(PyException::type_error(
            "heappushpop: first arg must be a list",
        ))
    }
}

fn heapq_replace(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heapreplace", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let (result, n) = {
            let mut items = lock.write();
            if items.is_empty() {
                return Err(PyException::index_error("index out of range"));
            }
            let result = std::mem::replace(&mut items[0], args[1].clone());
            (result, items.len())
        };
        heap_sift_down(lock, 0, n)?;
        Ok(result)
    } else {
        Err(PyException::type_error(
            "heapreplace: first arg must be a list",
        ))
    }
}

fn heapq_pop_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("_heappop_max", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let (result, n) = {
            let mut items = lock.write();
            if items.is_empty() {
                return Err(PyException::index_error("index out of range"));
            }
            let len = items.len();
            if len == 1 {
                return Ok(items.pop().unwrap());
            }
            let result = items[0].clone();
            let last = items.pop().unwrap();
            items[0] = last;
            (result, items.len())
        };
        heap_sift_down_max(lock, 0, n)?;
        Ok(result)
    } else {
        Err(PyException::type_error("_heappop_max: arg must be a list"))
    }
}

fn heapq_replace_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("_heapreplace_max", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let (result, n) = {
            let mut items = lock.write();
            if items.is_empty() {
                return Err(PyException::index_error("index out of range"));
            }
            let result = std::mem::replace(&mut items[0], args[1].clone());
            (result, items.len())
        };
        heap_sift_down_max(lock, 0, n)?;
        Ok(result)
    } else {
        Err(PyException::type_error(
            "_heapreplace_max: first arg must be a list",
        ))
    }
}

fn heapq_heapify_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("_heapify_max", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let n = lock.read().len();
        for i in (0..n / 2).rev() {
            heap_sift_down_max(lock, i, n)?;
        }
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error("_heapify_max: arg must be a list"))
    }
}

fn heapq_nlargest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = heap_split_kwargs(args);
    check_args("nlargest", pos, 2)?;
    let n = pos[0].to_int()? as usize;
    let items = heap_collect_iterable(&pos[1])?;
    let key = heap_kwarg(kwargs.as_ref(), "key");
    let mut sorted = heap_sort_with_key(items, key, true)?;
    sorted.truncate(n);
    Ok(PyObject::list(sorted))
}

fn heapq_nsmallest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = heap_split_kwargs(args);
    check_args("nsmallest", pos, 2)?;
    let n = pos[0].to_int()? as usize;
    let items = heap_collect_iterable(&pos[1])?;
    let key = heap_kwarg(kwargs.as_ref(), "key");
    let mut sorted = heap_sort_with_key(items, key, false)?;
    sorted.truncate(n);
    Ok(PyObject::list(sorted))
}

fn heapq_merge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = heap_split_kwargs(args);
    let key = heap_kwarg(kwargs.as_ref(), "key");
    let reverse = heap_kwarg(kwargs.as_ref(), "reverse")
        .map(|v| v.is_truthy())
        .unwrap_or(false);
    let mut all = Vec::new();
    for arg in pos {
        all.extend(heap_collect_iterable(arg)?);
    }
    all = heap_sort_with_key(all, key, reverse)?;
    Ok(PyObject::list(all))
}

// ── bisect module ──

pub fn create_bisect_module() -> PyObjectRef {
    make_module(
        "bisect",
        vec![
            ("bisect_left", make_builtin(bisect_left)),
            ("bisect_right", make_builtin(bisect_right)),
            ("bisect", make_builtin(bisect_right)), // bisect is alias for bisect_right
            ("insort_left", make_builtin(insort_left)),
            ("insort_right", make_builtin(insort_right)),
            ("insort", make_builtin(insort_right)), // insort is alias for insort_right
        ],
    )
}

struct BisectArgs {
    seq: PyObjectRef,
    x: PyObjectRef,
    lo: i64,
    hi: i64,
}

fn bisect_kwargs(args: &[PyObjectRef]) -> (usize, Vec<(String, PyObjectRef)>) {
    let Some(last) = args.last() else {
        return (0, Vec::new());
    };
    let PyObjectPayload::Dict(map) = &last.payload else {
        return (args.len(), Vec::new());
    };
    let read = map.read();
    let mut kwargs = Vec::new();
    for (key, value) in read.iter() {
        let HashableKey::Str(name) = key else {
            return (args.len(), Vec::new());
        };
        if !matches!(name.as_str(), "a" | "x" | "lo" | "hi") {
            return (args.len(), Vec::new());
        }
        kwargs.push((name.as_str().to_string(), value.clone()));
    }
    if kwargs.is_empty() {
        (args.len(), Vec::new())
    } else {
        (args.len() - 1, kwargs)
    }
}

fn parse_bisect_args(name: &str, args: &[PyObjectRef]) -> PyResult<BisectArgs> {
    let (pos_len, kwargs) = bisect_kwargs(args);
    if pos_len > 4 {
        return Err(PyException::type_error(format!(
            "{name}() takes at most 4 arguments ({} given)",
            pos_len
        )));
    }

    let mut a = args.first().filter(|_| pos_len > 0).cloned();
    let mut x = args.get(1).filter(|_| pos_len > 1).cloned();
    let mut lo = args.get(2).filter(|_| pos_len > 2).cloned();
    let mut hi = args.get(3).filter(|_| pos_len > 3).cloned();

    for (key, value) in kwargs {
        let slot = match key.as_str() {
            "a" => &mut a,
            "x" => &mut x,
            "lo" => &mut lo,
            "hi" => &mut hi,
            _ => unreachable!(),
        };
        if slot.is_some() {
            return Err(PyException::type_error(format!(
                "{name}() got multiple values for argument '{key}'"
            )));
        }
        *slot = Some(value);
    }

    let seq = a.ok_or_else(|| {
        PyException::type_error(format!("{name}() missing required argument 'a'"))
    })?;
    let x = x.ok_or_else(|| {
        PyException::type_error(format!("{name}() missing required argument 'x'"))
    })?;
    let lo = match lo {
        Some(value) => value.to_int()?,
        None => 0,
    };
    if lo < 0 {
        return Err(PyException::value_error("lo must be non-negative"));
    }

    let len =
        i64::try_from(seq.py_len()?).map_err(|_| PyException::overflow_error("len too large"))?;
    let hi = match hi {
        Some(value) if matches!(value.payload, PyObjectPayload::None) => len,
        Some(value) => value.to_int()?,
        None => len,
    };

    Ok(BisectArgs { seq, x, lo, hi })
}

fn call_order_dunder(
    obj: &PyObjectRef,
    method_name: &str,
    other: &PyObjectRef,
) -> PyResult<Option<bool>> {
    if !matches!(&obj.payload, PyObjectPayload::Instance(_)) {
        return Ok(None);
    }
    let Some(method) = obj.get_attr(method_name) else {
        return Ok(None);
    };
    if matches!(&method.payload, PyObjectPayload::None) {
        return Ok(None);
    }
    let result = call_callable(&method, std::slice::from_ref(other))?;
    if matches!(&result.payload, PyObjectPayload::NotImplemented) {
        Ok(None)
    } else {
        Ok(Some(result.is_truthy()))
    }
}

fn bisect_lt(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
    if let Some(result) = call_order_dunder(a, "__lt__", b)? {
        return Ok(result);
    }
    if let Some(result) = call_order_dunder(b, "__gt__", a)? {
        return Ok(result);
    }
    Ok(a.compare(b, CompareOp::Lt)?.is_truthy())
}

fn bisect_left_index(
    seq: &PyObjectRef,
    x: &PyObjectRef,
    mut lo: i64,
    mut hi: i64,
) -> PyResult<i64> {
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let item = seq.get_item(&PyObject::int(mid))?;
        if bisect_lt(&item, x)? {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    Ok(lo)
}

fn bisect_right_index(
    seq: &PyObjectRef,
    x: &PyObjectRef,
    mut lo: i64,
    mut hi: i64,
) -> PyResult<i64> {
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let item = seq.get_item(&PyObject::int(mid))?;
        if bisect_lt(x, &item)? {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    Ok(lo)
}

fn bisect_left(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let parsed = parse_bisect_args("bisect_left", args)?;
    Ok(PyObject::int(bisect_left_index(
        &parsed.seq,
        &parsed.x,
        parsed.lo,
        parsed.hi,
    )?))
}

fn bisect_right(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let parsed = parse_bisect_args("bisect_right", args)?;
    Ok(PyObject::int(bisect_right_index(
        &parsed.seq,
        &parsed.x,
        parsed.lo,
        parsed.hi,
    )?))
}

fn bisect_insert(seq: &PyObjectRef, idx: i64, x: &PyObjectRef) -> PyResult<PyObjectRef> {
    if let PyObjectPayload::List(lock) = &seq.payload {
        lock.write().insert(idx as usize, x.clone());
        return Ok(PyObject::none());
    }
    if let Some(insert) = seq.get_attr("insert") {
        call_callable(&insert, &[PyObject::int(idx), x.clone()])?;
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error(format!(
            "'{}' object has no attribute 'insert'",
            seq.type_name()
        )))
    }
}

fn insort_left(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let parsed = parse_bisect_args("insort_left", args)?;
    let idx = bisect_left_index(&parsed.seq, &parsed.x, parsed.lo, parsed.hi)?;
    bisect_insert(&parsed.seq, idx, &parsed.x)
}

fn insort_right(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let parsed = parse_bisect_args("insort_right", args)?;
    let idx = bisect_right_index(&parsed.seq, &parsed.x, parsed.lo, parsed.hi)?;
    bisect_insert(&parsed.seq, idx, &parsed.x)
}

// ── fractions module ─────────────────────────────────────────────────
pub fn create_fractions_module() -> PyObjectRef {
    use ferrython_core::object::{new_shared_fx, InstanceData};

    fn object_to_bigint(obj: &PyObjectRef) -> Option<BigInt> {
        match &obj.payload {
            PyObjectPayload::Int(PyInt::Small(n)) => Some(BigInt::from(*n)),
            PyObjectPayload::Int(PyInt::Big(n)) => Some(n.as_ref().clone()),
            PyObjectPayload::Bool(b) => Some(BigInt::from(if *b { 1 } else { 0 })),
            _ => None,
        }
    }

    fn get_frac_bigint_parts(obj: &PyObjectRef) -> Option<(BigInt, BigInt)> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__fraction__") {
                let n = attrs.get("numerator").and_then(object_to_bigint)?;
                let d = attrs.get("denominator").and_then(object_to_bigint)?;
                return Some((n, d));
            }
        }
        object_to_bigint(obj).map(|n| (n, BigInt::one()))
    }

    fn get_frac_parts(obj: &PyObjectRef) -> Option<(i64, i64)> {
        let (n, d) = get_frac_bigint_parts(obj)?;
        Some((n.to_i64()?, d.to_i64()?))
    }

    fn float_to_bigint_fraction(f: f64) -> Option<(BigInt, BigInt)> {
        if !f.is_finite() {
            return None;
        }
        if f == 0.0 {
            return Some((BigInt::zero(), BigInt::one()));
        }
        let bits = f.to_bits();
        let negative = (bits >> 63) != 0;
        let raw_exp = ((bits >> 52) & 0x7ff) as i32;
        let frac = bits & 0x000f_ffff_ffff_ffff;
        let (mantissa, exp) = if raw_exp == 0 {
            (frac, -1074)
        } else {
            ((1u64 << 52) | frac, raw_exp - 1075)
        };
        let mut numer = BigInt::from(mantissa);
        let mut denom = BigInt::one();
        if exp >= 0 {
            numer <<= exp as usize;
        } else {
            denom <<= (-exp) as usize;
        }
        if negative {
            numer = -numer;
        }
        let g = numer.abs().gcd(&denom);
        Some((numer / &g, denom / g))
    }

    fn get_frac_cmp_parts(obj: &PyObjectRef) -> Option<(BigInt, BigInt)> {
        if let Some(parts) = get_frac_bigint_parts(obj) {
            return Some(parts);
        }
        if let PyObjectPayload::Float(f) = &obj.payload {
            return float_to_bigint_fraction(*f);
        }
        None
    }

    fn decimal_str_to_fraction(s: &str) -> PyResult<PyObjectRef> {
        let s = s.trim();
        let (sign, s) = if let Some(rest) = s.strip_prefix('-') {
            (-1i64, rest)
        } else {
            (1i64, s)
        };
        if let Some((int_part, frac_part)) = s.split_once('.') {
            let int_part = if int_part.is_empty() { "0" } else { int_part };
            let frac_digits = frac_part.len() as u32;
            let denom = 10i64.checked_pow(frac_digits).unwrap_or(1);
            let int_val: i64 = int_part.parse().unwrap_or(0);
            let frac_val: i64 = frac_part.parse().unwrap_or(0);
            let numer = sign * (int_val * denom + frac_val);
            Ok(make_frac_instance(numer, denom))
        } else {
            let n: i64 = s.parse().unwrap_or(0);
            Ok(make_frac_instance(sign * n, 1))
        }
    }

    fn make_frac_bigint_instance(num: BigInt, den: BigInt) -> PyObjectRef {
        let g = num.abs().gcd(&den.abs());
        let mut num = num / &g;
        let mut den = den / &g;
        if den.sign() == Sign::Minus {
            num = -num;
            den = -den;
        }
        make_frac_normalized_instance(num, den)
    }

    fn make_frac_instance(num: i64, den: i64) -> PyObjectRef {
        make_frac_bigint_instance(BigInt::from(num), BigInt::from(den))
    }

    fn make_frac_normalized_instance(num: BigInt, den: BigInt) -> PyObjectRef {
        let num_obj = bigint_to_object(num);
        let den_obj = bigint_to_object(den);
        let mut frac_ns = IndexMap::new();
        frac_ns.insert(CompactString::from("__add__"), make_builtin(frac_add));
        frac_ns.insert(CompactString::from("__radd__"), make_builtin(frac_add));
        frac_ns.insert(CompactString::from("__sub__"), make_builtin(frac_sub));
        frac_ns.insert(CompactString::from("__rsub__"), make_builtin(frac_rsub));
        frac_ns.insert(CompactString::from("__mul__"), make_builtin(frac_mul));
        frac_ns.insert(CompactString::from("__rmul__"), make_builtin(frac_mul));
        frac_ns.insert(CompactString::from("__truediv__"), make_builtin(frac_div));
        frac_ns.insert(
            CompactString::from("__floordiv__"),
            make_builtin(frac_floordiv),
        );
        frac_ns.insert(CompactString::from("__neg__"), make_builtin(frac_neg));
        frac_ns.insert(CompactString::from("__abs__"), make_builtin(frac_abs));
        frac_ns.insert(CompactString::from("__eq__"), make_builtin(frac_eq));
        frac_ns.insert(CompactString::from("__lt__"), make_builtin(frac_lt));
        frac_ns.insert(CompactString::from("__le__"), make_builtin(frac_le));
        frac_ns.insert(CompactString::from("__gt__"), make_builtin(frac_gt));
        frac_ns.insert(CompactString::from("__ge__"), make_builtin(frac_ge));
        frac_ns.insert(CompactString::from("__hash__"), make_builtin(frac_hash));
        frac_ns.insert(CompactString::from("__str__"), make_builtin(frac_str));
        frac_ns.insert(CompactString::from("__repr__"), make_builtin(frac_repr));
        frac_ns.insert(CompactString::from("__float__"), make_builtin(frac_float));
        frac_ns.insert(CompactString::from("__int__"), make_builtin(frac_int));
        frac_ns.insert(CompactString::from("__bool__"), make_builtin(frac_bool));
        frac_ns.insert(
            CompactString::from("limit_denominator"),
            make_builtin(frac_limit_denominator),
        );
        frac_ns.insert(CompactString::from("__pow__"), make_builtin(frac_pow));
        frac_ns.insert(CompactString::from("__mod__"), make_builtin(frac_mod));
        frac_ns.insert(
            CompactString::from("__rtruediv__"),
            make_builtin(frac_rtruediv),
        );
        frac_ns.insert(
            CompactString::from("__rfloordiv__"),
            make_builtin(frac_rfloordiv),
        );
        frac_ns.insert(
            CompactString::from("__format__"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("0")));
                }
                let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
                let spec = args.get(1).map(|a| a.py_to_string()).unwrap_or_default();
                if spec.is_empty() || spec == "s" {
                    if d == 1 {
                        return Ok(PyObject::str_val(CompactString::from(format!("{}", n))));
                    }
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "{}/{}",
                        n, d
                    ))));
                }
                // For numeric format specs, convert to float
                let f = n as f64 / d as f64;
                Ok(PyObject::str_val(CompactString::from(format!("{}", f))))
            }),
        );
        frac_ns.insert(
            CompactString::from("as_integer_ratio"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::tuple(vec![PyObject::int(0), PyObject::int(1)]));
                }
                let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
                Ok(PyObject::tuple(vec![PyObject::int(n), PyObject::int(d)]))
            }),
        );
        let class = PyObject::class(CompactString::from("Fraction"), vec![], frac_ns);
        let class_flags = InstanceData::compute_flags(&class);
        let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
            Box::new(InstanceData {
                class,
                attrs: new_shared_fx(),
                is_special: true,
                dict_storage: None,
                class_flags,
            }),
        )));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut w = inst_data.attrs.write();
            w.insert(
                CompactString::from("__fraction__"),
                PyObject::bool_val(true),
            );
            w.insert(CompactString::from("numerator"), num_obj);
            w.insert(CompactString::from("denominator"), den_obj);
        }
        inst
    }

    fn frac_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__add__ requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(&an * &bd + &bn * &ad, ad * bd))
    }

    fn frac_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__sub__ requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(&an * &bd - &bn * &ad, ad * bd))
    }

    fn frac_rsub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(&bn * &ad - &an * &bd, ad * bd))
    }

    fn frac_mul(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__mul__ requires 2 args"));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(an * bn, ad * bd))
    }

    fn frac_div(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "Fraction.__truediv__ requires 2 args",
            ));
        }
        let (an, ad) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_bigint_parts(&args[1])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if bn.is_zero() {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        Ok(make_frac_bigint_instance(an * bd, ad * bn))
    }

    fn frac_floordiv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) =
            get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) =
            get_frac_parts(&args[1]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if bn == 0 {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        let result = (an * bd).div_euclid(ad * bn);
        Ok(PyObject::int(result))
    }

    fn frac_neg(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(-n, d))
    }

    fn frac_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0])
            .ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_bigint_instance(n.abs(), d))
    }

    fn frac_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let a = get_frac_cmp_parts(&args[0]);
        let b = get_frac_cmp_parts(&args[1]);
        match (a, b) {
            (Some((an, ad)), Some((bn, bd))) => Ok(PyObject::bool_val(an * bd == bn * ad)),
            _ => Ok(PyObject::bool_val(false)),
        }
    }

    fn frac_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let (an, ad) = get_frac_cmp_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let (bn, bd) = get_frac_cmp_parts(&args[1]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::bool_val(an * bd < bn * ad))
    }

    fn frac_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let (an, ad) = get_frac_cmp_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let (bn, bd) = get_frac_cmp_parts(&args[1]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::bool_val(an * bd <= bn * ad))
    }

    fn frac_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let (an, ad) = get_frac_cmp_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let (bn, bd) = get_frac_cmp_parts(&args[1]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::bool_val(an * bd > bn * ad))
    }

    fn frac_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let (an, ad) = get_frac_cmp_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let (bn, bd) = get_frac_cmp_parts(&args[1]).unwrap_or((BigInt::zero(), BigInt::one()));
        Ok(PyObject::bool_val(an * bd >= bn * ad))
    }

    fn frac_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::int(n.wrapping_mul(31).wrapping_add(d)))
    }

    fn frac_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let s = if d == 1 {
            format!("{}", n)
        } else {
            format!("{}/{}", n, d)
        };
        Ok(PyObject::str_val(CompactString::from(s)))
    }

    fn frac_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::str_val(CompactString::from(format!(
            "Fraction({}, {})",
            n, d
        ))))
    }

    fn frac_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_bigint_parts(&args[0]).unwrap_or((BigInt::zero(), BigInt::one()));
        let n = n
            .to_f64()
            .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
        let d = d
            .to_f64()
            .ok_or_else(|| PyException::overflow_error("int too large to convert to float"))?;
        Ok(PyObject::float(n / d))
    }

    fn frac_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::int(n / d))
    }

    fn frac_bool(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, _) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::bool_val(n != 0))
    }

    fn frac_limit_denominator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let max_den = if args.len() > 1 {
            args[1].to_int().unwrap_or(1_000_000)
        } else {
            1_000_000
        };
        if d <= max_den {
            return Ok(make_frac_instance(n, d));
        }
        // CPython algorithm: continued fraction convergents (handles negative n)
        let mut p0: i64 = 0;
        let mut q0: i64 = 1;
        let mut p1: i64 = 1;
        let mut q1: i64 = 0;
        let mut nn = n;
        let mut dd = d;
        loop {
            let a = nn.div_euclid(dd);
            let q2 = q0 + a * q1;
            if q2 > max_den {
                break;
            }
            let new_p1 = p0 + a * p1;
            let new_q1 = q2;
            p0 = p1;
            q0 = q1;
            p1 = new_p1;
            q1 = new_q1;
            let tmp = nn - a * dd;
            nn = dd;
            dd = tmp;
            if dd == 0 {
                break;
            }
        }
        let k = if q1 != 0 { (max_den - q0) / q1 } else { 0 };
        let (bound1_n, bound1_d) = (p0 + k * p1, q0 + k * q1);
        // bound2 = p1/q1 (convergent), bound1 = semi-convergent
        // Return convergent if at least as close, matching CPython tie-breaking
        let err2 = (n as i128 * q1 as i128 - d as i128 * p1 as i128).unsigned_abs();
        let err1 = (n as i128 * bound1_d as i128 - d as i128 * bound1_n as i128).unsigned_abs();
        let (rn, rd) =
            if err2 * (bound1_d as i128).unsigned_abs() <= err1 * (q1 as i128).unsigned_abs() {
                (p1, q1)
            } else {
                (bound1_n, bound1_d)
            };
        Ok(make_frac_instance(rn, rd))
    }

    fn frac_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__pow__ requires 2 args"));
        }
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let exp = args[1].to_int().unwrap_or(1);
        if exp >= 0 {
            let e = exp as u32;
            Ok(make_frac_instance(n.pow(e), d.pow(e)))
        } else {
            let e = (-exp) as u32;
            if n == 0 {
                return Err(PyException::zero_division_error(
                    "Fraction division by zero",
                ));
            }
            Ok(make_frac_instance(d.pow(e), n.pow(e)))
        }
    }

    fn frac_mod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("Fraction.__mod__ requires 2 args"));
        }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((1, 1));
        if bn == 0 {
            return Err(PyException::zero_division_error("Fraction modulo by zero"));
        }
        // a % b = a - b * floor(a/b)
        let num = an * bd;
        let den = ad * bn;
        let floor_div = if den > 0 {
            num.div_euclid(den)
        } else {
            -((-num).div_euclid(-den))
        };
        let result_n = an * bd * bd - floor_div * bn * ad * bd;
        let result_d = ad * bd * bd;
        Ok(make_frac_instance(result_n, result_d))
    }

    fn frac_rtruediv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((1, 1));
        if an == 0 {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        Ok(make_frac_instance(bn * ad, bd * an))
    }

    fn frac_rfloordiv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("requires 2 args"));
        }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((1, 1));
        if an == 0 {
            return Err(PyException::zero_division_error(
                "Fraction division by zero",
            ));
        }
        let num = bn * ad;
        let den = bd * an;
        let result = if (num < 0) ^ (den < 0) {
            -((-num).abs() / den.abs()) - if num.abs() % den.abs() != 0 { 1 } else { 0 }
        } else {
            num / den
        };
        Ok(make_frac_instance(result, 1))
    }

    // Fraction as a module-like callable with class methods
    let fraction_from_float = make_builtin(|args| {
        if args.is_empty() {
            return Err(PyException::type_error("from_float requires 1 argument"));
        }
        let f = args[0].to_float()?;
        let (n, d) = float_to_fraction(f);
        Ok(make_frac_instance(n, d))
    });
    let fraction_from_decimal = make_builtin(|args| {
        if args.is_empty() {
            return Err(PyException::type_error("from_decimal requires 1 argument"));
        }
        let f = args[0].to_float()?;
        let (n, d) = float_to_fraction(f);
        Ok(make_frac_instance(n, d))
    });

    let frac_class_ns = IndexMap::from([
        (CompactString::from("from_float"), fraction_from_float),
        (CompactString::from("from_decimal"), fraction_from_decimal),
    ]);
    let frac_class = PyObject::class(CompactString::from("Fraction"), vec![], frac_class_ns);

    // Store new function on the class for instantiation
    if let PyObjectPayload::Class(ref cd) = frac_class.payload {
        cd.namespace.write().insert(
            CompactString::from("__new__"),
            make_builtin(|args| {
                if args.is_empty() {
                    return Ok(make_frac_instance(0, 1));
                }
                // Skip cls argument if present (class object)
                let real_args =
                    if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                        &args[1..]
                    } else {
                        args
                    };
                if real_args.is_empty() {
                    return Ok(make_frac_instance(0, 1));
                }
                if real_args.len() == 1 {
                    match &real_args[0].payload {
                        PyObjectPayload::Int(n) => {
                            let n = match n {
                                PyInt::Small(v) => BigInt::from(*v),
                                PyInt::Big(v) => v.as_ref().clone(),
                            };
                            return Ok(make_frac_bigint_instance(n, BigInt::one()));
                        }
                        PyObjectPayload::Float(f) => {
                            let (n, d) = float_to_fraction(*f);
                            return Ok(make_frac_instance(n, d));
                        }
                        PyObjectPayload::Str(s) => {
                            if let Some((n_str, d_str)) = s.split_once('/') {
                                let n: i64 = n_str.trim().parse().map_err(|_| {
                                    PyException::value_error("Invalid fraction string")
                                })?;
                                let d: i64 = d_str.trim().parse().map_err(|_| {
                                    PyException::value_error("Invalid fraction string")
                                })?;
                                if d == 0 {
                                    return Err(PyException::new(
                                        ferrython_core::error::ExceptionKind::ZeroDivisionError,
                                        "Fraction(_, 0)",
                                    ));
                                }
                                return Ok(make_frac_instance(n, d));
                            } else if s.contains('.') || s.contains('e') || s.contains('E') {
                                return decimal_str_to_fraction(s.trim());
                            } else {
                                let n: i64 = s.trim().parse().map_err(|_| {
                                    PyException::value_error("Invalid fraction string")
                                })?;
                                return Ok(make_frac_instance(n, 1));
                            }
                        }
                        _ => {
                            if let Some((n, d)) = get_frac_parts(&real_args[0]) {
                                return Ok(make_frac_instance(n, d));
                            }
                            // Handle Decimal instances by converting via string
                            if let PyObjectPayload::Instance(inst) = &real_args[0].payload {
                                let attrs = inst.attrs.read();
                                if attrs.contains_key("__decimal__") {
                                    let s = attrs
                                        .get("_value")
                                        .map(|v| v.py_to_string())
                                        .unwrap_or_else(|| "0".to_string());
                                    drop(attrs);
                                    return decimal_str_to_fraction(&s);
                                }
                            }
                            return Err(PyException::type_error(
                                "Fraction() argument must be int, float, str, or Decimal",
                            ));
                        }
                    }
                }
                let n = index_bigint(&real_args[0], "Fraction")?;
                let d = index_bigint(&real_args[1], "Fraction")?;
                if d.is_zero() {
                    return Err(PyException::new(
                        ferrython_core::error::ExceptionKind::ZeroDivisionError,
                        "Fraction(_, 0)",
                    ));
                }
                Ok(make_frac_bigint_instance(n, d))
            }),
        );
        cd.invalidate_cache();
    }

    make_module(
        "fractions",
        vec![
            ("Fraction", frac_class),
            ("gcd", make_builtin(fraction_gcd)),
        ],
    )
}

fn float_to_fraction(f: f64) -> (i64, i64) {
    if f == 0.0 {
        return (0, 1);
    }
    if f.is_infinite() || f.is_nan() {
        return (0, 1);
    }
    // Exact IEEE 754 decomposition matching CPython's float.as_integer_ratio()
    let bits = f.to_bits();
    let sign: i64 = if (bits >> 63) != 0 { -1 } else { 1 };
    let raw_exp = ((bits >> 52) & 0x7FF) as i64;
    let mantissa = (bits & 0x000F_FFFF_FFFF_FFFF) as i64;

    let (mut numer, exp) = if raw_exp == 0 {
        (mantissa, -1074i64) // subnormal
    } else {
        ((1i64 << 52) | mantissa, raw_exp - 1075)
    };

    // Remove trailing zero bits to simplify
    if numer != 0 {
        let tz = numer.trailing_zeros();
        numer >>= tz;
        // exp += tz as i64; (already accounted for since we shift numer)
        let adjusted_exp = exp + tz as i64;
        if adjusted_exp >= 0 {
            let shift = adjusted_exp.min(62) as u32;
            return (sign * numer.checked_shl(shift).unwrap_or(numer), 1);
        } else {
            let shift = (-adjusted_exp).min(62) as u32;
            return (sign * numer, 1i64.checked_shl(shift).unwrap_or(i64::MAX));
        }
    }
    (0, 1)
}

fn fraction_gcd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("gcd", args, 2)?;
    let mut a = args[0].to_int()?.abs();
    let mut b = args[1].to_int()?.abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    Ok(PyObject::int(a))
}

// ── cmath module ─────────────────────────────────────────────────────

fn to_complex(obj: &PyObjectRef) -> (f64, f64) {
    match &obj.payload {
        PyObjectPayload::Complex { real, imag } => (*real, *imag),
        PyObjectPayload::Float(f) => (*f, 0.0),
        PyObjectPayload::Int(n) => (n.to_i64().unwrap_or(0) as f64, 0.0),
        _ => (0.0, 0.0),
    }
}

pub fn create_cmath_module() -> PyObjectRef {
    make_module(
        "cmath",
        vec![
            ("pi", PyObject::float(std::f64::consts::PI)),
            ("e", PyObject::float(std::f64::consts::E)),
            ("inf", PyObject::float(f64::INFINITY)),
            ("nan", PyObject::float(f64::NAN)),
            ("infj", PyObject::complex(0.0, f64::INFINITY)),
            ("nanj", PyObject::complex(0.0, f64::NAN)),
            ("sqrt", make_builtin(cmath_sqrt)),
            ("exp", make_builtin(cmath_exp)),
            ("log", make_builtin(cmath_log)),
            ("sin", make_builtin(cmath_sin)),
            ("cos", make_builtin(cmath_cos)),
            ("tan", make_builtin(cmath_tan)),
            ("phase", make_builtin(cmath_phase)),
            ("polar", make_builtin(cmath_polar)),
            ("rect", make_builtin(cmath_rect)),
            ("isnan", make_builtin(cmath_isnan)),
            ("isinf", make_builtin(cmath_isinf)),
            ("isfinite", make_builtin(cmath_isfinite)),
        ],
    )
}

fn cmath_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.sqrt", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    if im == 0.0 && re < 0.0 {
        return Ok(PyObject::complex(0.0, (-re).sqrt()));
    }
    let r = (re * re + im * im).sqrt();
    let out_re = ((r + re) / 2.0).sqrt();
    let out_im = if im < 0.0 {
        -((r - re) / 2.0).sqrt()
    } else {
        ((r - re) / 2.0).sqrt()
    };
    Ok(PyObject::complex(out_re, out_im))
}

fn cmath_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.exp", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    let e_re = re.exp();
    Ok(PyObject::complex(e_re * im.cos(), e_re * im.sin()))
}

fn cmath_log(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "cmath.log requires at least 1 argument",
        ));
    }
    let (re, im) = to_complex(&args[0]);
    let r = (re * re + im * im).sqrt();
    let theta = im.atan2(re);
    let ln_re = r.ln();
    let ln_im = theta;
    if args.len() > 1 {
        let (bre, bim) = to_complex(&args[1]);
        let br = (bre * bre + bim * bim).sqrt();
        let btheta = bim.atan2(bre);
        let bln_re = br.ln();
        let bln_im = btheta;
        // log_base(z) = ln(z) / ln(base), complex division
        let denom = bln_re * bln_re + bln_im * bln_im;
        let out_re = (ln_re * bln_re + ln_im * bln_im) / denom;
        let out_im = (ln_im * bln_re - ln_re * bln_im) / denom;
        Ok(PyObject::complex(out_re, out_im))
    } else {
        Ok(PyObject::complex(ln_re, ln_im))
    }
}

fn cmath_sin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.sin", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    Ok(PyObject::complex(
        re.sin() * im.cosh(),
        re.cos() * im.sinh(),
    ))
}

fn cmath_cos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.cos", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    Ok(PyObject::complex(
        re.cos() * im.cosh(),
        -(re.sin() * im.sinh()),
    ))
}

fn cmath_tan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.tan", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    let denom = (2.0 * re).cos() + (2.0 * im).cosh();
    if denom == 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::complex(
        (2.0 * re).sin() / denom,
        (2.0 * im).sinh() / denom,
    ))
}

fn cmath_phase(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.phase", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    Ok(PyObject::float(im.atan2(re)))
}

fn cmath_polar(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.polar", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    let r = (re * re + im * im).sqrt();
    let phi = im.atan2(re);
    Ok(PyObject::tuple(vec![
        PyObject::float(r),
        PyObject::float(phi),
    ]))
}

fn cmath_rect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.rect", args, 2)?;
    let r = args[0].to_float()?;
    let phi = args[1].to_float()?;
    Ok(PyObject::complex(r * phi.cos(), r * phi.sin()))
}

fn cmath_isnan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.isnan", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    Ok(PyObject::bool_val(re.is_nan() || im.is_nan()))
}

fn cmath_isinf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.isinf", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    Ok(PyObject::bool_val(re.is_infinite() || im.is_infinite()))
}

fn cmath_isfinite(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.isfinite", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    Ok(PyObject::bool_val(re.is_finite() && im.is_finite()))
}
