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
    dt_ns.insert(CompactString::from("strptime"), make_builtin(datetime_strptime));
    dt_ns.insert(CompactString::from("fromtimestamp"), make_builtin(datetime_fromtimestamp));
    dt_ns.insert(CompactString::from("combine"), make_builtin(datetime_combine));
    dt_ns.insert(CompactString::from("fromordinal"), make_builtin(datetime_fromordinal));
    dt_ns.insert(CompactString::from("__add__"), make_builtin(datetime_add_dunder));
    dt_ns.insert(CompactString::from("__sub__"), make_builtin(datetime_sub_dunder));
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
    date_ns.insert(CompactString::from("fromordinal"), make_builtin(date_fromordinal));
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
                    return Err(PyException::type_error("timezone() requires an offset argument"));
                }
                let offset = &args[1];
                let offset_secs = offset.get_attr("_total_seconds")
                    .and_then(|v| Some(v.to_float().unwrap_or(0.0)))
                    .unwrap_or(0.0);
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let mut w = inst.attrs.write();
                    w.insert(CompactString::from("__timezone__"), PyObject::bool_val(true));
                    w.insert(CompactString::from("_offset_seconds"), PyObject::float(offset_secs));
                    let total_mins = (offset_secs / 60.0) as i64;
                    let sign = if total_mins >= 0 { "+" } else { "-" };
                    let abs_mins = total_mins.abs();
                    let name = format!("UTC{}{:02}:{:02}", sign, abs_mins / 60, abs_mins % 60);
                    w.insert(CompactString::from("_name"), PyObject::str_val(CompactString::from(&name)));
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
        ("timezone", tz_cls),
        ("MINYEAR", PyObject::int(1)),
        ("MAXYEAR", PyObject::int(9999)),
    ])
}

fn make_timezone_utc() -> PyObjectRef {
    let class = PyObject::class(CompactString::from("timezone"), vec![], IndexMap::new());
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
        dict_storage: None,
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__timezone__"), PyObject::bool_val(true));
        w.insert(CompactString::from("_offset_seconds"), PyObject::float(0.0));
        w.insert(CompactString::from("_name"), PyObject::str_val(CompactString::from("UTC")));
    }
    inst
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
    Ok(make_datetime_instance(year, month, day, hour, minute, second, micros))
}

fn datetime_combine(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("combine() requires date and time arguments"));
    }
    let date_obj = &args[0];
    let time_obj = &args[1];
    let year = date_obj.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
    let month = date_obj.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
    let day = date_obj.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
    let hour = time_obj.get_attr("hour").and_then(|v| v.as_int()).unwrap_or(0);
    let minute = time_obj.get_attr("minute").and_then(|v| v.as_int()).unwrap_or(0);
    let second = time_obj.get_attr("second").and_then(|v| v.as_int()).unwrap_or(0);
    let microsecond = time_obj.get_attr("microsecond").and_then(|v| v.as_int()).unwrap_or(0);
    Ok(make_datetime_instance(year, month, day, hour, minute, second, microsecond))
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

