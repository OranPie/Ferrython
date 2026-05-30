use super::number::math_number_to_float;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    call_callable, check_args, make_builtin, make_module, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

// ── cmath module ─────────────────────────────────────────────────────

fn to_complex(obj: &PyObjectRef) -> PyResult<(f64, f64)> {
    match &obj.payload {
        PyObjectPayload::Complex { real, imag } => Ok((*real, *imag)),
        PyObjectPayload::Instance(inst) => {
            if let Some(method) = obj.get_attr("__complex__") {
                let result = call_callable(&method, &[])?;
                return match &result.payload {
                    PyObjectPayload::Complex { real, imag } => Ok((*real, *imag)),
                    PyObjectPayload::Instance(result_inst) => {
                        if let Some(value) =
                            result_inst.attrs.read().get("__builtin_value__").cloned()
                        {
                            if let PyObjectPayload::Complex { real, imag } = &value.payload {
                                return Ok((*real, *imag));
                            }
                        }
                        Err(PyException::type_error(format!(
                            "__complex__ returned non-complex (type {})",
                            result.type_name()
                        )))
                    }
                    _ => Err(PyException::type_error(format!(
                        "__complex__ returned non-complex (type {})",
                        result.type_name()
                    ))),
                };
            }
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                return to_complex(&value);
            }
            Ok((to_real_number(obj)?, 0.0))
        }
        _ => Ok((to_real_number(obj)?, 0.0)),
    }
}

fn to_real(obj: &PyObjectRef) -> PyResult<f64> {
    if matches!(&obj.payload, PyObjectPayload::Complex { .. }) {
        return Err(PyException::type_error("can't convert complex to float"));
    }
    to_real_number(obj)
}

fn to_real_number(obj: &PyObjectRef) -> PyResult<f64> {
    match math_number_to_float(obj) {
        Ok(value) => Ok(value),
        Err(err) => {
            if let PyObjectPayload::Instance(_) = &obj.payload {
                if let Some(method) = obj.get_attr("__index__") {
                    let result = call_callable(&method, &[])?;
                    return math_number_to_float(&result);
                }
            }
            Err(err)
        }
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
            ("log10", make_builtin(cmath_log10)),
            ("sin", make_builtin(cmath_sin)),
            ("cos", make_builtin(cmath_cos)),
            ("tan", make_builtin(cmath_tan)),
            ("sinh", make_builtin(cmath_sinh)),
            ("cosh", make_builtin(cmath_cosh)),
            ("tanh", make_builtin(cmath_tanh)),
            ("asin", make_builtin(cmath_asin)),
            ("acos", make_builtin(cmath_acos)),
            ("atan", make_builtin(cmath_atan)),
            ("asinh", make_builtin(cmath_asinh)),
            ("acosh", make_builtin(cmath_acosh)),
            ("atanh", make_builtin(cmath_atanh)),
            ("phase", make_builtin(cmath_phase)),
            ("polar", make_builtin(cmath_polar)),
            ("rect", make_builtin(cmath_rect)),
            ("isnan", make_builtin(cmath_isnan)),
            ("isinf", make_builtin(cmath_isinf)),
            ("isfinite", make_builtin(cmath_isfinite)),
            ("isclose", make_builtin(cmath_isclose)),
        ],
    )
}

fn c_sub(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 - b.0, a.1 - b.1)
}

fn c_mul(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

fn c_div(a: (f64, f64), b: (f64, f64)) -> PyResult<(f64, f64)> {
    let denom = b.0 * b.0 + b.1 * b.1;
    if denom == 0.0 {
        return Err(PyException::zero_division_error("complex division by zero"));
    }
    let real_num = a.0 * b.0 + a.1 * b.1;
    let imag_num = a.1 * b.0 - a.0 * b.1;
    let real = if real_num == 0.0 && a.0 == 0.0 && a.1 == 0.0 {
        0.0f64.copysign(b.0)
    } else {
        real_num / denom
    };
    let imag = if imag_num == 0.0 && a.0 == 0.0 && a.1 == 0.0 {
        0.0f64.copysign(b.0)
    } else {
        imag_num / denom
    };
    Ok((real, imag))
}

fn c_sqrt_pair(re: f64, im: f64) -> (f64, f64) {
    if re == 0.0 && im == 0.0 {
        return (0.0, im);
    }
    let r = re.hypot(im);
    let out_re = ((r + re) / 2.0).sqrt();
    let out_im = ((r - re) / 2.0).sqrt().copysign(im);
    (out_re, out_im)
}

fn c_log_pair(re: f64, im: f64) -> PyResult<(f64, f64)> {
    if re == 0.0 && im == 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok((re.hypot(im).ln(), im.atan2(re)))
}

fn cmath_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.sqrt", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    let (out_re, out_im) = c_sqrt_pair(re, im);
    Ok(PyObject::complex(out_re, out_im))
}

