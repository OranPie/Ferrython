use super::*;

pub(super) fn datetime_now(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn date_today(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let days = now.as_secs() / 86400;
    let (year, month, day) = days_to_ymd(days as i64 + 719468);
    Ok(make_date_instance(year, month, day))
}

pub(super) fn datetime_fromtimestamp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("fromtimestamp", args, 1)?;
    let ts = args[0].to_float()?;
    datetime_from_posix_timestamp(ts)
}

pub(super) fn datetime_utcfromtimestamp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("utcfromtimestamp", args, 1)?;
    let ts = args[0].to_float()?;
    datetime_from_posix_timestamp(ts)
}

fn datetime_from_posix_timestamp(ts: f64) -> PyResult<PyObjectRef> {
    if !ts.is_finite() {
        return Err(PyException::overflow_error("timestamp out of range"));
    }
    let secs = ts.floor() as i64;
    let micros = ((ts - secs as f64) * 1_000_000.0).round() as i64;
    let days = secs.div_euclid(86400);
    let time_of_day = secs.rem_euclid(86400);
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days + 719468);
    Ok(make_datetime_instance(
        year, month, day, hour, minute, second, micros,
    ))
}

pub(super) fn datetime_combine(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn datetime_fromordinal(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("fromordinal", args, 1)?;
    let ordinal = args[0].to_int()?;
    let (year, month, day) = ordinal_to_ymd(ordinal);
    Ok(make_datetime_instance(year, month, day, 0, 0, 0, 0))
}

pub(super) fn date_fromordinal(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("fromordinal", args, 1)?;
    let ordinal = args[0].to_int()?;
    let (year, month, day) = ordinal_to_ymd(ordinal);
    Ok(make_date_instance(year, month, day))
}

pub(super) fn date_fromisoformat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn datetime_fromisoformat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

/// datetime.strptime(date_string, format) — parse a date string with a format specifier.
pub(super) fn datetime_strptime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
