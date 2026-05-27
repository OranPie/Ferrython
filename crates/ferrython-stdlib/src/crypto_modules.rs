//! Cryptography and hashing stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, to_shared_fx, InstanceData, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;

use super::serial_modules::extract_bytes;

mod secrets;
mod uuid;

pub(crate) use secrets::create_secrets_module;
pub(crate) use uuid::create_uuid_module;

// ── hashlib module ──

pub fn create_hashlib_module() -> PyObjectRef {
    let algos = vec![
        "md5", "sha1", "sha224", "sha256", "sha384", "sha512", "sha3_224", "sha3_256", "sha3_384",
        "sha3_512", "blake2b", "blake2s",
    ];
    let algo_set: IndexMap<ferrython_core::types::HashableKey, PyObjectRef> = algos
        .iter()
        .map(|&a| {
            (
                ferrython_core::types::HashableKey::str_key(CompactString::from(a)),
                PyObject::none(),
            )
        })
        .collect();
    make_module(
        "hashlib",
        vec![
            ("md5", make_builtin(hashlib_md5)),
            ("sha1", make_builtin(hashlib_sha1)),
            ("sha256", make_builtin(hashlib_sha256)),
            ("sha512", make_builtin(hashlib_sha512)),
            ("sha224", make_builtin(hashlib_sha224)),
            ("sha384", make_builtin(hashlib_sha384)),
            (
                "sha3_224",
                make_builtin(|args| make_hash_obj("sha3_224", args)),
            ),
            (
                "sha3_256",
                make_builtin(|args| make_hash_obj("sha3_256", args)),
            ),
            (
                "sha3_384",
                make_builtin(|args| make_hash_obj("sha3_384", args)),
            ),
            (
                "sha3_512",
                make_builtin(|args| make_hash_obj("sha3_512", args)),
            ),
            (
                "blake2b",
                make_builtin(|args| make_hash_obj("blake2b", args)),
            ),
            (
                "blake2s",
                make_builtin(|args| make_hash_obj("blake2s", args)),
            ),
            ("new", make_builtin(hashlib_new)),
            ("pbkdf2_hmac", make_builtin(hashlib_pbkdf2_hmac)),
            ("scrypt", make_builtin(hashlib_scrypt)),
            (
                "algorithms_guaranteed",
                PyObject::frozenset(algo_set.clone()),
            ),
            ("algorithms_available", PyObject::frozenset(algo_set)),
        ],
    )
}

