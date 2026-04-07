//! Math and statistics stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, CompareOp,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

pub fn create_math_module() -> PyObjectRef {
    make_module("math", vec![
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
        ("isqrt", make_builtin(|args| {
            check_args("isqrt", args, 1)?;
            let n = args[0].as_int().ok_or_else(|| PyException::type_error("isqrt() argument must be an integer"))?;
            if n < 0 { return Err(PyException::value_error("isqrt() argument must be >= 0")); }
            Ok(PyObject::int((n as f64).sqrt() as i64))
        })),
        ("nextafter", make_builtin(|args| {
            check_args("nextafter", args, 2)?;
            let x = args[0].to_float()?;
            let y = args[1].to_float()?;
            // IEEE 754 nextafter: step x toward y
            if x == y { return Ok(PyObject::float(y)); }
            if x.is_nan() || y.is_nan() { return Ok(PyObject::float(f64::NAN)); }
            let bits = x.to_bits();
            let result = if (y > x) == (x >= 0.0) {
                f64::from_bits(bits + 1)
            } else {
                f64::from_bits(bits - 1)
            };
            Ok(PyObject::float(result))
        })),
        ("ulp", make_builtin(|args| {
            check_args("ulp", args, 1)?;
            let x = args[0].to_float()?;
            if x.is_nan() { return Ok(PyObject::float(f64::NAN)); }
            if x.is_infinite() { return Ok(PyObject::float(f64::INFINITY)); }
            let x = x.abs();
            let next = f64::from_bits(x.to_bits() + 1);
            Ok(PyObject::float(next - x))
        })),
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
    ])
}

fn math_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sqrt", args, 1)?;
    let x = args[0].to_float()?;
    if x < 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x.sqrt()))
}
fn math_ceil(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.ceil", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.ceil() as i64))
}
fn math_floor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.floor", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.floor() as i64))
}
fn math_fabs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fabs", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.abs()))
}
fn math_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.pow", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.powf(args[1].to_float()?)))
}
fn math_log(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("math.log requires at least 1 argument")); }
    let x = args[0].to_float()?;
    if x <= 0.0 { return Err(PyException::value_error("math domain error")); }
    if args.len() > 1 {
        let base = args[1].to_float()?;
        Ok(PyObject::float(x.ln() / base.ln()))
    } else {
        Ok(PyObject::float(x.ln()))
    }
}
fn math_log2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log2", args, 1)?;
    let x = args[0].to_float()?;
    if x <= 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x.log2()))
}
fn math_log10(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log10", args, 1)?;
    let x = args[0].to_float()?;
    if x <= 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x.log10()))
}
fn math_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.exp", args, 1)?;
    let result = args[0].to_float()?.exp();
    if result.is_infinite() {
        return Err(PyException::overflow_error("math range error"));
    }
    Ok(PyObject::float(result))
}
fn math_sin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sin", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.sin()))
}
fn math_cos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.cos", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.cos()))
}
fn math_tan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.tan", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.tan()))
}
fn math_asin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.asin", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.asin()))
}
fn math_acos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.acos", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.acos()))
}
fn math_atan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.atan()))
}
fn math_atan2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan2", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.atan2(args[1].to_float()?)))
}
fn math_degrees(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.degrees", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.to_degrees()))
}
fn math_radians(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.radians", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.to_radians()))
}
fn math_isnan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isnan", args, 1)?;
    Ok(PyObject::bool_val(args[0].to_float()?.is_nan()))
}
fn math_isinf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isinf", args, 1)?;
    Ok(PyObject::bool_val(args[0].to_float()?.is_infinite()))
}
fn math_isfinite(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isfinite", args, 1)?;
    Ok(PyObject::bool_val(args[0].to_float()?.is_finite()))
}
fn math_gcd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::int(0)); }
    if args.len() == 1 { return Ok(PyObject::int(args[0].to_int()?.abs())); }
    let mut result = args[0].to_int()?.abs();
    for arg in &args[1..] {
        let mut b = arg.to_int()?.abs();
        while b != 0 { let t = b; b = result % b; result = t; }
    }
    Ok(PyObject::int(result))
}
fn math_factorial(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.factorial", args, 1)?;
    let n = args[0].to_int()?;
    if n < 0 { return Err(PyException::value_error("factorial() not defined for negative values")); }
    let mut result: i64 = 1;
    for i in 2..=n {
        result = result.checked_mul(i).ok_or_else(|| PyException::overflow_error("factorial result too large"))?;
    }
    Ok(PyObject::int(result))
}
fn math_trunc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.trunc", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.trunc() as i64))
}
fn math_copysign(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.copysign", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.copysign(args[1].to_float()?)))
}
fn math_hypot(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.hypot", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.hypot(args[1].to_float()?)))
}
fn math_modf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.modf", args, 1)?;
    let x = args[0].to_float()?;
    let fract = x.fract();
    let trunc = x.trunc();
    Ok(PyObject::tuple(vec![PyObject::float(fract), PyObject::float(trunc)]))
}
fn math_fmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fmod", args, 2)?;
    Ok(PyObject::float(args[0].to_float()? % args[1].to_float()?))
}
fn math_frexp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.frexp", args, 1)?;
    let (m, e) = frexp(args[0].to_float()?);
    Ok(PyObject::tuple(vec![PyObject::float(m), PyObject::int(e as i64)]))
}
fn math_ldexp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.ldexp", args, 2)?;
    let x = args[0].to_float()?;
    let i = args[1].to_int()? as i32;
    Ok(PyObject::float(x * (2.0f64).powi(i)))
}

fn math_isclose(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("isclose() requires at least 2 arguments"));
    }
    let a = args[0].to_float()?;
    let b = args[1].to_float()?;
    // Extract rel_tol and abs_tol from positional args or trailing kwargs dict
    let mut rel_tol = 1e-9;
    let mut abs_tol = 0.0;
    let remaining = &args[2..];
    for arg in remaining {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            let map = d.read();
            if let Some(v) = map.get(&HashableKey::Str(CompactString::from("rel_tol"))) {
                rel_tol = v.to_float()?;
            }
            if let Some(v) = map.get(&HashableKey::Str(CompactString::from("abs_tol"))) {
                abs_tol = v.to_float()?;
            }
        } else if rel_tol == 1e-9 && abs_tol == 0.0 {
            // First non-dict remaining arg = rel_tol
            rel_tol = arg.to_float()?;
        } else {
            abs_tol = arg.to_float()?;
        }
    }
    if a == b { return Ok(PyObject::bool_val(true)); }
    if a.is_infinite() || b.is_infinite() { return Ok(PyObject::bool_val(false)); }
    let diff = (a - b).abs();
    Ok(PyObject::bool_val(diff <= (rel_tol * a.abs().max(b.abs())).max(abs_tol)))
}

fn math_comb(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.comb", args, 2)?;
    let n = args[0].to_int()?;
    let k = args[1].to_int()?;
    if k < 0 || n < 0 { return Ok(PyObject::int(0)); }
    if k > n { return Ok(PyObject::int(0)); }
    let k = k.min(n - k) as u64;
    let mut result: u64 = 1;
    for i in 0..k {
        result = result * (n as u64 - i) / (i + 1);
    }
    Ok(PyObject::int(result as i64))
}

fn math_perm(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() || args.len() > 2 {
        return Err(PyException::type_error("perm() requires 1 or 2 arguments"));
    }
    let n = args[0].to_int()?;
    let k = if args.len() == 2 { args[1].to_int()? } else { n };
    if k < 0 || n < 0 || k > n { return Ok(PyObject::int(0)); }
    let mut result: i64 = 1;
    for i in 0..k {
        result *= n - i;
    }
    Ok(PyObject::int(result))
}

fn math_prod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("prod() requires at least 1 argument"));
    }
    let items = args[0].to_list()?;
    let start = if args.len() >= 2 { args[1].to_float()? } else { 1.0 };
    let mut is_int = args.len() < 2;
    let mut product_f = start;
    let mut product_i: i64 = if args.len() >= 2 { start as i64 } else { 1 };
    for item in &items {
        if let Ok(v) = item.to_int() {
            if is_int { product_i *= v; }
            product_f *= v as f64;
        } else {
            is_int = false;
            product_f *= item.to_float()?;
        }
    }
    if is_int { Ok(PyObject::int(product_i)) } else { Ok(PyObject::float(product_f)) }
}

fn math_lcm(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::int(0)); }
    fn gcd(a: i64, b: i64) -> i64 { if b == 0 { a.abs() } else { gcd(b, a % b) } }
    let mut result = args[0].to_int()?.abs();
    for arg in &args[1..] {
        let b = arg.to_int()?.abs();
        if b == 0 { return Ok(PyObject::int(0)); }
        result = result / gcd(result, b) * b;
    }
    Ok(PyObject::int(result))
}

fn math_remainder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.remainder", args, 2)?;
    let x = args[0].to_float()?;
    let y = args[1].to_float()?;
    if y == 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x - (x / y).round() * y))
}

fn math_expm1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.expm1", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.exp_m1()))
}

fn math_log1p(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log1p", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.ln_1p()))
}

fn math_sinh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sinh", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.sinh()))
}
fn math_cosh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.cosh", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.cosh()))
}
fn math_tanh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.tanh", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.tanh()))
}
fn math_asinh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.asinh", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.asinh()))
}
fn math_acosh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.acosh", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.acosh()))
}
fn math_atanh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atanh", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.atanh()))
}

