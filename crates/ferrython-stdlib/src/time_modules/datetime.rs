//! datetime stdlib module implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, new_shared_fx, InstanceData, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use super::shared::{
    days_in_month, days_to_ymd, format_time, format_time_us, ordinal_to_ymd, ymd_to_ordinal,
    DAY_NAMES_ABBR, MONTH_NAMES_ABBR,
};

mod timedelta;

use timedelta::{
    datetime_add_dunder, datetime_eq, datetime_ge, datetime_gt, datetime_le, datetime_lt,
    datetime_sub_dunder, make_timedelta, make_timedelta_with_ops,
};

pub fn create_datetime_module() -> PyObjectRef {
    // Build datetime class with constructor and class methods
    let mut dt_ns = IndexMap::new();
    dt_ns.insert(CompactString::from("now"), make_builtin(datetime_now));
    dt_ns.insert(CompactString::from("today"), make_builtin(datetime_now));
    dt_ns.insert(CompactString::from("utcnow"), make_builtin(datetime_now));
    dt_ns.insert(
        CompactString::from("fromisoformat"),
        make_builtin(datetime_fromisoformat),
    );
    dt_ns.insert(
        CompactString::from("strptime"),
        make_builtin(datetime_strptime),
    );
    dt_ns.insert(
        CompactString::from("fromtimestamp"),
        make_builtin(datetime_fromtimestamp),
    );
    dt_ns.insert(
        CompactString::from("combine"),
        make_builtin(datetime_combine),
    );
    dt_ns.insert(
        CompactString::from("fromordinal"),
        make_builtin(datetime_fromordinal),
    );
    dt_ns.insert(
        CompactString::from("__add__"),
        make_builtin(datetime_add_dunder),
    );
    dt_ns.insert(
        CompactString::from("__sub__"),
        make_builtin(datetime_sub_dunder),
    );
    dt_ns.insert(CompactString::from("__eq__"), make_builtin(datetime_eq));
    dt_ns.insert(CompactString::from("__lt__"), make_builtin(datetime_lt));
    dt_ns.insert(CompactString::from("__le__"), make_builtin(datetime_le));
    dt_ns.insert(CompactString::from("__gt__"), make_builtin(datetime_gt));
    dt_ns.insert(CompactString::from("__ge__"), make_builtin(datetime_ge));
    let datetime_cls = PyObject::class(CompactString::from("datetime"), vec![], dt_ns);
    // Store __init__ for constructor dispatch
    if let PyObjectPayload::Class(ref cd) = datetime_cls.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args| {
                // datetime(year, month, day, hour=0, minute=0, second=0, microsecond=0, tzinfo=None)
                if args.len() < 4 {
                    return Err(PyException::type_error(
                        "datetime() requires at least year, month, day",
                    ));
                }

                // Detect trailing kwargs dict appended by the VM's call_object_kw
                let mut tzinfo_val: Option<PyObjectRef> = None;
                let positional_end = {
                    let last = &args[args.len() - 1];
                    if matches!(&last.payload, PyObjectPayload::Dict(_)) {
                        if let PyObjectPayload::Dict(ref map) = last.payload {
                            let map_r = map.read();
                            if let Some(v) =
                                map_r.get(&HashableKey::str_key(CompactString::from("tzinfo")))
                            {
                                tzinfo_val = Some(v.clone());
                            }
                        }
                        args.len() - 1
                    } else {
                        args.len()
                    }
                };

                let year = args[1].to_int()?;
                let month = args[2].to_int()?;
                let day = args[3].to_int()?;
                let hour = if positional_end > 4 {
                    args[4].to_int()?
                } else {
                    0
                };
                let minute = if positional_end > 5 {
                    args[5].to_int()?
                } else {
                    0
                };
                let second = if positional_end > 6 {
                    args[6].to_int()?
                } else {
                    0
                };
                let microsecond = if positional_end > 7 {
                    args[7].to_int()?
                } else {
                    0
                };

                // Build instance with all methods via install_datetime_methods
                install_datetime_methods(
                    &args[0],
                    year,
                    month,
                    day,
                    hour,
                    minute,
                    second,
                    microsecond,
                );
                if let Some(tz) = tzinfo_val {
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs.write().insert(CompactString::from("tzinfo"), tz);
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    // Class constants: datetime.min, datetime.max, datetime.resolution
    if let PyObjectPayload::Class(ref cd) = datetime_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("min"),
            make_datetime_instance(1, 1, 1, 0, 0, 0, 0),
        );
        ns.insert(
            CompactString::from("max"),
            make_datetime_instance(9999, 12, 31, 23, 59, 59, 999999),
        );
        ns.insert(
            CompactString::from("resolution"),
            datetime_timedelta(&[
                PyObject::none(),
                PyObject::int(0),
                PyObject::int(0),
                PyObject::int(1),
            ])
            .unwrap_or_else(|_| PyObject::none()),
        );
    }

    // Build date class with constructor and class methods
    let mut date_ns = IndexMap::new();
    date_ns.insert(CompactString::from("today"), make_builtin(date_today));
    date_ns.insert(
        CompactString::from("fromisoformat"),
        make_builtin(date_fromisoformat),
    );
    date_ns.insert(
        CompactString::from("fromordinal"),
        make_builtin(date_fromordinal),
    );
    date_ns.insert(CompactString::from("__add__"), make_builtin(date_add));
    date_ns.insert(CompactString::from("__sub__"), make_builtin(date_sub));
    date_ns.insert(CompactString::from("__eq__"), make_builtin(date_eq));
    date_ns.insert(CompactString::from("__lt__"), make_builtin(date_lt));
    date_ns.insert(CompactString::from("__le__"), make_builtin(date_le));
    date_ns.insert(CompactString::from("__gt__"), make_builtin(date_gt));
    date_ns.insert(CompactString::from("__ge__"), make_builtin(date_ge));
    let date_cls = PyObject::class(CompactString::from("date"), vec![], date_ns);
    if let PyObjectPayload::Class(ref cd) = date_cls.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args| {
                // date(year, month, day)
                if args.len() < 4 {
                    return Err(PyException::type_error("date() requires year, month, day"));
                }
                let year = args[1].to_int()?;
                let month = args[2].to_int()?;
                let day = args[3].to_int()?;
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let mut w = inst.attrs.write();
                    install_date_instance_attrs(&mut w, year, month, day);
                }
                Ok(PyObject::none())
            }),
        );
    }

    // Build timezone class
    let mut tz_ns = IndexMap::new();
    tz_ns.insert(CompactString::from("utc"), make_timezone_utc());
    let tz_cls = PyObject::class(CompactString::from("timezone"), vec![], tz_ns);
    if let PyObjectPayload::Class(ref cd) = tz_cls.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args| {
                // timezone(offset) where offset is a timedelta
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "timezone() requires an offset argument",
                    ));
                }
                let offset = &args[1];
                let offset_secs = offset
                    .get_attr("_total_seconds")
                    .and_then(|v| Some(v.to_float().unwrap_or(0.0)))
                    .unwrap_or(0.0);
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let mut w = inst.attrs.write();
                    w.insert(
                        CompactString::from("__timezone__"),
                        PyObject::bool_val(true),
                    );
                    w.insert(
                        CompactString::from("_offset_seconds"),
                        PyObject::float(offset_secs),
                    );
                    let total_mins = (offset_secs / 60.0) as i64;
                    let sign = if total_mins >= 0 { "+" } else { "-" };
                    let abs_mins = total_mins.abs();
                    let name = format!("UTC{}{:02}:{:02}", sign, abs_mins / 60, abs_mins % 60);
                    w.insert(
                        CompactString::from("_name"),
                        PyObject::str_val(CompactString::from(&name)),
                    );
                    let name_clone = name.clone();
                    w.insert(
                        CompactString::from("__str__"),
                        PyObject::native_closure("timezone.__str__", move |_| {
                            Ok(PyObject::str_val(CompactString::from(&name_clone)))
                        }),
                    );
                    let repr_offset = offset_secs;
                    w.insert(
                        CompactString::from("__repr__"),
                        PyObject::native_closure("timezone.__repr__", move |_| {
                            Ok(PyObject::str_val(CompactString::from(format!(
                                "datetime.timezone(datetime.timedelta(seconds={}))",
                                repr_offset
                            ))))
                        }),
                    );
                    w.insert(
                        CompactString::from("tzname"),
                        PyObject::native_closure("timezone.tzname", move |_| {
                            Ok(PyObject::str_val(CompactString::from(&name)))
                        }),
                    );
                    let off_s = offset_secs;
                    w.insert(
                        CompactString::from("utcoffset"),
                        PyObject::native_closure("timezone.utcoffset", move |_| {
                            make_timedelta(0, off_s as i64, 0, off_s)
                        }),
                    );
                    w.insert(
                        CompactString::from("dst"),
                        make_builtin(|_| Ok(PyObject::none())),
                    );
                }
                Ok(PyObject::none())
            }),
        );
    }
    // date class constants: date.min, date.max, date.resolution
    if let PyObjectPayload::Class(ref cd) = date_cls.payload {
        let mut ns = cd.namespace.write();
        let min_date = {
            let class = PyObject::class(CompactString::from("date"), vec![], IndexMap::new());
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
                    CompactString::from("__datetime__"),
                    PyObject::bool_val(true),
                );
                w.insert(
                    CompactString::from("__date_only__"),
                    PyObject::bool_val(true),
                );
                w.insert(CompactString::from("year"), PyObject::int(1));
                w.insert(CompactString::from("month"), PyObject::int(1));
                w.insert(CompactString::from("day"), PyObject::int(1));
            }
            inst
        };
        let max_date = {
            let class = PyObject::class(CompactString::from("date"), vec![], IndexMap::new());
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
                    CompactString::from("__datetime__"),
                    PyObject::bool_val(true),
                );
                w.insert(
                    CompactString::from("__date_only__"),
                    PyObject::bool_val(true),
                );
                w.insert(CompactString::from("year"), PyObject::int(9999));
                w.insert(CompactString::from("month"), PyObject::int(12));
                w.insert(CompactString::from("day"), PyObject::int(31));
            }
            inst
        };
        ns.insert(CompactString::from("min"), min_date);
        ns.insert(CompactString::from("max"), max_date);
        ns.insert(
            CompactString::from("resolution"),
            datetime_timedelta(&[
                PyObject::none(),
                PyObject::int(1),
                PyObject::int(0),
                PyObject::int(0),
            ])
            .unwrap_or_else(|_| PyObject::none()),
        );
    }

    // Build tzinfo abstract base class (base of timezone)
    let mut tzinfo_ns = IndexMap::new();
    tzinfo_ns.insert(
        CompactString::from("utcoffset"),
        make_builtin(|_| {
            Err(PyException::type_error(
                "tzinfo.utcoffset() must be overridden",
            ))
        }),
    );
    tzinfo_ns.insert(
        CompactString::from("tzname"),
        make_builtin(|_| {
            Err(PyException::type_error(
                "tzinfo.tzname() must be overridden",
            ))
        }),
    );
    tzinfo_ns.insert(
        CompactString::from("dst"),
        make_builtin(|_| Err(PyException::type_error("tzinfo.dst() must be overridden"))),
    );
    tzinfo_ns.insert(
        CompactString::from("fromutc"),
        make_builtin(|_args| Ok(PyObject::none())),
    );
    let tzinfo_cls = PyObject::class(CompactString::from("tzinfo"), vec![], tzinfo_ns);

    make_module(
        "datetime",
        vec![
            ("datetime", datetime_cls),
            ("date", date_cls),
            ("time", make_builtin(datetime_time_obj)),
            ("timedelta", make_builtin(datetime_timedelta)),
            ("timezone", tz_cls),
            ("tzinfo", tzinfo_cls),
            ("MINYEAR", PyObject::int(1)),
            ("MAXYEAR", PyObject::int(9999)),
        ],
    )
}

