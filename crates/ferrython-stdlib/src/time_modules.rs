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
        ("strptime", make_builtin(time_strptime)),
        ("localtime", make_builtin(time_localtime)),
        ("gmtime", make_builtin(time_gmtime)),
        ("mktime", make_builtin(time_mktime)),
        ("ctime", make_builtin(time_ctime)),
        ("asctime", make_builtin(time_asctime)),
        ("timezone", PyObject::int(0)),
        ("altzone", PyObject::int(0)),
        ("daylight", PyObject::int(0)),
        ("tzname", PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("UTC")),
            PyObject::str_val(CompactString::from("UTC")),
        ])),
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

// ── Shared time decomposition ──

const MONTH_NAMES_ABBR: [&str; 12] = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
const MONTH_NAMES_FULL: [&str; 12] = ["January","February","March","April","May","June","July","August","September","October","November","December"];
const DAY_NAMES_ABBR: [&str; 7] = ["Mon","Tue","Wed","Thu","Fri","Sat","Sun"];
const DAY_NAMES_FULL: [&str; 7] = ["Monday","Tuesday","Wednesday","Thursday","Friday","Saturday","Sunday"];

fn is_leap_year(y: i64) -> bool { y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) }

fn days_in_month(y: i64) -> [i64; 12] {
    [31, if is_leap_year(y) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
}

/// Decompose Unix timestamp into (year, month 1-12, day 1-31, hour, min, sec, wday 0=Mon, yday 1-366)
fn decompose_timestamp(epoch_secs: u64) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    let sec = (epoch_secs % 60) as i64;
    let min = ((epoch_secs / 60) % 60) as i64;
    let hour = ((epoch_secs / 3600) % 24) as i64;
    let total_days = (epoch_secs / 86400) as i64;
    let mut y: i64 = 1970;
    let mut remaining = total_days;
    loop {
        let dy = if is_leap_year(y) { 366 } else { 365 };
        if remaining < dy { break; }
        remaining -= dy;
        y += 1;
    }
    let md = days_in_month(y);
    let mut mon = 1i64;
    for &d in &md {
        if remaining < d { break; }
        remaining -= d;
        mon += 1;
    }
    let day = remaining + 1;
    let wday = ((total_days + 3) % 7) as i64; // epoch was Thursday, 0=Monday
    let yday = {
        let mut yd = day;
        for i in 0..(mon - 1) as usize { yd += md[i]; }
        yd
    };
    (y, mon, day, hour, min, sec, wday, yday)
}

fn make_struct_time(y: i64, mon: i64, day: i64, h: i64, m: i64, s: i64, wday: i64, yday: i64) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("struct_time"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref data) = inst.payload {
        let mut attrs = data.attrs.write();
        attrs.insert(CompactString::from("tm_year"), PyObject::int(y));
        attrs.insert(CompactString::from("tm_mon"), PyObject::int(mon));
        attrs.insert(CompactString::from("tm_mday"), PyObject::int(day));
        attrs.insert(CompactString::from("tm_hour"), PyObject::int(h));
        attrs.insert(CompactString::from("tm_min"), PyObject::int(m));
        attrs.insert(CompactString::from("tm_sec"), PyObject::int(s));
        attrs.insert(CompactString::from("tm_wday"), PyObject::int(wday));
        attrs.insert(CompactString::from("tm_yday"), PyObject::int(yday));
        attrs.insert(CompactString::from("tm_isdst"), PyObject::int(-1));
        // Also support indexing as tuple
        let items = vec![
            PyObject::int(y), PyObject::int(mon), PyObject::int(day),
            PyObject::int(h), PyObject::int(m), PyObject::int(s),
            PyObject::int(wday), PyObject::int(yday), PyObject::int(-1),
        ];
        attrs.insert(CompactString::from("__tuple__"), PyObject::tuple(items.clone()));
        // __repr__
        let repr = format!(
            "time.struct_time(tm_year={}, tm_mon={}, tm_mday={}, tm_hour={}, tm_min={}, tm_sec={}, tm_wday={}, tm_yday={}, tm_isdst=-1)",
            y, mon, day, h, m, s, wday, yday
        );
        attrs.insert(CompactString::from("__repr__"), PyObject::str_val(CompactString::from(repr)));
        // __len__ and __getitem__ for tuple-like access
        attrs.insert(CompactString::from("__len__"), make_builtin(|_| Ok(PyObject::int(9))));
        let items_ref = PyObject::tuple(items);
        attrs.insert(CompactString::from("__getitem__"), PyObject::native_closure(
            "__getitem__", move |args: &[PyObjectRef]| {
                if let Some(idx) = args.first().and_then(|a| a.as_int()) {
                    if let PyObjectPayload::Tuple(t) = &items_ref.payload {
                        let i = if idx < 0 { (t.len() as i64 + idx) as usize } else { idx as usize };
                        return Ok(t.get(i).cloned().unwrap_or_else(PyObject::none));
                    }
                }
                Ok(PyObject::none())
            }
        ));
    }
    inst
}

fn get_epoch_secs(args: &[PyObjectRef]) -> u64 {
    if let Some(secs_arg) = args.first() {
        if let Ok(f) = secs_arg.to_float() { return f as u64; }
        if let Some(i) = secs_arg.as_int() { return i as u64; }
    }
    use std::time::SystemTime;
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
}

