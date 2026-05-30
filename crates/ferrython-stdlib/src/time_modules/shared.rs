//! Shared calendar and formatting helpers for time and datetime modules.

// ── Shared time decomposition ──

pub(super) const MONTH_NAMES_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
pub(super) const MONTH_NAMES_FULL: [&str; 12] = [
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
pub(super) const DAY_NAMES_ABBR: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
pub(super) const DAY_NAMES_FULL: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

pub(super) fn is_leap_year(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

pub(super) fn days_in_month(y: i64) -> [i64; 12] {
    [
        31,
        if is_leap_year(y) { 29 } else { 28 },
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
    ]
}

/// Decompose Unix timestamp into (year, month 1-12, day 1-31, hour, min, sec, wday 0=Mon, yday 1-366)
pub(super) fn decompose_timestamp(epoch_secs: u64) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    let sec = (epoch_secs % 60) as i64;
    let min = ((epoch_secs / 60) % 60) as i64;
    let hour = ((epoch_secs / 3600) % 24) as i64;
    let total_days = (epoch_secs / 86400) as i64;
    let mut y: i64 = 1970;
    let mut remaining = total_days;
    loop {
        let dy = if is_leap_year(y) { 366 } else { 365 };
        if remaining < dy {
            break;
        }
        remaining -= dy;
        y += 1;
    }
    let md = days_in_month(y);
    let mut mon = 1i64;
    for &d in &md {
        if remaining < d {
            break;
        }
        remaining -= d;
        mon += 1;
    }
    let day = remaining + 1;
    let wday = ((total_days + 3) % 7) as i64; // epoch was Thursday, 0=Monday
    let yday = {
        let mut yd = day;
        for i in 0..(mon - 1) as usize {
            yd += md[i];
        }
        yd
    };
    (y, mon, day, hour, min, sec, wday, yday)
}
/// Format struct_time components using strftime format codes
pub(super) fn format_time(
    fmt: &str,
    y: i64,
    mon: i64,
    day: i64,
    h: i64,
    m: i64,
    s: i64,
    wday: i64,
    yday: i64,
) -> String {
    format_time_us(fmt, y, mon, day, h, m, s, 0, wday, yday)
}

pub(super) fn format_time_us(
    fmt: &str,
    y: i64,
    mon: i64,
    day: i64,
    h: i64,
    m: i64,
    s: i64,
    us: i64,
    wday: i64,
    yday: i64,
) -> String {
    let hour12 = if h == 0 {
        12
    } else if h > 12 {
        h - 12
    } else {
        h
    };
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
                Some('f') => result.push_str(&format!("{:06}", us)),
                Some('I') => result.push_str(&format!("{:02}", hour12)),
                Some('p') => result.push_str(ampm),
                Some('a') => result.push_str(DAY_NAMES_ABBR[wday as usize % 7]),
                Some('A') => result.push_str(DAY_NAMES_FULL[wday as usize % 7]),
                Some('b') | Some('h') => result.push_str(MONTH_NAMES_ABBR[(mon - 1) as usize % 12]),
                Some('B') => result.push_str(MONTH_NAMES_FULL[(mon - 1) as usize % 12]),
                Some('j') => result.push_str(&format!("{:03}", yday)),
                Some('w') => result.push_str(&format!("{}", (wday + 1) % 7)), // 0=Sunday
                Some('u') => result.push_str(&format!("{}", if wday == 6 { 7 } else { wday + 1 })), // 1=Monday
                Some('U') => result.push_str(&format!("{:02}", week_number_sunday(yday, wday))),
                Some('W') => result.push_str(&format!("{:02}", week_number_monday(yday, wday))),
                Some('y') => result.push_str(&format!("{:02}", y % 100)),
                Some('G') => {
                    // ISO year (may differ from calendar year at year boundaries)
                    let (iso_y, _, _) = iso_week_date(y, mon, day);
                    result.push_str(&format!("{:04}", iso_y));
                }
                Some('V') => {
                    // ISO week number (01-53)
                    let (_, iso_w, _) = iso_week_date(y, mon, day);
                    result.push_str(&format!("{:02}", iso_w));
                }
                Some('c') => {
                    result.push_str(&format!(
                        "{} {} {:2} {:02}:{:02}:{:02} {:04}",
                        DAY_NAMES_ABBR[wday as usize % 7],
                        MONTH_NAMES_ABBR[(mon - 1) as usize % 12],
                        day,
                        h,
                        m,
                        s,
                        y
                    ));
                }
                Some('x') => result.push_str(&format!("{:02}/{:02}/{:02}", mon, day, y % 100)),
                Some('X') => result.push_str(&format!("{:02}:{:02}:{:02}", h, m, s)),
                Some('Z') => result.push_str("UTC"),
                Some('z') => result.push_str("+0000"),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('%') => result.push('%'),
                Some(other) => {
                    result.push('%');
                    result.push(other);
                }
                None => result.push('%'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn week_number_sunday(yday: i64, wday: i64) -> i64 {
    let jan1_sunday = (wday - ((yday - 1) % 7)).rem_euclid(7);
    if yday <= (7 - jan1_sunday) % 7 {
        0
    } else {
        (yday - ((7 - jan1_sunday) % 7) + 6) / 7
    }
}

fn week_number_monday(yday: i64, wday: i64) -> i64 {
    let jan1_monday = (wday - ((yday - 1) % 7)).rem_euclid(7);
    if yday <= (7 - jan1_monday) % 7 {
        0
    } else {
        (yday - ((7 - jan1_monday) % 7) + 6) / 7
    }
}

/// ISO 8601 week date: returns (iso_year, iso_week, iso_weekday)
fn iso_week_date(y: i64, m: i64, d: i64) -> (i64, i64, i64) {
    let ord = ymd_to_ordinal(y, m, d);
    let wday = ((ord + 6) % 7) as i64; // 0=Mon
    let iso_wday = wday + 1; // 1=Mon
                             // Find Thursday of this week (ISO weeks are defined by Thursday)
    let thu_ord = ord + (3 - wday);
    // ISO year is the year that contains that Thursday
    let (thu_y, _, _) = ordinal_to_ymd(thu_ord);
    let jan1_ord = ymd_to_ordinal(thu_y, 1, 1);
    let jan1_wd = ((jan1_ord + 6) % 7) as i64;
    // Week 1 starts on the Monday <= Jan 4
    let week1_mon = jan1_ord - jan1_wd;
    let iso_week = (ord - week1_mon) / 7 + 1;
    (thu_y, iso_week, iso_wday)
}
pub(super) fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    // Civil days from epoch to Y-M-D (algorithm from Howard Hinnant)
    let z = days;
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
pub(super) fn ymd_to_ordinal(y: i64, m: i64, d: i64) -> i64 {
    // CPython-compatible: date(1, 1, 1).toordinal() == 1
    let dbm = [0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let y1 = y - 1;
    let days_before_year = y1 * 365 + y1 / 4 - y1 / 100 + y1 / 400;
    let is_leap = (y % 4 == 0) && (y % 100 != 0 || y % 400 == 0);
    let mut days_before_month = dbm[m as usize];
    if m > 2 && is_leap {
        days_before_month += 1;
    }
    days_before_year + days_before_month + d
}
pub(super) fn ordinal_to_ymd(mut ord: i64) -> (i64, i64, i64) {
    // CPython-compatible inverse of ymd_to_ordinal
    // Based on the algorithm from Lib/datetime.py _ord2ymd
    let n400 = (ord - 1) / 146097;
    ord -= n400 * 146097;
    let n100 = (ord - 1) / 36524;
    ord -= n100 * 36524;
    let n4 = (ord - 1) / 1461;
    ord -= n4 * 1461;
    let n1 = (ord - 1) / 365;
    ord -= n1 * 365;

    let year = n400 * 400 + n100 * 100 + n4 * 4 + n1 + 1;
    // ord is now the day-of-year (1-based) — but may need adjustment
    let day_of_year = if n1 == 4 || n100 == 4 {
        // Dec 31 of a leap year
        366
    } else {
        ord
    };
    let year = if n1 == 4 || n100 == 4 { year - 1 } else { year };

    let is_leap = (year % 4 == 0) && (year % 100 != 0 || year % 400 == 0);
    let dbm = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365];
    let mut month = 1i64;
    for m in (1..=12).rev() {
        let mut db = dbm[m as usize - 1];
        if m > 2 && is_leap {
            db += 1;
        }
        if day_of_year > db {
            month = m;
            break;
        }
    }
    let mut db = dbm[month as usize - 1];
    if month > 2 && is_leap {
        db += 1;
    }
    let day = day_of_year - db;
    (year, month, day)
}
