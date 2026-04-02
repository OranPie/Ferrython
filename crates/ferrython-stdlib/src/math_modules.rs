//! Math and statistics stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectRef,
    make_module, make_builtin, check_args,
};
use indexmap::IndexMap;

pub fn create_math_module() -> PyObjectRef {
    make_module("math", vec![
        ("pi", PyObject::float(std::f64::consts::PI)),
        ("e", PyObject::float(std::f64::consts::E)),
        ("tau", PyObject::float(std::f64::consts::TAU)),
        ("inf", PyObject::float(f64::INFINITY)),
        ("nan", PyObject::float(f64::NAN)),
        ("sqrt", make_builtin(math_sqrt)),
        ("ceil", make_builtin(math_ceil)),
        ("floor", make_builtin(math_floor)),
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
        ("trunc", make_builtin(math_trunc)),
        ("copysign", make_builtin(math_copysign)),
        ("hypot", make_builtin(math_hypot)),
        ("modf", make_builtin(math_modf)),
        ("fmod", make_builtin(math_fmod)),
        ("frexp", make_builtin(math_frexp)),
        ("ldexp", make_builtin(math_ldexp)),
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
    Ok(PyObject::float(args[0].to_float()?.exp()))
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

fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 { return (0.0, 0); }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1022;
    let mantissa = f64::from_bits((bits & 0x800FFFFFFFFFFFFF) | 0x3FE0000000000000);
    (mantissa, exp)
}

// ── sys module ──


pub fn create_statistics_module() -> PyObjectRef {
    make_module("statistics", vec![
        ("mean", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("mean requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.is_empty() { return Err(PyException::value_error("mean requires a non-empty dataset")); }
            let sum: f64 = items.iter().map(|x| x.to_float().unwrap_or(0.0)).sum();
            Ok(PyObject::float(sum / items.len() as f64))
        })),
        ("median", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("median requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.is_empty() { return Err(PyException::value_error("median requires a non-empty dataset")); }
            let mut vals: Vec<f64> = items.iter().map(|x| x.to_float().unwrap_or(0.0)).collect();
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = vals.len();
            if n % 2 == 1 { Ok(PyObject::float(vals[n / 2])) }
            else { Ok(PyObject::float((vals[n / 2 - 1] + vals[n / 2]) / 2.0)) }
        })),
        ("stdev", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("stdev requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.len() < 2 { return Err(PyException::value_error("stdev requires at least 2 data points")); }
            let vals: Vec<f64> = items.iter().map(|x| x.to_float().unwrap_or(0.0)).collect();
            let mean: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
            let variance: f64 = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (vals.len() - 1) as f64;
            Ok(PyObject::float(variance.sqrt()))
        })),
        ("variance", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("variance requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.len() < 2 { return Err(PyException::value_error("variance requires at least 2 data points")); }
            let vals: Vec<f64> = items.iter().map(|x| x.to_float().unwrap_or(0.0)).collect();
            let mean: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
            let variance: f64 = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (vals.len() - 1) as f64;
            Ok(PyObject::float(variance))
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
            let max = counts.values().max_by_key(|v| v.1).unwrap();
            Ok(max.0.clone())
        })),
    ])
}

// ── numbers module (stub) ──


pub fn create_numbers_module() -> PyObjectRef {
    make_module("numbers", vec![
        ("Number", PyObject::none()),
        ("Complex", PyObject::none()),
        ("Real", PyObject::none()),
        ("Rational", PyObject::none()),
        ("Integral", PyObject::none()),
    ])
}

// ── platform module ──


pub fn create_decimal_module() -> PyObjectRef {
    make_module("decimal", vec![
        ("Decimal", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::float(0.0)); }
            let s = args[0].py_to_string();
            match s.parse::<f64>() {
                Ok(f) => Ok(PyObject::float(f)),
                Err(_) => Err(PyException::value_error(format!("Invalid literal for Decimal: '{}'", s))),
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


