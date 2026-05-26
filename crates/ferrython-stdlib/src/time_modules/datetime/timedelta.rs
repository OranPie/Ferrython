use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, new_shared_fx, InstanceData, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use indexmap::IndexMap;

use super::{date_add, make_datetime_instance};
use crate::time_modules::shared::{ordinal_to_ymd, ymd_to_ordinal};

pub(super) fn make_timedelta_with_ops(
    days: i64,
    seconds: i64,
    microseconds: i64,
    total_secs: f64,
) -> PyResult<PyObjectRef> {
    let mut td_ns = IndexMap::new();
    td_ns.insert(CompactString::from("__add__"), make_builtin(timedelta_add));
    td_ns.insert(CompactString::from("__sub__"), make_builtin(timedelta_sub));
    td_ns.insert(CompactString::from("__radd__"), make_builtin(timedelta_add));
    td_ns.insert(CompactString::from("__mul__"), make_builtin(timedelta_mul));
    td_ns.insert(
        CompactString::from("__truediv__"),
        make_builtin(timedelta_truediv),
    );
    td_ns.insert(
        CompactString::from("__floordiv__"),
        make_builtin(timedelta_floordiv),
    );
    td_ns.insert(
        CompactString::from("__eq__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let a_ts = args[0]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(f64::NAN);
            let b_ts = args[1]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(f64::NAN);
            Ok(PyObject::bool_val((a_ts - b_ts).abs() < 1e-9))
        }),
    );
    td_ns.insert(
        CompactString::from("__ne__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(true));
            }
            let a_ts = args[0]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(f64::NAN);
            let b_ts = args[1]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(f64::NAN);
            Ok(PyObject::bool_val((a_ts - b_ts).abs() >= 1e-9))
        }),
    );
    td_ns.insert(
        CompactString::from("__lt__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let a_ts = args[0]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(0.0);
            let b_ts = args[1]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(0.0);
            Ok(PyObject::bool_val(a_ts < b_ts))
        }),
    );
    td_ns.insert(
        CompactString::from("__le__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let a_ts = args[0]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(0.0);
            let b_ts = args[1]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(0.0);
            Ok(PyObject::bool_val(a_ts <= b_ts))
        }),
    );
    td_ns.insert(
        CompactString::from("__gt__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let a_ts = args[0]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(0.0);
            let b_ts = args[1]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(0.0);
            Ok(PyObject::bool_val(a_ts > b_ts))
        }),
    );
    td_ns.insert(
        CompactString::from("__ge__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let a_ts = args[0]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(0.0);
            let b_ts = args[1]
                .get_attr("_total_seconds")
                .and_then(|v| v.to_float().ok())
                .unwrap_or(0.0);
            Ok(PyObject::bool_val(a_ts >= b_ts))
        }),
    );
    let class = PyObject::class(CompactString::from("timedelta"), vec![], td_ns);
    let class_flags = InstanceData::compute_flags(&class);
    let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
        Box::new(InstanceData {
            class,
            attrs: new_shared_fx(),
            is_special: true,
            dict_storage: None,
            class_flags,
            finalizer_state: std::cell::Cell::new(0),
        }),
    )));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__timedelta__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("days"), PyObject::int(days));
        w.insert(CompactString::from("seconds"), PyObject::int(seconds));
        w.insert(
            CompactString::from("microseconds"),
            PyObject::int(microseconds),
        );
        w.insert(
            CompactString::from("_total_seconds"),
            PyObject::float(total_secs),
        );
        let total_us_128 = days as i128 * 86_400_000_000i128
            + seconds as i128 * 1_000_000i128
            + microseconds as i128;
        let total_us_obj = if total_us_128 >= i64::MIN as i128 && total_us_128 <= i64::MAX as i128 {
            PyObject::int(total_us_128 as i64)
        } else {
            PyObject::big_int(num_bigint::BigInt::from(total_us_128))
        };
        w.insert(CompactString::from("_total_us"), total_us_obj);
        // total_seconds() as a callable method
        let ts = total_secs;
        w.insert(
            CompactString::from("total_seconds"),
            PyObject::native_closure("total_seconds", move |_args: &[PyObjectRef]| {
                Ok(PyObject::float(ts))
            }),
        );
        // __repr__ / __str__
        let repr = if microseconds != 0 {
            format!(
                "datetime.timedelta(days={}, seconds={}, microseconds={})",
                days, seconds, microseconds
            )
        } else if seconds != 0 {
            format!("datetime.timedelta(days={}, seconds={})", days, seconds)
        } else {
            format!("datetime.timedelta(days={})", days)
        };
        // CPython __str__ format: [D day[s], ]H:MM:SS[.UUUUUU]
        // After normalization: days can be negative, seconds in [0,86400), microseconds in [0,1000000)
        let str_val = {
            let hh = seconds / 3600;
            let mm = (seconds % 3600) / 60;
            let ss = seconds % 60;
            let mut s = String::new();
            if days != 0 {
                if days == 1 || days == -1 {
                    s.push_str(&format!("{} day, ", days));
                } else {
                    s.push_str(&format!("{} days, ", days));
                }
            }
            if microseconds != 0 {
                s.push_str(&format!("{}:{:02}:{:02}.{:06}", hh, mm, ss, microseconds));
            } else {
                s.push_str(&format!("{}:{:02}:{:02}", hh, mm, ss));
            }
            s
        };
        w.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("__repr__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(&repr)))
            }),
        );
        w.insert(
            CompactString::from("__str__"),
            PyObject::native_closure("__str__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(&str_val)))
            }),
        );
        // __bool__: timedelta is falsy only when all zero
        let is_nonzero = days != 0 || seconds != 0 || microseconds != 0;
        w.insert(
            CompactString::from("__bool__"),
            PyObject::native_closure("__bool__", move |_: &[PyObjectRef]| {
                Ok(PyObject::bool_val(is_nonzero))
            }),
        );
        // __neg__
        let (nd, ns, nus) = (-days, -seconds, -microseconds);
        let nts = -total_secs;
        w.insert(
            CompactString::from("__neg__"),
            PyObject::native_closure("__neg__", move |_: &[PyObjectRef]| {
                make_timedelta_with_ops(nd, ns, nus, nts)
            }),
        );
        // __abs__
        let (ad, as_, aus) = (days.abs(), seconds.abs(), microseconds.abs());
        let ats = total_secs.abs();
        w.insert(
            CompactString::from("__abs__"),
            PyObject::native_closure("__abs__", move |_: &[PyObjectRef]| {
                make_timedelta_with_ops(ad, as_, aus, ats)
            }),
        );
    }
    Ok(inst)
}

