use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

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
            let upper = s.to_uppercase();
            let bytes: Vec<u8> = (0..upper.len())
                .step_by(2)
                .filter_map(|i| {
                    if i + 2 <= upper.len() {
                        u8::from_str_radix(&upper[i..i+2], 16).ok()
                    } else {
                        None
                    }
                })
                .collect();
            Ok(PyObject::bytes(bytes))
        })),
        ("b32encode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b32encode requires data")); }
            let data = extract_bytes(&args[0])?;
            const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
            let mut result = Vec::new();
            let chunks = data.chunks(5);
            for chunk in chunks {
                let mut buf = [0u8; 5];
                buf[..chunk.len()].copy_from_slice(chunk);
                let b = buf;
                // 5 bytes = 40 bits -> 8 base32 chars
                let indices = [
                    (b[0] >> 3) & 0x1F,
                    ((b[0] & 0x07) << 2) | ((b[1] >> 6) & 0x03),
                    (b[1] >> 1) & 0x1F,
                    ((b[1] & 0x01) << 4) | ((b[2] >> 4) & 0x0F),
                    ((b[2] & 0x0F) << 1) | ((b[3] >> 7) & 0x01),
                    (b[3] >> 2) & 0x1F,
                    ((b[3] & 0x03) << 3) | ((b[4] >> 5) & 0x07),
                    b[4] & 0x1F,
                ];
                let num_chars = match chunk.len() {
                    1 => 2, 2 => 4, 3 => 5, 4 => 7, 5 => 8, _ => 0,
                };
                let padding = 8 - num_chars;
                for i in 0..num_chars {
                    result.push(ALPHABET[indices[i] as usize]);
                }
                for _ in 0..padding {
                    result.push(b'=');
                }
            }
            Ok(PyObject::bytes(result))
        })),
        ("b32decode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b32decode requires data")); }
            let input_bytes = extract_bytes(&args[0])?;
            let input: Vec<u8> = input_bytes.iter().copied()
                .filter(|&b| b != b'\n' && b != b'\r')
                .collect();
            fn decode_b32(c: u8) -> u8 {
                match c {
                    b'A'..=b'Z' => c - b'A',
                    b'a'..=b'z' => c - b'a',
                    b'2'..=b'7' => c - b'2' + 26,
                    _ => 0,
                }
            }
            let mut result = Vec::new();
            for chunk in input.chunks(8) {
                let pad_count = chunk.iter().filter(|&&b| b == b'=').count();
                let mut vals = [0u8; 8];
                for (i, &b) in chunk.iter().enumerate() {
                    vals[i] = decode_b32(b);
                }
                let n = ((vals[0] as u64) << 35) | ((vals[1] as u64) << 30)
                    | ((vals[2] as u64) << 25) | ((vals[3] as u64) << 20)
                    | ((vals[4] as u64) << 15) | ((vals[5] as u64) << 10)
                    | ((vals[6] as u64) << 5) | (vals[7] as u64);
                let out_bytes = match pad_count {
                    6 => 1, 4 => 2, 3 => 3, 1 => 4, 0 => 5, _ => 0,
                };
                if out_bytes >= 1 { result.push((n >> 32) as u8); }
                if out_bytes >= 2 { result.push((n >> 24) as u8); }
                if out_bytes >= 3 { result.push((n >> 16) as u8); }
                if out_bytes >= 4 { result.push((n >> 8) as u8); }
                if out_bytes >= 5 { result.push(n as u8); }
            }
            Ok(PyObject::bytes(result))
        })),
        ("urlsafe_b64encode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("urlsafe_b64encode requires data")); }
            let encoded = base64_encode(args)?;
            let bytes = extract_bytes(&encoded)?;
            let safe: Vec<u8> = bytes.iter().map(|&b| match b {
                b'+' => b'-',
                b'/' => b'_',
                _ => b,
            }).collect();
            Ok(PyObject::bytes(safe))
        })),
        ("urlsafe_b64decode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("urlsafe_b64decode requires data")); }
            let input_bytes = extract_bytes(&args[0])?;
            let standard: Vec<u8> = input_bytes.iter().map(|&b| match b {
                b'-' => b'+',
                b'_' => b'/',
                _ => b,
            }).collect();
            let standard_obj = PyObject::bytes(standard);
            base64_decode(&[standard_obj])
        })),
        ("encodebytes", make_builtin(base64_encode)),
        ("decodebytes", make_builtin(base64_decode)),
        ("standard_b64encode", make_builtin(base64_encode)),
        ("standard_b64decode", make_builtin(base64_decode)),
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
        ("unpack_from", make_builtin(struct_unpack_from)),
        ("iter_unpack", make_builtin(struct_iter_unpack)),
        ("calcsize", make_builtin(struct_calcsize)),
        ("Struct", make_builtin(struct_struct_ctor)),
        ("error", PyObject::class(CompactString::from("error"), vec![], indexmap::IndexMap::new())),
    ])
}

