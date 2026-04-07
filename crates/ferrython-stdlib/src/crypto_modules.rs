//! Cryptography and hashing stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, InstanceData,
    make_module, make_builtin, check_args,
};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

use super::serial_modules::extract_bytes;

// ── hashlib module ──

pub fn create_hashlib_module() -> PyObjectRef {
    let algos = vec!["md5", "sha1", "sha224", "sha256", "sha384", "sha512"];
    let algo_set: IndexMap<ferrython_core::types::HashableKey, PyObjectRef> = algos.iter()
        .map(|&a| (ferrython_core::types::HashableKey::Str(CompactString::from(a)), PyObject::none()))
        .collect();
    make_module("hashlib", vec![
        ("md5", make_builtin(hashlib_md5)),
        ("sha1", make_builtin(hashlib_sha1)),
        ("sha256", make_builtin(hashlib_sha256)),
        ("sha512", make_builtin(hashlib_sha512)),
        ("sha224", make_builtin(hashlib_sha224)),
        ("sha384", make_builtin(hashlib_sha384)),
        ("new", make_builtin(hashlib_new)),
        ("pbkdf2_hmac", make_builtin(hashlib_pbkdf2_hmac)),
        ("scrypt", make_builtin(hashlib_scrypt)),
        ("algorithms_guaranteed", PyObject::frozenset(algo_set.clone())),
        ("algorithms_available", PyObject::frozenset(algo_set)),
    ])
}

/// Compute digest for an algorithm name + data buffer.
fn compute_digest(name: &str, data: &[u8]) -> PyResult<(String, Vec<u8>)> {
    use digest::Digest;
    match name {
        "md5"    => { let mut h = md5::Md5::new(); h.update(data); let r = h.finalize(); Ok((hex_encode(&r), r.to_vec())) }
        "sha1"   => { let mut h = sha1::Sha1::new(); h.update(data); let r = h.finalize(); Ok((hex_encode(&r), r.to_vec())) }
        "sha224" => { let mut h = sha2::Sha224::new(); h.update(data); let r = h.finalize(); Ok((hex_encode(&r), r.to_vec())) }
        "sha256" => { let mut h = sha2::Sha256::new(); h.update(data); let r = h.finalize(); Ok((hex_encode(&r), r.to_vec())) }
        "sha384" => { let mut h = sha2::Sha384::new(); h.update(data); let r = h.finalize(); Ok((hex_encode(&r), r.to_vec())) }
        "sha512" => { let mut h = sha2::Sha512::new(); h.update(data); let r = h.finalize(); Ok((hex_encode(&r), r.to_vec())) }
        _ => Err(PyException::value_error(format!("unsupported hash type {}", name))),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hash_block_size(name: &str) -> i64 {
    match name { "sha384" | "sha512" => 128, _ => 64 }
}

fn hash_digest_size(name: &str) -> i64 {
    match name { "md5" => 16, "sha1" => 20, "sha224" => 28, "sha256" => 32, "sha384" => 48, "sha512" => 64, _ => 0 }
}

/// Build a hash object with incremental update/digest/hexdigest/copy support.
/// The accumulated data buffer is stored in a shared Arc<RwLock<Vec<u8>>>.
fn make_hash_object(name: &str, data: Vec<u8>, _digest_hex: String, _digest_bytes: Vec<u8>, _block_size: i64, _digest_size: i64) -> PyObjectRef {
    let algo = CompactString::from(name);
    let buf = Arc::new(RwLock::new(data));
    let class = PyObject::class(CompactString::from("_hashlib.HASH"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();

    attrs.insert(CompactString::from("name"), PyObject::str_val(algo.clone()));
    attrs.insert(CompactString::from("block_size"), PyObject::int(hash_block_size(name)));
    attrs.insert(CompactString::from("digest_size"), PyObject::int(hash_digest_size(name)));

    // update(data) — append to internal buffer
    let buf_c = buf.clone();
    attrs.insert(CompactString::from("update"), PyObject::native_closure("update", move |args| {
        if args.is_empty() { return Err(PyException::type_error("update() takes exactly 1 argument")); }
        let new_data = extract_bytes(&args[0])?;
        buf_c.write().extend_from_slice(&new_data);
        Ok(PyObject::none())
    }));

    // digest() — compute and return bytes
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(CompactString::from("digest"), PyObject::native_closure("digest", move |_args| {
        let data = buf_c.read().clone();
        let (_, digest_bytes) = compute_digest(&algo_c, &data)?;
        Ok(PyObject::bytes(digest_bytes))
    }));

    // hexdigest() — compute and return hex string
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(CompactString::from("hexdigest"), PyObject::native_closure("hexdigest", move |_args| {
        let data = buf_c.read().clone();
        let (hex, _) = compute_digest(&algo_c, &data)?;
        Ok(PyObject::str_val(CompactString::from(hex)))
    }));

    // copy() — return independent hash with same accumulated state
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(CompactString::from("copy"), PyObject::native_closure("copy", move |_args| {
        let data = buf_c.read().clone();
        let (hex, digest_bytes) = compute_digest(&algo_c, &data)?;
        Ok(make_hash_object(&algo_c, data, hex, digest_bytes, 0, 0))
    }));

    // Legacy compatibility: _hexdigest / _digest attributes (compute on access would be ideal,
    // but for backwards compat keep them — they reflect initial data only)
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(CompactString::from("_hexdigest"), PyObject::native_closure("_hexdigest", move |_args| {
        let data = buf_c.read().clone();
        let (hex, _) = compute_digest(&algo_c, &data)?;
        Ok(PyObject::str_val(CompactString::from(hex)))
    }));
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(CompactString::from("_digest"), PyObject::native_closure("_digest", move |_args| {
        let data = buf_c.read().clone();
        let (_, digest_bytes) = compute_digest(&algo_c, &data)?;
        Ok(PyObject::bytes(digest_bytes))
    }));

    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(attrs)),
        is_special: true, dict_storage: None,
    }));
    inst
}

