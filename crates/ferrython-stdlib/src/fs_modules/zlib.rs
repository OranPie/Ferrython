use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

// ── byte extraction helper (used by zlib) ──

fn gzip_extract_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok((**b).clone()),
        PyObjectPayload::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(PyException::type_error("expected bytes-like object")),
    }
}

// ── pathlib module (basic) ──

// ── zlib module ──

pub fn create_zlib_module() -> PyObjectRef {
    let compress_fn = make_builtin(|args: &[PyObjectRef]| {
        use flate2::write::ZlibEncoder;
        use std::io::Write;
        if args.is_empty() {
            return Err(PyException::type_error(
                "zlib.compress requires data argument",
            ));
        }
        let data = gzip_extract_bytes(&args[0])?;
        let level = if args.len() > 1 {
            args[1].to_int().unwrap_or(6).max(-1).min(9)
        } else {
            6
        };
        let flate_level = if level == -1 { 6 } else { level as u32 };
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::new(flate_level));
        encoder
            .write_all(&data)
            .map_err(|e| PyException::runtime_error(format!("zlib.compress: {}", e)))?;
        let compressed = encoder
            .finish()
            .map_err(|e| PyException::runtime_error(format!("zlib.compress: {}", e)))?;
        Ok(PyObject::bytes(compressed))
    });

    let decompress_fn = make_builtin(|args: &[PyObjectRef]| {
        use flate2::write::ZlibDecoder;
        use std::io::Write;
        if args.is_empty() {
            return Err(PyException::type_error(
                "zlib.decompress requires data argument",
            ));
        }
        let data = gzip_extract_bytes(&args[0])?;
        if data.len() < 2 {
            return Err(PyException::runtime_error(
                "zlib.decompress: incomplete data",
            ));
        }
        let mut decoder = ZlibDecoder::new(Vec::new());
        decoder
            .write_all(&data)
            .map_err(|e| PyException::runtime_error(format!("zlib.decompress: {}", e)))?;
        let result = decoder
            .finish()
            .map_err(|e| PyException::runtime_error(format!("zlib.decompress: {}", e)))?;
        Ok(PyObject::bytes(result))
    });

    let crc32_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("zlib.crc32 requires data argument"));
        }
        let data = gzip_extract_bytes(&args[0])?;
        let init = if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Int(n) => n.to_i64().unwrap_or(0) as u32,
                _ => 0,
            }
        } else {
            0
        };
        let crc = gzip_crc32_with_init(&data, init);
        Ok(PyObject::int(crc as i64))
    });

    let adler32_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "zlib.adler32 requires data argument",
            ));
        }
        let data = gzip_extract_bytes(&args[0])?;
        let adler = zlib_adler32(&data);
        Ok(PyObject::int(adler as i64))
    });

    make_module(
        "zlib",
        vec![
            ("compress", compress_fn),
            ("decompress", decompress_fn),
            ("crc32", crc32_fn),
            ("adler32", adler32_fn),
            ("DEFLATED", PyObject::int(8)),
            ("MAX_WBITS", PyObject::int(15)),
            ("DEF_MEM_LEVEL", PyObject::int(8)),
            ("DEF_BUF_SIZE", PyObject::int(16384)),
            ("Z_DEFAULT_COMPRESSION", PyObject::int(-1)),
            ("Z_NO_COMPRESSION", PyObject::int(0)),
            ("Z_BEST_SPEED", PyObject::int(1)),
            ("Z_BEST_COMPRESSION", PyObject::int(9)),
        ],
    )
}

fn zlib_adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn gzip_crc32_with_init(data: &[u8], init: u32) -> u32 {
    let mut crc = !init;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}
