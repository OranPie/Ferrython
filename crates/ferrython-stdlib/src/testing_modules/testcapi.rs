use ferrython_core::error::ExceptionKind;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use num_bigint::BigInt;

// ── _testcapi module ──

pub fn create_testcapi_module() -> PyObjectRef {
    make_module(
        "_testcapi",
        vec![
            ("INT_MIN", PyObject::int(i32::MIN as i64)),
            ("INT_MAX", PyObject::int(i32::MAX as i64)),
            ("UINT_MAX", PyObject::int(u32::MAX as i64)),
            ("LONG_MIN", PyObject::int(libc::c_long::MIN as i64)),
            ("LONG_MAX", PyObject::int(libc::c_long::MAX as i64)),
            (
                "ULONG_MAX",
                PyObject::big_int(BigInt::from(libc::c_ulong::MAX)),
            ),
            ("PY_SSIZE_T_MIN", PyObject::int(isize::MIN as i64)),
            ("PY_SSIZE_T_MAX", PyObject::int(isize::MAX as i64)),
            (
                "SIZEOF_TIME_T",
                PyObject::int(std::mem::size_of::<libc::time_t>() as i64),
            ),
            ("SIZEOF_PYGC_HEAD", PyObject::int(16)),
            ("PyTime_FromSeconds", make_builtin(pytime_from_seconds)),
            (
                "PyTime_FromSecondsObject",
                make_builtin(pytime_from_seconds_object),
            ),
            (
                "PyTime_AsSecondsDouble",
                make_builtin(pytime_as_seconds_double),
            ),
            ("PyTime_AsTimeval", make_builtin(pytime_as_timeval)),
            ("PyTime_AsTimespec", make_builtin(pytime_as_timespec)),
            (
                "PyTime_AsMilliseconds",
                make_builtin(pytime_as_milliseconds),
            ),
            (
                "PyTime_AsMicroseconds",
                make_builtin(pytime_as_microseconds),
            ),
            (
                "pytime_object_to_time_t",
                make_builtin(pytime_object_to_time_t),
            ),
            (
                "pytime_object_to_timeval",
                make_builtin(pytime_object_to_timeval),
            ),
            (
                "pytime_object_to_timespec",
                make_builtin(pytime_object_to_timespec),
            ),
        ],
    )
}

const SEC_TO_NS_I128: i128 = 1_000_000_000;
const SEC_TO_US_I128: i128 = 1_000_000;
const PYTIME_MIN: i128 = i64::MIN as i128;
const PYTIME_MAX: i128 = i64::MAX as i128;

#[derive(Clone, Copy)]
enum TimeRound {
    Floor,
    Ceiling,
    HalfEven,
    Up,
}

fn rounding_arg(args: &[PyObjectRef]) -> TimeRound {
    let value = args
        .get(1)
        .and_then(|v| {
            v.as_int()
                .or_else(|| v.get_attr("_value_").and_then(|inner| inner.as_int()))
                .or_else(|| v.get_attr("value").and_then(|inner| inner.as_int()))
        })
        .unwrap_or(2);
    match value {
        0 => TimeRound::Floor,
        1 => TimeRound::Ceiling,
        3 => TimeRound::Up,
        _ => TimeRound::HalfEven,
    }
}

fn arg_number(args: &[PyObjectRef], index: usize) -> PyResult<f64> {
    let value = args
        .get(index)
        .ok_or_else(|| PyException::type_error("missing argument"))?
        .to_float()?;
    if value.is_nan() {
        return Err(PyException::value_error("Invalid value NaN"));
    }
    Ok(value)
}

fn arg_i128(args: &[PyObjectRef], index: usize) -> PyResult<i128> {
    let obj = args
        .get(index)
        .ok_or_else(|| PyException::type_error("missing argument"))?;
    if matches!(&obj.payload, PyObjectPayload::Int(_)) {
        return obj
            .as_int()
            .map(|value| value as i128)
            .ok_or_else(|| PyException::overflow_error("timestamp out of range"));
    }
    let value = obj.to_float()?;
    if value.is_nan() {
        return Err(PyException::value_error("Invalid value NaN"));
    }
    Ok(value as i128)
}

