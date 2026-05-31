use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::float_as_integer_ratio;

pub(super) fn int_builtin_value(arg: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    match &arg.payload {
        PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => Ok(Some(arg.clone())),
        PyObjectPayload::Float(f) => {
            if f.is_nan() {
                return Err(PyException::value_error(
                    "cannot convert float NaN to integer",
                ));
            }
            if f.is_infinite() {
                return Err(PyException::overflow_error(
                    "cannot convert float infinity to integer",
                ));
            }
            let truncated = f.trunc();
            if truncated >= -9_007_199_254_740_992.0 && truncated <= 9_007_199_254_740_992.0 {
                Ok(Some(PyObject::int(truncated as i64)))
            } else {
                let (n, d) = float_as_integer_ratio(truncated);
                Ok(Some(PyObject::big_int(n / d)))
            }
        }
        PyObjectPayload::Str(s) => Ok(s.trim().parse::<i64>().ok().map(PyObject::int)),
        _ => Ok(None),
    }
}

pub(super) fn float_builtin_value(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Float(_) => Some(arg.clone()),
        PyObjectPayload::Int(n) => Some(PyObject::float(n.to_f64())),
        PyObjectPayload::Bool(b) => Some(PyObject::float(if *b { 1.0 } else { 0.0 })),
        PyObjectPayload::Str(s) => s.trim().parse::<f64>().ok().map(PyObject::float),
        _ => None,
    }
}

pub(super) fn complex_builtin_value(pos_args: &[PyObjectRef]) -> Option<PyObjectRef> {
    let to_ri = |obj: &PyObjectRef| -> Option<(f64, f64)> {
        match &obj.payload {
            PyObjectPayload::Complex { real, imag } => Some((*real, *imag)),
            PyObjectPayload::Int(n) => Some((n.to_f64(), 0.0)),
            PyObjectPayload::Float(f) => Some((*f, 0.0)),
            PyObjectPayload::Bool(b) => Some((if *b { 1.0 } else { 0.0 }, 0.0)),
            _ => None,
        }
    };

    if pos_args.len() >= 2 {
        match (to_ri(&pos_args[0]), to_ri(&pos_args[1])) {
            (Some((ar, ai)), Some((br, bi))) => {
                let a_c = matches!(&pos_args[0].payload, PyObjectPayload::Complex { .. });
                let b_c = matches!(&pos_args[1].payload, PyObjectPayload::Complex { .. });
                let r = if b_c { ar - bi } else { ar };
                let i = if a_c { ai + br } else { br };
                Some(PyObject::complex(r, i))
            }
            _ => None,
        }
    } else {
        match &pos_args[0].payload {
            PyObjectPayload::Complex { .. } => Some(pos_args[0].clone()),
            PyObjectPayload::Int(n) => Some(PyObject::complex(n.to_f64(), 0.0)),
            PyObjectPayload::Float(f) => Some(PyObject::complex(*f, 0.0)),
            PyObjectPayload::Bool(b) => Some(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0)),
            _ => None,
        }
    }
}
