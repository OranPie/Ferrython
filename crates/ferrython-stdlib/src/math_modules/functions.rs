use super::number::{
    bigint_to_object, float_log2_exact_power, float_to_integral_object, index_bigint,
    is_odd_integer_float, isqrt_bigint, math_ln_arg, math_number_to_float, pyint_log2,
};
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyInt};
use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
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

fn math_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sqrt", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x < 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.sqrt()))
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

fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 {
        return (0.0, 0);
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1022;
    let mantissa = f64::from_bits((bits & 0x800FFFFFFFFFFFFF) | 0x3FE0000000000000);
    (mantissa, exp)
}