fn math_erf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.erf", args, 1)?;
    let x = args[0].to_float()?;
    // Abramowitz and Stegun approximation (7.1.26)
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let erf = 1.0 - poly * (-x * x).exp();
    Ok(PyObject::float(if x < 0.0 { -erf } else { erf }))
}
fn math_erfc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.erfc", args, 1)?;
    let x = args[0].to_float()?;
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let erf = 1.0 - poly * (-x * x).exp();
    Ok(PyObject::float(if x < 0.0 { 1.0 + erf } else { 1.0 - erf }))
}
fn math_gamma(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.gamma", args, 1)?;
    let x = args[0].to_float()?;
    if x <= 0.0 && x == x.floor() {
        return Err(PyException::value_error("math domain error"));
    }
    // Lanczos approximation
    Ok(PyObject::float(lanczos_gamma(x)))
}
fn math_lgamma(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.lgamma", args, 1)?;
    let x = args[0].to_float()?;
    if x <= 0.0 && x == x.floor() {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(lanczos_gamma(x).abs().ln()))
}

fn math_fsum(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fsum", args, 1)?;
    let items = args[0].to_list()?;
    // Shewchuk algorithm for accurate floating-point summation
    let mut partials: Vec<f64> = Vec::new();
    for item in &items {
        let mut x = item.to_float()?;
        let mut j = 0;
        for i in 0..partials.len() {
            let mut y = partials[i];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let lo = y - (hi - x);
            if lo != 0.0 {
                partials[j] = lo;
                j += 1;
            }
            x = hi;
        }
        partials.truncate(j);
        partials.push(x);
    }
    Ok(PyObject::float(partials.iter().sum::<f64>()))
}

fn math_dist(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.dist", args, 2)?;
    let p = args[0].to_list()?;
    let q = args[1].to_list()?;
    if p.len() != q.len() {
        return Err(PyException::value_error("both points must have the same number of dimensions"));
    }
    let mut sum = 0.0f64;
    for (a, b) in p.iter().zip(q.iter()) {
        let diff = a.to_float()? - b.to_float()?;
        sum += diff * diff;
    }
    Ok(PyObject::float(sum.sqrt()))
}

fn lanczos_gamma(x: f64) -> f64 {
    if x < 0.5 {
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * lanczos_gamma(1.0 - x))
    } else {
        let g = 7.0;
        let coefs = [
            0.99999999999980993, 676.5203681218851, -1259.1392167224028,
            771.32342877765313, -176.61502916214059, 12.507343278686905,
            -0.13857109526572012, 9.9843695780195716e-6, 1.5056327351493116e-7,
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
    if x == 0.0 { return (0.0, 0); }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1022;
    let mantissa = f64::from_bits((bits & 0x800FFFFFFFFFFFFF) | 0x3FE0000000000000);
    (mantissa, exp)
}

// ── sys module ──


fn stats_extract_floats(args: &[PyObjectRef]) -> PyResult<Vec<f64>> {
    if args.is_empty() { return Err(PyException::type_error("requires at least 1 argument")); }
    let items = args[0].to_list()?;
    if items.is_empty() { return Err(PyException::value_error("requires a non-empty dataset")); }
    Ok(items.iter().map(|x| x.to_float().unwrap_or(x.as_int().unwrap_or(0) as f64)).collect())
}

pub fn create_statistics_module() -> PyObjectRef {
    make_module("statistics", vec![
        ("mean", make_builtin(|args| {
            let vals = stats_extract_floats(args)?;
            Ok(PyObject::float(vals.iter().sum::<f64>() / vals.len() as f64))
        })),
        ("median", make_builtin(|args| {
            let mut vals = stats_extract_floats(args)?;
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = vals.len();
            if n % 2 == 1 { Ok(PyObject::float(vals[n / 2])) }
            else { Ok(PyObject::float((vals[n / 2 - 1] + vals[n / 2]) / 2.0)) }
        })),
        ("median_low", make_builtin(|args| {
            let mut vals = stats_extract_floats(args)?;
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = vals.len();
            if n % 2 == 1 { Ok(PyObject::float(vals[n / 2])) }
            else { Ok(PyObject::float(vals[n / 2 - 1])) }
        })),
        ("median_high", make_builtin(|args| {
            let mut vals = stats_extract_floats(args)?;
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = vals.len();
            Ok(PyObject::float(vals[n / 2]))
        })),
        ("mode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("mode requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.is_empty() { return Err(PyException::value_error("mode requires a non-empty dataset")); }
            let mut counts: IndexMap<String, (PyObjectRef, usize)> = IndexMap::new();
            for item in &items {
                let key = item.py_to_string();
                counts.entry(key).or_insert_with(|| (item.clone(), 0)).1 += 1;
            }
            let max_count = counts.values().map(|v| v.1).max().unwrap();
            let modes: Vec<_> = counts.values().filter(|v| v.1 == max_count).collect();
            if modes.len() > 1 {
                return Err(PyException::runtime_error("no unique mode; found multiple equally common values"));
            }
            Ok(modes[0].0.clone())
        })),
        ("multimode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("multimode requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.is_empty() { return Ok(PyObject::list(vec![])); }
            let mut counts: IndexMap<String, (PyObjectRef, usize)> = IndexMap::new();
            for item in &items {
                let key = item.py_to_string();
                counts.entry(key).or_insert_with(|| (item.clone(), 0)).1 += 1;
            }
            let max_count = counts.values().map(|v| v.1).max().unwrap();
            let modes: Vec<PyObjectRef> = counts.values()
                .filter(|v| v.1 == max_count)
                .map(|v| v.0.clone())
                .collect();
            Ok(PyObject::list(modes))
        })),
        ("stdev", make_builtin(|args| {
            let vals = stats_extract_floats(args)?;
            if vals.len() < 2 { return Err(PyException::value_error("stdev requires at least 2 data points")); }
            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            let var = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (vals.len() - 1) as f64;
            Ok(PyObject::float(var.sqrt()))
        })),
        ("variance", make_builtin(|args| {
            let vals = stats_extract_floats(args)?;
            if vals.len() < 2 { return Err(PyException::value_error("variance requires at least 2 data points")); }
            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            let var = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (vals.len() - 1) as f64;
            Ok(PyObject::float(var))
        })),
        ("pstdev", make_builtin(|args| {
            let vals = stats_extract_floats(args)?;
            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            let var = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / vals.len() as f64;
            Ok(PyObject::float(var.sqrt()))
        })),
        ("pvariance", make_builtin(|args| {
            let vals = stats_extract_floats(args)?;
            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            let var = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / vals.len() as f64;
            Ok(PyObject::float(var))
        })),
        ("harmonic_mean", make_builtin(|args| {
            let vals = stats_extract_floats(args)?;
            for v in &vals {
                if *v <= 0.0 { return Err(PyException::value_error("harmonic_mean requires positive data")); }
            }
            let reciprocal_sum: f64 = vals.iter().map(|x| 1.0 / x).sum();
            Ok(PyObject::float(vals.len() as f64 / reciprocal_sum))
        })),
        ("geometric_mean", make_builtin(|args| {
            let vals = stats_extract_floats(args)?;
            for v in &vals {
                if *v <= 0.0 { return Err(PyException::value_error("geometric_mean requires positive data")); }
            }
            let log_mean = vals.iter().map(|x| x.ln()).sum::<f64>() / vals.len() as f64;
            Ok(PyObject::float(log_mean.exp()))
        })),
        ("quantiles", make_builtin(|args| {
            let vals = stats_extract_floats(args)?;
            let n = if args.len() >= 2 { args[1].to_int().unwrap_or(4) as usize } else { 4 };
            if n < 1 { return Err(PyException::value_error("n must be at least 1")); }
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
        })),
        ("StatisticsError", PyObject::str_val(CompactString::from("StatisticsError"))),
    ])
}

// ── numbers module (stub) ──