/// Format struct_time components using strftime format codes
fn format_time(fmt: &str, y: i64, mon: i64, day: i64, h: i64, m: i64, s: i64, wday: i64, yday: i64) -> String {
    let hour12 = if h == 0 { 12 } else if h > 12 { h - 12 } else { h };
    let ampm = if h < 12 { "AM" } else { "PM" };
    let mut result = String::with_capacity(fmt.len() * 2);
    let mut chars = fmt.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.next() {
                Some('Y') => result.push_str(&format!("{:04}", y)),
                Some('m') => result.push_str(&format!("{:02}", mon)),
                Some('d') => result.push_str(&format!("{:02}", day)),
                Some('H') => result.push_str(&format!("{:02}", h)),
                Some('M') => result.push_str(&format!("{:02}", m)),
                Some('S') => result.push_str(&format!("{:02}", s)),
                Some('I') => result.push_str(&format!("{:02}", hour12)),
                Some('p') => result.push_str(ampm),
                Some('a') => result.push_str(DAY_NAMES_ABBR[wday as usize % 7]),
                Some('A') => result.push_str(DAY_NAMES_FULL[wday as usize % 7]),
                Some('b') | Some('h') => result.push_str(MONTH_NAMES_ABBR[(mon - 1) as usize % 12]),
                Some('B') => result.push_str(MONTH_NAMES_FULL[(mon - 1) as usize % 12]),
                Some('j') => result.push_str(&format!("{:03}", yday)),
                Some('w') => result.push_str(&format!("{}", (wday + 1) % 7)), // 0=Sunday
                Some('u') => result.push_str(&format!("{}", if wday == 6 { 7 } else { wday + 1 })), // 1=Monday
                Some('y') => result.push_str(&format!("{:02}", y % 100)),
                Some('c') => {
                    // Locale's date+time: "Mon Jan  1 00:00:00 2024"
                    result.push_str(&format!("{} {} {:2} {:02}:{:02}:{:02} {:04}",
                        DAY_NAMES_ABBR[wday as usize % 7], MONTH_NAMES_ABBR[(mon - 1) as usize % 12],
                        day, h, m, s, y));
                }
                Some('x') => result.push_str(&format!("{:02}/{:02}/{:02}", mon, day, y % 100)),
                Some('X') => result.push_str(&format!("{:02}:{:02}:{:02}", h, m, s)),
                Some('Z') => result.push_str("UTC"),
                Some('z') => result.push_str("+0000"),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('%') => result.push('%'),
                Some(other) => { result.push('%'); result.push(other); }
                None => result.push('%'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn time_strftime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("strftime requires a format string")); }
    let fmt = args[0].py_to_string();
    // Use struct_time arg if provided, otherwise current time
    let (y, mon, day, h, m, s, wday, yday) = if args.len() >= 2 {
        extract_struct_time(&args[1])?
    } else {
        let secs = get_epoch_secs(&[]);
        decompose_timestamp(secs)
    };
    let result = format_time(&fmt, y, mon, day, h, m, s, wday, yday);
    Ok(PyObject::str_val(CompactString::from(result)))
}

/// Extract (y, mon, day, h, m, s, wday, yday) from a struct_time or tuple
fn extract_struct_time(obj: &PyObjectRef) -> PyResult<(i64, i64, i64, i64, i64, i64, i64, i64)> {
    match &obj.payload {
        PyObjectPayload::Tuple(t) if t.len() >= 9 => {
            Ok((
                t[0].as_int().unwrap_or(1970), t[1].as_int().unwrap_or(1),
                t[2].as_int().unwrap_or(1), t[3].as_int().unwrap_or(0),
                t[4].as_int().unwrap_or(0), t[5].as_int().unwrap_or(0),
                t[6].as_int().unwrap_or(0), t[7].as_int().unwrap_or(1),
            ))
        }
        PyObjectPayload::Instance(data) => {
            let attrs = data.attrs.read();
            if let Some(tup) = attrs.get("__tuple__") {
                if let PyObjectPayload::Tuple(t) = &tup.payload {
                    if t.len() >= 9 {
                        return Ok((
                            t[0].as_int().unwrap_or(1970), t[1].as_int().unwrap_or(1),
                            t[2].as_int().unwrap_or(1), t[3].as_int().unwrap_or(0),
                            t[4].as_int().unwrap_or(0), t[5].as_int().unwrap_or(0),
                            t[6].as_int().unwrap_or(0), t[7].as_int().unwrap_or(1),
                        ));
                    }
                }
            }
            // Try named attrs
            let y = attrs.get("tm_year").and_then(|v| v.as_int()).unwrap_or(1970);
            let mon = attrs.get("tm_mon").and_then(|v| v.as_int()).unwrap_or(1);
            let day = attrs.get("tm_mday").and_then(|v| v.as_int()).unwrap_or(1);
            let h = attrs.get("tm_hour").and_then(|v| v.as_int()).unwrap_or(0);
            let m = attrs.get("tm_min").and_then(|v| v.as_int()).unwrap_or(0);
            let s = attrs.get("tm_sec").and_then(|v| v.as_int()).unwrap_or(0);
            let wday = attrs.get("tm_wday").and_then(|v| v.as_int()).unwrap_or(0);
            let yday = attrs.get("tm_yday").and_then(|v| v.as_int()).unwrap_or(1);
            Ok((y, mon, day, h, m, s, wday, yday))
        }
        _ => Err(PyException::type_error("expected struct_time or 9-tuple")),
    }
}