fn struct_struct_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Struct() requires a format string"));
    }
    let fmt_str = args[0].py_to_string();
    let cls = PyObject::class(CompactString::from("Struct"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("format"), PyObject::str_val(CompactString::from(&fmt_str)));
        // Compute size
        let size_obj = struct_calcsize(&[PyObject::str_val(CompactString::from(&fmt_str))])?;
        w.insert(CompactString::from("size"), size_obj);
        let fmt_for_pack = fmt_str.clone();
        w.insert(CompactString::from("pack"), PyObject::native_closure("pack", move |args| {
            let mut full_args = vec![PyObject::str_val(CompactString::from(&fmt_for_pack))];
            full_args.extend_from_slice(args);
            struct_pack(&full_args)
        }));
        let fmt_for_unpack = fmt_str.clone();
        w.insert(CompactString::from("unpack"), PyObject::native_closure("unpack", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("Struct.unpack() requires a buffer"));
            }
            struct_unpack(&[PyObject::str_val(CompactString::from(&fmt_for_unpack)), args[0].clone()])
        }));
        let fmt_for_uf = fmt_str.clone();
        w.insert(CompactString::from("unpack_from"), PyObject::native_closure("unpack_from", move |args| {
            let mut full_args = vec![PyObject::str_val(CompactString::from(&fmt_for_uf))];
            full_args.extend_from_slice(args);
            struct_unpack_from(&full_args)
        }));
        let fmt_for_iu = fmt_str;
        w.insert(CompactString::from("iter_unpack"), PyObject::native_closure("iter_unpack", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("Struct.iter_unpack() requires a buffer"));
            }
            struct_iter_unpack(&[PyObject::str_val(CompactString::from(&fmt_for_iu)), args[0].clone()])
        }));
    }
    Ok(inst)
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

/// Extract a u64 from a PyObject, supporting both Small(i64) and Big(BigInt).
fn extract_u64(obj: &PyObjectRef) -> PyResult<u64> {
    use ferrython_core::types::PyInt;
    match &obj.payload {
        PyObjectPayload::Int(PyInt::Small(n)) => Ok(*n as u64),
        PyObjectPayload::Int(PyInt::Big(n)) => {
            use num_traits::ToPrimitive;
            n.to_u64().ok_or_else(|| PyException::overflow_error("int too large for unsigned 64-bit"))
        }
        PyObjectPayload::Bool(b) => Ok(if *b { 1 } else { 0 }),
        _ => Err(PyException::type_error("required integer")),
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
        // Parse optional repeat count
        let count = if c.is_ascii_digit() {
            let mut n = (c as u8 - b'0') as usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() { n = n * 10 + (d as u8 - b'0') as usize; chars.next(); } else { break; }
            }
            let fc = match chars.next() {
                Some(fc) => fc,
                None => break,
            };
            pack_one_format(fc, n, &args, &mut arg_idx, &mut result, little_endian)?;
            continue;
        } else { 1usize };
        pack_one_format(c, count, &args, &mut arg_idx, &mut result, little_endian)?;
    }
    Ok(PyObject::bytes(result))
}

fn pack_one_format(c: char, count: usize, args: &[PyObjectRef], arg_idx: &mut usize, result: &mut Vec<u8>, little_endian: bool) -> PyResult<()> {
    match c {
        's' => {
            if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
            let src = match &args[*arg_idx].payload {
                PyObjectPayload::Bytes(b) => b.clone(),
                _ => args[*arg_idx].py_to_string().into_bytes(),
            };
            for i in 0..count {
                result.push(if i < src.len() { src[i] } else { 0 });
            }
            *arg_idx += 1;
        }
        'b' | 'B' => {
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[*arg_idx].to_int()? as u8;
                result.push(val);
                *arg_idx += 1;
            }
        }
        'h' | 'H' => {
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[*arg_idx].to_int()? as u16;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'i' | 'I' | 'l' | 'L' => {
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[*arg_idx].to_int()? as u32;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'q' | 'Q' => {
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let bytes = if c == 'Q' {
                    // Unsigned 64-bit: extract as u64 (handles values > i64::MAX)
                    let val = extract_u64(&args[*arg_idx])?;
                    if little_endian { val.to_le_bytes() } else { val.to_be_bytes() }
                } else {
                    // Signed 64-bit
                    let val = args[*arg_idx].to_int()?;
                    if little_endian { val.to_le_bytes() } else { val.to_be_bytes() }
                };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'f' => {
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[*arg_idx].to_float()? as f32;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'd' => {
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[*arg_idx].to_float()?;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        '?' => {
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                result.push(if args[*arg_idx].is_truthy() { 1 } else { 0 });
                *arg_idx += 1;
            }
        }
        'x' => {
            for _ in 0..count { result.push(0); }
        }
        _ => {}
    }
    Ok(())
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
        // Parse optional repeat count
        let count = if c.is_ascii_digit() {
            let mut n = (c as u8 - b'0') as usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() { n = n * 10 + (d as u8 - b'0') as usize; chars.next(); } else { break; }
            }
            let fc = match chars.next() {
                Some(fc) => fc,
                None => break,
            };
            unpack_one_format(fc, n, &data, &mut offset, &mut result, little_endian);
            continue;
        } else { 1usize };
        unpack_one_format(c, count, &data, &mut offset, &mut result, little_endian);
    }
    Ok(PyObject::tuple(result))
}

/// struct.unpack_from(fmt, buffer, offset=0)
fn struct_unpack_from(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("unpack_from requires format and buffer")); }
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => b.clone(),
        _ => return Err(PyException::type_error("unpack_from requires bytes buffer")),
    };
    let start_offset = if args.len() > 2 { args[2].as_int().unwrap_or(0) as usize } else { 0 };
    if start_offset > data.len() {
        return Err(PyException::runtime_error("unpack_from offset out of range"));
    }
    let sliced = &data[start_offset..];
    struct_unpack(&[args[0].clone(), PyObject::bytes(sliced.to_vec())])
}

