use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, new_shared_fx, InstanceData, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use super::timedelta::make_timedelta_with_ops;
use crate::time_modules::shared::{
    days_in_month, format_time, ordinal_to_ymd, ymd_to_ordinal, DAY_NAMES_ABBR, MONTH_NAMES_ABBR,
};

pub(super) fn install_date_instance_attrs(
    w: &mut IndexMap<CompactString, PyObjectRef, impl std::hash::BuildHasher>,
    year: i64,
    month: i64,
    day: i64,
) {
    w.insert(
        CompactString::from("__datetime__"),
        PyObject::bool_val(true),
    );
    w.insert(
        CompactString::from("__date_only__"),
        PyObject::bool_val(true),
    );
    w.insert(CompactString::from("year"), PyObject::int(year));
    w.insert(CompactString::from("month"), PyObject::int(month));
    w.insert(CompactString::from("day"), PyObject::int(day));

    // isoformat() -> str
    let (y, mo, da) = (year, month, day);
    w.insert(
        CompactString::from("isoformat"),
        PyObject::native_closure("date.isoformat", move |_: &[PyObjectRef]| {
            Ok(PyObject::str_val(CompactString::from(format!(
                "{:04}-{:02}-{:02}",
                y, mo, da
            ))))
        }),
    );

    // strftime(format) -> str
    let ord = ymd_to_ordinal(year, month, day);
    let wd = ((ord + 6) % 7) as i64;
    let yday_d = {
        let md = days_in_month(y);
        let mut yd = da;
        for i in 0..(mo - 1) as usize {
            if i < 12 {
                yd += md[i];
            }
        }
        yd
    };
    let wd_d = wd;
    w.insert(
        CompactString::from("strftime"),
        PyObject::native_closure("date.strftime", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("strftime requires format string"));
            }
            let fmt = args[0].py_to_string();
            let result = format_time(&fmt, y, mo, da, 0, 0, 0, wd_d, yday_d);
            Ok(PyObject::str_val(CompactString::from(result)))
        }),
    );

    w.insert(
        CompactString::from("weekday"),
        PyObject::native_closure("date.weekday", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(wd))
        }),
    );
    let iwd = wd + 1;
    w.insert(
        CompactString::from("isoweekday"),
        PyObject::native_closure("date.isoweekday", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(iwd))
        }),
    );

    w.insert(
        CompactString::from("__str__"),
        PyObject::native_closure("date.__str__", move |_: &[PyObjectRef]| {
            Ok(PyObject::str_val(CompactString::from(format!(
                "{:04}-{:02}-{:02}",
                y, mo, da
            ))))
        }),
    );
    w.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("date.__repr__", move |_: &[PyObjectRef]| {
            Ok(PyObject::str_val(CompactString::from(format!(
                "datetime.date({}, {}, {})",
                y, mo, da
            ))))
        }),
    );

    w.insert(
        CompactString::from("toordinal"),
        PyObject::native_closure("date.toordinal", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(ymd_to_ordinal(y, mo, da)))
        }),
    );

    let (ry, rmo, rda) = (year, month, day);
    w.insert(
        CompactString::from("replace"),
        PyObject::native_closure("date.replace", move |args: &[PyObjectRef]| {
            let mut ny = ry;
            let mut nmo = rmo;
            let mut nda = rda;
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw) = &last.payload {
                    let r = kw.read();
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("year"))) {
                        ny = v.as_int().unwrap_or(ny);
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("month"))) {
                        nmo = v.as_int().unwrap_or(nmo);
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("day"))) {
                        nda = v.as_int().unwrap_or(nda);
                    }
                }
            }
            if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
                if args.len() > 0 {
                    ny = args[0].as_int().unwrap_or(ny);
                }
                if args.len() > 1 {
                    nmo = args[1].as_int().unwrap_or(nmo);
                }
                if args.len() > 2 {
                    nda = args[2].as_int().unwrap_or(nda);
                }
            }
            Ok(make_date_instance(ny, nmo, nda))
        }),
    );

    let (cy, cmo, cda) = (year, month, day);
    let cwd = wd;
    w.insert(
        CompactString::from("ctime"),
        PyObject::native_closure("date.ctime", move |_: &[PyObjectRef]| {
            Ok(PyObject::str_val(CompactString::from(format!(
                "{} {} {:2} 00:00:00 {:04}",
                DAY_NAMES_ABBR[cwd as usize % 7],
                MONTH_NAMES_ABBR[(cmo - 1) as usize % 12],
                cda,
                cy
            ))))
        }),
    );

    let (ty, tmo, tda) = (year, month, day);
    let twd = wd;
    let tyd = yday_d;
    w.insert(
        CompactString::from("timetuple"),
        PyObject::native_closure("date.timetuple", move |_: &[PyObjectRef]| {
            Ok(PyObject::tuple(vec![
                PyObject::int(ty),
                PyObject::int(tmo),
                PyObject::int(tda),
                PyObject::int(0),
                PyObject::int(0),
                PyObject::int(0),
                PyObject::int(twd),
                PyObject::int(tyd),
                PyObject::int(-1),
            ]))
        }),
    );

    let (hy, hmo, hda) = (year, month, day);
    w.insert(
        CompactString::from("__hash__"),
        PyObject::native_closure("date.__hash__", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(hy * 13 + hmo * 7 + hda * 3))
        }),
    );
}

