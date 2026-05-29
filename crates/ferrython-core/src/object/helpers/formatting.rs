//! Range, slice, and Python formatting helpers.

use super::super::payload::*;
use super::{index_to_i128_unbounded, is_hidden_dict_key};
use crate::error::{PyException, PyResult};
use crate::object::methods::PyObjectMethods;
use compact_str::CompactString;
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

pub(crate) fn range_len_i128(start: i64, stop: i64, step: i64) -> i128 {
    let start = start as i128;
    let stop = stop as i128;
    let step = step as i128;
    if step > 0 && start < stop {
        (stop - start + step - 1) / step
    } else if step < 0 && start > stop {
        (start - stop - step - 1) / (-step)
    } else {
        0
    }
}

pub fn range_len(start: i64, stop: i64, step: i64) -> i64 {
    range_len_i128(start, stop, step).min(i64::MAX as i128) as i64
}

pub fn range_next_i64(current: i64, stop: i64, step: i64) -> Option<(i64, i64)> {
    let done = if step > 0 {
        current >= stop
    } else {
        current <= stop
    };
    if done {
        None
    } else {
        Some((current, current.checked_add(step).unwrap_or(stop)))
    }
}

pub fn range_bound_bigint(obj: Option<&PyObjectRef>, fallback: i64) -> BigInt {
    match obj.map(|value| &value.payload) {
        Some(PyObjectPayload::Int(n)) => n.to_bigint(),
        Some(PyObjectPayload::Bool(flag)) => BigInt::from(if *flag { 1 } else { 0 }),
        Some(PyObjectPayload::Instance(inst)) => inst
            .attrs
            .read()
            .get("__builtin_value__")
            .map(|value| range_bound_bigint(Some(value), fallback))
            .unwrap_or_else(|| BigInt::from(fallback)),
        _ => BigInt::from(fallback),
    }
}

pub fn range_parts_bigint(rd: &RangeData) -> (BigInt, BigInt, BigInt) {
    (
        range_bound_bigint(rd.start_obj.as_ref(), rd.start),
        range_bound_bigint(rd.stop_obj.as_ref(), rd.stop),
        range_bound_bigint(rd.step_obj.as_ref(), rd.step),
    )
}

pub fn range_len_bigint(start: &BigInt, stop: &BigInt, step: &BigInt) -> BigInt {
    if step.is_positive() && start < stop {
        let diff = stop - start;
        (diff + step - BigInt::one()) / step
    } else if step.is_negative() && start > stop {
        let step_abs = -step;
        let diff = start - stop;
        (diff + &step_abs - BigInt::one()) / step_abs
    } else {
        BigInt::zero()
    }
}

pub fn range_data_len_bigint(rd: &RangeData) -> BigInt {
    let (start, stop, step) = range_parts_bigint(rd);
    range_len_bigint(&start, &stop, &step)
}

pub fn range_data_len_i128(rd: &RangeData) -> i128 {
    let len = range_data_len_bigint(rd);
    len.to_i128().unwrap_or_else(|| {
        if len.is_negative() {
            i128::MIN
        } else {
            i128::MAX
        }
    })
}

pub fn range_data_is_empty(rd: &RangeData) -> bool {
    range_data_len_bigint(rd).is_zero()
}

pub fn range_item_bigint(rd: &RangeData, index: &BigInt) -> BigInt {
    let (start, _, step) = range_parts_bigint(rd);
    start + step * index
}

pub fn range_iter_item_bigint(iter: &BigRangeIterData) -> BigInt {
    &iter.start + &iter.step * &iter.index
}

pub fn range_iter_len_bigint(iter: &BigRangeIterData) -> BigInt {
    let current = range_iter_item_bigint(iter);
    range_len_bigint(&current, &iter.stop, &iter.step)
}

pub fn py_int_from_bigint(value: BigInt) -> PyObjectRef {
    if let Some(value) = value.to_i64() {
        PyObject::int(value)
    } else {
        PyObject::big_int(value)
    }
}