/// struct.iter_unpack(fmt, buffer) → iterator of tuples
fn struct_iter_unpack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("iter_unpack requires format and buffer")); }
    let fmt_obj = &args[0];
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => b.clone(),
        _ => return Err(PyException::type_error("iter_unpack requires bytes buffer")),
    };
    let size = struct_calcsize(&[fmt_obj.clone()])?.as_int().unwrap_or(0) as usize;
    if size == 0 { return Err(PyException::runtime_error("iter_unpack format has zero size")); }
    let mut results = Vec::new();
    let mut offset = 0;
    while offset + size <= data.len() {
        let chunk = PyObject::bytes(data[offset..offset + size].to_vec());
        let tup = struct_unpack(&[fmt_obj.clone(), chunk])?;
        results.push(tup);
        offset += size;
    }
    Ok(PyObject::list(results))
}

fn unpack_one_format(c: char, count: usize, data: &[u8], offset: &mut usize, result: &mut Vec<PyObjectRef>, little_endian: bool) {
    match c {
        's' => {
            if *offset + count > data.len() { return; }
            let slice = data[*offset..*offset + count].to_vec();
            result.push(PyObject::bytes(slice));
            *offset += count;
        }
        'b' => {
            for _ in 0..count {
                if *offset >= data.len() { break; }
                result.push(PyObject::int(data[*offset] as i8 as i64));
                *offset += 1;
            }
        }
        'B' => {
            for _ in 0..count {
                if *offset >= data.len() { break; }
                result.push(PyObject::int(data[*offset] as i64));
                *offset += 1;
            }
        }
        'h' => {
            for _ in 0..count {
                if *offset + 2 > data.len() { break; }
                let bytes: [u8; 2] = [data[*offset], data[*offset + 1]];
                let val = if little_endian { i16::from_le_bytes(bytes) } else { i16::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                *offset += 2;
            }
        }
        'H' => {
            for _ in 0..count {
                if *offset + 2 > data.len() { break; }
                let bytes: [u8; 2] = [data[*offset], data[*offset + 1]];
                let val = if little_endian { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                *offset += 2;
            }
        }
        'i' | 'l' => {
            for _ in 0..count {
                if *offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[*offset], data[*offset+1], data[*offset+2], data[*offset+3]];
                let val = if little_endian { i32::from_le_bytes(bytes) } else { i32::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                *offset += 4;
            }
        }
        'I' | 'L' => {
            for _ in 0..count {
                if *offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[*offset], data[*offset+1], data[*offset+2], data[*offset+3]];
                let val = if little_endian { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                *offset += 4;
            }
        }
        'q' => {
            for _ in 0..count {
                if *offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[*offset..*offset+8]);
                let val = if little_endian { i64::from_le_bytes(bytes) } else { i64::from_be_bytes(bytes) };
                result.push(PyObject::int(val));
                *offset += 8;
            }
        }
        'Q' => {
            for _ in 0..count {
                if *offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[*offset..*offset+8]);
                let val = if little_endian { u64::from_le_bytes(bytes) } else { u64::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                *offset += 8;
            }
        }
        'f' => {
            for _ in 0..count {
                if *offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[*offset], data[*offset+1], data[*offset+2], data[*offset+3]];
                let val = if little_endian { f32::from_le_bytes(bytes) } else { f32::from_be_bytes(bytes) };
                result.push(PyObject::float(val as f64));
                *offset += 4;
            }
        }
        'd' => {
            for _ in 0..count {
                if *offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[*offset..*offset+8]);
                let val = if little_endian { f64::from_le_bytes(bytes) } else { f64::from_be_bytes(bytes) };
                result.push(PyObject::float(val));
                *offset += 8;
            }
        }
        '?' => {
            for _ in 0..count {
                if *offset >= data.len() { break; }
                result.push(PyObject::bool_val(data[*offset] != 0));
                *offset += 1;
            }
        }
        'x' => { *offset += count; }
        _ => {}
    }
}

