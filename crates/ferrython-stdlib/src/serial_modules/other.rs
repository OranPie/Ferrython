use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    FxHashKeyMap, new_fx_hashkey_map,PyCell, 
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::sync::Arc;
use std::rc::Rc;

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
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok((**b).clone()),
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
        ("pack_into", make_builtin(struct_pack_into)),
        ("unpack_from", make_builtin(struct_unpack_from)),
        ("iter_unpack", make_builtin(struct_iter_unpack)),
        ("calcsize", make_builtin(struct_calcsize)),
        ("Struct", make_builtin(struct_struct_ctor)),
        ("error", PyObject::class(CompactString::from("error"), vec![], IndexMap::new())),
    ])
}

fn struct_struct_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Struct() requires a format string"));
    }
    let fmt_str = args[0].py_to_string();
    let cls = PyObject::class(CompactString::from("Struct"), vec![], IndexMap::new());
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
        let fmt_for_pi = fmt_str.clone();
        w.insert(CompactString::from("pack_into"), PyObject::native_closure("pack_into", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("Struct.pack_into() requires buffer, offset, and values"));
            }
            let mut full_args = vec![PyObject::str_val(CompactString::from(&fmt_for_pi))];
            full_args.extend_from_slice(args);
            struct_pack_into(&full_args)
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
        'e' | 'h' | 'H' => 2,
        'i' | 'I' | 'l' | 'L' | 'f' => 4,
        'q' | 'Q' | 'd' => 8,
        'n' | 'N' | 'P' => std::mem::size_of::<usize>(),
        's' | 'p' => 1,
        _ => 0,
    }
}

/// Convert f32 to IEEE 754 half-precision (16-bit)
fn f32_to_f16(val: f32) -> u16 {
    let bits = val.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let frac = bits & 0x007FFFFF;
    if exp == 255 {
        // Inf or NaN
        sign | 0x7C00 | if frac != 0 { 0x0200 } else { 0 }
    } else if exp > 142 {
        sign | 0x7C00 // overflow to inf
    } else if exp < 113 {
        sign // underflow to zero
    } else {
        let new_exp = ((exp - 127 + 15) as u16) << 10;
        let new_frac = (frac >> 13) as u16;
        sign | new_exp | new_frac
    }
}

/// Convert IEEE 754 half-precision (16-bit) to f32
fn f16_to_f32(half: u16) -> f32 {
    let sign = ((half & 0x8000) as u32) << 16;
    let exp = ((half >> 10) & 0x1F) as u32;
    let frac = (half & 0x03FF) as u32;
    if exp == 0 {
        if frac == 0 { return f32::from_bits(sign); }
        // Subnormal
        let mut e = 1u32;
        let mut f = frac;
        while f & 0x0400 == 0 { f <<= 1; e += 1; }
        f &= 0x03FF;
        f32::from_bits(sign | ((127 - 15 + 1 - e) << 23) | (f << 13))
    } else if exp == 31 {
        f32::from_bits(sign | 0x7F800000 | if frac != 0 { 0x00400000 } else { 0 })
    } else {
        f32::from_bits(sign | ((exp + 112) << 23) | (frac << 13))
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
                PyObjectPayload::Bytes(b) => (**b).clone(),
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
        'c' => {
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let b = match &args[*arg_idx].payload {
                    PyObjectPayload::Bytes(v) if v.len() == 1 => v[0],
                    _ => return Err(PyException::type_error("char format requires a bytes object of length 1")),
                };
                result.push(b);
                *arg_idx += 1;
            }
        }
        'p' => {
            // Pascal string: first byte is length, then data, padded to `count` bytes total
            if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
            let src = match &args[*arg_idx].payload {
                PyObjectPayload::Bytes(b) => (**b).clone(),
                _ => args[*arg_idx].py_to_string().into_bytes(),
            };
            let max_len = if count > 0 { count - 1 } else { 0 };
            let actual = src.len().min(max_len).min(255);
            result.push(actual as u8);
            for i in 0..max_len {
                result.push(if i < actual { src[i] } else { 0 });
            }
            *arg_idx += 1;
        }
        'e' => {
            // IEEE 754 half-precision float (16-bit)
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[*arg_idx].to_float()? as f32;
                let half = f32_to_f16(val);
                let bytes = if little_endian { half.to_le_bytes() } else { half.to_be_bytes() };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'n' | 'N' | 'P' => {
            for _ in 0..count {
                if *arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[*arg_idx].to_int()? as usize;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        _ => {}
    }
    Ok(())
}

fn struct_unpack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("unpack requires format string and bytes")); }
    let fmt = args[0].py_to_string();
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        _ => return Err(PyException::type_error("unpack requires bytes argument")),
    };
    // Validate buffer length
    let expected_size = struct_calcsize(&[args[0].clone()])?.as_int().unwrap_or(0) as usize;
    if data.len() < expected_size {
        return Err(PyException::runtime_error(format!(
            "unpack requires a buffer of at least {} bytes (got {})", expected_size, data.len()
        )));
    }
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

/// struct.pack_into(fmt, buffer, offset, v1, v2, ...)
fn struct_pack_into(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("pack_into requires format, buffer, offset, and values")); }
    let offset = args[2].as_int().unwrap_or(0) as usize;
    // Pack using the same format and values
    let mut pack_args = vec![args[0].clone()];
    pack_args.extend_from_slice(&args[3..]);
    let packed = struct_pack(&pack_args)?;
    let packed_bytes = match &packed.payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        _ => return Err(PyException::runtime_error("pack returned non-bytes")),
    };
    // Write into the buffer
    match &args[1].payload {
        PyObjectPayload::ByteArray(buf) => {
            if offset + packed_bytes.len() > buf.len() {
                return Err(PyException::runtime_error("pack_into: offset + size exceeds buffer"));
            }
            let ptr = buf.as_ptr() as *mut u8;
            unsafe {
                std::ptr::copy_nonoverlapping(packed_bytes.as_ptr(), ptr.add(offset), packed_bytes.len());
            }
            Ok(PyObject::none())
        }
        PyObjectPayload::Bytes(buf) => {
            if offset + packed_bytes.len() > buf.len() {
                return Err(PyException::runtime_error("pack_into: offset + size exceeds buffer"));
            }
            let ptr = buf.as_ptr() as *mut u8;
            unsafe {
                std::ptr::copy_nonoverlapping(packed_bytes.as_ptr(), ptr.add(offset), packed_bytes.len());
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::type_error("pack_into requires a writable buffer (bytearray)"))
    }
}

/// struct.unpack_from(fmt, buffer, offset=0)
fn struct_unpack_from(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("unpack_from requires format and buffer")); }
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        PyObjectPayload::ByteArray(b) => (**b).clone(),
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
        PyObjectPayload::Bytes(b) => (**b).clone(),
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
        'c' => {
            for _ in 0..count {
                if *offset >= data.len() { break; }
                result.push(PyObject::bytes(vec![data[*offset]]));
                *offset += 1;
            }
        }
        'p' => {
            // Pascal string: first byte is length
            if *offset >= data.len() { return; }
            let str_len = data[*offset] as usize;
            *offset += 1;
            let available = count.saturating_sub(1);
            let actual = str_len.min(available);
            if *offset + available > data.len() { return; }
            result.push(PyObject::bytes(data[*offset..*offset + actual].to_vec()));
            *offset += available;
        }
        'e' => {
            for _ in 0..count {
                if *offset + 2 > data.len() { break; }
                let bytes: [u8; 2] = [data[*offset], data[*offset + 1]];
                let half = if little_endian { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) };
                result.push(PyObject::float(f16_to_f32(half) as f64));
                *offset += 2;
            }
        }
        'n' => {
            for _ in 0..count {
                let sz = std::mem::size_of::<isize>();
                if *offset + sz > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes[..sz].copy_from_slice(&data[*offset..*offset + sz]);
                let val = if little_endian { isize::from_le_bytes(bytes[..sz].try_into().unwrap_or([0; 8])) } else { isize::from_be_bytes(bytes[..sz].try_into().unwrap_or([0; 8])) };
                result.push(PyObject::int(val as i64));
                *offset += sz;
            }
        }
        'N' | 'P' => {
            for _ in 0..count {
                let sz = std::mem::size_of::<usize>();
                if *offset + sz > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes[..sz].copy_from_slice(&data[*offset..*offset + sz]);
                let val = if little_endian { usize::from_le_bytes(bytes[..sz].try_into().unwrap_or([0; 8])) } else { usize::from_be_bytes(bytes[..sz].try_into().unwrap_or([0; 8])) };
                result.push(PyObject::int(val as i64));
                *offset += sz;
            }
        }
        _ => {}
    }
}

// ── pickle module (CPython-compatible protocol 0 & 2) ──

// ── Helpers ──

fn hashable_key_to_pyobj(k: &HashableKey) -> PyObjectRef {
    match k {
        HashableKey::Str(s) => PyObject::str_val(s.as_ref().clone()),
        HashableKey::Int(n) => PyObject::int(n.to_i64().unwrap_or(0)),
        HashableKey::Float(f) => PyObject::float(f.0),
        HashableKey::Bool(b) => PyObject::bool_val(*b),
        _ => PyObject::str_val(CompactString::from(format!("{:?}", k))),
    }
}

fn format_float_repr(f: f64) -> String {
    if f.is_nan() { return "nan".to_string(); }
    if f.is_infinite() {
        return if f > 0.0 { "inf".to_string() } else { "-inf".to_string() };
    }
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains('E') { s } else { format!("{}.0", s) }
}

// ── Protocol 0 (text) serialization ──

fn p0_escape_unicode(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\0' => out.extend_from_slice(b"\\x00"),
            c if c.is_ascii() => out.push(c as u8),
            c if (c as u32) <= 0xff => {
                out.extend_from_slice(format!("\\x{:02x}", c as u32).as_bytes());
            }
            c if (c as u32) <= 0xffff => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                out.extend_from_slice(format!("\\U{:08x}", c as u32).as_bytes());
            }
        }
    }
    out
}

