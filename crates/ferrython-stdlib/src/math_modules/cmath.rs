use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

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
