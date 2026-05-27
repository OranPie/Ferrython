use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    check_args_min, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

fn append_tz_offset(base: &str, tzinfo: &Option<PyObjectRef>) -> String {
    if let Some(ref tz) = tzinfo {
        if !matches!(&tz.payload, PyObjectPayload::None) {
            if let PyObjectPayload::Instance(ref tz_inst) = tz.payload {
                let tz_attrs = tz_inst.attrs.read();
                let offset_secs = tz_attrs
                    .get("_offset_seconds")
                    .and_then(|v| match &v.payload {
                        PyObjectPayload::Float(f) => Some(*f as i64),
                        PyObjectPayload::Int(i) => i.to_i64(),
                        _ => None,
                    })
                    .unwrap_or(0);
                let sign = if offset_secs < 0 { '-' } else { '+' };
                let abs_secs = offset_secs.unsigned_abs();
                let oh = abs_secs / 3600;
                let om = (abs_secs % 3600) / 60;
                return format!("{}{}{:02}:{:02}", base, sign, oh, om);
            }
        }
    }
    base.to_string()
}

pub(crate) fn call_datetime_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    let year = attrs.get("year").and_then(|v| v.as_int()).unwrap_or(1970);
    let month = attrs.get("month").and_then(|v| v.as_int()).unwrap_or(1);
    let day = attrs.get("day").and_then(|v| v.as_int()).unwrap_or(1);
    let hour = attrs.get("hour").and_then(|v| v.as_int()).unwrap_or(0);
    let minute = attrs.get("minute").and_then(|v| v.as_int()).unwrap_or(0);
    let second = attrs.get("second").and_then(|v| v.as_int()).unwrap_or(0);
    let microsecond = attrs
        .get("microsecond")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    let date_only = attrs.contains_key("__date_only__");
    let time_only = attrs.contains_key("__time_only__");
    let tzinfo = attrs.get("tzinfo").cloned();
    drop(attrs);
    match method {
        "strftime" => {
            check_args_min("strftime", args, 1)?;
            let fmt = args[0].py_to_string();
            let result =
                datetime_strftime(&fmt, year, month, day, hour, minute, second, microsecond);
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        "isoformat" => {
            if time_only {
                let s = if microsecond != 0 {
                    format!("{:02}:{:02}:{:02}.{:06}", hour, minute, second, microsecond)
                } else {
                    format!("{:02}:{:02}:{:02}", hour, minute, second)
                };
                Ok(PyObject::str_val(CompactString::from(&s)))
            } else if date_only {
                let s = format!("{:04}-{:02}-{:02}", year, month, day);
                Ok(PyObject::str_val(CompactString::from(&s)))
            } else {
                let sep = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    "T".to_string()
                };
                let base = if microsecond != 0 {
                    format!(
                        "{:04}-{:02}-{:02}{}{:02}:{:02}:{:02}.{:06}",
                        year, month, day, sep, hour, minute, second, microsecond
                    )
                } else {
                    format!(
                        "{:04}-{:02}-{:02}{}{:02}:{:02}:{:02}",
                        year, month, day, sep, hour, minute, second
                    )
                };
                let s = append_tz_offset(&base, &tzinfo);
                Ok(PyObject::str_val(CompactString::from(&s)))
            }
        }
        "date" => {
            let cls = PyObject::class(CompactString::from("date"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__datetime__"), PyObject::bool_val(true));
                w.insert(intern_or_new("__date_only__"), PyObject::bool_val(true));
                w.insert(CompactString::from("year"), PyObject::int(year));
                w.insert(CompactString::from("month"), PyObject::int(month));
                w.insert(CompactString::from("day"), PyObject::int(day));
            }
            Ok(inst_obj)
        }
        "time" => {
            let cls = PyObject::class(CompactString::from("time"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__datetime__"), PyObject::bool_val(true));
                w.insert(intern_or_new("__time_only__"), PyObject::bool_val(true));
                w.insert(CompactString::from("hour"), PyObject::int(hour));
                w.insert(CompactString::from("minute"), PyObject::int(minute));
                w.insert(CompactString::from("second"), PyObject::int(second));
                w.insert(
                    CompactString::from("microsecond"),
                    PyObject::int(microsecond),
                );
            }
            Ok(inst_obj)
        }
        "replace" => {
            // replace(year=None, month=None, ...) via kwargs dict
            let mut ny = year;
            let mut nm = month;
            let mut nd = day;
            let mut nh = hour;
            let mut nmi = minute;
            let mut ns = second;
            let mut nus = microsecond;
            if let Some(kw) = args.last() {
                if let PyObjectPayload::Dict(map) = &kw.payload {
                    let r = map.read();
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("year"))) {
                        ny = v.as_int().unwrap_or(ny);
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("month"))) {
                        nm = v.as_int().unwrap_or(nm);
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("day"))) {
                        nd = v.as_int().unwrap_or(nd);
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("hour"))) {
                        nh = v.as_int().unwrap_or(nh);
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("minute"))) {
                        nmi = v.as_int().unwrap_or(nmi);
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("second"))) {
                        ns = v.as_int().unwrap_or(ns);
                    }
                    if let Some(v) =
                        r.get(&HashableKey::str_key(CompactString::from("microsecond")))
                    {
                        nus = v.as_int().unwrap_or(nus);
                    }
                }
            }
            let cls = PyObject::class(CompactString::from("datetime"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__datetime__"), PyObject::bool_val(true));
                w.insert(CompactString::from("year"), PyObject::int(ny));
                w.insert(CompactString::from("month"), PyObject::int(nm));
                w.insert(CompactString::from("day"), PyObject::int(nd));
                w.insert(CompactString::from("hour"), PyObject::int(nh));
                w.insert(CompactString::from("minute"), PyObject::int(nmi));
                w.insert(CompactString::from("second"), PyObject::int(ns));
                w.insert(CompactString::from("microsecond"), PyObject::int(nus));
            }
            Ok(inst_obj)
        }
        "timestamp" => {
            // Rough UNIX timestamp (ignoring timezone)
            let days = ymd_to_days(year, month, day) - 719468;
            let total = days as f64 * 86400.0
                + hour as f64 * 3600.0
                + minute as f64 * 60.0
                + second as f64
                + microsecond as f64 / 1_000_000.0;
            Ok(PyObject::float(total))
        }
        "weekday" => {
            let days = ymd_to_days(year, month, day);
            Ok(PyObject::int((days + 2) % 7)) // Monday=0
        }
        "isoweekday" => {
            let days = ymd_to_days(year, month, day);
            let wd = (days + 2) % 7; // Monday=0
            Ok(PyObject::int(wd + 1)) // Monday=1, Sunday=7
        }
        "toordinal" => {
            // Proleptic Gregorian ordinal: Jan 1 of year 1 = ordinal 1
            let days = ymd_to_days(year, month, day);
            // ymd_to_days returns civil days from epoch; year 1, Jan 1 ordinal = 1
            // Offset: ymd_to_days(1,1,1) gives the civil day number for year 1 Jan 1
            let epoch = ymd_to_days(1, 1, 1);
            Ok(PyObject::int(days - epoch + 1))
        }
        "ctime" => {
            let weekday_short = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
            let month_short = [
                "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                "Dec",
            ];
            let days = ymd_to_days(year, month, day);
            let wday = ((days + 2) % 7) as usize;
            let s = format!(
                "{} {} {:2} {:02}:{:02}:{:02} {:04}",
                weekday_short.get(wday).unwrap_or(&""),
                month_short.get(month as usize).unwrap_or(&""),
                day,
                hour,
                minute,
                second,
                year
            );
            Ok(PyObject::str_val(CompactString::from(&s)))
        }
        "timetuple" => {
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            let month_days = [
                31,
                if leap { 29 } else { 28 },
                31,
                30,
                31,
                30,
                31,
                31,
                30,
                31,
                30,
                31,
            ];
            let yday: i64 = month_days[..(month - 1) as usize]
                .iter()
                .map(|&d| d as i64)
                .sum::<i64>()
                + day;
            Ok(PyObject::tuple(vec![
                PyObject::int(year),
                PyObject::int(month),
                PyObject::int(day),
                PyObject::int(hour),
                PyObject::int(minute),
                PyObject::int(second),
                PyObject::int((ymd_to_days(year, month, day) + 2) % 7),
                PyObject::int(yday),
                PyObject::int(-1),
            ]))
        }
        "isocalendar" => {
            // ISO calendar: (year, week, weekday) where Monday=1, Sunday=7
            let days = ymd_to_days(year, month, day);
            let dow = ((days + 2) % 7 + 7) % 7; // 0=Monday
                                                // Find Thursday of the same ISO week
            let thu = days + 3 - dow;
            // ISO year is the year containing that Thursday
            let (iso_year, _, _) = days_to_ymd_civil(thu);
            let jan1_of_iso_year = ymd_to_days(iso_year, 1, 1);
            let jan1_dow = ((jan1_of_iso_year + 2) % 7 + 7) % 7;
            // Monday of ISO week 1
            let iso_week1_mon = if jan1_dow <= 3 {
                jan1_of_iso_year - jan1_dow
            } else {
                jan1_of_iso_year + 7 - jan1_dow
            };
            let week_num = (days - iso_week1_mon) / 7 + 1;
            Ok(PyObject::tuple(vec![
                PyObject::int(iso_year),
                PyObject::int(week_num),
                PyObject::int(dow + 1), // Monday=1
            ]))
        }
        "__str__" | "__repr__" => {
            if time_only {
                let s = format!("{:02}:{:02}:{:02}", hour, minute, second);
                Ok(PyObject::str_val(CompactString::from(&s)))
            } else if date_only {
                let s = format!("{:04}-{:02}-{:02}", year, month, day);
                Ok(PyObject::str_val(CompactString::from(&s)))
            } else {
                let base = format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    year, month, day, hour, minute, second
                );
                let s = append_tz_offset(&base, &tzinfo);
                Ok(PyObject::str_val(CompactString::from(&s)))
            }
        }
        "astimezone" => {
            // Stub: return self (datetime with same values)
            let cls = PyObject::class(CompactString::from("datetime"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__datetime__"), PyObject::bool_val(true));
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
            }
            Ok(inst_obj)
        }
        "utcoffset" => {
            if let Some(ref tz) = tzinfo {
                if !matches!(&tz.payload, PyObjectPayload::None) {
                    if let PyObjectPayload::Instance(ref tz_inst) = tz.payload {
                        let tz_attrs = tz_inst.attrs.read();
                        let offset_secs = tz_attrs
                            .get("_offset_seconds")
                            .and_then(|v| match &v.payload {
                                PyObjectPayload::Float(f) => Some(*f as i64),
                                PyObjectPayload::Int(i) => i.to_i64(),
                                _ => None,
                            })
                            .unwrap_or(0);
                        let td_cls = PyObject::class(
                            CompactString::from("timedelta"),
                            vec![],
                            IndexMap::new(),
                        );
                        let td = PyObject::instance(td_cls);
                        if let PyObjectPayload::Instance(ref d) = td.payload {
                            let mut w = d.attrs.write();
                            w.insert(intern_or_new("__timedelta__"), PyObject::bool_val(true));
                            w.insert(CompactString::from("days"), PyObject::int(0));
                            w.insert(CompactString::from("seconds"), PyObject::int(offset_secs));
                            w.insert(CompactString::from("microseconds"), PyObject::int(0));
                            w.insert(
                                CompactString::from("_total_seconds"),
                                PyObject::float(offset_secs as f64),
                            );
                        }
                        return Ok(td);
                    }
                }
            }
            Ok(PyObject::none())
        }
        "tzname" => {
            if let Some(ref tz) = tzinfo {
                if !matches!(&tz.payload, PyObjectPayload::None) {
                    if let PyObjectPayload::Instance(ref tz_inst) = tz.payload {
                        let tz_attrs = tz_inst.attrs.read();
                        if let Some(name) = tz_attrs.get("_name") {
                            return Ok(name.clone());
                        }
                    }
                }
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'datetime' object has no attribute '{}'",
            method
        ))),
    }
}

