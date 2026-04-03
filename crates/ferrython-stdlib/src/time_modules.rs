//! Time and datetime stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, InstanceData,
    make_module, make_builtin, check_args,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

pub fn create_time_module() -> PyObjectRef {
    make_module("time", vec![
        ("time", make_builtin(time_time)),
        ("sleep", make_builtin(time_sleep)),
        ("monotonic", make_builtin(time_monotonic)),
        ("perf_counter", make_builtin(time_monotonic)),
        ("perf_counter_ns", make_builtin(|_args| {
            use std::time::Instant;
            static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            let start = START.get_or_init(Instant::now);
            Ok(PyObject::int(start.elapsed().as_nanos() as i64))
        })),
        ("time_ns", make_builtin(|_args| {
            use std::time::SystemTime;
            let dur = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
            Ok(PyObject::int(dur.as_nanos() as i64))
        })),
        ("process_time", make_builtin(time_monotonic)),
        ("strftime", make_builtin(time_strftime)),
        ("localtime", make_builtin(time_localtime)),
        ("gmtime", make_builtin(time_localtime)),
    ])
}

fn time_time(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::SystemTime;
    let dur = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    Ok(PyObject::float(dur.as_secs_f64()))
}

fn time_sleep(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("time.sleep", args, 1)?;
    let secs = args[0].to_float()?;
    if secs < 0.0 { return Err(PyException::value_error("sleep length must be non-negative")); }
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    Ok(PyObject::none())
}

fn time_monotonic(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::Instant;
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    Ok(PyObject::float(start.elapsed().as_secs_f64()))
}

fn time_strftime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("strftime requires a format string")); }
    let fmt = args[0].py_to_string();
    // Simplified strftime — handle common format codes
    use std::time::SystemTime;
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    // Basic time decomposition (UTC)
    let s = (secs % 60) as u32;
    let m = ((secs / 60) % 60) as u32;
    let h = ((secs / 3600) % 24) as u32;
    let days = (secs / 86400) as i64;
    // Days since epoch → year/month/day (simplified)
    let mut y: i64 = 1970;
    let mut remaining = days;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mon = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md as i64 { mon = i; break; }
        remaining -= md as i64;
    }
    let day = remaining + 1;
    let result = fmt
        .replace("%Y", &format!("{:04}", y))
        .replace("%m", &format!("{:02}", mon + 1))
        .replace("%d", &format!("{:02}", day))
        .replace("%H", &format!("{:02}", h))
        .replace("%M", &format!("{:02}", m))
        .replace("%S", &format!("{:02}", s))
        .replace("%%", "%");
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn time_localtime(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Return a basic time tuple (year, month, day, hour, minute, second, weekday, yearday, dst)
    use std::time::SystemTime;
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    let s = (secs % 60) as i64;
    let m = ((secs / 60) % 60) as i64;
    let h = ((secs / 3600) % 24) as i64;
    let days = (secs / 86400) as i64;
    let mut y: i64 = 1970;
    let mut remaining = days;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mon = 1i64;
    for &md in &month_days {
        if remaining < md as i64 { break; }
        remaining -= md as i64;
        mon += 1;
    }
    let day = remaining + 1;
    let wday = ((days + 3) % 7) as i64; // 0=Monday for time.struct_time
    let yday = {
        let mut yd = day;
        for i in 0..(mon - 1) as usize { yd += month_days[i] as i64; }
        yd
    };
    Ok(PyObject::tuple(vec![
        PyObject::int(y), PyObject::int(mon), PyObject::int(day),
        PyObject::int(h), PyObject::int(m), PyObject::int(s),
        PyObject::int(wday), PyObject::int(yday), PyObject::int(-1),
    ]))
}

// ── random module (basic) ──


