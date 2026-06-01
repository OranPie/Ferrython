use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    checked_format_accumulate, checked_repeat_len, parse_format_precision, py_ascii_repr, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;
use num_bigint::BigInt;
use num_traits::Signed;

fn format_percent_radix_arg(
    arg: &PyObjectRef,
    spec: char,
    alternate: bool,
) -> Result<String, PyException> {
    let n = match &arg.payload {
        PyObjectPayload::Bool(v) => BigInt::from(if *v { 1 } else { 0 }),
        PyObjectPayload::Int(PyInt::Small(v)) => BigInt::from(*v),
        PyObjectPayload::Int(PyInt::Big(v)) => v.as_ref().clone(),
        _ => {
            return Err(PyException::type_error(&format!(
                "%{} format: an integer is required, not {}",
                spec,
                arg.type_name()
            )))
        }
    };
    let negative = n < BigInt::from(0);
    let radix = if spec == 'o' { 8 } else { 16 };
    let mut digits = n.abs().to_str_radix(radix);
    if spec == 'X' {
        digits.make_ascii_uppercase();
    }
    let prefix = if alternate {
        match spec {
            'o' => "0o",
            'x' => "0x",
            'X' => "0X",
            _ => "",
        }
    } else {
        ""
    };
    if negative {
        Ok(format!("-{}{}", prefix, digits))
    } else {
        Ok(format!("{}{}", prefix, digits))
    }
}

fn percent_int_string_arg(arg: &PyObjectRef, spec: char) -> Result<String, PyException> {
    match &arg.payload {
        PyObjectPayload::Bool(value) => Ok(i64::from(*value).to_string()),
        PyObjectPayload::Int(value) => Ok(value.to_string()),
        PyObjectPayload::Float(value) => Ok((*value as i64).to_string()),
        PyObjectPayload::Instance(inst) => {
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                return percent_int_string_arg(&value, spec);
            }
            let n = arg.as_int().ok_or_else(|| {
                PyException::type_error(format!(
                    "%{} format: a number is required, not {}",
                    spec,
                    arg.type_name()
                ))
            })?;
            Ok(n.to_string())
        }
        _ => {
            let n = arg.as_int().ok_or_else(|| {
                PyException::type_error(format!(
                    "%{} format: a number is required, not {}",
                    spec,
                    arg.type_name()
                ))
            })?;
            Ok(n.to_string())
        }
    }
}

fn percent_memory_error(err: PyException) -> PyException {
    err
}

fn percent_star_to_width(value: i64, flags: &mut String) -> Result<usize, PyException> {
    if value < 0 {
        if !flags.contains('-') {
            flags.push('-');
        }
        value
            .checked_abs()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| PyException::overflow_error("format width too large"))
    } else {
        usize::try_from(value).map_err(|_| PyException::overflow_error("format width too large"))
    }
}

fn percent_star_to_precision(value: i64) -> Result<Option<usize>, PyException> {
    if value < 0 {
        return Ok(None);
    }
    let precision =
        usize::try_from(value).map_err(|_| PyException::overflow_error("precision too big"))?;
    if precision > i32::MAX as usize {
        return Err(PyException::overflow_error("precision too big"));
    }
    checked_repeat_len(1, precision, "format precision")
        .map_err(|_| PyException::overflow_error("precision too big"))?;
    Ok(Some(precision))
}

fn apply_percent_width(formatted: &str, width: usize, flags: &str) -> Result<String, PyException> {
    if width == 0 || formatted.len() >= width {
        return Ok(formatted.to_string());
    }
    let pad_len = width - formatted.len();
    checked_repeat_len(1, pad_len, "format width").map_err(percent_memory_error)?;
    if flags.contains('-') {
        Ok(format!("{}{}", formatted, " ".repeat(pad_len)))
    } else if flags.contains('0') && formatted.starts_with('-') {
        Ok(format!("-{}{}", "0".repeat(pad_len), &formatted[1..]))
    } else {
        let pad = if flags.contains('0') { '0' } else { ' ' };
        Ok(format!("{}{}", pad.to_string().repeat(pad_len), formatted))
    }
}