// ── pickle module ──

// Marker bytes for our simplified pickle format
const PICKLE_NONE: u8 = b'N';
const PICKLE_TRUE: u8 = b'T';
const PICKLE_FALSE: u8 = b'F';
const PICKLE_INT: u8 = b'I';
const PICKLE_FLOAT: u8 = b'D';
const PICKLE_STR: u8 = b'S';
const PICKLE_BYTES: u8 = b'B';
const PICKLE_LIST: u8 = b'L';
const PICKLE_TUPLE: u8 = b'U';
const PICKLE_DICT: u8 = b'd';
const PICKLE_INSTANCE: u8 = b'O';
const PICKLE_SET: u8 = b's';
const PICKLE_FROZENSET: u8 = b'f';
const PICKLE_STOP: u8 = b'.';

pub fn create_pickle_module() -> PyObjectRef {
    make_module("pickle", vec![
        ("dumps", make_builtin(pickle_dumps)),
        ("loads", make_builtin(pickle_loads)),
        ("dump", make_builtin(pickle_dump)),
        ("load", make_builtin(pickle_load)),
        ("HIGHEST_PROTOCOL", PyObject::int(5)),
        ("DEFAULT_PROTOCOL", PyObject::int(4)),
        ("PicklingError", PyObject::str_val(CompactString::from("PicklingError"))),
        ("UnpicklingError", PyObject::str_val(CompactString::from("UnpicklingError"))),
    ])
}

fn pickle_serialize(obj: &PyObjectRef, buf: &mut Vec<u8>) -> PyResult<()> {
    match &obj.payload {
        PyObjectPayload::None => buf.push(PICKLE_NONE),
        PyObjectPayload::Bool(b) => buf.push(if *b { PICKLE_TRUE } else { PICKLE_FALSE }),
        PyObjectPayload::Int(n) => {
            buf.push(PICKLE_INT);
            let s = n.to_string();
            let bytes = s.as_bytes();
            let len = bytes.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        PyObjectPayload::Float(f) => {
            buf.push(PICKLE_FLOAT);
            buf.extend_from_slice(&f.to_le_bytes());
        }
        PyObjectPayload::Str(s) => {
            buf.push(PICKLE_STR);
            let bytes = s.as_bytes();
            let len = bytes.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            buf.push(PICKLE_BYTES);
            let len = b.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(b);
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            let count = items.len() as u32;
            for item in items.iter() {
                pickle_serialize(item, buf)?;
            }
            buf.push(PICKLE_LIST);
            buf.extend_from_slice(&count.to_le_bytes());
        }
        PyObjectPayload::Tuple(items) => {
            let count = items.len() as u32;
            for item in items.iter() {
                pickle_serialize(item, buf)?;
            }
            buf.push(PICKLE_TUPLE);
            buf.extend_from_slice(&count.to_le_bytes());
        }
        PyObjectPayload::Dict(map) => {
            let map = map.read();
            let count = map.len() as u32;
            for (k, v) in map.iter() {
                let key_obj = match k {
                    HashableKey::Str(s) => PyObject::str_val(s.clone()),
                    HashableKey::Int(n) => PyObject::int(n.to_i64().unwrap_or(0)),
                    HashableKey::Float(f) => PyObject::float(f.0),
                    HashableKey::Bool(b) => PyObject::bool_val(*b),
                    _ => PyObject::str_val(CompactString::from(format!("{:?}", k))),
                };
                pickle_serialize(&key_obj, buf)?;
                pickle_serialize(v, buf)?;
            }
            buf.push(PICKLE_DICT);
            buf.extend_from_slice(&count.to_le_bytes());
        }
        PyObjectPayload::Set(items) => {
            let items_r = items.read();
            let count = items_r.len() as u32;
            for (_, v) in items_r.iter() {
                pickle_serialize(v, buf)?;
            }
            buf.push(PICKLE_SET);
            buf.extend_from_slice(&count.to_le_bytes());
        }
        PyObjectPayload::FrozenSet(items) => {
            let count = items.len() as u32;
            for (_, v) in items.iter() {
                pickle_serialize(v, buf)?;
            }
            buf.push(PICKLE_FROZENSET);
            buf.extend_from_slice(&count.to_le_bytes());
        }
        PyObjectPayload::Instance(inst) => {
            // Collect serializable instance attrs (skip methods/closures)
            let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                cd.name.to_string()
            } else {
                "object".to_string()
            };
            let attrs_r = inst.attrs.read();
            let mut data_pairs: Vec<(CompactString, PyObjectRef)> = Vec::new();
            for (k, v) in attrs_r.iter() {
                match &v.payload {
                    PyObjectPayload::NativeFunction { .. }
                    | PyObjectPayload::NativeClosure { .. }
                    | PyObjectPayload::Function(_)
                    | PyObjectPayload::Class(_) => continue,
                    _ => {
                        data_pairs.push((k.clone(), v.clone()));
                    }
                }
            }
            // Serialize attrs as key-value pairs first (pushed to stack)
            for (k, v) in &data_pairs {
                pickle_serialize(&PyObject::str_val(k.clone()), buf)?;
                pickle_serialize(v, buf)?;
            }
            // Then the marker with class name + count
            let name_bytes = class_name.as_bytes();
            buf.push(PICKLE_INSTANCE);
            let name_len = name_bytes.len() as u32;
            buf.extend_from_slice(&name_len.to_le_bytes());
            buf.extend_from_slice(name_bytes);
            let count = data_pairs.len() as u32;
            buf.extend_from_slice(&count.to_le_bytes());
        }
        _ => {
            return Err(PyException::runtime_error(
                format!("PicklingError: can't pickle object of type {}", obj.type_name()),
            ));
        }
    }
    Ok(())
}