pub(super) fn make_timedelta(
    days: i64,
    seconds: i64,
    microseconds: i64,
    total_secs: f64,
) -> PyResult<PyObjectRef> {
    make_timedelta_with_ops(days, seconds, microseconds, total_secs)
}

fn timedelta_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("timedelta.__add__ requires 2 args"));
    }
    let a = &args[0];
    let b = &args[1];
    if b.get_attr("__date_only__").is_some() {
        return date_add(&[b.clone(), a.clone()]);
    }
    // datetime + timedelta (via __radd__): a=timedelta, b=datetime
    if b.get_attr("__datetime__").is_some() && b.get_attr("__date_only__").is_none() {
        return datetime_add_timedelta(b, a);
    }
    // Also handle timedelta + datetime (direct __add__)
    if a.get_attr("__datetime__").is_some() && a.get_attr("__date_only__").is_none() {
        return datetime_add_timedelta(a, b);
    }
    let a_days = a.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let a_secs = a.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let a_us = a
        .get_attr("microseconds")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let b_days = b.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let b_secs = b.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let b_us = b
        .get_attr("microseconds")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let days = a_days + b_days;
    let secs = a_secs + b_secs;
    let us = a_us + b_us;
    let total = days as f64 * 86400.0 + secs as f64 + us as f64 / 1_000_000.0;
    make_timedelta_with_ops(days, secs, us, total)
}

fn timedelta_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("timedelta.__sub__ requires 2 args"));
    }
    let a = &args[0];
    let b = &args[1];
    let a_days = a.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let a_secs = a.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let a_us = a
        .get_attr("microseconds")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let b_days = b.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let b_secs = b.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let b_us = b
        .get_attr("microseconds")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let days = a_days - b_days;
    let secs = a_secs - b_secs;
    let us = a_us - b_us;
    let total = days as f64 * 86400.0 + secs as f64 + us as f64 / 1_000_000.0;
    make_timedelta_with_ops(days, secs, us, total)
}

fn timedelta_mul(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("timedelta.__mul__ requires 2 args"));
    }
    let td = &args[0];
    let factor = args[1]
        .to_int()
        .map_err(|_| PyException::type_error("unsupported operand type(s) for *"))?;
    let td_days = td.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let td_secs = td.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let td_us = td
        .get_attr("microseconds")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let total_us = (td_days * 86_400_000_000 + td_secs * 1_000_000 + td_us) * factor;
    let days = total_us / 86_400_000_000;
    let rem = total_us % 86_400_000_000;
    let seconds = rem / 1_000_000;
    let microseconds = rem % 1_000_000;
    let total = total_us as f64 / 1_000_000.0;
    make_timedelta_with_ops(days, seconds, microseconds, total)
}

