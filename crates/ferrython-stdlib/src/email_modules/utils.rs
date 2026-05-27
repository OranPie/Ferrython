use super::*;

// ── email.utils module ─────────────────────────────────────────────────

// RFC 2822 date formatting/parsing helpers
const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Compute day-of-week (0=Mon, 6=Sun) via Zeller-like formula
fn day_of_week(year: i64, month: i64, day: i64) -> i64 {
    let (y, m) = if month <= 2 {
        (year - 1, month + 12)
    } else {
        (year, month)
    };
    let dow = (day + (13 * (m + 1)) / 5 + y + y / 4 - y / 100 + y / 400) % 7;
    // Zeller gives 0=Sat, 1=Sun, 2=Mon, ...
    (dow + 5) % 7 // Convert to 0=Mon
}

/// Convert Unix timestamp to (year, month, day, hour, min, sec, weekday)
fn timestamp_to_components(ts: i64) -> (i64, i64, i64, i64, i64, i64, i64) {
    let secs = ts;
    let sec = secs.rem_euclid(60);
    let mins_total = secs.div_euclid(60);
    let min = mins_total.rem_euclid(60);
    let hours_total = mins_total.div_euclid(60);
    let hour = hours_total.rem_euclid(24);
    let mut days = hours_total.div_euclid(24);

    // Calculate year/month/day from days since epoch (1970-01-01 = Thursday)
    let mut year = 1970i64;
    loop {
        let dy = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            366
        } else {
            365
        };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let mut month = 1i64;
    loop {
        let dm = days_in_month(year, month);
        if days < dm {
            break;
        }
        days -= dm;
        month += 1;
    }
    let day = days + 1;
    let wday = day_of_week(year, month, day);
    (year, month, day, hour, min, sec, wday)
}

fn email_formatdate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::{SystemTime, UNIX_EPOCH};
    // formatdate(timeval=None, localtime=False, usegmt=False)
    let ts = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
        if let Ok(f) = args[0].to_float() {
            f as i64
        } else if let Some(i) = args[0].as_int() {
            i
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64
        }
    } else {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    };
    let (year, month, day, hour, min, sec, wday) = timestamp_to_components(ts);
    let formatted = format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} +0000",
        WEEKDAY_NAMES[wday as usize % 7],
        day,
        MONTH_NAMES[(month - 1) as usize % 12],
        year,
        hour,
        min,
        sec,
    );
    Ok(PyObject::str_val(CompactString::from(formatted)))
}

/// Parse an RFC 2822 date string into (year, month, day, hour, min, sec, wday, yday, tz_offset)
fn parse_rfc2822_date(s: &str) -> Option<(i64, i64, i64, i64, i64, i64, i64, i64, i64)> {
    // Formats: "Mon, 01 Jan 2024 12:00:00 +0000" or "01 Jan 2024 12:00:00 +0000"
    let s = s.trim();
    let date_part = if let Some(pos) = s.find(',') {
        s[pos + 1..].trim()
    } else {
        s
    };
    let parts: Vec<&str> = date_part.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }

    let day: i64 = parts[0].parse().ok()?;
    let month: i64 = match parts[1].to_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    let year: i64 = parts[2].parse().ok()?;
    // Adjust 2-digit years
    let year = if year < 100 {
        if year < 50 {
            year + 2000
        } else {
            year + 1900
        }
    } else {
        year
    };

    let (hour, min, sec) = if parts.len() > 3 {
        let time_parts: Vec<&str> = parts[3].split(':').collect();
        let h: i64 = time_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let m: i64 = time_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let s: i64 = time_parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        (h, m, s)
    } else {
        (0, 0, 0)
    };

    // Timezone offset
    let tz_offset: i64 = if parts.len() > 4 {
        let tz = parts[4];
        match tz {
            "UT" | "UTC" | "GMT" => 0,
            "EST" => -5 * 3600,
            "EDT" => -4 * 3600,
            "CST" => -6 * 3600,
            "CDT" => -5 * 3600,
            "MST" => -7 * 3600,
            "MDT" => -6 * 3600,
            "PST" => -8 * 3600,
            "PDT" => -7 * 3600,
            _ => {
                // Parse +HHMM / -HHMM
                let (sign, digits) = if tz.starts_with('+') {
                    (1i64, &tz[1..])
                } else if tz.starts_with('-') {
                    (-1i64, &tz[1..])
                } else {
                    (1, tz)
                };
                if digits.len() >= 4 {
                    let hh: i64 = digits[..2].parse().unwrap_or(0);
                    let mm: i64 = digits[2..4].parse().unwrap_or(0);
                    sign * (hh * 3600 + mm * 60)
                } else {
                    0
                }
            }
        }
    } else {
        0
    };

    let wday = day_of_week(year, month, day);
    // Calculate day-of-year
    let mut yday = day;
    for m in 1..month {
        yday += days_in_month(year, m);
    }

    Some((year, month, day, hour, min, sec, wday, yday, tz_offset))
}

fn email_parsedate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "parsedate() requires a date string",
        ));
    }
    let s = args[0].py_to_string();
    if let Some((year, month, day, hour, min, sec, wday, yday, _tz)) = parse_rfc2822_date(&s) {
        Ok(PyObject::tuple(vec![
            PyObject::int(year),
            PyObject::int(month),
            PyObject::int(day),
            PyObject::int(hour),
            PyObject::int(min),
            PyObject::int(sec),
            PyObject::int(wday),
            PyObject::int(yday),
            PyObject::int(-1),
        ]))
    } else {
        Ok(PyObject::none())
    }
}

