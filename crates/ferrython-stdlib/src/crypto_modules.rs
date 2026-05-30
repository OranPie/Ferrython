//! Cryptography and hashing stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, new_fx_hashkey_map, to_shared_fx, InstanceData, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
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
        "md5",
        "sha1",
        "sha224",
        "sha256",
        "sha384",
        "sha512",
        "sha3_224",
        "sha3_256",
        "sha3_384",
        "sha3_512",
        "shake_128",
        "shake_256",
        "blake2b",
        "blake2s",
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
    let builtin_cache = PyObject::dict(new_fx_hashkey_map());
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
                "shake_128",
                make_builtin(|args| make_hash_obj("shake_128", args)),
            ),
            (
                "shake_256",
                make_builtin(|args| make_hash_obj("shake_256", args)),
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
            (
                "__get_builtin_constructor",
                make_builtin(hashlib_get_builtin_constructor),
            ),
            ("__builtin_constructor_cache", builtin_cache),
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
        "shake_128" | "shake128" => {
            let r = compute_shake_digest(name, data, 32)?;
            Ok((hex_encode(&r), r))
        }
        "shake_256" | "shake256" => {
            let r = compute_shake_digest(name, data, 64)?;
            Ok((hex_encode(&r), r))
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

fn normalized_hash_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('-', "_")
}

fn is_shake_name(name: &str) -> bool {
    matches!(name, "shake_128" | "shake128" | "shake_256" | "shake256")
}

fn shake_length_arg(args: &[PyObjectRef], method: &str) -> PyResult<usize> {
    if args.is_empty() {
        return Err(PyException::type_error(format!(
            "{}() missing required argument 'length'",
            method
        )));
    }
    let length = args[0].to_int()?;
    if length < 0 {
        return Err(PyException::value_error("length must be non-negative"));
    }
    if length > 256 * 1024 * 1024 {
        return Err(PyException::overflow_error("length too large"));
    }
    Ok(length as usize)
}

fn compute_shake_digest(name: &str, data: &[u8], length: usize) -> PyResult<Vec<u8>> {
    use digest::{ExtendableOutput, Update, XofReader};
    let mut out = vec![0u8; length];
    match name {
        "shake_128" | "shake128" => {
            let mut h = sha3::Shake128::default();
            h.update(data);
            h.finalize_xof().read(&mut out);
            Ok(out)
        }
        "shake_256" | "shake256" => {
            let mut h = sha3::Shake256::default();
            h.update(data);
            h.finalize_xof().read(&mut out);
            Ok(out)
        }
        _ => Err(PyException::value_error(format!(
            "unsupported hash type {}",
            name
        ))),
    }
}

fn hash_block_size(name: &str) -> i64 {
    match name {
        "sha3_224" => 144,
        "sha3_256" => 136,
        "sha3_384" => 104,
        "sha3_512" => 72,
        "shake_128" | "shake128" => 168,
        "shake_256" | "shake256" => 136,
        "sha384" | "sha512" | "blake2b" => 128,
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
        "shake_128" | "shake128" | "shake_256" | "shake256" => 0,
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
    let repr_name = algo.clone();
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("hashlib.HASH.__repr__", move |args| {
            let this = args.first().cloned().unwrap_or_else(PyObject::none);
            Ok(PyObject::str_val(CompactString::from(format!(
                "<{}.HASH object at 0x{:x}>",
                repr_name,
                PyObjectRef::as_ptr(&this) as usize
            ))))
        }),
    );

    // update(data) — append to internal buffer
    let buf_c = buf.clone();
    attrs.insert(
        CompactString::from("update"),
        PyObject::native_closure("update", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("update() takes exactly 1 argument"));
            }
            let new_data = extract_hash_bytes(&args[0])?;
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
            let digest_bytes = if is_shake_name(&algo_c) {
                let length = shake_length_arg(_args, "digest")?;
                compute_shake_digest(&algo_c, &data, length)?
            } else {
                let (_, digest_bytes) = compute_digest(&algo_c, &data)?;
                digest_bytes
            };
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
            let hex = if is_shake_name(&algo_c) {
                let length = shake_length_arg(_args, "hexdigest")?;
                hex_encode(&compute_shake_digest(&algo_c, &data, length)?)
            } else {
                let (hex, _) = compute_digest(&algo_c, &data)?;
                hex
            };
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
    let name = normalized_hash_name(name);
    let data = if args.is_empty() {
        vec![]
    } else {
        if matches!(args[0].payload, PyObjectPayload::Dict(_)) {
            vec![]
        } else {
            extract_hash_bytes(&args[0])?
        }
    };
    let bs = hash_block_size(&name);
    let ds = hash_digest_size(&name);
    let (hex, bytes) = compute_digest(&name, &data)?;
    Ok(make_hash_object(&name, data, hex, bytes, bs, ds))
}

fn extract_hash_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Str(_) => Err(PyException::type_error(
            "object supporting the buffer API required",
        )),
        _ => extract_bytes(obj),
    }
}

