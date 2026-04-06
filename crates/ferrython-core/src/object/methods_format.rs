//! String formatting and dir methods.

use crate::error::PyResult;
use compact_str::CompactString;

use super::payload::*;
use super::helpers::*;
use super::methods::PyObjectMethods;

pub(super) fn py_format_value(obj: &PyObjectRef, spec: &str) -> PyResult<String> {
        if spec.is_empty() {
            return Ok(obj.py_to_string());
        }
        // Parse format spec: [[fill]align][sign][#][0][width][grouping_option][.precision][type]
        let spec_bytes = spec.as_bytes();
        let len = spec_bytes.len();

        // Handle comma grouping: {:,} or {:,d} — only for simple specs without type specifier
        // Type-specific handlers (d, f, etc.) handle commas themselves
        let last_char = spec.as_bytes().last().copied().unwrap_or(0) as char;
        let has_type_char = matches!(last_char, 'd' | 'f' | 'F' | 'e' | 'E' | 'g' | 'G' | 'n' | 'b' | 'o' | 'x' | 'X');
        if spec.contains(',') && !has_type_char {
            let without_comma = spec.replace(',', "");
            let base_str = if without_comma.is_empty() {
                // Just {:,} — format as integer with commas
                let n = obj.to_int()?;
                n.to_string()
            } else {
                obj.format_value(&without_comma)?
            };
            // Apply comma grouping to the numeric part
            return Ok(add_thousands_separator(&base_str, ','));
        }
        // Handle underscore grouping: {:_} — only for simple specs without type specifier
        if spec.contains('_') && !spec.contains("__") && !has_type_char {
            let without_underscore = spec.replace('_', "");
            let base_str = if without_underscore.is_empty() {
                let n = obj.to_int()?;
                n.to_string()
            } else {
                obj.format_value(&without_underscore)?
            };
            return Ok(add_thousands_separator(&base_str, '_'));
        }

        // Simple parsing for common cases
        let type_char = spec_bytes[len - 1] as char;
        match type_char {
            'd' => {
                let n = obj.to_int()?;
                let inner_spec = &spec[..len - 1];
                let use_comma = inner_spec.contains(',');
                let use_underscore = inner_spec.contains('_');
                let clean_spec: String = inner_spec.chars().filter(|c| *c != ',' && *c != '_').collect();
                let num_str = n.to_string();
                let result = if use_comma {
                    add_thousands_separator(&num_str, ',')
                } else if use_underscore {
                    add_thousands_separator(&num_str, '_')
                } else {
                    num_str
                };
                if clean_spec.is_empty() {
                    return Ok(result);
                }
                return Ok(apply_numeric_sign(&result, &clean_spec));
            }
            'f' | 'F' => {
                let f = obj.to_float()?;
                let inner_spec = &spec[..len - 1];
                let use_comma = inner_spec.contains(',');
                let clean_spec: String = inner_spec.chars().filter(|c| *c != ',').collect();
                if let Some(dot_pos) = clean_spec.rfind('.') {
                    let prec: usize = clean_spec[dot_pos + 1..].parse().unwrap_or(6);
                    let num_str = format!("{:.prec$}", f, prec = prec);
                    let result = if use_comma {
                        add_thousands_separator(&num_str, ',')
                    } else {
                        num_str
                    };
                    let pre_dot = &clean_spec[..dot_pos];
                    if pre_dot.is_empty() {
                        return Ok(result);
                    }
                    return Ok(apply_numeric_sign(&result, pre_dot));
                }
                let num_str = format!("{:.6}", f);
                if !inner_spec.is_empty() {
                    let clean: String = inner_spec.chars().filter(|c| *c != ',').collect();
                    return Ok(apply_numeric_sign(&num_str, &clean));
                }
                if use_comma {
                    return Ok(add_thousands_separator(&num_str, ','));
                }
                return Ok(num_str);
            }
            'e' | 'E' => {
                let f = obj.to_float()?;
                let inner_spec = &spec[..len - 1];
                let prec = if let Some(dot_pos) = inner_spec.rfind('.') {
                    inner_spec[dot_pos + 1..].parse().unwrap_or(6)
                } else { 6 };
                // Python scientific notation: always show sign, zero-pad exponent to 2+ digits
                let raw = if type_char == 'e' {
                    format!("{:.prec$e}", f, prec = prec)
                } else {
                    format!("{:.prec$E}", f, prec = prec)
                };
                // Rust gives e.g. "1.23e3", Python wants "1.23e+03"
                let e_char = if type_char == 'e' { 'e' } else { 'E' };
                let result = if let Some(e_pos) = raw.rfind(e_char) {
                    let mantissa = &raw[..e_pos];
                    let exp_str = &raw[e_pos + 1..];
                    let exp_val: i64 = exp_str.parse().unwrap_or(0);
                    let exp_formatted = if exp_val >= 0 {
                        format!("{}{:+03}", e_char, exp_val)
                    } else {
                        format!("{}{:03}", e_char, exp_val)
                    };
                    format!("{}{}", mantissa, exp_formatted)
                } else {
                    raw
                };
                return Ok(result);
            }
            '%' => {
                let f = obj.to_float()?;
                let inner_spec = &spec[..len - 1];
                let prec = if let Some(dot_pos) = inner_spec.rfind('.') {
                    inner_spec[dot_pos + 1..].parse().unwrap_or(6)
                } else { 6 };
                let pct = f * 100.0;
                return Ok(format!("{:.prec$}%", pct, prec = prec));
            }
            'b' => {
                let n = obj.to_int()?;
                let digits = format!("{:b}", n);
                let inner_spec = &spec[..len - 1];
                let alt = inner_spec.contains('#');
                let clean_spec: String = inner_spec.chars().filter(|c| *c != '#').collect();
                if alt { return Ok(apply_prefixed_format(&digits, "0b", &clean_spec)); }
                if clean_spec.is_empty() { return Ok(digits); }
                return Ok(apply_numeric_sign(&digits, &clean_spec));
            }
            'o' => {
                let n = obj.to_int()?;
                let digits = format!("{:o}", n);
                let inner_spec = &spec[..len - 1];
                let alt = inner_spec.contains('#');
                let clean_spec: String = inner_spec.chars().filter(|c| *c != '#').collect();
                if alt { return Ok(apply_prefixed_format(&digits, "0o", &clean_spec)); }
                if clean_spec.is_empty() { return Ok(digits); }
                return Ok(apply_numeric_sign(&digits, &clean_spec));
            }
            'x' => {
                let n = obj.to_int()?;
                let digits = format!("{:x}", n);
                let inner_spec = &spec[..len - 1];
                let alt = inner_spec.contains('#');
                let clean_spec: String = inner_spec.chars().filter(|c| *c != '#').collect();
                if alt { return Ok(apply_prefixed_format(&digits, "0x", &clean_spec)); }
                if clean_spec.is_empty() { return Ok(digits); }
                return Ok(apply_numeric_sign(&digits, &clean_spec));
            }
            'X' => {
                let n = obj.to_int()?;
                let digits = format!("{:X}", n);
                let inner_spec = &spec[..len - 1];
                let alt = inner_spec.contains('#');
                let clean_spec: String = inner_spec.chars().filter(|c| *c != '#').collect();
                if alt { return Ok(apply_prefixed_format(&digits, "0X", &clean_spec)); }
                if clean_spec.is_empty() { return Ok(digits); }
                return Ok(apply_numeric_sign(&digits, &clean_spec));
            }
            's' => {
                let s = obj.py_to_string();
                let inner_spec = &spec[..len - 1];
                if inner_spec.is_empty() { return Ok(s); }
                return Ok(apply_string_format_spec(&s, inner_spec));
            }
            'g' | 'G' => {
                let f = obj.to_float()?;
                let inner_spec = &spec[..len - 1];
                let prec = if let Some(dot_pos) = inner_spec.rfind('.') {
                    inner_spec[dot_pos + 1..].parse().unwrap_or(6)
                } else { 6usize };
                // 'g' format: use fixed notation or scientific, whichever is shorter
                let abs_f = f.abs();
                let use_exp = if abs_f == 0.0 { false }
                    else { abs_f >= 10f64.powi(prec as i32) || abs_f < 1e-4 };
                let result = if use_exp {
                    // scientific: use precision-1 for mantissa digits
                    let raw = format!("{:.prec$e}", f, prec = if prec > 0 { prec - 1 } else { 0 });
                    let e_char = if type_char == 'g' { 'e' } else { 'E' };
                    if let Some(e_pos) = raw.rfind('e') {
                        let mantissa = &raw[..e_pos];
                        let exp_str = &raw[e_pos + 1..];
                        let exp_val: i64 = exp_str.parse().unwrap_or(0);
                        // Trim trailing zeros from mantissa
                        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
                        let exp_formatted = if exp_val >= 0 {
                            format!("{}{:+03}", e_char, exp_val)
                        } else {
                            format!("{}{:03}", e_char, exp_val)
                        };
                        format!("{}{}", mantissa, exp_formatted)
                    } else { raw }
                } else {
                    // fixed: show `prec` significant digits total
                    let formatted = if abs_f == 0.0 {
                        "0".to_string()
                    } else {
                        let digits = prec as i32;
                        let mag = abs_f.log10().floor() as i32 + 1;
                        let decimal_places = if digits > mag { (digits - mag) as usize } else { 0 };
                        let s = format!("{:.prec$}", f, prec = decimal_places);
                        // Trim trailing zeros
                        if s.contains('.') {
                            let trimmed = s.trim_end_matches('0').trim_end_matches('.');
                            trimmed.to_string()
                        } else { s }
                    };
                    formatted
                };
                return Ok(result);
            }
            _ => {
                // No type char — handle numeric sign, then alignment
                let is_numeric = obj.as_int().is_some() || obj.to_float().is_ok();
                let s = obj.py_to_string();
                if is_numeric {
                    let formatted = apply_numeric_sign(&s, spec);
                    return Ok(formatted);
                }
                return Ok(apply_string_format_spec(&s, spec));
            }
        }
}