fn pickle_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.dumps() missing 1 required positional argument: 'obj'",
        ));
    }
    // args[0] = obj, args[1] = protocol (ignored)
    let mut buf = Vec::new();
    // Protocol header
    buf.push(0x80);
    buf.push(4); // protocol version
    pickle_serialize(&args[0], &mut buf)?;
    buf.push(PICKLE_STOP);
    Ok(PyObject::bytes(buf))
}

fn _pickle_deserialize(data: &[u8], pos: &mut usize) -> PyResult<PyObjectRef> {
    if *pos >= data.len() {
        return Err(PyException::runtime_error("UnpicklingError: unexpected end of data"));
    }
    let marker = data[*pos];
    *pos += 1;
    match marker {
        PICKLE_NONE => Ok(PyObject::none()),
        PICKLE_TRUE => Ok(PyObject::bool_val(true)),
        PICKLE_FALSE => Ok(PyObject::bool_val(false)),
        PICKLE_INT => {
            if *pos + 4 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated int"));
            }
            let len = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]) as usize;
            *pos += 4;
            if *pos + len > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated int data"));
            }
            let s = std::str::from_utf8(&data[*pos..*pos+len])
                .map_err(|_| PyException::runtime_error("UnpicklingError: invalid int encoding"))?;
            *pos += len;
            let val: i64 = s.parse()
                .map_err(|_| PyException::runtime_error("UnpicklingError: invalid int value"))?;
            Ok(PyObject::int(val))
        }
        PICKLE_FLOAT => {
            if *pos + 8 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated float"));
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&data[*pos..*pos+8]);
            *pos += 8;
            Ok(PyObject::float(f64::from_le_bytes(bytes)))
        }
        PICKLE_STR => {
            if *pos + 4 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated str length"));
            }
            let len = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]) as usize;
            *pos += 4;
            if *pos + len > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated str data"));
            }
            let s = std::str::from_utf8(&data[*pos..*pos+len])
                .map_err(|_| PyException::runtime_error("UnpicklingError: invalid utf-8 in str"))?;
            *pos += len;
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        PICKLE_BYTES => {
            if *pos + 4 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated bytes length"));
            }
            let len = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]) as usize;
            *pos += 4;
            if *pos + len > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated bytes data"));
            }
            let b = data[*pos..*pos+len].to_vec();
            *pos += len;
            Ok(PyObject::bytes(b))
        }
        PICKLE_LIST => {
            if *pos + 4 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated list count"));
            }
            let count = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]) as usize;
            *pos += 4;
            // Elements were serialized before the marker, so we need to
            // re-parse from the start. Use a stack-based approach instead.
            // This branch is reached after elements are already on the stack,
            // but since we parse linearly, we handle it by collecting from
            // recursive calls.
            // Actually, the format serializes children first then marker.
            // So we need a stack-based parser.
            // Let's stash count for now and note that the caller should use
            // the stack-based pickle_loads_stack.
            // This won't be reached — we use stack-based deserialization.
            let _ = count;
            Err(PyException::runtime_error("UnpicklingError: internal error"))
        }
        _ => Err(PyException::runtime_error(
            format!("UnpicklingError: unknown marker byte 0x{:02x}", marker),
        )),
    }
}