pub fn create_datetime_module() -> PyObjectRef {
    // Build datetime class with constructor and class methods
    let mut dt_ns = IndexMap::new();
    dt_ns.insert(CompactString::from("now"), make_builtin(datetime_now));
    dt_ns.insert(CompactString::from("today"), make_builtin(datetime_now));
    dt_ns.insert(CompactString::from("utcnow"), make_builtin(datetime_now));
    dt_ns.insert(CompactString::from("fromisoformat"), make_builtin(datetime_fromisoformat));
    let datetime_cls = PyObject::class(CompactString::from("datetime"), vec![], dt_ns);
    // Store __init__ for constructor dispatch
    if let PyObjectPayload::Class(ref cd) = datetime_cls.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args| {
                // datetime(year, month, day, hour=0, minute=0, second=0, microsecond=0)
                if args.len() < 4 { return Err(PyException::type_error("datetime() requires at least year, month, day")); }
                let year = args[1].to_int()?;
                let month = args[2].to_int()?;
                let day = args[3].to_int()?;
                let hour = if args.len() > 4 { args[4].to_int()? } else { 0 };
                let minute = if args.len() > 5 { args[5].to_int()? } else { 0 };
                let second = if args.len() > 6 { args[6].to_int()? } else { 0 };
                let microsecond = if args.len() > 7 { args[7].to_int()? } else { 0 };
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let mut w = inst.attrs.write();
                    w.insert(CompactString::from("__datetime__"), PyObject::bool_val(true));
                    w.insert(CompactString::from("year"), PyObject::int(year));
                    w.insert(CompactString::from("month"), PyObject::int(month));
                    w.insert(CompactString::from("day"), PyObject::int(day));
                    w.insert(CompactString::from("hour"), PyObject::int(hour));
                    w.insert(CompactString::from("minute"), PyObject::int(minute));
                    w.insert(CompactString::from("second"), PyObject::int(second));
                    w.insert(CompactString::from("microsecond"), PyObject::int(microsecond));
                }
                Ok(PyObject::none())
            }),
        );
    }

    // Build date class with constructor and class methods
    let mut date_ns = IndexMap::new();
    date_ns.insert(CompactString::from("today"), make_builtin(date_today));
    date_ns.insert(CompactString::from("fromisoformat"), make_builtin(datetime_fromisoformat));
    date_ns.insert(CompactString::from("__add__"), make_builtin(date_add));
    date_ns.insert(CompactString::from("__sub__"), make_builtin(date_sub));
    let date_cls = PyObject::class(CompactString::from("date"), vec![], date_ns);
    if let PyObjectPayload::Class(ref cd) = date_cls.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args| {
                // date(year, month, day)
                if args.len() < 4 { return Err(PyException::type_error("date() requires year, month, day")); }
                let year = args[1].to_int()?;
                let month = args[2].to_int()?;
                let day = args[3].to_int()?;
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let mut w = inst.attrs.write();
                    w.insert(CompactString::from("__datetime__"), PyObject::bool_val(true));
                    w.insert(CompactString::from("__date_only__"), PyObject::bool_val(true));
                    w.insert(CompactString::from("year"), PyObject::int(year));
                    w.insert(CompactString::from("month"), PyObject::int(month));
                    w.insert(CompactString::from("day"), PyObject::int(day));
                }
                Ok(PyObject::none())
            }),
        );
    }

    make_module("datetime", vec![
        ("datetime", datetime_cls),
        ("date", date_cls),
        ("time", make_builtin(datetime_time_obj)),
        ("timedelta", make_builtin(datetime_timedelta)),
        ("MINYEAR", PyObject::int(1)),
        ("MAXYEAR", PyObject::int(9999)),
    ])
}

fn datetime_now(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let micros = now.subsec_micros();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hour = (time_of_day / 3600) as i64;
    let minute = ((time_of_day % 3600) / 60) as i64;
    let second = (time_of_day % 60) as i64;
    let (year, month, day) = days_to_ymd(days as i64 + 719468);
    Ok(make_datetime_instance(year, month, day, hour, minute, second, micros as i64))
}

fn date_today(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let days = now.as_secs() / 86400;
    let (year, month, day) = days_to_ymd(days as i64 + 719468);
    Ok(make_date_instance(year, month, day))
}






















fn datetime_fromisoformat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("fromisoformat", args, 1)?;
    let s = args[0].py_to_string();
    // Parse ISO format: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS
    let parts: Vec<&str> = s.split('T').collect();
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() < 3 { return Err(PyException::value_error("Invalid isoformat")); }
    let year: i64 = date_parts[0].parse().map_err(|_| PyException::value_error("Invalid year"))?;
    let month: i64 = date_parts[1].parse().map_err(|_| PyException::value_error("Invalid month"))?;
    let day: i64 = date_parts[2].parse().map_err(|_| PyException::value_error("Invalid day"))?;
    let (hour, minute, second) = if parts.len() > 1 {
        let time_parts: Vec<&str> = parts[1].split(':').collect();
        let h: i64 = time_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let m: i64 = time_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let sec: i64 = time_parts.get(2).and_then(|s| s.split('.').next().unwrap_or("0").parse().ok()).unwrap_or(0);
        (h, m, sec)
    } else { (0, 0, 0) };
    Ok(make_datetime_instance(year, month, day, hour, minute, second, 0))
}

fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    // Civil days from epoch to Y-M-D (algorithm from Howard Hinnant)
    let z = days;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2) / 153;
    let d = doy - (153*mp + 2)/5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn make_datetime_instance(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64, microsecond: i64) -> PyObjectRef {
    let class = PyObject::class(CompactString::from("datetime"), vec![], IndexMap::new());
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
        dict_storage: None,
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__datetime__"), PyObject::bool_val(true));
        w.insert(CompactString::from("year"), PyObject::int(year));
        w.insert(CompactString::from("month"), PyObject::int(month));
        w.insert(CompactString::from("day"), PyObject::int(day));
        w.insert(CompactString::from("hour"), PyObject::int(hour));
        w.insert(CompactString::from("minute"), PyObject::int(minute));
        w.insert(CompactString::from("second"), PyObject::int(second));
        w.insert(CompactString::from("microsecond"), PyObject::int(microsecond));
    }
    inst
}

fn datetime_time_obj(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let hour = if !args.is_empty() { args[0].to_int()? } else { 0 };
    let minute = if args.len() > 1 { args[1].to_int()? } else { 0 };
    let second = if args.len() > 2 { args[2].to_int()? } else { 0 };
    let microsecond = if args.len() > 3 { args[3].to_int()? } else { 0 };
    let class = PyObject::class(CompactString::from("time"), vec![], IndexMap::new());
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
        dict_storage: None,
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("hour"), PyObject::int(hour));
        w.insert(CompactString::from("minute"), PyObject::int(minute));
        w.insert(CompactString::from("second"), PyObject::int(second));
        w.insert(CompactString::from("microsecond"), PyObject::int(microsecond));
    }
    Ok(inst)
}

fn datetime_timedelta(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // timedelta(days=0, seconds=0, microseconds=0, milliseconds=0, minutes=0, hours=0, weeks=0)
    let mut days = 0i64;
    let mut seconds = 0i64;
    let mut microseconds = 0i64;
    if !args.is_empty() { days = args[0].to_int().unwrap_or(0); }
    if args.len() > 1 { seconds = args[1].to_int().unwrap_or(0); }
    if args.len() > 2 { microseconds = args[2].to_int().unwrap_or(0); }
    // Check for kwargs dict as last arg
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("days"))) { days = v.as_int().unwrap_or(0); }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("seconds"))) { seconds = v.as_int().unwrap_or(0); }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("microseconds"))) { microseconds = v.as_int().unwrap_or(0); }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("milliseconds"))) { microseconds += v.as_int().unwrap_or(0) * 1000; }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("minutes"))) { seconds += v.as_int().unwrap_or(0) * 60; }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("hours"))) { seconds += v.as_int().unwrap_or(0) * 3600; }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("weeks"))) { days += v.as_int().unwrap_or(0) * 7; }
        }
    }
    // Normalize: carry microseconds → seconds → days
    seconds += microseconds / 1_000_000;
    microseconds %= 1_000_000;
    if microseconds < 0 { microseconds += 1_000_000; seconds -= 1; }
    days += seconds / 86400;
    seconds %= 86400;
    if seconds < 0 { seconds += 86400; days -= 1; }
    let total_secs = days as f64 * 86400.0 + seconds as f64 + microseconds as f64 / 1_000_000.0;
    make_timedelta(days, seconds, microseconds, total_secs)
}
fn ymd_to_ordinal(y: i64, m: i64, d: i64) -> i64 {
    // Convert Y-M-D to a day ordinal (proleptic Gregorian)
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    365 * y + y / 4 - y / 100 + y / 400 + (m * 153 + 2) / 5 + d - 1
}

fn ordinal_to_ymd(ord: i64) -> (i64, i64, i64) {
    let y0 = (10000 * ord + 14780) / 3652425;
    let mut doy = ord - (365 * y0 + y0 / 4 - y0 / 100 + y0 / 400);
    let y0 = if doy < 0 { let y1 = y0 - 1; doy = ord - (365 * y1 + y1 / 4 - y1 / 100 + y1 / 400); y1 } else { y0 };
    let mi = (100 * doy + 52) / 3060;
    let month = if mi < 10 { mi + 3 } else { mi - 9 };
    let year = y0 + if month <= 2 { 1 } else { 0 };
    let day = doy - (mi * 306 + 5) / 10 + 1;
    (year, month, day)
}