fn hashlib_md5(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let (hex, bytes) = compute_digest("md5", &data)?;
    Ok(make_hash_object("md5", data, hex, bytes, 64, 16))
}

fn hashlib_sha1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let (hex, bytes) = compute_digest("sha1", &data)?;
    Ok(make_hash_object("sha1", data, hex, bytes, 64, 20))
}

fn hashlib_sha256(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let (hex, bytes) = compute_digest("sha256", &data)?;
    Ok(make_hash_object("sha256", data, hex, bytes, 64, 32))
}

fn hashlib_sha224(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let (hex, bytes) = compute_digest("sha224", &data)?;
    Ok(make_hash_object("sha224", data, hex, bytes, 64, 28))
}

fn hashlib_sha384(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let (hex, bytes) = compute_digest("sha384", &data)?;
    Ok(make_hash_object("sha384", data, hex, bytes, 128, 48))
}

fn hashlib_sha512(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let (hex, bytes) = compute_digest("sha512", &data)?;
    Ok(make_hash_object("sha512", data, hex, bytes, 128, 64))
}

fn hashlib_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("hashlib.new() requires algorithm name")); }
    let name = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("algorithm name must be a string")),
    };
    let data_args = if args.len() > 1 { &args[1..] } else { &[] as &[PyObjectRef] };
    match name.as_str() {
        "md5" => hashlib_md5(data_args),
        "sha1" => hashlib_sha1(data_args),
        "sha256" => hashlib_sha256(data_args),
        "sha224" => hashlib_sha224(data_args),
        "sha384" => hashlib_sha384(data_args),
        "sha512" => hashlib_sha512(data_args),
        _ => Err(PyException::value_error(format!("unsupported hash type {}", name))),
    }
}