fn apply_percent_precision(
    formatted: &str,
    precision: Option<usize>,
) -> Result<String, PyException> {
    let Some(precision) = precision else {
        return Ok(formatted.to_string());
    };
    if formatted.len() >= precision {
        return Ok(formatted.to_string());
    }
    let pad_len = precision - formatted.len();
    checked_repeat_len(1, pad_len, "format precision")?;
    let (sign, body) = if let Some(rest) = formatted.strip_prefix('-') {
        ("-", rest)
    } else if let Some(rest) = formatted.strip_prefix('+') {
        ("+", rest)
    } else if let Some(rest) = formatted.strip_prefix(' ') {
        (" ", rest)
    } else {
        ("", formatted)
    };
    Ok(format!("{}{}{}", sign, "0".repeat(pad_len), body))
}

fn apply_percent_radix_precision(
    formatted: &str,
    precision: Option<usize>,
) -> Result<String, PyException> {
    let Some(precision) = precision else {
        return Ok(formatted.to_string());
    };
    let (sign, rest) = if let Some(rest) = formatted.strip_prefix('-') {
        ("-", rest)
    } else if let Some(rest) = formatted.strip_prefix('+') {
        ("+", rest)
    } else if let Some(rest) = formatted.strip_prefix(' ') {
        (" ", rest)
    } else {
        ("", formatted)
    };
    let (prefix, digits) = if let Some(rest) = rest.strip_prefix("0x") {
        ("0x", rest)
    } else if let Some(rest) = rest.strip_prefix("0X") {
        ("0X", rest)
    } else if let Some(rest) = rest.strip_prefix("0o") {
        ("0o", rest)
    } else {
        ("", rest)
    };
    if digits.len() >= precision {
        return Ok(formatted.to_string());
    }
    let pad_len = precision - digits.len();
    checked_repeat_len(1, pad_len, "format precision")?;
    Ok(format!(
        "{}{}{}{}",
        sign,
        prefix,
        "0".repeat(pad_len),
        digits
    ))
}

fn apply_percent_numeric_width(
    formatted: &str,
    width: usize,
    flags: &str,
    _precision: Option<usize>,
) -> Result<String, PyException> {
    if width == 0 || formatted.len() >= width {
        return Ok(formatted.to_string());
    }
    let pad_len = width - formatted.len();
    checked_repeat_len(1, pad_len, "format width").map_err(percent_memory_error)?;
    if flags.contains('-') {
        Ok(format!("{}{}", formatted, " ".repeat(pad_len)))
    } else if flags.contains('0') {
        let (sign, body) = if let Some(rest) = formatted.strip_prefix("-0x") {
            ("-0x", rest)
        } else if let Some(rest) = formatted.strip_prefix("-0X") {
            ("-0X", rest)
        } else if let Some(rest) = formatted.strip_prefix("-0o") {
            ("-0o", rest)
        } else if let Some(rest) = formatted.strip_prefix("+0x") {
            ("+0x", rest)
        } else if let Some(rest) = formatted.strip_prefix("+0X") {
            ("+0X", rest)
        } else if let Some(rest) = formatted.strip_prefix("+0o") {
            ("+0o", rest)
        } else if let Some(rest) = formatted.strip_prefix(" 0x") {
            (" 0x", rest)
        } else if let Some(rest) = formatted.strip_prefix(" 0X") {
            (" 0X", rest)
        } else if let Some(rest) = formatted.strip_prefix(" 0o") {
            (" 0o", rest)
        } else if let Some(rest) = formatted.strip_prefix('+') {
            ("+", rest)
        } else if let Some(rest) = formatted.strip_prefix('-') {
            ("-", rest)
        } else if let Some(rest) = formatted.strip_prefix(' ') {
            (" ", rest)
        } else if let Some(rest) = formatted.strip_prefix("0x") {
            ("0x", rest)
        } else if let Some(rest) = formatted.strip_prefix("0X") {
            ("0X", rest)
        } else if let Some(rest) = formatted.strip_prefix("0o") {
            ("0o", rest)
        } else {
            ("", formatted)
        };
        Ok(format!("{}{}{}", sign, "0".repeat(pad_len), body))
    } else {
        Ok(format!("{}{}", " ".repeat(pad_len), formatted))
    }
}