fn p0_escape_bytes(b: &[u8]) -> Vec<u8> {
    let mut out = vec![b'\''];
    for &byte in b {
        match byte {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\'' => out.extend_from_slice(b"\\'"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x20..=0x7e => out.push(byte),
            _ => out.extend_from_slice(format!("\\x{:02x}", byte).as_bytes()),
        }
    }
    out.push(b'\'');
    out
}

fn pickle_serialize_p0(obj: &PyObjectRef, buf: &mut Vec<u8>, memo: &mut u32) -> PyResult<()> {
    match &obj.payload {
        PyObjectPayload::None => buf.push(b'N'),
        PyObjectPayload::Bool(b) => {
            buf.extend_from_slice(if *b { b"I01\n" } else { b"I00\n" });
        }
        PyObjectPayload::Int(n) => {
            buf.extend_from_slice(format!("I{}\n", n).as_bytes());
        }
        PyObjectPayload::Float(f) => {
            buf.extend_from_slice(format!("F{}\n", format_float_repr(*f)).as_bytes());
        }
        PyObjectPayload::Str(s) => {
            buf.push(b'V');
            buf.extend_from_slice(&p0_escape_unicode(s));
            buf.push(b'\n');
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            buf.push(b'S');
            buf.extend_from_slice(&p0_escape_bytes(b));
            buf.push(b'\n');
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            buf.extend_from_slice(b"(lp");
            buf.extend_from_slice(format!("{}\n", *memo).as_bytes());
            *memo += 1;
            for item in items.iter() {
                pickle_serialize_p0(item, buf, memo)?;
                buf.push(b'a');
            }
        }
        PyObjectPayload::Tuple(items) => {
            buf.push(b'(');
            for item in items.iter() {
                pickle_serialize_p0(item, buf, memo)?;
            }
            buf.push(b't');
        }
        PyObjectPayload::Dict(map) => {
            let map = map.read();
            buf.extend_from_slice(b"(dp");
            buf.extend_from_slice(format!("{}\n", *memo).as_bytes());
            *memo += 1;
            for (k, v) in map.iter() {
                pickle_serialize_p0(&hashable_key_to_pyobj(k), buf, memo)?;
                pickle_serialize_p0(v, buf, memo)?;
                buf.push(b's');
            }
        }
        PyObjectPayload::Set(items) => {
            buf.extend_from_slice(b"c__builtin__\nset\n(");
            let items_r = items.read();
            let list_items: Vec<PyObjectRef> = items_r.values().cloned().collect();
            pickle_serialize_p0(&PyObject::list(list_items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::FrozenSet(items) => {
            buf.extend_from_slice(b"c__builtin__\nfrozenset\n(");
            let list_items: Vec<PyObjectRef> = items.values().cloned().collect();
            pickle_serialize_p0(&PyObject::list(list_items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::Instance(inst) => {
            let (class_name, data_pairs) = pickle_extract_instance(obj, inst)?;
            // Serialize as dict via GLOBAL + REDUCE pattern:
            // c__main__\nClassName\n( {state_dict} tR
            buf.extend_from_slice(b"c__main__\n");
            buf.extend_from_slice(class_name.as_bytes());
            buf.push(b'\n');
            buf.push(b'(');
            // Build state dict
            buf.extend_from_slice(b"(d");
            let id = *memo; *memo += 1;
            buf.extend_from_slice(format!("p{}\n", id).as_bytes());
            for (k, v) in &data_pairs {
                pickle_serialize_p0(&PyObject::str_val(k.clone()), buf, memo)?;
                pickle_serialize_p0(v, buf, memo)?;
                buf.push(b's');
            }
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::ExceptionInstance(ei) => {
            // Serialize exceptions as: cbuiltins\nExceptionType\n(args)tR
            let type_name = format!("{}", ei.kind);
            buf.extend_from_slice(b"cbuiltins\n");
            buf.extend_from_slice(type_name.as_bytes());
            buf.push(b'\n');
            buf.push(b'(');
            if ei.args.is_empty() {
                // Use the message as the sole arg
                pickle_serialize_p0(&PyObject::str_val(CompactString::from(ei.message.as_str())), buf, memo)?;
            } else {
                for arg in &ei.args {
                    pickle_serialize_p0(arg, buf, memo)?;
                }
            }
            buf.extend_from_slice(b"tR");
        }
        _ => {
            return Err(PyException::runtime_error(
                format!("PicklingError: can't pickle object of type {}", obj.type_name()),
            ));
        }
    }
    Ok(())
}

fn pickle_extract_instance(
    obj: &PyObjectRef,
    inst: &ferrython_core::object::InstanceData,
) -> PyResult<(String, Vec<(CompactString, PyObjectRef)>)> {
    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
        cd.name.to_string()
    } else {
        "object".to_string()
    };
    let state_dict = if let Some(getstate) = obj.get_attr("__getstate__") {
        match &getstate.payload {
            PyObjectPayload::NativeFunction(nf) => (nf.func)(&[obj.clone()]).ok(),
            PyObjectPayload::NativeClosure(nc) => (nc.func)(&[obj.clone()]).ok(),
            _ => None,
        }
    } else {
        None
    };
    let mut data_pairs: Vec<(CompactString, PyObjectRef)> = Vec::new();
    if let Some(state) = &state_dict {
        if let PyObjectPayload::Dict(map) = &state.payload {
            for (k, v) in map.read().iter() {
                if let HashableKey::Str(name) = k {
                    data_pairs.push((name.as_ref().clone(), v.clone()));
                }
            }
        }
    } else {
        let attrs_r = inst.attrs.read();
        for (k, v) in attrs_r.iter() {
            match &v.payload {
                PyObjectPayload::NativeFunction(_)
                | PyObjectPayload::NativeClosure(_)
                | PyObjectPayload::Function(_)
                | PyObjectPayload::Class(_) => continue,
                _ => data_pairs.push((k.clone(), v.clone())),
            }
        }
    }
    Ok((class_name, data_pairs))
}

// ── Protocol 2 (binary) serialization ──

fn p2_emit_put(buf: &mut Vec<u8>, memo: &mut u32) {
    if *memo <= 0xff {
        buf.push(b'q');
        buf.push(*memo as u8);
    }
    *memo += 1;
}

fn pickle_serialize_p2(obj: &PyObjectRef, buf: &mut Vec<u8>, memo: &mut u32) -> PyResult<()> {
    match &obj.payload {
        PyObjectPayload::None => buf.push(b'N'),
        PyObjectPayload::Bool(b) => buf.push(if *b { 0x88 } else { 0x89 }),
        PyObjectPayload::Int(n) => {
            if let Some(val) = n.to_i64() {
                if val >= 0 && val <= 0xff {
                    buf.push(b'K');
                    buf.push(val as u8);
                } else if val >= 0 && val <= 0xffff {
                    buf.push(b'M');
                    buf.extend_from_slice(&(val as u16).to_le_bytes());
                } else if val >= i32::MIN as i64 && val <= i32::MAX as i64 {
                    buf.push(b'J');
                    buf.extend_from_slice(&(val as i32).to_le_bytes());
                } else {
                    buf.extend_from_slice(format!("I{}\n", n).as_bytes());
                }
            } else {
                buf.extend_from_slice(format!("I{}\n", n).as_bytes());
            }
        }
        PyObjectPayload::Float(f) => {
            buf.push(b'G');
            buf.extend_from_slice(&f.to_be_bytes());
        }
        PyObjectPayload::Str(s) => {
            let bytes = s.as_bytes();
            buf.push(b'X');
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            buf.push(b'B');
            buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
            buf.extend_from_slice(b);
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            buf.push(b']');
            p2_emit_put(buf, memo);
            if !items.is_empty() {
                buf.push(b'(');
                for item in items.iter() {
                    pickle_serialize_p2(item, buf, memo)?;
                }
                buf.push(b'e');
            }
        }
        PyObjectPayload::Tuple(items) => {
            match items.len() {
                0 => buf.push(b')'),
                1 => {
                    pickle_serialize_p2(&items[0], buf, memo)?;
                    buf.push(0x85);
                }
                2 => {
                    pickle_serialize_p2(&items[0], buf, memo)?;
                    pickle_serialize_p2(&items[1], buf, memo)?;
                    buf.push(0x86);
                }
                3 => {
                    pickle_serialize_p2(&items[0], buf, memo)?;
                    pickle_serialize_p2(&items[1], buf, memo)?;
                    pickle_serialize_p2(&items[2], buf, memo)?;
                    buf.push(0x87);
                }
                _ => {
                    buf.push(b'(');
                    for item in items.iter() {
                        pickle_serialize_p2(item, buf, memo)?;
                    }
                    buf.push(b't');
                }
            }
        }
        PyObjectPayload::Dict(map) => {
            let map = map.read();
            buf.push(b'}');
            p2_emit_put(buf, memo);
            if !map.is_empty() {
                buf.push(b'(');
                for (k, v) in map.iter() {
                    pickle_serialize_p2(&hashable_key_to_pyobj(k), buf, memo)?;
                    pickle_serialize_p2(v, buf, memo)?;
                }
                buf.push(b'u');
            }
        }
        PyObjectPayload::Set(items) => {
            buf.extend_from_slice(b"c__builtin__\nset\n(");
            let items_r = items.read();
            let list_items: Vec<PyObjectRef> = items_r.values().cloned().collect();
            pickle_serialize_p2(&PyObject::list(list_items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::FrozenSet(items) => {
            buf.extend_from_slice(b"c__builtin__\nfrozenset\n(");
            let list_items: Vec<PyObjectRef> = items.values().cloned().collect();
            pickle_serialize_p2(&PyObject::list(list_items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::Instance(inst) => {
            let (class_name, data_pairs) = pickle_extract_instance(obj, inst)?;
            // c__main__\nClassName\n( {state_dict} t R
            buf.extend_from_slice(b"c__main__\n");
            buf.extend_from_slice(class_name.as_bytes());
            buf.push(b'\n');
            buf.push(b'(');
            // Build state dict
            buf.push(b'}');
            p2_emit_put(buf, memo);
            if !data_pairs.is_empty() {
                buf.push(b'(');
                for (k, v) in &data_pairs {
                    pickle_serialize_p2(&PyObject::str_val(k.clone()), buf, memo)?;
                    pickle_serialize_p2(v, buf, memo)?;
                }
                buf.push(b'u');
            }
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::ExceptionInstance(ei) => {
            let type_name = format!("{}", ei.kind);
            buf.extend_from_slice(b"cbuiltins\n");
            buf.extend_from_slice(type_name.as_bytes());
            buf.push(b'\n');
            buf.push(b'(');
            if ei.args.is_empty() {
                pickle_serialize_p2(&PyObject::str_val(CompactString::from(ei.message.as_str())), buf, memo)?;
            } else {
                for arg in &ei.args {
                    pickle_serialize_p2(arg, buf, memo)?;
                }
            }
            buf.extend_from_slice(b"tR");
        }
        _ => {
            return Err(PyException::runtime_error(
                format!("PicklingError: can't pickle object of type {}", obj.type_name()),
            ));
        }
    }
    Ok(())
}

// ── Unified serializer (protocol 2 by default, used by shelve) ──

fn pickle_serialize(obj: &PyObjectRef, buf: &mut Vec<u8>) -> PyResult<()> {
    buf.extend_from_slice(b"\x80\x02");
    let mut memo: u32 = 0;
    pickle_serialize_p2(obj, buf, &mut memo)?;
    buf.push(b'.');
    Ok(())
}

// ── Protocol 0 (text) deserialization ──

#[derive(Clone)]
enum PklStackItem {
    Value(PyObjectRef),
    Mark,
    Global(String, String),
}

fn p0_read_line<'a>(data: &'a [u8], pos: &mut usize) -> &'a [u8] {
    let start = *pos;
    while *pos < data.len() && data[*pos] != b'\n' {
        *pos += 1;
    }
    let line = &data[start..*pos];
    if *pos < data.len() { *pos += 1; }
    line
}

fn p0_unescape_unicode(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' { result.push(c); continue; }
        match chars.next() {
            Some('\\') => result.push('\\'),
            Some('n') => result.push('\n'),
            Some('r') => result.push('\r'),
            Some('t') => result.push('\t'),
            Some('0') => result.push('\0'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) { result.push(ch); }
                }
            }
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) { result.push(ch); }
                }
            }
            Some('U') => {
                let hex: String = chars.by_ref().take(8).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) { result.push(ch); }
                }
            }
            Some(other) => { result.push('\\'); result.push(other); }
            None => result.push('\\'),
        }
    }
    result
}

fn p0_unescape_bytes(raw: &[u8]) -> Vec<u8> {
    if raw.len() < 2 { return Vec::new(); }
    let quote = raw[0];
    if quote != b'\'' && quote != b'"' { return raw.to_vec(); }
    let end = if raw.last() == Some(&quote) { raw.len() - 1 } else { raw.len() };
    let inner = &raw[1..end];
    let mut result = Vec::new();
    let mut i = 0;
    while i < inner.len() {
        if inner[i] == b'\\' && i + 1 < inner.len() {
            match inner[i + 1] {
                b'\\' => { result.push(b'\\'); i += 2; }
                b'\'' => { result.push(b'\''); i += 2; }
                b'"' => { result.push(b'"'); i += 2; }
                b'n' => { result.push(b'\n'); i += 2; }
                b'r' => { result.push(b'\r'); i += 2; }
                b't' => { result.push(b'\t'); i += 2; }
                b'x' if i + 3 < inner.len() => {
                    if let Ok(v) = u8::from_str_radix(
                        std::str::from_utf8(&inner[i+2..i+4]).unwrap_or("00"), 16
                    ) {
                        result.push(v);
                    }
                    i += 4;
                }
                _ => { result.push(inner[i]); result.push(inner[i+1]); i += 2; }
            }
        } else {
            result.push(inner[i]);
            i += 1;
        }
    }
    result
}

fn pkl_pop_to_mark(stack: &mut Vec<PklStackItem>) -> PyResult<Vec<PyObjectRef>> {
    let mut items = Vec::new();
    loop {
        match stack.pop() {
            Some(PklStackItem::Mark) => break,
            Some(PklStackItem::Value(v)) => items.push(v),
            Some(PklStackItem::Global(..)) => {
                return Err(PyException::runtime_error("UnpicklingError: unexpected global on stack"));
            }
            None => return Err(PyException::runtime_error("UnpicklingError: MARK not found on stack")),
        }
    }
    items.reverse();
    Ok(items)
}

fn pkl_stack_top_value(stack: &[PklStackItem]) -> PyResult<PyObjectRef> {
    match stack.last() {
        Some(PklStackItem::Value(v)) => Ok(v.clone()),
        _ => Err(PyException::runtime_error("UnpicklingError: expected value on stack top")),
    }
}

fn pkl_reduce(callable: &PklStackItem, args: &PyObjectRef) -> PyResult<PyObjectRef> {
    if let PklStackItem::Global(module, name) = callable {
        let arg_list = match &args.payload {
            PyObjectPayload::Tuple(items) => (**items).clone(),
            _ => vec![args.clone()],
        };
        match (module.as_str(), name.as_str()) {
            ("__builtin__" | "builtins", "set") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::List(items) = &first.payload {
                        let items = items.read();
                        let mut map = IndexMap::new();
                        for item in items.iter() {
                            if let Ok(hk) = HashableKey::from_object(item) {
                                map.insert(hk, item.clone());
                            }
                        }
                        return Ok(PyObject::set(map));
                    }
                }
                Ok(PyObject::set(IndexMap::new()))
            }
            ("__builtin__" | "builtins", "frozenset") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::List(items) = &first.payload {
                        let items = items.read();
                        let mut map = IndexMap::new();
                        for item in items.iter() {
                            if let Ok(hk) = HashableKey::from_object(item) {
                                map.insert(hk, item.clone());
                            }
                        }
                        return Ok(PyObject::frozenset(map));
                    }
                }
                Ok(PyObject::frozenset(IndexMap::new()))
            }
            _ => {
                // For __main__.ClassName — reconstruct as Instance with dict state
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::Dict(map) = &first.payload {
                        let map_r = map.read();
                        let mut attrs = IndexMap::new();
                        for (k, v) in map_r.iter() {
                            if let HashableKey::Str(s) = k {
                                attrs.insert(s.as_ref().clone(), v.clone());
                            }
                        }
                        let cls = PyObject::class(CompactString::from(name.as_str()), vec![], IndexMap::new());
                        return Ok(PyObject::instance_with_attrs(cls, attrs));
                    }
                }
                Err(PyException::runtime_error(
                    format!("UnpicklingError: unsupported global {}.{}", module, name),
                ))
            }
        }
    } else {
        Err(PyException::runtime_error("UnpicklingError: REDUCE requires a callable"))
    }
}

