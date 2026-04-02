//! Serialization stdlib modules (json, csv, base64, struct)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args,
};
use ferrython_core::types::HashableKey;

pub fn create_json_module() -> PyObjectRef {
    make_module("json", vec![
        ("dumps", make_builtin(json_dumps)),
        ("loads", make_builtin(json_loads)),
    ])
}

fn json_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("json.dumps", args, 1)?;
    let s = py_to_json(&args[0])?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn py_to_json(obj: &PyObjectRef) -> PyResult<String> {
    match &obj.payload {
        PyObjectPayload::None => Ok("null".into()),
        PyObjectPayload::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        PyObjectPayload::Int(n) => Ok(n.to_string()),
        PyObjectPayload::Float(f) => {
            if f.is_nan() { return Err(PyException::value_error("NaN is not JSON serializable")); }
            if f.is_infinite() { return Err(PyException::value_error("Infinity is not JSON serializable")); }
            Ok(format!("{}", f))
        }
        PyObjectPayload::Str(s) => Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"))),
        PyObjectPayload::List(items) => {
            let r = items.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|i| py_to_json(i)).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        PyObjectPayload::Tuple(items) => {
            let parts: Result<Vec<String>, _> = items.iter().map(|i| py_to_json(i)).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|(k, v)| {
                let key_str = match k {
                    HashableKey::Str(s) => format!("\"{}\"", s),
                    HashableKey::Int(n) => format!("\"{}\"", n),
                    _ => return Err(PyException::type_error("keys must be str")),
                };
                let val_str = py_to_json(v)?;
                Ok(format!("{}: {}", key_str, val_str))
            }).collect();
            Ok(format!("{{{}}}", parts?.join(", ")))
        }
        PyObjectPayload::InstanceDict(attrs) => {
            let r = attrs.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|(k, v)| {
                let key_str = format!("\"{}\"", k);
                let val_str = py_to_json(v)?;
                Ok(format!("{}: {}", key_str, val_str))
            }).collect();
            Ok(format!("{{{}}}", parts?.join(", ")))
        }
        _ => Err(PyException::type_error(format!("Object of type {} is not JSON serializable", obj.type_name()))),
    }
}

fn json_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("json.loads", args, 1)?;
    let s = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("json.loads requires a string")),
    };
    parse_json_value(&s, &mut 0)
}

fn parse_json_value(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    skip_ws(s, pos);
    if *pos >= s.len() { return Err(PyException::value_error("Unexpected end of JSON")); }
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
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_whitespace() { *pos += 1; }
}

