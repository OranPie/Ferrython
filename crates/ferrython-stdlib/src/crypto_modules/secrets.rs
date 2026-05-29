use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

pub(crate) fn create_secrets_module() -> PyObjectRef {
    make_module(
        "secrets",
        vec![
            ("token_bytes", make_builtin(secrets_token_bytes)),
            ("token_hex", make_builtin(secrets_token_hex)),
            ("token_urlsafe", make_builtin(secrets_token_urlsafe)),
            ("randbits", make_builtin(secrets_randbits)),
            ("randbelow", make_builtin(secrets_randbelow)),
            ("choice", make_builtin(secrets_choice)),
            ("compare_digest", make_builtin(secrets_compare_digest)),
            ("DEFAULT_ENTROPY", PyObject::int(32)),
        ],
    )
}

fn secrets_random_bytes(n: usize) -> Vec<u8> {
    use std::time::SystemTime;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut result = Vec::with_capacity(n);
    for _ in 0..n {
        let cnt = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64;
        let seed = nanos
            .wrapping_mul(6364136223846793005)
            .wrapping_add(cnt.wrapping_mul(1442695040888963407));
        result.push((seed >> 16) as u8);
    }
    result
}

fn secrets_random_f64() -> f64 {
    use std::time::SystemTime;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cnt = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as u64;
    let seed = nanos
        .wrapping_mul(6364136223846793005)
        .wrapping_add(cnt.wrapping_mul(1442695040888963407));
    (seed >> 11) as f64 / (1u64 << 53) as f64
}

fn secrets_token_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let nbytes = secrets_nbytes_arg(args)?;
    Ok(PyObject::bytes(secrets_random_bytes(nbytes)))
}

fn secrets_token_hex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let nbytes = secrets_nbytes_arg(args)?;
    let bytes = secrets_random_bytes(nbytes);
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(PyObject::str_val(CompactString::from(hex)))
}

fn secrets_token_urlsafe(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let nbytes = secrets_nbytes_arg(args)?;
    let bytes = secrets_random_bytes(nbytes);
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity((nbytes * 4 + 2) / 3);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() {
            bytes[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < bytes.len() {
            bytes[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < bytes.len() {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        }
        if i + 2 < bytes.len() {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        }
        i += 3;
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn secrets_nbytes_arg(args: &[PyObjectRef]) -> PyResult<usize> {
    if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
        return Ok(32);
    }
    let n = args[0].to_int()?;
    if n < 0 {
        return Err(PyException::value_error(
            "number of bytes must be non-negative",
        ));
    }
    Ok(n as usize)
}

fn secrets_randbits(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("secrets.randbits", args, 1)?;
    let k = args[0].to_int()?;
    if k < 0 {
        return Err(PyException::value_error(
            "number of bits must be greater than zero",
        ));
    }
    if k == 0 {
        return Ok(PyObject::int(0));
    }

    let nbytes = ((k + 7) / 8) as usize;
    let mut value = num_bigint::BigInt::from(0u8);
    for byte in secrets_random_bytes(nbytes) {
        value = (value << 8) | num_bigint::BigInt::from(byte);
    }
    let excess_bits = nbytes * 8 - k as usize;
    if excess_bits > 0 {
        value >>= excess_bits;
    }
    if let Some(value) = num_traits::ToPrimitive::to_i64(&value) {
        Ok(PyObject::int(value))
    } else {
        Ok(PyObject::big_int(value))
    }
}

fn secrets_randbelow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("secrets.randbelow", args, 1)?;
    let n = args[0].to_int()?;
    if n <= 0 {
        return Err(PyException::value_error("upper bound must be positive"));
    }
    let val = (secrets_random_f64() * n as f64) as i64;
    Ok(PyObject::int(val.min(n - 1)))
}

fn secrets_choice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("secrets.choice", args, 1)?;
    let items = args[0].to_list()?;
    if items.is_empty() {
        return Err(PyException::index_error(
            "cannot choose from an empty sequence",
        ));
    }
    let idx = (secrets_random_f64() * items.len() as f64) as usize;
    Ok(items[idx.min(items.len() - 1)].clone())
}

fn secrets_compare_digest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("secrets.compare_digest", args, 2)?;
    let (a_bytes, b_bytes) = match (&args[0].payload, &args[1].payload) {
        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => (
            a.as_str().as_bytes().to_vec(),
            b.as_str().as_bytes().to_vec(),
        ),
        (PyObjectPayload::Bytes(a), PyObjectPayload::Bytes(b))
        | (PyObjectPayload::ByteArray(a), PyObjectPayload::ByteArray(b))
        | (PyObjectPayload::Bytes(a), PyObjectPayload::ByteArray(b))
        | (PyObjectPayload::ByteArray(a), PyObjectPayload::Bytes(b)) => {
            ((**a).clone(), (**b).clone())
        }
        _ => {
            return Err(PyException::type_error(
                "unsupported operand types for compare_digest",
            ))
        }
    };
    let mut result: u8 = if a_bytes.len() != b_bytes.len() { 1 } else { 0 };
    let len = std::cmp::min(a_bytes.len(), b_bytes.len());
    for i in 0..len {
        result |= a_bytes[i] ^ b_bytes[i];
    }
    Ok(PyObject::bool_val(result == 0))
}