fn time_strptime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("strptime() takes exactly 2 arguments"));
    }
    let date_str = args[0].py_to_string();
    let fmt = args[1].py_to_string();

    let mut y: i64 = 1900; let mut mon: i64 = 1; let mut day: i64 = 1;
    let mut h: i64 = 0; let mut m: i64 = 0; let mut s: i64 = 0;

    // Parse format string and extract values from date_str
    let mut fi = fmt.chars().peekable();
    let mut di = date_str.chars().peekable();

    while let Some(fc) = fi.next() {
        if fc == '%' {
            match fi.next() {
                Some('Y') => { y = parse_digits(&mut di, 4)?; }
                Some('y') => {
                    let v = parse_digits(&mut di, 2)?;
                    y = if v >= 69 { 1900 + v } else { 2000 + v };
                }
                Some('m') => { mon = parse_digits(&mut di, 2)?; }
                Some('d') => { day = parse_digits(&mut di, 2)?; }
                Some('H') => { h = parse_digits(&mut di, 2)?; }
                Some('I') => { h = parse_digits(&mut di, 2)?; }
                Some('M') => { m = parse_digits(&mut di, 2)?; }
                Some('S') => { s = parse_digits(&mut di, 2)?; }
                Some('p') => {
                    // AM/PM
                    let a: String = (&mut di).take(2).collect();
                    if a.eq_ignore_ascii_case("PM") && h < 12 { h += 12; }
                    else if a.eq_ignore_ascii_case("AM") && h == 12 { h = 0; }
                }
                Some('j') => { let _ = parse_digits(&mut di, 3)?; } // yday - skip
                Some('b') | Some('B') => {
                    // Month name - consume letters
                    let name: String = (&mut di).take_while(|c| c.is_alphabetic()).collect();
                    let lower = name.to_lowercase();
                    for (i, &abbr) in MONTH_NAMES_ABBR.iter().enumerate() {
                        if lower == abbr.to_lowercase() || lower == MONTH_NAMES_FULL[i].to_lowercase() {
                            mon = i as i64 + 1;
                            break;
                        }
                    }
                }
                Some('a') | Some('A') => {
                    // Day name - consume and ignore
                    let _: String = (&mut di).take_while(|c| c.is_alphabetic()).collect();
                }
                Some('%') => { di.next(); }
                Some(_) => {}
                None => {}
            }
        } else {
            di.next(); // consume matching literal
        }
    }

    // Compute wday and yday
    let md = days_in_month(y);
    let yday = {
        let mut yd = day;
        for i in 0..(mon - 1) as usize { if i < 12 { yd += md[i]; } }
        yd
    };
    // Compute day of week using Tomohiko Sakamoto's algorithm
    let wday = {
        let t = [0i64, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let yy = if mon < 3 { y - 1 } else { y };
        let w = (yy + yy/4 - yy/100 + yy/400 + t[(mon - 1) as usize] + day) % 7;
        (w + 6) % 7 // convert Sunday=0 to Monday=0
    };

    Ok(make_struct_time(y, mon, day, h, m, s, wday, yday))
}

fn parse_digits(chars: &mut std::iter::Peekable<std::str::Chars>, max: usize) -> PyResult<i64> {
    let mut s = String::new();
    // skip leading whitespace
    while chars.peek().map_or(false, |c| *c == ' ') { chars.next(); }
    for _ in 0..max {
        match chars.peek() {
            Some(c) if c.is_ascii_digit() => s.push(chars.next().unwrap()),
            _ => break,
        }
    }
    if s.is_empty() { return Ok(0); }
    s.parse::<i64>().map_err(|_| PyException::value_error("time data does not match format"))
}

fn time_localtime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let secs = get_epoch_secs(args);
    let (y, mon, day, h, m, s, wday, yday) = decompose_timestamp(secs);
    Ok(make_struct_time(y, mon, day, h, m, s, wday, yday))
}

fn time_gmtime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let secs = get_epoch_secs(args);
    let (y, mon, day, h, m, s, wday, yday) = decompose_timestamp(secs);
    Ok(make_struct_time(y, mon, day, h, m, s, wday, yday))
}

fn time_mktime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("mktime() requires a struct_time argument"));
    }
    let (y, mon, day, h, m, s, _wday, _yday) = extract_struct_time(&args[0])?;
    // Convert to epoch seconds
    let mut total_days: i64 = 0;
    for yr in 1970..y { total_days += if is_leap_year(yr) { 366 } else { 365 }; }
    if y < 1970 {
        for yr in y..1970 { total_days -= if is_leap_year(yr) { 366 } else { 365 }; }
    }
    let md = days_in_month(y);
    for i in 0..(mon - 1) as usize { if i < 12 { total_days += md[i]; } }
    total_days += day - 1;
    let epoch = total_days * 86400 + h * 3600 + m * 60 + s;
    Ok(PyObject::float(epoch as f64))
}