fn apply_percent_sign(mut formatted: String, flags: &str) -> String {
    if !formatted.starts_with('-') && !formatted.starts_with('+') {
        if flags.contains('+') {
            formatted.insert(0, '+');
        } else if flags.contains(' ') {
            formatted.insert(0, ' ');
        }
    }
    formatted
}

fn normalize_float_percent_case(mut formatted: String, spec: char) -> String {
    if matches!(spec, 'f' | 'e' | 'g') {
        if formatted.contains("NaN") {
            formatted = formatted.replace("NaN", "nan");
        }
        if formatted.contains("NAN") {
            formatted = formatted.replace("NAN", "nan");
        }
        if formatted.contains("Inf") {
            formatted = formatted.replace("Inf", "inf");
        }
        if formatted.contains("INF") {
            formatted = formatted.replace("INF", "inf");
        }
    } else if matches!(spec, 'F' | 'E' | 'G') {
        if formatted.contains("NaN") {
            formatted = formatted.replace("NaN", "NAN");
        }
        if formatted.contains("nan") {
            formatted = formatted.replace("nan", "NAN");
        }
        if formatted.contains("Inf") {
            formatted = formatted.replace("Inf", "INF");
        }
        if formatted.contains("inf") {
            formatted = formatted.replace("inf", "INF");
        }
    }
    formatted
}

fn format_percent_fixed_float(value: f64, precision: usize, spec: char) -> String {
    if precision <= 10_000 {
        return normalize_float_percent_case(format!("{:.prec$}", value, prec = precision), spec);
    }
    if value.is_nan() {
        return normalize_float_percent_case("nan".to_string(), spec);
    }
    if value.is_infinite() {
        return normalize_float_percent_case(
            if value.is_sign_negative() {
                "-inf".to_string()
            } else {
                "inf".to_string()
            },
            spec,
        );
    }
    let mut base = format!("{:.6}", value);
    if let Some(dot) = base.find('.') {
        base.truncate(dot + 1);
    } else {
        base.push('.');
    }
    checked_repeat_len(1, precision, "format precision")
        .map(|_| {
            base.push_str(&"0".repeat(precision));
            normalize_float_percent_case(base, spec)
        })
        .unwrap_or_else(|_| normalize_float_percent_case(format!("{:.6}", value), spec))
}

fn trim_percent_g(mut text: String, alternate: bool) -> String {
    if alternate {
        return text;
    }
    if let Some(exp_pos) = text.find(['e', 'E']) {
        let (mantissa, exponent) = text.split_at(exp_pos);
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        text = format!("{}{}", mantissa, exponent);
    } else if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    text
}

fn parse_percent_sci_exponent(raw: &str, e_char: char) -> i32 {
    raw.rfind(e_char)
        .and_then(|e_pos| raw[e_pos + e_char.len_utf8()..].parse::<i32>().ok())
        .unwrap_or(0)
}

fn ensure_percent_alternate_decimal(mut text: String) -> String {
    if let Some(exp_pos) = text.find(['e', 'E']) {
        if !text[..exp_pos].contains('.') {
            text.insert(exp_pos, '.');
        }
    } else if !text.contains('.') {
        text.push('.');
    }
    text
}

fn format_percent_general_float(
    value: f64,
    precision: Option<usize>,
    spec: char,
    alternate: bool,
) -> Result<String, PyException> {
    if value.is_nan() {
        return Ok(normalize_float_percent_case("nan".to_string(), spec));
    }
    if value.is_infinite() {
        let text = if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        };
        return Ok(normalize_float_percent_case(text.to_string(), spec));
    }

    let precision = precision.unwrap_or(6).max(1);
    checked_repeat_len(1, precision, "format precision")
        .map_err(|_| PyException::value_error("precision too big"))?;
    let sci_precision = precision - 1;
    let e_char = if spec == 'g' { 'e' } else { 'E' };
    let sci_raw = if e_char == 'e' {
        format!("{:.prec$e}", value, prec = sci_precision)
    } else {
        format!("{:.prec$E}", value, prec = sci_precision)
    };
    let sci_exponent = parse_percent_sci_exponent(&sci_raw, e_char);
    let use_sci = sci_exponent < -4 || sci_exponent >= precision as i32;

    let text = if use_sci {
        normalize_float_percent_case(normalize_sci_exp(&sci_raw, e_char), spec)
    } else {
        let fixed_precision = if sci_exponent < 0 {
            precision + (-sci_exponent as usize) - 1
        } else {
            precision.saturating_sub(sci_exponent as usize + 1)
        };
        format_percent_fixed_float(value, fixed_precision, spec)
    };

    if alternate {
        Ok(ensure_percent_alternate_decimal(text))
    } else {
        Ok(trim_percent_g(text, false))
    }
}