/// datetime.strptime(date_string, format) — parse a date string with a format specifier.
fn datetime_strptime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("strptime() requires 2 arguments: date_string and format"));
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
                'Y' => { let (v, new_si) = parse_int(&date_str, si, 4)?; year = v; si = new_si; }
                'm' => { let (v, new_si) = parse_int(&date_str, si, 2)?; month = v; si = new_si; }
                'd' => { let (v, new_si) = parse_int(&date_str, si, 2)?; day = v; si = new_si; }
                'H' => { let (v, new_si) = parse_int(&date_str, si, 2)?; hour = v; si = new_si; }
                'M' => { let (v, new_si) = parse_int(&date_str, si, 2)?; minute = v; si = new_si; }
                'S' => { let (v, new_si) = parse_int(&date_str, si, 2)?; second = v; si = new_si; }
                'f' => { let (v, new_si) = parse_int(&date_str, si, 6)?; microsecond = v; si = new_si; }
                'y' => {
                    let (v, new_si) = parse_int(&date_str, si, 2)?;
                    year = if v >= 69 { 1900 + v } else { 2000 + v };
                    si = new_si;
                }
                'j' => { let (_v, new_si) = parse_int(&date_str, si, 3)?; si = new_si; }
                'p' => {
                    // AM/PM
                    let rest = &date_str[si..];
                    if rest.starts_with("PM") || rest.starts_with("pm") {
                        if hour < 12 { hour += 12; }
                        si += 2;
                    } else if rest.starts_with("AM") || rest.starts_with("am") {
                        if hour == 12 { hour = 0; }
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
                    let months = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"];
                    let rest = date_str[si..].to_lowercase();
                    let mut found = false;
                    for (i, m) in months.iter().enumerate() {
                        if rest.starts_with(m) {
                            month = (i + 1) as i64;
                            si += if code == 'b' { 3 } else {
                                // Full month names
                                let full = ["january","february","march","april","may","june",
                                            "july","august","september","october","november","december"];
                                if rest.starts_with(full[i]) { full[i].len() } else { 3 }
                            };
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(PyException::value_error(format!(
                            "time data '{}' does not match format '{}'", date_str, fmt)));
                    }
                }
                'a' | 'A' => {
                    // Day names (abbreviated or full) — consume but don't use (weekday is derived)
                    let day_abbrs = ["mon","tue","wed","thu","fri","sat","sun"];
                    let day_fulls = ["monday","tuesday","wednesday","thursday","friday","saturday","sunday"];
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
                            "time data '{}' does not match format '{}'", date_str, fmt)));
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
                            "time data '{}' does not match format '{}'", date_str, fmt)));
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
                    "time data '{}' does not match format '{}'", date_str, fmt)));
            }
        }
    }

    Ok(make_datetime_instance(year, month, day, hour, minute, second, microsecond))
}

/// Parse an integer of up to `max_digits` from `s` starting at position `pos`.
fn parse_int(s: &str, pos: usize, max_digits: usize) -> PyResult<(i64, usize)> {
    let bytes = s.as_bytes();
    let mut end = pos;
    while end < bytes.len() && end - pos < max_digits && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == pos {
        return Err(PyException::value_error(format!("unconverted data remains: '{}'", &s[pos..])));
    }
    let val: i64 = s[pos..end].parse().map_err(|_| PyException::value_error("invalid integer"))?;
    Ok((val, end))
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
    let mut dt_ns = IndexMap::new();
    dt_ns.insert(CompactString::from("__add__"), make_builtin(datetime_add_dunder));
    dt_ns.insert(CompactString::from("__sub__"), make_builtin(datetime_sub_dunder));
    dt_ns.insert(CompactString::from("__eq__"), make_builtin(datetime_eq));
    dt_ns.insert(CompactString::from("__lt__"), make_builtin(datetime_lt));
    dt_ns.insert(CompactString::from("__le__"), make_builtin(datetime_le));
    dt_ns.insert(CompactString::from("__gt__"), make_builtin(datetime_gt));
    dt_ns.insert(CompactString::from("__ge__"), make_builtin(datetime_ge));
    let class = PyObject::class(CompactString::from("datetime"), vec![], dt_ns);
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
        w.insert(CompactString::from("__datetime__"), PyObject::bool_val(true));
        w.insert(CompactString::from("__time_only__"), PyObject::bool_val(true));
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
    date_cls_ns.insert(CompactString::from("__eq__"), make_builtin(date_eq));
    date_cls_ns.insert(CompactString::from("__lt__"), make_builtin(date_lt));
    date_cls_ns.insert(CompactString::from("__le__"), make_builtin(date_le));
    date_cls_ns.insert(CompactString::from("__gt__"), make_builtin(date_gt));
    date_cls_ns.insert(CompactString::from("__ge__"), make_builtin(date_ge));
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

fn date_ordinal(obj: &PyObjectRef) -> i64 {
    let y = obj.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
    let m = obj.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
    let d = obj.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
    ymd_to_ordinal(y, m, d)
}

fn date_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(date_ordinal(&args[0]) == date_ordinal(&args[1])))
}