fn pickle_loads_stack(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos = 0;
    // Skip protocol header if present
    if pos < data.len() && data[pos] == 0x80 {
        pos += 2; // skip 0x80 + version byte
    }

    let mut stack: Vec<PyObjectRef> = Vec::new();

    while pos < data.len() {
        let marker = data[pos];
        pos += 1;
        match marker {
            PICKLE_STOP => break,
            PICKLE_NONE => stack.push(PyObject::none()),
            PICKLE_TRUE => stack.push(PyObject::bool_val(true)),
            PICKLE_FALSE => stack.push(PyObject::bool_val(false)),
            PICKLE_INT => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated int"));
                }
                let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated int data"));
                }
                let s = std::str::from_utf8(&data[pos..pos+len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid int encoding"))?;
                pos += len;
                let val: i64 = s.parse()
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid int value"))?;
                stack.push(PyObject::int(val));
            }
            PICKLE_FLOAT => {
                if pos + 8 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated float"));
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[pos..pos+8]);
                pos += 8;
                stack.push(PyObject::float(f64::from_le_bytes(bytes)));
            }
            PICKLE_STR => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated str"));
                }
                let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated str data"));
                }
                let s = std::str::from_utf8(&data[pos..pos+len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid utf-8"))?;
                pos += len;
                stack.push(PyObject::str_val(CompactString::from(s)));
            }
            PICKLE_BYTES => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated bytes"));
                }
                let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated bytes data"));
                }
                let b = data[pos..pos+len].to_vec();
                pos += len;
                stack.push(PyObject::bytes(b));
            }
            PICKLE_LIST => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated list count"));
                }
                let count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if stack.len() < count {
                    return Err(PyException::runtime_error("UnpicklingError: stack underflow for list"));
                }
                let items = stack.split_off(stack.len() - count);
                stack.push(PyObject::list(items));
            }
            PICKLE_TUPLE => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated tuple count"));
                }
                let count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if stack.len() < count {
                    return Err(PyException::runtime_error("UnpicklingError: stack underflow for tuple"));
                }
                let items = stack.split_off(stack.len() - count);
                stack.push(PyObject::tuple(items));
            }
            PICKLE_DICT => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated dict count"));
                }
                let count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                let pair_count = count * 2;
                if stack.len() < pair_count {
                    return Err(PyException::runtime_error("UnpicklingError: stack underflow for dict"));
                }
                let kv_items = stack.split_off(stack.len() - pair_count);
                let mut pairs = Vec::new();
                for chunk in kv_items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PyObject::dict_from_pairs(pairs));
            }
            PICKLE_SET | PICKLE_FROZENSET => {
                let is_frozen = marker == PICKLE_FROZENSET;
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated set count"));
                }
                let count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if stack.len() < count {
                    return Err(PyException::runtime_error("UnpicklingError: stack underflow for set"));
                }
                let items = stack.split_off(stack.len() - count);
                let mut map = IndexMap::new();
                for item in &items {
                    if let Ok(hk) = HashableKey::from_object(item) {
                        map.insert(hk, item.clone());
                    }
                }
                if is_frozen {
                    stack.push(PyObject::frozenset(map));
                } else {
                    stack.push(PyObject::set(map));
                }
            }
            PICKLE_INSTANCE => {
                // Read class name
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated instance class name"));
                }
                let name_len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + name_len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated class name data"));
                }
                let class_name = std::str::from_utf8(&data[pos..pos+name_len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid class name utf-8"))?;
                pos += name_len;
                // Read attr count
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated instance attr count"));
                }
                let count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                let pair_count = count * 2;
                if stack.len() < pair_count {
                    return Err(PyException::runtime_error("UnpicklingError: stack underflow for instance"));
                }
                let kv_items = stack.split_off(stack.len() - pair_count);
                let mut attrs = IndexMap::new();
                for chunk in kv_items.chunks_exact(2) {
                    let key = chunk[0].py_to_string();
                    attrs.insert(CompactString::from(key), chunk[1].clone());
                }
                let cls = PyObject::class(CompactString::from(class_name), vec![], IndexMap::new());
                stack.push(PyObject::instance_with_attrs(cls, attrs));
            }
            _ => {
                return Err(PyException::runtime_error(
                    format!("UnpicklingError: unknown marker byte 0x{:02x}", marker),
                ));
            }
        }
    }

    stack.pop().ok_or_else(|| PyException::runtime_error("UnpicklingError: empty pickle data"))
}

fn pickle_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.loads() missing 1 required positional argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;
    pickle_loads_stack(&data)
}

fn pickle_dump(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "pickle.dump() missing required arguments: 'obj' and 'file'",
        ));
    }
    let data = pickle_dumps(&args[0..1])?;
    let data_bytes = extract_bytes(&data)?;
    // Write to file — expects a file-like object with a write() method
    if let Some(write_fn) = args[1].get_attr("write") {
        let _ = write_fn; // stub: actual call dispatch not available without VM
    }
    // Fallback: if the file arg is a string path, write directly
    if let PyObjectPayload::Str(path) = &args[1].payload {
        std::fs::write(path.as_str(), &data_bytes)
            .map_err(|e| PyException::runtime_error(format!("pickle.dump: {}", e)))?;
    }
    Ok(PyObject::none())
}

