//! Punycode helpers for string and bytes codecs.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectRef};

fn punycode_digit(d: u32) -> u8 {
    if d < 26 {
        b'a' + d as u8
    } else {
        b'0' + (d as u8 - 26)
    }
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

pub(crate) fn punycode_encode_str(s: &str) -> PyResult<PyObjectRef> {
    let mut output = Vec::new();
    let mut basic_count = 0u32;
    for ch in s.chars() {
        if ch.is_ascii() {
            output.push(ch as u8);
            basic_count += 1;
        }
    }
    // RFC 3492: always output delimiter when basic code points exist
    if basic_count > 0 {
        output.push(b'-');
    }
    let mut n: u32 = 128;
    let mut delta: u32 = 0;
    let mut bias: u32 = 72;
    let mut h = basic_count;
    let all_chars: Vec<u32> = s.chars().map(|c| c as u32).collect();
    let total = all_chars.len() as u32;
    while h < total {
        let m = *all_chars.iter().filter(|&&cp| cp >= n).min().unwrap_or(&n);
        delta = delta.wrapping_add((m - n).wrapping_mul(h + 1));
        n = m;
        for &cp in &all_chars {
            if cp < n {
                delta = delta.wrapping_add(1);
            }
            if cp == n {
                let mut q = delta;
                let mut k = 36u32;
                loop {
                    let t = if k <= bias {
                        1
                    } else if k >= bias + 26 {
                        26
                    } else {
                        k - bias
                    };
                    if q < t {
                        break;
                    }
                    let digit = t + (q - t) % (36 - t);
                    output.push(punycode_digit(digit));
                    q = (q - t) / (36 - t);
                    k += 36;
                }
                output.push(punycode_digit(q));
                bias = punycode_adapt(delta, h + 1, h == basic_count);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Ok(PyObject::bytes(output))
}

pub(crate) fn punycode_decode_bytes(bytes: &[u8]) -> PyResult<PyObjectRef> {
    let input = std::str::from_utf8(bytes)
        .map_err(|_| PyException::value_error("punycode: invalid input"))?;
    let (basic_part, encoded_part) = if let Some(pos) = input.rfind('-') {
        (&input[..pos], &input[pos + 1..])
    } else {
        ("", input)
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
            if idx >= encoded_bytes.len() {
                break;
            }
            let byte = encoded_bytes[idx];
            idx += 1;
            let digit = match byte {
                b'a'..=b'z' => (byte - b'a') as u32,
                b'A'..=b'Z' => (byte - b'A') as u32,
                b'0'..=b'9' => (byte - b'0') as u32 + 26,
                _ => return Err(PyException::value_error("punycode: bad input")),
            };
            i = i.wrapping_add(digit.wrapping_mul(w));
            let t = if k <= bias {
                1
            } else if k >= bias + 26 {
                26
            } else {
                k - bias
            };
            if digit < t {
                break;
            }
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