fn date_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(date_ordinal(&args[0]) < date_ordinal(&args[1])))
}

fn date_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(date_ordinal(&args[0]) <= date_ordinal(&args[1])))
}

fn date_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(date_ordinal(&args[0]) > date_ordinal(&args[1])))
}

fn date_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(date_ordinal(&args[0]) >= date_ordinal(&args[1])))
}


fn make_timedelta_with_ops(days: i64, seconds: i64, microseconds: i64, total_secs: f64) -> PyResult<PyObjectRef> {
    let mut td_ns = IndexMap::new();
    td_ns.insert(CompactString::from("__add__"), make_builtin(timedelta_add));
    td_ns.insert(CompactString::from("__sub__"), make_builtin(timedelta_sub));
    td_ns.insert(CompactString::from("__radd__"), make_builtin(timedelta_add));
    td_ns.insert(CompactString::from("__mul__"), make_builtin(timedelta_mul));
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

fn timedelta_mul(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("timedelta.__mul__ requires 2 args")); }
    let td = &args[0];
    let factor = args[1].to_int().map_err(|_| PyException::type_error("unsupported operand type(s) for *"))?;
    let td_days = td.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let td_secs = td.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let td_us = td.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);
    let total_us = (td_days * 86_400_000_000 + td_secs * 1_000_000 + td_us) * factor;
    let days = total_us / 86_400_000_000;
    let rem = total_us % 86_400_000_000;
    let seconds = rem / 1_000_000;
    let microseconds = rem % 1_000_000;
    let total = total_us as f64 / 1_000_000.0;
    make_timedelta_with_ops(days, seconds, microseconds, total)
}

/// datetime + timedelta → datetime
fn datetime_add_timedelta(dt: &PyObjectRef, td: &PyObjectRef) -> PyResult<PyObjectRef> {
    let year = dt.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
    let month = dt.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
    let day = dt.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
    let hour = dt.get_attr("hour").and_then(|v| v.as_int()).unwrap_or(0);
    let minute = dt.get_attr("minute").and_then(|v| v.as_int()).unwrap_or(0);
    let second = dt.get_attr("second").and_then(|v| v.as_int()).unwrap_or(0);
    let microsecond = dt.get_attr("microsecond").and_then(|v| v.as_int()).unwrap_or(0);

    let td_days = td.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
    let td_secs = td.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
    let td_us = td.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);

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

    Ok(make_datetime_instance(ny, nm, nd, new_h, new_m, new_s, new_us))
}

/// datetime - timedelta → datetime; datetime - datetime → timedelta
fn datetime_sub_dunder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("datetime.__sub__ requires 2 args")); }
    let dt = &args[0];
    let other = &args[1];
    if other.get_attr("__timedelta__").is_some() {
        // datetime - timedelta → datetime (negate and add)
        let neg_days = -other.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0);
        let neg_secs = -other.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0);
        let neg_us = -other.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);
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
        let us1 = dt.get_attr("microsecond").and_then(|v| v.as_int()).unwrap_or(0);

        let y2 = other.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
        let m2 = other.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
        let d2 = other.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
        let h2 = other.get_attr("hour").and_then(|v| v.as_int()).unwrap_or(0);
        let mi2 = other.get_attr("minute").and_then(|v| v.as_int()).unwrap_or(0);
        let s2 = other.get_attr("second").and_then(|v| v.as_int()).unwrap_or(0);
        let us2 = other.get_attr("microsecond").and_then(|v| v.as_int()).unwrap_or(0);

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
fn datetime_add_dunder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("datetime.__add__ requires 2 args")); }
    datetime_add_timedelta(&args[0], &args[1])
}