fn pickle_load(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.load() missing 1 required positional argument: 'file'",
        ));
    }
    // If file arg is a string path, read directly
    if let PyObjectPayload::Str(path) = &args[0].payload {
        let data = std::fs::read(path.as_str())
            .map_err(|e| PyException::runtime_error(format!("pickle.load: {}", e)))?;
        return pickle_loads_stack(&data);
    }
    Err(PyException::runtime_error(
        "pickle.load: expected a file path or file-like object",
    ))
}

// ── textwrap module ──


// ── binascii module ──

pub fn create_binascii_module() -> PyObjectRef {
    let hexlify_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("hexlify", args, 1)?;
        let data = extract_bytes(&args[0])?;
        let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(PyObject::bytes(hex.into_bytes()))
    });

    let unhexlify_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("unhexlify", args, 1)?;
        let hex_str = match &args[0].payload {
            PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
            PyObjectPayload::Str(s) => s.to_string(),
            _ => args[0].py_to_string(),
        };
        let hex_str = hex_str.trim();
        if hex_str.len() % 2 != 0 {
            return Err(PyException::value_error("Odd-length string"));
        }
        let mut result = Vec::with_capacity(hex_str.len() / 2);
        for i in (0..hex_str.len()).step_by(2) {
            let byte = u8::from_str_radix(&hex_str[i..i+2], 16)
                .map_err(|_| PyException::value_error("Non-hexadecimal digit found"))?;
            result.push(byte);
        }
        Ok(PyObject::bytes(result))
    });

    let crc32_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("crc32", args, 1)?;
        let data = extract_bytes(&args[0])?;
        let mut crc: u32 = if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Int(n) => n.to_i64().unwrap_or(0) as u32,
                _ => 0,
            }
        } else { 0 };
        crc = !crc;
        for &byte in &data {
            crc ^= byte as u32;
            for _ in 0..8 {
                if crc & 1 != 0 { crc = (crc >> 1) ^ 0xEDB88320; }
                else { crc >>= 1; }
            }
        }
        Ok(PyObject::int(!crc as i64))
    });

    let b2a_base64_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("b2a_base64", args, 1)?;
        let data = extract_bytes(&args[0])?;
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
        result.push(b'\n');
        Ok(PyObject::bytes(result))
    });

    let a2b_base64_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("a2b_base64", args, 1)?;
        let input_str = match &args[0].payload {
            PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
            _ => args[0].py_to_string(),
        };
        let input: Vec<u8> = input_str.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
        fn decode_char(c: u8) -> u32 {
            match c {
                b'A'..=b'Z' => (c - b'A') as u32, b'a'..=b'z' => (c - b'a' + 26) as u32,
                b'0'..=b'9' => (c - b'0' + 52) as u32, b'+' => 62, b'/' => 63, _ => 0,
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
    });

    make_module("binascii", vec![
        ("hexlify", hexlify_fn),
        ("b2a_hex", make_builtin(|args: &[PyObjectRef]| {
            let data = extract_bytes(&args[0])?;
            Ok(PyObject::bytes(data.iter().map(|b| format!("{:02x}", b)).collect::<String>().into_bytes()))
        })),
        ("unhexlify", unhexlify_fn),
        ("a2b_hex", make_builtin(|args: &[PyObjectRef]| {
            let hex_str = match &args[0].payload {
                PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
                _ => args[0].py_to_string(),
            };
            let hex_str = hex_str.trim();
            if hex_str.len() % 2 != 0 { return Err(PyException::value_error("Odd-length string")); }
            let mut result = Vec::with_capacity(hex_str.len() / 2);
            for i in (0..hex_str.len()).step_by(2) {
                result.push(u8::from_str_radix(&hex_str[i..i+2], 16)
                    .map_err(|_| PyException::value_error("Non-hexadecimal digit found"))?);
            }
            Ok(PyObject::bytes(result))
        })),
        ("crc32", crc32_fn),
        ("b2a_base64", b2a_base64_fn),
        ("a2b_base64", a2b_base64_fn),
    ])
}

// ── codecs module ──────────────────────────────────────────────────
pub fn create_codecs_module() -> PyObjectRef {
    make_module("codecs", vec![
        ("encode", make_builtin(codecs_encode)),
        ("decode", make_builtin(codecs_decode)),
        ("lookup", make_builtin(codecs_lookup)),
        ("getencoder", make_builtin(codecs_getencoder)),
        ("getdecoder", make_builtin(codecs_getdecoder)),
        ("utf_8_encode", make_builtin(codecs_utf8_encode)),
        ("utf_8_decode", make_builtin(codecs_utf8_decode)),
    ])
}