/// HMAC helper used by pbkdf2_hmac (same logic as hmac module's compute_hmac)
fn hmac_digest(key: &[u8], msg: &[u8], algo: &str) -> Vec<u8> {
    let block_size: usize = match algo { "sha384" | "sha512" => 128, _ => 64 };
    let mut k = key.to_vec();
    if k.len() > block_size {
        k = compute_digest(algo, &k).map(|(_, b)| b).unwrap_or_default();
    }
    while k.len() < block_size { k.push(0); }
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let mut inner = ipad;
    inner.extend_from_slice(msg);
    let inner_hash = compute_digest(algo, &inner).map(|(_, b)| b).unwrap_or_default();
    let mut outer = opad;
    outer.extend_from_slice(&inner_hash);
    compute_digest(algo, &outer).map(|(_, b)| b).unwrap_or_default()
}

/// hashlib.pbkdf2_hmac(hash_name, password, salt, iterations, dklen=None)
fn hashlib_pbkdf2_hmac(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 4 {
        return Err(PyException::type_error("pbkdf2_hmac requires at least 4 arguments"));
    }
    let algo = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("hash_name must be a string")),
    };
    let password = extract_bytes(&args[1])?;
    let salt = extract_bytes(&args[2])?;
    let iterations = args[3].to_int()? as usize;
    if iterations < 1 {
        return Err(PyException::value_error("iterations must be positive"));
    }
    let dk_len = if args.len() > 4 && !matches!(args[4].payload, PyObjectPayload::None) {
        args[4].to_int()? as usize
    } else {
        hash_digest_size(&algo) as usize
    };
    if dk_len == 0 {
        return Err(PyException::value_error("unsupported hash type for pbkdf2"));
    }

    let h_len = hash_digest_size(&algo) as usize;
    let blocks_needed = (dk_len + h_len - 1) / h_len;
    let mut dk = Vec::with_capacity(dk_len);

    for block_num in 1..=blocks_needed {
        // U_1 = HMAC(password, salt || INT_32_BE(block_num))
        let mut msg = salt.clone();
        msg.extend_from_slice(&(block_num as u32).to_be_bytes());
        let mut u = hmac_digest(&password, &msg, &algo);
        let mut result = u.clone();

        for _ in 1..iterations {
            u = hmac_digest(&password, &u, &algo);
            for (r, b) in result.iter_mut().zip(u.iter()) {
                *r ^= *b;
            }
        }
        dk.extend_from_slice(&result);
    }
    dk.truncate(dk_len);
    Ok(PyObject::bytes(dk))
}

/// hashlib.scrypt(password, *, salt, n, r, p, dklen=64)
/// Simplified scrypt implementation using PBKDF2-HMAC-SHA256 as a fallback.
/// Full scrypt requires Salsa20/8 core; this provides the API with a simplified KDF.
fn hashlib_scrypt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("scrypt requires password argument"));
    }
    let password = extract_bytes(&args[0])?;
    let mut salt = vec![0u8; 16];
    let mut _n: u64 = 16384;
    let mut _r: u64 = 8;
    let mut _p: u64 = 1;
    let mut dklen: usize = 64;

    // Parse remaining positional/keyword-style args
    for (i, arg) in args[1..].iter().enumerate() {
        match i {
            0 => salt = extract_bytes(arg)?,
            1 => _n = arg.to_int()? as u64,
            2 => _r = arg.to_int()? as u64,
            3 => _p = arg.to_int()? as u64,
            4 => dklen = arg.to_int()? as usize,
            _ => {}
        }
    }

    // Use PBKDF2-HMAC-SHA256 with N iterations as simplified scrypt
    let iterations = (_n as usize).min(100000);
    let h_len = 32usize; // SHA-256
    let blocks_needed = (dklen + h_len - 1) / h_len;
    let mut dk = Vec::with_capacity(dklen);

    for block_num in 1..=blocks_needed {
        let mut msg = salt.clone();
        msg.extend_from_slice(&(block_num as u32).to_be_bytes());
        let mut u = hmac_digest(&password, &msg, "sha256");
        let mut result = u.clone();
        for _ in 1..iterations {
            u = hmac_digest(&password, &u, "sha256");
            for (r, b) in result.iter_mut().zip(u.iter()) {
                *r ^= *b;
            }
        }
        dk.extend_from_slice(&result);
    }
    dk.truncate(dklen);
    Ok(PyObject::bytes(dk))
}