fn pickle_loads_p0(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos: usize = 0;
    let mut stack: Vec<PklStackItem> = Vec::new();
    let mut memo: std::collections::HashMap<u32, PyObjectRef> = std::collections::HashMap::new();

    while pos < data.len() {
        let opcode = data[pos];
        pos += 1;
        match opcode {
            b'.' => break, // STOP
            b'N' => stack.push(PklStackItem::Value(PyObject::none())),
            b'I' => {
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid INT encoding"))?
                    .trim();
                if s == "01" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(true)));
                } else if s == "00" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(false)));
                } else {
                    let val: i64 = s.parse()
                        .map_err(|_| PyException::runtime_error(
                            format!("UnpicklingError: invalid INT value '{}'", s),
                        ))?;
                    stack.push(PklStackItem::Value(PyObject::int(val)));
                }
            }
            b'L' => {
                // LONG — like I but for big ints, trailing L
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid LONG encoding"))?
                    .trim().trim_end_matches('L');
                let val: i64 = s.parse().unwrap_or(0);
                stack.push(PklStackItem::Value(PyObject::int(val)));
            }
            b'F' => {
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid FLOAT encoding"))?
                    .trim();
                let val: f64 = match s {
                    "nan" | "NaN" => f64::NAN,
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => s.parse().map_err(|_| PyException::runtime_error(
                        format!("UnpicklingError: invalid FLOAT value '{}'", s),
                    ))?,
                };
                stack.push(PklStackItem::Value(PyObject::float(val)));
            }
            b'V' => {
                // UNICODE — read raw-unicode-escape line
                let line = p0_read_line(data, &mut pos);
                let s = p0_unescape_unicode(line);
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(s))));
            }
            b'S' => {
                // STRING — read quoted string line (bytes)
                let line = p0_read_line(data, &mut pos);
                let bytes = p0_unescape_bytes(line);
                stack.push(PklStackItem::Value(PyObject::bytes(bytes)));
            }
            b'(' => stack.push(PklStackItem::Mark),
            b'l' => {
                // LIST — pop to mark, build list
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::list(items)));
            }
            b't' => {
                // TUPLE — pop to mark, build tuple
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::tuple(items)));
            }
            b'd' => {
                // DICT — pop to mark, build dict from pairs
                let items = pkl_pop_to_mark(&mut stack)?;
                let mut pairs = Vec::new();
                for chunk in items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PklStackItem::Value(PyObject::dict_from_pairs(pairs)));
            }
            b'a' => {
                // APPEND — pop item, append to list on stack
                let item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: APPEND expects value")),
                };
                // Find the list on top of the remaining stack
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().push(item);
                    }
                }
            }
            b's' => {
                // SETITEM — pop value, pop key, set on dict
                let val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: SETITEM expects value")),
                };
                let key = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: SETITEM expects key")),
                };
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        if let Ok(hk) = HashableKey::from_object(&key) {
                            dict_map.write().insert(hk, val);
                        }
                    }
                }
            }
            b'p' => {
                // PUT — memoize top of stack
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line).unwrap_or("0").trim().parse().unwrap_or(0);
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'g' => {
                // GET — recall from memo
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line).unwrap_or("0").trim().parse().unwrap_or(0);
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!("UnpicklingError: memo key {} not found", id))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'c' => {
                // GLOBAL — read module\nqualname\n
                let mod_line = p0_read_line(data, &mut pos);
                let name_line = p0_read_line(data, &mut pos);
                let module = String::from_utf8_lossy(mod_line).to_string();
                let name = String::from_utf8_lossy(name_line).to_string();
                stack.push(PklStackItem::Global(module, name));
            }
            b'R' => {
                // REDUCE — pop args tuple, pop callable, call
                let args_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: REDUCE expects args")),
                };
                let callable = match stack.pop() {
                    Some(item) => item,
                    None => return Err(PyException::runtime_error("UnpicklingError: REDUCE expects callable")),
                };
                let result = pkl_reduce(&callable, &args_item)?;
                stack.push(PklStackItem::Value(result));
            }
            b'\n' | b'\r' | b' ' => {} // skip whitespace
            _ => {
                return Err(PyException::runtime_error(
                    format!("UnpicklingError: unknown protocol 0 opcode 0x{:02x} ('{}')",
                        opcode, if opcode.is_ascii_graphic() { opcode as char } else { '?' }),
                ));
            }
        }
    }

    // Return top of stack
    for item in stack.iter().rev() {
        if let PklStackItem::Value(v) = item {
            return Ok(v.clone());
        }
    }
    Err(PyException::runtime_error("UnpicklingError: empty pickle data"))
}

// ── Protocol 2 (binary) deserialization ──

fn pickle_loads_p2(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos: usize = 0;
    let mut stack: Vec<PklStackItem> = Vec::new();
    let mut memo: std::collections::HashMap<u32, PyObjectRef> = std::collections::HashMap::new();

    // Skip protocol header
    if pos + 1 < data.len() && data[pos] == 0x80 {
        pos += 2;
    }

    while pos < data.len() {
        let opcode = data[pos];
        pos += 1;
        match opcode {
            b'.' => break, // STOP
            b'N' => stack.push(PklStackItem::Value(PyObject::none())),
            0x88 => stack.push(PklStackItem::Value(PyObject::bool_val(true))),
            0x89 => stack.push(PklStackItem::Value(PyObject::bool_val(false))),
            b'K' => {
                // BININT1 — 1-byte unsigned int
                if pos >= data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BININT1"));
                }
                stack.push(PklStackItem::Value(PyObject::int(data[pos] as i64)));
                pos += 1;
            }
            b'M' => {
                // BININT2 — 2-byte LE unsigned short
                if pos + 2 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BININT2"));
                }
                let val = u16::from_le_bytes([data[pos], data[pos + 1]]) as i64;
                stack.push(PklStackItem::Value(PyObject::int(val)));
                pos += 2;
            }
            b'J' => {
                // BININT — 4-byte LE signed int
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BININT"));
                }
                let val = i32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as i64;
                stack.push(PklStackItem::Value(PyObject::int(val)));
                pos += 4;
            }
            b'I' => {
                // INT (text fallback) — read to newline
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line).unwrap_or("0").trim();
                if s == "01" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(true)));
                } else if s == "00" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(false)));
                } else {
                    let val: i64 = s.parse().unwrap_or(0);
                    stack.push(PklStackItem::Value(PyObject::int(val)));
                }
            }
            b'G' => {
                // BINFLOAT — 8-byte BE IEEE 754 double
                if pos + 8 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BINFLOAT"));
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[pos..pos + 8]);
                let val = f64::from_be_bytes(bytes);
                stack.push(PklStackItem::Value(PyObject::float(val)));
                pos += 8;
            }
            b'X' => {
                // BINUNICODE — 4-byte LE len + UTF-8
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BINUNICODE length"));
                }
                let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BINUNICODE data"));
                }
                let s = std::str::from_utf8(&data[pos..pos + len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid utf-8 in BINUNICODE"))?;
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(s))));
                pos += len;
            }
            0x8c => {
                // SHORT_BINUNICODE — 1-byte len + UTF-8
                if pos >= data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated SHORT_BINUNICODE"));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated SHORT_BINUNICODE data"));
                }
                let s = std::str::from_utf8(&data[pos..pos + len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid utf-8"))?;
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(s))));
                pos += len;
            }
            b'T' => {
                // BINSTRING — 4-byte LE len + bytes
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BINSTRING length"));
                }
                let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BINSTRING data"));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(data[pos..pos + len].to_vec())));
                pos += len;
            }
            b'U' => {
                // SHORT_BINSTRING — 1-byte len + bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated SHORT_BINSTRING"));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated SHORT_BINSTRING data"));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(data[pos..pos + len].to_vec())));
                pos += len;
            }
            b'B' => {
                // BINBYTES — 4-byte LE len + raw bytes
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BINBYTES length"));
                }
                let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BINBYTES data"));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(data[pos..pos + len].to_vec())));
                pos += len;
            }
            b'C' => {
                // SHORT_BINBYTES — 1-byte len + raw bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated SHORT_BINBYTES"));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated SHORT_BINBYTES data"));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(data[pos..pos + len].to_vec())));
                pos += len;
            }
            b']' => stack.push(PklStackItem::Value(PyObject::list(vec![]))),
            b'}' => stack.push(PklStackItem::Value(PyObject::dict_from_pairs(vec![]))),
            b')' => stack.push(PklStackItem::Value(PyObject::tuple(vec![]))),
            b'(' => stack.push(PklStackItem::Mark),
            b'l' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::list(items)));
            }
            b't' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::tuple(items)));
            }
            b'd' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                let mut pairs = Vec::new();
                for chunk in items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PklStackItem::Value(PyObject::dict_from_pairs(pairs)));
            }
            0x85 => {
                // TUPLE1
                let v = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: TUPLE1 stack underflow")),
                };
                stack.push(PklStackItem::Value(PyObject::tuple(vec![v])));
            }
            0x86 => {
                // TUPLE2
                let b_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: TUPLE2 stack underflow")),
                };
                let a_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: TUPLE2 stack underflow")),
                };
                stack.push(PklStackItem::Value(PyObject::tuple(vec![a_val, b_val])));
            }
            0x87 => {
                // TUPLE3
                let c_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: TUPLE3 stack underflow")),
                };
                let b_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: TUPLE3 stack underflow")),
                };
                let a_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: TUPLE3 stack underflow")),
                };
                stack.push(PklStackItem::Value(PyObject::tuple(vec![a_val, b_val, c_val])));
            }
            b'a' => {
                // APPEND
                let item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: APPEND expects value")),
                };
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().push(item);
                    }
                }
            }
            b'e' => {
                // APPENDS — pop items to mark, extend list
                let items = pkl_pop_to_mark(&mut stack)?;
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().extend(items);
                    }
                }
            }
            b's' => {
                // SETITEM
                let val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: SETITEM expects value")),
                };
                let key = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: SETITEM expects key")),
                };
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        if let Ok(hk) = HashableKey::from_object(&key) {
                            dict_map.write().insert(hk, val);
                        }
                    }
                }
            }
            b'u' => {
                // SETITEMS — pop pairs to mark, update dict
                let items = pkl_pop_to_mark(&mut stack)?;
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        let mut map = dict_map.write();
                        for chunk in items.chunks_exact(2) {
                            if let Ok(hk) = HashableKey::from_object(&chunk[0]) {
                                map.insert(hk, chunk[1].clone());
                            }
                        }
                    }
                }
            }
            b'q' => {
                // BINPUT — 1-byte memo index
                if pos >= data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BINPUT"));
                }
                let id = data[pos] as u32;
                pos += 1;
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'h' => {
                // BINGET — 1-byte memo index
                if pos >= data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated BINGET"));
                }
                let id = data[pos] as u32;
                pos += 1;
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!("UnpicklingError: memo key {} not found", id))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'r' => {
                // LONG_BINPUT — 4-byte LE memo index
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated LONG_BINPUT"));
                }
                let id = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                pos += 4;
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'j' => {
                // LONG_BINGET — 4-byte LE memo index
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated LONG_BINGET"));
                }
                let id = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
                pos += 4;
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!("UnpicklingError: memo key {} not found", id))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'p' => {
                // PUT (text) — read id to newline
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line).unwrap_or("0").trim().parse().unwrap_or(0);
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'g' => {
                // GET (text) — read id to newline
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line).unwrap_or("0").trim().parse().unwrap_or(0);
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!("UnpicklingError: memo key {} not found", id))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'c' => {
                // GLOBAL — module\nqualname\n
                let mod_line = p0_read_line(data, &mut pos);
                let name_line = p0_read_line(data, &mut pos);
                let module = String::from_utf8_lossy(mod_line).to_string();
                let name = String::from_utf8_lossy(name_line).to_string();
                stack.push(PklStackItem::Global(module, name));
            }
            0x93 => {
                // STACK_GLOBAL — pop name, pop module, push global
                let name_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v.py_to_string(),
                    _ => return Err(PyException::runtime_error("UnpicklingError: STACK_GLOBAL expects name")),
                };
                let mod_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v.py_to_string(),
                    _ => return Err(PyException::runtime_error("UnpicklingError: STACK_GLOBAL expects module")),
                };
                stack.push(PklStackItem::Global(mod_item, name_item));
            }
            b'R' => {
                // REDUCE
                let args_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => return Err(PyException::runtime_error("UnpicklingError: REDUCE expects args")),
                };
                let callable = match stack.pop() {
                    Some(item) => item,
                    None => return Err(PyException::runtime_error("UnpicklingError: REDUCE expects callable")),
                };
                let result = pkl_reduce(&callable, &args_item)?;
                stack.push(PklStackItem::Value(result));
            }
            0x8a => {
                // LONG1 — 1-byte count + little-endian 2's complement bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated LONG1"));
                }
                let count = data[pos] as usize;
                pos += 1;
                if pos + count > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated LONG1 data"));
                }
                let bytes = &data[pos..pos + count];
                pos += count;
                if count == 0 {
                    stack.push(PklStackItem::Value(PyObject::int(0)));
                } else {
                    // Little-endian 2's complement
                    let mut val: i64 = 0;
                    for (i, &b) in bytes.iter().enumerate() {
                        val |= (b as i64) << (i * 8);
                    }
                    // Sign extend if high bit set
                    if bytes[count - 1] & 0x80 != 0 {
                        for i in count..8 {
                            val |= 0xffi64 << (i * 8);
                        }
                    }
                    stack.push(PklStackItem::Value(PyObject::int(val)));
                }
            }
            b'V' => {
                // UNICODE (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let s = p0_unescape_unicode(line);
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(s))));
            }
            b'S' => {
                // STRING (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let bytes = p0_unescape_bytes(line);
                stack.push(PklStackItem::Value(PyObject::bytes(bytes)));
            }
            b'F' => {
                // FLOAT (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line).unwrap_or("0").trim();
                let val: f64 = match s {
                    "nan" | "NaN" => f64::NAN,
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => s.parse().unwrap_or(0.0),
                };
                stack.push(PklStackItem::Value(PyObject::float(val)));
            }
            b'0' => {} // POP — discard top (used after PUT sometimes)
            b'1' => {} // POP_MARK — discard stack to mark
            b'2' => {
                // DUP — duplicate top of stack
                let val = pkl_stack_top_value(&stack)?;
                stack.push(PklStackItem::Value(val));
            }
            _ => {
                return Err(PyException::runtime_error(
                    format!("UnpicklingError: unknown opcode 0x{:02x}", opcode),
                ));
            }
        }
    }

    for item in stack.iter().rev() {
        if let PklStackItem::Value(v) = item {
            return Ok(v.clone());
        }
    }
    Err(PyException::runtime_error("UnpicklingError: empty pickle data"))
}

