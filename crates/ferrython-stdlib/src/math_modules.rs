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
    check_args("math.gcd", args, 2)?;
    let mut a = args[0].to_int()?.abs();
    let mut b = args[1].to_int()?.abs();
    while b != 0 { let t = b; b = a % b; a = t; }
    Ok(PyObject::int(a))
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
    // Number — base class with __number__ marker
    let mut number_ns = IndexMap::new();
    number_ns.insert(CompactString::from("__number__"), PyObject::bool_val(true));
    let number_class = PyObject::class(
        CompactString::from("Number"),
        vec![],
        number_ns,
    );

    // Complex — subclass of Number with __complex__ marker
    let mut complex_ns = IndexMap::new();
    complex_ns.insert(CompactString::from("__complex__"), PyObject::bool_val(true));
    complex_ns.insert(CompactString::from("__number__"), PyObject::bool_val(true));
    let complex_class = PyObject::class(
        CompactString::from("Complex"),
        vec![number_class.clone()],
        complex_ns,
    );

    // Real — subclass of Complex with __real__ marker
    let mut real_ns = IndexMap::new();
    real_ns.insert(CompactString::from("__real__"), PyObject::bool_val(true));
    real_ns.insert(CompactString::from("__complex__"), PyObject::bool_val(true));
    real_ns.insert(CompactString::from("__number__"), PyObject::bool_val(true));
    let real_class = PyObject::class(
        CompactString::from("Real"),
        vec![complex_class.clone()],
        real_ns,
    );

    // Rational — subclass of Real with __rational__ marker
    let mut rational_ns = IndexMap::new();
    rational_ns.insert(CompactString::from("__rational__"), PyObject::bool_val(true));
    rational_ns.insert(CompactString::from("__real__"), PyObject::bool_val(true));
    rational_ns.insert(CompactString::from("__complex__"), PyObject::bool_val(true));
    rational_ns.insert(CompactString::from("__number__"), PyObject::bool_val(true));
    let rational_class = PyObject::class(
        CompactString::from("Rational"),
        vec![real_class.clone()],
        rational_ns,
    );

    // Integral — subclass of Rational with __integral__ marker
    let mut integral_ns = IndexMap::new();
    integral_ns.insert(CompactString::from("__integral__"), PyObject::bool_val(true));
    integral_ns.insert(CompactString::from("__rational__"), PyObject::bool_val(true));
    integral_ns.insert(CompactString::from("__real__"), PyObject::bool_val(true));
    integral_ns.insert(CompactString::from("__complex__"), PyObject::bool_val(true));
    integral_ns.insert(CompactString::from("__number__"), PyObject::bool_val(true));
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

    fn make_decimal(s: &str) -> PyObjectRef {
        let mut dec_ns = IndexMap::new();
        dec_ns.insert(CompactString::from("__add__"), make_builtin(decimal_add));
        dec_ns.insert(CompactString::from("__radd__"), make_builtin(decimal_add));
        dec_ns.insert(CompactString::from("__sub__"), make_builtin(decimal_sub));
        dec_ns.insert(CompactString::from("__mul__"), make_builtin(decimal_mul));
        dec_ns.insert(CompactString::from("__truediv__"), make_builtin(decimal_div));
        // __repr__ and __str__ handled by py_to_string via __decimal__ marker
        dec_ns.insert(CompactString::from("__eq__"), make_builtin(decimal_eq));
        dec_ns.insert(CompactString::from("__lt__"), make_builtin(decimal_lt));
        dec_ns.insert(CompactString::from("__float__"), make_builtin(decimal_float));
        dec_ns.insert(CompactString::from("__int__"), make_builtin(decimal_int));
        dec_ns.insert(CompactString::from("__neg__"), make_builtin(decimal_neg));
        dec_ns.insert(CompactString::from("__abs__"), make_builtin(decimal_abs));
        let class = PyObject::class(CompactString::from("Decimal"), vec![], dec_ns);
        let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
            class,
            attrs: Arc::new(RwLock::new(IndexMap::new())),
            dict_storage: None,
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
        if scale == 0 {
            if neg && digits != 0 { format!("-{}", digits) } else { format!("{}", digits) }
        } else {
            let s = format!("{:0>width$}", digits, width = scale as usize + 1);
            let (int_part, frac_part) = s.split_at(s.len() - scale as usize);
            let frac_trimmed = frac_part.trim_end_matches('0');
            if frac_trimmed.is_empty() {
                if neg && digits != 0 { format!("-{}", int_part) } else { int_part.to_string() }
            } else {
                if neg && digits != 0 { format!("-{}.{}", int_part, frac_trimmed) } else { format!("{}.{}", int_part, frac_trimmed) }
            }
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
        let precision = 28u32;
        let a_scaled = a.1 * 10i128.pow(precision);
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

    make_module("decimal", vec![
        ("Decimal", make_builtin(|args| {
            if args.is_empty() { return Ok(make_decimal("0")); }
            let s = args[0].py_to_string();
            let trimmed = s.trim();
            if trimmed.is_empty() { return Ok(make_decimal("0")); }
            match &args[0].payload {
                PyObjectPayload::Int(n) => return Ok(make_decimal(&format!("{}", n.to_i64().unwrap_or(0)))),
                PyObjectPayload::Float(f) => return Ok(make_decimal(&format!("{}", f))),
                _ => {}
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
        })),
        ("ROUND_HALF_UP", PyObject::str_val(CompactString::from("ROUND_HALF_UP"))),
        ("ROUND_HALF_DOWN", PyObject::str_val(CompactString::from("ROUND_HALF_DOWN"))),
        ("ROUND_CEILING", PyObject::str_val(CompactString::from("ROUND_CEILING"))),
        ("ROUND_FLOOR", PyObject::str_val(CompactString::from("ROUND_FLOOR"))),
        ("getcontext", make_builtin(|_| Ok(PyObject::none()))),
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
            let k = if args.len() > 1 { args[1].to_int()? as usize } else { 1 };
            let mut result = Vec::with_capacity(k);
            for _ in 0..k {
                let idx = (simple_random() * items.len() as f64) as usize;
                result.push(items[idx.min(items.len()-1)].clone());
            }
            Ok(PyObject::list(result))
        })),
    ])
}