fn time_ctime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let secs = get_epoch_secs(args);
    let (y, mon, day, h, m, s, wday, _yday) = decompose_timestamp(secs);
    let result = format!("{} {} {:2} {:02}:{:02}:{:02} {:04}",
        DAY_NAMES_ABBR[wday as usize % 7], MONTH_NAMES_ABBR[(mon - 1) as usize % 12],
        day, h, m, s, y);
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn time_asctime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (y, mon, day, h, m, s, wday, _yday) = if args.is_empty() {
        let secs = get_epoch_secs(&[]);
        decompose_timestamp(secs)
    } else {
        extract_struct_time(&args[0])?
    };
    let result = format!("{} {} {:2} {:02}:{:02}:{:02} {:04}",
        DAY_NAMES_ABBR[wday as usize % 7], MONTH_NAMES_ABBR[(mon - 1) as usize % 12],
        day, h, m, s, y);
    Ok(PyObject::str_val(CompactString::from(result)))
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
                // datetime(year, month, day, hour=0, minute=0, second=0, microsecond=0, tzinfo=None)
                if args.len() < 4 { return Err(PyException::type_error("datetime() requires at least year, month, day")); }

                // Detect trailing kwargs dict appended by the VM's call_object_kw
                let mut tzinfo_val: Option<PyObjectRef> = None;
                let positional_end = {
                    let last = &args[args.len() - 1];
                    if matches!(&last.payload, PyObjectPayload::Dict(_)) {
                        if let PyObjectPayload::Dict(ref map) = last.payload {
                            let map_r = map.read();
                            if let Some(v) = map_r.get(&HashableKey::Str(CompactString::from("tzinfo"))) {
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
                let hour = if positional_end > 4 { args[4].to_int()? } else { 0 };
                let minute = if positional_end > 5 { args[5].to_int()? } else { 0 };
                let second = if positional_end > 6 { args[6].to_int()? } else { 0 };
                let microsecond = if positional_end > 7 { args[7].to_int()? } else { 0 };

                // Build instance with all methods via install_datetime_methods
                install_datetime_methods(&args[0], year, month, day, hour, minute, second, microsecond);
                if let Some(tz) = tzinfo_val {
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs.write().insert(CompactString::from("tzinfo"), tz);
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    // Build date class with constructor and class methods
    let mut date_ns = IndexMap::new();
    date_ns.insert(CompactString::from("today"), make_builtin(date_today));
    date_ns.insert(CompactString::from("fromisoformat"), make_builtin(date_fromisoformat));
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
        is_special: true, dict_storage: None,
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__timezone__"), PyObject::bool_val(true));
        w.insert(CompactString::from("_offset_seconds"), PyObject::float(0.0));
        w.insert(CompactString::from("_name"), PyObject::str_val(CompactString::from("UTC")));
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
                if let Some(v) = map_r.get(&HashableKey::Str(CompactString::from("tz"))) {
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
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let micros = now.subsec_micros();

    // Apply timezone offset if provided
    let offset_secs: i64 = tz_val.as_ref().and_then(|tz| {
        tz.get_attr("_offset_secs").and_then(|v| v.as_int())
    }).unwrap_or(0);

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

fn date_fromisoformat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("date.fromisoformat", args, 1)?;
    let s = args[0].py_to_string();
    let date_parts: Vec<&str> = s.split('-').collect();
    if date_parts.len() < 3 { return Err(PyException::value_error("Invalid isoformat string")); }
    let year: i64 = date_parts[0].parse().map_err(|_| PyException::value_error("Invalid year"))?;
    let month: i64 = date_parts[1].parse().map_err(|_| PyException::value_error("Invalid month"))?;
    let day: i64 = date_parts[2].split('T').next().unwrap_or("1")
        .parse().map_err(|_| PyException::value_error("Invalid day"))?;
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
        is_special: true, dict_storage: None,
    }));
    install_datetime_methods(&inst, year, month, day, hour, minute, second, microsecond);
    inst
}

/// Install all datetime instance methods (isoformat, strftime, astimezone, etc.) on the given instance.
/// Called from both make_datetime_instance and __init__.
fn install_datetime_methods(inst: &PyObjectRef, year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64, microsecond: i64) {
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
        w.insert(CompactString::from("tzinfo"), PyObject::none());

        // isoformat(sep='T') -> str
        let (y, mo, da, h, mi, s, us) = (year, month, day, hour, minute, second, microsecond);
        w.insert(CompactString::from("isoformat"), PyObject::native_closure(
            "datetime.isoformat", move |args: &[PyObjectRef]| {
                let sep = if args.is_empty() { "T".to_string() } else { args[0].py_to_string() };
                let base = format!("{:04}-{:02}-{:02}{}{:02}:{:02}:{:02}", y, mo, da, sep, h, mi, s);
                if us != 0 {
                    Ok(PyObject::str_val(CompactString::from(format!("{}.{:06}", base, us))))
                } else {
                    Ok(PyObject::str_val(CompactString::from(base)))
                }
            }
        ));

        // strftime(format) -> str (using shared format_time with full format codes)
        let (y2, mo2, da2, h2, mi2, s2) = (year, month, day, hour, minute, second);
        let ord = ymd_to_ordinal(year, month, day);
        let wd = ((ord + 6) % 7) as i64; // 0=Mon
        let wd_for_fmt = wd;
        let yday_for_fmt = {
            let md = days_in_month(year);
            let mut yd = day;
            for i in 0..(month - 1) as usize { if i < 12 { yd += md[i]; } }
            yd
        };
        w.insert(CompactString::from("strftime"), PyObject::native_closure(
            "datetime.strftime", move |args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("strftime requires format string")); }
                let fmt = args[0].py_to_string();
                let result = format_time(&fmt, y2, mo2, da2, h2, mi2, s2, wd_for_fmt, yday_for_fmt);
                Ok(PyObject::str_val(CompactString::from(result)))
            }
        ));

        // weekday() -> int (0=Monday, 6=Sunday)
        w.insert(CompactString::from("weekday"), PyObject::native_closure(
            "datetime.weekday", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(wd))
            }
        ));

        // isoweekday() -> int (1=Monday, 7=Sunday)
        let iwd = wd + 1;
        w.insert(CompactString::from("isoweekday"), PyObject::native_closure(
            "datetime.isoweekday", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(iwd))
            }
        ));

        // date() -> date object
        w.insert(CompactString::from("date"), PyObject::native_closure(
            "datetime.date", move |_: &[PyObjectRef]| {
                Ok(make_date_instance(y, mo, da))
            }
        ));

        // timestamp() -> float (POSIX timestamp)
        let ts = {
            let days_since_epoch = ymd_to_ordinal(year, month, day) - ymd_to_ordinal(1970, 1, 1);
            days_since_epoch as f64 * 86400.0 + hour as f64 * 3600.0 + minute as f64 * 60.0 + second as f64 + microsecond as f64 / 1_000_000.0
        };
        w.insert(CompactString::from("timestamp"), PyObject::native_closure(
            "datetime.timestamp", move |_: &[PyObjectRef]| {
                Ok(PyObject::float(ts))
            }
        ));

        // __str__() / __repr__()
        let iso = if microsecond != 0 {
            format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}", year, month, day, hour, minute, second, microsecond)
        } else {
            format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, hour, minute, second)
        };

        w.insert(CompactString::from("__str__"), PyObject::native_closure(
            "datetime.__str__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(&iso)))
            }
        ));
        w.insert(CompactString::from("__repr__"), PyObject::native_closure(
            "datetime.__repr__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(format!("datetime.datetime({}, {}, {}, {}, {}, {})", y, mo, da, h, mi, s))))
            }
        ));

        // timetuple() -> time.struct_time compatible tuple
        w.insert(CompactString::from("timetuple"), PyObject::native_closure(
            "datetime.timetuple", move |_: &[PyObjectRef]| {
                Ok(PyObject::tuple(vec![
                    PyObject::int(y), PyObject::int(mo), PyObject::int(da),
                    PyObject::int(h), PyObject::int(mi), PyObject::int(s),
                    PyObject::int(wd), PyObject::int(0), PyObject::int(-1),
                ]))
            }
        ));

        // astimezone(tz) -> datetime converted to target timezone
        // This closure receives args from method call; self attrs read at call-time
        let inst_ref = inst.clone();
        w.insert(CompactString::from("astimezone"), PyObject::native_closure(
            "datetime.astimezone", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(inst_ref.clone());
                }
                // Read source datetime fields from the instance
                let sy = inst_ref.get_attr("year").and_then(|v| v.as_int()).unwrap_or(1970);
                let smo = inst_ref.get_attr("month").and_then(|v| v.as_int()).unwrap_or(1);
                let sda = inst_ref.get_attr("day").and_then(|v| v.as_int()).unwrap_or(1);
                let sh = inst_ref.get_attr("hour").and_then(|v| v.as_int()).unwrap_or(0);
                let smi = inst_ref.get_attr("minute").and_then(|v| v.as_int()).unwrap_or(0);
                let ss = inst_ref.get_attr("second").and_then(|v| v.as_int()).unwrap_or(0);
                let sus = inst_ref.get_attr("microsecond").and_then(|v| v.as_int()).unwrap_or(0);

                // Get source timezone offset (0 if naive or UTC)
                let src_offset = inst_ref.get_attr("tzinfo")
                    .and_then(|tz| tz.get_attr("_offset_seconds"))
                    .and_then(|v| v.to_float().ok())
                    .unwrap_or(0.0);

                let target_tz = &args[0];
                let target_offset = target_tz.get_attr("_offset_seconds")
                    .and_then(|v| v.to_float().ok())
                    .unwrap_or(0.0);

                // Convert to UTC epoch seconds, then to target timezone
                let epoch_days = ymd_to_ordinal(sy, smo, sda) - ymd_to_ordinal(1970, 1, 1);
                let utc_secs = epoch_days as f64 * 86400.0
                    + sh as f64 * 3600.0 + smi as f64 * 60.0 + ss as f64
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
            }
        ));

        // utcoffset() -> timedelta or None
        w.insert(CompactString::from("utcoffset"), PyObject::native_closure(
            "datetime.utcoffset", move |_: &[PyObjectRef]| {
                Ok(PyObject::none())
            }
        ));

        // replace(**kwargs) -> datetime with replaced fields
        let (ry, rmo, rda, rh, rmi, rs, rus) = (year, month, day, hour, minute, second, microsecond);
        w.insert(CompactString::from("replace"), PyObject::native_closure(
            "datetime.replace", move |args: &[PyObjectRef]| {
                let mut ny = ry; let mut nmo = rmo; let mut nda = rda;
                let mut nh = rh; let mut nmi = rmi; let mut ns = rs; let mut nus = rus;
                // Accept kwargs dict
                if let Some(last) = args.last() {
                    if let PyObjectPayload::Dict(kw) = &last.payload {
                        let r = kw.read();
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("year"))) { ny = v.as_int().unwrap_or(ny); }
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("month"))) { nmo = v.as_int().unwrap_or(nmo); }
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("day"))) { nda = v.as_int().unwrap_or(nda); }
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("hour"))) { nh = v.as_int().unwrap_or(nh); }
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("minute"))) { nmi = v.as_int().unwrap_or(nmi); }
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("second"))) { ns = v.as_int().unwrap_or(ns); }
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("microsecond"))) { nus = v.as_int().unwrap_or(nus); }
                    }
                }
                // Also accept positional args: year, month, day, hour, minute, second, microsecond
                if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
                    if args.len() > 0 { ny = args[0].as_int().unwrap_or(ny); }
                    if args.len() > 1 { nmo = args[1].as_int().unwrap_or(nmo); }
                    if args.len() > 2 { nda = args[2].as_int().unwrap_or(nda); }
                    if args.len() > 3 { nh = args[3].as_int().unwrap_or(nh); }
                    if args.len() > 4 { nmi = args[4].as_int().unwrap_or(nmi); }
                    if args.len() > 5 { ns = args[5].as_int().unwrap_or(ns); }
                    if args.len() > 6 { nus = args[6].as_int().unwrap_or(nus); }
                }
                Ok(make_datetime_instance(ny, nmo, nda, nh, nmi, ns, nus))
            }
        ));
    }
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
        is_special: true, dict_storage: None,
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
        is_special: true, dict_storage: None,
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__datetime__"), PyObject::bool_val(true));
        w.insert(CompactString::from("__date_only__"), PyObject::bool_val(true));
        w.insert(CompactString::from("year"), PyObject::int(year));
        w.insert(CompactString::from("month"), PyObject::int(month));
        w.insert(CompactString::from("day"), PyObject::int(day));

        // isoformat() -> str
        let (y, mo, da) = (year, month, day);
        w.insert(CompactString::from("isoformat"), PyObject::native_closure(
            "date.isoformat", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(format!("{:04}-{:02}-{:02}", y, mo, da))))
            }
        ));

        // strftime(format) -> str (using shared format_time with full codes)
        let ord = ymd_to_ordinal(year, month, day);
        let wd = ((ord + 6) % 7) as i64;
        let yday_d = {
            let md = days_in_month(y);
            let mut yd = da;
            for i in 0..(mo - 1) as usize { if i < 12 { yd += md[i]; } }
            yd
        };
        let wd_d = wd;
        w.insert(CompactString::from("strftime"), PyObject::native_closure(
            "date.strftime", move |args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("strftime requires format string")); }
                let fmt = args[0].py_to_string();
                let result = format_time(&fmt, y, mo, da, 0, 0, 0, wd_d, yday_d);
                Ok(PyObject::str_val(CompactString::from(result)))
            }
        ));

        // weekday() -> int (0=Monday)
        w.insert(CompactString::from("weekday"), PyObject::native_closure(
            "date.weekday", move |_: &[PyObjectRef]| { Ok(PyObject::int(wd)) }
        ));

        // isoweekday() -> int (1=Monday)
        let iwd = wd + 1;
        w.insert(CompactString::from("isoweekday"), PyObject::native_closure(
            "date.isoweekday", move |_: &[PyObjectRef]| { Ok(PyObject::int(iwd)) }
        ));

        // __str__() / __repr__()
        w.insert(CompactString::from("__str__"), PyObject::native_closure(
            "date.__str__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(format!("{:04}-{:02}-{:02}", y, mo, da))))
            }
        ));
        w.insert(CompactString::from("__repr__"), PyObject::native_closure(
            "date.__repr__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(format!("datetime.date({}, {}, {})", y, mo, da))))
            }
        ));

        // toordinal() -> int
        w.insert(CompactString::from("toordinal"), PyObject::native_closure(
            "date.toordinal", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(ymd_to_ordinal(y, mo, da)))
            }
        ));

        // replace(**kwargs) -> date with replaced fields
        let (ry, rmo, rda) = (year, month, day);
        w.insert(CompactString::from("replace"), PyObject::native_closure(
            "date.replace", move |args: &[PyObjectRef]| {
                let mut ny = ry; let mut nmo = rmo; let mut nda = rda;
                if let Some(last) = args.last() {
                    if let PyObjectPayload::Dict(kw) = &last.payload {
                        let r = kw.read();
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("year"))) { ny = v.as_int().unwrap_or(ny); }
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("month"))) { nmo = v.as_int().unwrap_or(nmo); }
                        if let Some(v) = r.get(&HashableKey::Str(CompactString::from("day"))) { nda = v.as_int().unwrap_or(nda); }
                    }
                }
                if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
                    if args.len() > 0 { ny = args[0].as_int().unwrap_or(ny); }
                    if args.len() > 1 { nmo = args[1].as_int().unwrap_or(nmo); }
                    if args.len() > 2 { nda = args[2].as_int().unwrap_or(nda); }
                }
                Ok(make_date_instance(ny, nmo, nda))
            }
        ));
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
    td_ns.insert(CompactString::from("__truediv__"), make_builtin(timedelta_truediv));
    td_ns.insert(CompactString::from("__floordiv__"), make_builtin(timedelta_floordiv));
    td_ns.insert(CompactString::from("__eq__"), make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let a_ts = args[0].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(f64::NAN);
        let b_ts = args[1].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(f64::NAN);
        Ok(PyObject::bool_val((a_ts - b_ts).abs() < 1e-9))
    }));
    td_ns.insert(CompactString::from("__ne__"), make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Ok(PyObject::bool_val(true)); }
        let a_ts = args[0].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(f64::NAN);
        let b_ts = args[1].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(f64::NAN);
        Ok(PyObject::bool_val((a_ts - b_ts).abs() >= 1e-9))
    }));
    td_ns.insert(CompactString::from("__lt__"), make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let a_ts = args[0].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(0.0);
        let b_ts = args[1].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(0.0);
        Ok(PyObject::bool_val(a_ts < b_ts))
    }));
    td_ns.insert(CompactString::from("__le__"), make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let a_ts = args[0].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(0.0);
        let b_ts = args[1].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(0.0);
        Ok(PyObject::bool_val(a_ts <= b_ts))
    }));
    td_ns.insert(CompactString::from("__gt__"), make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let a_ts = args[0].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(0.0);
        let b_ts = args[1].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(0.0);
        Ok(PyObject::bool_val(a_ts > b_ts))
    }));
    td_ns.insert(CompactString::from("__ge__"), make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let a_ts = args[0].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(0.0);
        let b_ts = args[1].get_attr("_total_seconds").and_then(|v| v.to_float().ok()).unwrap_or(0.0);
        Ok(PyObject::bool_val(a_ts >= b_ts))
    }));
    let class = PyObject::class(CompactString::from("timedelta"), vec![], td_ns);
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
        is_special: true, dict_storage: None,
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__timedelta__"), PyObject::bool_val(true));
        w.insert(CompactString::from("days"), PyObject::int(days));
        w.insert(CompactString::from("seconds"), PyObject::int(seconds));
        w.insert(CompactString::from("microseconds"), PyObject::int(microseconds));
        w.insert(CompactString::from("_total_seconds"), PyObject::float(total_secs));
        w.insert(CompactString::from("_total_us"), PyObject::int(days * 86_400_000_000 + seconds * 1_000_000 + microseconds));
        // total_seconds() as a callable method
        let ts = total_secs;
        w.insert(CompactString::from("total_seconds"), PyObject::native_closure(
            "total_seconds", move |_args: &[PyObjectRef]| Ok(PyObject::float(ts))
        ));
        // __repr__ / __str__
        let repr = if microseconds != 0 {
            format!("datetime.timedelta(days={}, seconds={}, microseconds={})", days, seconds, microseconds)
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
        w.insert(CompactString::from("__repr__"), PyObject::native_closure(
            "__repr__", move |_: &[PyObjectRef]| Ok(PyObject::str_val(CompactString::from(&repr)))
        ));
        w.insert(CompactString::from("__str__"), PyObject::native_closure(
            "__str__", move |_: &[PyObjectRef]| Ok(PyObject::str_val(CompactString::from(&str_val)))
        ));
        // __bool__: timedelta is falsy only when all zero
        let is_nonzero = days != 0 || seconds != 0 || microseconds != 0;
        w.insert(CompactString::from("__bool__"), PyObject::native_closure(
            "__bool__", move |_: &[PyObjectRef]| Ok(PyObject::bool_val(is_nonzero))
        ));
        // __neg__
        let (nd, ns, nus) = (-days, -seconds, -microseconds);
        let nts = -total_secs;
        w.insert(CompactString::from("__neg__"), PyObject::native_closure(
            "__neg__", move |_: &[PyObjectRef]| make_timedelta_with_ops(nd, ns, nus, nts)
        ));
        // __abs__
        let (ad, as_, aus) = (days.abs(), seconds.abs(), microseconds.abs());
        let ats = total_secs.abs();
        w.insert(CompactString::from("__abs__"), PyObject::native_closure(
            "__abs__", move |_: &[PyObjectRef]| make_timedelta_with_ops(ad, as_, aus, ats)
        ));
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

/// timedelta / int_or_float → timedelta, timedelta / timedelta → float
fn timedelta_truediv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("timedelta.__truediv__ requires 2 args")); }
    let td = &args[0];
    let other = &args[1];
    let td_us = td.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0) * 86_400_000_000
        + td.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0) * 1_000_000
        + td.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);

    // timedelta / timedelta → float ratio
    if other.get_attr("__timedelta__").is_some() {
        let other_us = other.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0) * 86_400_000_000
            + other.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0) * 1_000_000
            + other.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);
        if other_us == 0 { return Err(PyException::runtime_error("division by zero")); }
        return Ok(PyObject::float(td_us as f64 / other_us as f64));
    }

    // timedelta / number → timedelta
    let divisor = other.to_float().map_err(|_| PyException::type_error(
        "unsupported operand type(s) for /: 'timedelta' and non-numeric"
    ))?;
    if divisor == 0.0 { return Err(PyException::runtime_error("division by zero")); }
    let result_us = (td_us as f64 / divisor).round() as i64;
    let days = result_us / 86_400_000_000;
    let rem = result_us % 86_400_000_000;
    let seconds = rem / 1_000_000;
    let microseconds = rem % 1_000_000;
    make_timedelta_with_ops(days, seconds, microseconds, result_us as f64 / 1_000_000.0)
}