// ── Unified deserialization (auto-detects protocol) ──

fn pickle_loads_stack(data: &[u8]) -> PyResult<PyObjectRef> {
    if data.is_empty() {
        return Err(PyException::runtime_error("UnpicklingError: empty pickle data"));
    }
    if data[0] == 0x80 {
        pickle_loads_p2(data)
    } else {
        pickle_loads_p0(data)
    }
}

// ── Public API ──

pub fn create_pickle_module() -> PyObjectRef {
    let pickler_cls = {
        PyObject::native_closure("Pickler", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Pickler requires a file argument"));
            }
            let file = args[0].clone();
            let protocol = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
            let buf: Rc<PyCell<Vec<u8>>> = Rc::new(PyCell::new(Vec::new()));

            let cls_inner = PyObject::class(CompactString::from("Pickler"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls_inner);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_file"), file.clone());
                w.insert(CompactString::from("protocol"), PyObject::int(protocol));
                let b = buf.clone();
                let f = file.clone();
                w.insert(CompactString::from("dump"), PyObject::native_closure("dump", move |dargs| {
                    if dargs.is_empty() {
                        return Err(PyException::type_error("dump requires an object"));
                    }
                    let obj = &dargs[dargs.len() - 1];
                    let mut data = b.write();
                    data.clear();
                    pickle_serialize(obj, &mut data)?;
                    if let Some(write_fn) = f.get_attr("write") {
                        let bytes_obj = PyObject::bytes(data.clone());
                        ferrython_core::error::request_vm_call(write_fn, vec![bytes_obj]);
                    }
                    Ok(PyObject::none())
                }));
                w.insert(CompactString::from("clear_memo"), make_builtin(|_| Ok(PyObject::none())));
            }
            Ok(inst)
        })
    };

    let unpickler_cls = {
        PyObject::native_closure("Unpickler", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Unpickler requires a file argument"));
            }
            let file = args[0].clone();
            let cls_inner = PyObject::class(CompactString::from("Unpickler"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls_inner);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_file"), file.clone());
                let f = file.clone();
                w.insert(CompactString::from("load"), PyObject::native_closure("load", move |_largs| {
                    if let Some(read_fn) = f.get_attr("read") {
                        ferrython_core::error::request_vm_call(read_fn, vec![]);
                    }
                    Ok(PyObject::none())
                }));
            }
            Ok(inst)
        })
    };

    let pickling_error = PyObject::class(CompactString::from("PicklingError"), vec![], IndexMap::new());
    let unpickling_error = PyObject::class(CompactString::from("UnpicklingError"), vec![], IndexMap::new());

    make_module("pickle", vec![
        ("dumps", make_builtin(pickle_dumps)),
        ("loads", make_builtin(pickle_loads)),
        ("dump", make_builtin(pickle_dump)),
        ("load", make_builtin(pickle_load)),
        ("Pickler", pickler_cls),
        ("Unpickler", unpickler_cls),
        ("HIGHEST_PROTOCOL", PyObject::int(5)),
        ("DEFAULT_PROTOCOL", PyObject::int(4)),
        ("PicklingError", pickling_error),
        ("UnpicklingError", unpickling_error),
        ("PickleError", PyObject::class(CompactString::from("PickleError"), vec![], IndexMap::new())),
    ])
}

fn pickle_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.dumps() missing 1 required positional argument: 'obj'",
        ));
    }
    let protocol = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
    let mut buf = Vec::new();
    let mut memo: u32 = 0;
    if protocol >= 2 {
        buf.extend_from_slice(b"\x80\x02");
        pickle_serialize_p2(&args[0], &mut buf, &mut memo)?;
    } else {
        pickle_serialize_p0(&args[0], &mut buf, &mut memo)?;
    }
    buf.push(b'.');
    Ok(PyObject::bytes(buf))
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
    let protocol = args.get(2).and_then(|a| a.as_int()).unwrap_or(0);
    let data = pickle_dumps(&[args[0].clone(), PyObject::int(protocol)])?;
    let data_bytes = extract_bytes(&data)?;

    // Try file path first (via .name attribute)
    if let Some(name) = args[1].get_attr("name") {
        let path = name.py_to_string();
        if !path.is_empty() {
            std::fs::write(&path, &data_bytes)
                .map_err(|e| PyException::runtime_error(format!("pickle.dump: {}", e)))?;
            return Ok(PyObject::none());
        }
    }
    // Try file-like object with write method (BytesIO, etc.)
    if let Some(write_method) = args[1].get_attr("write") {
        match &write_method.payload {
            PyObjectPayload::NativeFunction(nf) => {
                let _ = (nf.func)(&[PyObject::bytes(data_bytes.clone())]);
                return Ok(PyObject::none());
            }
            PyObjectPayload::NativeClosure(nc) => {
                let _ = (nc.func)(&[PyObject::bytes(data_bytes.clone())]);
                return Ok(PyObject::none());
            }
            _ => {}
        }
    }
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
    // Try file path first (via .name attribute)
    if let Some(name) = args[0].get_attr("name") {
        let path = name.py_to_string();
        if !path.is_empty() && std::path::Path::new(&path).exists() {
            let data = std::fs::read(&path)
                .map_err(|e| PyException::runtime_error(format!("pickle.load: {}", e)))?;
            return pickle_loads_stack(&data);
        }
    }
    // Try file-like object with read method (BytesIO, etc.)
    if let Some(read_method) = args[0].get_attr("read") {
        let read_result = match &read_method.payload {
            PyObjectPayload::NativeFunction(nf) => (nf.func)(&[]).ok(),
            PyObjectPayload::NativeClosure(nc) => (nc.func)(&[]).ok(),
            _ => None,
        };
        if let Some(data_obj) = read_result {
            let data = extract_bytes(&data_obj)?;
            if !data.is_empty() {
                return pickle_loads_stack(&data);
            }
        }
    }
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

// ── codecs helpers ─────────────────────────────────────────────────

fn normalize_encoding(enc: &str) -> String {
    enc.to_lowercase().replace('-', "_")
}

fn rot13(c: char) -> char {
    match c {
        'a'..='m' | 'A'..='M' => (c as u8 + 13) as char,
        'n'..='z' | 'N'..='Z' => (c as u8 - 13) as char,
        _ => c,
    }
}

fn punycode_digit(d: u32) -> u8 {
    if d < 26 { b'a' + d as u8  } else { b'0' + (d as u8 - 26)  }
}

fn punycode_adapt(delta: u32, numpoints: u32, firsttime: bool) -> u32 {
    let mut d = if firsttime { delta / 700 } else { delta / 2 };
    d += d / numpoints;
    let mut k = 0u32;
    while d > 455 {
        d /= 35;
        k += 36;
    }
    k + (36 * d) / (d + 38)
}

fn cp1252_encode(c: char) -> Result<u8, String> {
    let u = c as u32;
    if u < 0x80 || (0xA0..=0xFF).contains(&u) {
        return Ok(u as u8);
    }
    // Windows-1252 special range 0x80-0x9F
    match u {
        0x20AC => Ok(0x80), 0x201A => Ok(0x82), 0x0192 => Ok(0x83),
        0x201E => Ok(0x84), 0x2026 => Ok(0x85), 0x2020 => Ok(0x86),
        0x2021 => Ok(0x87), 0x02C6 => Ok(0x88), 0x2030 => Ok(0x89),
        0x0160 => Ok(0x8A), 0x2039 => Ok(0x8B), 0x0152 => Ok(0x8C),
        0x017D => Ok(0x8E), 0x2018 => Ok(0x91), 0x2019 => Ok(0x92),
        0x201C => Ok(0x93), 0x201D => Ok(0x94), 0x2022 => Ok(0x95),
        0x2013 => Ok(0x96), 0x2014 => Ok(0x97), 0x02DC => Ok(0x98),
        0x2122 => Ok(0x99), 0x0161 => Ok(0x9A), 0x203A => Ok(0x9B),
        0x0153 => Ok(0x9C), 0x017E => Ok(0x9E), 0x0178 => Ok(0x9F),
        _ => Err(format!("'cp1252' codec can't encode character '\\u{:04x}'", u)),
    }
}

fn cp1252_decode(b: u8) -> char {
    if b < 0x80 || b >= 0xA0 {
        return b as char;
    }
    match b {
        0x80 => '\u{20AC}', 0x82 => '\u{201A}', 0x83 => '\u{0192}',
        0x84 => '\u{201E}', 0x85 => '\u{2026}', 0x86 => '\u{2020}',
        0x87 => '\u{2021}', 0x88 => '\u{02C6}', 0x89 => '\u{2030}',
        0x8A => '\u{0160}', 0x8B => '\u{2039}', 0x8C => '\u{0152}',
        0x8E => '\u{017D}', 0x91 => '\u{2018}', 0x92 => '\u{2019}',
        0x93 => '\u{201C}', 0x94 => '\u{201D}', 0x95 => '\u{2022}',
        0x96 => '\u{2013}', 0x97 => '\u{2014}', 0x98 => '\u{02DC}',
        0x99 => '\u{2122}', 0x9A => '\u{0161}', 0x9B => '\u{203A}',
        0x9C => '\u{0153}', 0x9E => '\u{017E}', 0x9F => '\u{0178}',
        _ => '\u{FFFD}', // undefined bytes → replacement char
    }
}

fn decode_utf16_with_bom(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        decode_utf16_le(&bytes[2..])
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        decode_utf16_be(&bytes[2..])
    } else {
        decode_utf16_le(bytes) // default to LE like CPython on little-endian
    }
}

