use ferrython_core::object::{PyObjectMethods, PyObjectRef};

pub(super) fn current_asctime(datefmt: Option<&str>) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days as i64);
    if let Some(fmt) = datefmt {
        fmt.replace("%Y", &format!("{:04}", year))
            .replace("%m", &format!("{:02}", month))
            .replace("%d", &format!("{:02}", day))
            .replace("%H", &format!("{:02}", hours))
            .replace("%M", &format!("{:02}", minutes))
            .replace("%S", &format!("{:02}", seconds))
            .replace("%f", &format!("{:06}", millis * 1000))
    } else {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02},{:03}",
            year, month, day, hours, minutes, seconds, millis
        )
    }
}

fn days_to_ymd(mut days: i64) -> (i64, u32, u32) {
    let mut year = 1970i64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
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
    let mut month = 1u32;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, (days + 1) as u32)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub(super) fn format_log_message(fmt: &str, level_name: &str, name: &str, msg: &str) -> String {
    let asctime = current_asctime(None);
    fmt.replace("%(levelname)s", level_name)
        .replace("%(name)s", name)
        .replace("%(message)s", msg)
        .replace("%(asctime)s", &asctime)
        .replace("%(lineno)d", "0")
        .replace("%(filename)s", "")
        .replace("%(funcName)s", "")
        .replace("%(module)s", "")
        .replace("%(pathname)s", "")
}

pub(super) fn apply_percent_format(fmt: &str, args: &[PyObjectRef]) -> String {
    if args.is_empty() {
        return fmt.to_string();
    }
    let mut result = String::with_capacity(fmt.len() + 32);
    let mut chars = fmt.chars().peekable();
    let mut arg_idx = 0;
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(&next) = chars.peek() {
                match next {
                    's' => {
                        chars.next();
                        if arg_idx < args.len() {
                            result.push_str(&args[arg_idx].py_to_string());
                            arg_idx += 1;
                        } else {
                            result.push_str("%s");
                        }
                    }
                    'd' | 'i' => {
                        chars.next();
                        if arg_idx < args.len() {
                            result.push_str(&format!("{}", args[arg_idx].as_int().unwrap_or(0)));
                            arg_idx += 1;
                        } else {
                            result.push('%');
                            result.push(next);
                        }
                    }
                    'f' => {
                        chars.next();
                        if arg_idx < args.len() {
                            let val = args[arg_idx].to_float().unwrap_or(0.0);
                            result.push_str(&format!("{:.6}", val));
                            arg_idx += 1;
                        } else {
                            result.push_str("%f");
                        }
                    }
                    'r' => {
                        chars.next();
                        if arg_idx < args.len() {
                            result.push_str(&format!("'{}'", args[arg_idx].py_to_string()));
                            arg_idx += 1;
                        } else {
                            result.push_str("%r");
                        }
                    }
                    '.' => {
                        chars.next();
                        let mut precision = String::new();
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() {
                                precision.push(c);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if let Some(&fmt_char) = chars.peek() {
                            chars.next();
                            if fmt_char == 'f' && arg_idx < args.len() {
                                let prec: usize = precision.parse().unwrap_or(6);
                                let val = args[arg_idx].to_float().unwrap_or(0.0);
                                result.push_str(&format!("{:.prec$}", val, prec = prec));
                                arg_idx += 1;
                            } else {
                                result.push('%');
                                result.push('.');
                                result.push_str(&precision);
                                result.push(fmt_char);
                            }
                        }
                    }
                    '%' => {
                        chars.next();
                        result.push('%');
                    }
                    _ => {
                        result.push('%');
                    }
                }
            } else {
                result.push('%');
            }
        } else {
            result.push(ch);
        }
    }
    result
}