// ── secrets module ──────────────────────────────────────────────────

pub fn create_secrets_module() -> PyObjectRef {
    make_module("secrets", vec![
        ("token_bytes", make_builtin(secrets_token_bytes)),
        ("token_hex", make_builtin(secrets_token_hex)),
        ("token_urlsafe", make_builtin(secrets_token_urlsafe)),
        ("randbelow", make_builtin(secrets_randbelow)),
        ("choice", make_builtin(secrets_choice)),
        ("compare_digest", make_builtin(secrets_compare_digest)),
    ])
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
    let nbytes = if args.is_empty() { 32 } else { args[0].to_int()? as usize };
    Ok(PyObject::bytes(secrets_random_bytes(nbytes)))
}

fn secrets_token_hex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let nbytes = if args.is_empty() { 32 } else { args[0].to_int()? as usize };
    let bytes = secrets_random_bytes(nbytes);
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(PyObject::str_val(CompactString::from(hex)))
}

fn secrets_token_urlsafe(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let nbytes = if args.is_empty() { 32 } else { args[0].to_int()? as usize };
    let bytes = secrets_random_bytes(nbytes);
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity((nbytes * 4 + 2) / 3);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };
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
        return Err(PyException::index_error("cannot choose from an empty sequence"));
    }
    let idx = (secrets_random_f64() * items.len() as f64) as usize;
    Ok(items[idx.min(items.len() - 1)].clone())
}

fn secrets_compare_digest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("secrets.compare_digest", args, 2)?;
    let a = args[0].py_to_string();
    let b = args[1].py_to_string();
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut result: u8 = if a_bytes.len() != b_bytes.len() { 1 } else { 0 };
    let len = std::cmp::min(a_bytes.len(), b_bytes.len());
    for i in 0..len {
        result |= a_bytes[i] ^ b_bytes[i];
    }
    Ok(PyObject::bool_val(result == 0))
}


