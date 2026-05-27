use super::*;

pub(super) fn make_datetime_instance(
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

/// Install all datetime instance methods (isoformat, strftime, astimezone, etc.) on the given instance.
/// Called from both make_datetime_instance and __init__.
pub(super) fn install_datetime_methods(
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
