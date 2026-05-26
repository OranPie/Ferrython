use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

// ── pprint module ──

pub fn create_struct_module() -> PyObjectRef {
    make_module(
        "struct",
        vec![
            ("pack", make_builtin(struct_pack)),
            ("unpack", make_builtin(struct_unpack)),
            ("pack_into", make_builtin(struct_pack_into)),
            ("unpack_from", make_builtin(struct_unpack_from)),
            ("iter_unpack", make_builtin(struct_iter_unpack)),
            ("calcsize", make_builtin(struct_calcsize)),
            ("Struct", make_builtin(struct_struct_ctor)),
            (
                "error",
                PyObject::class(CompactString::from("error"), vec![], IndexMap::new()),
            ),
        ],
    )
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
        w.insert(
            CompactString::from("format"),
            PyObject::str_val(CompactString::from(&fmt_str)),
        );
        // Compute size
        let size_obj = struct_calcsize(&[PyObject::str_val(CompactString::from(&fmt_str))])?;
        w.insert(CompactString::from("size"), size_obj);
        let fmt_for_pack = fmt_str.clone();
        w.insert(
            CompactString::from("pack"),
            PyObject::native_closure("pack", move |args| {
                let mut full_args = vec![PyObject::str_val(CompactString::from(&fmt_for_pack))];
                full_args.extend_from_slice(args);
                struct_pack(&full_args)
            }),
        );
        let fmt_for_unpack = fmt_str.clone();
        w.insert(
            CompactString::from("unpack"),
            PyObject::native_closure("unpack", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("Struct.unpack() requires a buffer"));
                }
                struct_unpack(&[
                    PyObject::str_val(CompactString::from(&fmt_for_unpack)),
                    args[0].clone(),
                ])
            }),
        );
        let fmt_for_pi = fmt_str.clone();
        w.insert(
            CompactString::from("pack_into"),
            PyObject::native_closure("pack_into", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "Struct.pack_into() requires buffer, offset, and values",
                    ));
                }
                let mut full_args = vec![PyObject::str_val(CompactString::from(&fmt_for_pi))];
                full_args.extend_from_slice(args);
                struct_pack_into(&full_args)
            }),
        );
        let fmt_for_uf = fmt_str.clone();
        w.insert(
            CompactString::from("unpack_from"),
            PyObject::native_closure("unpack_from", move |args| {
                let mut full_args = vec![PyObject::str_val(CompactString::from(&fmt_for_uf))];
                full_args.extend_from_slice(args);
                struct_unpack_from(&full_args)
            }),
        );
        let fmt_for_iu = fmt_str;
        w.insert(
            CompactString::from("iter_unpack"),
            PyObject::native_closure("iter_unpack", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "Struct.iter_unpack() requires a buffer",
                    ));
                }
                struct_iter_unpack(&[
                    PyObject::str_val(CompactString::from(&fmt_for_iu)),
                    args[0].clone(),
                ])
            }),
        );
    }
    Ok(inst)
}

fn struct_calcsize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("calcsize requires format string"));
    }
    let fmt = args[0].py_to_string();
    let mut size = 0usize;
    let mut chars = fmt.chars().peekable();
    // Skip byte order
    if let Some(&c) = chars.peek() {
        if "<>!=@".contains(c) {
            chars.next();
        }
    }
    while let Some(c) = chars.next() {
        let count = if c.is_ascii_digit() {
            let mut n = (c as u8 - b'0') as usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    n = n * 10 + (d as u8 - b'0') as usize;
                    chars.next();
                } else {
                    break;
                }
            }
            let fc = chars.next().unwrap_or('x');
            size += n * format_char_size(fc);
            continue;
        } else {
            1
        };
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
        if frac == 0 {
            return f32::from_bits(sign);
        }
        // Subnormal
        let mut e = 1u32;
        let mut f = frac;
        while f & 0x0400 == 0 {
            f <<= 1;
            e += 1;
        }
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
            n.to_u64()
                .ok_or_else(|| PyException::overflow_error("int too large for unsigned 64-bit"))
        }
        PyObjectPayload::Bool(b) => Ok(if *b { 1 } else { 0 }),
        _ => Err(PyException::type_error("required integer")),
    }
}