fn decode_utf16_le(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 2 != 0 {
        return Err(PyException::value_error("utf-16-le: truncated data"));
    }
    let u16s: Vec<u16> = bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let s = String::from_utf16(&u16s).map_err(|_| PyException::value_error("invalid utf-16-le"))?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn decode_utf16_be(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 2 != 0 {
        return Err(PyException::value_error("utf-16-be: truncated data"));
    }
    let u16s: Vec<u16> = bytes.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
    let s = String::from_utf16(&u16s).map_err(|_| PyException::value_error("invalid utf-16-be"))?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn decode_utf32_with_bom(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() >= 4 && bytes[..4] == [0xFF, 0xFE, 0x00, 0x00] {
        decode_utf32_le(&bytes[4..])
    } else if bytes.len() >= 4 && bytes[..4] == [0x00, 0x00, 0xFE, 0xFF] {
        decode_utf32_be(&bytes[4..])
    } else {
        decode_utf32_le(bytes)
    }
}

fn decode_utf32_le(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 4 != 0 {
        return Err(PyException::value_error("utf-32-le: truncated data"));
    }
    let s: Result<String, _> = bytes.chunks(4)
        .map(|c| {
            let cp = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            char::from_u32(cp).ok_or_else(|| PyException::value_error("invalid utf-32-le codepoint"))
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(s?)))
}

fn decode_utf32_be(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 4 != 0 {
        return Err(PyException::value_error("utf-32-be: truncated data"));
    }
    let s: Result<String, _> = bytes.chunks(4)
        .map(|c| {
            let cp = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
            char::from_u32(cp).ok_or_else(|| PyException::value_error("invalid utf-32-be codepoint"))
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(s?)))
}

fn backslashreplace_char(c: char) -> String {
    let cp = c as u32;
    if cp <= 0xFF {
        format!("\\x{:02x}", cp)
    } else if cp <= 0xFFFF {
        format!("\\u{:04x}", cp)
    } else {
        format!("\\U{:08x}", cp)
    }
}

fn xmlcharrefreplace_char(c: char) -> String {
    format!("&#{};", c as u32)
}

fn resolve_encoding(norm: &str) -> &str {
    match norm {
        "utf_8" | "utf8" => "utf_8",
        "ascii" | "us_ascii" => "ascii",
        "latin_1" | "latin1" | "iso_8859_1" | "iso8859_1" | "8859" | "cp819" | "l1" => "latin_1",
        "utf_16" | "utf16" => "utf_16",
        "utf_16_le" | "utf16_le" | "utf_16le" => "utf_16_le",
        "utf_16_be" | "utf16_be" | "utf_16be" => "utf_16_be",
        "utf_32" | "utf32" => "utf_32",
        "utf_32_le" | "utf32_le" => "utf_32_le",
        "utf_32_be" | "utf32_be" => "utf_32_be",
        "cp1252" | "windows_1252" => "cp1252",
        "rot_13" | "rot13" => "rot_13",
        "punycode" => "punycode",
        "idna" => "idna",
        // ISO-8859 family
        "iso8859_2" | "iso_8859_2" | "latin2" | "l2" => "iso8859_2",
        "iso8859_3" | "iso_8859_3" | "latin3" | "l3" => "iso8859_3",
        "iso8859_4" | "iso_8859_4" | "latin4" | "l4" => "iso8859_4",
        "iso8859_5" | "iso_8859_5" | "cyrillic" => "iso8859_5",
        "iso8859_6" | "iso_8859_6" | "arabic" => "iso8859_6",
        "iso8859_7" | "iso_8859_7" | "greek" => "iso8859_7",
        "iso8859_8" | "iso_8859_8" | "hebrew" => "iso8859_8",
        "iso8859_9" | "iso_8859_9" | "latin5" | "l5" => "iso8859_9",
        "iso8859_10" | "iso_8859_10" | "latin6" | "l6" => "iso8859_10",
        "iso8859_11" | "iso_8859_11" | "thai" => "iso8859_11",
        "iso8859_13" | "iso_8859_13" | "latin7" | "l7" => "iso8859_13",
        "iso8859_14" | "iso_8859_14" | "latin8" | "l8" => "iso8859_14",
        "iso8859_15" | "iso_8859_15" | "latin9" | "l9" => "iso8859_15",
        "iso8859_16" | "iso_8859_16" | "latin10" | "l10" => "iso8859_16",
        // Windows code pages
        "cp437" => "cp437",
        "cp850" => "cp850",
        "cp866" => "cp866",
        "cp874" | "windows_874" => "cp874",
        "cp932" | "ms932" | "mskanji" | "ms_kanji" => "cp932",
        "cp949" | "ms949" | "uhc" => "cp949",
        "cp950" | "ms950" => "cp950",
        "cp1250" | "windows_1250" => "cp1250",
        "cp1251" | "windows_1251" => "cp1251",
        "cp1253" | "windows_1253" => "cp1253",
        "cp1254" | "windows_1254" => "cp1254",
        "cp1255" | "windows_1255" => "cp1255",
        "cp1256" | "windows_1256" => "cp1256",
        "cp1257" | "windows_1257" => "cp1257",
        "cp1258" | "windows_1258" => "cp1258",
        // CJK encodings
        "big5" | "big5_tw" | "csbig5" => "big5",
        "big5hkscs" | "big5_hkscs" => "big5hkscs",
        "euc_jp" | "eucjp" | "ujis" | "u_jis" => "euc_jp",
        "euc_kr" | "euckr" | "korean" => "euc_kr",
        "euc_cn" | "gb2312" | "chinese" | "csiso58gb231280" => "gb2312",
        "gbk" | "cp936" | "ms936" => "gbk",
        "gb18030" => "gb18030",
        "hz" | "hzgb" | "hz_gb" | "hz_gb_2312" => "hz",
        "shift_jis" | "shiftjis" | "sjis" | "s_jis" | "csshiftjis" => "shift_jis",
        "shift_jis_2004" | "shiftjis2004" | "sjis_2004" => "shift_jis_2004",
        "shift_jisx0213" | "shiftjisx0213" | "sjisx0213" => "shift_jisx0213",
        "iso2022_jp" | "iso_2022_jp" | "csiso2022jp" => "iso2022_jp",
        "iso2022_jp_2" | "iso_2022_jp_2" => "iso2022_jp_2",
        "iso2022_kr" | "iso_2022_kr" | "csiso2022kr" => "iso2022_kr",
        "iso2022_cn" | "iso_2022_cn" => "iso2022_cn",
        // Russian/Ukrainian
        "koi8_r" | "koi8r" => "koi8_r",
        "koi8_u" | "koi8u" => "koi8_u",
        "koi8_t" => "koi8_t",
        // Mac encodings
        "mac_roman" | "macroman" | "macintosh" => "mac_roman",
        "mac_cyrillic" | "maccyrillic" => "mac_cyrillic",
        "mac_greek" | "macgreek" => "mac_greek",
        "mac_latin2" | "maclatin2" | "maccentraleurope" => "mac_latin2",
        // Other
        "johab" => "johab",
        "tis_620" | "tis620" => "tis_620",
        "viscii" => "viscii",
        other => other,
    }
}

// ── codecs module ──────────────────────────────────────────────────
pub fn create_codecs_module() -> PyObjectRef {
    // IncrementalDecoder base class
    let inc_decoder_cls = {
        let cls = PyObject::class(CompactString::from("IncrementalDecoder"), vec![], IndexMap::new());
        if let PyObjectPayload::Class(ref cd) = cls.payload {
            cd.namespace.write().insert(
                CompactString::from("__init__"),
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() { return Err(PyException::type_error("IncrementalDecoder.__init__ requires self")); }
                    let encoding = if args.len() > 1 { args[1].py_to_string() } else { "strict".to_string() };
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs.write().insert(CompactString::from("errors"), PyObject::str_val(CompactString::from(encoding)));
                    }
                    Ok(PyObject::none())
                }),
            );
            cd.namespace.write().insert(
                CompactString::from("decode"),
                make_builtin(|_args: &[PyObjectRef]| {
                    Err(PyException::not_implemented_error("IncrementalDecoder.decode() is abstract"))
                }),
            );
            cd.namespace.write().insert(
                CompactString::from("reset"),
                make_builtin(|_args: &[PyObjectRef]| { Ok(PyObject::none()) }),
            );
            cd.namespace.write().insert(
                CompactString::from("getstate"),
                make_builtin(|_args: &[PyObjectRef]| { Ok(PyObject::tuple(vec![PyObject::bytes(vec![]), PyObject::int(0)])) }),
            );
            cd.namespace.write().insert(
                CompactString::from("setstate"),
                make_builtin(|_args: &[PyObjectRef]| { Ok(PyObject::none()) }),
            );
        }
        cls
    };

    // IncrementalEncoder base class
    let inc_encoder_cls = {
        let cls = PyObject::class(CompactString::from("IncrementalEncoder"), vec![], IndexMap::new());
        if let PyObjectPayload::Class(ref cd) = cls.payload {
            cd.namespace.write().insert(
                CompactString::from("__init__"),
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() { return Err(PyException::type_error("IncrementalEncoder.__init__ requires self")); }
                    let errors = if args.len() > 1 { args[1].py_to_string() } else { "strict".to_string() };
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs.write().insert(CompactString::from("errors"), PyObject::str_val(CompactString::from(errors)));
                    }
                    Ok(PyObject::none())
                }),
            );
            cd.namespace.write().insert(
                CompactString::from("encode"),
                make_builtin(|_args: &[PyObjectRef]| {
                    Err(PyException::not_implemented_error("IncrementalEncoder.encode() is abstract"))
                }),
            );
            cd.namespace.write().insert(
                CompactString::from("reset"),
                make_builtin(|_args: &[PyObjectRef]| { Ok(PyObject::none()) }),
            );
            cd.namespace.write().insert(
                CompactString::from("getstate"),
                make_builtin(|_args: &[PyObjectRef]| { Ok(PyObject::int(0)) }),
            );
            cd.namespace.write().insert(
                CompactString::from("setstate"),
                make_builtin(|_args: &[PyObjectRef]| { Ok(PyObject::none()) }),
            );
        }
        cls
    };

    // StreamReader / StreamWriter / CodecInfo base classes (stubs)
    let stream_reader_cls = PyObject::class(CompactString::from("StreamReader"), vec![], IndexMap::new());
    let stream_writer_cls = PyObject::class(CompactString::from("StreamWriter"), vec![], IndexMap::new());
    let codec_info_cls = PyObject::class(CompactString::from("CodecInfo"), vec![], IndexMap::new());

    make_module("codecs", vec![
        ("encode", make_builtin(codecs_encode)),
        ("decode", make_builtin(codecs_decode)),
        ("lookup", make_builtin(codecs_lookup)),
        ("getencoder", make_builtin(codecs_getencoder)),
        ("getdecoder", make_builtin(codecs_getdecoder)),
        ("getincrementaldecoder", {
            let idc = inc_decoder_cls.clone();
            PyObject::native_closure("getincrementaldecoder", move |args: &[PyObjectRef]| {
                check_args("codecs.getincrementaldecoder", args, 1)?;
                let _encoding = args[0].py_to_string();
                // Return the IncrementalDecoder class (simplified — always returns base class)
                Ok(idc.clone())
            })
        }),
        ("getincrementalencoder", {
            let iec = inc_encoder_cls.clone();
            PyObject::native_closure("getincrementalencoder", move |args: &[PyObjectRef]| {
                check_args("codecs.getincrementalencoder", args, 1)?;
                let _encoding = args[0].py_to_string();
                Ok(iec.clone())
            })
        }),
        ("getreader", {
            let sr = stream_reader_cls.clone();
            PyObject::native_closure("getreader", move |args: &[PyObjectRef]| {
                check_args("codecs.getreader", args, 1)?;
                Ok(sr.clone())
            })
        }),
        ("getwriter", {
            let sw = stream_writer_cls.clone();
            PyObject::native_closure("getwriter", move |args: &[PyObjectRef]| {
                check_args("codecs.getwriter", args, 1)?;
                Ok(sw.clone())
            })
        }),
        ("utf_8_encode", make_builtin(codecs_utf8_encode)),
        ("utf_8_decode", make_builtin(codecs_utf8_decode)),
        ("IncrementalDecoder", inc_decoder_cls),
        ("IncrementalEncoder", inc_encoder_cls),
        ("StreamReader", stream_reader_cls),
        ("StreamWriter", stream_writer_cls),
        ("CodecInfo", codec_info_cls),
        ("register", make_builtin(|_args: &[PyObjectRef]| { Ok(PyObject::none()) })),
        ("register_error", make_builtin(|_args: &[PyObjectRef]| { Ok(PyObject::none()) })),
        ("lookup_error", make_builtin(|args: &[PyObjectRef]| {
            check_args("codecs.lookup_error", args, 1)?;
            Err(PyException::new(ExceptionKind::LookupError, format!("unknown error handler name '{}'", args[0].py_to_string())))
        })),
        ("open", make_builtin(|args: &[PyObjectRef]| {
            check_args_min("codecs.open", args, 1)?;
            let filename = args[0].py_to_string();
            let mode = if args.len() > 1 && !matches!(args[1].payload, PyObjectPayload::Dict(_)) {
                args[1].py_to_string()
            } else { "r".to_string() };
            let _encoding = if args.len() > 2 && !matches!(args[2].payload, PyObjectPayload::Dict(_)) {
                args[2].py_to_string()
            } else { "utf-8".to_string() };
            // Delegate to Rust file I/O — codecs.open is just open() with encoding
            if mode.contains('w') {
                // Verify file is creatable
                let _ = std::fs::File::create(&filename)
                    .map_err(|e| PyException::os_error(format!("{}: {}", e, filename)))?;
                let mut attrs = IndexMap::new();
                let path = filename.clone();
                let buf = Rc::new(PyCell::new(String::new()));
                let buf_w = buf.clone();
                let buf_r = buf.clone();
                let path_w = path.clone();
                attrs.insert(CompactString::from("write"), PyObject::native_closure(
                    "write", move |wargs: &[PyObjectRef]| {
                        if let Some(s) = wargs.first() {
                            buf_w.write().push_str(&s.py_to_string());
                        }
                        Ok(PyObject::none())
                    }
                ));
                attrs.insert(CompactString::from("flush"), PyObject::native_closure(
                    "flush", move |_| {
                        let content = buf_r.read().clone();
                        std::fs::write(&path_w, content.as_bytes())
                            .map_err(|e| PyException::os_error(e.to_string()))?;
                        Ok(PyObject::none())
                    }
                ));
                let path_c = path.clone();
                let buf_c = buf.clone();
                attrs.insert(CompactString::from("close"), PyObject::native_closure(
                    "close", move |_| {
                        let content = buf_c.read().clone();
                        std::fs::write(&path_c, content.as_bytes())
                            .map_err(|e| PyException::os_error(e.to_string()))?;
                        Ok(PyObject::none())
                    }
                ));
                attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
                    "__enter__", |a: &[PyObjectRef]| {
                        Ok(if !a.is_empty() { a[0].clone() } else { PyObject::none() })
                    }
                ));
                let path_e = path.clone();
                let buf_e = buf.clone();
                attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                    "__exit__", move |_| {
                        let content = buf_e.read().clone();
                        let _ = std::fs::write(&path_e, content.as_bytes());
                        Ok(PyObject::bool_val(false))
                    }
                ));
                Ok(PyObject::module_with_attrs(
                    CompactString::from("TextIOWrapper"),
                    attrs,
                ))
            } else {
                // Read mode
                let content = std::fs::read_to_string(&filename)
                    .map_err(|e| PyException::os_error(format!("{}: {}", e, filename)))?;
                let content_arc = Arc::new(content);
                let c1 = content_arc.clone();
                let c2 = content_arc.clone();
                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("read"), PyObject::native_closure(
                    "read", move |_| Ok(PyObject::str_val(CompactString::from(c1.as_str())))
                ));
                attrs.insert(CompactString::from("readlines"), PyObject::native_closure(
                    "readlines", move |_| {
                        let lines: Vec<PyObjectRef> = c2.lines()
                            .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
                            .collect();
                        Ok(PyObject::list(lines))
                    }
                ));
                attrs.insert(CompactString::from("close"), PyObject::native_function(
                    "close", |_: &[PyObjectRef]| Ok(PyObject::none())
                ));
                attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
                    "__enter__", |a: &[PyObjectRef]| {
                        Ok(if !a.is_empty() { a[0].clone() } else { PyObject::none() })
                    }
                ));
                attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
                    "__exit__", |_: &[PyObjectRef]| Ok(PyObject::bool_val(false))
                ));
                Ok(PyObject::module_with_attrs(
                    CompactString::from("TextIOWrapper"),
                    attrs,
                ))
            }
        })),
        ("BOM", PyObject::bytes(vec![0xFF, 0xFE])),
        ("BOM_UTF8", PyObject::bytes(vec![0xEF, 0xBB, 0xBF])),
        ("BOM_UTF16", PyObject::bytes(vec![0xFF, 0xFE])),
        ("BOM_UTF16_LE", PyObject::bytes(vec![0xFF, 0xFE])),
        ("BOM_UTF16_BE", PyObject::bytes(vec![0xFE, 0xFF])),
        ("BOM_UTF32", PyObject::bytes(vec![0xFF, 0xFE, 0x00, 0x00])),
        ("BOM_UTF32_LE", PyObject::bytes(vec![0xFF, 0xFE, 0x00, 0x00])),
        ("BOM_UTF32_BE", PyObject::bytes(vec![0x00, 0x00, 0xFE, 0xFF])),
        // Error handlers (CPython exposes these as module-level functions)
        ("strict_errors", PyObject::native_function("codecs.strict_errors", |args: &[PyObjectRef]| {
            let exc = if args.is_empty() { PyException::runtime_error("strict_errors") } else {
                PyException::runtime_error(args[0].py_to_string())
            };
            Err(exc)
        })),
        ("ignore_errors", PyObject::native_function("codecs.ignore_errors", |args: &[PyObjectRef]| {
            // Returns (replacement, position) tuple
            let end = if !args.is_empty() {
                args[0].get_attr("end").and_then(|v| v.to_int().ok()).unwrap_or(0)
            } else { 0 };
            Ok(PyObject::tuple(vec![PyObject::str_val(CompactString::from("")), PyObject::int(end)]))
        })),
        ("replace_errors", PyObject::native_function("codecs.replace_errors", |args: &[PyObjectRef]| {
            let end = if !args.is_empty() {
                args[0].get_attr("end").and_then(|v| v.to_int().ok()).unwrap_or(0)
            } else { 0 };
            Ok(PyObject::tuple(vec![PyObject::str_val(CompactString::from("?")), PyObject::int(end)]))
        })),
        ("xmlcharrefreplace_errors", PyObject::native_function("codecs.xmlcharrefreplace_errors", |args: &[PyObjectRef]| {
            // Replace unencodable characters with XML character references
            let (obj_str, start, end) = if !args.is_empty() {
                let exc = &args[0];
                let o = exc.get_attr("object").map(|v| v.py_to_string()).unwrap_or_default();
                let s = exc.get_attr("start").and_then(|v| v.to_int().ok()).unwrap_or(0) as usize;
                let e = exc.get_attr("end").and_then(|v| v.to_int().ok()).unwrap_or(0) as usize;
                (o, s, e)
            } else { (String::new(), 0, 0) };
            let chars: Vec<char> = obj_str.chars().collect();
            let mut replacement = String::new();
            for i in start..end.min(chars.len()) {
                replacement.push_str(&format!("&#{};", chars[i] as u32));
            }
            Ok(PyObject::tuple(vec![PyObject::str_val(CompactString::from(replacement)), PyObject::int(end as i64)]))
        })),
        ("backslashreplace_errors", PyObject::native_function("codecs.backslashreplace_errors", |args: &[PyObjectRef]| {
            let (obj_str, start, end) = if !args.is_empty() {
                let exc = &args[0];
                let o = exc.get_attr("object").map(|v| v.py_to_string()).unwrap_or_default();
                let s = exc.get_attr("start").and_then(|v| v.to_int().ok()).unwrap_or(0) as usize;
                let e = exc.get_attr("end").and_then(|v| v.to_int().ok()).unwrap_or(0) as usize;
                (o, s, e)
            } else { (String::new(), 0, 0) };
            let chars: Vec<char> = obj_str.chars().collect();
            let mut replacement = String::new();
            for i in start..end.min(chars.len()) {
                let c = chars[i] as u32;
                if c <= 0xFF { replacement.push_str(&format!("\\x{:02x}", c)); }
                else if c <= 0xFFFF { replacement.push_str(&format!("\\u{:04x}", c)); }
                else { replacement.push_str(&format!("\\U{:08x}", c)); }
            }
            Ok(PyObject::tuple(vec![PyObject::str_val(CompactString::from(replacement)), PyObject::int(end as i64)]))
        })),
        ("namereplace_errors", PyObject::native_function("codecs.namereplace_errors", |args: &[PyObjectRef]| {
            let (obj_str, start, end) = if !args.is_empty() {
                let exc = &args[0];
                let o = exc.get_attr("object").map(|v| v.py_to_string()).unwrap_or_default();
                let s = exc.get_attr("start").and_then(|v| v.to_int().ok()).unwrap_or(0) as usize;
                let e = exc.get_attr("end").and_then(|v| v.to_int().ok()).unwrap_or(0) as usize;
                (o, s, e)
            } else { (String::new(), 0, 0) };
            let chars: Vec<char> = obj_str.chars().collect();
            let mut replacement = String::new();
            for i in start..end.min(chars.len()) {
                replacement.push_str(&format!("\\N{{{:04X}}}", chars[i] as u32));
            }
            Ok(PyObject::tuple(vec![PyObject::str_val(CompactString::from(replacement)), PyObject::int(end as i64)]))
        })),
    ])
}