pub(super) fn py_dir(obj: &PyObjectRef) -> Vec<CompactString> {
        // Common dunders shared by all types
        let common_dunders = &[
            "__class__", "__delattr__", "__dir__", "__doc__", "__eq__", "__format__",
            "__ge__", "__getattribute__", "__gt__", "__hash__", "__init__",
            "__init_subclass__", "__le__", "__lt__", "__ne__", "__new__",
            "__reduce__", "__reduce_ex__", "__repr__", "__setattr__",
            "__sizeof__", "__str__", "__subclasshook__",
        ];
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                let mut names: Vec<CompactString> = inst.attrs.read().keys().cloned().collect();
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    names.extend(cd.namespace.read().keys().cloned());
                }
                for d in common_dunders { names.push(CompactString::from(*d)); }
                names.sort(); names.dedup(); names
            }
            PyObjectPayload::Class(cd) => {
                let mut n: Vec<_> = cd.namespace.read().keys().cloned().collect();
                for d in common_dunders { n.push(CompactString::from(*d)); }
                n.sort(); n.dedup(); n
            }
            PyObjectPayload::Module(m) => { let mut n: Vec<_> = m.attrs.read().keys().cloned().collect(); n.sort(); n }
            PyObjectPayload::List(_) => {
                let mut v: Vec<&str> = vec![
                    "append", "clear", "copy", "count", "extend",
                    "index", "insert", "pop", "remove", "reverse", "sort",
                    "__add__", "__contains__", "__getitem__", "__iadd__", "__imul__",
                    "__iter__", "__len__", "__mul__", "__reversed__", "__rmul__",
                    "__setitem__", "__delitem__", "__bool__",
                ];
                v.extend_from_slice(common_dunders);
                v.sort(); v.dedup();
                v.into_iter().map(CompactString::from).collect()
            }
            PyObjectPayload::Dict(_) => {
                let mut v: Vec<&str> = vec![
                    "clear", "copy", "fromkeys", "get", "items", "keys",
                    "pop", "popitem", "setdefault", "update", "values",
                    "__contains__", "__getitem__", "__setitem__", "__delitem__",
                    "__iter__", "__len__", "__bool__",
                ];
                v.extend_from_slice(common_dunders);
                v.sort(); v.dedup();
                v.into_iter().map(CompactString::from).collect()
            }
            PyObjectPayload::Str(_) => {
                let mut v: Vec<&str> = vec![
                    "capitalize", "casefold", "center", "count", "encode",
                    "endswith", "expandtabs", "find", "format", "format_map", "index",
                    "isalnum", "isalpha", "isascii", "isdecimal", "isdigit", "isidentifier",
                    "islower", "isnumeric", "isprintable", "isspace", "istitle", "isupper",
                    "join", "ljust", "lower", "lstrip", "maketrans", "partition",
                    "removeprefix", "removesuffix", "replace",
                    "rfind", "rindex", "rjust", "rpartition", "rsplit", "rstrip", "split",
                    "splitlines", "startswith", "strip", "swapcase", "title", "translate",
                    "upper", "zfill",
                    "__add__", "__contains__", "__getitem__", "__iter__", "__len__",
                    "__mod__", "__mul__", "__rmul__", "__bool__",
                ];
                v.extend_from_slice(common_dunders);
                v.sort(); v.dedup();
                v.into_iter().map(CompactString::from).collect()
            }
            PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => {
                let mut v: Vec<&str> = vec![
                    "bit_length", "conjugate", "denominator", "imag",
                    "numerator", "real", "to_bytes", "from_bytes",
                    "__abs__", "__add__", "__and__", "__bool__", "__ceil__",
                    "__divmod__", "__float__", "__floor__", "__floordiv__",
                    "__index__", "__int__", "__invert__", "__lshift__", "__mod__",
                    "__mul__", "__neg__", "__or__", "__pos__", "__pow__",
                    "__radd__", "__rand__", "__rfloordiv__", "__rlshift__",
                    "__rmod__", "__rmul__", "__ror__", "__rpow__", "__rrshift__",
                    "__rshift__", "__rsub__", "__rtruediv__", "__rxor__",
                    "__sub__", "__truediv__", "__trunc__", "__xor__",
                ];
                v.extend_from_slice(common_dunders);
                v.sort(); v.dedup();
                v.into_iter().map(CompactString::from).collect()
            }
            PyObjectPayload::Float(_) => {
                let mut v: Vec<&str> = vec![
                    "as_integer_ratio", "conjugate", "hex", "imag",
                    "is_integer", "real",
                    "__abs__", "__add__", "__bool__", "__divmod__",
                    "__float__", "__floordiv__", "__int__", "__mod__",
                    "__mul__", "__neg__", "__pos__", "__pow__",
                    "__radd__", "__rfloordiv__", "__rmod__", "__rmul__",
                    "__rpow__", "__rsub__", "__rtruediv__",
                    "__sub__", "__truediv__", "__trunc__",
                ];
                v.extend_from_slice(common_dunders);
                v.sort(); v.dedup();
                v.into_iter().map(CompactString::from).collect()
            }
            PyObjectPayload::Tuple(_) => {
                let mut v: Vec<&str> = vec![
                    "count", "index",
                    "__add__", "__contains__", "__getitem__", "__iter__",
                    "__len__", "__mul__", "__rmul__", "__bool__",
                ];
                v.extend_from_slice(common_dunders);
                v.sort(); v.dedup();
                v.into_iter().map(CompactString::from).collect()
            }
            PyObjectPayload::Set(_) => {
                let mut v: Vec<&str> = vec![
                    "add", "clear", "copy", "difference", "discard",
                    "intersection", "isdisjoint", "issubset", "issuperset", "pop",
                    "remove", "symmetric_difference", "union", "update",
                    "__and__", "__contains__", "__iand__", "__ior__", "__isub__",
                    "__iter__", "__ixor__", "__len__", "__or__", "__sub__",
                    "__xor__", "__bool__",
                ];
                v.extend_from_slice(common_dunders);
                v.sort(); v.dedup();
                v.into_iter().map(CompactString::from).collect()
            }
            PyObjectPayload::FrozenSet(_) => {
                let mut v: Vec<&str> = vec![
                    "copy", "difference", "intersection", "isdisjoint",
                    "issubset", "issuperset", "symmetric_difference", "union",
                    "__and__", "__contains__", "__iter__", "__len__",
                    "__or__", "__sub__", "__xor__", "__bool__",
                ];
                v.extend_from_slice(common_dunders);
                v.sort(); v.dedup();
                v.into_iter().map(CompactString::from).collect()
            }
            PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => {
                let mut v: Vec<&str> = vec![
                    "count", "decode", "endswith", "find", "hex",
                    "index", "join", "lower", "replace", "split", "startswith", "strip",
                    "upper", "fromhex",
                    "__add__", "__contains__", "__getitem__", "__iter__",
                    "__len__", "__mul__", "__bool__",
                ];
                v.extend_from_slice(common_dunders);
                v.sort(); v.dedup();
                v.into_iter().map(CompactString::from).collect()
            }
            _ => common_dunders.iter().map(|s| CompactString::from(*s)).collect(),
        }
}