pub fn create_numbers_module() -> PyObjectRef {
    // Abstract method that raises NotImplementedError
    fn make_abstract(name: &str) -> PyObjectRef {
        let n = CompactString::from(name);
        PyObject::native_closure(
            name,
            move |_args: &[PyObjectRef]| {
                Err(PyException::type_error(format!("{} is abstract", n)))
            },
        )
    }

    // Number — root of the numeric tower
    let mut number_ns = IndexMap::new();
    number_ns.insert(CompactString::from("__hash__"), make_abstract("Number.__hash__"));
    let number_class = PyObject::class(
        CompactString::from("Number"),
        vec![],
        number_ns,
    );

    // Complex — adds complex arithmetic operations
    let mut complex_ns = IndexMap::new();
    for op in &["__add__", "__radd__", "__sub__", "__rsub__",
                "__mul__", "__rmul__", "__truediv__", "__rtruediv__",
                "__pow__", "__rpow__", "__neg__", "__pos__", "__abs__",
                "__complex__", "__eq__", "__hash__",
                "real", "imag", "conjugate"] {
        complex_ns.insert(CompactString::from(*op), make_abstract(&format!("Complex.{}", op)));
    }
    complex_ns.insert(CompactString::from("__bool__"), make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::bool_val(true))
    }));
    let complex_class = PyObject::class(
        CompactString::from("Complex"),
        vec![number_class.clone()],
        complex_ns,
    );

    // Real — adds ordering and real-valued operations
    let mut real_ns = IndexMap::new();
    for op in &["__float__", "__trunc__", "__floor__", "__ceil__",
                "__round__", "__floordiv__", "__rfloordiv__",
                "__mod__", "__rmod__", "__lt__", "__le__"] {
        real_ns.insert(CompactString::from(*op), make_abstract(&format!("Real.{}", op)));
    }
    real_ns.insert(CompactString::from("real"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::float(0.0)); }
        Ok(args[0].clone())
    }));
    real_ns.insert(CompactString::from("imag"), make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::int(0))
    }));
    real_ns.insert(CompactString::from("conjugate"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::float(0.0)); }
        Ok(args[0].clone())
    }));
    let real_class = PyObject::class(
        CompactString::from("Real"),
        vec![complex_class.clone()],
        real_ns,
    );

    // Rational — adds numerator/denominator
    let mut rational_ns = IndexMap::new();
    rational_ns.insert(CompactString::from("numerator"), make_abstract("Rational.numerator"));
    rational_ns.insert(CompactString::from("denominator"), make_abstract("Rational.denominator"));
    rational_ns.insert(CompactString::from("__float__"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::float(0.0)); }
        let self_obj = &args[0];
        if let (Some(num), Some(den)) = (self_obj.get_attr("numerator"), self_obj.get_attr("denominator")) {
            let n = num.to_int().unwrap_or(0) as f64;
            let d = den.to_int().unwrap_or(1) as f64;
            return Ok(PyObject::float(if d != 0.0 { n / d } else { f64::NAN }));
        }
        Ok(PyObject::float(0.0))
    }));
    let rational_class = PyObject::class(
        CompactString::from("Rational"),
        vec![real_class.clone()],
        rational_ns,
    );

    // Integral — adds integer-specific operations
    let mut integral_ns = IndexMap::new();
    for op in &["__int__", "__index__", "__lshift__", "__rlshift__",
                "__rshift__", "__rrshift__", "__and__", "__rand__",
                "__xor__", "__rxor__", "__or__", "__ror__",
                "__invert__"] {
        integral_ns.insert(CompactString::from(*op), make_abstract(&format!("Integral.{}", op)));
    }
    integral_ns.insert(CompactString::from("__float__"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::float(0.0)); }
        let v = args[0].to_int().unwrap_or(0);
        Ok(PyObject::float(v as f64))
    }));
    integral_ns.insert(CompactString::from("numerator"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::int(0)); }
        Ok(args[0].clone())
    }));
    integral_ns.insert(CompactString::from("denominator"), make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::int(1))
    }));
    let integral_class = PyObject::class(
        CompactString::from("Integral"),
        vec![rational_class.clone()],
        integral_ns,
    );

    make_module("numbers", vec![
        ("Number", number_class),
        ("Complex", complex_class),
        ("Real", real_class),
        ("Rational", rational_class),
        ("Integral", integral_class),
    ])
}

// ── platform module ──