fn simple_random() -> f64 {
    use std::time::SystemTime;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cnt = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().subsec_nanos() as u64;
    let seed = nanos.wrapping_mul(6364136223846793005).wrapping_add(cnt.wrapping_mul(1442695040888963407));
    (seed >> 11) as f64 / (1u64 << 53) as f64
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
    // Simplified in-place shuffle
    Ok(PyObject::none())
}
fn random_seed(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
    make_module("fractions", vec![
        ("Fraction", make_builtin(fraction_new)),
        ("gcd", make_builtin(fraction_gcd)),
    ])
}

fn fraction_gcd_val(mut a: i64, mut b: i64) -> i64 {
    a = a.abs(); b = b.abs();
    while b != 0 { let t = b; b = a % b; a = t; }
    a
}

fn fraction_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (num, den) = if args.is_empty() {
        (0i64, 1i64)
    } else if args.len() == 1 {
        match &args[0].payload {
            PyObjectPayload::Int(n) => (n.to_i64().unwrap_or(0), 1),
            PyObjectPayload::Float(f) => {
                // Convert float to fraction via limiting denominator
                let (n, d) = float_to_fraction(*f);
                (n, d)
            }
            PyObjectPayload::Str(s) => {
                if let Some((n_str, d_str)) = s.split_once('/') {
                    let n: i64 = n_str.trim().parse().map_err(|_| PyException::value_error("Invalid fraction string"))?;
                    let d: i64 = d_str.trim().parse().map_err(|_| PyException::value_error("Invalid fraction string"))?;
                    if d == 0 { return Err(PyException::new(ferrython_core::error::ExceptionKind::ZeroDivisionError, "Fraction(_, 0)")); }
                    (n, d)
                } else {
                    let n: i64 = s.trim().parse().map_err(|_| PyException::value_error("Invalid fraction string"))?;
                    (n, 1)
                }
            }
            _ => return Err(PyException::type_error("Fraction() argument must be int, float, or str")),
        }
    } else {
        let n = args[0].to_int()?;
        let d = args[1].to_int()?;
        if d == 0 { return Err(PyException::new(ferrython_core::error::ExceptionKind::ZeroDivisionError, "Fraction(_, 0)")); }
        (n, d)
    };
    // Normalize
    let g = fraction_gcd_val(num, den);
    let (n, d) = if den < 0 { (-num / g, -den / g) } else { (num / g, den / g) };
    // Create instance with numerator/denominator attributes
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("numerator"), PyObject::int(n));
    attrs.insert(CompactString::from("denominator"), PyObject::int(d));
    attrs.insert(CompactString::from("__fraction__"), PyObject::bool_val(true));
    // Store as a tuple for easy access
    Ok(PyObject::instance_with_attrs(
        PyObject::str_val(CompactString::from("Fraction")),
        attrs,
    ))
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
    let a = args[0].to_int()?;
    let b = args[1].to_int()?;
    Ok(PyObject::int(fraction_gcd_val(a, b)))
}