pub(crate) fn call_timedelta_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    _args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    match method {
        "total_seconds" => Ok(attrs
            .get("_total_seconds")
            .cloned()
            .unwrap_or_else(|| PyObject::float(0.0))),
        "__str__" | "__repr__" => {
            let days = attrs.get("days").and_then(|v| v.as_int()).unwrap_or(0);
            let secs = attrs.get("seconds").and_then(|v| v.as_int()).unwrap_or(0);
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            let result = if days != 0 {
                format!(
                    "{} day{}, {}:{:02}:{:02}",
                    days,
                    if days.abs() != 1 { "s" } else { "" },
                    h,
                    m,
                    s
                )
            } else {
                format!("{}:{:02}:{:02}", h, m, s)
            };
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        "__neg__" => {
            let total_us = attrs.get("_total_us").and_then(|v| v.as_int()).unwrap_or(0);
            let neg = -total_us;
            let days = neg / 86_400_000_000;
            let rem = neg % 86_400_000_000;
            let seconds = rem / 1_000_000;
            let microseconds = rem % 1_000_000;
            let total = neg as f64 / 1_000_000.0;
            drop(attrs);
            let cls = PyObject::class(CompactString::from("timedelta"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__timedelta__"), PyObject::bool_val(true));
                w.insert(CompactString::from("days"), PyObject::int(days));
                w.insert(CompactString::from("seconds"), PyObject::int(seconds));
                w.insert(
                    CompactString::from("microseconds"),
                    PyObject::int(microseconds),
                );
                w.insert(CompactString::from("total_seconds"), PyObject::float(total));
                w.insert(CompactString::from("_total_us"), PyObject::int(neg));
            }
            Ok(inst_obj)
        }
        _ => Err(PyException::attribute_error(format!(
            "'timedelta' object has no attribute '{}'",
            method
        ))),
    }
}

pub(super) fn datetime_strftime(
    fmt: &str,
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    _microsecond: i64,
) -> String {
    let weekday_names = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    let weekday_short = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let month_names = [
        "",
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month_short = [
        "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days_civil = ymd_to_days(year, month, day);
    let wday = ((days_civil + 2) % 7) as usize; // Monday=0
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_lengths = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let yday: i64 = month_lengths[..(month - 1) as usize]
        .iter()
        .map(|&d| d as i64)
        .sum::<i64>()
        + day;

    let mut result = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.next() {
                Some('Y') => result.push_str(&format!("{:04}", year)),
                Some('y') => result.push_str(&format!("{:02}", year % 100)),
                Some('m') => result.push_str(&format!("{:02}", month)),
                Some('d') => result.push_str(&format!("{:02}", day)),
                Some('H') => result.push_str(&format!("{:02}", hour)),
                Some('I') => result.push_str(&format!(
                    "{:02}",
                    if hour % 12 == 0 { 12 } else { hour % 12 }
                )),
                Some('M') => result.push_str(&format!("{:02}", minute)),
                Some('S') => result.push_str(&format!("{:02}", second)),
                Some('f') => result.push_str(&format!("{:06}", _microsecond)),
                Some('p') => result.push_str(if hour < 12 { "AM" } else { "PM" }),
                Some('A') => result.push_str(weekday_names.get(wday).unwrap_or(&"")),
                Some('a') => result.push_str(weekday_short.get(wday).unwrap_or(&"")),
                Some('B') => result.push_str(month_names.get(month as usize).unwrap_or(&"")),
                Some('b') | Some('h') => {
                    result.push_str(month_short.get(month as usize).unwrap_or(&""))
                }
                Some('w') => result.push_str(&format!("{}", (wday + 1) % 7)), // Sunday=0
                Some('j') => result.push_str(&format!("{:03}", yday)),
                Some('c') => result.push_str(&format!(
                    "{} {} {:2} {:02}:{:02}:{:02} {:04}",
                    weekday_short.get(wday).unwrap_or(&""),
                    month_short.get(month as usize).unwrap_or(&""),
                    day,
                    hour,
                    minute,
                    second,
                    year
                )),
                Some('x') => result.push_str(&format!("{:02}/{:02}/{:02}", month, day, year % 100)),
                Some('X') => result.push_str(&format!("{:02}:{:02}:{:02}", hour, minute, second)),
                Some('%') => result.push('%'),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some(c) => {
                    result.push('%');
                    result.push(c);
                }
                None => result.push('%'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

pub(super) fn ymd_to_days(year: i64, month: i64, day: i64) -> i64 {
    // Inverse of days_to_ymd (Hinnant civil_from_days)
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe
}

fn days_to_ymd_civil(z: i64) -> (i64, i64, i64) {
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