pub fn create_decimal_module() -> PyObjectRef {
    use parking_lot::RwLock;
    use std::sync::Arc;
    use ferrython_core::object::InstanceData;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::OnceLock;

    static DECIMAL_PREC: AtomicU32 = AtomicU32::new(28);
    static DECIMAL_CLASS: OnceLock<PyObjectRef> = OnceLock::new();

    fn get_prec() -> u32 {
        DECIMAL_PREC.load(Ordering::Relaxed)
    }

    fn get_decimal_class() -> PyObjectRef {
        DECIMAL_CLASS.get_or_init(|| {
            let mut dec_ns = IndexMap::new();
            dec_ns.insert(CompactString::from("__add__"), make_builtin(decimal_add));
            dec_ns.insert(CompactString::from("__radd__"), make_builtin(decimal_add));
            dec_ns.insert(CompactString::from("__sub__"), make_builtin(decimal_sub));
            dec_ns.insert(CompactString::from("__mul__"), make_builtin(decimal_mul));
            dec_ns.insert(CompactString::from("__truediv__"), make_builtin(decimal_div));
            dec_ns.insert(CompactString::from("__eq__"), make_builtin(decimal_eq));
            dec_ns.insert(CompactString::from("__lt__"), make_builtin(decimal_lt));
            dec_ns.insert(CompactString::from("__float__"), make_builtin(decimal_float));
            dec_ns.insert(CompactString::from("__int__"), make_builtin(decimal_int));
            dec_ns.insert(CompactString::from("__neg__"), make_builtin(decimal_neg));
            dec_ns.insert(CompactString::from("__abs__"), make_builtin(decimal_abs));
            dec_ns.insert(CompactString::from("__le__"), make_builtin(decimal_le));
            dec_ns.insert(CompactString::from("__gt__"), make_builtin(decimal_gt));
            dec_ns.insert(CompactString::from("__ge__"), make_builtin(decimal_ge));
            dec_ns.insert(CompactString::from("__str__"), make_builtin(decimal_str));
            dec_ns.insert(CompactString::from("__repr__"), make_builtin(decimal_str));
            dec_ns.insert(CompactString::from("__hash__"), make_builtin(decimal_hash));
            dec_ns.insert(CompactString::from("quantize"), make_builtin(decimal_quantize));
            dec_ns.insert(CompactString::from("sqrt"), make_builtin(decimal_sqrt));
            dec_ns.insert(CompactString::from("ln"), make_builtin(decimal_ln));
            dec_ns.insert(CompactString::from("exp"), make_builtin(decimal_exp));
            dec_ns.insert(CompactString::from("is_zero"), make_builtin(decimal_is_zero));
            dec_ns.insert(CompactString::from("is_nan"), make_builtin(decimal_is_nan));
            dec_ns.insert(CompactString::from("is_infinite"), make_builtin(decimal_is_infinite));
            dec_ns.insert(CompactString::from("is_finite"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::bool_val(true)); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                let v: f64 = s.parse().unwrap_or(0.0);
                Ok(PyObject::bool_val(v.is_finite()))
            }));
            dec_ns.insert(CompactString::from("is_signed"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::bool_val(false)); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                Ok(PyObject::bool_val(s.starts_with('-')))
            }));
            dec_ns.insert(CompactString::from("is_normal"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::bool_val(false)); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                let v: f64 = s.parse().unwrap_or(0.0);
                Ok(PyObject::bool_val(v.is_normal()))
            }));
            dec_ns.insert(CompactString::from("is_subnormal"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::bool_val(false)); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                let v: f64 = s.parse().unwrap_or(0.0);
                Ok(PyObject::bool_val(v.is_subnormal()))
            }));
            dec_ns.insert(CompactString::from("copy_abs"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("copy_abs requires self")); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                let s = s.trim_start_matches('-');
                Ok(make_decimal(s))
            }));
            dec_ns.insert(CompactString::from("copy_negate"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("copy_negate requires self")); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                let neg = if s.starts_with('-') { s[1..].to_string() } else { format!("-{}", s) };
                Ok(make_decimal(&neg))
            }));
            dec_ns.insert(CompactString::from("normalize"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("normalize requires self")); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                if s.contains('.') {
                    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
                    Ok(make_decimal(trimmed))
                } else {
                    Ok(make_decimal(&s))
                }
            }));
            dec_ns.insert(CompactString::from("adjusted"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::int(0)); }
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
            }));
            dec_ns.insert(CompactString::from("to_eng_string"), make_builtin(decimal_to_eng_string));
            // as_tuple() → DecimalTuple(sign, digits, exponent)
            dec_ns.insert(CompactString::from("as_tuple"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("as_tuple requires self")); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                let sign = if s.starts_with('-') { 1i64 } else { 0i64 };
                let abs_s = s.trim_start_matches('-').trim_start_matches('+');
                if abs_s == "NaN" {
                    return Ok(PyObject::tuple(vec![PyObject::int(0), PyObject::tuple(vec![]), PyObject::str_val(CompactString::from("n"))]));
                }
                if abs_s == "Infinity" {
                    return Ok(PyObject::tuple(vec![PyObject::int(sign), PyObject::tuple(vec![]), PyObject::str_val(CompactString::from("F"))]));
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
                let digit_objs: Vec<PyObjectRef> = digits_str.chars()
                    .filter(|c| c.is_ascii_digit())
                    .map(|c| PyObject::int((c as u8 - b'0') as i64))
                    .collect();
                Ok(PyObject::tuple(vec![PyObject::int(sign), PyObject::tuple(digit_objs), PyObject::int(exponent)]))
            }));
            // copy_sign(other) → Decimal with sign of other
            dec_ns.insert(CompactString::from("copy_sign"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("copy_sign requires self and other")); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                let other_s = get_decimal_str(&args[1]).unwrap_or_else(|| args[1].py_to_string());
                let abs_val = s.trim_start_matches('-').trim_start_matches('+');
                if other_s.starts_with('-') {
                    Ok(make_decimal(&format!("-{}", abs_val)))
                } else {
                    Ok(make_decimal(abs_val))
                }
            }));
            // __pow__
            dec_ns.insert(CompactString::from("__pow__"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("__pow__ requires two arguments")); }
                let a = get_decimal_str(&args[0]).unwrap_or_default().parse::<f64>().unwrap_or(0.0);
                let b = get_decimal_str(&args[1]).unwrap_or_else(|| args[1].py_to_string()).parse::<f64>().unwrap_or(0.0);
                Ok(make_decimal(&format!("{}", a.powf(b))))
            }));
            // __mod__
            dec_ns.insert(CompactString::from("__mod__"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("__mod__ requires two arguments")); }
                let a = get_decimal_str(&args[0]).unwrap_or_default().parse::<f64>().unwrap_or(0.0);
                let b = get_decimal_str(&args[1]).unwrap_or_else(|| args[1].py_to_string()).parse::<f64>().unwrap_or(1.0);
                if b == 0.0 { return Err(PyException::zero_division_error("decimal modulo by zero")); }
                let r = a % b;
                Ok(make_decimal(&format!("{}", r)))
            }));
            // __floordiv__
            dec_ns.insert(CompactString::from("__floordiv__"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("__floordiv__ requires two arguments")); }
                let a = get_decimal_str(&args[0]).unwrap_or_default().parse::<f64>().unwrap_or(0.0);
                let b = get_decimal_str(&args[1]).unwrap_or_else(|| args[1].py_to_string()).parse::<f64>().unwrap_or(1.0);
                if b == 0.0 { return Err(PyException::zero_division_error("decimal floor division by zero")); }
                Ok(make_decimal(&format!("{}", (a / b).floor())))
            }));
            // __bool__
            dec_ns.insert(CompactString::from("__bool__"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::bool_val(false)); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                let v: f64 = s.parse().unwrap_or(0.0);
                Ok(PyObject::bool_val(v != 0.0))
            }));
            // __round__
            dec_ns.insert(CompactString::from("__round__"), make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(make_decimal("0")); }
                let s = get_decimal_str(&args[0]).unwrap_or_default();
                let v: f64 = s.parse().unwrap_or(0.0);
                let ndigits = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
                let factor = 10f64.powi(ndigits as i32);
                let rounded = (v * factor).round() / factor;
                Ok(make_decimal(&format!("{}", rounded)))
            }));
            // max / min
            dec_ns.insert(CompactString::from("max"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("max requires self and other")); }
                let a = get_decimal_str(&args[0]).unwrap_or_default().parse::<f64>().unwrap_or(0.0);
                let b = get_decimal_str(&args[1]).unwrap_or_else(|| args[1].py_to_string()).parse::<f64>().unwrap_or(0.0);
                Ok(if a >= b { args[0].clone() } else { args[1].clone() })
            }));
            dec_ns.insert(CompactString::from("min"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("min requires self and other")); }
                let a = get_decimal_str(&args[0]).unwrap_or_default().parse::<f64>().unwrap_or(0.0);
                let b = get_decimal_str(&args[1]).unwrap_or_else(|| args[1].py_to_string()).parse::<f64>().unwrap_or(0.0);
                Ok(if a <= b { args[0].clone() } else { args[1].clone() })
            }));
            // __new__ enables Decimal("1.23") to work when called as class constructor
            dec_ns.insert(CompactString::from("__new__"), PyObject::native_function(
                "Decimal.__new__", |args: &[PyObjectRef]| {
                    // args[0] = cls, args[1..] = constructor args
                    if args.len() < 2 { return Ok(make_decimal("0")); }
                    let s = args[1].py_to_string();
                    let trimmed = s.trim();
                    if trimmed.is_empty() { return Ok(make_decimal("0")); }
                    match &args[1].payload {
                        PyObjectPayload::Int(n) => return Ok(make_decimal(&format!("{}", n.to_i64().unwrap_or(0)))),
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
                    let parts: Vec<&str> = check.splitn(2, '.').collect();
                    let valid = parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
                        || check == "Infinity" || check == "NaN";
                    if valid {
                        Ok(make_decimal(trimmed))
                    } else if check.contains('E') || check.contains('e') {
                        match trimmed.parse::<f64>() {
                            Ok(f) => Ok(make_decimal(&format!("{}", f))),
                            Err(_) => Err(PyException::value_error(format!("Invalid literal for Decimal: '{}'", s))),
                        }
                    } else {
                        Err(PyException::value_error(format!("Invalid literal for Decimal: '{}'", s)))
                    }
                }
            ));
            PyObject::class(CompactString::from("Decimal"), vec![], dec_ns)
        }).clone()
    }

    fn make_decimal(s: &str) -> PyObjectRef {
        let class = get_decimal_class();
        let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
            class,
            attrs: Arc::new(RwLock::new(IndexMap::new())),
            is_special: true, dict_storage: None,
        }));
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("__decimal__"), PyObject::bool_val(true));
            w.insert(CompactString::from("_value"), PyObject::str_val(CompactString::from(s)));
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
        let (neg, s) = if s.starts_with('-') { (true, &s[1..]) } else if s.starts_with('+') { (false, &s[1..]) } else { (false, s) };
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
            if neg && digits != 0 { format!("-{}", digits) } else { format!("{}", digits) }
        } else {
            let s = format!("{:0>width$}", digits, width = scale as usize + 1);
            let (int_part, frac_part) = s.split_at(s.len() - scale as usize);
            if neg && digits != 0 { format!("-{}.{}", int_part, frac_part) } else { format!("{}.{}", int_part, frac_part) }
        }
    }

    fn align_scales(a: (bool, i128, u32), b: (bool, i128, u32)) -> ((bool, i128, u32), (bool, i128, u32)) {
        let max_scale = a.2.max(b.2);
        let a_digits = a.1 * 10i128.pow(max_scale - a.2);
        let b_digits = b.1 * 10i128.pow(max_scale - b.2);
        ((a.0, a_digits, max_scale), (b.0, b_digits, max_scale))
    }

    fn decimal_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("Decimal.__add__ requires 2 args")); }
        let a_str = get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let b_str = get_decimal_str(&args[1]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
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
        if args.len() < 2 { return Err(PyException::type_error("Decimal.__sub__ requires 2 args")); }
        let a_str = get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let b_str = get_decimal_str(&args[1]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
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
        if args.len() < 2 { return Err(PyException::type_error("Decimal.__mul__ requires 2 args")); }
        let a_str = get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let b_str = get_decimal_str(&args[1]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let a = decimal_parse(&a_str);
        let b = decimal_parse(&b_str);
        let neg = a.0 != b.0;
        let digits = a.1 * b.1;
        let scale = a.2 + b.2;
        Ok(make_decimal(&decimal_format(neg, digits, scale)))
    }

    fn decimal_div(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("Decimal.__truediv__ requires 2 args")); }
        let a_str = get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let b_str = get_decimal_str(&args[1]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let a = decimal_parse(&a_str);
        let b = decimal_parse(&b_str);
        if b.1 == 0 { return Err(PyException::zero_division_error("decimal division by zero")); }
        let neg = a.0 != b.0;
        // Cap precision to avoid i128 overflow (i128 has ~38 digits of range)
        let precision = get_prec().min(36);
        let a_scaled = a.1.checked_mul(10i128.pow(precision))
            .unwrap_or_else(|| {
                // Fallback: reduce precision to fit
                let safe_prec = 18u32.min(precision);
                a.1 * 10i128.pow(safe_prec)
            });
        let result = a_scaled / b.1;
        let total_scale = a.2 + precision - b.2;
        Ok(make_decimal(&decimal_format(neg, result, total_scale)))
    }

    fn decimal_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
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
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
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
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        let f: f64 = s.parse().unwrap_or(0.0);
        Ok(PyObject::float(f))
    }

    fn decimal_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        let (neg, digits, scale) = decimal_parse(&s);
        let int_val = digits / 10i128.pow(scale);
        Ok(PyObject::int(if neg { -(int_val as i64) } else { int_val as i64 }))
    }

    fn decimal_neg(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        let (neg, digits, scale) = decimal_parse(&s);
        Ok(make_decimal(&decimal_format(!neg, digits, scale)))
    }

    fn decimal_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        let (_, digits, scale) = decimal_parse(&s);
        Ok(make_decimal(&decimal_format(false, digits, scale)))
    }

    fn decimal_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
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
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
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
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
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
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        Ok(PyObject::str_val(CompactString::from(s)))
    }

    fn decimal_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        let f: f64 = s.parse().unwrap_or(0.0);
        Ok(PyObject::int(f.to_bits() as i64))
    }

    fn decimal_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        if s == "NaN" { return Ok(make_decimal("NaN")); }
        let f: f64 = s.parse().unwrap_or(0.0);
        if f < 0.0 { return Err(PyException::value_error("Square root of negative number")); }
        let result = f.sqrt();
        Ok(make_decimal(&format!("{}", result)))
    }

    fn decimal_ln(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        if s == "NaN" { return Ok(make_decimal("NaN")); }
        let f: f64 = s.parse().unwrap_or(0.0);
        if f <= 0.0 { return Err(PyException::value_error("ln of non-positive number")); }
        let result = f.ln();
        Ok(make_decimal(&format!("{}", result)))
    }

    fn decimal_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        if s == "NaN" { return Ok(make_decimal("NaN")); }
        let f: f64 = s.parse().unwrap_or(0.0);
        let result = f.exp();
        Ok(make_decimal(&format!("{}", result)))
    }

    fn decimal_is_zero(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        let (_, digits, _) = decimal_parse(&s);
        Ok(PyObject::bool_val(digits == 0))
    }

    fn decimal_is_nan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        Ok(PyObject::bool_val(s == "NaN"))
    }

    fn decimal_is_infinite(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
        Ok(PyObject::bool_val(s == "Infinity" || s == "-Infinity"))
    }

    fn decimal_to_eng_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let s = get_decimal_str(&args[0]).unwrap_or_else(|| "0".to_string());
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
            if neg { format!("-{}", mantissa) } else { format!("{}", mantissa) }
        } else {
            if neg { format!("-{}E+{}", mantissa, eng_exp) } else { format!("{}E+{}", mantissa, eng_exp) }
        };
        Ok(PyObject::str_val(CompactString::from(&result)))
    }

    /// quantize(self, exp, rounding=None) — round to the scale of exp
    fn decimal_quantize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("quantize requires 2 args")); }
        let a_str = get_decimal_str(&args[0]).ok_or_else(|| PyException::type_error("not a Decimal"))?;
        let exp_str = get_decimal_str(&args[1]).ok_or_else(|| PyException::type_error("quantize exp must be Decimal"))?;
        let (neg, digits, scale) = decimal_parse(&a_str);
        let (_, _, target_scale) = decimal_parse(&exp_str);

        // Extract rounding mode from kwargs
        let rounding = if args.len() > 2 {
            if let Some(s) = args[2].as_str() { s.to_string() }
            else if let PyObjectPayload::Dict(d) = &args[args.len() - 1].payload {
                d.read().get(&HashableKey::Str(CompactString::from("rounding")))
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default()
            } else { String::new() }
        } else { String::new() };

        let val = if neg { -(digits as i128) } else { digits as i128 };
        let result = if target_scale < scale {
            // Reduce scale — need rounding
            let factor = 10i128.pow(scale - target_scale);
            let truncated = val / factor;
            let remainder = (val % factor).unsigned_abs();
            let half = factor.unsigned_abs() / 2;
            let rounded = match rounding.as_str() {
                "ROUND_HALF_UP" => {
                    if remainder >= half { if val >= 0 { truncated + 1 } else { truncated - 1 } }
                    else { truncated }
                }
                "ROUND_CEILING" => {
                    if remainder > 0 && val > 0 { truncated + 1 } else { truncated }
                }
                "ROUND_FLOOR" => {
                    if remainder > 0 && val < 0 { truncated - 1 } else { truncated }
                }
                _ => {
                    // ROUND_HALF_EVEN (default banker's rounding)
                    if remainder > half { if val >= 0 { truncated + 1 } else { truncated - 1 } }
                    else if remainder == half {
                        if truncated % 2 != 0 { if val >= 0 { truncated + 1 } else { truncated - 1 } }
                        else { truncated }
                    } else { truncated }
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
            let s = if r_neg { format!("-{}", r_digits) } else { format!("{}", r_digits) };
            Ok(make_decimal(&s))
        } else {
            let s = format!("{:0>width$}", r_digits, width = target_scale as usize + 1);
            let (int_part, frac_part) = s.split_at(s.len() - target_scale as usize);
            let formatted = if r_neg { format!("-{}.{}", int_part, frac_part) } else { format!("{}.{}", int_part, frac_part) };
            Ok(make_decimal(&formatted))
        }
    }

    make_module("decimal", vec![
        ("Decimal", get_decimal_class()),
        ("ROUND_HALF_UP", PyObject::str_val(CompactString::from("ROUND_HALF_UP"))),
        ("ROUND_HALF_DOWN", PyObject::str_val(CompactString::from("ROUND_HALF_DOWN"))),
        ("ROUND_HALF_EVEN", PyObject::str_val(CompactString::from("ROUND_HALF_EVEN"))),
        ("ROUND_CEILING", PyObject::str_val(CompactString::from("ROUND_CEILING"))),
        ("ROUND_FLOOR", PyObject::str_val(CompactString::from("ROUND_FLOOR"))),
        ("ROUND_DOWN", PyObject::str_val(CompactString::from("ROUND_DOWN"))),
        ("ROUND_UP", PyObject::str_val(CompactString::from("ROUND_UP"))),
        ("ROUND_05UP", PyObject::str_val(CompactString::from("ROUND_05UP"))),
        ("getcontext", make_builtin(|_| {
            use std::sync::atomic::Ordering;
            let current_prec = DECIMAL_PREC.load(Ordering::Relaxed);
            let mut ctx_ns = IndexMap::new();
            ctx_ns.insert(CompactString::from("prec"), PyObject::int(current_prec as i64));
            ctx_ns.insert(CompactString::from("rounding"), PyObject::str_val(CompactString::from("ROUND_HALF_EVEN")));
            ctx_ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
            ctx_ns.insert(CompactString::from("Emax"), PyObject::int(999999));
            ctx_ns.insert(CompactString::from("capitals"), PyObject::int(1));
            ctx_ns.insert(CompactString::from("clamp"), PyObject::int(0));
            // Add __setattr__ to intercept prec assignment
            let cls_ns = {
                let mut ns = IndexMap::new();
                ns.insert(CompactString::from("__setattr__"), make_builtin(|args| {
                    use std::sync::atomic::Ordering;
                    if args.len() < 3 { return Ok(PyObject::none()); }
                    let attr_name = args[1].py_to_string();
                    if attr_name == "prec" {
                        let new_prec = args[2].to_int()? as u32;
                        DECIMAL_PREC.store(new_prec, Ordering::Relaxed);
                        // Also update the instance attribute
                        if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                            inst.attrs.write().insert(CompactString::from("prec"), PyObject::int(new_prec as i64));
                        }
                    } else if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs.write().insert(CompactString::from(attr_name), args[2].clone());
                    }
                    Ok(PyObject::none())
                }));
                ns
            };
            let cls = PyObject::class(CompactString::from("Context"), vec![], cls_ns);
            let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
                class: cls,
                attrs: Arc::new(RwLock::new(ctx_ns)),
                is_special: true, dict_storage: None,
            }));
            Ok(inst)
        })),
        ("setcontext", make_builtin(|args| {
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
        })),
        ("localcontext", make_builtin(|args| {
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
            ctx_ns.insert(CompactString::from("prec"), PyObject::int(DECIMAL_PREC.load(Ordering::Relaxed) as i64));
            ctx_ns.insert(CompactString::from("rounding"), PyObject::str_val(CompactString::from("ROUND_HALF_EVEN")));
            ctx_ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
            ctx_ns.insert(CompactString::from("Emax"), PyObject::int(999999));

            // __setattr__ on the context
            let cls_ns = {
                let mut ns = IndexMap::new();
                ns.insert(CompactString::from("__setattr__"), make_builtin(|args| {
                    if args.len() < 3 { return Ok(PyObject::none()); }
                    let attr_name = args[1].py_to_string();
                    if attr_name == "prec" {
                        let new_prec = args[2].to_int()? as u32;
                        DECIMAL_PREC.store(new_prec, Ordering::Relaxed);
                        if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                            inst.attrs.write().insert(CompactString::from("prec"), PyObject::int(new_prec as i64));
                        }
                    } else if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs.write().insert(CompactString::from(attr_name), args[2].clone());
                    }
                    Ok(PyObject::none())
                }));
                ns
            };
            let cls = PyObject::class(CompactString::from("Context"), vec![], cls_ns);
            let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
                class: cls,
                attrs: Arc::new(RwLock::new(ctx_ns)),
                is_special: true, dict_storage: None,
            }));
            // Add __enter__ and __exit__ for context manager
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();
                let ctx_clone = inst.clone();
                attrs.insert(CompactString::from("__enter__"), PyObject::native_closure("localcontext.__enter__", move |_| {
                    Ok(ctx_clone.clone())
                }));
                attrs.insert(CompactString::from("__exit__"), PyObject::native_closure("localcontext.__exit__", move |_| {
                    DECIMAL_PREC.store(saved_prec, Ordering::Relaxed);
                    Ok(PyObject::bool_val(false))
                }));
            }
            Ok(inst)
        })),
        ("InvalidOperation", PyObject::exception_type(ferrython_core::error::ExceptionKind::ArithmeticError)),
        ("DivisionByZero", PyObject::exception_type(ferrython_core::error::ExceptionKind::ZeroDivisionError)),
        ("Overflow", PyObject::exception_type(ferrython_core::error::ExceptionKind::OverflowError)),
        ("Underflow", PyObject::exception_type(ferrython_core::error::ExceptionKind::ArithmeticError)),
        ("Inexact", PyObject::exception_type(ferrython_core::error::ExceptionKind::ArithmeticError)),
        ("Rounded", PyObject::exception_type(ferrython_core::error::ExceptionKind::ArithmeticError)),
        ("Subnormal", PyObject::exception_type(ferrython_core::error::ExceptionKind::ArithmeticError)),
        ("FloatOperation", PyObject::exception_type(ferrython_core::error::ExceptionKind::ArithmeticError)),
        ("DecimalException", PyObject::exception_type(ferrython_core::error::ExceptionKind::ArithmeticError)),
        ("BasicContext", {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("prec"), PyObject::int(9));
            ns.insert(CompactString::from("rounding"), PyObject::str_val(CompactString::from("ROUND_HALF_UP")));
            ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
            ns.insert(CompactString::from("Emax"), PyObject::int(999999));
            let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
            PyObject::wrap(PyObjectPayload::Instance(InstanceData {
                class: cls,
                attrs: Arc::new(RwLock::new(ns)),
                is_special: true, dict_storage: None,
            }))
        }),
        ("ExtendedContext", {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("prec"), PyObject::int(9));
            ns.insert(CompactString::from("rounding"), PyObject::str_val(CompactString::from("ROUND_HALF_EVEN")));
            ns.insert(CompactString::from("Emin"), PyObject::int(-999999));
            ns.insert(CompactString::from("Emax"), PyObject::int(999999));
            let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
            PyObject::wrap(PyObjectPayload::Instance(InstanceData {
                class: cls,
                attrs: Arc::new(RwLock::new(ns)),
                is_special: true, dict_storage: None,
            }))
        }),
    ])
}