pub fn range_data_from_bigints(start: BigInt, stop: BigInt, step: BigInt) -> RangeData {
    let saturate = |value: &BigInt| {
        value.to_i64().unwrap_or_else(|| {
            if value.is_negative() {
                i64::MIN
            } else {
                i64::MAX
            }
        })
    };
    RangeData {
        start: saturate(&start),
        stop: saturate(&stop),
        step: saturate(&step),
        start_obj: Some(py_int_from_bigint(start)),
        stop_obj: Some(py_int_from_bigint(stop)),
        step_obj: Some(py_int_from_bigint(step)),
    }
}

pub fn py_int_bigint(obj: &PyObjectRef) -> Option<BigInt> {
    match &obj.payload {
        PyObjectPayload::Int(n) => Some(n.to_bigint()),
        PyObjectPayload::Bool(flag) => Some(BigInt::from(if *flag { 1 } else { 0 })),
        PyObjectPayload::Float(value) if value.fract() == 0.0 => value.to_i64().map(BigInt::from),
        PyObjectPayload::Complex { real, imag } if *imag == 0.0 && real.fract() == 0.0 => {
            real.to_i64().map(BigInt::from)
        }
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__builtin_value__")
            .and_then(py_int_bigint),
        _ => None,
    }
}

pub fn py_exact_numeric_bigint(obj: &PyObjectRef) -> Option<BigInt> {
    match &obj.payload {
        PyObjectPayload::Int(n) => Some(n.to_bigint()),
        PyObjectPayload::Bool(flag) => Some(BigInt::from(if *flag { 1 } else { 0 })),
        PyObjectPayload::Float(value) if value.fract() == 0.0 => value.to_i64().map(BigInt::from),
        PyObjectPayload::Complex { real, imag } if *imag == 0.0 && real.fract() == 0.0 => {
            real.to_i64().map(BigInt::from)
        }
        _ => None,
    }
}

pub fn range_contains_bigint(rd: &RangeData, value: &BigInt) -> bool {
    let (start, stop, step) = range_parts_bigint(rd);
    if step.is_positive() {
        value >= &start && value < &stop && (value - start).mod_floor(&step).is_zero()
    } else if step.is_negative() {
        let step_abs = -step;
        value <= &start && value > &stop && (start - value).mod_floor(&step_abs).is_zero()
    } else {
        false
    }
}

pub fn range_canonical_parts(rd: &RangeData) -> (BigInt, Option<BigInt>, Option<BigInt>) {
    let len = range_data_len_bigint(rd);
    if len.is_zero() {
        return (BigInt::zero(), None, None);
    }
    let start = range_item_bigint(rd, &BigInt::zero());
    if len == BigInt::one() {
        return (len, Some(start), None);
    }
    let step = range_parts_bigint(rd).2;
    (len, Some(start), Some(step))
}

pub fn range_iterator_from_data(rd: &RangeData) -> IteratorData {
    let (start, stop, step) = range_parts_bigint(rd);
    if let (Some(start_i64), Some(stop_i64), Some(step_i64)) =
        (start.to_i64(), stop.to_i64(), step.to_i64())
    {
        IteratorData::Range {
            current: start_i64,
            stop: stop_i64,
            step: step_i64,
        }
    } else {
        IteratorData::BigRange(BigRangeIterData {
            start,
            stop,
            step,
            index: BigInt::zero(),
        })
    }
}

pub fn iterator_supports_reduce(obj: &PyObjectRef) -> bool {
    matches!(
        &obj.payload,
        PyObjectPayload::Iterator(iter_data)
            if matches!(
                &*iter_data.read(),
                IteratorData::Islice { .. }
                    | IteratorData::TakeWhile { .. }
                    | IteratorData::DropWhile { .. }
                    | IteratorData::Tee { .. }
            )
    )
}

