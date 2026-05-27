use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

use super::compute_digest;

pub(crate) fn create_uuid_module() -> PyObjectRef {
    let ns_dns = make_uuid_from_hex("6ba7b810-9dad-11d1-80b4-00c04fd430c8");
    let ns_url = make_uuid_from_hex("6ba7b811-9dad-11d1-80b4-00c04fd430c8");
    let ns_oid = make_uuid_from_hex("6ba7b812-9dad-11d1-80b4-00c04fd430c8");
    let ns_x500 = make_uuid_from_hex("6ba7b814-9dad-11d1-80b4-00c04fd430c8");
    make_module(
        "uuid",
        vec![
            ("uuid1", make_builtin(uuid_uuid1)),
            ("uuid3", make_builtin(uuid_uuid3)),
            ("uuid4", make_builtin(uuid_uuid4)),
            ("uuid5", make_builtin(uuid_uuid5)),
            ("UUID", make_builtin(uuid_UUID)),
            ("NAMESPACE_DNS", ns_dns),
            ("NAMESPACE_URL", ns_url),
            ("NAMESPACE_OID", ns_oid),
            ("NAMESPACE_X500", ns_x500),
            (
                "RESERVED_NCS",
                PyObject::str_val(CompactString::from("reserved for NCS compatibility")),
            ),
            (
                "RFC_4122",
                PyObject::str_val(CompactString::from("specified in RFC 4122")),
            ),
            (
                "RESERVED_MICROSOFT",
                PyObject::str_val(CompactString::from("reserved for Microsoft compatibility")),
            ),
            (
                "RESERVED_FUTURE",
                PyObject::str_val(CompactString::from("reserved for future definition")),
            ),
            (
                "getnode",
                make_builtin(|_| Ok(PyObject::int(0x001122334455_i64))),
            ),
        ],
    )
}

fn random_uuid_bytes() -> [u8; 16] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
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
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    bytes
}

fn build_uuid_object(bytes: [u8; 16]) -> PyObjectRef {
    let hex_str = format_uuid(&bytes);
    let hex_flat = hex_str.replace('-', "");
    let version = (bytes[6] >> 4) & 0x0F;
    let variant_byte = bytes[8];
    let variant = if variant_byte & 0x80 == 0 {
        "reserved for NCS compatibility"
    } else if variant_byte & 0xC0 == 0x80 {
        "specified in RFC 4122"
    } else if variant_byte & 0xE0 == 0xC0 {
        "reserved for Microsoft compatibility"
    } else {
        "reserved for future definition"
    };

    let int_val = bytes.iter().fold(0u128, |acc, &b| (acc << 8) | b as u128);
    let time_low = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let time_mid = u16::from_be_bytes([bytes[4], bytes[5]]);
    let time_hi_ver = u16::from_be_bytes([bytes[6], bytes[7]]);
    let clock_hi = bytes[8];
    let clock_low = bytes[9];
    let node = bytes[10..16]
        .iter()
        .fold(0u64, |acc, &b| (acc << 8) | b as u64);

    let cls = PyObject::class(CompactString::from("UUID"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("hex"),
        PyObject::str_val(CompactString::from(&hex_flat)),
    );
    attrs.insert(
        CompactString::from("version"),
        PyObject::int(version as i64),
    );
    attrs.insert(
        CompactString::from("variant"),
        PyObject::str_val(CompactString::from(variant)),
    );
    attrs.insert(CompactString::from("int"), PyObject::int(int_val as i64));
    attrs.insert(
        CompactString::from("bytes"),
        PyObject::bytes(bytes.to_vec()),
    );
    attrs.insert(
        CompactString::from("bytes_le"),
        PyObject::bytes(vec![
            bytes[3], bytes[2], bytes[1], bytes[0], bytes[5], bytes[4], bytes[7], bytes[6],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]),
    );
    attrs.insert(
        CompactString::from("urn"),
        PyObject::str_val(CompactString::from(format!("urn:uuid:{}", hex_str))),
    );
    attrs.insert(
        CompactString::from("time_low"),
        PyObject::int(time_low as i64),
    );
    attrs.insert(
        CompactString::from("time_mid"),
        PyObject::int(time_mid as i64),
    );
    attrs.insert(
        CompactString::from("time_hi_version"),
        PyObject::int(time_hi_ver as i64),
    );
    attrs.insert(
        CompactString::from("clock_seq_hi_variant"),
        PyObject::int(clock_hi as i64),
    );
    attrs.insert(
        CompactString::from("clock_seq_low"),
        PyObject::int(clock_low as i64),
    );
    attrs.insert(CompactString::from("node"), PyObject::int(node as i64));
    attrs.insert(
        CompactString::from("fields"),
        PyObject::tuple(vec![
            PyObject::int(time_low as i64),
            PyObject::int(time_mid as i64),
            PyObject::int(time_hi_ver as i64),
            PyObject::int(clock_hi as i64),
            PyObject::int(clock_low as i64),
            PyObject::int(node as i64),
        ]),
    );
    attrs.insert(CompactString::from("is_safe"), PyObject::int(0));
    attrs.insert(
        CompactString::from("__str_val__"),
        PyObject::str_val(CompactString::from(&hex_str)),
    );
    attrs.insert(CompactString::from("__uuid__"), PyObject::bool_val(true));
    {
        let str_val = hex_str.clone();
        attrs.insert(
            CompactString::from("__str__"),
            PyObject::native_closure("UUID.__str__", move |_| {
                Ok(PyObject::str_val(CompactString::from(&str_val)))
            }),
        );
    }
    {
        let repr_str = format!("UUID('{}')", hex_str);
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("UUID.__repr__", move |_| {
                Ok(PyObject::str_val(CompactString::from(&repr_str)))
            }),
        );
    }
    {
        let eq_hex = hex_flat.clone();
        attrs.insert(
            CompactString::from("__eq__"),
            PyObject::native_closure("UUID.__eq__", move |args| {
                if args.is_empty() {
                    return Ok(PyObject::bool_val(false));
                }
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    if let Some(h) = inst.attrs.read().get("hex") {
                        return Ok(PyObject::bool_val(h.py_to_string() == eq_hex));
                    }
                }
                Ok(PyObject::bool_val(false))
            }),
        );
    }
    {
        let hash_int = (int_val % (i64::MAX as u128)) as i64;
        attrs.insert(
            CompactString::from("__hash__"),
            PyObject::native_closure("UUID.__hash__", move |_| Ok(PyObject::int(hash_int))),
        );
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
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    Ok(build_uuid_object(bytes))
}