fn struct_pack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("pack requires format string"));
    }
    let fmt = args[0].py_to_string();
    let mut result = Vec::new();
    let mut arg_idx = 1;
    let mut chars = fmt.chars().peekable();
    let little_endian = match chars.peek() {
        Some('<') => {
            chars.next();
            true
        }
        Some('>') | Some('!') => {
            chars.next();
            false
        }
        Some('=') | Some('@') => {
            chars.next();
            cfg!(target_endian = "little")
        }
        _ => cfg!(target_endian = "little"),
    };
    while let Some(c) = chars.next() {
        // Parse optional repeat count
        let count = if c.is_ascii_digit() {
            let mut n = (c as u8 - b'0') as usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    n = n * 10 + (d as u8 - b'0') as usize;
                    chars.next();
                } else {
                    break;
                }
            }
            let fc = match chars.next() {
                Some(fc) => fc,
                None => break,
            };
            pack_one_format(fc, n, &args, &mut arg_idx, &mut result, little_endian)?;
            continue;
        } else {
            1usize
        };
        pack_one_format(c, count, &args, &mut arg_idx, &mut result, little_endian)?;
    }
    Ok(PyObject::bytes(result))
}

fn pack_one_format(
    c: char,
    count: usize,
    args: &[PyObjectRef],
    arg_idx: &mut usize,
    result: &mut Vec<u8>,
    little_endian: bool,
) -> PyResult<()> {
    match c {
        's' => {
            if *arg_idx >= args.len() {
                return Err(PyException::type_error("not enough args"));
            }
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
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                let val = args[*arg_idx].to_int()? as u8;
                result.push(val);
                *arg_idx += 1;
            }
        }
        'h' | 'H' => {
            for _ in 0..count {
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                let val = args[*arg_idx].to_int()? as u16;
                let bytes = if little_endian {
                    val.to_le_bytes()
                } else {
                    val.to_be_bytes()
                };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'i' | 'I' | 'l' | 'L' => {
            for _ in 0..count {
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                let val = args[*arg_idx].to_int()? as u32;
                let bytes = if little_endian {
                    val.to_le_bytes()
                } else {
                    val.to_be_bytes()
                };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'q' | 'Q' => {
            for _ in 0..count {
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                let bytes = if c == 'Q' {
                    // Unsigned 64-bit: extract as u64 (handles values > i64::MAX)
                    let val = extract_u64(&args[*arg_idx])?;
                    if little_endian {
                        val.to_le_bytes()
                    } else {
                        val.to_be_bytes()
                    }
                } else {
                    // Signed 64-bit
                    let val = args[*arg_idx].to_int()?;
                    if little_endian {
                        val.to_le_bytes()
                    } else {
                        val.to_be_bytes()
                    }
                };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'f' => {
            for _ in 0..count {
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                let val = args[*arg_idx].to_float()? as f32;
                let bytes = if little_endian {
                    val.to_le_bytes()
                } else {
                    val.to_be_bytes()
                };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'd' => {
            for _ in 0..count {
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                let val = args[*arg_idx].to_float()?;
                let bytes = if little_endian {
                    val.to_le_bytes()
                } else {
                    val.to_be_bytes()
                };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        '?' => {
            for _ in 0..count {
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                result.push(if args[*arg_idx].is_truthy() { 1 } else { 0 });
                *arg_idx += 1;
            }
        }
        'x' => {
            for _ in 0..count {
                result.push(0);
            }
        }
        'c' => {
            for _ in 0..count {
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                let b = match &args[*arg_idx].payload {
                    PyObjectPayload::Bytes(v) if v.len() == 1 => v[0],
                    _ => {
                        return Err(PyException::type_error(
                            "char format requires a bytes object of length 1",
                        ))
                    }
                };
                result.push(b);
                *arg_idx += 1;
            }
        }
        'p' => {
            // Pascal string: first byte is length, then data, padded to `count` bytes total
            if *arg_idx >= args.len() {
                return Err(PyException::type_error("not enough args"));
            }
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
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                let val = args[*arg_idx].to_float()? as f32;
                let half = f32_to_f16(val);
                let bytes = if little_endian {
                    half.to_le_bytes()
                } else {
                    half.to_be_bytes()
                };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        'n' | 'N' | 'P' => {
            for _ in 0..count {
                if *arg_idx >= args.len() {
                    return Err(PyException::type_error("not enough args"));
                }
                let val = args[*arg_idx].to_int()? as usize;
                let bytes = if little_endian {
                    val.to_le_bytes()
                } else {
                    val.to_be_bytes()
                };
                result.extend_from_slice(&bytes);
                *arg_idx += 1;
            }
        }
        _ => {}
    }
    Ok(())
}

fn struct_unpack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "unpack requires format string and bytes",
        ));
    }
    let fmt = args[0].py_to_string();
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        _ => return Err(PyException::type_error("unpack requires bytes argument")),
    };
    // Validate buffer length
    let expected_size = struct_calcsize(&[args[0].clone()])?.as_int().unwrap_or(0) as usize;
    if data.len() < expected_size {
        return Err(PyException::runtime_error(format!(
            "unpack requires a buffer of at least {} bytes (got {})",
            expected_size,
            data.len()
        )));
    }
    let mut result = Vec::new();
    let mut offset = 0;
    let mut chars = fmt.chars().peekable();
    let little_endian = match chars.peek() {
        Some('<') => {
            chars.next();
            true
        }
        Some('>') | Some('!') => {
            chars.next();
            false
        }
        Some('=') | Some('@') => {
            chars.next();
            cfg!(target_endian = "little")
        }
        _ => cfg!(target_endian = "little"),
    };
    while let Some(c) = chars.next() {
        // Parse optional repeat count
        let count = if c.is_ascii_digit() {
            let mut n = (c as u8 - b'0') as usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    n = n * 10 + (d as u8 - b'0') as usize;
                    chars.next();
                } else {
                    break;
                }
            }
            let fc = match chars.next() {
                Some(fc) => fc,
                None => break,
            };
            unpack_one_format(fc, n, &data, &mut offset, &mut result, little_endian);
            continue;
        } else {
            1usize
        };
        unpack_one_format(c, count, &data, &mut offset, &mut result, little_endian);
    }
    Ok(PyObject::tuple(result))
}

/// struct.pack_into(fmt, buffer, offset, v1, v2, ...)
fn struct_pack_into(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "pack_into requires format, buffer, offset, and values",
        ));
    }
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
                return Err(PyException::runtime_error(
                    "pack_into: offset + size exceeds buffer",
                ));
            }
            let ptr = buf.as_ptr() as *mut u8;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    packed_bytes.as_ptr(),
                    ptr.add(offset),
                    packed_bytes.len(),
                );
            }
            Ok(PyObject::none())
        }
        PyObjectPayload::Bytes(buf) => {
            if offset + packed_bytes.len() > buf.len() {
                return Err(PyException::runtime_error(
                    "pack_into: offset + size exceeds buffer",
                ));
            }
            let ptr = buf.as_ptr() as *mut u8;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    packed_bytes.as_ptr(),
                    ptr.add(offset),
                    packed_bytes.len(),
                );
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::type_error(
            "pack_into requires a writable buffer (bytearray)",
        )),
    }
}