/// timedelta / int_or_float → timedelta, timedelta / timedelta → float
fn timedelta_truediv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "timedelta.__truediv__ requires 2 args",
        ));
    }
    let td = &args[0];
    let other = &args[1];
    let td_us = td.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0) * 86_400_000_000
        + td.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0) * 1_000_000
        + td.get_attr("microseconds")
            .and_then(|v| v.as_int())
            .unwrap_or(0);

    // timedelta / timedelta → float ratio
    if other.get_attr("__timedelta__").is_some() {
        let other_us = other.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0)
            * 86_400_000_000
            + other
                .get_attr("seconds")
                .and_then(|v| v.as_int())
                .unwrap_or(0)
                * 1_000_000
            + other
                .get_attr("microseconds")
                .and_then(|v| v.as_int())
                .unwrap_or(0);
        if other_us == 0 {
            return Err(PyException::runtime_error("division by zero"));
        }
        return Ok(PyObject::float(td_us as f64 / other_us as f64));
    }

    // timedelta / number → timedelta
    let divisor = other.to_float().map_err(|_| {
        PyException::type_error("unsupported operand type(s) for /: 'timedelta' and non-numeric")
    })?;
    if divisor == 0.0 {
        return Err(PyException::runtime_error("division by zero"));
    }
    let result_us = (td_us as f64 / divisor).round() as i64;
    let days = result_us / 86_400_000_000;
    let rem = result_us % 86_400_000_000;
    let seconds = rem / 1_000_000;
    let microseconds = rem % 1_000_000;
    make_timedelta_with_ops(days, seconds, microseconds, result_us as f64 / 1_000_000.0)
}

/// timedelta // int → timedelta, timedelta // timedelta → int
fn timedelta_floordiv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "timedelta.__floordiv__ requires 2 args",
        ));
    }
    let td = &args[0];
    let other = &args[1];
    let td_us = td.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0) * 86_400_000_000
        + td.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0) * 1_000_000
        + td.get_attr("microseconds")
            .and_then(|v| v.as_int())
            .unwrap_or(0);

    // timedelta // timedelta → int
    if other.get_attr("__timedelta__").is_some() {
        let other_us = other.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0)
            * 86_400_000_000
            + other
                .get_attr("seconds")
                .and_then(|v| v.as_int())
                .unwrap_or(0)
                * 1_000_000
            + other
                .get_attr("microseconds")
                .and_then(|v| v.as_int())
                .unwrap_or(0);
        if other_us == 0 {
            return Err(PyException::runtime_error("division by zero"));
        }
        return Ok(PyObject::int(td_us / other_us));
    }

    // timedelta // int → timedelta
    let divisor = other.to_int().map_err(|_| {
        PyException::type_error("unsupported operand type(s) for //: 'timedelta' and non-numeric")
    })?;
    if divisor == 0 {
        return Err(PyException::runtime_error("division by zero"));
    }
    let result_us = td_us / divisor;
    let days = result_us / 86_400_000_000;
    let rem = result_us % 86_400_000_000;
    let seconds = rem / 1_000_000;
    let microseconds = rem % 1_000_000;
    make_timedelta_with_ops(days, seconds, microseconds, result_us as f64 / 1_000_000.0)
}

/// datetime + timedelta → datetime
fn datetime_add_timedelta(dt: &PyObjectRef, td: &PyObjectRef) -> PyResult<PyObjectRef> {
    let year = dt.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
    let month = dt.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
    let day = dt.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
    let hour = dt.get_attr("hour").and_then(|v| v.as_int()).unwrap_or(0);
    let minute = dt.get_attr("minute").and_then(|v| v.as_int()).unwrap_or(0);
    let second = dt.get_attr("second").and_then(|v| v.as_int()).unwrap_or(0);
    let microsecond = dt
        .get_attr("microsecond")
        .and_then(|v| v.as_int())
        .unwrap_or(0);

    let td_days = td.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let td_secs = td.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let td_us = td
        .get_attr("microseconds")
        .and_then(|v| v.as_int())
        .unwrap_or(0);

    // Add microseconds/seconds/days with carry
    let total_us = microsecond + td_us;
    let carry_s = total_us.div_euclid(1_000_000);
    let new_us = total_us.rem_euclid(1_000_000);

    let total_s = second + td_secs + carry_s;
    let carry_m = total_s.div_euclid(60);
    let new_s = total_s.rem_euclid(60);

    let total_m = minute + carry_m;
    let carry_h = total_m.div_euclid(60);
    let new_m = total_m.rem_euclid(60);

    let total_h = hour + carry_h;
    let carry_d = total_h.div_euclid(24);
    let new_h = total_h.rem_euclid(24);

    let ord = ymd_to_ordinal(year, month, day) + td_days + carry_d;
    let (ny, nm, nd) = ordinal_to_ymd(ord);

    Ok(make_datetime_instance(
        ny, nm, nd, new_h, new_m, new_s, new_us,
    ))
}