// ── statistics module ──


pub fn create_random_module() -> PyObjectRef {
    make_module("random", vec![
        ("random", make_builtin(random_random)),
        ("randint", make_builtin(random_randint)),
        ("choice", make_builtin(random_choice)),
        ("shuffle", make_builtin(random_shuffle)),
        ("seed", make_builtin(random_seed)),
        ("randrange", make_builtin(random_randrange)),
        ("uniform", make_builtin(|args| {
            check_args("random.uniform", args, 2)?;
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a + simple_random() * (b - a)))
        })),
        ("sample", make_builtin(|args| {
            check_args("random.sample", args, 2)?;
            let items = args[0].to_list()?;
            let k = args[1].to_int()? as usize;
            if k > items.len() { return Err(PyException::value_error("Sample larger than population")); }
            let mut result = Vec::with_capacity(k);
            let mut pool = items.clone();
            for _ in 0..k {
                let idx = (simple_random() * pool.len() as f64) as usize;
                let idx = idx.min(pool.len() - 1);
                result.push(pool.remove(idx));
            }
            Ok(PyObject::list(result))
        })),
        ("choices", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("random.choices requires at least 1 argument")); }
            let items = args[0].to_list()?;
            let mut k = 1usize;
            let mut weights: Option<Vec<f64>> = None;
            for arg in args.iter().skip(1) {
                if let PyObjectPayload::Dict(d) = &arg.payload {
                    let d = d.read();
                    if let Some(kv) = d.get(&HashableKey::Str(CompactString::from("k"))) {
                        k = kv.to_int()? as usize;
                    }
                    if let Some(wv) = d.get(&HashableKey::Str(CompactString::from("weights"))) {
                        let wl = wv.to_list()?;
                        weights = Some(wl.iter().map(|w| w.to_float().unwrap_or(1.0)).collect());
                    }
                    if let Some(cwv) = d.get(&HashableKey::Str(CompactString::from("cum_weights"))) {
                        let cwl = cwv.to_list()?;
                        let cw: Vec<f64> = cwl.iter().map(|w| w.to_float().unwrap_or(0.0)).collect();
                        // Convert cumulative weights back to regular weights
                        let mut w = Vec::with_capacity(cw.len());
                        for i in 0..cw.len() {
                            w.push(if i == 0 { cw[0] } else { cw[i] - cw[i-1] });
                        }
                        weights = Some(w);
                    }
                }
            }
            if items.is_empty() {
                return Err(PyException::value_error("Cannot choose from an empty population"));
            }
            let mut result = Vec::with_capacity(k);
            if let Some(ref w) = weights {
                let total: f64 = w.iter().sum();
                for _ in 0..k {
                    let mut r = simple_random() * total;
                    let mut chosen = items.len() - 1;
                    for (i, &weight) in w.iter().enumerate() {
                        r -= weight;
                        if r <= 0.0 { chosen = i; break; }
                    }
                    result.push(items[chosen.min(items.len()-1)].clone());
                }
            } else {
                for _ in 0..k {
                    let idx = (simple_random() * items.len() as f64) as usize;
                    result.push(items[idx.min(items.len()-1)].clone());
                }
            }
            Ok(PyObject::list(result))
        })),
        ("gauss", make_builtin(|args| {
            let mu = if !args.is_empty() { args[0].to_float()? } else { 0.0 };
            let sigma = if args.len() > 1 { args[1].to_float()? } else { 1.0 };
            // Box-Muller transform
            let u1 = simple_random().max(1e-10);
            let u2 = simple_random();
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            Ok(PyObject::float(mu + sigma * z))
        })),
        ("normalvariate", make_builtin(|args| {
            let mu = if !args.is_empty() { args[0].to_float()? } else { 0.0 };
            let sigma = if args.len() > 1 { args[1].to_float()? } else { 1.0 };
            let u1 = simple_random().max(1e-10);
            let u2 = simple_random();
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            Ok(PyObject::float(mu + sigma * z))
        })),
        ("expovariate", make_builtin(|args| {
            check_args("random.expovariate", args, 1)?;
            let lambd = args[0].to_float()?;
            if lambd == 0.0 { return Err(PyException::value_error("expovariate: lambd must not be 0")); }
            let u = simple_random().max(1e-10);
            Ok(PyObject::float(-u.ln() / lambd))
        })),
        ("triangular", make_builtin(|args| {
            let low = if !args.is_empty() { args[0].to_float()? } else { 0.0 };
            let high = if args.len() > 1 { args[1].to_float()? } else { 1.0 };
            let mode = if args.len() > 2 { args[2].to_float()? } else { (low + high) / 2.0 };
            let u = simple_random();
            let c = (mode - low) / (high - low);
            if u < c {
                Ok(PyObject::float(low + (u * (high - low) * (mode - low)).sqrt()))
            } else {
                Ok(PyObject::float(high - ((1.0 - u) * (high - low) * (high - mode)).sqrt()))
            }
        })),
        ("getrandbits", make_builtin(|args| {
            check_args("random.getrandbits", args, 1)?;
            let k = args[0].to_int()? as u32;
            if k == 0 { return Ok(PyObject::int(0)); }
            let mut result: i64 = 0;
            for _ in 0..k.min(62) {
                result = (result << 1) | (if simple_random() < 0.5 { 1 } else { 0 });
            }
            Ok(PyObject::int(result))
        })),
        ("getstate", make_builtin(|_| {
            RNG.with(|rng| {
                let r = rng.borrow();
                Ok(PyObject::tuple(vec![
                    PyObject::int(r.s[0] as i64),
                    PyObject::int(r.s[1] as i64),
                    PyObject::int(r.s[2] as i64),
                    PyObject::int(r.s[3] as i64),
                ]))
            })
        })),
        ("setstate", make_builtin(|args| {
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
            Err(PyException::type_error("state must be a 4-tuple of integers"))
        })),
    ])
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
    if items.is_empty() { return Err(PyException::index_error("Cannot choose from an empty sequence")); }
    let idx = (simple_random() * items.len() as f64) as usize;
    Ok(items[idx.min(items.len()-1)].clone())
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
            .unwrap_or_default().as_nanos() as u64
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
    if args.is_empty() { return Err(PyException::type_error("randrange requires at least 1 argument")); }
    let start = if args.len() == 1 { 0 } else { args[0].to_int()? };
    let stop = if args.len() == 1 { args[0].to_int()? } else { args[1].to_int()? };
    let step = if args.len() > 2 { args[2].to_int()? } else { 1 };
    let range = ((stop - start) as f64 / step as f64).ceil() as i64;
    if range <= 0 { return Err(PyException::value_error("empty range for randrange()")); }
    let idx = (simple_random() * range as f64) as i64;
    Ok(PyObject::int(start + idx * step))
}