fn pytime_arg_i128(args: &[PyObjectRef], index: usize) -> PyResult<i128> {
    let value = arg_i128(args, index).map_err(|exc| {
        if exc.kind == ExceptionKind::ValueError && exc.message == "Invalid value NaN" {
            PyException::type_error("Invalid value NaN")
        } else {
            exc
        }
    })?;
    checked_pytime(value)
}

fn exact_int_arg(args: &[PyObjectRef], index: usize) -> PyResult<Option<i128>> {
    let obj = args
        .get(index)
        .ok_or_else(|| PyException::type_error("missing argument"))?;
    if matches!(&obj.payload, PyObjectPayload::Int(_)) {
        return obj
            .as_int()
            .map(|value| Some(value as i128))
            .ok_or_else(|| PyException::overflow_error("timestamp out of range"));
    }
    Ok(None)
}

fn arg_number_with_nan(args: &[PyObjectRef], index: usize, kind: ExceptionKind) -> PyResult<f64> {
    let value = args
        .get(index)
        .ok_or_else(|| PyException::type_error("missing argument"))?
        .to_float()?;
    if value.is_nan() {
        return Err(PyException::new(kind, "Invalid value NaN"));
    }
    Ok(value)
}

fn round_float_to_i128(value: f64, mode: TimeRound) -> PyResult<i128> {
    if !value.is_finite() {
        return Err(PyException::overflow_error("timestamp out of range"));
    }
    let rounded = match mode {
        TimeRound::Floor => value.floor(),
        TimeRound::Ceiling => value.ceil(),
        TimeRound::Up => {
            if value >= 0.0 {
                value.ceil()
            } else {
                value.floor()
            }
        }
        TimeRound::HalfEven => round_half_even_f64(value),
    };
    Ok(rounded as i128)
}

fn round_half_even_f64(value: f64) -> f64 {
    let truncated = value.trunc();
    let frac = value - truncated;
    let abs_frac = frac.abs();
    if abs_frac < 0.5 {
        truncated
    } else if abs_frac > 0.5 {
        truncated + value.signum()
    } else if (truncated as i128) % 2 == 0 {
        truncated
    } else {
        truncated + value.signum()
    }
}

fn int_obj(value: i128) -> PyObjectRef {
    if let Ok(n) = i64::try_from(value) {
        PyObject::int(n)
    } else {
        PyObject::big_int(BigInt::from(value))
    }
}

fn pair_obj(first: i128, second: i128) -> PyObjectRef {
    PyObject::tuple(vec![int_obj(first), int_obj(second)])
}

fn checked_pytime(value: i128) -> PyResult<i128> {
    if !(PYTIME_MIN..=PYTIME_MAX).contains(&value) {
        Err(PyException::overflow_error(
            "timestamp too large to convert",
        ))
    } else {
        Ok(value)
    }
}

fn checked_time_t(value: i128) -> PyResult<i128> {
    let bits = std::mem::size_of::<libc::time_t>() * 8 - 1;
    let min = -(1i128 << bits);
    let max = (1i128 << bits) - 1;
    if value < min || value > max {
        Err(PyException::overflow_error("timestamp out of range"))
    } else {
        Ok(value)
    }
}

fn pytime_from_seconds(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(seconds) = exact_int_arg(args, 0)? {
        if seconds < i32::MIN as i128 || seconds > i32::MAX as i128 {
            return Err(PyException::overflow_error("timestamp out of range"));
        }
        return Ok(int_obj(seconds * SEC_TO_NS_I128));
    }
    let seconds = arg_number_with_nan(args, 0, ExceptionKind::TypeError)?;
    if seconds < i32::MIN as f64 || seconds > i32::MAX as f64 {
        return Err(PyException::overflow_error("timestamp out of range"));
    }
    Ok(int_obj(
        round_float_to_i128(seconds, TimeRound::HalfEven)? * SEC_TO_NS_I128,
    ))
}

