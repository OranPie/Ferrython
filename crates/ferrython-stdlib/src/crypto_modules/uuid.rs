use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    call_callable, make_builtin, make_module, py_int_from_bigint, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use num_bigint::{BigInt, Sign};
use num_traits::{One, Signed, ToPrimitive};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use super::compute_digest;

static RANDOM_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn create_uuid_module() -> PyObjectRef {
    let uuid_class = build_uuid_class();
    let safe_uuid_class = build_safe_uuid_class();
    let ns_dns = make_uuid_from_hex("6ba7b810-9dad-11d1-80b4-00c04fd430c8", &uuid_class);
    let ns_url = make_uuid_from_hex("6ba7b811-9dad-11d1-80b4-00c04fd430c8", &uuid_class);
    let ns_oid = make_uuid_from_hex("6ba7b812-9dad-11d1-80b4-00c04fd430c8", &uuid_class);
    let ns_x500 = make_uuid_from_hex("6ba7b814-9dad-11d1-80b4-00c04fd430c8", &uuid_class);

    let uuid1_class = uuid_class.clone();
    let uuid3_class = uuid_class.clone();
    let uuid4_class = uuid_class.clone();
    let uuid5_class = uuid_class.clone();
    make_module(
        "uuid",
        vec![
            (
                "uuid1",
                PyObject::native_closure("uuid.uuid1", move |args| uuid_uuid1(args, &uuid1_class)),
            ),
            (
                "uuid3",
                PyObject::native_closure("uuid.uuid3", move |args| uuid_uuid3(args, &uuid3_class)),
            ),
            (
                "uuid4",
                PyObject::native_closure("uuid.uuid4", move |args| uuid_uuid4(args, &uuid4_class)),
            ),
            (
                "uuid5",
                PyObject::native_closure("uuid.uuid5", move |args| uuid_uuid5(args, &uuid5_class)),
            ),
            ("UUID", uuid_class),
            ("SafeUUID", safe_uuid_class),
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
            ("_GETTERS", PyObject::list(vec![])),
            ("_node", PyObject::none()),
            ("_last_timestamp", PyObject::none()),
            ("_has_uuid_generate_time_safe", PyObject::bool_val(false)),
            ("_generate_time_safe", PyObject::none()),
            (
                "_load_system_functions",
                make_builtin(|_| Ok(PyObject::none())),
            ),
            ("_find_mac", make_builtin(uuid_find_mac)),
            ("_random_getnode", make_builtin(uuid_random_getnode)),
            ("_ifconfig_getnode", make_builtin(uuid_none_getnode)),
            ("_ip_getnode", make_builtin(uuid_none_getnode)),
            ("_arp_getnode", make_builtin(uuid_none_getnode)),
            ("_lanscan_getnode", make_builtin(uuid_none_getnode)),
            ("_netstat_getnode", make_builtin(uuid_none_getnode)),
            ("_ipconfig_getnode", make_builtin(uuid_none_getnode)),
            ("_netbios_getnode", make_builtin(uuid_none_getnode)),
            ("_unix_getnode", make_builtin(uuid_none_getnode)),
            ("_windll_getnode", make_builtin(uuid_none_getnode)),
        ],
    )
}

fn build_uuid_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__module__"),
        PyObject::str_val(CompactString::from("uuid")),
    );
    ns.insert(
        CompactString::from("__qualname__"),
        PyObject::str_val(CompactString::from("UUID")),
    );
    ns.insert(
        CompactString::from("__new__"),
        PyObject::native_function("UUID.__new__", uuid_UUID),
    );
    ns.insert(
        CompactString::from("__str__"),
        PyObject::native_function("UUID.__str__", uuid_obj_str),
    );
    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_function("UUID.__repr__", uuid_obj_repr),
    );
    ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_function("UUID.__eq__", uuid_obj_eq),
    );
    ns.insert(
        CompactString::from("__ne__"),
        PyObject::native_function("UUID.__ne__", uuid_obj_ne),
    );
    ns.insert(
        CompactString::from("__lt__"),
        PyObject::native_function("UUID.__lt__", uuid_obj_lt),
    );
    ns.insert(
        CompactString::from("__le__"),
        PyObject::native_function("UUID.__le__", uuid_obj_le),
    );
    ns.insert(
        CompactString::from("__gt__"),
        PyObject::native_function("UUID.__gt__", uuid_obj_gt),
    );
    ns.insert(
        CompactString::from("__ge__"),
        PyObject::native_function("UUID.__ge__", uuid_obj_ge),
    );
    ns.insert(
        CompactString::from("__hash__"),
        PyObject::native_function("UUID.__hash__", uuid_obj_hash),
    );
    ns.insert(
        CompactString::from("__int__"),
        PyObject::native_function("UUID.__int__", uuid_obj_int),
    );
    ns.insert(
        CompactString::from("__index__"),
        PyObject::native_function("UUID.__index__", uuid_obj_int),
    );
    ns.insert(
        CompactString::from("__getnewargs__"),
        PyObject::native_function("UUID.__getnewargs__", uuid_obj_getnewargs),
    );
    ns.insert(
        CompactString::from("__setattr__"),
        PyObject::native_function("UUID.__setattr__", uuid_obj_setattr),
    );
    ns.insert(
        CompactString::from("__delattr__"),
        PyObject::native_function("UUID.__delattr__", uuid_obj_setattr),
    );
    PyObject::class(CompactString::from("UUID"), vec![], ns)
}