fn codecs_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.encode", args, 1)?;
    let s = args[0].py_to_string();
    let encoding = if args.len() > 1 { args[1].py_to_string() } else { "utf-8".to_string() };
    let errors = if args.len() > 2 { args[2].py_to_string() } else { "strict".to_string() };
    let norm = normalize_encoding(&encoding);
    let enc = resolve_encoding(&norm);
    match enc {
        "utf_8" => Ok(PyObject::bytes(s.as_bytes().to_vec())),
        "ascii" => {
            let mut out = Vec::new();
            for (i, c) in s.chars().enumerate() {
                if c.is_ascii() {
                    out.push(c as u8);
                } else {
                    match errors.as_str() {
                        "ignore" => {}
                        "replace" => out.push(b'?'),
                        "backslashreplace" => out.extend_from_slice(backslashreplace_char(c).as_bytes()),
                        "xmlcharrefreplace" => out.extend_from_slice(xmlcharrefreplace_char(c).as_bytes()),
                        _ => return Err(PyException::value_error(format!(
                            "'ascii' codec can't encode character '\\u{:04x}' in position {}", c as u32, i
                        ))),
                    }
                }
            }
            Ok(PyObject::bytes(out))
        }
        "latin_1" => {
            let mut out = Vec::new();
            for (i, c) in s.chars().enumerate() {
                if (c as u32) <= 255 {
                    out.push(c as u8);
                } else {
                    match errors.as_str() {
                        "ignore" => {}
                        "replace" => out.push(b'?'),
                        "backslashreplace" => out.extend_from_slice(backslashreplace_char(c).as_bytes()),
                        "xmlcharrefreplace" => out.extend_from_slice(xmlcharrefreplace_char(c).as_bytes()),
                        _ => return Err(PyException::value_error(format!(
                            "'latin-1' codec can't encode character '\\u{:04x}' in position {}", c as u32, i
                        ))),
                    }
                }
            }
            Ok(PyObject::bytes(out))
        }
        "utf_16" => {
            let mut bytes = vec![0xFF, 0xFE]; // BOM (little-endian)
            for c in s.encode_utf16() {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
            Ok(PyObject::bytes(bytes))
        }
        "utf_16_le" => {
            let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "utf_16_be" => {
            let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "utf_32" => {
            let mut bytes = vec![0xFF, 0xFE, 0x00, 0x00]; // BOM (little-endian)
            for c in s.chars() {
                bytes.extend_from_slice(&(c as u32).to_le_bytes());
            }
            Ok(PyObject::bytes(bytes))
        }
        "utf_32_le" => {
            let bytes: Vec<u8> = s.chars().flat_map(|c| (c as u32).to_le_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "utf_32_be" => {
            let bytes: Vec<u8> = s.chars().flat_map(|c| (c as u32).to_be_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "cp1252" => {
            let mut out = Vec::new();
            for (i, c) in s.chars().enumerate() {
                match cp1252_encode(c) {
                    Ok(b) => out.push(b),
                    Err(_) => match errors.as_str() {
                        "ignore" => {}
                        "replace" => out.push(b'?'),
                        "backslashreplace" => out.extend_from_slice(backslashreplace_char(c).as_bytes()),
                        "xmlcharrefreplace" => out.extend_from_slice(xmlcharrefreplace_char(c).as_bytes()),
                        _ => return Err(PyException::value_error(format!(
                            "'cp1252' codec can't encode character '\\u{:04x}' in position {}", c as u32, i
                        ))),
                    },
                }
            }
            Ok(PyObject::bytes(out))
        }
        "rot_13" => {
            let rotated: String = s.chars().map(|c| rot13(c)).collect();
            Ok(PyObject::str_val(CompactString::from(rotated)))
        }
        "punycode" => {
            // Simple punycode encoding (RFC 3492)
            let mut output = Vec::new();
            let mut basic: Vec<char> = Vec::new();
            let mut non_basic: Vec<char> = Vec::new();
            for ch in s.chars() {
                if ch.is_ascii() {
                    basic.push(ch);
                    output.push(ch as u8);
                } else {
                    non_basic.push(ch);
                }
            }
            if !basic.is_empty() && !non_basic.is_empty() {
                output.push(b'-');
            }
            // Simplified: for non-ASCII chars, use the Punycode delta encoding
            let mut n: u32 = 128;
            let mut delta: u32 = 0;
            let mut bias: u32 = 72;
            let mut h = basic.len() as u32;
            let b_len = h;
            let all_chars: Vec<u32> = s.chars().map(|c| c as u32).collect();
            let total = all_chars.len() as u32;
            while h < total {
                let m = *all_chars.iter().filter(|&&cp| cp >= n).min().unwrap_or(&n);
                delta = delta.wrapping_add((m - n).wrapping_mul(h + 1));
                n = m;
                for &cp in &all_chars {
                    if cp < n { delta = delta.wrapping_add(1); }
                    if cp == n {
                        let mut q = delta;
                        let mut k = 36u32;
                        loop {
                            let t = if k <= bias { 1 } else if k >= bias + 26 { 26 } else { k - bias };
                            if q < t { break; }
                            let digit = t + (q - t) % (36 - t);
                            output.push(punycode_digit(digit));
                            q = (q - t) / (36 - t);
                            k += 36;
                        }
                        output.push(punycode_digit(q));
                        bias = punycode_adapt(delta, h + 1, h == b_len);
                        delta = 0;
                        h += 1;
                    }
                }
                delta += 1;
                n += 1;
            }
            Ok(PyObject::bytes(output))
        }
        "idna" => {
            // Simple IDNA encoding: lowercase ASCII
            let encoded = s.to_ascii_lowercase();
            Ok(PyObject::bytes(encoded.into_bytes()))
        }
        _ => Err(PyException::value_error(format!("unknown encoding: {}", encoding))),
    }
}

fn codecs_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.decode", args, 1)?;
    let encoding = if args.len() > 1 { args[1].py_to_string() } else { "utf-8".to_string() };
    let errors = if args.len() > 2 { args[2].py_to_string() } else { "strict".to_string() };
    let norm = normalize_encoding(&encoding);
    let enc = resolve_encoding(&norm);
    // rot_13 decode works on strings
    if enc == "rot_13" {
        let s = args[0].py_to_string();
        let rotated: String = s.chars().map(|c| rot13(c)).collect();
        return Ok(PyObject::str_val(CompactString::from(rotated)));
    }
    let bytes = extract_bytes(&args[0])?;
    match enc {
        "utf_8" => {
            match String::from_utf8(bytes.clone()) {
                Ok(s) => Ok(PyObject::str_val(CompactString::from(s))),
                Err(_) => match errors.as_str() {
                    "ignore" => {
                        let s: String = bytes.iter().filter(|b| b.is_ascii()).map(|&b| b as char).collect();
                        Ok(PyObject::str_val(CompactString::from(s)))
                    }
                    "replace" => {
                        let s = String::from_utf8_lossy(&bytes).to_string();
                        Ok(PyObject::str_val(CompactString::from(s)))
                    }
                    _ => Err(PyException::value_error("invalid utf-8")),
                },
            }
        }
        "ascii" => {
            let mut out = String::new();
            for (i, &b) in bytes.iter().enumerate() {
                if b <= 127 {
                    out.push(b as char);
                } else {
                    match errors.as_str() {
                        "ignore" => {}
                        "replace" => out.push('\u{FFFD}'),
                        _ => return Err(PyException::value_error(format!(
                            "'ascii' codec can't decode byte 0x{:02x} in position {}", b, i
                        ))),
                    }
                }
            }
            Ok(PyObject::str_val(CompactString::from(out)))
        }
        "latin_1" => {
            let s: String = bytes.iter().map(|&b| b as char).collect();
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "utf_16" => decode_utf16_with_bom(&bytes),
        "utf_16_le" => decode_utf16_le(&bytes),
        "utf_16_be" => decode_utf16_be(&bytes),
        "utf_32" => decode_utf32_with_bom(&bytes),
        "utf_32_le" => decode_utf32_le(&bytes),
        "utf_32_be" => decode_utf32_be(&bytes),
        "cp1252" => {
            let s: String = bytes.iter().map(|&b| cp1252_decode(b)).collect();
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "punycode" => {
            // Punycode decode (RFC 3492)
            let input = std::str::from_utf8(&bytes)
                .map_err(|_| PyException::value_error("punycode: invalid input"))?;
            let (basic_part, encoded_part) = if let Some(pos) = input.rfind('-') {
                (&input[..pos], &input[pos + 1..])
            } else {
                ("", input.as_ref())
            };
            let mut output: Vec<u32> = basic_part.chars().map(|c| c as u32).collect();
            let mut n: u32 = 128;
            let mut i: u32 = 0;
            let mut bias: u32 = 72;
            let encoded_bytes = encoded_part.as_bytes();
            let mut idx = 0;
            while idx < encoded_bytes.len() {
                let oldi = i;
                let mut w: u32 = 1;
                let mut k: u32 = 36;
                loop {
                    if idx >= encoded_bytes.len() { break; }
                    let byte = encoded_bytes[idx];
                    idx += 1;
                    let digit = if byte >= b'a' && byte <= b'z' { (byte - b'a') as u32 }
                        else if byte >= b'A' && byte <= b'Z' { (byte - b'A') as u32 }
                        else if byte >= b'0' && byte <= b'9' { (byte - b'0') as u32 + 26 }
                        else { return Err(PyException::value_error("punycode: bad input")); };
                    i = i.wrapping_add(digit.wrapping_mul(w));
                    let t = if k <= bias { 1 } else if k >= bias + 26 { 26 } else { k - bias };
                    if digit < t { break; }
                    w = w.wrapping_mul(36 - t);
                    k += 36;
                }
                let out_len = output.len() as u32 + 1;
                bias = punycode_adapt(i.wrapping_sub(oldi), out_len, oldi == 0);
                n = n.wrapping_add(i / out_len);
                i %= out_len;
                output.insert(i as usize, n);
                i += 1;
            }
            let result: String = output.iter().filter_map(|&cp| char::from_u32(cp)).collect();
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "idna" => {
            let s = std::str::from_utf8(&bytes)
                .map_err(|_| PyException::value_error("idna: invalid bytes"))?;
            Ok(PyObject::str_val(CompactString::from(s.to_string())))
        }
        _ => Err(PyException::value_error(format!("unknown encoding: {}", encoding))),
    }
}

fn codecs_lookup(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.lookup", args, 1)?;
    let norm = normalize_encoding(&args[0].py_to_string());
    let enc = resolve_encoding(&norm);
    let known = matches!(enc,
        "utf_8" | "ascii" | "latin_1"
        | "utf_16" | "utf_16_le" | "utf_16_be"
        | "utf_32" | "utf_32_le" | "utf_32_be"
        | "cp1252" | "rot_13"
        | "iso8859_2" | "iso8859_3" | "iso8859_4" | "iso8859_5"
        | "iso8859_6" | "iso8859_7" | "iso8859_8" | "iso8859_9"
        | "iso8859_10" | "iso8859_11" | "iso8859_13" | "iso8859_14"
        | "iso8859_15" | "iso8859_16"
        | "cp437" | "cp850" | "cp866" | "cp874" | "cp932"
        | "cp949" | "cp950" | "cp1250" | "cp1251" | "cp1253"
        | "cp1254" | "cp1255" | "cp1256" | "cp1257" | "cp1258"
        | "big5" | "big5hkscs" | "euc_jp" | "euc_kr" | "euc_cn"
        | "gb2312" | "gbk" | "gb18030" | "hz"
        | "shift_jis" | "shift_jis_2004" | "shift_jisx0213"
        | "iso2022_jp" | "iso2022_jp_2" | "iso2022_kr" | "iso2022_cn"
        | "koi8_r" | "koi8_u" | "koi8_t"
        | "mac_roman" | "mac_cyrillic" | "mac_greek" | "mac_latin2"
        | "johab" | "tis_620" | "viscii"
    );
    if known {
        // Return a CodecInfo-like object with .name attribute (CPython compat)
        let display_name = enc.replace('_', "-");
        let cls = PyObject::class(CompactString::from("CodecInfo"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(display_name.as_str())));
        attrs.insert(CompactString::from("encode"), make_builtin(codecs_encode));
        attrs.insert(CompactString::from("decode"), make_builtin(codecs_decode));
        attrs.insert(CompactString::from("incrementalencoder"), PyObject::none());
        attrs.insert(CompactString::from("incrementaldecoder"), PyObject::none());
        attrs.insert(CompactString::from("streamreader"), PyObject::none());
        attrs.insert(CompactString::from("streamwriter"), PyObject::none());
        // Also support tuple-like indexing (CPython CodecInfo is a 4-tuple subclass)
        let enc_fn = make_builtin(codecs_encode);
        let dec_fn = make_builtin(codecs_decode);
        attrs.insert(CompactString::from("__getitem__"), PyObject::native_closure(
            "CodecInfo.__getitem__",
            {
                let enc2 = enc_fn.clone();
                let dec2 = dec_fn.clone();
                let name2 = CompactString::from(display_name.as_str());
                move |gargs: &[PyObjectRef]| {
                    let idx = if !gargs.is_empty() { gargs[0].as_int().unwrap_or(0) } else { 0 };
                    match idx {
                        0 => Ok(PyObject::str_val(name2.clone())),
                        1 => Ok(enc2.clone()),
                        2 => Ok(dec2.clone()),
                        3 => Ok(PyObject::none()),
                        _ => Err(PyException::index_error("CodecInfo index out of range")),
                    }
                }
            }
        ));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    } else {
        Err(PyException::lookup_error(format!("unknown encoding: {}", norm)))
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
        let filename = if !args.is_empty() { args[0].py_to_string() } else { "shelf.db".to_string() };
        let cls = PyObject::class(CompactString::from("Shelf"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let data: Rc<PyCell<FxHashKeyMap>> = ferrython_core::object::alloc_map_inner();
            let file_path = Arc::new(filename.clone());

            // Load existing data from file if it exists
            if let Ok(bytes) = std::fs::read(&*file_path) {
                if let Ok(loaded) = pickle_loads_stack(&bytes) {
                    if let PyObjectPayload::Dict(ref dict_data) = loaded.payload {
                        let mut store = data.write();
                        for (k, v) in dict_data.read().iter() {
                            store.insert(k.clone(), v.clone());
                        }
                    }
                }
            }

            let d1 = data.clone();
            w.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                "Shelf.__getitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__getitem__", args, 1)?;
                    let key = HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    d1.read().get(&key).cloned().ok_or_else(|| PyException::key_error(args[0].py_to_string()))
                }
            ));

            let d2 = data.clone();
            w.insert(CompactString::from("__setitem__"), PyObject::native_closure(
                "Shelf.__setitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__setitem__", args, 2)?;
                    let key = HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    d2.write().insert(key, args[1].clone());
                    Ok(PyObject::none())
                }
            ));

            let d2b = data.clone();
            w.insert(CompactString::from("__delitem__"), PyObject::native_closure(
                "Shelf.__delitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__delitem__", args, 1)?;
                    let key = HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    match d2b.write().swap_remove(&key) {
                        Some(_) => Ok(PyObject::none()),
                        None => Err(PyException::key_error(args[0].py_to_string())),
                    }
                }
            ));

            let d3 = data.clone();
            w.insert(CompactString::from("__contains__"), PyObject::native_closure(
                "Shelf.__contains__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__contains__", args, 1)?;
                    let key = HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    Ok(PyObject::bool_val(d3.read().contains_key(&key)))
                }
            ));

            let d4 = data.clone();
            w.insert(CompactString::from("keys"), PyObject::native_closure(
                "Shelf.keys", move |_: &[PyObjectRef]| {
                    let keys: Vec<PyObjectRef> = d4.read().keys().map(|k| match k {
                        HashableKey::Str(s) => PyObject::str_val(s.as_ref().clone()),
                        _ => PyObject::none(),
                    }).collect();
                    Ok(PyObject::list(keys))
                }
            ));

            let d4b = data.clone();
            w.insert(CompactString::from("values"), PyObject::native_closure(
                "Shelf.values", move |_: &[PyObjectRef]| {
                    let vals: Vec<PyObjectRef> = d4b.read().values().cloned().collect();
                    Ok(PyObject::list(vals))
                }
            ));

            let d4c = data.clone();
            w.insert(CompactString::from("items"), PyObject::native_closure(
                "Shelf.items", move |_: &[PyObjectRef]| {
                    let items: Vec<PyObjectRef> = d4c.read().iter().map(|(k, v)| {
                        let key = match k {
                            HashableKey::Str(s) => PyObject::str_val(s.as_ref().clone()),
                            _ => PyObject::none(),
                        };
                        PyObject::tuple(vec![key, v.clone()])
                    }).collect();
                    Ok(PyObject::list(items))
                }
            ));

            let d5 = data.clone();
            w.insert(CompactString::from("__len__"), PyObject::native_closure(
                "Shelf.__len__", move |_: &[PyObjectRef]| {
                    Ok(PyObject::int(d5.read().len() as i64))
                }
            ));

            let d6 = data.clone();
            w.insert(CompactString::from("get"), PyObject::native_closure(
                "Shelf.get", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.get", args, 1)?;
                    let key = HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                    Ok(d6.read().get(&key).cloned().unwrap_or(default))
                }
            ));

            // sync() / close() — persist to disk
            let sync_data = data.clone();
            let sync_path = file_path.clone();
            let sync_fn = move || -> PyResult<()> {
                let store = sync_data.read();
                let dict = PyObject::dict(store.clone());
                let mut buf = Vec::new();
                pickle_serialize(&dict, &mut buf)?;
                std::fs::write(&**sync_path, &buf)
                    .map_err(|e| PyException::runtime_error(format!("shelve.sync: {}", e)))?;
                Ok(())
            };
            let sf1 = sync_fn.clone();
            w.insert(CompactString::from("sync"), PyObject::native_closure(
                "Shelf.sync", move |_: &[PyObjectRef]| { sf1()?; Ok(PyObject::none()) }
            ));
            let sf2 = sync_fn.clone();
            w.insert(CompactString::from("close"), PyObject::native_closure(
                "Shelf.close", move |_: &[PyObjectRef]| { sf2()?; Ok(PyObject::none()) }
            ));

            let ir = inst.clone();
            w.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "Shelf.__enter__", move |_: &[PyObjectRef]| Ok(ir.clone())
            ));
            let sf3 = sync_fn;
            w.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "Shelf.__exit__", move |_: &[PyObjectRef]| { let _ = sf3(); Ok(PyObject::bool_val(false)) }
            ));
        }
        Ok(inst)
    });

    make_module("shelve", vec![
        ("open", open_fn),
    ])
}