pub(in crate::object) fn float_to_str(f: f64) -> String {
    if f == f64::INFINITY {
        return "inf".into();
    }

    if f == f64::NEG_INFINITY {
        return "-inf".into();
    }
    if f.is_nan() {
        return "nan".into();
    }

    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }

    let abs_f = f.abs();
    // CPython uses scientific notation for |f| >= 1e16 or |f| < 1e-4
    if abs_f >= 1e16 || abs_f < 1e-4 {
        // Format as shortest scientific notation
        let s = format!("{:e}", f);
        // Python uses e+XX format (pad exponent to 2 digits minimum)
        // Rust gives e.g. "1e20", Python wants "1e+20"
        let s = if let Some(pos) = s.find('e') {
            let (mantissa, exp_part) = s.split_at(pos);
            let exp_str = &exp_part[1..]; // skip 'e'
            let exp: i32 = exp_str.parse().unwrap_or(0);
            if exp >= 0 {
                format!("{}e+{:02}", mantissa, exp)
            } else {
                format!("{}e-{:02}", mantissa, exp.abs())
            }
        } else {
            s
        };
        // Clean up trailing zeros in mantissa: 1.00000000000000000e+20 -> 1e+20
        if let Some(dot_pos) = s.find('.') {
            if let Some(e_pos) = s.find('e') {
                let frac = &s[dot_pos + 1..e_pos];
                let trimmed = frac.trim_end_matches('0');
                if trimmed.is_empty() {
                    format!("{}{}", &s[..dot_pos], &s[e_pos..])
                } else {
                    format!("{}.{}{}", &s[..dot_pos], trimmed, &s[e_pos..])
                }
            } else {
                s
            }
        } else {
            s
        }
    } else {
        // Use Rust's Debug which preserves precision
        let s = format!("{}", f);
        // Ensure it has a decimal point
        if s.contains('.') || s.contains('e') {
            s
        } else {
            format!("{}.0", s)
        }
    }
}

pub(in crate::object) fn python_fmod(a: f64, b: f64) -> f64 {
    let r = a % b;
    if (r != 0.0) && ((r < 0.0) != (b < 0.0)) {
        r + b
    } else {
        r
    }
}

pub(in crate::object) fn format_int_spec(n: i64, spec: &str) -> String {
    // Parse width from spec
    let width: usize = spec
        .trim_start_matches(|c: char| "- +#0".contains(c))
        .parse()
        .unwrap_or(0);
    let zero_pad = spec.starts_with('0');
    let left_align = spec.starts_with('-');
    let s = n.to_string();
    if width == 0 {
        return s;
    }
    if zero_pad && !left_align {
        if n < 0 {
            format!("-{:0>width$}", &s[1..], width = width - 1)
        } else {
            format!("{:0>width$}", s, width = width)
        }
    } else if left_align {
        format!("{:<width$}", s, width = width)
    } else {
        format!("{:>width$}", s, width = width)
    }
}

pub(in crate::object) fn format_float_spec(f: f64, spec: &str) -> String {
    // Parse precision from spec (e.g., ".2")
    if let Some(dot_pos) = spec.find('.') {
        let prec_str = &spec[dot_pos + 1..];
        let prec: usize = prec_str.parse().unwrap_or(6);
        format!("{:.prec$}", f, prec = prec)
    } else {
        format!("{:.6}", f)
    }
}

/// Parse precision from a printf spec string like ".2" or "10.3"
pub(in crate::object) fn parse_precision(spec: &str) -> Option<usize> {
    if let Some(dot_pos) = spec.find('.') {
        spec[dot_pos + 1..].parse().ok()
    } else {
        None
    }
}

/// Normalize Rust scientific notation to CPython format.
/// Rust: "1.23e3" or "1.23e-3"  →  Python: "1.23e+03" or "1.23e-03"
pub(in crate::object) fn normalize_scientific_exponent(raw: &str, e_char: char) -> String {
    if let Some(e_pos) = raw.rfind(e_char) {
        let mantissa = &raw[..e_pos];
        let exp_str = &raw[e_pos + 1..];
        let exp_val: i64 = exp_str.parse().unwrap_or(0);
        if exp_val >= 0 {
            format!("{}{}+{:02}", mantissa, e_char, exp_val)
        } else {
            format!("{}{}-{:02}", mantissa, e_char, -exp_val)
        }
    } else {
        raw.to_string()
    }
}