fn build_safe_uuid_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__module__"),
        PyObject::str_val(CompactString::from("uuid")),
    );
    ns.insert(
        CompactString::from("__qualname__"),
        PyObject::str_val(CompactString::from("SafeUUID")),
    );
    ns.insert(CompactString::from("safe"), PyObject::int(0));
    ns.insert(CompactString::from("unsafe"), PyObject::int(-1));
    ns.insert(CompactString::from("unknown"), PyObject::none());
    ns.insert(
        CompactString::from("__new__"),
        PyObject::native_function("SafeUUID.__new__", safe_uuid_new),
    );
    ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_function("SafeUUID.__iter__", safe_uuid_iter),
    );
    PyObject::class(CompactString::from("SafeUUID"), vec![], ns)
}

fn safe_uuid_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let value = args.get(1).or_else(|| args.first());
    match value.map(|v| &v.payload) {
        Some(PyObjectPayload::Int(PyInt::Small(0))) => Ok(PyObject::int(0)),
        Some(PyObjectPayload::Int(PyInt::Small(-1))) => Ok(PyObject::int(-1)),
        Some(PyObjectPayload::None) | None => Ok(PyObject::none()),
        _ => Ok(PyObject::none()),
    }
}

fn safe_uuid_iter(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::list(vec![
        PyObject::int(0),
        PyObject::int(-1),
        PyObject::none(),
    ]))
}

fn uuid_none_getnode(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::none())
}

fn uuid_find_mac(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(0x1234567890ab_i64))
}

fn uuid_random_getnode(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mut bytes = random_uuid_bytes();
    bytes[0] |= 0x01;
    let node = bytes[..6]
        .iter()
        .fold(0u64, |acc, &b| (acc << 8) | b as u64);
    Ok(PyObject::int(node as i64))
}

fn random_uuid_bytes() -> [u8; 16] {
    let seed = system_time_ns();
    let mut state =
        seed ^ 0x517c_c1b7_2722_0a95 ^ RANDOM_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
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

fn clean_uuid_hex(s: &str) -> String {
    let mut clean = s.trim().to_ascii_lowercase();
    if let Some(rest) = clean.strip_prefix("urn:uuid:") {
        clean = rest.to_string();
    }
    if clean.starts_with('{') && clean.ends_with('}') && clean.len() >= 2 {
        clean = clean[1..clean.len() - 1].to_string();
    }
    clean.replace('-', "")
}

fn hex_to_bytes(hex: &str) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    for i in 0..16 {
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap_or(0);
    }
    bytes
}

fn uuid_int_from_bytes(bytes: &[u8; 16]) -> BigInt {
    BigInt::from_bytes_be(Sign::Plus, bytes)
}

fn bigint_to_uuid_bytes(value: &BigInt) -> PyResult<[u8; 16]> {
    let limit = BigInt::one() << 128usize;
    if value.is_negative() || value >= &limit {
        return Err(PyException::value_error(
            "int is out of range (need a 128-bit value)",
        ));
    }
    let int_val = value
        .to_u128()
        .ok_or_else(|| PyException::value_error("int is out of range (need a 128-bit value)"))?;
    Ok(int_val.to_be_bytes())
}

fn int_bigint(obj: &PyObjectRef) -> Option<BigInt> {
    match &obj.payload {
        PyObjectPayload::Int(PyInt::Small(n)) => Some(BigInt::from(*n)),
        PyObjectPayload::Int(PyInt::Big(n)) => Some(n.as_ref().clone()),
        PyObjectPayload::Bool(b) => Some(BigInt::from(if *b { 1 } else { 0 })),
        _ => None,
    }
}

