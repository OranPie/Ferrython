use super::helpers::{arg_or_kw, bool_arg, extract_bytes_like, is_none, split_kwargs};
use super::*;

pub(super) fn base16_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b16encode requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, false, "b16encode")?;
    let hex: String = data.iter().map(|b| format!("{:02X}", b)).collect();
    Ok(PyObject::bytes(hex.into_bytes()))
}

fn hex_value(b: u8, casefold: bool) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' if casefold => Some(b - b'a' + 10),
        _ => None,
    }
}

pub(super) fn base16_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b16decode requires data"));
    }
    let casefold = bool_arg(pos, kwargs.as_ref(), 1, "casefold", false);
    let input = extract_bytes_like(&pos[0], true, false, "b16decode")?;
    if input.len() % 2 != 0 {
        return Err(PyException::value_error("Odd-length string"));
    }
    let mut result = Vec::with_capacity(input.len() / 2);
    for pair in input.chunks(2) {
        let hi = hex_value(pair[0], casefold)
            .ok_or_else(|| PyException::value_error("Non-base16 digit found"))?;
        let lo = hex_value(pair[1], casefold)
            .ok_or_else(|| PyException::value_error("Non-base16 digit found"))?;
        result.push((hi << 4) | lo);
    }
    Ok(PyObject::bytes(result))
}

pub(super) fn base32_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, _) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b32encode requires data"));
    }
    let data = extract_bytes_like(&pos[0], false, false, "b32encode")?;
    Ok(PyObject::bytes(base32_encode_bytes(&data)))
}

fn base32_encode_bytes(data: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut result = Vec::new();
    for chunk in data.chunks(5) {
        let mut buf = [0u8; 5];
        buf[..chunk.len()].copy_from_slice(chunk);
        let b = buf;
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
            1 => 2,
            2 => 4,
            3 => 5,
            4 => 7,
            5 => 8,
            _ => 0,
        };
        for i in 0..num_chars {
            result.push(ALPHABET[indices[i] as usize]);
        }
        for _ in 0..(8 - num_chars) {
            result.push(b'=');
        }
    }
    result
}

fn base32_value(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'2'..=b'7' => Some(b - b'2' + 26),
        _ => None,
    }
}

pub(super) fn base32_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("b32decode requires data"));
    }
    let casefold = bool_arg(pos, kwargs.as_ref(), 1, "casefold", false);
    let map01 = arg_or_kw(pos, kwargs.as_ref(), 2, "map01")
        .filter(|v| !is_none(v))
        .map(|v| extract_bytes_like(&v, true, false, "map01"))
        .transpose()?
        .and_then(|v| v.first().copied());
    let mut input = extract_bytes_like(&pos[0], true, false, "b32decode")?;
    if input.is_empty() {
        return Ok(PyObject::bytes(Vec::new()));
    }
    if input.len() % 8 != 0 {
        return Err(PyException::value_error("Incorrect padding"));
    }
    for b in &mut input {
        if *b == b'0' {
            if map01.is_some() {
                *b = b'O';
            }
        } else if *b == b'1' {
            if let Some(m) = map01 {
                *b = m.to_ascii_uppercase();
            }
        } else if b.is_ascii_lowercase() {
            if casefold {
                *b = b.to_ascii_uppercase();
            }
        }
    }

    let mut result = Vec::new();
    for chunk in input.chunks(8) {
        let pad_count = chunk.iter().filter(|&&b| b == b'=').count();
        if !matches!(pad_count, 0 | 1 | 3 | 4 | 6) {
            return Err(PyException::value_error("Incorrect padding"));
        }
        if pad_count > 0 && chunk[8 - pad_count..].iter().any(|&b| b != b'=') {
            return Err(PyException::value_error("Incorrect padding"));
        }
        let mut vals = [0u8; 8];
        for i in 0..8 - pad_count {
            vals[i] = base32_value(chunk[i])
                .ok_or_else(|| PyException::value_error("Non-base32 digit found"))?;
        }
        let n = ((vals[0] as u64) << 35)
            | ((vals[1] as u64) << 30)
            | ((vals[2] as u64) << 25)
            | ((vals[3] as u64) << 20)
            | ((vals[4] as u64) << 15)
            | ((vals[5] as u64) << 10)
            | ((vals[6] as u64) << 5)
            | (vals[7] as u64);
        let out_bytes = match pad_count {
            6 => 1,
            4 => 2,
            3 => 3,
            1 => 4,
            0 => 5,
            _ => unreachable!(),
        };
        if out_bytes >= 1 {
            result.push((n >> 32) as u8);
        }
        if out_bytes >= 2 {
            result.push((n >> 24) as u8);
        }
        if out_bytes >= 3 {
            result.push((n >> 16) as u8);
        }
        if out_bytes >= 4 {
            result.push((n >> 8) as u8);
        }
        if out_bytes >= 5 {
            result.push(n as u8);
        }
    }
    Ok(PyObject::bytes(result))
}