fn cmath_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.exp", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    let e_re = re.exp();
    if e_re.is_infinite() && re.is_finite() {
        return Err(PyException::overflow_error("math range error"));
    }
    Ok(PyObject::complex(e_re * im.cos(), e_re * im.sin()))
}

fn cmath_log(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "cmath.log requires at least 1 argument",
        ));
    }
    if args.len() > 2 {
        return Err(PyException::type_error(
            "cmath.log expected at most 2 arguments",
        ));
    }
    let (re, im) = to_complex(&args[0])?;
    let (ln_re, ln_im) = c_log_pair(re, im)?;
    if args.len() > 1 {
        let (bre, bim) = to_complex(&args[1])?;
        let base_log = c_log_pair(bre, bim)?;
        let (out_re, out_im) = c_div((ln_re, ln_im), base_log)?;
        Ok(PyObject::complex(out_re, out_im))
    } else {
        Ok(PyObject::complex(ln_re, ln_im))
    }
}

fn cmath_log10(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.log10", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    let (ln_re, ln_im) = c_log_pair(re, im)?;
    Ok(PyObject::complex(
        ln_re / std::f64::consts::LN_10,
        ln_im / std::f64::consts::LN_10,
    ))
}

fn cmath_sin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.sin", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    Ok(PyObject::complex(
        re.sin() * im.cosh(),
        re.cos() * im.sinh(),
    ))
}

fn cmath_cos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.cos", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    Ok(PyObject::complex(
        re.cos() * im.cosh(),
        -(re.sin() * im.sinh()),
    ))
}

fn cmath_tan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.tan", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    let denom = (2.0 * re).cos() + (2.0 * im).cosh();
    if denom == 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::complex(
        (2.0 * re).sin() / denom,
        (2.0 * im).sinh() / denom,
    ))
}

fn cmath_sinh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.sinh", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    Ok(PyObject::complex(
        re.sinh() * im.cos(),
        re.cosh() * im.sin(),
    ))
}

fn cmath_cosh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.cosh", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    Ok(PyObject::complex(
        re.cosh() * im.cos(),
        re.sinh() * im.sin(),
    ))
}

fn cmath_tanh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.tanh", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    if re == 0.0 && im == 0.0 {
        return Ok(PyObject::complex(re, im));
    }
    let denom = (2.0 * re).cosh() + (2.0 * im).cos();
    if denom == 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::complex(
        (2.0 * re).sinh() / denom,
        (2.0 * im).sin() / denom,
    ))
}

fn cmath_asin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.asin", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    if im == 0.0 && (-1.0..=1.0).contains(&re) {
        return Ok(PyObject::complex(re.asin(), im));
    }
    let z2 = c_mul((re, im), (re, im));
    let root = c_sqrt_pair(1.0 - z2.0, -z2.1);
    let logged = c_log_pair(root.0 - im, root.1 + re)?;
    Ok(PyObject::complex(logged.1, -logged.0))
}

fn cmath_acos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.acos", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    if im == 0.0 && (-1.0..=1.0).contains(&re) {
        return Ok(PyObject::complex(re.acos(), -im));
    }
    let asin = match cmath_asin(args)?.payload {
        PyObjectPayload::Complex { real, imag } => (real, imag),
        _ => unreachable!(),
    };
    Ok(PyObject::complex(
        std::f64::consts::FRAC_PI_2 - asin.0,
        -asin.1,
    ))
}

fn cmath_atan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.atan", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    if re == 0.0 && im == 0.0 {
        return Ok(PyObject::complex(re, im));
    }
    let left = c_log_pair(1.0 - im, re)?;
    let right = c_log_pair(1.0 + im, -re)?;
    let diff = c_sub(left, right);
    Ok(PyObject::complex(diff.1 / 2.0, -diff.0 / 2.0))
}