fn bigint_to_ranged_u64(value: &BigInt, bits: usize, name: &str) -> PyResult<u64> {
    let limit = BigInt::one() << bits;
    if value.is_negative() || value >= &limit {
        return Err(PyException::value_error(format!(
            "field {} is out of range",
            name
        )));
    }
    value
        .to_u64()
        .ok_or_else(|| PyException::value_error(format!("field {} is out of range", name)))
}

fn extract_bytes_arg(obj: &PyObjectRef, name: &str) -> PyResult<[u8; 16]> {
    let data = match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
        _ => obj.py_to_string().into_bytes(),
    };
    if data.len() != 16 {
        return Err(PyException::value_error(format!(
            "{} is not a 16-byte string",
            name
        )));
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&data);
    Ok(bytes)
}

fn extract_fields_arg(obj: &PyObjectRef) -> PyResult<[u8; 16]> {
    let items = match &obj.payload {
        PyObjectPayload::Tuple(items) => (**items).clone(),
        PyObjectPayload::List(items) => items.read().clone(),
        _ => return Err(PyException::value_error("fields is not a 6-tuple")),
    };
    if items.len() != 6 {
        return Err(PyException::value_error("fields is not a 6-tuple"));
    }
    let field = |idx: usize, bits: usize, name: &str| -> PyResult<u64> {
        let value = int_bigint(&items[idx])
            .ok_or_else(|| PyException::value_error(format!("field {} is out of range", name)))?;
        bigint_to_ranged_u64(&value, bits, name)
    };
    let time_low = field(0, 32, "time_low")? as u32;
    let time_mid = field(1, 16, "time_mid")? as u16;
    let time_hi = field(2, 16, "time_hi_version")? as u16;
    let clock_hi = field(3, 8, "clock_seq_hi_variant")? as u8;
    let clock_low = field(4, 8, "clock_seq_low")? as u8;
    let node = field(5, 48, "node")?;

    let mut bytes = [0u8; 16];
    bytes[0..4].copy_from_slice(&time_low.to_be_bytes());
    bytes[4..6].copy_from_slice(&time_mid.to_be_bytes());
    bytes[6..8].copy_from_slice(&time_hi.to_be_bytes());
    bytes[8] = clock_hi;
    bytes[9] = clock_low;
    for i in 0..6 {
        bytes[10 + i] = ((node >> (8 * (5 - i))) & 0xff) as u8;
    }
    Ok(bytes)
}

fn kwargs_get(kwargs: Option<&PyObjectRef>, key: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &kwargs?.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(key)))
        .cloned()
}

fn active_module(name: &str) -> Option<PyObjectRef> {
    let sys = crate::get_current_sys_module()?;
    let modules = sys.get_attr("modules")?;
    let PyObjectPayload::Dict(map) = &modules.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(name)))
        .cloned()
        .filter(|obj| !matches!(&obj.payload, PyObjectPayload::None))
}

fn module_attr(name: &str, attr: &str) -> Option<PyObjectRef> {
    active_module(name).and_then(|module| module.get_attr(attr))
}

fn call_module_attr(name: &str, attr: &str, args: &[PyObjectRef]) -> Option<PyObjectRef> {
    let func = module_attr(name, attr)?;
    call_callable(&func, args).ok()
}

fn system_time_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn py_time_ns() -> u64 {
    call_module_attr("time", "time_ns", &[])
        .and_then(|obj| int_bigint(&obj))
        .and_then(|value| value.to_u64())
        .unwrap_or_else(system_time_ns)
}

fn py_random_getrandbits(bits: i64) -> Option<u64> {
    call_module_attr("random", "getrandbits", &[PyObject::int(bits)])
        .and_then(|obj| int_bigint(&obj))
        .and_then(|value| value.to_u64())
}

fn split_kwargs(args: &[PyObjectRef]) -> (&[PyObjectRef], Option<&PyObjectRef>) {
    if let Some(last) = args.last() {
        if matches!(&last.payload, PyObjectPayload::Dict(_)) {
            return (&args[..args.len() - 1], Some(last));
        }
    }
    (args, None)
}

fn constructor_pos_args(args: &[PyObjectRef]) -> &[PyObjectRef] {
    if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
        &args[1..]
    } else {
        args
    }
}