/// datetime - timedelta → datetime; datetime - datetime → timedelta
pub(super) fn datetime_sub_dunder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("datetime.__sub__ requires 2 args"));
    }
    let dt = &args[0];
    let other = &args[1];
    if other.get_attr("__timedelta__").is_some() {
        // datetime - timedelta → datetime (negate and add)
        let neg_days = -other.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
        let neg_secs = -other
            .get_attr("seconds")
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let neg_us = -other
            .get_attr("microseconds")
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let total = neg_days as f64 * 86400.0 + neg_secs as f64 + neg_us as f64 / 1_000_000.0;
        let neg_td = make_timedelta_with_ops(neg_days, neg_secs, neg_us, total)?;
        datetime_add_timedelta(dt, &neg_td)
    } else if other.get_attr("__datetime__").is_some() {
        // datetime - datetime → timedelta
        let y1 = dt.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
        let m1 = dt.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
        let d1 = dt.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
        let h1 = dt.get_attr("hour").and_then(|v| v.as_int()).unwrap_or(0);
        let mi1 = dt.get_attr("minute").and_then(|v| v.as_int()).unwrap_or(0);
        let s1 = dt.get_attr("second").and_then(|v| v.as_int()).unwrap_or(0);
        let us1 = dt
            .get_attr("microsecond")
            .and_then(|v| v.as_int())
            .unwrap_or(0);

        let y2 = other
            .get_attr("year")
            .and_then(|v| v.as_int())
            .unwrap_or(1970);
        let m2 = other
            .get_attr("month")
            .and_then(|v| v.as_int())
            .unwrap_or(1);
        let d2 = other.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
        let h2 = other.get_attr("hour").and_then(|v| v.as_int()).unwrap_or(0);
        let mi2 = other
            .get_attr("minute")
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let s2 = other
            .get_attr("second")
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let us2 = other
            .get_attr("microsecond")
            .and_then(|v| v.as_int())
            .unwrap_or(0);

        let total_secs1 = ymd_to_ordinal(y1, m1, d1) * 86400 + h1 * 3600 + mi1 * 60 + s1;
        let total_secs2 = ymd_to_ordinal(y2, m2, d2) * 86400 + h2 * 3600 + mi2 * 60 + s2;
        let diff_secs = total_secs1 - total_secs2;
        let diff_us = us1 - us2;
        let days = diff_secs / 86400;
        let secs = diff_secs % 86400;
        let total = diff_secs as f64 + diff_us as f64 / 1_000_000.0;
        make_timedelta_with_ops(days, secs, diff_us, total)
    } else {
        Err(PyException::type_error("unsupported operand type(s) for -"))
    }
}

/// datetime + timedelta → datetime (dunder)
pub(super) fn datetime_add_dunder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("datetime.__add__ requires 2 args"));
    }
    datetime_add_timedelta(&args[0], &args[1])
}

fn datetime_to_ordinal_secs(obj: &PyObjectRef) -> (i64, i64) {
    let y = obj
        .get_attr("year")
        .and_then(|v| v.as_int())
        .unwrap_or(1970);
    let m = obj.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
    let d = obj.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
    let h = obj.get_attr("hour").and_then(|v| v.as_int()).unwrap_or(0);
    let mi = obj.get_attr("minute").and_then(|v| v.as_int()).unwrap_or(0);
    let s = obj.get_attr("second").and_then(|v| v.as_int()).unwrap_or(0);
    let ord = ymd_to_ordinal(y, m, d);
    (ord, h * 3600 + mi * 60 + s)
}

fn datetime_cmp(a: &PyObjectRef, b: &PyObjectRef) -> std::cmp::Ordering {
    let (ord_a, sec_a) = datetime_to_ordinal_secs(a);
    let (ord_b, sec_b) = datetime_to_ordinal_secs(b);
    ord_a.cmp(&ord_b).then(sec_a.cmp(&sec_b))
}

pub(super) fn datetime_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        datetime_cmp(&args[0], &args[1]) == std::cmp::Ordering::Equal,
    ))
}

pub(super) fn datetime_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        datetime_cmp(&args[0], &args[1]) == std::cmp::Ordering::Less,
    ))
}

pub(super) fn datetime_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        datetime_cmp(&args[0], &args[1]) != std::cmp::Ordering::Greater,
    ))
}

pub(super) fn datetime_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        datetime_cmp(&args[0], &args[1]) == std::cmp::Ordering::Greater,
    ))
}

pub(super) fn datetime_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        datetime_cmp(&args[0], &args[1]) != std::cmp::Ordering::Less,
    ))
}