// ── Stub modules ──

// ── heapq module ──

pub fn create_heapq_module() -> PyObjectRef {
    make_module("heapq", vec![
        ("heappush", make_builtin(heapq_push)),
        ("heappop", make_builtin(heapq_pop)),
        ("heapify", make_builtin(heapq_heapify)),
        ("heappushpop", make_builtin(heapq_pushpop)),
        ("heapreplace", make_builtin(heapq_replace)),
        ("nlargest", make_builtin(heapq_nlargest)),
        ("nsmallest", make_builtin(heapq_nsmallest)),
        ("merge", make_builtin(heapq_merge)),
    ])
}

fn heap_cmp_lt(a: &PyObjectRef, b: &PyObjectRef) -> bool {
    a.compare(b, CompareOp::Lt).map(|v| v.is_truthy()).unwrap_or(false)
}

fn heap_sift_up(items: &mut Vec<PyObjectRef>, mut pos: usize) {
    while pos > 0 {
        let parent = (pos - 1) / 2;
        if heap_cmp_lt(&items[pos], &items[parent]) {
            items.swap(pos, parent);
            pos = parent;
        } else {
            break;
        }
    }
}

fn heap_sift_down(items: &mut Vec<PyObjectRef>, mut pos: usize, end: usize) {
    loop {
        let mut child = 2 * pos + 1;
        if child >= end { break; }
        let right = child + 1;
        if right < end && heap_cmp_lt(&items[right], &items[child]) {
            child = right;
        }
        if heap_cmp_lt(&items[child], &items[pos]) {
            items.swap(pos, child);
            pos = child;
        } else {
            break;
        }
    }
}

fn heapq_push(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heappush", args, 2)?;
    let list_obj = &args[0];
    if let PyObjectPayload::List(lock) = &list_obj.payload {
        let mut items = lock.write();
        items.push(args[1].clone());
        let pos = items.len() - 1;
        heap_sift_up(&mut items, pos);
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error("heappush: first arg must be a list"))
    }
}

fn heapq_pop(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heappop", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let mut items = lock.write();
        if items.is_empty() { return Err(PyException::index_error("index out of range")); }
        let len = items.len();
        if len == 1 { return Ok(items.pop().unwrap()); }
        items.swap(0, len - 1);
        let result = items.pop().unwrap();
        let n = items.len();
        heap_sift_down(&mut items, 0, n);
        Ok(result)
    } else {
        Err(PyException::type_error("heappop: arg must be a list"))
    }
}

fn heapq_heapify(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heapify", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let mut items = lock.write();
        let n = items.len();
        for i in (0..n / 2).rev() {
            heap_sift_down(&mut items, i, n);
        }
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error("heapify: arg must be a list"))
    }
}

