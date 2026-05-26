use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};
use indexmap::IndexMap;

// ── statistics module ──

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