/// timedelta // int → timedelta, timedelta // timedelta → int
fn timedelta_floordiv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("timedelta.__floordiv__ requires 2 args")); }
    let td = &args[0];
    let other = &args[1];
    let td_us = td.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0) * 86_400_000_000
        + td.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0) * 1_000_000
        + td.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);

    // timedelta // timedelta → int
    if other.get_attr("__timedelta__").is_some() {
        let other_us = other.get_attr("days").and_then(|v| v.as_int()).unwrap_or(0) * 86_400_000_000
            + other.get_attr("seconds").and_then(|v| v.as_int()).unwrap_or(0) * 1_000_000
            + other.get_attr("microseconds").and_then(|v| v.as_int()).unwrap_or(0);
        if other_us == 0 { return Err(PyException::runtime_error("division by zero")); }
        return Ok(PyObject::int(td_us / other_us));
    }

    // timedelta // int → timedelta
    let divisor = other.to_int().map_err(|_| PyException::type_error(
        "unsupported operand type(s) for //: 'timedelta' and non-numeric"
    ))?;
    if divisor == 0 { return Err(PyException::runtime_error("division by zero")); }
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
        ("TextCalendar", build_text_calendar_class(
            is_leap, days_in_month, weekday,
        )),
        ("HTMLCalendar", build_html_calendar_class(
            is_leap, days_in_month, weekday,
        )),
        ("Calendar", build_text_calendar_class(
            is_leap, days_in_month, weekday,
        )),
    ])
}