fn make_timezone_utc() -> PyObjectRef {
    let class = PyObject::class(CompactString::from("timezone"), vec![], IndexMap::new());
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
            CompactString::from("__timezone__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("_offset_seconds"), PyObject::float(0.0));
        w.insert(
            CompactString::from("_name"),
            PyObject::str_val(CompactString::from("UTC")),
        );
        w.insert(
            CompactString::from("__str__"),
            make_builtin(|_| Ok(PyObject::str_val(CompactString::from("UTC")))),
        );
        w.insert(
            CompactString::from("__repr__"),
            make_builtin(|_| {
                Ok(PyObject::str_val(CompactString::from(
                    "datetime.timezone.utc",
                )))
            }),
        );
        w.insert(
            CompactString::from("utcoffset"),
            make_builtin(|_| make_timedelta(0, 0, 0, 0.0)),
        );
        w.insert(
            CompactString::from("tzname"),
            make_builtin(|_| Ok(PyObject::str_val(CompactString::from("UTC")))),
        );
        w.insert(
            CompactString::from("dst"),
            make_builtin(|_| Ok(PyObject::none())),
        );
    }
    inst
}

fn datetime_now(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Extract optional tz argument (positional or kwarg)
    let mut tz_val: Option<PyObjectRef> = None;
    for arg in args {
        match &arg.payload {
            PyObjectPayload::Dict(ref map) => {
                let map_r = map.read();
                if let Some(v) = map_r.get(&HashableKey::str_key(CompactString::from("tz"))) {
                    if !matches!(v.payload, PyObjectPayload::None) {
                        tz_val = Some(v.clone());
                    }
                }
            }
            PyObjectPayload::Instance(_) => {
                // Positional tz argument
                if arg.get_attr("__timezone__").is_some() {
                    tz_val = Some(arg.clone());
                }
            }
            _ => {}
        }
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let micros = now.subsec_micros();

    // Apply timezone offset if provided
    let offset_secs: i64 = tz_val
        .as_ref()
        .and_then(|tz| tz.get_attr("_offset_secs").and_then(|v| v.as_int()))
        .unwrap_or(0);

    let adjusted = secs as i64 + offset_secs;
    let adjusted_u = adjusted.unsigned_abs();
    let days = adjusted_u / 86400;
    let time_of_day = adjusted_u % 86400;
    let hour = (time_of_day / 3600) as i64;
    let minute = ((time_of_day % 3600) / 60) as i64;
    let second = (time_of_day % 60) as i64;
    let (year, month, day) = days_to_ymd(days as i64 + 719468);
    let inst = make_datetime_instance(year, month, day, hour, minute, second, micros as i64);

    // Set tzinfo on the result
    if let Some(tz) = tz_val {
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            d.attrs.write().insert(CompactString::from("tzinfo"), tz);
        }
    }
    Ok(inst)
}