/// dbm module — simple key-value database stub (in-memory, not persistent)
pub fn create_dbm_module() -> PyObjectRef {
    let open_fn = make_builtin(|args: &[PyObjectRef]| {
        let filename = if !args.is_empty() { args[0].py_to_string().to_string() } else { "db".to_string() };
        let flag = if args.len() >= 2 { args[1].py_to_string().to_string() } else { "r".to_string() };

        // Add .db extension if no extension present
        let db_path = if filename.contains('.') { filename.clone() } else { format!("{}.db", filename) };

        // Load existing data from disk
        let mut initial_data = new_fx_hashkey_map();
        if flag != "n" {
            if let Ok(content) = std::fs::read(&db_path) {
                // Simple format: length-prefixed key-value pairs
                // [4 bytes key_len][key bytes][4 bytes val_len][val bytes] ...
                let mut pos = 0;
                while pos + 4 <= content.len() {
                    let kl = u32::from_le_bytes([content[pos], content[pos+1], content[pos+2], content[pos+3]]) as usize;
                    pos += 4;
                    if pos + kl > content.len() { break; }
                    let key = String::from_utf8_lossy(&content[pos..pos+kl]).to_string();
                    pos += kl;
                    if pos + 4 > content.len() { break; }
                    let vl = u32::from_le_bytes([content[pos], content[pos+1], content[pos+2], content[pos+3]]) as usize;
                    pos += 4;
                    if pos + vl > content.len() { break; }
                    let val = content[pos..pos+vl].to_vec();
                    pos += vl;
                    initial_data.insert(
                        HashableKey::str_key(CompactString::from(key.as_str())),
                        PyObject::bytes(val),
                    );
                }
            } else if flag == "r" {
                return Err(PyException::os_error(format!("No such file: '{}'", db_path)));
            }
        }

        let data: Rc<PyCell<FxHashKeyMap>> = Rc::new(PyCell::new(initial_data));
        let path_for_sync = Arc::new(db_path.clone());

        let cls = PyObject::class(CompactString::from("_Database"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("_path"), PyObject::str_val(CompactString::from(db_path.as_str())));

            let d1 = data.clone();
            w.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                "dbm.__getitem__", move |args: &[PyObjectRef]| {
                    check_args_min("dbm.__getitem__", args, 1)?;
                    let key = HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    d1.read().get(&key).cloned().ok_or_else(|| PyException::key_error(args[0].py_to_string()))
                }));
            let d2 = data.clone();
            let p2 = path_for_sync.clone();
            w.insert(CompactString::from("__setitem__"), PyObject::native_closure(
                "dbm.__setitem__", move |args: &[PyObjectRef]| {
                    check_args_min("dbm.__setitem__", args, 2)?;
                    let key_str = args[0].py_to_string();
                    let key = HashableKey::str_key(CompactString::from(key_str.as_str()));
                    // Convert value to bytes if it's a string
                    let val = match &args[1].payload {
                        PyObjectPayload::Bytes(b) => PyObject::bytes((**b).clone()),
                        _ => PyObject::bytes(args[1].py_to_string().as_bytes().to_vec()),
                    };
                    d2.write().insert(key, val);
                    // Sync to disk
                    sync_dbm_to_disk(&d2, &p2);
                    Ok(PyObject::none())
                }));
            let d3 = data.clone();
            w.insert(CompactString::from("__contains__"), PyObject::native_closure(
                "dbm.__contains__", move |args: &[PyObjectRef]| {
                    check_args_min("dbm.__contains__", args, 1)?;
                    let key = HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    Ok(PyObject::bool_val(d3.read().contains_key(&key)))
                }));
            let d4 = data.clone();
            w.insert(CompactString::from("keys"), PyObject::native_closure(
                "dbm.keys", move |_args: &[PyObjectRef]| {
                    let keys: Vec<PyObjectRef> = d4.read().keys().map(|k| match k {
                        HashableKey::Str(s) => PyObject::str_val(s.as_ref().clone()),
                        _ => PyObject::str_val(CompactString::from(format!("{:?}", k))),
                    }).collect();
                    Ok(PyObject::list(keys))
                }));
            let d5 = data.clone();
            w.insert(CompactString::from("values"), PyObject::native_closure(
                "dbm.values", move |_args: &[PyObjectRef]| {
                    let vals: Vec<PyObjectRef> = d5.read().values().cloned().collect();
                    Ok(PyObject::list(vals))
                }));
            let d6 = data.clone();
            w.insert(CompactString::from("__len__"), PyObject::native_closure(
                "dbm.__len__", move |_args: &[PyObjectRef]| {
                    Ok(PyObject::int(d6.read().len() as i64))
                }));
            let d7 = data.clone();
            let p7 = path_for_sync.clone();
            w.insert(CompactString::from("__delitem__"), PyObject::native_closure(
                "dbm.__delitem__", move |args: &[PyObjectRef]| {
                    check_args_min("dbm.__delitem__", args, 1)?;
                    let key = HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    if d7.write().shift_remove(&key).is_none() {
                        return Err(PyException::key_error(args[0].py_to_string()));
                    }
                    sync_dbm_to_disk(&d7, &p7);
                    Ok(PyObject::none())
                }));
            let d8 = data.clone();
            let p8 = path_for_sync.clone();
            w.insert(CompactString::from("sync"), PyObject::native_closure(
                "dbm.sync", move |_args: &[PyObjectRef]| {
                    sync_dbm_to_disk(&d8, &p8);
                    Ok(PyObject::none())
                }));
            let d9 = data.clone();
            let p9 = path_for_sync.clone();
            w.insert(CompactString::from("close"), PyObject::native_closure(
                "dbm.close", move |_args: &[PyObjectRef]| {
                    sync_dbm_to_disk(&d9, &p9);
                    Ok(PyObject::none())
                }));
            w.insert(CompactString::from("__enter__"), make_builtin(|args: &[PyObjectRef]| {
                check_args_min("dbm.__enter__", args, 1)?; Ok(args[0].clone())
            }));
            let d10 = data.clone();
            let p10 = path_for_sync.clone();
            w.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "dbm.__exit__", move |_args: &[PyObjectRef]| {
                    sync_dbm_to_disk(&d10, &p10);
                    Ok(PyObject::bool_val(false))
                }));
        }
        Ok(inst)
    });

    make_module("dbm", vec![
        ("open", open_fn),
        ("error", PyObject::str_val(CompactString::from("dbm.error"))),
    ])
}