fn format_month_text(
    year: i64, month: i64, w: usize,
    days_in_month: fn(i64, i64) -> i64,
    weekday: fn(i64, i64, i64) -> i64,
) -> String {
    let month_names = ["", "January", "February", "March", "April", "May", "June",
                       "July", "August", "September", "October", "November", "December"];
    let mname = month_names.get(month as usize).unwrap_or(&"");
    let header = format!("{} {}", mname, year);
    let col_w = w.max(2);
    let total_w = col_w * 7 + 6;
    let mut lines = vec![format!("{:^width$}", header, width = total_w)];
    let day_hdrs: Vec<String> = ["Mo","Tu","We","Th","Fr","Sa","Su"]
        .iter().map(|d| format!("{:>width$}", d, width = col_w)).collect();
    lines.push(day_hdrs.join(" "));
    let first_wd = weekday(year, month, 1) as usize;
    let ndays = days_in_month(year, month) as usize;
    let mut line = format!("{:>width$} ", "", width = col_w).repeat(first_wd);
    // trim trailing space if line starts with padding
    if first_wd > 0 { line = line.trim_end().to_string(); line.push(' '); }
    let mut col = first_wd;
    for d in 1..=ndays {
        line.push_str(&format!("{:>width$}", d, width = col_w));
        col += 1;
        if col == 7 { lines.push(line.trim_end().to_string()); line = String::new(); col = 0; }
        else { line.push(' '); }
    }
    if col > 0 { lines.push(line.trim_end().to_string()); }
    lines.push(String::new());
    lines.join("\n")
}