fn uuid_constructor(args: &[PyObjectRef], cls: PyObjectRef) -> PyResult<PyObjectRef> {
    let (pos_with_cls, kwargs) = split_kwargs(args);
    let pos = constructor_pos_args(pos_with_cls);
    if pos.len() > 1 {
        return Err(PyException::type_error(
            "UUID() takes at most 1 positional argument",
        ));
    }

    let positional_hex = pos.first().cloned();
    let hex_kw = kwargs_get(kwargs, "hex");
    let bytes_kw = kwargs_get(kwargs, "bytes");
    let bytes_le_kw = kwargs_get(kwargs, "bytes_le");
    let fields_kw = kwargs_get(kwargs, "fields");
    let int_kw = kwargs_get(kwargs, "int");
    let version_kw = kwargs_get(kwargs, "version");
    let is_safe = kwargs_get(kwargs, "is_safe").unwrap_or_else(PyObject::none);

    let supplied = [
        positional_hex.is_some(),
        hex_kw.is_some(),
        bytes_kw.is_some(),
        bytes_le_kw.is_some(),
        fields_kw.is_some(),
        int_kw.is_some(),
    ]
    .into_iter()
    .filter(|v| *v)
    .count();
    if supplied != 1 {
        return Err(PyException::type_error(
            "one of the hex, bytes, bytes_le, fields, or int arguments must be given",
        ));
    }

    let mut bytes = if let Some(value) = positional_hex.or(hex_kw) {
        let original = value.py_to_string();
        let clean = clean_uuid_hex(&original);
        if clean.len() != 32 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(PyException::value_error(format!(
                "badly formed hexadecimal UUID string: '{}'",
                original
            )));
        }
        hex_to_bytes(&clean)
    } else if let Some(value) = bytes_kw {
        extract_bytes_arg(&value, "bytes")?
    } else if let Some(value) = bytes_le_kw {
        let le = extract_bytes_arg(&value, "bytes_le")?;
        [
            le[3], le[2], le[1], le[0], le[5], le[4], le[7], le[6], le[8], le[9], le[10], le[11],
            le[12], le[13], le[14], le[15],
        ]
    } else if let Some(value) = fields_kw {
        extract_fields_arg(&value)?
    } else if let Some(value) = int_kw {
        let int_val = int_bigint(&value).ok_or_else(|| {
            PyException::value_error("int is out of range (need a 128-bit value)")
        })?;
        bigint_to_uuid_bytes(&int_val)?
    } else {
        unreachable!()
    };

    if let Some(version_obj) = version_kw {
        let version = int_bigint(&version_obj)
            .and_then(|v| v.to_i64())
            .ok_or_else(|| PyException::value_error("illegal version number"))?;
        if !(1..=5).contains(&version) {
            return Err(PyException::value_error("illegal version number"));
        }
        bytes[6] = (bytes[6] & 0x0f) | ((version as u8) << 4);
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
    }

    Ok(build_uuid_object(bytes, cls, is_safe))
}

fn build_uuid_object(bytes: [u8; 16], cls: PyObjectRef, is_safe: PyObjectRef) -> PyObjectRef {
    let hex_str = format_uuid(&bytes);
    let hex_flat = hex_str.replace('-', "");
    let variant_byte = bytes[8];
    let (variant, version_obj) = if variant_byte & 0x80 == 0 {
        ("reserved for NCS compatibility", PyObject::none())
    } else if variant_byte & 0xC0 == 0x80 {
        (
            "specified in RFC 4122",
            PyObject::int(((bytes[6] >> 4) & 0x0f) as i64),
        )
    } else if variant_byte & 0xE0 == 0xC0 {
        ("reserved for Microsoft compatibility", PyObject::none())
    } else {
        ("reserved for future definition", PyObject::none())
    };

    let int_val = uuid_int_from_bytes(&bytes);
    let time_low = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let time_mid = u16::from_be_bytes([bytes[4], bytes[5]]);
    let time_hi_ver = u16::from_be_bytes([bytes[6], bytes[7]]);
    let clock_hi = bytes[8];
    let clock_low = bytes[9];
    let node = bytes[10..16]
        .iter()
        .fold(0u64, |acc, &b| (acc << 8) | b as u64);
    let time =
        (((time_hi_ver as u64) & 0x0fff) << 48) | ((time_mid as u64) << 32) | time_low as u64;
    let clock_seq = (((clock_hi as u16) & 0x3f) << 8) | clock_low as u16;

    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("hex"),
        PyObject::str_val(CompactString::from(&hex_flat)),
    );
    attrs.insert(CompactString::from("version"), version_obj);
    attrs.insert(
        CompactString::from("variant"),
        PyObject::str_val(CompactString::from(variant)),
    );
    attrs.insert(CompactString::from("int"), py_int_from_bigint(int_val));
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
    attrs.insert(CompactString::from("time"), PyObject::int(time as i64));
    attrs.insert(
        CompactString::from("clock_seq"),
        PyObject::int(clock_seq as i64),
    );
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
    attrs.insert(CompactString::from("is_safe"), is_safe);
    attrs.insert(
        CompactString::from("__str_val__"),
        PyObject::str_val(CompactString::from(&hex_str)),
    );
    attrs.insert(CompactString::from("__uuid__"), PyObject::bool_val(true));
    PyObject::instance_with_attrs(cls, attrs)
}