fn datetime_to_ordinal_secs(obj: &PyObjectRef) -> (i64, i64) {
    let y = obj.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
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

fn datetime_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(datetime_cmp(&args[0], &args[1]) == std::cmp::Ordering::Equal))
}

fn datetime_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(datetime_cmp(&args[0], &args[1]) == std::cmp::Ordering::Less))
}

fn datetime_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(datetime_cmp(&args[0], &args[1]) != std::cmp::Ordering::Greater))
}

fn datetime_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(datetime_cmp(&args[0], &args[1]) == std::cmp::Ordering::Greater))
}

fn datetime_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
    Ok(PyObject::bool_val(datetime_cmp(&args[0], &args[1]) != std::cmp::Ordering::Less))
}














// ── calendar module ──────────────────────────────────────────────────
pub fn create_calendar_module() -> PyObjectRef {
    fn is_leap(year: i64) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    fn days_in_month(year: i64, month: i64) -> i64 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => if is_leap(year) { 29 } else { 28 },
            _ => 30,
        }
    }

    // day_of_week: 0=Mon, 6=Sun (ISO standard, matches Python calendar)
    fn weekday(year: i64, month: i64, day: i64) -> i64 {
        // Tomohiko Sakamoto's algorithm
        let t = [0i64, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let y = if month < 3 { year - 1 } else { year };
        ((y + y / 4 - y / 100 + y / 400 + t[(month - 1) as usize] + day) % 7 + 6) % 7
    }

    fn cal_isleap(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("isleap requires 1 argument")); }
        let year = args[0].to_int()?;
        Ok(PyObject::bool_val(is_leap(year)))
    }

    fn cal_leapdays(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("leapdays requires 2 arguments")); }
        let y1 = args[0].to_int()?;
        let y2 = args[1].to_int()?;
        let count_leaps = |y: i64| -> i64 { (y - 1) / 4 - (y - 1) / 100 + (y - 1) / 400 };
        Ok(PyObject::int(count_leaps(y2) - count_leaps(y1)))
    }

    fn cal_weekday(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 { return Err(PyException::type_error("weekday requires 3 arguments")); }
        let y = args[0].to_int()?;
        let m = args[1].to_int()?;
        let d = args[2].to_int()?;
        Ok(PyObject::int(weekday(y, m, d)))
    }

    fn cal_monthrange(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("monthrange requires 2 arguments")); }
        let year = args[0].to_int()?;
        let month = args[1].to_int()?;
        let first_day = weekday(year, month, 1);
        let num_days = days_in_month(year, month);
        Ok(PyObject::tuple(vec![PyObject::int(first_day), PyObject::int(num_days)]))
    }

    fn cal_month(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("month requires 2 arguments")); }
        let year = args[0].to_int()?;
        let month = args[1].to_int()?;
        let month_names = ["", "January", "February", "March", "April", "May", "June",
                           "July", "August", "September", "October", "November", "December"];
        let mname = month_names.get(month as usize).unwrap_or(&"");
        let mut lines = vec![format!("   {:^20}", format!("{} {}", mname, year))];
        lines.push("Mo Tu We Th Fr Sa Su".to_string());
        let first_weekday = weekday(year, month, 1);
        let ndays = days_in_month(year, month);
        let mut line = "   ".repeat(first_weekday as usize);
        for d in 1..=ndays {
            line.push_str(&format!("{:2} ", d));
            if (first_weekday + d) % 7 == 0 { lines.push(line.trim_end().to_string()); line = String::new(); }
        }
        if !line.trim().is_empty() { lines.push(line.trim_end().to_string()); }
        lines.push(String::new());
        Ok(PyObject::str_val(CompactString::from(lines.join("\n"))))
    }

    fn cal_monthcalendar(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("monthcalendar requires 2 arguments")); }
        let year = args[0].to_int()?;
        let month = args[1].to_int()?;
        let first_weekday = weekday(year, month, 1) as usize;
        let ndays = days_in_month(year, month);
        let mut weeks = Vec::new();
        let mut week: Vec<PyObjectRef> = vec![PyObject::int(0); first_weekday];
        for d in 1..=ndays {
            week.push(PyObject::int(d));
            if week.len() == 7 { weeks.push(PyObject::list(week.clone())); week.clear(); }
        }
        if !week.is_empty() {
            while week.len() < 7 { week.push(PyObject::int(0)); }
            weeks.push(PyObject::list(week));
        }
        Ok(PyObject::list(weeks))
    }

    make_module("calendar", vec![
        ("isleap", make_builtin(cal_isleap)),
        ("leapdays", make_builtin(cal_leapdays)),
        ("weekday", make_builtin(cal_weekday)),
        ("monthrange", make_builtin(cal_monthrange)),
        ("month", make_builtin(cal_month)),
        ("monthcalendar", make_builtin(cal_monthcalendar)),
        ("day_name", PyObject::list(vec![
            PyObject::str_val(CompactString::from("Monday")),
            PyObject::str_val(CompactString::from("Tuesday")),
            PyObject::str_val(CompactString::from("Wednesday")),
            PyObject::str_val(CompactString::from("Thursday")),
            PyObject::str_val(CompactString::from("Friday")),
            PyObject::str_val(CompactString::from("Saturday")),
            PyObject::str_val(CompactString::from("Sunday")),
        ])),
        ("day_abbr", PyObject::list(vec![
            PyObject::str_val(CompactString::from("Mon")),
            PyObject::str_val(CompactString::from("Tue")),
            PyObject::str_val(CompactString::from("Wed")),
            PyObject::str_val(CompactString::from("Thu")),
            PyObject::str_val(CompactString::from("Fri")),
            PyObject::str_val(CompactString::from("Sat")),
            PyObject::str_val(CompactString::from("Sun")),
        ])),
        ("month_name", PyObject::list(vec![
            PyObject::str_val(CompactString::from("")),
            PyObject::str_val(CompactString::from("January")),
            PyObject::str_val(CompactString::from("February")),
            PyObject::str_val(CompactString::from("March")),
            PyObject::str_val(CompactString::from("April")),
            PyObject::str_val(CompactString::from("May")),
            PyObject::str_val(CompactString::from("June")),
            PyObject::str_val(CompactString::from("July")),
            PyObject::str_val(CompactString::from("August")),
            PyObject::str_val(CompactString::from("September")),
            PyObject::str_val(CompactString::from("October")),
            PyObject::str_val(CompactString::from("November")),
            PyObject::str_val(CompactString::from("December")),
        ])),
        ("month_abbr", PyObject::list(vec![
            PyObject::str_val(CompactString::from("")),
            PyObject::str_val(CompactString::from("Jan")),
            PyObject::str_val(CompactString::from("Feb")),
            PyObject::str_val(CompactString::from("Mar")),
            PyObject::str_val(CompactString::from("Apr")),
            PyObject::str_val(CompactString::from("May")),
            PyObject::str_val(CompactString::from("Jun")),
            PyObject::str_val(CompactString::from("Jul")),
            PyObject::str_val(CompactString::from("Aug")),
            PyObject::str_val(CompactString::from("Sep")),
            PyObject::str_val(CompactString::from("Oct")),
            PyObject::str_val(CompactString::from("Nov")),
            PyObject::str_val(CompactString::from("Dec")),
        ])),
        ("MONDAY", PyObject::int(0)),
        ("TUESDAY", PyObject::int(1)),
        ("WEDNESDAY", PyObject::int(2)),
        ("THURSDAY", PyObject::int(3)),
        ("FRIDAY", PyObject::int(4)),
        ("SATURDAY", PyObject::int(5)),
        ("SUNDAY", PyObject::int(6)),
    ])
}

// ── weakref module ──