fn build_text_calendar_class(
    _is_leap: fn(i64) -> bool,
    days_in_month: fn(i64, i64) -> i64,
    weekday: fn(i64, i64, i64) -> i64,
) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("TextCalendar"), vec![], IndexMap::new());
    let cls_ref = cls.clone();
    PyObject::native_closure("TextCalendar", move |args: &[PyObjectRef]| {
        let _firstweekday = if !args.is_empty() {
            args[0].to_int().unwrap_or(0)
        } else { 0 };
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("firstweekday"), PyObject::int(_firstweekday));

        attrs.insert(CompactString::from("formatmonth"),
            PyObject::native_closure("formatmonth", move |a: &[PyObjectRef]| {
                if a.len() < 2 {
                    return Err(PyException::type_error("formatmonth(year, month[, w[, l]])"));
                }
                let year = a[0].to_int()?;
                let month = a[1].to_int()?;
                let w = if a.len() > 2 { a[2].to_int().unwrap_or(2) as usize } else { 2 };
                Ok(PyObject::str_val(CompactString::from(
                    format_month_text(year, month, w, days_in_month, weekday)
                )))
            }));

        attrs.insert(CompactString::from("prmonth"),
            PyObject::native_closure("prmonth", move |a: &[PyObjectRef]| {
                if a.len() < 2 {
                    return Err(PyException::type_error("prmonth(year, month[, w[, l]])"));
                }
                let year = a[0].to_int()?;
                let month = a[1].to_int()?;
                let w = if a.len() > 2 { a[2].to_int().unwrap_or(2) as usize } else { 2 };
                let text = format_month_text(year, month, w, days_in_month, weekday);
                print!("{}", text);
                Ok(PyObject::none())
            }));

        attrs.insert(CompactString::from("formatyear"),
            PyObject::native_closure("formatyear", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("formatyear(year[, w[, l[, c[, m]]]])"));
                }
                let year = a[0].to_int()?;
                let w = if a.len() > 1 { a[1].to_int().unwrap_or(2) as usize } else { 2 };
                let mut result = format!("{:^66}\n\n", year);
                for q in 0..4 {
                    for m in 1..=3 {
                        let month = q * 3 + m;
                        result.push_str(&format_month_text(
                            year, month as i64, w, days_in_month, weekday
                        ));
                        result.push('\n');
                    }
                }
                Ok(PyObject::str_val(CompactString::from(result)))
            }));

        attrs.insert(CompactString::from("pryear"),
            PyObject::native_closure("pryear", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("pryear(year[, w[, l[, c[, m]]]])"));
                }
                let year = a[0].to_int()?;
                let mut result = format!("{:^66}\n\n", year);
                for q in 0..4 {
                    for m in 1..=3 {
                        let month = q * 3 + m;
                        result.push_str(&format_month_text(
                            year, month as i64, 2, days_in_month, weekday
                        ));
                        result.push('\n');
                    }
                }
                print!("{}", result);
                Ok(PyObject::none())
            }));

        Ok(PyObject::instance_with_attrs(cls_ref.clone(), attrs))
    })
}

