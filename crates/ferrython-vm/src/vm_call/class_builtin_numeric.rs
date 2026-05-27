use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};

pub(super) fn int_builtin_value(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => Some(arg.clone()),
        PyObjectPayload::Float(f) => Some(PyObject::int(*f as i64)),
        PyObjectPayload::Str(s) => s.trim().parse::<i64>().ok().map(PyObject::int),
        _ => None,
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