fn one_byte_arg(arg: &PyObjectRef) -> Option<u8> {
    match &arg.payload {
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) if bytes.len() == 1 => {
            Some(bytes[0])
        }
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__builtin_value__")
            .and_then(one_byte_arg),
        _ => None,
    }
}

fn bytes_like_arg(arg: &PyObjectRef) -> Option<Vec<u8>> {
    match &arg.payload {
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
            Some((**bytes).clone())
        }
        PyObjectPayload::Instance(inst) => {
            if inst.attrs.read().contains_key("__memoryview__") {
                return inst.attrs.read().get("obj").and_then(bytes_like_arg);
            }
            inst.attrs
                .read()
                .get("__builtin_value__")
                .and_then(bytes_like_arg)
        }
        _ => None,
    }
}

fn bytes_format_arg(vm: &mut VirtualMachine, arg: &PyObjectRef) -> Result<Vec<u8>, PyException> {
    if let Some(bytes) = bytes_like_arg(arg) {
        return Ok(bytes);
    }
    if let Some(bytes_method) = arg.get_attr("__bytes__") {
        let result = vm.call_object(bytes_method, vec![])?;
        if let Some(bytes) = bytes_like_arg(&result) {
            return Ok(bytes);
        }
    }
    Err(PyException::type_error(format!(
        "%b requires a bytes-like object, or an object that implements __bytes__, not '{}'",
        arg.type_name()
    )))
}

fn apply_string_percent_precision(
    formatted: String,
    precision: Option<usize>,
) -> Result<String, PyException> {
    let Some(precision) = precision else {
        return Ok(formatted);
    };
    if formatted.chars().count() <= precision {
        return Ok(formatted);
    }
    Ok(formatted.chars().take(precision).collect())
}

fn apply_bytes_percent_precision(
    formatted: Vec<u8>,
    precision: Option<usize>,
) -> Result<Vec<u8>, PyException> {
    let Some(precision) = precision else {
        return Ok(formatted);
    };
    if formatted.len() <= precision {
        return Ok(formatted);
    }
    Ok(formatted[..precision].to_vec())
}

fn percent_no_conversion_accepts_arg(args: &PyObjectRef, bytes_format: bool) -> bool {
    match &args.payload {
        PyObjectPayload::Dict(_)
        | PyObjectPayload::MappingProxy(_)
        | PyObjectPayload::List(_)
        | PyObjectPayload::Range(_) => true,
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => !bytes_format,
        PyObjectPayload::Tuple(items) => items.is_empty(),
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__memoryview__") || inst.dict_storage.is_some() {
                return true;
            }
            if attrs.contains_key("__deque__") {
                return false;
            }
            let base = match &inst.class.payload {
                PyObjectPayload::Class(cd) => {
                    cd.builtin_base_name.as_ref().map(|name| name.as_str())
                }
                _ => None,
            };
            if let Some(base) = base {
                if base == "tuple" {
                    if let Some(value) = attrs.get("__builtin_value__") {
                        return matches!(&value.payload, PyObjectPayload::Tuple(items) if items.is_empty());
                    }
                    return true;
                }
                if matches!(base, "bytes" | "bytearray") {
                    return !bytes_format;
                }
                return matches!(base, "list" | "dict" | "range");
            }
            false
        }
        _ => false,
    }
}