fn heapq_pushpop(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heappushpop", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let mut items = lock.write();
        if items.is_empty() || heap_cmp_lt(&args[1], &items[0]) {
            return Ok(args[1].clone());
        }
        let result = std::mem::replace(&mut items[0], args[1].clone());
        let n = items.len();
        heap_sift_down(&mut items, 0, n);
        Ok(result)
    } else {
        Err(PyException::type_error("heappushpop: first arg must be a list"))
    }
}

fn heapq_replace(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heapreplace", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let mut items = lock.write();
        if items.is_empty() { return Err(PyException::index_error("index out of range")); }
        let result = std::mem::replace(&mut items[0], args[1].clone());
        let n = items.len();
        heap_sift_down(&mut items, 0, n);
        Ok(result)
    } else {
        Err(PyException::type_error("heapreplace: first arg must be a list"))
    }
}

fn heapq_nlargest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("nlargest", args, 2)?;
    let n = args[0].to_int()? as usize;
    let items = args[1].to_list()?;
    let mut sorted = items.clone();
    sorted.sort_by(|a, b| {
        if heap_cmp_lt(b, a) { std::cmp::Ordering::Less }
        else if heap_cmp_lt(a, b) { std::cmp::Ordering::Greater }
        else { std::cmp::Ordering::Equal }
    });
    sorted.truncate(n);
    Ok(PyObject::list(sorted))
}

fn heapq_nsmallest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("nsmallest", args, 2)?;
    let n = args[0].to_int()? as usize;
    let items = args[1].to_list()?;
    let mut sorted = items.clone();
    sorted.sort_by(|a, b| {
        if heap_cmp_lt(a, b) { std::cmp::Ordering::Less }
        else if heap_cmp_lt(b, a) { std::cmp::Ordering::Greater }
        else { std::cmp::Ordering::Equal }
    });
    sorted.truncate(n);
    Ok(PyObject::list(sorted))
}

fn heapq_merge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Simplified: concatenate all iterables and sort
    let mut all = Vec::new();
    for arg in args {
        all.extend(arg.to_list()?);
    }
    all.sort_by(|a, b| {
        if heap_cmp_lt(a, b) { std::cmp::Ordering::Less }
        else if heap_cmp_lt(b, a) { std::cmp::Ordering::Greater }
        else { std::cmp::Ordering::Equal }
    });
    Ok(PyObject::list(all))
}

// ── bisect module ──

pub fn create_bisect_module() -> PyObjectRef {
    make_module("bisect", vec![
        ("bisect_left", make_builtin(bisect_left)),
        ("bisect_right", make_builtin(bisect_right)),
        ("bisect", make_builtin(bisect_right)), // bisect is alias for bisect_right
        ("insort_left", make_builtin(insort_left)),
        ("insort_right", make_builtin(insort_right)),
        ("insort", make_builtin(insort_right)), // insort is alias for insort_right
    ])
}

fn bisect_left_idx(items: &[PyObjectRef], x: &PyObjectRef, lo: usize, hi: usize) -> usize {
    let mut lo = lo;
    let mut hi = hi;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if heap_cmp_lt(&items[mid], x) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn bisect_right_idx(items: &[PyObjectRef], x: &PyObjectRef, lo: usize, hi: usize) -> usize {
    let mut lo = lo;
    let mut hi = hi;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if heap_cmp_lt(x, &items[mid]) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    lo
}

fn bisect_left(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("bisect_left", args, 2)?;
    let items = args[0].to_list()?;
    let lo = if args.len() > 2 { args[2].to_int()? as usize } else { 0 };
    let hi = if args.len() > 3 { args[3].to_int()? as usize } else { items.len() };
    Ok(PyObject::int(bisect_left_idx(&items, &args[1], lo, hi) as i64))
}

fn bisect_right(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("bisect_right", args, 2)?;
    let items = args[0].to_list()?;
    let lo = if args.len() > 2 { args[2].to_int()? as usize } else { 0 };
    let hi = if args.len() > 3 { args[3].to_int()? as usize } else { items.len() };
    Ok(PyObject::int(bisect_right_idx(&items, &args[1], lo, hi) as i64))
}

fn insort_left(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("insort_left", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let mut items = lock.write();
        let lo = if args.len() > 2 { args[2].to_int()? as usize } else { 0 };
        let hi = if args.len() > 3 { args[3].to_int()? as usize } else { items.len() };
        let idx = bisect_left_idx(&items, &args[1], lo, hi);
        items.insert(idx, args[1].clone());
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error("insort_left: first arg must be a list"))
    }
}

fn insort_right(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("insort_right", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let mut items = lock.write();
        let lo = if args.len() > 2 { args[2].to_int()? as usize } else { 0 };
        let hi = if args.len() > 3 { args[3].to_int()? as usize } else { items.len() };
        let idx = bisect_right_idx(&items, &args[1], lo, hi);
        items.insert(idx, args[1].clone());
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error("insort_right: first arg must be a list"))
    }
}