// ── hmac module ──────────────────────────────────────────────────────
pub fn create_hmac_module() -> PyObjectRef {

    /// Compute HMAC from key, message, and digestmod strings
    fn compute_hmac(key: &[u8], msg: &[u8], digestmod: &str) -> Vec<u8> {
        let block_size = 64usize;
        let mut k = key.to_vec();
        if k.len() > block_size {
            k = simple_hash(&k, digestmod);
        }
        while k.len() < block_size { k.push(0); }
        let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
        let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
        let mut inner = ipad;
        inner.extend_from_slice(msg);
        let inner_hash = simple_hash(&inner, digestmod);
        let mut outer = opad;
        outer.extend_from_slice(&inner_hash);
        simple_hash(&outer, digestmod)
    }

    /// Recompute digest from stored key + accumulated message
    fn recompute_digest(inst: &InstanceData) {
        let attrs = inst.attrs.read();
        let key = match attrs.get("_key").map(|k| &k.payload) {
            Some(PyObjectPayload::Bytes(b)) => b.clone(),
            _ => return,
        };
        let msg = match attrs.get("_msg").map(|m| &m.payload) {
            Some(PyObjectPayload::Bytes(b)) => b.clone(),
            _ => vec![],
        };
        let digestmod = attrs.get("_digestmod").map(|d| d.py_to_string()).unwrap_or_else(|| "sha256".to_string());
        drop(attrs);

        let result = compute_hmac(&key, &msg, &digestmod);
        let hex_str: String = result.iter().map(|b| format!("{:02x}", b)).collect();

        let mut attrs = inst.attrs.write();
        attrs.insert(CompactString::from("digest_size"), PyObject::int(result.len() as i64));
        attrs.insert(CompactString::from("_digest_bytes"), PyObject::bytes(result));
        attrs.insert(CompactString::from("_hex_str"), PyObject::str_val(CompactString::from(&hex_str)));
    }

    fn hmac_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("hmac.new() requires key argument")); }
        let key = match &args[0].payload {
            PyObjectPayload::Bytes(b) => b.clone(),
            PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
            _ => return Err(PyException::type_error("key must be bytes")),
        };
        // msg is optional (default empty)
        let msg = if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Bytes(b) => b.clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                PyObjectPayload::None => vec![],
                _ => vec![],
            }
        } else { vec![] };
        // digestmod: 3rd positional OR keyword "digestmod"
        let digestmod = if args.len() > 2 {
            args[2].py_to_string()
        } else { "sha256".to_string() };

        let result = compute_hmac(&key, &msg, &digestmod);
        let hex_str: String = result.iter().map(|b| format!("{:02x}", b)).collect();

        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("_key"), PyObject::bytes(key));
        attrs.insert(CompactString::from("_msg"), PyObject::bytes(msg));
        attrs.insert(CompactString::from("_digestmod"), PyObject::str_val(CompactString::from(&digestmod)));
        attrs.insert(CompactString::from("digest_size"), PyObject::int(result.len() as i64));
        attrs.insert(CompactString::from("block_size"), PyObject::int(64));
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(format!("hmac-{}", digestmod))));
        attrs.insert(CompactString::from("_digest_bytes"), PyObject::bytes(result));
        attrs.insert(CompactString::from("_hex_str"), PyObject::str_val(CompactString::from(&hex_str)));

        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("update"), make_builtin(|args| {
            let (inst_ref, data_arg) = if args.len() >= 2 {
                (&args[0], &args[1])
            } else {
                return Err(PyException::type_error("update() takes exactly 1 argument"));
            };
            if let PyObjectPayload::Instance(inst) = &inst_ref.payload {
                let new_data = match &data_arg.payload {
                    PyObjectPayload::Bytes(b) => b.clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    _ => return Err(PyException::type_error("update() argument must be bytes")),
                };
                {
                    let mut attrs = inst.attrs.write();
                    let cur_msg = match attrs.get("_msg").map(|m| &m.payload) {
                        Some(PyObjectPayload::Bytes(b)) => b.clone(),
                        _ => vec![],
                    };
                    let mut combined = cur_msg;
                    combined.extend_from_slice(&new_data);
                    attrs.insert(CompactString::from("_msg"), PyObject::bytes(combined));
                }
                recompute_digest(inst);
            }
            Ok(PyObject::none())
        }));
        ns.insert(CompactString::from("digest"), make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::bytes(vec![])); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(v) = inst.attrs.read().get("_digest_bytes") { return Ok(v.clone()); }
            }
            Ok(PyObject::bytes(vec![]))
        }));
        ns.insert(CompactString::from("hexdigest"), make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(v) = inst.attrs.read().get("_hex_str") { return Ok(v.clone()); }
            }
            Ok(PyObject::str_val(CompactString::from("")))
        }));
        ns.insert(CompactString::from("copy"), make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("copy() requires self")); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs_copy = inst.attrs.read().clone();
                let new_inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
                    class: inst.class.clone(),
                    attrs: Arc::new(RwLock::new(attrs_copy)),
                    is_special: true, dict_storage: None,
                }));
                return Ok(new_inst);
            }
            Err(PyException::type_error("copy() requires HMAC instance"))
        }));

        let class = PyObject::class(CompactString::from("HMAC"), vec![], ns);
        let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
            class,
            attrs: Arc::new(RwLock::new(attrs)),
            is_special: true, dict_storage: None,
        }));
        Ok(inst)
    }

    fn simple_hash(data: &[u8], algo: &str) -> Vec<u8> {
        compute_digest(algo, data).map(|(_, bytes)| bytes).unwrap_or_else(|_| {
            compute_digest("sha256", data).map(|(_, b)| b).unwrap_or_default()
        })
    }

    fn hmac_compare_digest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("compare_digest requires 2 arguments")); }
        let a = args[0].py_to_string();
        let b = args[1].py_to_string();
        let a_bytes = a.as_bytes();
        let b_bytes = b.as_bytes();
        if a_bytes.len() != b_bytes.len() { return Ok(PyObject::bool_val(false)); }
        let mut result = 0u8;
        for i in 0..a_bytes.len() { result |= a_bytes[i] ^ b_bytes[i]; }
        Ok(PyObject::bool_val(result == 0))
    }

    make_module("hmac", vec![
        ("new", make_builtin(hmac_new)),
        ("compare_digest", make_builtin(hmac_compare_digest)),
        ("digest", make_builtin(|args| hmac_new(args).and_then(|h| {
            h.get_attr("_digest_bytes").ok_or_else(|| PyException::runtime_error("no digest"))
        }))),
        ("HMAC", make_builtin(hmac_new)),
    ])
}