fn make_uuid_from_hex(hex: &str, cls: &PyObjectRef) -> PyObjectRef {
    let clean = clean_uuid_hex(hex);
    let bytes = hex_to_bytes(&clean);
    build_uuid_object(bytes, cls.clone(), PyObject::none())
}

fn uuid_uuid4(_args: &[PyObjectRef], cls: &PyObjectRef) -> PyResult<PyObjectRef> {
    let mut bytes = random_uuid_bytes();
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    Ok(build_uuid_object(bytes, cls.clone(), PyObject::none()))
}

fn uuid_uuid1(args: &[PyObjectRef], cls: &PyObjectRef) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = split_kwargs(args);
    let node_arg = pos.first().cloned().or_else(|| kwargs_get(kwargs, "node"));
    let clock_seq_arg = pos
        .get(1)
        .cloned()
        .or_else(|| kwargs_get(kwargs, "clock_seq"));
    if pos.len() > 2 {
        return Err(PyException::type_error(
            "uuid1() takes at most 2 positional arguments",
        ));
    }

    let ticks = py_time_ns() / 100 + 0x01b21dd213814000;
    let mut bytes = [0u8; 16];
    let tl = (ticks & 0xFFFFFFFF) as u32;
    bytes[0..4].copy_from_slice(&tl.to_be_bytes());
    let tm = ((ticks >> 32) & 0xFFFF) as u16;
    bytes[4..6].copy_from_slice(&tm.to_be_bytes());
    let th = (((ticks >> 48) & 0x0FFF) | 0x1000) as u16;
    bytes[6..8].copy_from_slice(&th.to_be_bytes());

    let clock_seq = if let Some(arg) = clock_seq_arg {
        let value = int_bigint(&arg)
            .ok_or_else(|| PyException::value_error("clock_seq is out of range"))?;
        bigint_to_ranged_u64(&value, 14, "clock_seq")? as u16
    } else {
        py_random_getrandbits(14)
            .map(|value| value as u16 & 0x3fff)
            .unwrap_or_else(|| {
                let rand = random_uuid_bytes();
                u16::from_be_bytes([rand[8], rand[9]]) & 0x3fff
            })
    };
    bytes[8] = ((clock_seq >> 8) as u8 & 0x3f) | 0x80;
    bytes[9] = clock_seq as u8;

    let node = if let Some(arg) = node_arg {
        let value =
            int_bigint(&arg).ok_or_else(|| PyException::value_error("node is out of range"))?;
        bigint_to_ranged_u64(&value, 48, "node")?
    } else if let Some(node_obj) = call_module_attr("uuid", "getnode", &[]) {
        let value = int_bigint(&node_obj)
            .ok_or_else(|| PyException::value_error("node is out of range"))?;
        bigint_to_ranged_u64(&value, 48, "node")?
    } else {
        let rand = random_uuid_bytes();
        rand[10..16]
            .iter()
            .fold(0u64, |acc, &b| (acc << 8) | b as u64)
    };
    for i in 0..6 {
        bytes[10 + i] = ((node >> (8 * (5 - i))) & 0xff) as u8;
    }
    Ok(build_uuid_object(bytes, cls.clone(), PyObject::none()))
}

fn uuid_uuid3(args: &[PyObjectRef], cls: &PyObjectRef) -> PyResult<PyObjectRef> {
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
    Ok(build_uuid_object(bytes, cls.clone(), PyObject::none()))
}

fn uuid_uuid5(args: &[PyObjectRef], cls: &PyObjectRef) -> PyResult<PyObjectRef> {
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
    Ok(build_uuid_object(bytes, cls.clone(), PyObject::none()))
}