fn date_today(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let days = now.as_secs() / 86400;
    let (year, month, day) = days_to_ymd(days as i64 + 719468);
    Ok(make_date_instance(year, month, day))
}

fn datetime_fromtimestamp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("fromtimestamp", args, 1)?;
    let ts = args[0].to_float()?;
    let secs = ts as u64;
    let micros = ((ts - secs as f64) * 1_000_000.0) as i64;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hour = (time_of_day / 3600) as i64;
    let minute = ((time_of_day % 3600) / 60) as i64;
    let second = (time_of_day % 60) as i64;
    let (year, month, day) = days_to_ymd(days as i64 + 719468);
    Ok(make_datetime_instance(
        year, month, day, hour, minute, second, micros,
    ))
}

fn datetime_combine(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "combine() requires date and time arguments",
        ));
    }
    let date_obj = &args[0];
    let time_obj = &args[1];
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
    let hour = time_obj
        .get_attr("hour")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let minute = time_obj
        .get_attr("minute")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let second = time_obj
        .get_attr("second")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let microsecond = time_obj
        .get_attr("microsecond")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    Ok(make_datetime_instance(
        year,
        month,
        day,
        hour,
        minute,
        second,
        microsecond,
    ))
}

fn datetime_fromordinal(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("fromordinal", args, 1)?;
    let ordinal = args[0].to_int()?;
    let (year, month, day) = ordinal_to_ymd(ordinal);
    Ok(make_datetime_instance(year, month, day, 0, 0, 0, 0))
}

fn date_fromordinal(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("fromordinal", args, 1)?;
    let ordinal = args[0].to_int()?;
    let (year, month, day) = ordinal_to_ymd(ordinal);
    Ok(make_date_instance(year, month, day))
}

fn date_fromisoformat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("date.fromisoformat", args, 1)?;
    let s = args[0].py_to_string();
    let date_parts: Vec<&str> = s.split('-').collect();
    if date_parts.len() < 3 {
        return Err(PyException::value_error("Invalid isoformat string"));
    }
    let year: i64 = date_parts[0]
        .parse()
        .map_err(|_| PyException::value_error("Invalid year"))?;
    let month: i64 = date_parts[1]
        .parse()
        .map_err(|_| PyException::value_error("Invalid month"))?;
    let day: i64 = date_parts[2]
        .split('T')
        .next()
        .unwrap_or("1")
        .parse()
        .map_err(|_| PyException::value_error("Invalid day"))?;
    Ok(make_date_instance(year, month, day))
}

fn datetime_fromisoformat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("fromisoformat", args, 1)?;
    let s = args[0].py_to_string();
    // Parse ISO format: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS
    let parts: Vec<&str> = s.split('T').collect();
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() < 3 {
        return Err(PyException::value_error("Invalid isoformat"));
    }
    let year: i64 = date_parts[0]
        .parse()
        .map_err(|_| PyException::value_error("Invalid year"))?;
    let month: i64 = date_parts[1]
        .parse()
        .map_err(|_| PyException::value_error("Invalid month"))?;
    let day: i64 = date_parts[2]
        .parse()
        .map_err(|_| PyException::value_error("Invalid day"))?;
    let (hour, minute, second) = if parts.len() > 1 {
        let time_parts: Vec<&str> = parts[1].split(':').collect();
        let h: i64 = time_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let m: i64 = time_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let sec: i64 = time_parts
            .get(2)
            .and_then(|s| s.split('.').next().unwrap_or("0").parse().ok())
            .unwrap_or(0);
        (h, m, sec)
    } else {
        (0, 0, 0)
    };
    Ok(make_datetime_instance(
        year, month, day, hour, minute, second, 0,
    ))
}