// ── uuid module ────────────────────────────────────────────────────
pub fn create_uuid_module() -> PyObjectRef {
    // NAMESPACE constants as proper UUID objects
    let ns_dns = make_uuid_from_hex("6ba7b810-9dad-11d1-80b4-00c04fd430c8");
    let ns_url = make_uuid_from_hex("6ba7b811-9dad-11d1-80b4-00c04fd430c8");
    let ns_oid = make_uuid_from_hex("6ba7b812-9dad-11d1-80b4-00c04fd430c8");
    let ns_x500 = make_uuid_from_hex("6ba7b814-9dad-11d1-80b4-00c04fd430c8");
    make_module("uuid", vec![
        ("uuid1", make_builtin(uuid_uuid1)),
        ("uuid3", make_builtin(uuid_uuid3)),
        ("uuid4", make_builtin(uuid_uuid4)),
        ("uuid5", make_builtin(uuid_uuid5)),
        ("UUID", make_builtin(uuid_UUID)),
        ("NAMESPACE_DNS", ns_dns),
        ("NAMESPACE_URL", ns_url),
        ("NAMESPACE_OID", ns_oid),
        ("NAMESPACE_X500", ns_x500),
        ("RESERVED_NCS", PyObject::str_val(CompactString::from("reserved for NCS compatibility"))),
        ("RFC_4122", PyObject::str_val(CompactString::from("specified in RFC 4122"))),
        ("RESERVED_MICROSOFT", PyObject::str_val(CompactString::from("reserved for Microsoft compatibility"))),
        ("RESERVED_FUTURE", PyObject::str_val(CompactString::from("reserved for future definition"))),
        ("getnode", make_builtin(|_| Ok(PyObject::int(0x001122334455_i64)))),
    ])
}

fn random_uuid_bytes() -> [u8; 16] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    let mut state = seed ^ 0x517cc1b727220a95;
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        for (i, b) in chunk.iter_mut().enumerate() {
            *b = ((state >> (i * 8)) & 0xFF) as u8;
        }
    }
    bytes
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn hex_to_bytes(hex: &str) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    for i in 0..16 {
        bytes[i] = u8::from_str_radix(&hex[i*2..i*2+2], 16).unwrap_or(0);
    }
    bytes
}

