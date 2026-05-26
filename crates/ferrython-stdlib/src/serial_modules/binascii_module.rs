use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{
    check_args, check_args_min, make_builtin, make_module, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};

use super::base64_module::{b64_decode_bytes, b64_encode_bytes, extract_bytes, extract_bytes_like};

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
            let byte = u8::from_str_radix(&hex_str[i..i + 2], 16)
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
        } else {
            0
        };
        crc = !crc;
        for &byte in &data {
            crc ^= byte as u32;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB88320;
                } else {
                    crc >>= 1;
                }
            }
        }
        Ok(PyObject::int(!crc as i64))
    });

    let b2a_base64_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("b2a_base64", args, 1)?;
        let data = extract_bytes_like(&args[0], false, false, "b2a_base64")?;
        let mut result = b64_encode_bytes(&data);
        result.push(b'\n');
        Ok(PyObject::bytes(result))
    });

    let a2b_base64_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("a2b_base64", args, 1)?;
        let input = extract_bytes_like(&args[0], true, false, "a2b_base64")?;
        let result = b64_decode_bytes(&input, None, false)?;
        Ok(PyObject::bytes(result))
    });

    make_module(
        "binascii",
        vec![
            ("hexlify", hexlify_fn),
            (
                "b2a_hex",
                make_builtin(|args: &[PyObjectRef]| {
                    let data = extract_bytes(&args[0])?;
                    Ok(PyObject::bytes(
                        data.iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<String>()
                            .into_bytes(),
                    ))
                }),
            ),
            ("unhexlify", unhexlify_fn),
            (
                "a2b_hex",
                make_builtin(|args: &[PyObjectRef]| {
                    let hex_str = match &args[0].payload {
                        PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
                        _ => args[0].py_to_string(),
                    };
                    let hex_str = hex_str.trim();
                    if hex_str.len() % 2 != 0 {
                        return Err(PyException::value_error("Odd-length string"));
                    }
                    let mut result = Vec::with_capacity(hex_str.len() / 2);
                    for i in (0..hex_str.len()).step_by(2) {
                        result.push(u8::from_str_radix(&hex_str[i..i + 2], 16).map_err(|_| {
                            PyException::value_error("Non-hexadecimal digit found")
                        })?);
                    }
                    Ok(PyObject::bytes(result))
                }),
            ),
            ("crc32", crc32_fn),
            ("b2a_base64", b2a_base64_fn),
            ("a2b_base64", a2b_base64_fn),
            ("Error", PyObject::exception_type(ExceptionKind::ValueError)),
        ],
    )
}