fn cmath_asinh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.asinh", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    if re == 0.0 && im == 0.0 {
        return Ok(PyObject::complex(re, im));
    }
    let z2 = c_mul((re, im), (re, im));
    let root = c_sqrt_pair(z2.0 + 1.0, z2.1);
    let logged = c_log_pair(re + root.0, im + root.1)?;
    Ok(PyObject::complex(logged.0, logged.1))
}

fn cmath_acosh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.acosh", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    let root1 = c_sqrt_pair(re + 1.0, im);
    let root2 = c_sqrt_pair(re - 1.0, im);
    let product = c_mul(root1, root2);
    let logged = c_log_pair(re + product.0, im + product.1)?;
    Ok(PyObject::complex(logged.0, logged.1))
}

fn cmath_atanh(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.atanh", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    if re == 0.0 && im == 0.0 {
        return Ok(PyObject::complex(re, im));
    }
    let left = c_log_pair(1.0 + re, im)?;
    let right = c_log_pair(1.0 - re, -im)?;
    let diff = c_sub(left, right);
    Ok(PyObject::complex(diff.0 / 2.0, diff.1 / 2.0))
}

fn cmath_phase(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.phase", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    Ok(PyObject::float(im.atan2(re)))
}

fn cmath_polar(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.polar", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    let r = re.hypot(im);
    let phi = im.atan2(re);
    Ok(PyObject::tuple(vec![
        PyObject::float(r),
        PyObject::float(phi),
    ]))
}

fn cmath_rect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.rect", args, 2)?;
    let r = to_real(&args[0])?;
    let phi = to_real(&args[1])?;
    Ok(PyObject::complex(r * phi.cos(), r * phi.sin()))
}

fn cmath_isnan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.isnan", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    Ok(PyObject::bool_val(re.is_nan() || im.is_nan()))
}

fn cmath_isinf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.isinf", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    Ok(PyObject::bool_val(re.is_infinite() || im.is_infinite()))
}

fn cmath_isfinite(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("cmath.isfinite", args, 1)?;
    let (re, im) = to_complex(&args[0])?;
    Ok(PyObject::bool_val(re.is_finite() && im.is_finite()))
}

fn cmath_isclose(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "isclose() requires at least 2 arguments",
        ));
    }
    let a = to_complex(&args[0])?;
    let b = to_complex(&args[1])?;
    let mut rel_tol = 1e-9;
    let mut abs_tol = 0.0;
    let mut next_positional_tol = 0usize;

    for arg in &args[2..] {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            for (key, value) in d.read().iter() {
                if *key == HashableKey::str_key(CompactString::from("rel_tol")) {
                    rel_tol = to_real_tolerance(value)?;
                } else if *key == HashableKey::str_key(CompactString::from("abs_tol")) {
                    abs_tol = to_real_tolerance(value)?;
                } else if let HashableKey::Str(name) = key {
                    return Err(PyException::type_error(format!(
                        "'{}' is an invalid keyword argument for isclose()",
                        name
                    )));
                } else {
                    return Err(PyException::type_error(
                        "isclose() keywords must be strings",
                    ));
                }
            }
            continue;
        }
        match next_positional_tol {
            0 => rel_tol = to_real_tolerance(arg)?,
            1 => abs_tol = to_real_tolerance(arg)?,
            _ => {
                return Err(PyException::type_error(
                    "isclose() takes at most 4 arguments",
                ))
            }
        }
        next_positional_tol += 1;
    }

    if rel_tol < 0.0 || abs_tol < 0.0 {
        return Err(PyException::value_error("tolerances must be non-negative"));
    }
    if a == b {
        return Ok(PyObject::bool_val(true));
    }
    if a.0.is_infinite() || a.1.is_infinite() || b.0.is_infinite() || b.1.is_infinite() {
        return Ok(PyObject::bool_val(false));
    }
    let diff = (a.0 - b.0).hypot(a.1 - b.1);
    let a_abs = a.0.hypot(a.1);
    let b_abs = b.0.hypot(b.1);
    Ok(PyObject::bool_val(
        diff <= (rel_tol * a_abs.max(b_abs)).max(abs_tol),
    ))
}

fn to_real_tolerance(obj: &PyObjectRef) -> PyResult<f64> {
    if matches!(&obj.payload, PyObjectPayload::Complex { .. }) {
        return Err(PyException::type_error("tolerances must be real"));
    }
    to_real(obj)
}
