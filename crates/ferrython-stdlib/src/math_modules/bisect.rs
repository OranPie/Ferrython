use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    call_callable, make_module, CompareOp, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

// ── bisect module ──

pub fn create_bisect_module() -> PyObjectRef {
    create_bisect_module_named("bisect")
}

pub fn create_bisect_accel_module() -> PyObjectRef {
    create_bisect_module_named("_bisect")
}

fn bisect_function(
    module: &str,
    name: &str,
    func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>,
) -> PyObjectRef {
    PyObject::native_function(&format!("{module}.{name}"), func)
}

fn create_bisect_module_named(module: &str) -> PyObjectRef {
    let bisect_right_obj = bisect_function(module, "bisect_right", bisect_right);
    let insort_right_obj = bisect_function(module, "insort_right", insort_right);
    make_module(
        module,
        vec![
            (
                "bisect_left",
                bisect_function(module, "bisect_left", bisect_left),
            ),
            ("bisect_right", bisect_right_obj.clone()),
            ("bisect", bisect_right_obj),
            (
                "insort_left",
                bisect_function(module, "insort_left", insort_left),
            ),
            ("insort_right", insort_right_obj.clone()),
            ("insort", insort_right_obj),
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