fn pytime_from_seconds_object(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(seconds) = exact_int_arg(args, 0)? {
        return Ok(int_obj(checked_pytime(seconds * SEC_TO_NS_I128)?));
    }
    let seconds = arg_number(args, 0)?;
    let ns = round_float_to_i128(seconds * SEC_TO_NS_I128 as f64, rounding_arg(args))?;
    Ok(int_obj(checked_pytime(ns)?))
}

fn pytime_as_seconds_double(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let ns = pytime_arg_i128(args, 0)?;
    Ok(PyObject::float(ns as f64 / SEC_TO_NS_I128 as f64))
}

fn pytime_as_timeval(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let ns = pytime_arg_i128(args, 0)?;
    let us = round_div(ns, 1_000, rounding_arg(args));
    Ok(pair_obj(
        us.div_euclid(SEC_TO_US_I128),
        us.rem_euclid(SEC_TO_US_I128),
    ))
}

fn pytime_as_timespec(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let ns = pytime_arg_i128(args, 0)?;
    Ok(pair_obj(
        ns.div_euclid(SEC_TO_NS_I128),
        ns.rem_euclid(SEC_TO_NS_I128),
    ))
}

fn pytime_as_milliseconds(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let ns = pytime_arg_i128(args, 0)?;
    Ok(int_obj(round_div(ns, 1_000_000, rounding_arg(args))))
}

fn pytime_as_microseconds(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let ns = pytime_arg_i128(args, 0)?;
    Ok(int_obj(round_div(ns, 1_000, rounding_arg(args))))
}

fn pytime_object_to_time_t(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(seconds) = exact_int_arg(args, 0)? {
        return Ok(int_obj(checked_time_t(seconds)?));
    }
    Ok(int_obj(checked_time_t(round_float_to_i128(
        arg_number(args, 0)?,
        rounding_arg(args),
    )?)?))
}

fn pytime_object_to_timeval(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(seconds) = exact_int_arg(args, 0)? {
        return Ok(pair_obj(checked_time_t(seconds)?, 0));
    }
    let seconds = arg_number(args, 0)?;
    let (secs, fraction) = split_float_seconds(seconds, SEC_TO_US_I128, rounding_arg(args))?;
    Ok(pair_obj(checked_time_t(secs)?, fraction))
}

fn pytime_object_to_timespec(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(seconds) = exact_int_arg(args, 0)? {
        return Ok(pair_obj(checked_time_t(seconds)?, 0));
    }
    let seconds = arg_number(args, 0)?;
    let (secs, fraction) = split_float_seconds(seconds, SEC_TO_NS_I128, rounding_arg(args))?;
    Ok(pair_obj(checked_time_t(secs)?, fraction))
}

fn split_float_seconds(
    seconds: f64,
    units_per_sec: i128,
    mode: TimeRound,
) -> PyResult<(i128, i128)> {
    if !seconds.is_finite() {
        return Err(PyException::overflow_error("timestamp out of range"));
    }
    let intpart = seconds.trunc();
    let frac = seconds - intpart;
    let mut secs = intpart as i128;
    let mut fraction = round_float_to_i128(frac * units_per_sec as f64, mode)?;
    if fraction < 0 {
        fraction += units_per_sec;
        secs -= 1;
    } else if fraction >= units_per_sec {
        fraction -= units_per_sec;
        secs += 1;
    }
    Ok((secs, fraction))
}

fn round_div(value: i128, divisor: i128, mode: TimeRound) -> i128 {
    match mode {
        TimeRound::Floor => value.div_euclid(divisor),
        TimeRound::Ceiling => {
            let q = value.div_euclid(divisor);
            if value.rem_euclid(divisor) == 0 {
                q
            } else {
                q + 1
            }
        }
        TimeRound::Up => {
            let q = value / divisor;
            if value % divisor == 0 {
                q
            } else {
                q + value.signum()
            }
        }
        TimeRound::HalfEven => {
            let quotient = value / divisor;
            let remainder = value % divisor;
            let twice_abs = remainder.abs() * 2;
            if twice_abs < divisor {
                quotient
            } else if twice_abs > divisor {
                quotient + value.signum()
            } else if quotient % 2 == 0 {
                quotient
            } else {
                quotient + value.signum()
            }
        }
    }
}
