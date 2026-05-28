use super::super::number::{
    float_log2_exact_power, float_to_integral_object, is_odd_integer_float, math_ln_arg,
    math_number_to_float, pyint_log2,
};
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{check_args, PyObject, PyObjectPayload, PyObjectRef};

pub(super) fn math_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sqrt", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if x < 0.0 {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.sqrt()))
}

pub(super) fn math_ceil(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.ceil", args, 1)?;
    float_to_integral_object(math_number_to_float(&args[0])?, "math.ceil", f64::ceil)
}
pub(super) fn math_floor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.floor", args, 1)?;
    float_to_integral_object(math_number_to_float(&args[0])?, "math.floor", f64::floor)
}
pub(super) fn math_fabs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fabs", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.abs()))
}
pub(super) fn math_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
pub(super) fn math_log(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
pub(super) fn math_log2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
pub(super) fn math_log10(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log10", args, 1)?;
    Ok(PyObject::float(
        math_ln_arg(&args[0])? / std::f64::consts::LN_10,
    ))
}
pub(super) fn math_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
pub(super) fn math_sin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sin", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.sin()))
}
pub(super) fn math_cos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.cos", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.cos()))
}
pub(super) fn math_tan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.tan", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.tan()))
}
pub(super) fn math_asin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.asin", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if !x.is_nan() && !(-1.0..=1.0).contains(&x) {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.asin()))
}
pub(super) fn math_acos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.acos", args, 1)?;
    let x = math_number_to_float(&args[0])?;
    if !x.is_nan() && !(-1.0..=1.0).contains(&x) {
        return Err(PyException::value_error("math domain error"));
    }
    Ok(PyObject::float(x.acos()))
}
pub(super) fn math_atan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan", args, 1)?;
    Ok(PyObject::float(math_number_to_float(&args[0])?.atan()))
}
pub(super) fn math_atan2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan2", args, 2)?;
    Ok(PyObject::float(
        math_number_to_float(&args[0])?.atan2(math_number_to_float(&args[1])?),
    ))
}
pub(super) fn math_degrees(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.degrees", args, 1)?;
    Ok(PyObject::float(
        math_number_to_float(&args[0])?.to_degrees(),
    ))
}
pub(super) fn math_radians(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.radians", args, 1)?;
    Ok(PyObject::float(
        math_number_to_float(&args[0])?.to_radians(),
    ))
}
pub(super) fn math_isnan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isnan", args, 1)?;
    Ok(PyObject::bool_val(math_number_to_float(&args[0])?.is_nan()))
}
pub(super) fn math_isinf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isinf", args, 1)?;
    Ok(PyObject::bool_val(
        math_number_to_float(&args[0])?.is_infinite(),
    ))
}
pub(super) fn math_isfinite(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isfinite", args, 1)?;
    Ok(PyObject::bool_val(
        math_number_to_float(&args[0])?.is_finite(),
    ))
}