fn codecs_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.encode", args, 1)?;
    let s = args[0].py_to_string();
    let encoding = if args.len() > 1 { args[1].py_to_string() } else { "utf-8".to_string() };
    match encoding.to_lowercase().replace('-', "_").as_str() {
        "utf_8" | "utf8" => Ok(PyObject::bytes(s.as_bytes().to_vec())),
        "ascii" => {
            let bytes: Vec<u8> = s.chars().filter_map(|c| if c.is_ascii() { Some(c as u8) } else { None }).collect();
            Ok(PyObject::bytes(bytes))
        }
        "latin_1" | "latin1" | "iso_8859_1" => {
            let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
            Ok(PyObject::bytes(bytes))
        }
        _ => Err(PyException::value_error(format!("unknown encoding: {}", encoding))),
    }
}

fn codecs_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.decode", args, 1)?;
    let bytes = extract_bytes(&args[0])?;
    let encoding = if args.len() > 1 { args[1].py_to_string() } else { "utf-8".to_string() };
    match encoding.to_lowercase().replace('-', "_").as_str() {
        "utf_8" | "utf8" => {
            let s = String::from_utf8(bytes).map_err(|_| PyException::value_error("invalid utf-8"))?;
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "ascii" => {
            let s: String = bytes.iter().map(|&b| b as char).collect();
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "latin_1" | "latin1" | "iso_8859_1" => {
            let s: String = bytes.iter().map(|&b| b as char).collect();
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        _ => Err(PyException::value_error(format!("unknown encoding: {}", encoding))),
    }
}

fn codecs_lookup(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.lookup", args, 1)?;
    let encoding = args[0].py_to_string().to_lowercase().replace('-', "_");
    match encoding.as_str() {
        "utf_8" | "utf8" | "ascii" | "latin_1" | "latin1" | "iso_8859_1" => {
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(&encoding)),
                PyObject::none(), // encode
                PyObject::none(), // decode
                PyObject::none(), // stream reader
            ]))
        }
        _ => Err(PyException::value_error(format!("unknown encoding: {}", encoding))),
    }
}

fn codecs_getencoder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.getencoder", args, 1)?;
    Ok(make_builtin(codecs_encode))
}

fn codecs_getdecoder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.getdecoder", args, 1)?;
    Ok(make_builtin(codecs_decode))
}

fn codecs_utf8_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.utf_8_encode", args, 1)?;
    let s = args[0].py_to_string();
    let b = s.as_bytes().to_vec();
    let len = b.len() as i64;
    Ok(PyObject::tuple(vec![PyObject::bytes(b), PyObject::int(len)]))
}

fn codecs_utf8_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.utf_8_decode", args, 1)?;
    let bytes = extract_bytes(&args[0])?;
    let s = String::from_utf8(bytes.clone()).map_err(|_| PyException::value_error("invalid utf-8"))?;
    let len = bytes.len() as i64;
    Ok(PyObject::tuple(vec![PyObject::str_val(CompactString::from(s)), PyObject::int(len)]))
}

// ── shelve module ──

pub fn create_shelve_module() -> PyObjectRef {
    let open_fn = make_builtin(|args: &[PyObjectRef]| {
        let _filename = if !args.is_empty() { args[0].py_to_string() } else { "shelf.db".to_string() };
        let cls = PyObject::class(CompactString::from("Shelf"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let data: Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>> = Arc::new(RwLock::new(IndexMap::new()));

            let d1 = data.clone();
            w.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                "Shelf.__getitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__getitem__", args, 1)?;
                    let key = HashableKey::Str(CompactString::from(args[0].py_to_string().as_str()));
                    d1.read().get(&key).cloned().ok_or_else(|| PyException::key_error(args[0].py_to_string()))
                }
            ));

            let d2 = data.clone();
            w.insert(CompactString::from("__setitem__"), PyObject::native_closure(
                "Shelf.__setitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__setitem__", args, 2)?;
                    let key = HashableKey::Str(CompactString::from(args[0].py_to_string().as_str()));
                    d2.write().insert(key, args[1].clone());
                    Ok(PyObject::none())
                }
            ));

            let d3 = data.clone();
            w.insert(CompactString::from("__contains__"), PyObject::native_closure(
                "Shelf.__contains__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__contains__", args, 1)?;
                    let key = HashableKey::Str(CompactString::from(args[0].py_to_string().as_str()));
                    Ok(PyObject::bool_val(d3.read().contains_key(&key)))
                }
            ));

            let d4 = data.clone();
            w.insert(CompactString::from("keys"), PyObject::native_closure(
                "Shelf.keys", move |_: &[PyObjectRef]| {
                    let keys: Vec<PyObjectRef> = d4.read().keys().map(|k| match k {
                        HashableKey::Str(s) => PyObject::str_val(s.clone()),
                        _ => PyObject::none(),
                    }).collect();
                    Ok(PyObject::list(keys))
                }
            ));

            w.insert(CompactString::from("close"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("sync"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));

            let ir = inst.clone();
            w.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "Shelf.__enter__", move |_: &[PyObjectRef]| Ok(ir.clone())
            ));
            w.insert(CompactString::from("__exit__"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))));
        }
        Ok(inst)
    });

    make_module("shelve", vec![
        ("open", open_fn),
    ])
}