/// Compute digest for an algorithm name + data buffer.
pub(super) fn compute_digest(name: &str, data: &[u8]) -> PyResult<(String, Vec<u8>)> {
    use digest::Digest;
    match name {
        "md5" => {
            let mut h = md5::Md5::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "sha1" => {
            let mut h = sha1::Sha1::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "sha224" => {
            let mut h = sha2::Sha224::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "sha256" => {
            let mut h = sha2::Sha256::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "sha384" => {
            let mut h = sha2::Sha384::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "sha512" => {
            let mut h = sha2::Sha512::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "sha3_224" | "sha3-224" => {
            let mut h = sha3::Sha3_224::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "sha3_256" | "sha3-256" => {
            let mut h = sha3::Sha3_256::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "sha3_384" | "sha3-384" => {
            let mut h = sha3::Sha3_384::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "sha3_512" | "sha3-512" => {
            let mut h = sha3::Sha3_512::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "blake2b" => {
            let mut h = blake2::Blake2b512::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        "blake2s" => {
            let mut h = blake2::Blake2s256::new();
            h.update(data);
            let r = h.finalize();
            Ok((hex_encode(&r), r.to_vec()))
        }
        _ => Err(PyException::value_error(format!(
            "unsupported hash type {}",
            name
        ))),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hash_block_size(name: &str) -> i64 {
    match name {
        "sha384" | "sha512" | "sha3_384" | "sha3_512" | "blake2b" => 128,
        "blake2s" => 64,
        _ => 64,
    }
}

fn hash_digest_size(name: &str) -> i64 {
    match name {
        "md5" => 16,
        "sha1" => 20,
        "sha224" => 28,
        "sha256" => 32,
        "sha384" => 48,
        "sha512" => 64,
        "sha3_224" | "sha3-224" => 28,
        "sha3_256" | "sha3-256" => 32,
        "sha3_384" | "sha3-384" => 48,
        "sha3_512" | "sha3-512" => 64,
        "blake2b" => 64,
        "blake2s" => 32,
        _ => 0,
    }
}

/// Build a hash object with incremental update/digest/hexdigest/copy support.
/// The accumulated data buffer is stored in a shared Rc<PyCell<Vec<u8>>>.
fn make_hash_object(
    name: &str,
    data: Vec<u8>,
    _digest_hex: String,
    _digest_bytes: Vec<u8>,
    _block_size: i64,
    _digest_size: i64,
) -> PyObjectRef {
    let algo = CompactString::from(name);
    let buf = Rc::new(PyCell::new(data));
    let class = PyObject::class(
        CompactString::from("_hashlib.HASH"),
        vec![],
        IndexMap::new(),
    );
    let mut attrs = IndexMap::new();

    attrs.insert(CompactString::from("name"), PyObject::str_val(algo.clone()));
    attrs.insert(
        CompactString::from("block_size"),
        PyObject::int(hash_block_size(name)),
    );
    attrs.insert(
        CompactString::from("digest_size"),
        PyObject::int(hash_digest_size(name)),
    );

    // update(data) — append to internal buffer
    let buf_c = buf.clone();
    attrs.insert(
        CompactString::from("update"),
        PyObject::native_closure("update", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("update() takes exactly 1 argument"));
            }
            let new_data = extract_bytes(&args[0])?;
            buf_c.write().extend_from_slice(&new_data);
            Ok(PyObject::none())
        }),
    );

    // digest() — compute and return bytes
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(
        CompactString::from("digest"),
        PyObject::native_closure("digest", move |_args| {
            let data = buf_c.read().clone();
            let (_, digest_bytes) = compute_digest(&algo_c, &data)?;
            Ok(PyObject::bytes(digest_bytes))
        }),
    );

    // hexdigest() — compute and return hex string
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(
        CompactString::from("hexdigest"),
        PyObject::native_closure("hexdigest", move |_args| {
            let data = buf_c.read().clone();
            let (hex, _) = compute_digest(&algo_c, &data)?;
            Ok(PyObject::str_val(CompactString::from(hex)))
        }),
    );

    // copy() — return independent hash with same accumulated state
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(
        CompactString::from("copy"),
        PyObject::native_closure("copy", move |_args| {
            let data = buf_c.read().clone();
            let (hex, digest_bytes) = compute_digest(&algo_c, &data)?;
            Ok(make_hash_object(&algo_c, data, hex, digest_bytes, 0, 0))
        }),
    );

    // Legacy compatibility: _hexdigest / _digest attributes (compute on access would be ideal,
    // but for backwards compat keep them — they reflect initial data only)
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(
        CompactString::from("_hexdigest"),
        PyObject::native_closure("_hexdigest", move |_args| {
            let data = buf_c.read().clone();
            let (hex, _) = compute_digest(&algo_c, &data)?;
            Ok(PyObject::str_val(CompactString::from(hex)))
        }),
    );
    let algo_c = algo.clone();
    let buf_c = buf.clone();
    attrs.insert(
        CompactString::from("_digest"),
        PyObject::native_closure("_digest", move |_args| {
            let data = buf_c.read().clone();
            let (_, digest_bytes) = compute_digest(&algo_c, &data)?;
            Ok(PyObject::bytes(digest_bytes))
        }),
    );

    let class_flags = InstanceData::compute_flags(&class);
    let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
        Box::new(InstanceData {
            class,
            attrs: to_shared_fx(attrs),
            is_special: true,
            dict_storage: None,
            class_flags,
            finalizer_state: std::cell::Cell::new(0),
        }),
    )));
    inst
}

fn make_hash_obj(name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let data = if args.is_empty() {
        vec![]
    } else {
        extract_bytes(&args[0])?
    };
    let bs = hash_block_size(name);
    let ds = hash_digest_size(name);
    let (hex, bytes) = compute_digest(name, &data)?;
    Ok(make_hash_object(name, data, hex, bytes, bs, ds))
}

fn hashlib_md5(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    make_hash_obj("md5", args)
}
fn hashlib_sha1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    make_hash_obj("sha1", args)
}
fn hashlib_sha256(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    make_hash_obj("sha256", args)
}
fn hashlib_sha224(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    make_hash_obj("sha224", args)
}
fn hashlib_sha384(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    make_hash_obj("sha384", args)
}
fn hashlib_sha512(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    make_hash_obj("sha512", args)
}

fn hashlib_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "hashlib.new() requires algorithm name",
        ));
    }
    let name = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("algorithm name must be a string")),
    };
    let data_args = if args.len() > 1 {
        &args[1..]
    } else {
        &[] as &[PyObjectRef]
    };
    make_hash_obj(&name, data_args)
}