/// Build a UUID object from 16 bytes with all CPython-compatible attributes.
fn build_uuid_object(bytes: [u8; 16]) -> PyObjectRef {
    let hex_str = format_uuid(&bytes);
    let hex_flat = hex_str.replace('-', "");
    let version = (bytes[6] >> 4) & 0x0F;
    let variant_byte = bytes[8];
    let variant = if variant_byte & 0x80 == 0 { "reserved for NCS compatibility" }
        else if variant_byte & 0xC0 == 0x80 { "specified in RFC 4122" }
        else if variant_byte & 0xE0 == 0xC0 { "reserved for Microsoft compatibility" }
        else { "reserved for future definition" };

    // int value from bytes
    let int_val = bytes.iter().fold(0u128, |acc, &b| (acc << 8) | b as u128);

    // fields: (time_low, time_mid, time_hi_version, clock_seq_hi_variant, clock_seq_low, node)
    let time_low = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let time_mid = u16::from_be_bytes([bytes[4], bytes[5]]);
    let time_hi_ver = u16::from_be_bytes([bytes[6], bytes[7]]);
    let clock_hi = bytes[8];
    let clock_low = bytes[9];
    let node = bytes[10..16].iter().fold(0u64, |acc, &b| (acc << 8) | b as u64);

    let cls = PyObject::class(CompactString::from("UUID"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("hex"), PyObject::str_val(CompactString::from(&hex_flat)));
    attrs.insert(CompactString::from("version"), PyObject::int(version as i64));
    attrs.insert(CompactString::from("variant"), PyObject::str_val(CompactString::from(variant)));
    attrs.insert(CompactString::from("int"), PyObject::int(int_val as i64));
    attrs.insert(CompactString::from("bytes"), PyObject::bytes(bytes.to_vec()));
    attrs.insert(CompactString::from("bytes_le"), PyObject::bytes(vec![
        bytes[3], bytes[2], bytes[1], bytes[0],
        bytes[5], bytes[4], bytes[7], bytes[6],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15],
    ]));
    attrs.insert(CompactString::from("urn"), PyObject::str_val(CompactString::from(format!("urn:uuid:{}", hex_str))));
    attrs.insert(CompactString::from("time_low"), PyObject::int(time_low as i64));
    attrs.insert(CompactString::from("time_mid"), PyObject::int(time_mid as i64));
    attrs.insert(CompactString::from("time_hi_version"), PyObject::int(time_hi_ver as i64));
    attrs.insert(CompactString::from("clock_seq_hi_variant"), PyObject::int(clock_hi as i64));
    attrs.insert(CompactString::from("clock_seq_low"), PyObject::int(clock_low as i64));
    attrs.insert(CompactString::from("node"), PyObject::int(node as i64));
    attrs.insert(CompactString::from("fields"), PyObject::tuple(vec![
        PyObject::int(time_low as i64), PyObject::int(time_mid as i64),
        PyObject::int(time_hi_ver as i64), PyObject::int(clock_hi as i64),
        PyObject::int(clock_low as i64), PyObject::int(node as i64),
    ]));
    attrs.insert(CompactString::from("is_safe"), PyObject::int(0));
    attrs.insert(CompactString::from("__str_val__"), PyObject::str_val(CompactString::from(&hex_str)));
    attrs.insert(CompactString::from("__uuid__"), PyObject::bool_val(true));
    {
        let str_val = hex_str.clone();
        attrs.insert(CompactString::from("__str__"), PyObject::native_closure("UUID.__str__", move |_| {
            Ok(PyObject::str_val(CompactString::from(&str_val)))
        }));
    }
    {
        let repr_str = format!("UUID('{}')", hex_str);
        attrs.insert(CompactString::from("__repr__"), PyObject::native_closure("UUID.__repr__", move |_| {
            Ok(PyObject::str_val(CompactString::from(&repr_str)))
        }));
    }
    {
        let eq_hex = hex_flat.clone();
        attrs.insert(CompactString::from("__eq__"), PyObject::native_closure("UUID.__eq__", move |args| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            // Compare by hex
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                if let Some(h) = inst.attrs.read().get("hex") {
                    return Ok(PyObject::bool_val(h.py_to_string() == eq_hex));
                }
            }
            Ok(PyObject::bool_val(false))
        }));
    }
    {
        let hash_int = (int_val % (i64::MAX as u128)) as i64;
        attrs.insert(CompactString::from("__hash__"), PyObject::native_closure("UUID.__hash__", move |_| {
            Ok(PyObject::int(hash_int))
        }));
    }
    PyObject::instance_with_attrs(cls, attrs)
}

