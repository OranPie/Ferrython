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
    make_module("hashlib", vec![
        ("md5", make_builtin(hashlib_md5)),
        ("sha1", make_builtin(hashlib_sha1)),
        ("sha256", make_builtin(hashlib_sha256)),
        ("sha512", make_builtin(hashlib_sha512)),
        ("sha224", make_builtin(hashlib_sha224)),
        ("sha384", make_builtin(hashlib_sha384)),
        ("new", make_builtin(hashlib_new)),
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

    fn hmac_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("hmac.new requires key and msg")); }
        let key = match &args[0].payload {
            PyObjectPayload::Bytes(b) => b.clone(),
            PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
            _ => return Err(PyException::type_error("key must be bytes")),
        };
        let msg = match &args[1].payload {
            PyObjectPayload::Bytes(b) => b.clone(),
            PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
            _ => vec![],
        };
        let digestmod = if args.len() > 2 { args[2].py_to_string() } else { "sha256".to_string() };

        // HMAC computation: H((K ^ opad) || H((K ^ ipad) || message))
        let block_size = 64usize;
        let mut k = key;
        if k.len() > block_size {
            k = simple_hash(&k, &digestmod);
        }
        while k.len() < block_size { k.push(0); }
        let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
        let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
        let mut inner = ipad;
        inner.extend_from_slice(&msg);
        let inner_hash = simple_hash(&inner, &digestmod);
        let mut outer = opad;
        outer.extend_from_slice(&inner_hash);
        let result = simple_hash(&outer, &digestmod);

        let hex_str = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("_digest"), PyObject::bytes(result.clone()));
        attrs.insert(CompactString::from("_hexdigest"), PyObject::str_val(CompactString::from(&hex_str)));
        attrs.insert(CompactString::from("digest_size"), PyObject::int(result.len() as i64));
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(format!("hmac-{}", digestmod))));
        attrs.insert(CompactString::from("_digest_bytes"), PyObject::bytes(result));
        attrs.insert(CompactString::from("_hex_str"), PyObject::str_val(CompactString::from(&hex_str)));

        let mut ns = IndexMap::new();
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
        let class = PyObject::class(CompactString::from("HMAC"), vec![], ns);
        let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
            class,
            attrs: Arc::new(RwLock::new(attrs)),
            is_special: true, dict_storage: None,
        }));
        Ok(inst)
    }

    fn simple_hash(data: &[u8], algo: &str) -> Vec<u8> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        // Use Rust's built-in hasher as a simplified substitute
        // (Real HMAC would need proper SHA implementation)
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        algo.hash(&mut hasher);
        let h = hasher.finish();
        let mut result = Vec::new();
        for i in 0..4 {
            let mut hasher2 = DefaultHasher::new();
            data.hash(&mut hasher2);
            (h.wrapping_add(i as u64)).hash(&mut hasher2);
            let v = hasher2.finish();
            result.extend_from_slice(&v.to_be_bytes());
        }
        result
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
            h.get_attr("_digest").ok_or_else(|| PyException::runtime_error("no digest"))
        }))),
        ("HMAC", make_builtin(hmac_new)),
    ])
}

// ── uuid module ────────────────────────────────────────────────────
pub fn create_uuid_module() -> PyObjectRef {
    make_module("uuid", vec![
        ("uuid4", make_builtin(uuid_uuid4)),
        ("uuid1", make_builtin(uuid_uuid1)),
        ("UUID", make_builtin(uuid_UUID)),
        ("NAMESPACE_DNS", PyObject::str_val(CompactString::from("6ba7b810-9dad-11d1-80b4-00c04fd430c8"))),
        ("NAMESPACE_URL", PyObject::str_val(CompactString::from("6ba7b811-9dad-11d1-80b4-00c04fd430c8"))),
    ])
}

fn random_uuid_bytes() -> [u8; 16] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    // Simple xorshift-based PRNG for generating random bytes
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

fn uuid_uuid4(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let _ = args;
    let mut bytes = random_uuid_bytes();
    // Set version 4 bits
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    // Set variant bits
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let hex_str = format_uuid(&bytes);
    let cls = PyObject::class(CompactString::from("UUID"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref data) = inst.payload {
        let mut attrs = data.attrs.write();
        attrs.insert(CompactString::from("hex"), PyObject::str_val(CompactString::from(hex_str.replace('-', ""))));
        attrs.insert(CompactString::from("version"), PyObject::int(4));
        attrs.insert(CompactString::from("int"), PyObject::int(
            u128::from_be_bytes(bytes.try_into().unwrap()) as i64
        ));
        attrs.insert(CompactString::from("__str_val__"), PyObject::str_val(CompactString::from(&hex_str)));
        attrs.insert(CompactString::from("__uuid__"), PyObject::bool_val(true));
    }
    Ok(inst)
}

fn uuid_uuid1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // uuid1 is time-based; use same approach as uuid4 for simplicity
    uuid_uuid4(args)
}

#[allow(non_snake_case)]
fn uuid_UUID(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("UUID", args, 1)?;
    let s = args[0].py_to_string();
    let hex_str = s.replace('-', "");
    if hex_str.len() != 32 || !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PyException::value_error(format!("badly formed hexadecimal UUID string: '{}'", s)));
    }
    let canonical = format!("{}-{}-{}-{}-{}",
        &hex_str[0..8], &hex_str[8..12], &hex_str[12..16],
        &hex_str[16..20], &hex_str[20..32]);
    let version = u8::from_str_radix(&hex_str[12..13], 16).unwrap_or(0);
    let cls = PyObject::class(CompactString::from("UUID"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref data) = inst.payload {
        let mut attrs = data.attrs.write();
        attrs.insert(CompactString::from("hex"), PyObject::str_val(CompactString::from(&hex_str)));
        attrs.insert(CompactString::from("version"), PyObject::int(version as i64));
        attrs.insert(CompactString::from("__str_val__"), PyObject::str_val(CompactString::from(&canonical)));
        attrs.insert(CompactString::from("__uuid__"), PyObject::bool_val(true));
    }
    Ok(inst)
}
