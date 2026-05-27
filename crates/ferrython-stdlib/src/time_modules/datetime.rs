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
    days_in_month, days_to_ymd, format_time_us, ordinal_to_ymd, ymd_to_ordinal, DAY_NAMES_ABBR,
    MONTH_NAMES_ABBR,
};

mod classmethods;
mod date;
mod instance;
mod time_obj;
mod timedelta;
mod timezone;

use classmethods::{
    date_fromisoformat, date_fromordinal, date_today, datetime_combine, datetime_fromisoformat,
    datetime_fromordinal, datetime_fromtimestamp, datetime_now, datetime_strptime,
};
use date::{
    date_add, date_eq, date_ge, date_gt, date_le, date_lt, date_sub, install_date_instance_attrs,
    make_date_instance,
};
use instance::{install_datetime_methods, make_datetime_instance};
use time_obj::{datetime_time_obj, make_time_instance};
use timedelta::{
    datetime_add_dunder, datetime_eq, datetime_ge, datetime_gt, datetime_le, datetime_lt,
    datetime_sub_dunder, datetime_timedelta, make_timedelta,
};
use timezone::make_timezone_utc;

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