fn email_formataddr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "formataddr() requires a (name, addr) pair",
        ));
    }
    // Expect a tuple (name, addr)
    let pair = &args[0];
    let (name, addr) = match &pair.payload {
        PyObjectPayload::Tuple(items) if items.len() >= 2 => {
            (items[0].py_to_string(), items[1].py_to_string())
        }
        _ => {
            return Err(PyException::type_error(
                "formataddr() argument must be a (name, addr) tuple",
            ));
        }
    };
    if name.is_empty() {
        Ok(PyObject::str_val(CompactString::from(addr)))
    } else {
        Ok(PyObject::str_val(CompactString::from(format!(
            "{} <{}>",
            name, addr
        ))))
    }
}

fn email_parseaddr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "parseaddr() requires an address string",
        ));
    }
    let addr_str = args[0].py_to_string();
    // Simple parsing: "Name <email>" or just "email"
    if let Some(lt) = addr_str.find('<') {
        if let Some(gt) = addr_str.find('>') {
            let name = addr_str[..lt].trim().to_string();
            let email = addr_str[lt + 1..gt].trim().to_string();
            return Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(name)),
                PyObject::str_val(CompactString::from(email)),
            ]));
        }
    }
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from("")),
        PyObject::str_val(CompactString::from(addr_str)),
    ]))
}

fn email_make_msgid(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let _ = args;
    // Generate a simple unique-ish message ID
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let msgid = format!("<{}.ferrython@localhost>", ts);
    Ok(PyObject::str_val(CompactString::from(msgid)))
}

pub fn create_email_utils_module() -> PyObjectRef {
    // parsedate_tz — like parsedate but includes timezone offset as 10th element
    let parsedate_tz_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "parsedate_tz() requires a date string",
            ));
        }
        let s = args[0].py_to_string();
        if let Some((year, month, day, hour, min, sec, wday, yday, tz)) = parse_rfc2822_date(&s) {
            Ok(PyObject::tuple(vec![
                PyObject::int(year),
                PyObject::int(month),
                PyObject::int(day),
                PyObject::int(hour),
                PyObject::int(min),
                PyObject::int(sec),
                PyObject::int(wday),
                PyObject::int(yday),
                PyObject::int(-1),
                PyObject::int(tz),
            ]))
        } else {
            Ok(PyObject::none())
        }
    });

    // parsedate_to_datetime — returns datetime.datetime from RFC 2822 string
    let parsedate_to_datetime_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "parsedate_to_datetime() requires a date string",
            ));
        }
        let s = args[0].py_to_string();
        if let Some((year, month, day, hour, min, sec, _wday, _yday, _tz)) = parse_rfc2822_date(&s)
        {
            // Build a datetime-like instance
            let cls = PyObject::class(CompactString::from("datetime"), vec![], IndexMap::new());
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("year"), PyObject::int(year));
            attrs.insert(CompactString::from("month"), PyObject::int(month));
            attrs.insert(CompactString::from("day"), PyObject::int(day));
            attrs.insert(CompactString::from("hour"), PyObject::int(hour));
            attrs.insert(CompactString::from("minute"), PyObject::int(min));
            attrs.insert(CompactString::from("second"), PyObject::int(sec));
            attrs.insert(CompactString::from("microsecond"), PyObject::int(0));
            let repr_str = format!(
                "datetime.datetime({}, {}, {}, {}, {}, {})",
                year, month, day, hour, min, sec
            );
            let str_val = format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                year, month, day, hour, min, sec
            );
            attrs.insert(
                CompactString::from("__str__"),
                PyObject::native_closure("__str__", move |_: &[PyObjectRef]| {
                    Ok(PyObject::str_val(CompactString::from(str_val.clone())))
                }),
            );
            attrs.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("__repr__", move |_: &[PyObjectRef]| {
                    Ok(PyObject::str_val(CompactString::from(repr_str.clone())))
                }),
            );
            Ok(PyObject::instance_with_attrs(cls, attrs))
        } else {
            Err(PyException::value_error(format!(
                "Invalid date header: {}",
                s
            )))
        }
    });

    // decode_rfc2231 — simplified RFC 2231 parameter decoding
    let decode_rfc2231_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "decode_rfc2231() requires a string",
            ));
        }
        let s = args[0].py_to_string();
        // Format: charset'language'value or just value
        let parts: Vec<&str> = s.splitn(3, '\'').collect();
        if parts.len() == 3 {
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(parts[0])),
                PyObject::str_val(CompactString::from(parts[1])),
                PyObject::str_val(CompactString::from(parts[2])),
            ]))
        } else {
            Ok(PyObject::tuple(vec![
                PyObject::none(),
                PyObject::none(),
                PyObject::str_val(CompactString::from(s)),
            ]))
        }
    });

    make_module(
        "email.utils",
        vec![
            ("formatdate", make_builtin(email_formatdate)),
            ("parsedate", make_builtin(email_parsedate)),
            ("parsedate_tz", parsedate_tz_fn),
            ("parsedate_to_datetime", parsedate_to_datetime_fn),
            ("formataddr", make_builtin(email_formataddr)),
            ("parseaddr", make_builtin(email_parseaddr)),
            ("make_msgid", make_builtin(email_make_msgid)),
            ("decode_rfc2231", decode_rfc2231_fn),
            (
                "collapse_rfc2231_value",
                make_builtin(|args: &[PyObjectRef]| {
                    // collapse_rfc2231_value((charset, language, text)) -> text
                    if let Some(t) = args.first() {
                        if let PyObjectPayload::Tuple(items) = &t.payload {
                            if items.len() >= 3 {
                                return Ok(items[2].clone());
                            }
                        }
                    }
                    Ok(args.first().cloned().unwrap_or_else(PyObject::none))
                }),
            ),
        ],
    )
}