fn int_arg(obj: &PyObjectRef, name: &str) -> PyResult<i64> {
    if matches!(obj.payload, PyObjectPayload::None) {
        return Err(PyException::type_error(format!(
            "{} must be an integer",
            name
        )));
    }
    obj.to_int()
}

fn kw_value(kwargs: Option<&PyObjectRef>, key: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &kwargs?.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(key)))
        .cloned()
}

fn split_trailing_kwargs(args: &[PyObjectRef]) -> (&[PyObjectRef], Option<PyObjectRef>) {
    if !args.is_empty() && matches!(args[args.len() - 1].payload, PyObjectPayload::Dict(_)) {
        (&args[..args.len() - 1], Some(args[args.len() - 1].clone()))
    } else {
        (args, None)
    }
}

fn arg_or_kw(
    pos: &[PyObjectRef],
    kwargs: Option<&PyObjectRef>,
    idx: usize,
    key: &str,
) -> Option<PyObjectRef> {
    pos.get(idx).cloned().or_else(|| kw_value(kwargs, key))
}

fn make_builtin_hash_constructor(name: &str) -> PyResult<PyObjectRef> {
    match name {
        "md5" => Ok(make_builtin(hashlib_md5)),
        "sha1" => Ok(make_builtin(hashlib_sha1)),
        "sha224" => Ok(make_builtin(hashlib_sha224)),
        "sha256" => Ok(make_builtin(hashlib_sha256)),
        "sha384" => Ok(make_builtin(hashlib_sha384)),
        "sha512" => Ok(make_builtin(hashlib_sha512)),
        "sha3_224" => Ok(make_builtin(|args| make_hash_obj("sha3_224", args))),
        "sha3_256" => Ok(make_builtin(|args| make_hash_obj("sha3_256", args))),
        "sha3_384" => Ok(make_builtin(|args| make_hash_obj("sha3_384", args))),
        "sha3_512" => Ok(make_builtin(|args| make_hash_obj("sha3_512", args))),
        "shake_128" => Ok(make_builtin(|args| make_hash_obj("shake_128", args))),
        "shake_256" => Ok(make_builtin(|args| make_hash_obj("shake_256", args))),
        "blake2b" => Ok(make_builtin(|args| make_hash_obj("blake2b", args))),
        "blake2s" => Ok(make_builtin(|args| make_hash_obj("blake2s", args))),
        _ => Err(PyException::value_error(format!(
            "unsupported hash type {}",
            name
        ))),
    }
}