/// struct.unpack_from(fmt, buffer, offset=0)
fn struct_unpack_from(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "unpack_from requires format and buffer",
        ));
    }
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        PyObjectPayload::ByteArray(b) => (**b).clone(),
        _ => return Err(PyException::type_error("unpack_from requires bytes buffer")),
    };
    let start_offset = if args.len() > 2 {
        args[2].as_int().unwrap_or(0) as usize
    } else {
        0
    };
    if start_offset > data.len() {
        return Err(PyException::runtime_error(
            "unpack_from offset out of range",
        ));
    }
    let sliced = &data[start_offset..];
    struct_unpack(&[args[0].clone(), PyObject::bytes(sliced.to_vec())])
}

/// struct.iter_unpack(fmt, buffer) → iterator of tuples
fn struct_iter_unpack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "iter_unpack requires format and buffer",
        ));
    }
    let fmt_obj = &args[0];
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        _ => return Err(PyException::type_error("iter_unpack requires bytes buffer")),
    };
    let size = struct_calcsize(&[fmt_obj.clone()])?.as_int().unwrap_or(0) as usize;
    if size == 0 {
        return Err(PyException::runtime_error(
            "iter_unpack format has zero size",
        ));
    }
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

fn unpack_one_format(
    c: char,
    count: usize,
    data: &[u8],
    offset: &mut usize,
    result: &mut Vec<PyObjectRef>,
    little_endian: bool,
) {
    match c {
        's' => {
            if *offset + count > data.len() {
                return;
            }
            let slice = data[*offset..*offset + count].to_vec();
            result.push(PyObject::bytes(slice));
            *offset += count;
        }
        'b' => {
            for _ in 0..count {
                if *offset >= data.len() {
                    break;
                }
                result.push(PyObject::int(data[*offset] as i8 as i64));
                *offset += 1;
            }
        }
        'B' => {
            for _ in 0..count {
                if *offset >= data.len() {
                    break;
                }
                result.push(PyObject::int(data[*offset] as i64));
                *offset += 1;
            }
        }
        'h' => {
            for _ in 0..count {
                if *offset + 2 > data.len() {
                    break;
                }
                let bytes: [u8; 2] = [data[*offset], data[*offset + 1]];
                let val = if little_endian {
                    i16::from_le_bytes(bytes)
                } else {
                    i16::from_be_bytes(bytes)
                };
                result.push(PyObject::int(val as i64));
                *offset += 2;
            }
        }
        'H' => {
            for _ in 0..count {
                if *offset + 2 > data.len() {
                    break;
                }
                let bytes: [u8; 2] = [data[*offset], data[*offset + 1]];
                let val = if little_endian {
                    u16::from_le_bytes(bytes)
                } else {
                    u16::from_be_bytes(bytes)
                };
                result.push(PyObject::int(val as i64));
                *offset += 2;
            }
        }
        'i' | 'l' => {
            for _ in 0..count {
                if *offset + 4 > data.len() {
                    break;
                }
                let bytes: [u8; 4] = [
                    data[*offset],
                    data[*offset + 1],
                    data[*offset + 2],
                    data[*offset + 3],
                ];
                let val = if little_endian {
                    i32::from_le_bytes(bytes)
                } else {
                    i32::from_be_bytes(bytes)
                };
                result.push(PyObject::int(val as i64));
                *offset += 4;
            }
        }
        'I' | 'L' => {
            for _ in 0..count {
                if *offset + 4 > data.len() {
                    break;
                }
                let bytes: [u8; 4] = [
                    data[*offset],
                    data[*offset + 1],
                    data[*offset + 2],
                    data[*offset + 3],
                ];
                let val = if little_endian {
                    u32::from_le_bytes(bytes)
                } else {
                    u32::from_be_bytes(bytes)
                };
                result.push(PyObject::int(val as i64));
                *offset += 4;
            }
        }
        'q' => {
            for _ in 0..count {
                if *offset + 8 > data.len() {
                    break;
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[*offset..*offset + 8]);
                let val = if little_endian {
                    i64::from_le_bytes(bytes)
                } else {
                    i64::from_be_bytes(bytes)
                };
                result.push(PyObject::int(val));
                *offset += 8;
            }
        }
        'Q' => {
            for _ in 0..count {
                if *offset + 8 > data.len() {
                    break;
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[*offset..*offset + 8]);
                let val = if little_endian {
                    u64::from_le_bytes(bytes)
                } else {
                    u64::from_be_bytes(bytes)
                };
                result.push(PyObject::int(val as i64));
                *offset += 8;
            }
        }
        'f' => {
            for _ in 0..count {
                if *offset + 4 > data.len() {
                    break;
                }
                let bytes: [u8; 4] = [
                    data[*offset],
                    data[*offset + 1],
                    data[*offset + 2],
                    data[*offset + 3],
                ];
                let val = if little_endian {
                    f32::from_le_bytes(bytes)
                } else {
                    f32::from_be_bytes(bytes)
                };
                result.push(PyObject::float(val as f64));
                *offset += 4;
            }
        }
        'd' => {
            for _ in 0..count {
                if *offset + 8 > data.len() {
                    break;
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[*offset..*offset + 8]);
                let val = if little_endian {
                    f64::from_le_bytes(bytes)
                } else {
                    f64::from_be_bytes(bytes)
                };
                result.push(PyObject::float(val));
                *offset += 8;
            }
        }
        '?' => {
            for _ in 0..count {
                if *offset >= data.len() {
                    break;
                }
                result.push(PyObject::bool_val(data[*offset] != 0));
                *offset += 1;
            }
        }
        'x' => {
            *offset += count;
        }
        'c' => {
            for _ in 0..count {
                if *offset >= data.len() {
                    break;
                }
                result.push(PyObject::bytes(vec![data[*offset]]));
                *offset += 1;
            }
        }
        'p' => {
            // Pascal string: first byte is length
            if *offset >= data.len() {
                return;
            }
            let str_len = data[*offset] as usize;
            *offset += 1;
            let available = count.saturating_sub(1);
            let actual = str_len.min(available);
            if *offset + available > data.len() {
                return;
            }
            result.push(PyObject::bytes(data[*offset..*offset + actual].to_vec()));
            *offset += available;
        }
        'e' => {
            for _ in 0..count {
                if *offset + 2 > data.len() {
                    break;
                }
                let bytes: [u8; 2] = [data[*offset], data[*offset + 1]];
                let half = if little_endian {
                    u16::from_le_bytes(bytes)
                } else {
                    u16::from_be_bytes(bytes)
                };
                result.push(PyObject::float(f16_to_f32(half) as f64));
                *offset += 2;
            }
        }
        'n' => {
            for _ in 0..count {
                let sz = std::mem::size_of::<isize>();
                if *offset + sz > data.len() {
                    break;
                }
                let mut bytes = [0u8; 8];
                bytes[..sz].copy_from_slice(&data[*offset..*offset + sz]);
                let val = if little_endian {
                    isize::from_le_bytes(bytes[..sz].try_into().unwrap_or([0; 8]))
                } else {
                    isize::from_be_bytes(bytes[..sz].try_into().unwrap_or([0; 8]))
                };
                result.push(PyObject::int(val as i64));
                *offset += sz;
            }
        }
        'N' | 'P' => {
            for _ in 0..count {
                let sz = std::mem::size_of::<usize>();
                if *offset + sz > data.len() {
                    break;
                }
                let mut bytes = [0u8; 8];
                bytes[..sz].copy_from_slice(&data[*offset..*offset + sz]);
                let val = if little_endian {
                    usize::from_le_bytes(bytes[..sz].try_into().unwrap_or([0; 8]))
                } else {
                    usize::from_be_bytes(bytes[..sz].try_into().unwrap_or([0; 8]))
                };
                result.push(PyObject::int(val as i64));
                *offset += sz;
            }
        }
        _ => {}
    }
}