pub fn format_str_spec(s: &str, spec: &str) -> String {
    let left_align = spec.starts_with('-');
    let width_str = spec.trim_start_matches(|c: char| "-+ #0".contains(c));
    // Parse precision (max string length)
    let (width_part, precision) = if let Some(dot) = width_str.find('.') {
        (
            &width_str[..dot],
            width_str[dot + 1..].parse::<usize>().ok(),
        )
    } else {
        (width_str, None)
    };
    let width: usize = width_part.parse().unwrap_or(0);
    let display = if let Some(prec) = precision {
        if s.len() > prec {
            &s[..prec]
        } else {
            s
        }
    } else {
        s
    };
    if width == 0 {
        return display.to_string();
    }
    if left_align {
        format!("{:<width$}", display, width = width)
    } else {
        format!("{:>width$}", display, width = width)
    }
}

/// Python format spec mini-language: [[fill]align][sign][#][0][width][grouping][.precision][type]
pub fn format_value_spec(s: &str, spec: &str) -> String {
    if spec.is_empty() {
        return s.to_string();
    }
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    // Parse optional fill and align
    let (fill, align) = if chars.len() >= 2 && matches!(chars[1], '<' | '>' | '^' | '=') {
        i = 2;
        (chars[0], chars[1])
    } else if !chars.is_empty() && matches!(chars[0], '<' | '>' | '^' | '=') {
        i = 1;
        (' ', chars[0])
    } else {
        (' ', '<') // default: left-align for strings
    };
    // Parse width
    let mut width = 0usize;
    while i < chars.len() && chars[i].is_ascii_digit() {
        width = width * 10 + (chars[i] as usize - '0' as usize);
        i += 1;
    }
    // Parse .precision
    let mut precision: Option<usize> = None;
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        let mut p = 0usize;
        while i < chars.len() && chars[i].is_ascii_digit() {
            p = p * 10 + (chars[i] as usize - '0' as usize);
            i += 1;
        }
        precision = Some(p);
    }
    // Parse type (d, f, s, etc.) — ignored for string formatting
    // Apply precision (truncation for strings)
    let display = if let Some(prec) = precision {
        let chars_vec: Vec<char> = s.chars().collect();
        if chars_vec.len() > prec {
            chars_vec[..prec].iter().collect()
        } else {
            s.to_string()
        }
    } else {
        s.to_string()
    };
    if width == 0 || display.len() >= width {
        return display;
    }
    let pad = width - display.len();
    match align {
        '<' => format!(
            "{}{}",
            display,
            std::iter::repeat(fill).take(pad).collect::<String>()
        ),
        '>' => format!(
            "{}{}",
            std::iter::repeat(fill).take(pad).collect::<String>(),
            display
        ),
        '^' => {
            let left = pad / 2;
            let right = pad - left;
            format!(
                "{}{}{}",
                std::iter::repeat(fill).take(left).collect::<String>(),
                display,
                std::iter::repeat(fill).take(right).collect::<String>()
            )
        }
        _ => display,
    }
}

pub(in crate::object) fn add_thousands_separator(s: &str, sep: char) -> String {
    // Find the integer part (before any decimal point)
    let (sign, rest) = if s.starts_with('-') {
        ("-", &s[1..])
    } else {
        ("", s)
    };
    let (int_part, frac_part) = if let Some(dot) = rest.find('.') {
        (&rest[..dot], &rest[dot..])
    } else {
        (rest, "")
    };
    let mut result = String::new();
    for (i, ch) in int_part.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(sep);
        }
        result.push(ch);
    }
    let grouped: String = result.chars().rev().collect();
    format!("{}{}{}", sign, grouped, frac_part)
}