/// datetime.strptime(date_string, format) — parse a date string with a format specifier.
fn datetime_strptime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "strptime() requires 2 arguments: date_string and format",
        ));
    }
    let date_str = args[0].py_to_string();
    let fmt = args[1].py_to_string();

    let mut year: i64 = 1900;
    let mut month: i64 = 1;
    let mut day: i64 = 1;
    let mut hour: i64 = 0;
    let mut minute: i64 = 0;
    let mut second: i64 = 0;
    let mut microsecond: i64 = 0;

    let fmt_bytes = fmt.as_bytes();
    let str_bytes = date_str.as_bytes();
    let mut fi = 0;
    let mut si = 0;

    while fi < fmt_bytes.len() && si < str_bytes.len() {
        if fmt_bytes[fi] == b'%' && fi + 1 < fmt_bytes.len() {
            fi += 1;
            let code = fmt_bytes[fi] as char;
            fi += 1;
            match code {
                'Y' => {
                    let (v, new_si) = parse_int(&date_str, si, 4)?;
                    year = v;
                    si = new_si;
                }
                'm' => {
                    let (v, new_si) = parse_int(&date_str, si, 2)?;
                    month = v;
                    si = new_si;
                }
                'd' => {
                    let (v, new_si) = parse_int(&date_str, si, 2)?;
                    day = v;
                    si = new_si;
                }
                'H' => {
                    let (v, new_si) = parse_int(&date_str, si, 2)?;
                    hour = v;
                    si = new_si;
                }
                'M' => {
                    let (v, new_si) = parse_int(&date_str, si, 2)?;
                    minute = v;
                    si = new_si;
                }
                'S' => {
                    let (v, new_si) = parse_int(&date_str, si, 2)?;
                    second = v;
                    si = new_si;
                }
                'f' => {
                    let (v, new_si) = parse_int(&date_str, si, 6)?;
                    microsecond = v;
                    si = new_si;
                }
                'y' => {
                    let (v, new_si) = parse_int(&date_str, si, 2)?;
                    year = if v >= 69 { 1900 + v } else { 2000 + v };
                    si = new_si;
                }
                'j' => {
                    let (_v, new_si) = parse_int(&date_str, si, 3)?;
                    si = new_si;
                }
                'p' => {
                    // AM/PM
                    let rest = &date_str[si..];
                    if rest.starts_with("PM") || rest.starts_with("pm") {
                        if hour < 12 {
                            hour += 12;
                        }
                        si += 2;
                    } else if rest.starts_with("AM") || rest.starts_with("am") {
                        if hour == 12 {
                            hour = 0;
                        }
                        si += 2;
                    }
                }
                'I' => {
                    // 12-hour clock
                    let (v, new_si) = parse_int(&date_str, si, 2)?;
                    hour = v;
                    si = new_si;
                }
                'b' | 'B' => {
                    // Month name (abbreviated or full)
                    let months = [
                        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct",
                        "nov", "dec",
                    ];
                    let rest = date_str[si..].to_lowercase();
                    let mut found = false;
                    for (i, m) in months.iter().enumerate() {
                        if rest.starts_with(m) {
                            month = (i + 1) as i64;
                            si += if code == 'b' {
                                3
                            } else {
                                // Full month names
                                let full = [
                                    "january",
                                    "february",
                                    "march",
                                    "april",
                                    "may",
                                    "june",
                                    "july",
                                    "august",
                                    "september",
                                    "october",
                                    "november",
                                    "december",
                                ];
                                if rest.starts_with(full[i]) {
                                    full[i].len()
                                } else {
                                    3
                                }
                            };
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(PyException::value_error(format!(
                            "time data '{}' does not match format '{}'",
                            date_str, fmt
                        )));
                    }
                }
                'a' | 'A' => {
                    // Day names (abbreviated or full) — consume but don't use (weekday is derived)
                    let day_abbrs = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
                    let day_fulls = [
                        "monday",
                        "tuesday",
                        "wednesday",
                        "thursday",
                        "friday",
                        "saturday",
                        "sunday",
                    ];
                    let rest = date_str[si..].to_lowercase();
                    let mut found = false;
                    for i in 0..7 {
                        if code == 'A' && rest.starts_with(day_fulls[i]) {
                            si += day_fulls[i].len();
                            found = true;
                            break;
                        } else if code == 'a' && rest.starts_with(day_abbrs[i]) {
                            si += 3;
                            found = true;
                            break;
                        } else if code == 'A' && rest.starts_with(day_abbrs[i]) {
                            si += 3;
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(PyException::value_error(format!(
                            "time data '{}' does not match format '{}'",
                            date_str, fmt
                        )));
                    }
                }
                'z' => {
                    // Timezone offset: +HHMM or -HHMM or +HH:MM
                    let rest = &date_str[si..];
                    if rest.starts_with('+') || rest.starts_with('-') {
                        let sign = if rest.starts_with('+') { 1 } else { -1 };
                        si += 1;
                        let (hh, new_si) = parse_int(&date_str, si, 2)?;
                        si = new_si;
                        // Skip optional colon
                        if si < str_bytes.len() && str_bytes[si] == b':' {
                            si += 1;
                        }
                        let (mm, new_si) = parse_int(&date_str, si, 2)?;
                        si = new_si;
                        let _ = sign * (hh * 3600 + mm * 60); // parsed but not stored
                    }
                }
                '%' => {
                    if str_bytes[si] != b'%' {
                        return Err(PyException::value_error(format!(
                            "time data '{}' does not match format '{}'",
                            date_str, fmt
                        )));
                    }
                    si += 1;
                }
                _ => {
                    // Skip unknown format codes
                }
            }
        } else {
            // Literal character — must match
            if fmt_bytes[fi] == str_bytes[si] {
                fi += 1;
                si += 1;
            } else {
                return Err(PyException::value_error(format!(
                    "time data '{}' does not match format '{}'",
                    date_str, fmt
                )));
            }
        }
    }

    Ok(make_datetime_instance(
        year,
        month,
        day,
        hour,
        minute,
        second,
        microsecond,
    ))
}

/// Parse an integer of up to `max_digits` from `s` starting at position `pos`.
fn parse_int(s: &str, pos: usize, max_digits: usize) -> PyResult<(i64, usize)> {
    let bytes = s.as_bytes();
    let mut end = pos;
    while end < bytes.len() && end - pos < max_digits && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == pos {
        return Err(PyException::value_error(format!(
            "unconverted data remains: '{}'",
            &s[pos..]
        )));
    }
    let val: i64 = s[pos..end]
        .parse()
        .map_err(|_| PyException::value_error("invalid integer"))?;
    Ok((val, end))
}

fn make_datetime_instance(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    microsecond: i64,
) -> PyObjectRef {
    let mut dt_ns = IndexMap::new();
    dt_ns.insert(
        CompactString::from("__add__"),
        make_builtin(datetime_add_dunder),
    );
    dt_ns.insert(
        CompactString::from("__sub__"),
        make_builtin(datetime_sub_dunder),
    );
    dt_ns.insert(CompactString::from("__eq__"), make_builtin(datetime_eq));
    dt_ns.insert(CompactString::from("__lt__"), make_builtin(datetime_lt));
    dt_ns.insert(CompactString::from("__le__"), make_builtin(datetime_le));
    dt_ns.insert(CompactString::from("__gt__"), make_builtin(datetime_gt));
    dt_ns.insert(CompactString::from("__ge__"), make_builtin(datetime_ge));
    let class = PyObject::class(CompactString::from("datetime"), vec![], dt_ns);
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
    install_datetime_methods(&inst, year, month, day, hour, minute, second, microsecond);
    inst
}

/// Install all datetime instance methods (isoformat, strftime, astimezone, etc.) on the given instance.
/// Called from both make_datetime_instance and __init__.
fn install_datetime_methods(
    inst: &PyObjectRef,
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    microsecond: i64,
) {
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__datetime__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("year"), PyObject::int(year));
        w.insert(CompactString::from("month"), PyObject::int(month));
        w.insert(CompactString::from("day"), PyObject::int(day));
        w.insert(CompactString::from("hour"), PyObject::int(hour));
        w.insert(CompactString::from("minute"), PyObject::int(minute));
        w.insert(CompactString::from("second"), PyObject::int(second));
        w.insert(
            CompactString::from("microsecond"),
            PyObject::int(microsecond),
        );
        w.insert(CompactString::from("tzinfo"), PyObject::none());

        // isoformat(sep='T') -> str
        let (y, mo, da, h, mi, s, us) = (year, month, day, hour, minute, second, microsecond);
        w.insert(
            CompactString::from("isoformat"),
            PyObject::native_closure("datetime.isoformat", move |args: &[PyObjectRef]| {
                let sep = if args.is_empty() {
                    "T".to_string()
                } else {
                    args[0].py_to_string()
                };
                let base = format!(
                    "{:04}-{:02}-{:02}{}{:02}:{:02}:{:02}",
                    y, mo, da, sep, h, mi, s
                );
                if us != 0 {
                    Ok(PyObject::str_val(CompactString::from(format!(
                        "{}.{:06}",
                        base, us
                    ))))
                } else {
                    Ok(PyObject::str_val(CompactString::from(base)))
                }
            }),
        );

        // strftime(format) -> str (using shared format_time with full format codes)
        let (y2, mo2, da2, h2, mi2, s2) = (year, month, day, hour, minute, second);
        let ord = ymd_to_ordinal(year, month, day);
        let wd = ((ord + 6) % 7) as i64; // 0=Mon
        let wd_for_fmt = wd;
        let yday_for_fmt = {
            let md = days_in_month(year);
            let mut yd = day;
            for i in 0..(month - 1) as usize {
                if i < 12 {
                    yd += md[i];
                }
            }
            yd
        };
        let us_for_fmt = microsecond;
        w.insert(
            CompactString::from("strftime"),
            PyObject::native_closure("datetime.strftime", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("strftime requires format string"));
                }
                let fmt = args[0].py_to_string();
                let result = format_time_us(
                    &fmt,
                    y2,
                    mo2,
                    da2,
                    h2,
                    mi2,
                    s2,
                    us_for_fmt,
                    wd_for_fmt,
                    yday_for_fmt,
                );
                Ok(PyObject::str_val(CompactString::from(result)))
            }),
        );

        // weekday() -> int (0=Monday, 6=Sunday)
        w.insert(
            CompactString::from("weekday"),
            PyObject::native_closure("datetime.weekday", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(wd))
            }),
        );

        // isocalendar() -> (iso_year, iso_week, iso_weekday)
        w.insert(
            CompactString::from("isocalendar"),
            PyObject::native_closure("datetime.isocalendar", move |_: &[PyObjectRef]| {
                let ordinal = ymd_to_ordinal(y, mo, da);
                let dow = (ordinal + 6) % 7; // 0=Monday
                                             // Find Thursday of the same ISO week
                let thu = ordinal + 3 - dow;
                // ISO year is the year containing that Thursday
                let (iso_year, _, _) = ordinal_to_ymd(thu);
                let jan1_ord = ymd_to_ordinal(iso_year, 1, 1);
                let jan1_dow = (jan1_ord + 6) % 7;
                // Monday of ISO week 1
                let iso_week1_mon = if jan1_dow <= 3 {
                    jan1_ord - jan1_dow
                } else {
                    jan1_ord + 7 - jan1_dow
                };
                let week_num = (ordinal - iso_week1_mon) / 7 + 1;
                Ok(PyObject::tuple(vec![
                    PyObject::int(iso_year),
                    PyObject::int(week_num),
                    PyObject::int(dow + 1), // Monday=1
                ]))
            }),
        );

        // isoweekday() -> int (1=Monday, 7=Sunday)
        let iwd = wd + 1;
        w.insert(
            CompactString::from("isoweekday"),
            PyObject::native_closure("datetime.isoweekday", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(iwd))
            }),
        );

        // date() -> date object
        w.insert(
            CompactString::from("date"),
            PyObject::native_closure("datetime.date", move |_: &[PyObjectRef]| {
                Ok(make_date_instance(y, mo, da))
            }),
        );

        // timestamp() -> float (POSIX timestamp)
        let ts = {
            let days_since_epoch = ymd_to_ordinal(year, month, day) - ymd_to_ordinal(1970, 1, 1);
            days_since_epoch as f64 * 86400.0
                + hour as f64 * 3600.0
                + minute as f64 * 60.0
                + second as f64
                + microsecond as f64 / 1_000_000.0
        };
        w.insert(
            CompactString::from("timestamp"),
            PyObject::native_closure("datetime.timestamp", move |_: &[PyObjectRef]| {
                Ok(PyObject::float(ts))
            }),
        );

        // __str__() / __repr__()
        let iso = if microsecond != 0 {
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
                year, month, day, hour, minute, second, microsecond
            )
        } else {
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                year, month, day, hour, minute, second
            )
        };

        w.insert(
            CompactString::from("__str__"),
            PyObject::native_closure("datetime.__str__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(&iso)))
            }),
        );
        w.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("datetime.__repr__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "datetime.datetime({}, {}, {}, {}, {}, {})",
                    y, mo, da, h, mi, s
                ))))
            }),
        );

        // timetuple() -> time.struct_time compatible tuple
        w.insert(
            CompactString::from("timetuple"),
            PyObject::native_closure("datetime.timetuple", move |_: &[PyObjectRef]| {
                Ok(PyObject::tuple(vec![
                    PyObject::int(y),
                    PyObject::int(mo),
                    PyObject::int(da),
                    PyObject::int(h),
                    PyObject::int(mi),
                    PyObject::int(s),
                    PyObject::int(wd),
                    PyObject::int(0),
                    PyObject::int(-1),
                ]))
            }),
        );

        // astimezone(tz) -> datetime converted to target timezone
        // This closure receives args from method call; self attrs read at call-time
        let inst_ref = inst.clone();
        w.insert(
            CompactString::from("astimezone"),
            PyObject::native_closure("datetime.astimezone", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(inst_ref.clone());
                }
                // Read source datetime fields from the instance
                let sy = inst_ref
                    .get_attr("year")
                    .and_then(|v| v.as_int())
                    .unwrap_or(1970);
                let smo = inst_ref
                    .get_attr("month")
                    .and_then(|v| v.as_int())
                    .unwrap_or(1);
                let sda = inst_ref
                    .get_attr("day")
                    .and_then(|v| v.as_int())
                    .unwrap_or(1);
                let sh = inst_ref
                    .get_attr("hour")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0);
                let smi = inst_ref
                    .get_attr("minute")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0);
                let ss = inst_ref
                    .get_attr("second")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0);
                let sus = inst_ref
                    .get_attr("microsecond")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0);

                // Get source timezone offset (0 if naive or UTC)
                let src_offset = inst_ref
                    .get_attr("tzinfo")
                    .and_then(|tz| tz.get_attr("_offset_seconds"))
                    .and_then(|v| v.to_float().ok())
                    .unwrap_or(0.0);

                let target_tz = &args[0];
                let target_offset = target_tz
                    .get_attr("_offset_seconds")
                    .and_then(|v| v.to_float().ok())
                    .unwrap_or(0.0);

                // Convert to UTC epoch seconds, then to target timezone
                let epoch_days = ymd_to_ordinal(sy, smo, sda) - ymd_to_ordinal(1970, 1, 1);
                let utc_secs = epoch_days as f64 * 86400.0
                    + sh as f64 * 3600.0
                    + smi as f64 * 60.0
                    + ss as f64
                    + sus as f64 / 1_000_000.0
                    - src_offset; // subtract source offset to get UTC

                let local_secs = utc_secs + target_offset;
                let total_days = (local_secs / 86400.0).floor() as i64;
                let day_secs = local_secs - total_days as f64 * 86400.0;
                let ord = ymd_to_ordinal(1970, 1, 1) + total_days;
                let (ny, nm, nd) = ordinal_to_ymd(ord);
                let nh = (day_secs / 3600.0).floor() as i64;
                let nmi = ((day_secs - nh as f64 * 3600.0) / 60.0).floor() as i64;
                let ns = (day_secs - nh as f64 * 3600.0 - nmi as f64 * 60.0).floor() as i64;
                let nus = ((day_secs.fract()) * 1_000_000.0).round() as i64;
                Ok(make_datetime_instance(ny, nm, nd, nh, nmi, ns, nus))
            }),
        );

        // utcoffset() -> timedelta or None
        let inst_for_utcoff = inst.clone();
        w.insert(
            CompactString::from("utcoffset"),
            PyObject::native_closure("datetime.utcoffset", move |_: &[PyObjectRef]| {
                if let Some(tz) = inst_for_utcoff.get_attr("tzinfo") {
                    if !matches!(tz.payload, PyObjectPayload::None) {
                        if let Some(utcoff_fn) = tz.get_attr("utcoffset") {
                            match &utcoff_fn.payload {
                                PyObjectPayload::NativeFunction(nf) => {
                                    return (nf.func)(&[inst_for_utcoff.clone()])
                                }
                                PyObjectPayload::NativeClosure(nc) => {
                                    return (nc.func)(&[inst_for_utcoff.clone()])
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // replace(**kwargs) -> datetime with replaced fields
        let (ry, rmo, rda, rh, rmi, rs, rus) =
            (year, month, day, hour, minute, second, microsecond);
        w.insert(
            CompactString::from("replace"),
            PyObject::native_closure("datetime.replace", move |args: &[PyObjectRef]| {
                let mut ny = ry;
                let mut nmo = rmo;
                let mut nda = rda;
                let mut nh = rh;
                let mut nmi = rmi;
                let mut ns = rs;
                let mut nus = rus;
                // Accept kwargs dict
                if let Some(last) = args.last() {
                    if let PyObjectPayload::Dict(kw) = &last.payload {
                        let r = kw.read();
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("year"))) {
                            ny = v.as_int().unwrap_or(ny);
                        }
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("month")))
                        {
                            nmo = v.as_int().unwrap_or(nmo);
                        }
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("day"))) {
                            nda = v.as_int().unwrap_or(nda);
                        }
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("hour"))) {
                            nh = v.as_int().unwrap_or(nh);
                        }
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("minute")))
                        {
                            nmi = v.as_int().unwrap_or(nmi);
                        }
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("second")))
                        {
                            ns = v.as_int().unwrap_or(ns);
                        }
                        if let Some(v) =
                            r.get(&HashableKey::str_key(CompactString::from("microsecond")))
                        {
                            nus = v.as_int().unwrap_or(nus);
                        }
                    }
                }
                // Also accept positional args: year, month, day, hour, minute, second, microsecond
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
                    if args.len() > 3 {
                        nh = args[3].as_int().unwrap_or(nh);
                    }
                    if args.len() > 4 {
                        nmi = args[4].as_int().unwrap_or(nmi);
                    }
                    if args.len() > 5 {
                        ns = args[5].as_int().unwrap_or(ns);
                    }
                    if args.len() > 6 {
                        nus = args[6].as_int().unwrap_or(nus);
                    }
                }
                Ok(make_datetime_instance(ny, nmo, nda, nh, nmi, ns, nus))
            }),
        );

        // time() -> time object (extract time component)
        let (th, tmi, ts, tus) = (hour, minute, second, microsecond);
        w.insert(
            CompactString::from("time"),
            PyObject::native_closure("datetime.time", move |_: &[PyObjectRef]| {
                make_time_instance(th, tmi, ts, tus)
            }),
        );

        // ctime() -> str (C format: "Mon Jan  1 00:00:00 2024")
        let (cy, cmo, cda, ch2, cmi2, cs2) = (year, month, day, hour, minute, second);
        let cwd = wd;
        w.insert(
            CompactString::from("ctime"),
            PyObject::native_closure("datetime.ctime", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "{} {} {:2} {:02}:{:02}:{:02} {:04}",
                    DAY_NAMES_ABBR[cwd as usize % 7],
                    MONTH_NAMES_ABBR[(cmo - 1) as usize % 12],
                    cda,
                    ch2,
                    cmi2,
                    cs2,
                    cy
                ))))
            }),
        );

        // toordinal() -> int
        let to_y = year;
        let to_m = month;
        let to_d = day;
        w.insert(
            CompactString::from("toordinal"),
            PyObject::native_closure("datetime.toordinal", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(ymd_to_ordinal(to_y, to_m, to_d)))
            }),
        );

        // dst() -> timedelta or None (delegates to tzinfo.dst)
        let inst_for_dst = inst.clone();
        w.insert(
            CompactString::from("dst"),
            PyObject::native_closure("datetime.dst", move |_: &[PyObjectRef]| {
                if let Some(tz) = inst_for_dst.get_attr("tzinfo") {
                    if !matches!(tz.payload, PyObjectPayload::None) {
                        if let Some(dst_fn) = tz.get_attr("dst") {
                            match &dst_fn.payload {
                                PyObjectPayload::NativeFunction(nf) => {
                                    return (nf.func)(&[inst_for_dst.clone()])
                                }
                                PyObjectPayload::NativeClosure(nc) => {
                                    return (nc.func)(&[inst_for_dst.clone()])
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // tzname() -> str or None (delegates to tzinfo.tzname)
        let inst_for_tzname = inst.clone();
        w.insert(
            CompactString::from("tzname"),
            PyObject::native_closure("datetime.tzname", move |_: &[PyObjectRef]| {
                if let Some(tz) = inst_for_tzname.get_attr("tzinfo") {
                    if !matches!(tz.payload, PyObjectPayload::None) {
                        if let Some(tzn_fn) = tz.get_attr("tzname") {
                            match &tzn_fn.payload {
                                PyObjectPayload::NativeFunction(nf) => {
                                    return (nf.func)(&[inst_for_tzname.clone()])
                                }
                                PyObjectPayload::NativeClosure(nc) => {
                                    return (nc.func)(&[inst_for_tzname.clone()])
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // __hash__() for use in sets/dicts
        let (hy, hmo, hda, hh, hmi, hs, hus) =
            (year, month, day, hour, minute, second, microsecond);
        w.insert(
            CompactString::from("__hash__"),
            PyObject::native_closure("datetime.__hash__", move |_: &[PyObjectRef]| {
                let hash = hy * 13 + hmo * 7 + hda * 3 + hh * 31 + hmi * 37 + hs * 41 + hus;
                Ok(PyObject::int(hash))
            }),
        );
    }
}

fn make_time_instance(
    hour: i64,
    minute: i64,
    second: i64,
    microsecond: i64,
) -> PyResult<PyObjectRef> {
    let class = PyObject::class(CompactString::from("time"), vec![], IndexMap::new());
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
            CompactString::from("__datetime__"),
            PyObject::bool_val(true),
        );
        w.insert(
            CompactString::from("__time_only__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("hour"), PyObject::int(hour));
        w.insert(CompactString::from("minute"), PyObject::int(minute));
        w.insert(CompactString::from("second"), PyObject::int(second));
        w.insert(
            CompactString::from("microsecond"),
            PyObject::int(microsecond),
        );
        w.insert(CompactString::from("tzinfo"), PyObject::none());

        // isoformat() -> str
        let (h, mi, s, us) = (hour, minute, second, microsecond);
        w.insert(
            CompactString::from("isoformat"),
            PyObject::native_closure("time.isoformat", move |_: &[PyObjectRef]| {
                let base = format!("{:02}:{:02}:{:02}", h, mi, s);
                if us != 0 {
                    Ok(PyObject::str_val(CompactString::from(format!(
                        "{}.{:06}",
                        base, us
                    ))))
                } else {
                    Ok(PyObject::str_val(CompactString::from(base)))
                }
            }),
        );

        // strftime(format) -> str
        w.insert(
            CompactString::from("strftime"),
            PyObject::native_closure("time.strftime", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("strftime requires format string"));
                }
                let fmt = args[0].py_to_string();
                let result = format_time_us(&fmt, 1900, 1, 1, h, mi, s, us, 0, 1);
                Ok(PyObject::str_val(CompactString::from(result)))
            }),
        );

        // replace(**kwargs)
        let (rh, rmi, rs, rus) = (hour, minute, second, microsecond);
        w.insert(
            CompactString::from("replace"),
            PyObject::native_closure("time.replace", move |args: &[PyObjectRef]| {
                let mut nh = rh;
                let mut nmi = rmi;
                let mut ns = rs;
                let mut nus = rus;
                if let Some(last) = args.last() {
                    if let PyObjectPayload::Dict(kw) = &last.payload {
                        let r = kw.read();
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("hour"))) {
                            nh = v.as_int().unwrap_or(nh);
                        }
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("minute")))
                        {
                            nmi = v.as_int().unwrap_or(nmi);
                        }
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("second")))
                        {
                            ns = v.as_int().unwrap_or(ns);
                        }
                        if let Some(v) =
                            r.get(&HashableKey::str_key(CompactString::from("microsecond")))
                        {
                            nus = v.as_int().unwrap_or(nus);
                        }
                    }
                }
                if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
                    if !args.is_empty() {
                        nh = args[0].as_int().unwrap_or(nh);
                    }
                    if args.len() > 1 {
                        nmi = args[1].as_int().unwrap_or(nmi);
                    }
                    if args.len() > 2 {
                        ns = args[2].as_int().unwrap_or(ns);
                    }
                    if args.len() > 3 {
                        nus = args[3].as_int().unwrap_or(nus);
                    }
                }
                make_time_instance(nh, nmi, ns, nus)
            }),
        );

        // __str__() / __repr__()
        let iso_str = if microsecond != 0 {
            format!("{:02}:{:02}:{:02}.{:06}", hour, minute, second, microsecond)
        } else {
            format!("{:02}:{:02}:{:02}", hour, minute, second)
        };
        let iso_clone = iso_str.clone();
        w.insert(
            CompactString::from("__str__"),
            PyObject::native_closure("time.__str__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(&iso_str)))
            }),
        );
        w.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("time.__repr__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "datetime.time({})",
                    iso_clone
                ))))
            }),
        );

        // __eq__, __lt__, __le__, __gt__, __ge__
        let time_key = hour * 3600_000_000 + minute * 60_000_000 + second * 1_000_000 + microsecond;
        w.insert(
            CompactString::from("__eq__"),
            PyObject::native_closure("time.__eq__", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::bool_val(false));
                }
                let other = &args[0];
                let ok = other
                    .get_attr("hour")
                    .and_then(|h| h.as_int())
                    .and_then(|oh| {
                        let om = other
                            .get_attr("minute")
                            .and_then(|v| v.as_int())
                            .unwrap_or(-1);
                        let os = other
                            .get_attr("second")
                            .and_then(|v| v.as_int())
                            .unwrap_or(-1);
                        let ou = other
                            .get_attr("microsecond")
                            .and_then(|v| v.as_int())
                            .unwrap_or(-1);
                        Some(oh * 3600_000_000 + om * 60_000_000 + os * 1_000_000 + ou)
                    })
                    .unwrap_or(-1);
                Ok(PyObject::bool_val(time_key == ok))
            }),
        );

        // __hash__
        w.insert(
            CompactString::from("__hash__"),
            PyObject::native_closure("time.__hash__", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(time_key))
            }),
        );
    }
    Ok(inst)
}