// ── fractions module ─────────────────────────────────────────────────
pub fn create_fractions_module() -> PyObjectRef {
    use ferrython_core::object::InstanceData;
    use parking_lot::RwLock;
    use std::sync::Arc;

    fn frac_gcd(mut a: i64, mut b: i64) -> i64 {
        a = a.abs(); b = b.abs();
        while b != 0 { let t = b; b = a % b; a = t; }
        a
    }

    fn get_frac_parts(obj: &PyObjectRef) -> Option<(i64, i64)> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__fraction__") {
                let n = attrs.get("numerator").and_then(|v| v.to_int().ok()).unwrap_or(0);
                let d = attrs.get("denominator").and_then(|v| v.to_int().ok()).unwrap_or(1);
                return Some((n, d));
            }
        }
        if let PyObjectPayload::Int(n) = &obj.payload {
            return Some((n.to_i64().unwrap_or(0), 1));
        }
        None
    }

    fn make_frac_instance(num: i64, den: i64) -> PyObjectRef {
        let g = frac_gcd(num.abs(), den.abs());
        let (num, den) = if den < 0 { (-num / g, -den / g) } else { (num / g, den / g) };
        let mut frac_ns = IndexMap::new();
        frac_ns.insert(CompactString::from("__add__"), make_builtin(frac_add));
        frac_ns.insert(CompactString::from("__radd__"), make_builtin(frac_add));
        frac_ns.insert(CompactString::from("__sub__"), make_builtin(frac_sub));
        frac_ns.insert(CompactString::from("__rsub__"), make_builtin(frac_rsub));
        frac_ns.insert(CompactString::from("__mul__"), make_builtin(frac_mul));
        frac_ns.insert(CompactString::from("__rmul__"), make_builtin(frac_mul));
        frac_ns.insert(CompactString::from("__truediv__"), make_builtin(frac_div));
        frac_ns.insert(CompactString::from("__floordiv__"), make_builtin(frac_floordiv));
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
        frac_ns.insert(CompactString::from("limit_denominator"), make_builtin(frac_limit_denominator));
        frac_ns.insert(CompactString::from("__pow__"), make_builtin(frac_pow));
        frac_ns.insert(CompactString::from("__mod__"), make_builtin(frac_mod));
        frac_ns.insert(CompactString::from("__rtruediv__"), make_builtin(frac_rtruediv));
        frac_ns.insert(CompactString::from("__rfloordiv__"), make_builtin(frac_rfloordiv));
        frac_ns.insert(CompactString::from("__format__"), make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("0"))); }
            let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
            let spec = args.get(1).map(|a| a.py_to_string()).unwrap_or_default();
            if spec.is_empty() || spec == "s" {
                if d == 1 { return Ok(PyObject::str_val(CompactString::from(format!("{}", n)))); }
                return Ok(PyObject::str_val(CompactString::from(format!("{}/{}", n, d))));
            }
            // For numeric format specs, convert to float
            let f = n as f64 / d as f64;
            Ok(PyObject::str_val(CompactString::from(format!("{}", f))))
        }));
        frac_ns.insert(CompactString::from("as_integer_ratio"), make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::tuple(vec![PyObject::int(0), PyObject::int(1)])); }
            let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
            Ok(PyObject::tuple(vec![PyObject::int(n), PyObject::int(d)]))
        }));
        let class = PyObject::class(CompactString::from("Fraction"), vec![], frac_ns);
        let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
            class,
            attrs: Arc::new(RwLock::new(IndexMap::new())),
            is_special: true, dict_storage: None,
        }));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut w = inst_data.attrs.write();
            w.insert(CompactString::from("__fraction__"), PyObject::bool_val(true));
            w.insert(CompactString::from("numerator"), PyObject::int(num));
            w.insert(CompactString::from("denominator"), PyObject::int(den));
        }
        inst
    }

    fn frac_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("Fraction.__add__ requires 2 args")); }
        let (an, ad) = get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_parts(&args[1]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_instance(an * bd + bn * ad, ad * bd))
    }

    fn frac_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("Fraction.__sub__ requires 2 args")); }
        let (an, ad) = get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_parts(&args[1]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_instance(an * bd - bn * ad, ad * bd))
    }

    fn frac_rsub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("requires 2 args")); }
        let (an, ad) = get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_parts(&args[1]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_instance(bn * ad - an * bd, ad * bd))
    }

    fn frac_mul(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("Fraction.__mul__ requires 2 args")); }
        let (an, ad) = get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_parts(&args[1]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_instance(an * bn, ad * bd))
    }

    fn frac_div(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("Fraction.__truediv__ requires 2 args")); }
        let (an, ad) = get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_parts(&args[1]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if bn == 0 { return Err(PyException::zero_division_error("Fraction division by zero")); }
        Ok(make_frac_instance(an * bd, ad * bn))
    }

    fn frac_floordiv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("requires 2 args")); }
        let (an, ad) = get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        let (bn, bd) = get_frac_parts(&args[1]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        if bn == 0 { return Err(PyException::zero_division_error("Fraction division by zero")); }
        let result = (an * bd).div_euclid(ad * bn);
        Ok(PyObject::int(result))
    }

    fn frac_neg(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_instance(-n, d))
    }

    fn frac_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).ok_or_else(|| PyException::type_error("not a Fraction"))?;
        Ok(make_frac_instance(n.abs(), d))
    }

    fn frac_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let a = get_frac_parts(&args[0]);
        let b = get_frac_parts(&args[1]);
        match (a, b) {
            (Some((an, ad)), Some((bn, bd))) => Ok(PyObject::bool_val(an * bd == bn * ad)),
            _ => Ok(PyObject::bool_val(false)),
        }
    }

    fn frac_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((0, 1));
        Ok(PyObject::bool_val(an * bd < bn * ad))
    }

    fn frac_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((0, 1));
        Ok(PyObject::bool_val(an * bd <= bn * ad))
    }

    fn frac_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((0, 1));
        Ok(PyObject::bool_val(an * bd > bn * ad))
    }

    fn frac_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((0, 1));
        Ok(PyObject::bool_val(an * bd >= bn * ad))
    }

    fn frac_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::int(n.wrapping_mul(31).wrapping_add(d)))
    }

    fn frac_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let s = if d == 1 { format!("{}", n) } else { format!("{}/{}", n, d) };
        Ok(PyObject::str_val(CompactString::from(s)))
    }

    fn frac_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::str_val(CompactString::from(format!("Fraction({}, {})", n, d))))
    }

    fn frac_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        Ok(PyObject::float(n as f64 / d as f64))
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
        let max_den = if args.len() > 1 { args[1].to_int().unwrap_or(1_000_000) } else { 1_000_000 };
        if d <= max_den { return Ok(make_frac_instance(n, d)); }
        // Stern-Brocot tree convergent search
        let f = n as f64 / d as f64;
        let mut p0: i64 = 0; let mut q0: i64 = 1;
        let mut p1: i64 = 1; let mut q1: i64 = 0;
        loop {
            let a = ((f * q0 as f64 - p0 as f64) / (p1 as f64 - f * q1 as f64)) as i64;
            let p2 = p0 + a * p1;
            let q2 = q0 + a * q1;
            if q2 > max_den { break; }
            p0 = p1; q0 = q1; p1 = p2; q1 = q2;
        }
        // Choose closest between p0/q0 and p1/q1
        let err0 = (f - p0 as f64 / q0 as f64).abs();
        let err1 = (f - p1 as f64 / q1 as f64).abs();
        if err0 <= err1 { Ok(make_frac_instance(p0, q0)) }
        else { Ok(make_frac_instance(p1, q1)) }
    }

    fn frac_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("Fraction.__pow__ requires 2 args")); }
        let (n, d) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let exp = args[1].to_int().unwrap_or(1);
        if exp >= 0 {
            let e = exp as u32;
            Ok(make_frac_instance(n.pow(e), d.pow(e)))
        } else {
            let e = (-exp) as u32;
            if n == 0 { return Err(PyException::zero_division_error("Fraction division by zero")); }
            Ok(make_frac_instance(d.pow(e), n.pow(e)))
        }
    }

    fn frac_mod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("Fraction.__mod__ requires 2 args")); }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((1, 1));
        if bn == 0 { return Err(PyException::zero_division_error("Fraction modulo by zero")); }
        // a % b = a - b * floor(a/b)
        let num = an * bd;
        let den = ad * bn;
        let floor_div = if den > 0 { num.div_euclid(den) } else { -((-num).div_euclid(-den)) };
        let result_n = an * bd * bd - floor_div * bn * ad * bd;
        let result_d = ad * bd * bd;
        Ok(make_frac_instance(result_n, result_d))
    }

    fn frac_rtruediv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("requires 2 args")); }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((1, 1));
        if an == 0 { return Err(PyException::zero_division_error("Fraction division by zero")); }
        Ok(make_frac_instance(bn * ad, bd * an))
    }

    fn frac_rfloordiv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("requires 2 args")); }
        let (an, ad) = get_frac_parts(&args[0]).unwrap_or((0, 1));
        let (bn, bd) = get_frac_parts(&args[1]).unwrap_or((1, 1));
        if an == 0 { return Err(PyException::zero_division_error("Fraction division by zero")); }
        let num = bn * ad;
        let den = bd * an;
        let result = if (num < 0) ^ (den < 0) { -((-num).abs() / den.abs()) - if num.abs() % den.abs() != 0 { 1 } else { 0 } }
        else { num / den };
        Ok(make_frac_instance(result, 1))
    }

    // Fraction as a module-like callable with class methods
    let fraction_from_float = make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("from_float requires 1 argument")); }
        let f = args[0].to_float()?;
        let (n, d) = float_to_fraction(f);
        Ok(make_frac_instance(n, d))
    });
    let fraction_from_decimal = make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("from_decimal requires 1 argument")); }
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
                if args.is_empty() { return Ok(make_frac_instance(0, 1)); }
                // Skip cls argument if present (class object)
                let real_args = if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                    &args[1..]
                } else {
                    args
                };
                if real_args.is_empty() { return Ok(make_frac_instance(0, 1)); }
                if real_args.len() == 1 {
                    match &real_args[0].payload {
                        PyObjectPayload::Int(n) => return Ok(make_frac_instance(n.to_i64().unwrap_or(0), 1)),
                        PyObjectPayload::Float(f) => {
                            let (n, d) = float_to_fraction(*f);
                            return Ok(make_frac_instance(n, d));
                        }
                        PyObjectPayload::Str(s) => {
                            if let Some((n_str, d_str)) = s.split_once('/') {
                                let n: i64 = n_str.trim().parse().map_err(|_| PyException::value_error("Invalid fraction string"))?;
                                let d: i64 = d_str.trim().parse().map_err(|_| PyException::value_error("Invalid fraction string"))?;
                                if d == 0 { return Err(PyException::new(ferrython_core::error::ExceptionKind::ZeroDivisionError, "Fraction(_, 0)")); }
                                return Ok(make_frac_instance(n, d));
                            } else {
                                let n: i64 = s.trim().parse().map_err(|_| PyException::value_error("Invalid fraction string"))?;
                                return Ok(make_frac_instance(n, 1));
                            }
                        }
                        _ => {
                            if let Some((n, d)) = get_frac_parts(&real_args[0]) {
                                return Ok(make_frac_instance(n, d));
                            }
                            return Err(PyException::type_error("Fraction() argument must be int, float, or str"));
                        }
                    }
                }
                let n = real_args[0].to_int()?;
                let d = real_args[1].to_int()?;
                if d == 0 { return Err(PyException::new(ferrython_core::error::ExceptionKind::ZeroDivisionError, "Fraction(_, 0)")); }
                Ok(make_frac_instance(n, d))
            }),
        );
    }

    make_module("fractions", vec![
        ("Fraction", frac_class),
        ("gcd", make_builtin(fraction_gcd)),
    ])
}

fn float_to_fraction(f: f64) -> (i64, i64) {
    if f == 0.0 { return (0, 1); }
    // Use continued fraction approximation
    let sign = if f < 0.0 { -1i64 } else { 1 };
    let mut x = f.abs();
    let mut p0: i64 = 0; let mut q0: i64 = 1;
    let mut p1: i64 = 1; let mut q1: i64 = 0;
    for _ in 0..64 {
        let a = x as i64;
        let p2 = a * p1 + p0;
        let q2 = a * q1 + q0;
        if q2 > 1_000_000_000 { break; }
        p0 = p1; q0 = q1;
        p1 = p2; q1 = q2;
        let frac = x - a as f64;
        if frac.abs() < 1e-15 { break; }
        x = 1.0 / frac;
    }
    (sign * p1, q1)
}

fn fraction_gcd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("gcd", args, 2)?;
    let mut a = args[0].to_int()?.abs();
    let mut b = args[1].to_int()?.abs();
    while b != 0 { let t = b; b = a % b; a = t; }
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
    make_module("cmath", vec![
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
    ])
}

fn cmath_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.sqrt", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    if im == 0.0 && re < 0.0 {
        return Ok(PyObject::complex(0.0, (-re).sqrt()));
    }
    let r = (re * re + im * im).sqrt();
    let out_re = ((r + re) / 2.0).sqrt();
    let out_im = if im < 0.0 { -((r - re) / 2.0).sqrt() } else { ((r - re) / 2.0).sqrt() };
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
        return Err(PyException::type_error("cmath.log requires at least 1 argument"));
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
    Ok(PyObject::complex(re.sin() * im.cosh(), re.cos() * im.sinh()))
}

fn cmath_cos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.cos", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    Ok(PyObject::complex(re.cos() * im.cosh(), -(re.sin() * im.sinh())))
}

fn cmath_tan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.tan", args, 1)?;
    let (re, im) = to_complex(&args[0]);
    let denom = (2.0 * re).cos() + (2.0 * im).cosh();
    if denom == 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::complex((2.0 * re).sin() / denom, (2.0 * im).sinh() / denom))
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
    Ok(PyObject::tuple(vec![PyObject::float(r), PyObject::float(phi)]))
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
