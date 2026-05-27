use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

pub(super) fn parse_json_value(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    skip_ws(s, pos);
    if *pos >= s.len() {
        return Err(PyException::json_decode_error("Unexpected end of JSON"));
    }
    let ch = s.as_bytes()[*pos] as char;
    match ch {
        '"' => parse_json_string(s, pos),
        't' | 'f' => parse_json_bool(s, pos),
        'n' => parse_json_null(s, pos),
        '[' => parse_json_array(s, pos),
        '{' => parse_json_object(s, pos),
        _ => parse_json_number(s, pos),
    }
}

fn skip_ws(s: &str, pos: &mut usize) {
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

fn parse_json_string(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip "
    let mut result = String::new();
    while *pos < s.len() {
        let ch = s.as_bytes()[*pos] as char;
        if ch == '"' {
            *pos += 1;
            return Ok(PyObject::str_val(CompactString::from(result)));
        }
        if ch == '\\' {
            *pos += 1;
            if *pos >= s.len() {
                break;
            }
            let esc = s.as_bytes()[*pos] as char;
            match esc {
                'n' => result.push('\n'),
                't' => result.push('\t'),
                'r' => result.push('\r'),
                'b' => result.push('\u{0008}'),
                'f' => result.push('\u{000C}'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                'u' => {
                    // Parse \uXXXX unicode escape and UTF-16 surrogate pairs.
                    if *pos + 4 >= s.len() {
                        return Err(PyException::json_decode_error("Incomplete \\uXXXX escape"));
                    }
                    let hex = &s[*pos + 1..*pos + 5];
                    let cp = u32::from_str_radix(hex, 16)
                        .map_err(|_| PyException::json_decode_error("Invalid \\uXXXX escape"))?;
                    *pos += 4;
                    if (0xD800..=0xDBFF).contains(&cp) {
                        if *pos + 6 < s.len()
                            && s.as_bytes()[*pos + 1] == b'\\'
                            && s.as_bytes()[*pos + 2] == b'u'
                        {
                            let lo_hex = &s[*pos + 3..*pos + 7];
                            if let Ok(lo) = u32::from_str_radix(lo_hex, 16) {
                                if (0xDC00..=0xDFFF).contains(&lo) {
                                    let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    if let Some(c) = char::from_u32(combined) {
                                        result.push(c);
                                    }
                                    *pos += 6;
                                } else {
                                    result.push(char::REPLACEMENT_CHARACTER);
                                }
                            } else {
                                result.push(char::REPLACEMENT_CHARACTER);
                            }
                        } else {
                            result.push(char::REPLACEMENT_CHARACTER);
                        }
                    } else if let Some(c) = char::from_u32(cp) {
                        result.push(c);
                    } else {
                        result.push(char::REPLACEMENT_CHARACTER);
                    }
                }
                _ => {
                    result.push('\\');
                    result.push(esc);
                }
            }
        } else {
            result.push(ch);
        }
        *pos += 1;
    }
    Err(PyException::json_decode_error("Unterminated string"))
}

fn parse_json_bool(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("true") {
        *pos += 4;
        return Ok(PyObject::bool_val(true));
    }
    if s[*pos..].starts_with("false") {
        *pos += 5;
        return Ok(PyObject::bool_val(false));
    }
    Err(PyException::json_decode_error("Invalid JSON"))
}

fn parse_json_null(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("null") {
        *pos += 4;
        return Ok(PyObject::none());
    }
    Err(PyException::json_decode_error("Invalid JSON"))
}

fn parse_json_number(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    let start = *pos;
    let mut is_float = false;
    if *pos < s.len() && s.as_bytes()[*pos] == b'-' {
        *pos += 1;
    }
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos < s.len() && s.as_bytes()[*pos] == b'.' {
        is_float = true;
        *pos += 1;
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() {
            *pos += 1;
        }
    }
    if *pos < s.len() && (s.as_bytes()[*pos] == b'e' || s.as_bytes()[*pos] == b'E') {
        is_float = true;
        *pos += 1;
        if *pos < s.len() && (s.as_bytes()[*pos] == b'+' || s.as_bytes()[*pos] == b'-') {
            *pos += 1;
        }
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() {
            *pos += 1;
        }
    }
    let num_str = &s[start..*pos];
    if is_float {
        let f: f64 = num_str
            .parse()
            .map_err(|_| PyException::json_decode_error("Invalid number"))?;
        Ok(PyObject::float(f))
    } else {
        let i: i64 = num_str
            .parse()
            .map_err(|_| PyException::json_decode_error("Invalid number"))?;
        Ok(PyObject::int(i))
    }
}

fn parse_json_array(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip [
    let mut items = Vec::new();
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b']' {
        *pos += 1;
        return Ok(PyObject::list(items));
    }
    loop {
        items.push(parse_json_value(s, pos)?);
        skip_ws(s, pos);
        if *pos >= s.len() {
            break;
        }
        if s.as_bytes()[*pos] == b']' {
            *pos += 1;
            return Ok(PyObject::list(items));
        }
        if s.as_bytes()[*pos] == b',' {
            *pos += 1;
        } else {
            break;
        }
    }
    Err(PyException::json_decode_error("Invalid JSON array"))
}

fn parse_json_object(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip {
    let pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
    let dict = PyObject::dict_from_pairs(pairs);
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b'}' {
        *pos += 1;
        return Ok(dict);
    }
    loop {
        skip_ws(s, pos);
        let key = parse_json_string(s, pos)?;
        skip_ws(s, pos);
        if *pos >= s.len() || s.as_bytes()[*pos] != b':' {
            return Err(PyException::json_decode_error("Expected ':'"));
        }
        *pos += 1;
        let value = parse_json_value(s, pos)?;
        let hk = HashableKey::str_key(CompactString::from(key.py_to_string()));
        match &dict.payload {
            PyObjectPayload::Dict(map) => {
                map.write().insert(hk, value);
            }
            _ => unreachable!(),
        }
        skip_ws(s, pos);
        if *pos >= s.len() {
            break;
        }
        if s.as_bytes()[*pos] == b'}' {
            *pos += 1;
            return Ok(dict);
        }
        if s.as_bytes()[*pos] == b',' {
            *pos += 1;
        } else {
            break;
        }
    }
    Err(PyException::json_decode_error("Invalid JSON object"))
}