fn parse_json_string(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip "
    let mut result = String::new();
    while *pos < s.len() {
        let ch = s.as_bytes()[*pos] as char;
        if ch == '"' { *pos += 1; return Ok(PyObject::str_val(CompactString::from(result))); }
        if ch == '\\' {
            *pos += 1;
            if *pos >= s.len() { break; }
            let esc = s.as_bytes()[*pos] as char;
            match esc {
                'n' => result.push('\n'),
                't' => result.push('\t'),
                'r' => result.push('\r'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                _ => { result.push('\\'); result.push(esc); }
            }
        } else {
            result.push(ch);
        }
        *pos += 1;
    }
    Err(PyException::value_error("Unterminated string"))
}

fn parse_json_bool(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("true") { *pos += 4; return Ok(PyObject::bool_val(true)); }
    if s[*pos..].starts_with("false") { *pos += 5; return Ok(PyObject::bool_val(false)); }
    Err(PyException::value_error("Invalid JSON"))
}

fn parse_json_null(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("null") { *pos += 4; return Ok(PyObject::none()); }
    Err(PyException::value_error("Invalid JSON"))
}

fn parse_json_number(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    let start = *pos;
    let mut is_float = false;
    if *pos < s.len() && s.as_bytes()[*pos] == b'-' { *pos += 1; }
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    if *pos < s.len() && s.as_bytes()[*pos] == b'.' {
        is_float = true; *pos += 1;
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    }
    if *pos < s.len() && (s.as_bytes()[*pos] == b'e' || s.as_bytes()[*pos] == b'E') {
        is_float = true; *pos += 1;
        if *pos < s.len() && (s.as_bytes()[*pos] == b'+' || s.as_bytes()[*pos] == b'-') { *pos += 1; }
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    }
    let num_str = &s[start..*pos];
    if is_float {
        let f: f64 = num_str.parse().map_err(|_| PyException::value_error("Invalid number"))?;
        Ok(PyObject::float(f))
    } else {
        let i: i64 = num_str.parse().map_err(|_| PyException::value_error("Invalid number"))?;
        Ok(PyObject::int(i))
    }
}

fn parse_json_array(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip [
    let mut items = Vec::new();
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b']' { *pos += 1; return Ok(PyObject::list(items)); }
    loop {
        items.push(parse_json_value(s, pos)?);
        skip_ws(s, pos);
        if *pos >= s.len() { break; }
        if s.as_bytes()[*pos] == b']' { *pos += 1; return Ok(PyObject::list(items)); }
        if s.as_bytes()[*pos] == b',' { *pos += 1; } else { break; }
    }
    Err(PyException::value_error("Invalid JSON array"))
}

fn parse_json_object(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip {
    let pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
    let dict = PyObject::dict_from_pairs(pairs);
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b'}' { *pos += 1; return Ok(dict); }
    loop {
        skip_ws(s, pos);
        let key = parse_json_string(s, pos)?;
        skip_ws(s, pos);
        if *pos >= s.len() || s.as_bytes()[*pos] != b':' { return Err(PyException::value_error("Expected ':'")); }
        *pos += 1;
        let value = parse_json_value(s, pos)?;
        let hk = HashableKey::Str(CompactString::from(key.py_to_string()));
        match &dict.payload {
            PyObjectPayload::Dict(map) => { map.write().insert(hk, value); }
            _ => unreachable!(),
        }
        skip_ws(s, pos);
        if *pos >= s.len() { break; }
        if s.as_bytes()[*pos] == b'}' { *pos += 1; return Ok(dict); }
        if s.as_bytes()[*pos] == b',' { *pos += 1; } else { break; }
    }
    Err(PyException::value_error("Invalid JSON object"))
}

// ── time module ──


pub fn create_csv_module() -> PyObjectRef {
    make_module("csv", vec![
        ("reader", make_builtin(csv_reader)),
        ("writer", make_builtin(|_| Ok(PyObject::none()))),
        ("DictReader", make_builtin(|_| Ok(PyObject::none()))),
        ("DictWriter", make_builtin(|_| Ok(PyObject::none()))),
        ("QUOTE_ALL", PyObject::int(1)),
        ("QUOTE_MINIMAL", PyObject::int(0)),
        ("QUOTE_NONNUMERIC", PyObject::int(2)),
        ("QUOTE_NONE", PyObject::int(3)),
    ])
}

fn csv_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.reader requires an iterable"));
    }
    // Convert iterable of strings into list of lists
    let lines = args[0].to_list()?;
    let mut rows = Vec::new();
    for line in &lines {
        let s = line.py_to_string();
        let fields: Vec<PyObjectRef> = s.split(',')
            .map(|f| {
                let f = f.trim();
                let f = if f.starts_with('"') && f.ends_with('"') {
                    &f[1..f.len()-1]
                } else {
                    f
                };
                PyObject::str_val(CompactString::from(f))
            })
            .collect();
        rows.push(PyObject::list(fields));
    }
    Ok(PyObject::list(rows))
}

// ── shutil module (basic) ──


pub fn create_base64_module() -> PyObjectRef {
    make_module("base64", vec![
        ("b64encode", make_builtin(base64_encode)),
        ("b64decode", make_builtin(base64_decode)),
        ("b16encode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b16encode requires data")); }
            let data = extract_bytes(&args[0])?;
            let hex: String = data.iter().map(|b| format!("{:02X}", b)).collect();
            Ok(PyObject::bytes(hex.into_bytes()))
        })),
        ("b16decode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b16decode requires data")); }
            let s = args[0].py_to_string();
            let bytes: Vec<u8> = (0..s.len())
                .step_by(2)
                .filter_map(|i| u8::from_str_radix(&s[i..i+2], 16).ok())
                .collect();
            Ok(PyObject::bytes(bytes))
        })),
        ("encodebytes", make_builtin(base64_encode)),
        ("decodebytes", make_builtin(base64_decode)),
    ])
}

pub(crate) fn extract_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.clone()),
        PyObjectPayload::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(PyException::type_error("expected bytes-like object")),
    }
}

fn base64_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("b64encode requires data")); }
    let data = extract_bytes(&args[0])?;
    // Simple base64 encoding
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize]);
        result.push(CHARS[((n >> 12) & 63) as usize]);
        if chunk.len() > 1 { result.push(CHARS[((n >> 6) & 63) as usize]); } else { result.push(b'='); }
        if chunk.len() > 2 { result.push(CHARS[(n & 63) as usize]); } else { result.push(b'='); }
    }
    Ok(PyObject::bytes(result))
}

fn base64_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("b64decode requires data")); }
    let input_bytes = extract_bytes(&args[0])?;
    let input: Vec<u8> = input_bytes.iter().copied().filter(|&b| b != b'\n' && b != b'\r').collect();
    fn decode_char(c: u8) -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    }
    let mut result = Vec::new();
    for chunk in input.chunks(4) {
        if chunk.len() < 4 { break; }
        let n = (decode_char(chunk[0]) << 18) | (decode_char(chunk[1]) << 12) | (decode_char(chunk[2]) << 6) | decode_char(chunk[3]);
        result.push((n >> 16) as u8);
        if chunk[2] != b'=' { result.push((n >> 8) as u8); }
        if chunk[3] != b'=' { result.push(n as u8); }
    }
    Ok(PyObject::bytes(result))
}

// ── pprint module ──


pub fn create_struct_module() -> PyObjectRef {
    make_module("struct", vec![
        ("pack", make_builtin(struct_pack)),
        ("unpack", make_builtin(struct_unpack)),
        ("calcsize", make_builtin(struct_calcsize)),
    ])
}

