/// Apply format spec to an already-converted string value.
pub(crate) fn apply_format_spec_str(
    s: &str,
    spec: &str,
) -> ferrython_core::error::PyResult<String> {
    ferrython_core::object::format_value_spec_checked(s, spec)
}

pub(crate) fn apply_format_spec_int(n: i64, spec: &str) -> String {
    if spec.is_empty() {
        return n.to_string();
    }
    // Parse format spec: [[fill]align][sign][#][0][width][,][.precision][type]
    let chars: Vec<char> = spec.chars().collect();
    let len = chars.len();
    let type_char = chars[len - 1];
    match type_char {
        'd' => format_int_with_spec(n, &n.to_string(), spec),
        'b' => {
            let s = format!("{:b}", n.unsigned_abs());
            let prefix = if n < 0 { "-0b" } else { "0b" };
            format!("{}{}", prefix, s)
        }
        'o' => {
            let s = format!("{:o}", n.unsigned_abs());
            let prefix = if n < 0 { "-0o" } else { "0o" };
            format!("{}{}", prefix, s)
        }
        'x' => {
            let s = format!("{:x}", n.unsigned_abs());
            let prefix = if n < 0 { "-0x" } else { "0x" };
            format!("{}{}", prefix, s)
        }
        'X' => {
            let s = format!("{:X}", n.unsigned_abs());
            let prefix = if n < 0 { "-0X" } else { "0X" };
            format!("{}{}", prefix, s)
        }
        'n' => format_int_with_spec(n, &n.to_string(), spec),
        'c' => {
            if n >= 0 && n <= 0x10FFFF {
                char::from_u32(n as u32).map_or_else(|| n.to_string(), |c| c.to_string())
            } else {
                n.to_string()
            }
        }
        'e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%' => {
            // Delegate to float formatting
            apply_format_spec_float(n as f64, spec)
        }
        _ => {
            // Try as width specifier: e.g., "5" means right-align in 5 chars
            if let Ok(width) = spec.parse::<usize>() {
                format!("{:>width$}", n, width = width)
            } else {
                format_int_with_spec(n, &n.to_string(), spec)
            }
        }
    }
}

fn format_int_with_spec(n: i64, formatted: &str, spec: &str) -> String {
    // Handle comma separator
    if spec.contains(',') || spec.contains('_') {
        let sep = if spec.contains('_') { '_' } else { ',' };
        let abs_str = n.unsigned_abs().to_string();
        let mut result = String::new();
        for (i, c) in abs_str.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                result.push(sep);
            }
            result.push(c);
        }
        let s: String = result.chars().rev().collect();
        let s = if n < 0 { format!("-{}", s) } else { s };
        // Apply width
        let width = spec
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<usize>()
            .unwrap_or(0);
        if width > 0 {
            format!("{:>width$}", s, width = width)
        } else {
            s
        }
    } else {
        formatted.to_string()
    }
}

pub(crate) fn apply_format_spec_float(f: f64, spec: &str) -> String {
    if spec.is_empty() {
        return format_float_repr(f);
    }
    let chars: Vec<char> = spec.chars().collect();
    let len = chars.len();
    let type_char = chars[len - 1];
    // Extract precision from .N before type char
    let dot_pos = spec.find('.');
    let precision: usize = if let Some(dp) = dot_pos {
        spec[dp + 1..len - 1].parse().unwrap_or(6)
    } else {
        6
    };
    match type_char {
        'f' | 'F' => format!("{:.prec$}", f, prec = precision),
        'e' => format!("{:.prec$e}", f, prec = precision),
        'E' => format!("{:.prec$E}", f, prec = precision),
        'g' | 'G' => {
            if f.abs() >= 1e-4 && f.abs() < 10f64.powi(precision as i32) {
                let s = format!("{:.prec$}", f, prec = precision);
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            } else {
                format!("{:.prec$e}", f, prec = precision.saturating_sub(1))
            }
        }
        '%' => format!("{:.prec$}%", f * 100.0, prec = precision),
        'n' => format!("{}", f),
        _ => {
            if let Ok(width) = spec.parse::<usize>() {
                format!("{:>width$}", format_float_repr(f), width = width)
            } else {
                format_float_repr(f)
            }
        }
    }
}

pub(crate) fn format_float_repr(f: f64) -> String {
    if f.is_infinite() {
        return if f > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if f.is_nan() {
        return "nan".into();
    }
    let s = format!("{}", f);
    // Python always shows decimal point for float repr
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        format!("{}.0", s)
    } else {
        s
    }
}