fn datetime_time_obj(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let hour = if !args.is_empty() {
        args[0].to_int()?
    } else {
        0
    };
    let minute = if args.len() > 1 { args[1].to_int()? } else { 0 };
    let second = if args.len() > 2 { args[2].to_int()? } else { 0 };
    let microsecond = if args.len() > 3 { args[3].to_int()? } else { 0 };
    make_time_instance(hour, minute, second, microsecond)
}

fn datetime_timedelta(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // timedelta(days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0)
    let mut days = 0i64;
    let mut seconds = 0i64;
    let mut microseconds = 0i64;
    if !args.is_empty() {
        days = args[0].to_int().unwrap_or(0);
    }
    if args.len() > 1 {
        seconds = args[1].to_int().unwrap_or(0);
    }
    if args.len() > 2 {
        microseconds = args[2].to_int().unwrap_or(0);
    }
    // Check for kwargs dict as last arg
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("days"))) {
                days = v.as_int().unwrap_or(0);
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("seconds"))) {
                seconds = v.as_int().unwrap_or(0);
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("microseconds"))) {
                microseconds = v.as_int().unwrap_or(0);
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("milliseconds"))) {
                microseconds += v.as_int().unwrap_or(0) * 1000;
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("minutes"))) {
                seconds += v.as_int().unwrap_or(0) * 60;
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("hours"))) {
                seconds += v.as_int().unwrap_or(0) * 3600;
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("weeks"))) {
                days += v.as_int().unwrap_or(0) * 7;
            }
        }
    }
    // Normalize: carry microseconds → seconds → days
    seconds += microseconds / 1_000_000;
    microseconds %= 1_000_000;
    if microseconds < 0 {
        microseconds += 1_000_000;
        seconds -= 1;
    }
    days += seconds / 86400;
    seconds %= 86400;
    if seconds < 0 {
        seconds += 86400;
        days -= 1;
    }
    let total_secs = days as f64 * 86400.0 + seconds as f64 + microseconds as f64 / 1_000_000.0;
    make_timedelta(days, seconds, microseconds, total_secs)
}

fn install_date_instance_attrs(
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

fn make_date_instance(year: i64, month: i64, day: i64) -> PyObjectRef {
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

fn date_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn date_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn date_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) == date_ordinal(&args[1]),
    ))
}

fn date_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) < date_ordinal(&args[1]),
    ))
}

fn date_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) <= date_ordinal(&args[1]),
    ))
}

fn date_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) > date_ordinal(&args[1]),
    ))
}

fn date_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(
        date_ordinal(&args[0]) >= date_ordinal(&args[1]),
    ))
}