fn make_uuid_from_hex(hex: &str) -> PyObjectRef {
    let clean = hex.replace('-', "");
    let bytes = hex_to_bytes(&clean);
    build_uuid_object(bytes)
}

fn uuid_uuid4(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let _ = args;
    let mut bytes = random_uuid_bytes();
    bytes[6] = (bytes[6] & 0x0F) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // variant RFC 4122
    Ok(build_uuid_object(bytes))
}

fn uuid_uuid1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // uuid1 is time-based; approximate with timestamp + random
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let ticks = ts.as_nanos() as u64 / 100 + 0x01b21dd213814000; // offset to UUID epoch
    let mut bytes = [0u8; 16];
    // time_low (bytes 0-3)
    let tl = (ticks & 0xFFFFFFFF) as u32;
    bytes[0..4].copy_from_slice(&tl.to_be_bytes());
    // time_mid (bytes 4-5)
    let tm = ((ticks >> 32) & 0xFFFF) as u16;
    bytes[4..6].copy_from_slice(&tm.to_be_bytes());
    // time_hi_and_version (bytes 6-7)
    let th = (((ticks >> 48) & 0x0FFF) | 0x1000) as u16;
    bytes[6..8].copy_from_slice(&th.to_be_bytes());
    // clock_seq with variant
    let rand = random_uuid_bytes();
    bytes[8] = (rand[8] & 0x3F) | 0x80;
    bytes[9] = rand[9];
    // node from arg or random
    let node = if !args.is_empty() {
        let _ = args;
        &rand[10..16]
    } else { &rand[10..16] };
    bytes[10..16].copy_from_slice(node);
    Ok(build_uuid_object(bytes))
}

fn uuid_uuid3(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // uuid3(namespace, name) — MD5 hash-based
    if args.len() < 2 { return Err(PyException::type_error("uuid3() requires namespace and name")); }
    let ns_bytes = get_uuid_bytes(&args[0])?;
    let name = args[1].py_to_string();
    let mut data = ns_bytes.to_vec();
    data.extend_from_slice(name.as_bytes());
    let (_, digest) = compute_digest("md5", &data)?;
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0F) | 0x30; // version 3
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // variant RFC 4122
    Ok(build_uuid_object(bytes))
}

fn uuid_uuid5(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // uuid5(namespace, name) — SHA-1 hash-based
    if args.len() < 2 { return Err(PyException::type_error("uuid5() requires namespace and name")); }
    let ns_bytes = get_uuid_bytes(&args[0])?;
    let name = args[1].py_to_string();
    let mut data = ns_bytes.to_vec();
    data.extend_from_slice(name.as_bytes());
    let (_, digest) = compute_digest("sha1", &data)?;
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0F) | 0x50; // version 5
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // variant RFC 4122
    Ok(build_uuid_object(bytes))
}

fn get_uuid_bytes(obj: &PyObjectRef) -> PyResult<[u8; 16]> {
    if let PyObjectPayload::Instance(ref inst) = obj.payload {
        let attrs = inst.attrs.read();
        if let Some(b) = attrs.get("bytes") {
            if let PyObjectPayload::Bytes(v) = &b.payload {
                if v.len() == 16 {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(v);
                    return Ok(arr);
                }
            }
        }
        if let Some(h) = attrs.get("hex") {
            let hex = h.py_to_string();
            return Ok(hex_to_bytes(&hex));
        }
    }
    // Try as string
    let s = obj.py_to_string().replace('-', "");
    if s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(hex_to_bytes(&s));
    }
    Err(PyException::type_error("expected a UUID object"))
}

#[allow(non_snake_case)]
fn uuid_UUID(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("UUID", args, 1)?;
    let s = args[0].py_to_string();
    let hex_str = s.replace('-', "");
    if hex_str.len() != 32 || !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PyException::value_error(format!("badly formed hexadecimal UUID string: '{}'", s)));
    }
    let bytes = hex_to_bytes(&hex_str);
    Ok(build_uuid_object(bytes))
}