fn get_uuid_bytes(obj: &PyObjectRef) -> PyResult<[u8; 16]> {
    if let PyObjectPayload::Instance(ref inst) = obj.payload {
        let attrs = inst.attrs.read();
        if attrs.contains_key("__uuid__") {
            if let Some(b) = attrs.get("bytes") {
                if let PyObjectPayload::Bytes(v) = &b.payload {
                    if v.len() == 16 {
                        let mut arr = [0u8; 16];
                        arr.copy_from_slice(v);
                        return Ok(arr);
                    }
                }
            }
        }
    }
    let s = clean_uuid_hex(&obj.py_to_string());
    if s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(hex_to_bytes(&s));
    }
    Err(PyException::type_error("expected a UUID object"))
}

fn uuid_hex_attr(obj: &PyObjectRef) -> Option<String> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        if attrs.contains_key("__uuid__") {
            return attrs.get("hex").map(|h| h.py_to_string());
        }
    }
    None
}

fn uuid_bigint_attr(obj: &PyObjectRef) -> Option<BigInt> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        if attrs.contains_key("__uuid__") {
            return attrs.get("int").and_then(int_bigint);
        }
    }
    None
}

fn uuid_obj_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let Some(this) = args.first() else {
        return Err(PyException::type_error("UUID.__str__ requires self"));
    };
    let text = this
        .get_attr("__str_val__")
        .map(|v| v.py_to_string())
        .unwrap_or_else(|| "00000000-0000-0000-0000-000000000000".to_string());
    Ok(PyObject::str_val(CompactString::from(text)))
}

fn uuid_obj_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let Some(this) = args.first() else {
        return Err(PyException::type_error("UUID.__repr__ requires self"));
    };
    let text = this
        .get_attr("__str_val__")
        .map(|v| v.py_to_string())
        .unwrap_or_else(|| "00000000-0000-0000-0000-000000000000".to_string());
    Ok(PyObject::str_val(CompactString::from(format!(
        "UUID('{}')",
        text
    ))))
}

fn uuid_obj_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::not_implemented());
    }
    let Some(left) = uuid_hex_attr(&args[0]) else {
        return Ok(PyObject::not_implemented());
    };
    let Some(right) = uuid_hex_attr(&args[1]) else {
        return Ok(PyObject::bool_val(false));
    };
    Ok(PyObject::bool_val(left == right))
}

fn uuid_obj_ne(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let eq = uuid_obj_eq(args)?;
    if matches!(&eq.payload, PyObjectPayload::NotImplemented) {
        return Ok(eq);
    }
    Ok(PyObject::bool_val(!eq.is_truthy()))
}

fn uuid_compare_order(args: &[PyObjectRef], want: fn(Ordering) -> bool) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::not_implemented());
    }
    let Some(left) = uuid_bigint_attr(&args[0]) else {
        return Ok(PyObject::not_implemented());
    };
    let Some(right) = uuid_bigint_attr(&args[1]) else {
        return Ok(PyObject::not_implemented());
    };
    Ok(PyObject::bool_val(want(left.cmp(&right))))
}

fn uuid_obj_lt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    uuid_compare_order(args, |ord| ord == Ordering::Less)
}

fn uuid_obj_le(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    uuid_compare_order(args, |ord| matches!(ord, Ordering::Less | Ordering::Equal))
}

fn uuid_obj_gt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    uuid_compare_order(args, |ord| ord == Ordering::Greater)
}

fn uuid_obj_ge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    uuid_compare_order(args, |ord| {
        matches!(ord, Ordering::Greater | Ordering::Equal)
    })
}

fn uuid_obj_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let Some(hex) = args.first().and_then(uuid_hex_attr) else {
        return Err(PyException::type_error("UUID.__hash__ requires self"));
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hex.hash(&mut hasher);
    Ok(PyObject::int(hasher.finish() as i64))
}

fn uuid_obj_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let Some(this) = args.first() else {
        return Err(PyException::type_error("UUID.__int__ requires self"));
    };
    if let PyObjectPayload::Instance(inst) = &this.payload {
        if let Some(value) = inst.attrs.read().get("int").cloned() {
            return Ok(value);
        }
    }
    Ok(PyObject::int(0))
}

fn uuid_obj_getnewargs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let int_value = uuid_obj_int(args)?;
    Ok(PyObject::tuple(vec![int_value]))
}

fn uuid_obj_setattr(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Err(PyException::type_error("UUID objects are immutable"))
}

#[allow(non_snake_case)]
fn uuid_UUID(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cls = if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
        args[0].clone()
    } else {
        build_uuid_class()
    };
    uuid_constructor(args, cls)
}