fn sync_dbm_to_disk(data: &Rc<PyCell<FxHashKeyMap>>, path: &str) {
    let guard = data.read();
    let mut buf = Vec::new();
    for (k, v) in guard.iter() {
        let key_bytes = match k {
            HashableKey::Str(s) => s.as_bytes().to_vec(),
            _ => format!("{:?}", k).into_bytes(),
        };
        let val_bytes = match &v.payload {
            PyObjectPayload::Bytes(b) => (**b).clone(),
            _ => v.py_to_string().as_bytes().to_vec(),
        };
        buf.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(&key_bytes);
        buf.extend_from_slice(&(val_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(&val_bytes);
    }
    let _ = std::fs::write(path, &buf);
}

/// `marshal` — internal Python object serialization
pub fn create_marshal_module() -> PyObjectRef {
    let dumps_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("marshal.dumps", args, 1)?;
        fn marshal_encode(obj: &PyObjectRef) -> Vec<u8> {
            match &obj.payload {
                PyObjectPayload::None => vec![b'N'],
                PyObjectPayload::Bool(b) => {
                    if *b { vec![b'T'] } else { vec![b'F'] }
                }
                PyObjectPayload::Int(n) => {
                    let mut buf = vec![b'i'];
                    buf.extend_from_slice(&n.to_i64().unwrap_or(0).to_le_bytes());
                    buf
                }
                PyObjectPayload::Float(f) => {
                    let mut buf = vec![b'g'];
                    buf.extend_from_slice(&f.to_le_bytes());
                    buf
                }
                PyObjectPayload::Str(s) => {
                    let bytes = s.as_bytes();
                    let mut buf = vec![b's'];
                    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    buf.extend_from_slice(bytes);
                    buf
                }
                PyObjectPayload::Bytes(b) => {
                    let mut buf = vec![b'z'];
                    buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
                    buf.extend_from_slice(b);
                    buf
                }
                PyObjectPayload::List(items) => {
                    let items = items.read();
                    let mut buf = vec![b'['];
                    buf.extend_from_slice(&(items.len() as u32).to_le_bytes());
                    for item in items.iter() { buf.extend(marshal_encode(item)); }
                    buf
                }
                PyObjectPayload::Tuple(items) => {
                    let mut buf = vec![b'('];
                    buf.extend_from_slice(&(items.len() as u32).to_le_bytes());
                    for item in items.iter() { buf.extend(marshal_encode(item)); }
                    buf
                }
                PyObjectPayload::Dict(map) => {
                    let map = map.read();
                    let mut buf = vec![b'{'];
                    buf.extend_from_slice(&(map.len() as u32).to_le_bytes());
                    for (k, v) in map.iter() {
                        let key_obj = match k {
                            HashableKey::Str(s) => PyObject::str_val(s.as_ref().clone()),
                            HashableKey::Int(n) => PyObject::int(n.to_i64().unwrap_or(0)),
                            HashableKey::Bool(b) => PyObject::bool_val(*b),
                            _ => PyObject::none(),
                        };
                        buf.extend(marshal_encode(&key_obj));
                        buf.extend(marshal_encode(v));
                    }
                    buf
                }
                _ => vec![b'N'],
            }
        }
        Ok(PyObject::bytes(marshal_encode(&args[0])))
    });
    let loads_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("marshal.loads", args, 1)?;
        let data = match &args[0].payload {
            PyObjectPayload::Bytes(b) => (**b).clone(),
            _ => return Err(PyException::type_error("marshal.loads requires bytes")),
        };
        fn marshal_decode(data: &[u8], pos: &mut usize) -> PyResult<PyObjectRef> {
            if *pos >= data.len() { return Err(PyException::value_error("marshal: truncated data")); }
            let tag = data[*pos]; *pos += 1;
            match tag {
                b'N' => Ok(PyObject::none()),
                b'T' => Ok(PyObject::bool_val(true)),
                b'F' => Ok(PyObject::bool_val(false)),
                b'i' => {
                    if *pos + 8 > data.len() { return Err(PyException::value_error("marshal: truncated")); }
                    let n = i64::from_le_bytes(data[*pos..*pos+8].try_into().unwrap()); *pos += 8;
                    Ok(PyObject::int(n))
                }
                b'g' => {
                    if *pos + 8 > data.len() { return Err(PyException::value_error("marshal: truncated")); }
                    let f = f64::from_le_bytes(data[*pos..*pos+8].try_into().unwrap()); *pos += 8;
                    Ok(PyObject::float(f))
                }
                b's' | b'z' => {
                    if *pos + 4 > data.len() { return Err(PyException::value_error("marshal: truncated")); }
                    let len = u32::from_le_bytes(data[*pos..*pos+4].try_into().unwrap()) as usize; *pos += 4;
                    if *pos + len > data.len() { return Err(PyException::value_error("marshal: truncated")); }
                    let slice = data[*pos..*pos+len].to_vec(); *pos += len;
                    if tag == b's' {
                        Ok(PyObject::str_val(CompactString::from(String::from_utf8_lossy(&slice).as_ref())))
                    } else {
                        Ok(PyObject::bytes(slice))
                    }
                }
                b'[' | b'(' => {
                    if *pos + 4 > data.len() { return Err(PyException::value_error("marshal: truncated")); }
                    let len = u32::from_le_bytes(data[*pos..*pos+4].try_into().unwrap()) as usize; *pos += 4;
                    let mut items = Vec::with_capacity(len);
                    for _ in 0..len { items.push(marshal_decode(data, pos)?); }
                    if tag == b'[' { Ok(PyObject::list(items)) } else { Ok(PyObject::tuple(items)) }
                }
                b'{' => {
                    if *pos + 4 > data.len() { return Err(PyException::value_error("marshal: truncated")); }
                    let len = u32::from_le_bytes(data[*pos..*pos+4].try_into().unwrap()) as usize; *pos += 4;
                    let mut map = IndexMap::new();
                    for _ in 0..len {
                        let k = marshal_decode(data, pos)?;
                        let v = marshal_decode(data, pos)?;
                        let key = match &k.payload {
                            PyObjectPayload::Str(s) => HashableKey::str_key(s.to_compact_string()),
                            PyObjectPayload::Int(n) => HashableKey::Int(n.clone()),
                            PyObjectPayload::Bool(b) => HashableKey::Bool(*b),
                            _ => HashableKey::str_key(CompactString::from(k.py_to_string())),
                        };
                        map.insert(key, v);
                    }
                    Ok(PyObject::dict(map))
                }
                _ => Err(PyException::value_error(format!("marshal: unknown tag {}", tag))),
            }
        }
        let mut pos = 0;
        marshal_decode(&data, &mut pos)
    });
    make_module("marshal", vec![
        ("dumps", dumps_fn), ("loads", loads_fn),
        ("dump", make_builtin(|_| Err(PyException::type_error("marshal.dump() requires a file object")))),
        ("load", make_builtin(|_| Err(PyException::type_error("marshal.load() requires a file object")))),
        ("version", PyObject::int(4)),
    ])
}