fn build_html_calendar_class(
    _is_leap: fn(i64) -> bool,
    days_in_month: fn(i64, i64) -> i64,
    weekday: fn(i64, i64, i64) -> i64,
) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("HTMLCalendar"), vec![], IndexMap::new());
    let cls_ref = cls.clone();
    PyObject::native_closure("HTMLCalendar", move |args: &[PyObjectRef]| {
        let _firstweekday = if !args.is_empty() {
            args[0].to_int().unwrap_or(0)
        } else { 0 };
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("firstweekday"), PyObject::int(_firstweekday));

        attrs.insert(CompactString::from("formatmonth"),
            PyObject::native_closure("formatmonth", move |a: &[PyObjectRef]| {
                if a.len() < 2 {
                    return Err(PyException::type_error("formatmonth(year, month)"));
                }
                let year = a[0].to_int()?;
                let month = a[1].to_int()?;
                let month_names = ["", "January", "February", "March", "April", "May", "June",
                                   "July", "August", "September", "October", "November", "December"];
                let mname = month_names.get(month as usize).unwrap_or(&"");
                let first_wd = weekday(year, month, 1) as usize;
                let ndays = days_in_month(year, month) as usize;
                let mut html = String::from("<table border=\"0\" cellpadding=\"0\" cellspacing=\"0\" class=\"month\">\n");
                html.push_str(&format!("<tr><th colspan=\"7\" class=\"month\">{} {}</th></tr>\n", mname, year));
                html.push_str("<tr>");
                for dh in &["Mon","Tue","Wed","Thu","Fri","Sat","Sun"] {
                    html.push_str(&format!("<th class=\"{dh}\">{dh}</th>"));
                }
                html.push_str("</tr>\n<tr>");
                for _ in 0..first_wd { html.push_str("<td class=\"noday\">&nbsp;</td>"); }
                let mut col = first_wd;
                for d in 1..=ndays {
                    html.push_str(&format!("<td class=\"day\">{}</td>", d));
                    col += 1;
                    if col == 7 { html.push_str("</tr>\n<tr>"); col = 0; }
                }
                while col > 0 && col < 7 { html.push_str("<td class=\"noday\">&nbsp;</td>"); col += 1; }
                html.push_str("</tr>\n</table>\n");
                Ok(PyObject::str_val(CompactString::from(html)))
            }));

        Ok(PyObject::instance_with_attrs(cls_ref.clone(), attrs))
    })
}

// ── weakref module ──