/// Apply sign and alignment to a numeric string. Handles +, -, space signs and width/fill.
pub fn apply_numeric_sign(value_str: &str, spec: &str) -> String {
    if spec.is_empty() {
        return value_str.to_string();
    }
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut fill = ' ';
    let mut align = None;
    let mut sign = '-'; // default: only show negative

    // Check for fill+align
    if chars.len() >= 2 && "<>^=".contains(chars[1]) {
        fill = chars[0];
        align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && "<>^=".contains(chars[0]) {
        align = Some(chars[0]);
        i = 1;
    }
    // Check for sign
    if i < chars.len() && "+-  ".contains(chars[i]) {
        sign = chars[i];
        i += 1;
    }
    // Check for # (alt form)
    if i < chars.len() && chars[i] == '#' {
        i += 1;
    }
    // Check for 0 fill (zero padding)
    if i < chars.len() && chars[i] == '0' && align.is_none() {
        fill = '0';
        align = Some('=');
        i += 1;
    }
    // Parse width
    let width_str: String = chars[i..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    i += width_str.len();
    let width: usize = width_str.parse().unwrap_or(0);

    // Parse .precision
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        // skip precision digits
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
    }

    // Apply sign to the numeric value
    let is_negative = value_str.starts_with('-');
    let digits = if is_negative {
        &value_str[1..]
    } else {
        value_str
    };
    let sign_str = if is_negative {
        "-"
    } else {
        match sign {
            '+' => "+",
            ' ' => " ",
            _ => "",
        }
    };

    let full = format!("{}{}", sign_str, digits);
    if width == 0 || full.len() >= width {
        return full;
    }

    let pad_len = width - full.len();
    let actual_align = align.unwrap_or('>');
    match actual_align {
        '<' => format!(
            "{}{}",
            full,
            std::iter::repeat(fill).take(pad_len).collect::<String>()
        ),
        '>' => format!(
            "{}{}",
            std::iter::repeat(fill).take(pad_len).collect::<String>(),
            full
        ),
        '=' => format!(
            "{}{}{}",
            sign_str,
            std::iter::repeat(fill).take(pad_len).collect::<String>(),
            digits
        ),
        '^' => {
            let left = pad_len / 2;
            let right = pad_len - left;
            format!(
                "{}{}{}",
                std::iter::repeat(fill).take(left).collect::<String>(),
                full,
                std::iter::repeat(fill).take(right).collect::<String>()
            )
        }
        _ => full,
    }
}

/// Apply formatting to a prefixed number (0x, 0o, 0b). Handles zero-padding between prefix and digits.
pub fn apply_prefixed_format(digits: &str, prefix: &str, spec: &str) -> String {
    if spec.is_empty() {
        return format!("{}{}", prefix, digits);
    }
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut fill = ' ';
    let mut align = None;

    // Check for fill+align
    if chars.len() >= 2 && "<>^=".contains(chars[1]) {
        fill = chars[0];
        align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && "<>^=".contains(chars[0]) {
        align = Some(chars[0]);
        i = 1;
    }
    // Skip sign
    if i < chars.len() && "+-  ".contains(chars[i]) {
        i += 1;
    }
    // Check for 0 fill
    if i < chars.len() && chars[i] == '0' && align.is_none() {
        fill = '0';
        align = Some('=');
        i += 1;
    }
    // Parse width
    let width_str: String = chars[i..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let width: usize = width_str.parse().unwrap_or(0);

    let full = format!("{}{}", prefix, digits);
    if width == 0 || full.len() >= width {
        return full;
    }

    let pad_len = width - full.len();
    match align.unwrap_or('>') {
        '=' | '>' if fill == '0' => {
            format!(
                "{}{}{}",
                prefix,
                std::iter::repeat('0').take(pad_len).collect::<String>(),
                digits
            )
        }
        '<' => format!(
            "{}{}",
            full,
            std::iter::repeat(fill).take(pad_len).collect::<String>()
        ),
        '>' => format!(
            "{}{}",
            std::iter::repeat(fill).take(pad_len).collect::<String>(),
            full
        ),
        '^' => {
            let left = pad_len / 2;
            let right = pad_len - left;
            format!(
                "{}{}{}",
                std::iter::repeat(fill).take(left).collect::<String>(),
                full,
                std::iter::repeat(fill).take(right).collect::<String>()
            )
        }
        _ => full,
    }
}

pub fn apply_string_format_spec(s: &str, spec: &str) -> String {
    if spec.is_empty() {
        return s.to_string();
    }
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut fill = ' ';
    let mut align = None;
    // Check for fill+align
    if chars.len() >= 2 && "<>^=".contains(chars[1]) {
        fill = chars[0];
        align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && "<>^".contains(chars[0]) {
        align = Some(chars[0]);
        i = 1;
    }
    // Check for sign
    if i < chars.len() && "+-".contains(chars[i]) {
        i += 1;
    }
    // Check for 0 fill (only when no explicit fill+align given)
    if i < chars.len() && chars[i] == '0' && align.is_none() {
        fill = '0';
        align = Some('>');
        i += 1;
    }
    // Parse width
    let width_str: String = chars[i..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let width: usize = width_str.parse().unwrap_or(0);
    i += width_str.len();
    // Parse precision (.N truncates string to N chars)
    let precision: Option<usize> = if i < chars.len() && chars[i] == '.' {
        i += 1;
        let prec_str: String = chars[i..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let _prec_len = prec_str.len(); // advance past precision digits
        i += _prec_len;
        let _ = i; // mark as intentionally used for future spec parsing
        Some(prec_str.parse().unwrap_or(0))
    } else {
        None
    };
    // Apply precision truncation
    let s = if let Some(prec) = precision {
        if s.chars().count() > prec {
            &s[..s
                .char_indices()
                .nth(prec)
                .map(|(i, _)| i)
                .unwrap_or(s.len())]
        } else {
            s
        }
    } else {
        s
    };
    if width <= s.len() {
        return s.to_string();
    }
    let pad_len = width - s.len();
    // Strings default to left-aligned (CPython behavior)
    match align.unwrap_or('<') {
        '<' => format!(
            "{}{}",
            s,
            std::iter::repeat(fill).take(pad_len).collect::<String>()
        ),
        '>' | '=' => format!(
            "{}{}",
            std::iter::repeat(fill).take(pad_len).collect::<String>(),
            s
        ),
        '^' => {
            let left = pad_len / 2;
            let right = pad_len - left;
            format!(
                "{}{}{}",
                std::iter::repeat(fill).take(left).collect::<String>(),
                s,
                std::iter::repeat(fill).take(right).collect::<String>()
            )
        }
        _ => s.to_string(),
    }
}

/// Resolve slice start/stop/step into actual indices for a sequence of given length.
pub(in crate::object) fn resolve_slice(
    start: &Option<PyObjectRef>,
    stop: &Option<PyObjectRef>,
    step: &Option<PyObjectRef>,
    len: i64,
) -> PyResult<(i64, i64, i64)> {
    let (start, stop, step) = resolve_slice_i128(start, stop, step, len as i128)?;
    let to_i64 = |n: i128| -> i64 {
        if n > i64::MAX as i128 {
            i64::MAX
        } else if n < i64::MIN as i128 {
            i64::MIN
        } else {
            n as i64
        }
    };
    Ok((to_i64(start), to_i64(stop), to_i64(step)))
}

pub(in crate::object) fn resolve_slice_i128(
    start: &Option<PyObjectRef>,
    stop: &Option<PyObjectRef>,
    step: &Option<PyObjectRef>,
    len: i128,
) -> PyResult<(i128, i128, i128)> {
    let step_val = step
        .as_ref()
        .and_then(|s| {
            if matches!(s.payload, PyObjectPayload::None) {
                None
            } else {
                Some(s)
            }
        })
        .map(index_to_i128_unbounded)
        .transpose()?
        .unwrap_or(1);
    if step_val == 0 {
        return Err(PyException::value_error("slice step cannot be zero"));
    }

    let (default_start, default_stop) = if step_val < 0 {
        (len - 1, -len - 1)
    } else {
        (0, len)
    };

    let normalize = |index: i128| {
        if index < 0 {
            let lower = if step_val < 0 { -1 } else { 0 };
            if index <= -len {
                lower
            } else {
                (len + index).max(lower)
            }
        } else if step_val < 0 {
            index.min(len - 1)
        } else {
            index.min(len)
        }
    };

    let start_val = start
        .as_ref()
        .and_then(|s| {
            if matches!(s.payload, PyObjectPayload::None) {
                None
            } else {
                Some(s)
            }
        })
        .map(index_to_i128_unbounded)
        .transpose()?
        .map(normalize)
        .unwrap_or(default_start);

    let stop_val = stop
        .as_ref()
        .and_then(|s| {
            if matches!(s.payload, PyObjectPayload::None) {
                None
            } else {
                Some(s)
            }
        })
        .map(index_to_i128_unbounded)
        .transpose()?
        .map(normalize)
        .unwrap_or(default_stop);

    Ok((start_val, stop_val, step_val))
}

pub(in crate::object) fn get_slice_impl(
    obj: &PyObjectRef,
    start: &Option<PyObjectRef>,
    stop: &Option<PyObjectRef>,
    step: &Option<PyObjectRef>,
) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::List(items) => {
            let items = items.read();
            let len = items.len() as i64;
            let (s, e, step) = resolve_slice(start, stop, step, len)?;
            let mut result = Vec::new();
            let mut i = s;
            if step > 0 {
                while i < e && i < len {
                    result.push(items[i as usize].clone());
                    let Some(next) = i.checked_add(step) else {
                        break;
                    };
                    i = next;
                }
            } else if step < 0 {
                while i > e && i >= 0 && i < len {
                    result.push(items[i as usize].clone());
                    let Some(next) = i.checked_add(step) else {
                        break;
                    };
                    i = next;
                }
            }
            Ok(PyObject::list(result))
        }
        PyObjectPayload::Tuple(items) => {
            let len = items.len() as i64;
            let (s, e, step) = resolve_slice(start, stop, step, len)?;
            let mut result = Vec::new();
            let mut i = s;
            if step > 0 {
                while i < e && i < len {
                    result.push(items[i as usize].clone());
                    let Some(next) = i.checked_add(step) else {
                        break;
                    };
                    i = next;
                }
            } else if step < 0 {
                while i > e && i >= 0 && i < len {
                    result.push(items[i as usize].clone());
                    let Some(next) = i.checked_add(step) else {
                        break;
                    };
                    i = next;
                }
            }
            Ok(PyObject::tuple(result))
        }
        PyObjectPayload::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let (sv, ev, step) = resolve_slice(start, stop, step, len)?;
            let mut result = String::new();
            let mut i = sv;
            if step > 0 {
                while i < ev && i < len {
                    result.push(chars[i as usize]);
                    let Some(next) = i.checked_add(step) else {
                        break;
                    };
                    i = next;
                }
            } else if step < 0 {
                while i > ev && i >= 0 && i < len {
                    result.push(chars[i as usize]);
                    let Some(next) = i.checked_add(step) else {
                        break;
                    };
                    i = next;
                }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            let len = b.len() as i64;
            let (sv, ev, step) = resolve_slice(start, stop, step, len)?;
            let mut result = Vec::new();
            let mut i = sv;
            if step > 0 {
                while i < ev && i < len {
                    result.push(b[i as usize]);
                    let Some(next) = i.checked_add(step) else {
                        break;
                    };
                    i = next;
                }
            } else if step < 0 {
                while i > ev && i >= 0 && i < len {
                    result.push(b[i as usize]);
                    let Some(next) = i.checked_add(step) else {
                        break;
                    };
                    i = next;
                }
            }
            Ok(PyObject::bytes(result))
        }
        PyObjectPayload::Range(rd) => {
            let len = range_data_len_bigint(rd);
            let clamp_index = |value: BigInt, step_negative: bool| {
                if value.is_negative() {
                    let lower = if step_negative {
                        -BigInt::one()
                    } else {
                        BigInt::zero()
                    };
                    if value <= -&len {
                        lower
                    } else {
                        (len.clone() + value).max(lower)
                    }
                } else if step_negative {
                    value.min(&len - BigInt::one())
                } else {
                    value.min(len.clone())
                }
            };
            let slice_step = step
                .as_ref()
                .and_then(|s| {
                    if matches!(s.payload, PyObjectPayload::None) {
                        None
                    } else {
                        Some(s)
                    }
                })
                .map(index_to_i128_unbounded)
                .transpose()?
                .map(BigInt::from)
                .unwrap_or_else(BigInt::one);
            if slice_step.is_zero() {
                return Err(PyException::value_error("slice step cannot be zero"));
            }
            let step_negative = slice_step.is_negative();
            let default_start = if step_negative {
                &len - BigInt::one()
            } else {
                BigInt::zero()
            };
            let default_stop = if step_negative {
                -&len - BigInt::one()
            } else {
                len.clone()
            };
            let slice_start = start
                .as_ref()
                .and_then(|s| {
                    if matches!(s.payload, PyObjectPayload::None) {
                        None
                    } else {
                        Some(s)
                    }
                })
                .map(index_to_i128_unbounded)
                .transpose()?
                .map(BigInt::from)
                .map(|value| clamp_index(value, step_negative))
                .unwrap_or(default_start);
            let slice_stop = stop
                .as_ref()
                .and_then(|s| {
                    if matches!(s.payload, PyObjectPayload::None) {
                        None
                    } else {
                        Some(s)
                    }
                })
                .map(index_to_i128_unbounded)
                .transpose()?
                .map(BigInt::from)
                .map(|value| clamp_index(value, step_negative))
                .unwrap_or(default_stop);
            let slice_len = range_len_bigint(&slice_start, &slice_stop, &slice_step);
            let new_start = range_item_bigint(rd, &slice_start);
            let new_step = range_parts_bigint(rd).2 * &slice_step;
            let new_stop = if slice_len.is_zero() {
                new_start.clone()
            } else {
                &new_start + &new_step * slice_len
            };
            Ok(PyObject::wrap(PyObjectPayload::Range(Box::new(
                range_data_from_bigints(new_start, new_stop, new_step),
            ))))
        }
        _ => Err(PyException::type_error(format!(
            "'{}' object is not subscriptable",
            obj.type_name()
        ))),
    }
}

/// Format a bytes literal like b'...' with proper escaping (shared by bytes and bytearray repr).
pub(in crate::object) fn format_bytes_literal(b: &[u8], prefix: &str) -> String {
    let mut out = String::new();
    out.push_str(prefix);
    out.push('\'');
    for &byte in b {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\'' => out.push_str("\\'"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\x{:02x}", byte)),
        }
    }
    out.push('\'');
    out
}

pub(in crate::object) fn format_collection(
    open: &str,
    close: &str,
    items: &[PyObjectRef],
) -> String {
    let inner: Vec<String> = items.iter().map(|i| i.repr()).collect();
    format!("{}{}{}", open, inner.join(", "), close)
}

pub(in crate::object) fn format_set(open: &str, close: &str, map: &FxHashKeyMap) -> String {
    let inner: Vec<String> = map.values().map(|v| v.repr()).collect();
    format!("{}{}{}", open, inner.join(", "), close)
}

pub(in crate::object) fn format_set_flat(
    open: &str,
    close: &str,
    map: &FxHashKeyFlatMap,
) -> String {
    let inner: Vec<String> = map.values().map(|v| v.repr()).collect();
    format!("{}{}{}", open, inner.join(", "), close)
}

pub(in crate::object) fn format_dict(map: &FxHashKeyMap) -> String {
    let inner: Vec<String> = map
        .iter()
        .filter(|(k, _)| !is_hidden_dict_key(k))
        .map(|(k, v)| format!("{}: {}", k.to_object().repr(), v.repr()))
        .collect();
    format!("{{{}}}", inner.join(", "))
}