/// Python printf-style string formatting: "hello %s, %d items" % (name, count)
impl VirtualMachine {
    /// VM-aware string % formatting. Uses vm_repr/vm_str to properly call user
    /// __repr__/__str__ dunders that need VM context.
    pub(super) fn vm_string_percent_format(
        &mut self,
        fmt: &str,
        args: &PyObjectRef,
    ) -> Result<PyObjectRef, PyException> {
        let arg_list: Vec<PyObjectRef> = match &args.payload {
            PyObjectPayload::Tuple(items) => (**items).clone(),
            _ => vec![args.clone()],
        };
        let using_tuple_args = matches!(&args.payload, PyObjectPayload::Tuple(_));
        let mut used_mapping_key = false;
        let mut consumed_arg = false;

        let mut result = String::with_capacity(fmt.len() + 32);
        let mut chars = fmt.char_indices().peekable();
        let mut arg_idx = 0;

        while let Some((_ch_index, ch)) = chars.next() {
            if ch != '%' {
                result.push(ch);
                continue;
            }
            match chars.peek() {
                Some(&(_, '%')) => {
                    chars.next();
                    result.push('%');
                }
                Some(_) => {
                    // Check for %(name) dict-keyed format
                    let dict_key = if matches!(chars.peek(), Some(&(_, '('))) {
                        chars.next();
                        let mut key = String::new();
                        while let Some(&(_, c)) = chars.peek() {
                            if c == ')' {
                                chars.next();
                                break;
                            }
                            key.push(c);
                            chars.next();
                        }
                        Some(key)
                    } else {
                        None
                    };

                    let mut flags = String::new();
                    while let Some(&(_, c)) = chars.peek() {
                        if c == '-' || c == '+' || c == '0' || c == ' ' || c == '#' {
                            flags.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    let mut width = 0usize;
                    if let Some(&(_, '*')) = chars.peek() {
                        chars.next();
                        if arg_idx < arg_list.len() {
                            let value =
                                arg_list[arg_idx].to_index()?.to_i64().ok_or_else(|| {
                                    PyException::overflow_error("format width too large")
                                })?;
                            width = percent_star_to_width(value, &mut flags)?;
                            arg_idx += 1;
                            consumed_arg = true;
                        } else {
                            return Err(PyException::type_error(
                                "not enough arguments for format string",
                            ));
                        }
                    } else {
                        while let Some(&(_, c)) = chars.peek() {
                            if c.is_ascii_digit() {
                                width = checked_format_accumulate(width, c)?;
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                    let mut precision: Option<usize> = None;
                    if let Some(&(_, '.')) = chars.peek() {
                        chars.next();
                        if let Some(&(_, '*')) = chars.peek() {
                            chars.next();
                            if arg_idx < arg_list.len() {
                                let value =
                                    arg_list[arg_idx].to_index()?.to_i64().ok_or_else(|| {
                                        PyException::overflow_error("format precision too large")
                                    })?;
                                precision = percent_star_to_precision(value)?;
                                arg_idx += 1;
                                consumed_arg = true;
                            } else {
                                return Err(PyException::type_error(
                                    "not enough arguments for format string",
                                ));
                            }
                        } else {
                            let mut precision_digits = String::new();
                            while let Some(&(_, c)) = chars.peek() {
                                if c.is_ascii_digit() {
                                    precision_digits.push(c);
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                            if !precision_digits.is_empty() {
                                precision = Some(parse_format_precision(&precision_digits)?);
                            } else {
                                precision = Some(0);
                            }
                        }
                    }
                    let (spec_index, spec) = match chars.next() {
                        Some((idx, spec)) => (idx, spec),
                        None => return Err(PyException::value_error("incomplete format")),
                    };

                    let arg = if let Some(ref key) = dict_key {
                        used_mapping_key = true;
                        let key_obj = PyObject::str_val(CompactString::from(key.as_str()));
                        args.get_item(&key_obj)?
                    } else {
                        if arg_idx >= arg_list.len() {
                            return Err(PyException::type_error(
                                "not enough arguments for format string",
                            ));
                        }
                        let a = arg_list[arg_idx].clone();
                        arg_idx += 1;
                        consumed_arg = true;
                        a
                    };

                    let formatted = match spec {
                        's' => apply_string_percent_precision(self.vm_str(&arg)?, precision)?,
                        'r' => apply_string_percent_precision(self.vm_repr(&arg)?, precision)?,
                        'a' => apply_string_percent_precision(py_ascii_repr(&arg), precision)?,
                        'd' | 'i' => {
                            let raw = percent_int_string_arg(&arg, spec)?;
                            let raw = apply_percent_precision(&raw, precision)?;
                            apply_percent_sign(raw, &flags)
                        }
                        'f' | 'F' => {
                            if matches!(&arg.payload, PyObjectPayload::Str(_)) {
                                return Err(PyException::type_error(format!(
                                    "must be real number, not {}",
                                    arg.type_name()
                                )));
                            }
                            let v = arg.to_float().map_err(|_| {
                                PyException::type_error(format!(
                                    "must be real number, not {}",
                                    arg.type_name()
                                ))
                            })?;
                            let p = precision.unwrap_or(6);
                            format_percent_fixed_float(v, p, spec)
                        }
                        'e' | 'E' => {
                            if matches!(&arg.payload, PyObjectPayload::Str(_)) {
                                return Err(PyException::type_error(format!(
                                    "must be real number, not {}",
                                    arg.type_name()
                                )));
                            }
                            let v = arg.to_float().map_err(|_| {
                                PyException::type_error(format!(
                                    "must be real number, not {}",
                                    arg.type_name()
                                ))
                            })?;
                            let p = precision.unwrap_or(6);
                            let raw = if spec == 'e' {
                                format!("{:.prec$e}", v, prec = p)
                            } else {
                                format!("{:.prec$E}", v, prec = p)
                            };
                            normalize_float_percent_case(normalize_sci_exp(&raw, spec), spec)
                        }
                        'g' | 'G' => {
                            if matches!(&arg.payload, PyObjectPayload::Str(_)) {
                                return Err(PyException::type_error(format!(
                                    "must be real number, not {}",
                                    arg.type_name()
                                )));
                            }
                            let v = arg.to_float().map_err(|_| {
                                PyException::type_error(format!(
                                    "must be real number, not {}",
                                    arg.type_name()
                                ))
                            })?;
                            let alternate = flags.contains('#');
                            format_percent_general_float(v, precision, spec, alternate)?
                        }
                        'x' | 'X' | 'o' => {
                            let raw = format_percent_radix_arg(&arg, spec, flags.contains('#'))?;
                            let raw = apply_percent_radix_precision(&raw, precision)?;
                            apply_percent_sign(raw, &flags)
                        }
                        'c' => {
                            if let Some(n) = arg.as_int() {
                                if !(0..=0x10ffff).contains(&n) {
                                    return Err(PyException::overflow_error(
                                        "%c arg not in range(0x110000)",
                                    ));
                                }
                                char::from_u32(n as u32).map(|c| c.to_string()).ok_or_else(
                                    || PyException::overflow_error("%c arg not in range(0x110000)"),
                                )?
                            } else if let PyObjectPayload::Str(s) = &arg.payload {
                                let mut chars = s.chars();
                                let Some(ch) = chars.next() else {
                                    return Err(PyException::type_error("%c requires int or char"));
                                };
                                if chars.next().is_some() {
                                    return Err(PyException::type_error("%c requires int or char"));
                                }
                                ch.to_string()
                            } else {
                                return Err(PyException::type_error("%c requires int or char"));
                            }
                        }
                        _ => {
                            return Err(PyException::value_error(format!(
                                "unsupported format character '{}' (0x{:x}) at index {}",
                                spec, spec as u32, spec_index
                            )));
                        }
                    };

                    let formatted = if matches!(spec, 'd' | 'i' | 'x' | 'X' | 'o') {
                        apply_percent_numeric_width(&formatted, width, &flags, precision)?
                    } else {
                        apply_percent_width(&formatted, width, &flags)?
                    };
                    result.push_str(&formatted);
                }
                None => {
                    return Err(PyException::value_error("incomplete format"));
                }
            }
        }
        if !consumed_arg && !used_mapping_key {
            if !percent_no_conversion_accepts_arg(args, false) {
                return Err(PyException::type_error(
                    "not all arguments converted during string formatting",
                ));
            }
        }
        if using_tuple_args && !used_mapping_key && arg_idx < arg_list.len() {
            return Err(PyException::type_error(
                "not all arguments converted during string formatting",
            ));
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }

    /// Bytes % formatting (PEP 461)
    pub(super) fn vm_bytes_percent_format(
        &mut self,
        fmt: &[u8],
        args: &PyObjectRef,
        mutable_result: bool,
    ) -> Result<PyObjectRef, PyException> {
        let arg_list: Vec<PyObjectRef> = match &args.payload {
            PyObjectPayload::Tuple(items) => (**items).clone(),
            _ => vec![args.clone()],
        };
        let using_tuple_args = matches!(&args.payload, PyObjectPayload::Tuple(_));
        let mut consumed_arg = false;

        let mut result = Vec::with_capacity(fmt.len() + 32);
        let mut i = 0;
        let mut arg_idx = 0;

        while i < fmt.len() {
            if fmt[i] != b'%' {
                result.push(fmt[i]);
                i += 1;
                continue;
            }
            i += 1;
            if i >= fmt.len() {
                return Err(PyException::value_error("incomplete format"));
            }
            if fmt[i] == b'%' {
                result.push(b'%');
                i += 1;
                continue;
            }

            // Parse flags
            let mut zero_pad = false;
            let mut left_align = false;
            let mut alternate = false;
            let mut sign_plus = false;
            let mut sign_space = false;
            while i < fmt.len() && matches!(fmt[i], b'-' | b'+' | b'0' | b' ' | b'#') {
                if fmt[i] == b'0' {
                    zero_pad = true;
                }
                if fmt[i] == b'-' {
                    left_align = true;
                }
                if fmt[i] == b'#' {
                    alternate = true;
                }
                if fmt[i] == b'+' {
                    sign_plus = true;
                }
                if fmt[i] == b' ' {
                    sign_space = true;
                }
                i += 1;
            }
            // Parse width
            let mut width = 0usize;
            if i < fmt.len() && fmt[i] == b'*' {
                if arg_idx >= arg_list.len() {
                    return Err(PyException::type_error(
                        "not enough arguments for format string",
                    ));
                }
                let value = arg_list[arg_idx]
                    .to_index()?
                    .to_i64()
                    .ok_or_else(|| PyException::overflow_error("format width too large"))?;
                let mut width_flags = String::new();
                width = percent_star_to_width(value, &mut width_flags)?;
                if width_flags.contains('-') {
                    left_align = true;
                }
                i += 1;
                arg_idx += 1;
            } else {
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    width = checked_format_accumulate(width, fmt[i] as char)?;
                    i += 1;
                }
            }
            // Parse precision
            let mut _precision: Option<usize> = None;
            if i < fmt.len() && fmt[i] == b'.' {
                i += 1;
                if i < fmt.len() && fmt[i] == b'*' {
                    if arg_idx >= arg_list.len() {
                        return Err(PyException::type_error(
                            "not enough arguments for format string",
                        ));
                    }
                    let value = arg_list[arg_idx]
                        .to_index()?
                        .to_i64()
                        .ok_or_else(|| PyException::overflow_error("precision too big"))?;
                    _precision = percent_star_to_precision(value)?;
                    i += 1;
                    arg_idx += 1;
                } else {
                    let precision_start = i;
                    while i < fmt.len() && fmt[i].is_ascii_digit() {
                        i += 1;
                    }
                    if precision_start < i {
                        let digits = std::str::from_utf8(&fmt[precision_start..i])
                            .map_err(|_| PyException::value_error("invalid format"))?;
                        _precision = Some(parse_format_precision(digits)?);
                    } else {
                        _precision = Some(0);
                    }
                }
            }

            if i >= fmt.len() {
                return Err(PyException::value_error("incomplete format"));
            }
            let spec = fmt[i];
            i += 1;

            if arg_idx >= arg_list.len() {
                return Err(PyException::type_error(
                    "not enough arguments for format string",
                ));
            }
            let arg = &arg_list[arg_idx];
            arg_idx += 1;
            consumed_arg = true;

            let formatted: Vec<u8> = match spec {
                b's' | b'b' => {
                    apply_bytes_percent_precision(bytes_format_arg(self, arg)?, _precision)?
                }
                b'r' | b'a' => {
                    apply_bytes_percent_precision(py_ascii_repr(arg).into_bytes(), _precision)?
                }
                b'd' | b'i' | b'u' => {
                    let raw = percent_int_string_arg(arg, spec as char)?;
                    let raw = apply_percent_precision(&raw, _precision)?;
                    let mut flags = String::new();
                    if sign_plus {
                        flags.push('+');
                    } else if sign_space {
                        flags.push(' ');
                    }
                    apply_percent_sign(raw, &flags).into_bytes()
                }
                b'f' | b'F' | b'e' | b'E' | b'g' | b'G' => {
                    if matches!(
                        &arg.payload,
                        PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
                    ) {
                        return Err(PyException::type_error(format!(
                            "float argument required, not {}",
                            arg.type_name()
                        )));
                    }
                    let v = arg.to_float().map_err(|_| {
                        PyException::type_error(format!(
                            "float argument required, not {}",
                            arg.type_name()
                        ))
                    })?;
                    let p = _precision.unwrap_or(6);
                    match spec {
                        b'f' | b'F' => format_percent_fixed_float(v, p, spec as char).into_bytes(),
                        b'e' | b'E' => {
                            let raw = if spec == b'e' {
                                format!("{:.prec$e}", v, prec = p)
                            } else {
                                format!("{:.prec$E}", v, prec = p)
                            };
                            normalize_float_percent_case(
                                normalize_sci_exp(&raw, spec as char),
                                spec as char,
                            )
                            .into_bytes()
                        }
                        b'g' | b'G' => {
                            let alternate = alternate;
                            format_percent_general_float(v, _precision, spec as char, alternate)?
                                .into_bytes()
                        }
                        _ => unreachable!(),
                    }
                }
                b'x' | b'X' | b'o' => {
                    let spec_char = spec as char;
                    let raw = format_percent_radix_arg(arg, spec_char, alternate)?;
                    let mut flags = String::new();
                    if sign_plus {
                        flags.push('+');
                    } else if sign_space {
                        flags.push(' ');
                    }
                    let raw = apply_percent_sign(raw, &flags);
                    apply_percent_radix_precision(&raw, _precision)?.into_bytes()
                }
                b'c' => {
                    if let Some(byte) = one_byte_arg(arg) {
                        vec![byte]
                    } else if matches!(&arg.payload, PyObjectPayload::Int(PyInt::Big(_))) {
                        return Err(PyException::overflow_error("%c arg not in range(256)"));
                    } else {
                        let v = arg.as_int().ok_or_else(|| {
                            PyException::type_error(
                                "%c requires an integer in range(256) or a single byte",
                            )
                        })?;
                        if !(0..=255).contains(&v) {
                            return Err(PyException::overflow_error("%c arg not in range(256)"));
                        }
                        vec![v as u8]
                    }
                }
                _ => {
                    return Err(PyException::value_error(format!(
                        "unsupported format character '{}' (0x{:x}) at index {}",
                        spec as char,
                        spec,
                        i.saturating_sub(1)
                    )));
                }
            };

            if matches!(spec, b'd' | b'i' | b'u' | b'x' | b'X' | b'o') {
                let mut flags = String::new();
                if left_align {
                    flags.push('-');
                }
                if zero_pad {
                    flags.push('0');
                }
                let text = String::from_utf8(formatted)
                    .map_err(|_| PyException::value_error("invalid bytes format"))?;
                let padded = apply_percent_numeric_width(&text, width, &flags, _precision)?;
                result.extend_from_slice(padded.as_bytes());
                continue;
            }

            // Apply width/padding
            if width > 0 && formatted.len() < width {
                let pad_len = width - formatted.len();
                checked_repeat_len(1, pad_len, "format width")?;
                let pad_byte = if zero_pad && !left_align { b'0' } else { b' ' };
                if left_align {
                    result.extend_from_slice(&formatted);
                    result.extend(std::iter::repeat(b' ').take(pad_len));
                } else {
                    result.extend(std::iter::repeat(pad_byte).take(pad_len));
                    result.extend_from_slice(&formatted);
                }
            } else {
                result.extend_from_slice(&formatted);
            }
        }

        if !consumed_arg {
            if !percent_no_conversion_accepts_arg(args, true) {
                return Err(PyException::type_error(
                    "not all arguments converted during bytes formatting",
                ));
            }
        }
        if using_tuple_args && arg_idx < arg_list.len() {
            return Err(PyException::type_error(
                "not all arguments converted during bytes formatting",
            ));
        }
        if mutable_result {
            Ok(PyObject::bytearray(result))
        } else {
            Ok(PyObject::bytes(result))
        }
    }
}

/// Normalize Rust scientific notation to CPython format.
/// Rust: "1.23e3" -> Python: "1.23e+03"
fn normalize_sci_exp(raw: &str, e_char: char) -> String {
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