fn make_date_instance(year: i64, month: i64, day: i64) -> PyObjectRef {
    let mut date_cls_ns = IndexMap::new();
    date_cls_ns.insert(CompactString::from("__add__"), make_builtin(date_add));
    date_cls_ns.insert(CompactString::from("__sub__"), make_builtin(date_sub));
    let class = PyObject::class(CompactString::from("date"), vec![], date_cls_ns);
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
        dict_storage: None,
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__datetime__"), PyObject::bool_val(true));
        w.insert(CompactString::from("__date_only__"), PyObject::bool_val(true));
        w.insert(CompactString::from("year"), PyObject::int(year));
        w.insert(CompactString::from("month"), PyObject::int(month));
        w.insert(CompactString::from("day"), PyObject::int(day));
    }
    inst
}

fn date_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("date.__add__ requires 2 args")); }
    let date_obj = &args[0];
    let td_obj = &args[1];
    let year = date_obj.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
    let month = date_obj.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
    let day = date_obj.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
    let td_days = td_obj.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let ord = ymd_to_ordinal(year, month, day) + td_days;
    let (ny, nm, nd) = ordinal_to_ymd(ord);
    Ok(make_date_instance(ny, nm, nd))
}

fn date_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("date.__sub__ requires 2 args")); }
    let date_obj = &args[0];
    let other = &args[1];
    let year = date_obj.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
    let month = date_obj.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
    let day = date_obj.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
    if other.get_attr("__timedelta__").is_some() {
        let td_days = other.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
        let ord = ymd_to_ordinal(year, month, day) - td_days;
        let (ny, nm, nd) = ordinal_to_ymd(ord);
        Ok(make_date_instance(ny, nm, nd))
    } else if other.get_attr("__date_only__").is_some() {
        let y2 = other.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
        let m2 = other.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
        let d2 = other.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
        let diff = ymd_to_ordinal(year, month, day) - ymd_to_ordinal(y2, m2, d2);
        make_timedelta_with_ops(diff, 0, 0, diff as f64 * 86400.0)
    } else {
        Err(PyException::type_error("unsupported operand type(s) for -"))
    }
}


fn make_timedelta_with_ops(days: i64, seconds: i64, microseconds: i64, total_secs: f64) -> PyResult<PyObjectRef> {
    let mut td_ns = IndexMap::new();
    td_ns.insert(CompactString::from("__add__"), make_builtin(timedelta_add));
    td_ns.insert(CompactString::from("__sub__"), make_builtin(timedelta_sub));
    td_ns.insert(CompactString::from("__radd__"), make_builtin(timedelta_add));
    let class = PyObject::class(CompactString::from("timedelta"), vec![], td_ns);
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
        dict_storage: None,
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__timedelta__"), PyObject::bool_val(true));
        w.insert(CompactString::from("days"), PyObject::int(days));
        w.insert(CompactString::from("seconds"), PyObject::int(seconds));
        w.insert(CompactString::from("microseconds"), PyObject::int(microseconds));
        w.insert(CompactString::from("_total_seconds"), PyObject::float(total_secs));
        w.insert(CompactString::from("_total_us"), PyObject::int(days * 86_400_000_000 + seconds * 1_000_000 + microseconds));
    }
    Ok(inst)
}

fn make_timedelta(days: i64, seconds: i64, microseconds: i64, total_secs: f64) -> PyResult<PyObjectRef> {
    make_timedelta_with_ops(days, seconds, microseconds, total_secs)
}

fn timedelta_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("timedelta.__add__ requires 2 args")); }
    let a = &args[0];
    let b = &args[1];
    if b.get_attr("__date_only__").is_some() {
        return date_add(&[b.clone(), a.clone()]);
    }
    let a_days = a.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let a_secs = a.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let a_us = a.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);
    let b_days = b.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let b_secs = b.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let b_us = b.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);
    let days = a_days + b_days;
    let secs = a_secs + b_secs;
    let us = a_us + b_us;
    let total = days as f64 * 86400.0 + secs as f64 + us as f64 / 1_000_000.0;
    make_timedelta_with_ops(days, secs, us, total)
}

fn timedelta_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("timedelta.__sub__ requires 2 args")); }
    let a = &args[0];
    let b = &args[1];
    let a_days = a.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let a_secs = a.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let a_us = a.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);
    let b_days = b.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let b_secs = b.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let b_us = b.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);
    let days = a_days - b_days;
    let secs = a_secs - b_secs;
    let us = a_us - b_us;
    let total = days as f64 * 86400.0 + secs as f64 + us as f64 / 1_000_000.0;
    make_timedelta_with_ops(days, secs, us, total)
}



















// ── weakref module ──