/// HMAC helper used by pbkdf2_hmac (same logic as hmac module's compute_hmac)
fn hmac_digest(key: &[u8], msg: &[u8], algo: &str) -> Vec<u8> {
    let block_size: usize = match algo {
        "sha384" | "sha512" => 128,
        _ => 64,
    };
    let mut k = key.to_vec();
    if k.len() > block_size {
        k = compute_digest(algo, &k).map(|(_, b)| b).unwrap_or_default();
    }
    while k.len() < block_size {
        k.push(0);
    }
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let mut inner = ipad;
    inner.extend_from_slice(msg);
    let inner_hash = compute_digest(algo, &inner)
        .map(|(_, b)| b)
        .unwrap_or_default();
    let mut outer = opad;
    outer.extend_from_slice(&inner_hash);
    compute_digest(algo, &outer)
        .map(|(_, b)| b)
        .unwrap_or_default()
}

/// hashlib.pbkdf2_hmac(hash_name, password, salt, iterations, dklen=None)
fn hashlib_pbkdf2_hmac(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 4 {
        return Err(PyException::type_error(
            "pbkdf2_hmac requires at least 4 arguments",
        ));
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

// ── hmac module ──────────────────────────────────────────────────────
pub fn create_hmac_module() -> PyObjectRef {
    /// Compute HMAC from key, message, and digestmod strings
    fn compute_hmac(key: &[u8], msg: &[u8], digestmod: &str) -> Vec<u8> {
        let block_size = 64usize;
        let mut k = key.to_vec();
        if k.len() > block_size {
            k = simple_hash(&k, digestmod);
        }
        while k.len() < block_size {
            k.push(0);
        }
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
            Some(PyObjectPayload::Bytes(b)) => (**b).clone(),
            _ => return,
        };
        let msg = match attrs.get("_msg").map(|m| &m.payload) {
            Some(PyObjectPayload::Bytes(b)) => (**b).clone(),
            _ => vec![],
        };
        let digestmod = attrs
            .get("_digestmod")
            .map(|d| d.py_to_string())
            .unwrap_or_else(|| "sha256".to_string());
        drop(attrs);

        let result = compute_hmac(&key, &msg, &digestmod);
        let hex_str: String = result.iter().map(|b| format!("{:02x}", b)).collect();

        let mut attrs = inst.attrs.write();
        attrs.insert(
            CompactString::from("digest_size"),
            PyObject::int(result.len() as i64),
        );
        attrs.insert(
            CompactString::from("_digest_bytes"),
            PyObject::bytes(result),
        );
        attrs.insert(
            CompactString::from("_hex_str"),
            PyObject::str_val(CompactString::from(&hex_str)),
        );
    }

    fn hmac_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("hmac.new() requires key argument"));
        }
        let key = match &args[0].payload {
            PyObjectPayload::Bytes(b) => (**b).clone(),
            PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
            _ => return Err(PyException::type_error("key must be bytes")),
        };
        // msg is optional (default empty)
        let msg = if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Bytes(b) => (**b).clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                PyObjectPayload::None => vec![],
                _ => vec![],
            }
        } else {
            vec![]
        };
        // digestmod: 3rd positional OR keyword "digestmod"
        let digestmod = if args.len() > 2 {
            args[2].py_to_string()
        } else {
            "sha256".to_string()
        };

        let result = compute_hmac(&key, &msg, &digestmod);
        let hex_str: String = result.iter().map(|b| format!("{:02x}", b)).collect();

        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("_key"), PyObject::bytes(key));
        attrs.insert(CompactString::from("_msg"), PyObject::bytes(msg));
        attrs.insert(
            CompactString::from("_digestmod"),
            PyObject::str_val(CompactString::from(&digestmod)),
        );
        attrs.insert(
            CompactString::from("digest_size"),
            PyObject::int(result.len() as i64),
        );
        attrs.insert(CompactString::from("block_size"), PyObject::int(64));
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(format!("hmac-{}", digestmod))),
        );
        attrs.insert(
            CompactString::from("_digest_bytes"),
            PyObject::bytes(result),
        );
        attrs.insert(
            CompactString::from("_hex_str"),
            PyObject::str_val(CompactString::from(&hex_str)),
        );

        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("update"),
            make_builtin(|args| {
                let (inst_ref, data_arg) = if args.len() >= 2 {
                    (&args[0], &args[1])
                } else {
                    return Err(PyException::type_error("update() takes exactly 1 argument"));
                };
                if let PyObjectPayload::Instance(inst) = &inst_ref.payload {
                    let new_data = match &data_arg.payload {
                        PyObjectPayload::Bytes(b) => (**b).clone(),
                        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                        _ => {
                            return Err(PyException::type_error("update() argument must be bytes"))
                        }
                    };
                    {
                        let mut attrs = inst.attrs.write();
                        let cur_msg = match attrs.get("_msg").map(|m| &m.payload) {
                            Some(PyObjectPayload::Bytes(b)) => (**b).clone(),
                            _ => vec![],
                        };
                        let mut combined = cur_msg;
                        combined.extend_from_slice(&new_data);
                        attrs.insert(CompactString::from("_msg"), PyObject::bytes(combined));
                    }
                    recompute_digest(inst);
                }
                Ok(PyObject::none())
            }),
        );
        ns.insert(
            CompactString::from("digest"),
            make_builtin(|args| {
                if args.is_empty() {
                    return Ok(PyObject::bytes(vec![]));
                }
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if let Some(v) = inst.attrs.read().get("_digest_bytes") {
                        return Ok(v.clone());
                    }
                }
                Ok(PyObject::bytes(vec![]))
            }),
        );
        ns.insert(
            CompactString::from("hexdigest"),
            make_builtin(|args| {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if let Some(v) = inst.attrs.read().get("_hex_str") {
                        return Ok(v.clone());
                    }
                }
                Ok(PyObject::str_val(CompactString::from("")))
            }),
        );
        ns.insert(
            CompactString::from("copy"),
            make_builtin(|args| {
                if args.is_empty() {
                    return Err(PyException::type_error("copy() requires self"));
                }
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    let attrs_copy = inst.attrs.read().clone();
                    let new_inst = PyObject::wrap(PyObjectPayload::Instance(
                        std::mem::ManuallyDrop::new(Box::new(InstanceData {
                            class: inst.class.clone(),
                            attrs: Rc::new(PyCell::new(attrs_copy)),
                            is_special: true,
                            dict_storage: None,
                            class_flags: InstanceData::compute_flags(&inst.class),
                            finalizer_state: std::cell::Cell::new(0),
                        })),
                    ));
                    return Ok(new_inst);
                }
                Err(PyException::type_error("copy() requires HMAC instance"))
            }),
        );

        let class = PyObject::class(CompactString::from("HMAC"), vec![], ns);
        let class_flags = InstanceData::compute_flags(&class);
        let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
            Box::new(InstanceData {
                class,
                attrs: to_shared_fx(attrs),
                is_special: true,
                dict_storage: None,
                class_flags,
                finalizer_state: std::cell::Cell::new(0),
            }),
        )));
        Ok(inst)
    }

    fn simple_hash(data: &[u8], algo: &str) -> Vec<u8> {
        compute_digest(algo, data)
            .map(|(_, bytes)| bytes)
            .unwrap_or_else(|_| {
                compute_digest("sha256", data)
                    .map(|(_, b)| b)
                    .unwrap_or_default()
            })
    }

    fn hmac_compare_digest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "compare_digest requires 2 arguments",
            ));
        }
        let a = args[0].py_to_string();
        let b = args[1].py_to_string();
        let a_bytes = a.as_bytes();
        let b_bytes = b.as_bytes();
        if a_bytes.len() != b_bytes.len() {
            return Ok(PyObject::bool_val(false));
        }
        let mut result = 0u8;
        for i in 0..a_bytes.len() {
            result |= a_bytes[i] ^ b_bytes[i];
        }
        Ok(PyObject::bool_val(result == 0))
    }

    make_module(
        "hmac",
        vec![
            ("new", make_builtin(hmac_new)),
            ("compare_digest", make_builtin(hmac_compare_digest)),
            (
                "digest",
                make_builtin(|args| {
                    hmac_new(args).and_then(|h| {
                        h.get_attr("_digest_bytes")
                            .ok_or_else(|| PyException::runtime_error("no digest"))
                    })
                }),
            ),
            ("HMAC", make_builtin(hmac_new)),
        ],
    )
}