fn struct_calcsize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("calcsize requires format string")); }
    let fmt = args[0].py_to_string();
    let mut size = 0usize;
    let mut chars = fmt.chars().peekable();
    // Skip byte order
    if let Some(&c) = chars.peek() {
        if "<>!=@".contains(c) { chars.next(); }
    }
    while let Some(c) = chars.next() {
        let count = if c.is_ascii_digit() {
            let mut n = (c as u8 - b'0') as usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() { n = n * 10 + (d as u8 - b'0') as usize; chars.next(); } else { break; }
            }
            let fc = chars.next().unwrap_or('x');
            size += n * format_char_size(fc);
            continue;
        } else { 1 };
        size += count * format_char_size(c);
    }
    Ok(PyObject::int(size as i64))
}

fn format_char_size(c: char) -> usize {
    match c {
        'x' | 'c' | 'b' | 'B' | '?' => 1,
        'h' | 'H' => 2,
        'i' | 'I' | 'l' | 'L' | 'f' => 4,
        'q' | 'Q' | 'd' => 8,
        'n' | 'N' | 'P' => std::mem::size_of::<usize>(),
        's' | 'p' => 1,
        _ => 0,
    }
}

fn struct_pack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("pack requires format string")); }
    let fmt = args[0].py_to_string();
    let mut result = Vec::new();
    let mut arg_idx = 1;
    let mut chars = fmt.chars().peekable();
    let little_endian = match chars.peek() {
        Some('<') => { chars.next(); true }
        Some('>') | Some('!') => { chars.next(); false }
        Some('=') | Some('@') => { chars.next(); cfg!(target_endian = "little") }
        _ => cfg!(target_endian = "little"),
    };
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() { continue; } // count handling simplified
        match c {
            'b' | 'B' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u8;
                result.push(val);
                arg_idx += 1;
            }
            'h' | 'H' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u16;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'i' | 'I' | 'l' | 'L' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u32;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'q' | 'Q' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u64;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'f' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_float()? as f32;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'd' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_float()?;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            '?' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                result.push(if args[arg_idx].is_truthy() { 1 } else { 0 });
                arg_idx += 1;
            }
            'x' => result.push(0),
            _ => {}
        }
    }
    Ok(PyObject::bytes(result))
}

fn struct_unpack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("unpack requires format string and bytes")); }
    let fmt = args[0].py_to_string();
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => b.clone(),
        _ => return Err(PyException::type_error("unpack requires bytes argument")),
    };
    let mut result = Vec::new();
    let mut offset = 0;
    let mut chars = fmt.chars().peekable();
    let little_endian = match chars.peek() {
        Some('<') => { chars.next(); true }
        Some('>') | Some('!') => { chars.next(); false }
        Some('=') | Some('@') => { chars.next(); cfg!(target_endian = "little") }
        _ => cfg!(target_endian = "little"),
    };
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() { continue; }
        match c {
            'b' => {
                if offset >= data.len() { break; }
                result.push(PyObject::int(data[offset] as i8 as i64));
                offset += 1;
            }
            'B' => {
                if offset >= data.len() { break; }
                result.push(PyObject::int(data[offset] as i64));
                offset += 1;
            }
            'h' => {
                if offset + 2 > data.len() { break; }
                let bytes: [u8; 2] = [data[offset], data[offset + 1]];
                let val = if little_endian { i16::from_le_bytes(bytes) } else { i16::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 2;
            }
            'H' => {
                if offset + 2 > data.len() { break; }
                let bytes: [u8; 2] = [data[offset], data[offset + 1]];
                let val = if little_endian { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 2;
            }
            'i' | 'l' => {
                if offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
                let val = if little_endian { i32::from_le_bytes(bytes) } else { i32::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 4;
            }
            'I' | 'L' => {
                if offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
                let val = if little_endian { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 4;
            }
            'q' => {
                if offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[offset..offset+8]);
                let val = if little_endian { i64::from_le_bytes(bytes) } else { i64::from_be_bytes(bytes) };
                result.push(PyObject::int(val));
                offset += 8;
            }
            'Q' => {
                if offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[offset..offset+8]);
                let val = if little_endian { u64::from_le_bytes(bytes) } else { u64::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 8;
            }
            'f' => {
                if offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
                let val = if little_endian { f32::from_le_bytes(bytes) } else { f32::from_be_bytes(bytes) };
                result.push(PyObject::float(val as f64));
                offset += 4;
            }
            'd' => {
                if offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[offset..offset+8]);
                let val = if little_endian { f64::from_le_bytes(bytes) } else { f64::from_be_bytes(bytes) };
                result.push(PyObject::float(val));
                offset += 8;
            }
            '?' => {
                if offset >= data.len() { break; }
                result.push(PyObject::bool_val(data[offset] != 0));
                offset += 1;
            }
            'x' => { offset += 1; }
            _ => {}
        }
    }
    Ok(PyObject::tuple(result))
}

// ── textwrap module ──