pub(super) fn make_date_instance(year: i64, month: i64, day: i64) -> PyObjectRef {
    let mut date_cls_ns = IndexMap::new();
    date_cls_ns.insert(CompactString::from("__add__"), make_builtin(date_add));
    date_cls_ns.insert(CompactString::from("__sub__"), make_builtin(date_sub));
    date_cls_ns.insert(CompactString::from("__eq__"), make_builtin(date_eq));
    date_cls_ns.insert(CompactString::from("__lt__"), make_builtin(date_lt));
    date_cls_ns.insert(CompactString::from("__le__"), make_builtin(date_le));
    date_cls_ns.insert(CompactString::from("__gt__"), make_builtin(date_gt));
    date_cls_ns.insert(CompactString::from("__ge__"), make_builtin(date_ge));
    let class = PyObject::class(CompactString::from("date"), vec![], date_cls_ns);
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
        install_date_instance_attrs(&mut w, year, month, day);
    }
    inst
}

pub(super) fn date_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("date.__add__ requires 2 args"));
    }
    let date_obj = &args[0];
    let td_obj = &args[1];
    let year = date_obj
        .get_attr("year")
        .and_then(|v| v.as_int())
        .unwrap_or(1970);
    let month = date_obj
        .get_attr("month")
        .and_then(|v| v.as_int())
        .unwrap_or(1);
    let day = date_obj
        .get_attr("day")
        .and_then(|v| v.as_int())
        .unwrap_or(1);
    let td_days = td_obj
        .get_attr("days")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let ord = ymd_to_ordinal(year, month, day) + td_days;
    let (ny, nm, nd) = ordinal_to_ymd(ord);
    Ok(make_date_instance(ny, nm, nd))
}

pub(super) fn date_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("date.__sub__ requires 2 args"));
    }
    let date_obj = &args[0];
    let other = &args[1];
    let year = date_obj
        .get_attr("year")
        .and_then(|v| v.as_int())
        .unwrap_or(1970);
    let month = date_obj
        .get_attr("month")
        .and_then(|v| v.as_int())
        .unwrap_or(1);
    let day = date_obj
        .get_attr("day")
        .and_then(|v| v.as_int())
        .unwrap_or(1);
    if other.get_attr("__timedelta__").is_some() {
        let td_days = other.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
        let ord = ymd_to_ordinal(year, month, day) - td_days;
        let (ny, nm, nd) = ordinal_to_ymd(ord);
        Ok(make_date_instance(ny, nm, nd))
    } else if other.get_attr("__date_only__").is_some() {
        let y2 = other
            .get_attr("year")
            .and_then(|v| v.as_int())
            .unwrap_or(1970);
        let m2 = other
            .get_attr("month")
            .and_then(|v| v.as_int())
            .unwrap_or(1);
        let d2 = other.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
        let diff = ymd_to_ordinal(year, month, day) - ymd_to_ordinal(y2, m2, d2);
        make_timedelta_with_ops(diff, 0, 0, diff as f64 * 86400.0)
    } else {
        Err(PyException::type_error("unsupported operand type(s) for -"))
    }
}

fn date_ordinal(obj: &PyObjectRef) -> i64 {
    let y = obj
        .get_attr("year")
        .and_then(|v| v.as_int())
        .unwrap_or(1970);
    let m = obj.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
    let d = obj.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
    ymd_to_ordinal(y, m, d)
}

pub(super) fn date_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) == date_ordinal(&args[1]),
    ))
}

pub(super) fn date_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) < date_ordinal(&args[1]),
    ))
}

pub(super) fn date_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) <= date_ordinal(&args[1]),
    ))
}

pub(super) fn date_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) > date_ordinal(&args[1]),
    ))
}

pub(super) fn date_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) >= date_ordinal(&args[1]),
    ))
}