fn hashlib_get_builtin_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 1 {
        return Err(PyException::type_error(
            "__get_builtin_constructor() takes exactly one argument",
        ));
    }
    let PyObjectPayload::Str(raw) = &args[0].payload else {
        return Err(PyException::type_error("name must be a string"));
    };
    make_builtin_hash_constructor(&normalized_hash_name(raw.as_str()))
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
    let (pos, _kwargs) = split_trailing_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error(
            "hashlib.new() requires algorithm name",
        ));
    }
    let name = match &pos[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("algorithm name must be a string")),
    };
    let data_args = if pos.len() > 1 {
        &pos[1..]
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

fn pbkdf2_hmac_bytes(
    algo: &str,
    password: &[u8],
    salt: &[u8],
    iterations: usize,
    dk_len: usize,
) -> PyResult<Vec<u8>> {
    let h_len = hash_digest_size(&algo) as usize;
    if h_len == 0 {
        return Err(PyException::value_error(format!(
            "unsupported hash type {}",
            algo
        )));
    }
    let blocks_needed = (dk_len + h_len - 1) / h_len;
    let mut dk = Vec::with_capacity(dk_len);

    for block_num in 1..=blocks_needed {
        // U_1 = HMAC(password, salt || INT_32_BE(block_num))
        let mut msg = salt.to_vec();
        msg.extend_from_slice(&(block_num as u32).to_be_bytes());
        let mut u = hmac_digest(password, &msg, algo);
        let mut result = u.clone();

        for _ in 1..iterations {
            u = hmac_digest(password, &u, algo);
            for (r, b) in result.iter_mut().zip(u.iter()) {
                *r ^= *b;
            }
        }
        dk.extend_from_slice(&result);
    }
    dk.truncate(dk_len);
    Ok(dk)
}

/// hashlib.pbkdf2_hmac(hash_name, password, salt, iterations, dklen=None)
fn hashlib_pbkdf2_hmac(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_trailing_kwargs(args);
    let Some(hash_name_obj) = arg_or_kw(pos, kwargs.as_ref(), 0, "hash_name") else {
        return Err(PyException::type_error(
            "pbkdf2_hmac requires hash_name argument",
        ));
    };
    let Some(password_obj) = arg_or_kw(pos, kwargs.as_ref(), 1, "password") else {
        return Err(PyException::type_error(
            "pbkdf2_hmac requires password argument",
        ));
    };
    let Some(salt_obj) = arg_or_kw(pos, kwargs.as_ref(), 2, "salt") else {
        return Err(PyException::type_error(
            "pbkdf2_hmac requires salt argument",
        ));
    };
    let Some(iterations_obj) = arg_or_kw(pos, kwargs.as_ref(), 3, "iterations") else {
        return Err(PyException::type_error(
            "pbkdf2_hmac requires iterations argument",
        ));
    };

    let algo = match &hash_name_obj.payload {
        PyObjectPayload::Str(s) => normalized_hash_name(s.as_str()),
        _ => return Err(PyException::type_error("hash_name must be a string")),
    };
    let password = extract_hash_bytes(&password_obj)?;
    let salt = extract_hash_bytes(&salt_obj)?;
    let iterations_i = int_arg(&iterations_obj, "iterations")?;
    if iterations_i <= 0 {
        return Err(PyException::value_error("iterations must be positive"));
    }
    let dk_len = if let Some(dklen_obj) = arg_or_kw(pos, kwargs.as_ref(), 4, "dklen") {
        if matches!(dklen_obj.payload, PyObjectPayload::None) {
            hash_digest_size(&algo) as usize
        } else {
            let value = int_arg(&dklen_obj, "dklen")?;
            if value <= 0 {
                return Err(PyException::value_error("dklen must be greater than 0"));
            }
            value as usize
        }
    } else {
        hash_digest_size(&algo) as usize
    };
    if dk_len == 0 {
        return Err(PyException::value_error(format!(
            "unsupported hash type {}",
            algo
        )));
    }

    let dk = pbkdf2_hmac_bytes(&algo, &password, &salt, iterations_i as usize, dk_len)?;
    Ok(PyObject::bytes(dk))
}

fn salsa20_8(block: &[u8; 64]) -> [u8; 64] {
    let mut input = [0u32; 16];
    for (i, chunk) in block.chunks_exact(4).enumerate() {
        input[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    let mut x = input;

    for _ in 0..4 {
        x[4] ^= x[0].wrapping_add(x[12]).rotate_left(7);
        x[8] ^= x[4].wrapping_add(x[0]).rotate_left(9);
        x[12] ^= x[8].wrapping_add(x[4]).rotate_left(13);
        x[0] ^= x[12].wrapping_add(x[8]).rotate_left(18);

        x[9] ^= x[5].wrapping_add(x[1]).rotate_left(7);
        x[13] ^= x[9].wrapping_add(x[5]).rotate_left(9);
        x[1] ^= x[13].wrapping_add(x[9]).rotate_left(13);
        x[5] ^= x[1].wrapping_add(x[13]).rotate_left(18);

        x[14] ^= x[10].wrapping_add(x[6]).rotate_left(7);
        x[2] ^= x[14].wrapping_add(x[10]).rotate_left(9);
        x[6] ^= x[2].wrapping_add(x[14]).rotate_left(13);
        x[10] ^= x[6].wrapping_add(x[2]).rotate_left(18);

        x[3] ^= x[15].wrapping_add(x[11]).rotate_left(7);
        x[7] ^= x[3].wrapping_add(x[15]).rotate_left(9);
        x[11] ^= x[7].wrapping_add(x[3]).rotate_left(13);
        x[15] ^= x[11].wrapping_add(x[7]).rotate_left(18);

        x[1] ^= x[0].wrapping_add(x[3]).rotate_left(7);
        x[2] ^= x[1].wrapping_add(x[0]).rotate_left(9);
        x[3] ^= x[2].wrapping_add(x[1]).rotate_left(13);
        x[0] ^= x[3].wrapping_add(x[2]).rotate_left(18);

        x[6] ^= x[5].wrapping_add(x[4]).rotate_left(7);
        x[7] ^= x[6].wrapping_add(x[5]).rotate_left(9);
        x[4] ^= x[7].wrapping_add(x[6]).rotate_left(13);
        x[5] ^= x[4].wrapping_add(x[7]).rotate_left(18);

        x[11] ^= x[10].wrapping_add(x[9]).rotate_left(7);
        x[8] ^= x[11].wrapping_add(x[10]).rotate_left(9);
        x[9] ^= x[8].wrapping_add(x[11]).rotate_left(13);
        x[10] ^= x[9].wrapping_add(x[8]).rotate_left(18);

        x[12] ^= x[15].wrapping_add(x[14]).rotate_left(7);
        x[13] ^= x[12].wrapping_add(x[15]).rotate_left(9);
        x[14] ^= x[13].wrapping_add(x[12]).rotate_left(13);
        x[15] ^= x[14].wrapping_add(x[13]).rotate_left(18);
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        out[i * 4..i * 4 + 4].copy_from_slice(&x[i].wrapping_add(input[i]).to_le_bytes());
    }
    out
}

fn blockmix_salsa8(input: &[u8], r: usize) -> Vec<u8> {
    let mut x = [0u8; 64];
    x.copy_from_slice(&input[(2 * r - 1) * 64..2 * r * 64]);
    let mut y = vec![0u8; input.len()];

    for i in 0..2 * r {
        let mut block = [0u8; 64];
        for j in 0..64 {
            block[j] = x[j] ^ input[i * 64 + j];
        }
        x = salsa20_8(&block);
        y[i * 64..(i + 1) * 64].copy_from_slice(&x);
    }

    let mut out = Vec::with_capacity(input.len());
    for i in (0..2 * r).step_by(2) {
        out.extend_from_slice(&y[i * 64..(i + 1) * 64]);
    }
    for i in (1..2 * r).step_by(2) {
        out.extend_from_slice(&y[i * 64..(i + 1) * 64]);
    }
    out
}

fn integerify(block: &[u8], r: usize) -> u64 {
    let offset = (2 * r - 1) * 64;
    u64::from_le_bytes([
        block[offset],
        block[offset + 1],
        block[offset + 2],
        block[offset + 3],
        block[offset + 4],
        block[offset + 5],
        block[offset + 6],
        block[offset + 7],
    ])
}

fn scrypt_romix(block: &[u8], n: usize, r: usize) -> Vec<u8> {
    let mut x = block.to_vec();
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(x.clone());
        x = blockmix_salsa8(&x, r);
    }
    for _ in 0..n {
        let j = (integerify(&x, r) as usize) & (n - 1);
        for (byte, other) in x.iter_mut().zip(v[j].iter()) {
            *byte ^= *other;
        }
        x = blockmix_salsa8(&x, r);
    }
    x
}

fn scrypt_bytes(
    password: &[u8],
    salt: &[u8],
    n: usize,
    r: usize,
    p: usize,
    dklen: usize,
) -> PyResult<Vec<u8>> {
    let block_len = 128usize
        .checked_mul(r)
        .and_then(|v| v.checked_mul(p))
        .ok_or_else(|| PyException::overflow_error("scrypt parameters are too large"))?;
    let mut b = pbkdf2_hmac_bytes("sha256", password, salt, 1, block_len)?;
    let chunk_len = 128 * r;
    for chunk in b.chunks_mut(chunk_len) {
        let mixed = scrypt_romix(chunk, n, r);
        chunk.copy_from_slice(&mixed);
    }
    pbkdf2_hmac_bytes("sha256", password, &b, 1, dklen)
}

/// hashlib.scrypt(password, *, salt, n, r, p, dklen=64)
fn hashlib_scrypt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_trailing_kwargs(args);
    if pos.is_empty() {
        return Err(PyException::type_error("scrypt requires password argument"));
    }
    if kwargs.is_some() && pos.len() > 1 || kwargs.is_none() && pos.len() != 1 && pos.len() != 6 {
        return Err(PyException::type_error(
            "scrypt() takes exactly one positional argument",
        ));
    }

    let password = extract_hash_bytes(&pos[0])?;
    let legacy_positional = kwargs.is_none() && pos.len() == 6;
    let salt_obj = if legacy_positional {
        pos.get(1).cloned()
    } else {
        kw_value(kwargs.as_ref(), "salt")
    }
    .ok_or_else(|| PyException::type_error("scrypt requires salt keyword argument"))?;
    let n_obj = if legacy_positional {
        pos.get(2).cloned()
    } else {
        kw_value(kwargs.as_ref(), "n")
    }
    .ok_or_else(|| PyException::type_error("scrypt requires n keyword argument"))?;
    let r_obj = if legacy_positional {
        pos.get(3).cloned()
    } else {
        kw_value(kwargs.as_ref(), "r")
    }
    .ok_or_else(|| PyException::type_error("scrypt requires r keyword argument"))?;
    let p_obj = if legacy_positional {
        pos.get(4).cloned()
    } else {
        kw_value(kwargs.as_ref(), "p")
    }
    .ok_or_else(|| PyException::type_error("scrypt requires p keyword argument"))?;
    let dklen_obj = if legacy_positional {
        pos.get(5).cloned()
    } else {
        kw_value(kwargs.as_ref(), "dklen")
    };
    let maxmem_obj = kw_value(kwargs.as_ref(), "maxmem");

    let salt = extract_hash_bytes(&salt_obj)?;
    let n_i = int_arg(&n_obj, "n")?;
    let r_i = int_arg(&r_obj, "r")?;
    let p_i = int_arg(&p_obj, "p")?;
    if n_i <= 1 || (n_i & (n_i - 1)) != 0 {
        return Err(PyException::value_error(
            "n must be a power of 2 greater than 1",
        ));
    }
    if r_i <= 0 {
        return Err(PyException::value_error("r must be greater than 0"));
    }
    if p_i <= 0 {
        return Err(PyException::value_error("p must be greater than 0"));
    }
    if let Some(maxmem) = maxmem_obj {
        let maxmem_i = int_arg(&maxmem, "maxmem")?;
        if maxmem_i < 0 {
            return Err(PyException::value_error("maxmem must be non-negative"));
        }
    }
    let dklen = if let Some(dklen_obj) = dklen_obj {
        let value = int_arg(&dklen_obj, "dklen")?;
        if value <= 0 {
            return Err(PyException::value_error("dklen must be greater than 0"));
        }
        value as usize
    } else {
        64
    };

    let n = n_i as usize;
    let r = r_i as usize;
    let p = p_i as usize;
    let memory = 128usize
        .checked_mul(r)
        .and_then(|v| v.checked_mul(n))
        .ok_or_else(|| PyException::overflow_error("scrypt parameters are too large"))?;
    if memory > 512 * 1024 * 1024 {
        return Err(PyException::value_error("scrypt parameters are too large"));
    }
    Ok(PyObject::bytes(scrypt_bytes(
        &password, &salt, n, r, p, dklen,
    )?))
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
