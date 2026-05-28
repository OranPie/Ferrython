use super::number::{index_bigint, math_number_to_float};
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use num_bigint::Sign;
use num_traits::ToPrimitive;
mod combinatorics;
mod elementary;
use combinatorics::{
    math_comb, math_factorial, math_gcd, math_isqrt, math_lcm, math_perm, math_prod,
};
use elementary::{
    math_acos, math_asin, math_atan, math_atan2, math_ceil, math_cos, math_degrees, math_exp,
    math_fabs, math_floor, math_isfinite, math_isinf, math_isnan, math_log, math_log10, math_log2,
    math_pow, math_radians, math_sin, math_sqrt, math_tan,
};
mod special;
use special::{math_dist, math_erf, math_erfc, math_fsum, math_gamma, math_lgamma};

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

fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 {
        return (0.0, 0);
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1022;
    let mantissa = f64::from_bits((bits & 0x800FFFFFFFFFFFFF) | 0x3FE0000000000000);
    (mantissa, exp)
}