fn uuid_uuid1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let ticks = ts.as_nanos() as u64 / 100 + 0x01b21dd213814000;
    let mut bytes = [0u8; 16];
    let tl = (ticks & 0xFFFFFFFF) as u32;
    bytes[0..4].copy_from_slice(&tl.to_be_bytes());
    let tm = ((ticks >> 32) & 0xFFFF) as u16;
    bytes[4..6].copy_from_slice(&tm.to_be_bytes());
    let th = (((ticks >> 48) & 0x0FFF) | 0x1000) as u16;
    bytes[6..8].copy_from_slice(&th.to_be_bytes());
    let rand = random_uuid_bytes();
    bytes[8] = (rand[8] & 0x3F) | 0x80;
    bytes[9] = rand[9];
    let node = if !args.is_empty() {
        let _ = args;
        &rand[10..16]
    } else {
        &rand[10..16]
    };
    bytes[10..16].copy_from_slice(node);
    Ok(build_uuid_object(bytes))
}

fn uuid_uuid3(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "uuid3() requires namespace and name",
        ));
    }
    let ns_bytes = get_uuid_bytes(&args[0])?;
    let name = args[1].py_to_string();
    let mut data = ns_bytes.to_vec();
    data.extend_from_slice(name.as_bytes());
    let (_, digest) = compute_digest("md5", &data)?;
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0F) | 0x30;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    Ok(build_uuid_object(bytes))
}

fn uuid_uuid5(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "uuid5() requires namespace and name",
        ));
    }
    let ns_bytes = get_uuid_bytes(&args[0])?;
    let name = args[1].py_to_string();
    let mut data = ns_bytes.to_vec();
    data.extend_from_slice(name.as_bytes());
    let (_, digest) = compute_digest("sha1", &data)?;
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0F) | 0x50;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
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
        return Err(PyException::value_error(format!(
            "badly formed hexadecimal UUID string: '{}'",
            s
        )));
    }
    let bytes = hex_to_bytes(&hex_str);
    Ok(build_uuid_object(bytes))
}
