use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
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

        let mut result = String::with_capacity(fmt.len() + 32);
        let mut chars = fmt.chars().peekable();
        let mut arg_idx = 0;

        while let Some(ch) = chars.next() {
            if ch != '%' {
                result.push(ch);
                continue;
            }
            match chars.peek() {
                Some(&'%') => {
                    chars.next();
                    result.push('%');
                }
                Some(_) => {
                    // Check for %(name) dict-keyed format
                    let dict_key = if chars.peek() == Some(&'(') {
                        chars.next();
                        let mut key = String::new();
                        while let Some(&c) = chars.peek() {
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
                    while let Some(&c) = chars.peek() {
                        if c == '-' || c == '+' || c == '0' || c == ' ' || c == '#' {
                            flags.push(c);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    let mut width = 0usize;
                    if let Some(&'*') = chars.peek() {
                        chars.next();
                        if arg_idx < arg_list.len() {
                            width = arg_list[arg_idx].as_int().unwrap_or(0) as usize;
                            arg_idx += 1;
                        }
                    } else {
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() {
                                width = width * 10 + (c as usize - '0' as usize);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                    let mut precision: Option<usize> = None;
                    if let Some(&'.') = chars.peek() {
                        chars.next();
                        let mut p = 0usize;
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() {
                                p = p * 10 + (c as usize - '0' as usize);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        precision = Some(p);
                    }
                    let spec = chars.next().unwrap_or('s');

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
                        a
                    };

                    let formatted = match spec {
                        's' => self.vm_str(&arg)?,
                        'r' => self.vm_repr(&arg)?,
                        'd' | 'i' => match &arg.payload {
                            PyObjectPayload::Int(n) => n.to_string(),
                            PyObjectPayload::Bool(b) => i64::from(*b).to_string(),
                            _ => {
                                let n = arg.as_int().ok_or_else(|| {
                                    PyException::type_error(&format!(
                                        "%{} format: a number is required, not {}",
                                        spec,
                                        arg.type_name()
                                    ))
                                })?;
                                format!("{}", n)
                            }
                        },
                        'f' | 'F' => {
                            let v = arg.to_float().unwrap_or(0.0);
                            let p = precision.unwrap_or(6);
                            format!("{:.prec$}", v, prec = p)
                        }
                        'e' | 'E' => {
                            let v = arg.to_float().unwrap_or(0.0);
                            let p = precision.unwrap_or(6);
                            let raw = if spec == 'e' {
                                format!("{:.prec$e}", v, prec = p)
                            } else {
                                format!("{:.prec$E}", v, prec = p)
                            };
                            normalize_sci_exp(&raw, spec)
                        }
                        'g' | 'G' => {
                            let v = arg.to_float().unwrap_or(0.0);
                            let p = precision.unwrap_or(6);
                            let abs_v = v.abs();
                            let use_sci =
                                abs_v != 0.0 && (abs_v >= 10f64.powi(p as i32) || abs_v < 1e-4);
                            if use_sci {
                                let sp = if p > 0 { p - 1 } else { 0 };
                                let ec = if spec == 'g' { 'e' } else { 'E' };
                                let raw = if ec == 'e' {
                                    format!("{:.prec$e}", v, prec = sp)
                                } else {
                                    format!("{:.prec$E}", v, prec = sp)
                                };
                                normalize_sci_exp(&raw, ec)
                            } else {
                                let s = format!("{:.prec$}", v, prec = p);
                                if s.contains('.') {
                                    s.trim_end_matches('0').trim_end_matches('.').to_string()
                                } else {
                                    s
                                }
                            }
                        }
                        'x' => format_percent_radix_arg(&arg, spec, flags.contains('#'))?,
                        'X' => format_percent_radix_arg(&arg, spec, flags.contains('#'))?,
                        'o' => format_percent_radix_arg(&arg, spec, flags.contains('#'))?,
                        'c' => {
                            if let Some(n) = arg.as_int() {
                                char::from_u32(n as u32)
                                    .map(|c| c.to_string())
                                    .unwrap_or_default()
                            } else {
                                arg.py_to_string()
                                    .chars()
                                    .next()
                                    .map(|c| c.to_string())
                                    .unwrap_or_default()
                            }
                        }
                        _ => format!("%{}", spec),
                    };

                    if width > 0 && formatted.len() < width {
                        if flags.contains('-') {
                            result.push_str(&formatted);
                            for _ in 0..(width - formatted.len()) {
                                result.push(' ');
                            }
                        } else {
                            let pad = if flags.contains('0') { '0' } else { ' ' };
                            for _ in 0..(width - formatted.len()) {
                                result.push(pad);
                            }
                            result.push_str(&formatted);
                        }
                    } else {
                        result.push_str(&formatted);
                    }
                }
                None => {
                    result.push('%');
                }
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
    ) -> Result<PyObjectRef, PyException> {
        let arg_list: Vec<PyObjectRef> = match &args.payload {
            PyObjectPayload::Tuple(items) => (**items).clone(),
            _ => vec![args.clone()],
        };

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
                break;
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
                i += 1;
            }
            // Parse width
            let mut width = 0usize;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                width = width * 10 + (fmt[i] - b'0') as usize;
                i += 1;
            }
            // Parse precision
            let mut _precision: Option<usize> = None;
            if i < fmt.len() && fmt[i] == b'.' {
                i += 1;
                let mut p = 0usize;
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    p = p * 10 + (fmt[i] - b'0') as usize;
                    i += 1;
                }
                _precision = Some(p);
            }

            if i >= fmt.len() {
                break;
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

            let formatted: Vec<u8> = match spec {
                b's' | b'b' => match &arg.payload {
                    PyObjectPayload::Bytes(b) => (**b).clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    _ => {
                        let s = self.vm_str(arg)?;
                        s.into_bytes()
                    }
                },
                b'r' | b'a' => {
                    let s = self.vm_repr(arg)?;
                    s.into_bytes()
                }
                b'd' | b'i' | b'u' => {
                    let v = arg.as_int().unwrap_or(0);
                    format!("{}", v).into_bytes()
                }
                b'x' => format_percent_radix_arg(arg, 'x', alternate)?.into_bytes(),
                b'X' => format_percent_radix_arg(arg, 'X', alternate)?.into_bytes(),
                b'o' => format_percent_radix_arg(arg, 'o', alternate)?.into_bytes(),
                b'c' => {
                    let v = arg.as_int().unwrap_or(0) as u8;
                    vec![v]
                }
                _ => {
                    let mut fallback = vec![b'%'];
                    fallback.push(spec);
                    fallback
                }
            };

            // Apply width/padding
            if width > 0 && formatted.len() < width {
                let pad_len = width - formatted.len();
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

        Ok(PyObject::bytes(result))
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
