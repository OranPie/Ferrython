//! Miscellaneous stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, CompareOp, InstanceData,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

use super::serial_modules::extract_bytes;

pub fn create_typing_module() -> PyObjectRef {
    let mut attrs: Vec<(&str, PyObjectRef)> = vec![
        ("Any", PyObject::none()),
        ("Union", PyObject::none()),
        ("Optional", PyObject::none()),
        ("List", PyObject::builtin_type(CompactString::from("list"))),
        ("Dict", PyObject::builtin_type(CompactString::from("dict"))),
        ("Set", PyObject::builtin_type(CompactString::from("set"))),
        ("Tuple", PyObject::builtin_type(CompactString::from("tuple"))),
        ("FrozenSet", PyObject::builtin_type(CompactString::from("frozenset"))),
        ("Type", PyObject::builtin_type(CompactString::from("type"))),
        ("Callable", PyObject::none()),
        ("Iterator", PyObject::none()),
        ("Generator", PyObject::none()),
        ("Sequence", PyObject::none()),
        ("Mapping", PyObject::none()),
        ("MutableMapping", PyObject::none()),
        ("Iterable", PyObject::none()),
    ];
    attrs.push(("TYPE_CHECKING", PyObject::bool_val(false)));
    make_module("typing", attrs)
}

// ── abc module (stub) ──


pub fn create_abc_module() -> PyObjectRef {
    make_module("abc", vec![
        ("ABC", PyObject::none()),
        ("ABCMeta", PyObject::none()),
        ("abstractmethod", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("abstractmethod requires 1 argument")); }
            Ok(args[0].clone())
        })),
    ])
}

// ── enum module (stub) ──


pub fn create_enum_module() -> PyObjectRef {
    // Create Enum as a base class marker
    let enum_class = PyObject::class(
        CompactString::from("Enum"),
        vec![],
        IndexMap::new(),
    );
    // Mark it as enum base
    if let PyObjectPayload::Class(ref cd) = enum_class.payload {
        cd.namespace.write().insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    }
    let int_enum = PyObject::class(
        CompactString::from("IntEnum"),
        vec![enum_class.clone()],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = int_enum.payload {
        cd.namespace.write().insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    }
    
    // auto() counter
    static AUTO_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
    
    make_module("enum", vec![
        ("Enum", enum_class),
        ("IntEnum", int_enum),
        ("Flag", PyObject::none()),
        ("IntFlag", PyObject::none()),
        ("auto", make_builtin(|_| {
            let val = AUTO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(PyObject::int(val))
        })),
        ("unique", make_builtin(|args| {
            if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
        })),
    ])
}

// ── contextlib module ──


pub fn create_contextlib_module() -> PyObjectRef {
    make_module("contextlib", vec![
        ("contextmanager", make_builtin(contextlib_contextmanager)),
        ("suppress", make_builtin(|_args| {
            // Stub: returns a no-op context manager
            Ok(make_module("suppress_cm", vec![
                ("__enter__", make_builtin(|_| Ok(PyObject::none()))),
                ("__exit__", make_builtin(|_| Ok(PyObject::bool_val(true)))),
            ]))
        })),
        ("closing", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("closing requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("ExitStack", make_builtin(|_| Ok(PyObject::none()))),
        ("redirect_stdout", make_builtin(|_| Ok(PyObject::none()))),
        ("redirect_stderr", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

fn contextlib_contextmanager(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // contextmanager decorator — returns the function unchanged.
    // The function is a generator function. When called, it returns a Generator.
    // The VM's SetupWith handles Generator objects as context managers directly.
    if args.is_empty() { return Err(PyException::type_error("contextmanager requires 1 argument")); }
    Ok(args[0].clone())
}

// ── dataclasses module ──


pub fn create_dataclasses_module() -> PyObjectRef {
    make_module("dataclasses", vec![
        ("dataclass", make_builtin(dataclass_decorator)),
        ("field", make_builtin(|args| {
            // Return a sentinel field object
            let default = if args.is_empty() { PyObject::none() } else { args[0].clone() };
            Ok(default)
        })),
        ("asdict", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("asdict requires 1 argument")); }
            // Convert instance attrs to dict
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                let mut map = IndexMap::new();
                for (k, v) in attrs.iter() {
                    if !k.starts_with('_') {
                        map.insert(HashableKey::Str(k.clone()), v.clone());
                    }
                }
                Ok(PyObject::dict(map))
            } else {
                Err(PyException::type_error("asdict() should be called on dataclass instances"))
            }
        })),
        ("astuple", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("astuple requires 1 argument")); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                let items: Vec<_> = attrs.values().cloned().collect();
                Ok(PyObject::tuple(items))
            } else {
                Err(PyException::type_error("astuple() should be called on dataclass instances"))
            }
        })),
        ("fields", make_builtin(|_| Ok(PyObject::tuple(vec![])))),
    ])
}

fn dataclass_decorator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("dataclass requires 1 argument")); }
    let cls = &args[0];
    
    // Get annotations to discover fields
    let mut field_names: Vec<CompactString> = Vec::new();
    let mut field_defaults: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let ns = cd.namespace.read();
        if let Some(annotations) = ns.get("__annotations__") {
            if let PyObjectPayload::Dict(ann_map) = &annotations.payload {
                for (k, _v) in ann_map.read().iter() {
                    if let HashableKey::Str(name) = k {
                        field_names.push(name.clone());
                        // Check for default value in class namespace
                        if let Some(default) = ns.get(name.as_str()) {
                            field_defaults.insert(name.clone(), default.clone());
                        }
                    }
                }
            }
        }
    }
    
    // Store __dataclass_fields__ as a tuple of (name, has_default, default_val) tuples
    let fields_info: Vec<PyObjectRef> = field_names.iter().map(|name| {
        let has_default = field_defaults.contains_key(name.as_str());
        let default_val = field_defaults.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
        PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(name.as_str())),
            PyObject::bool_val(has_default),
            default_val,
        ])
    }).collect();
    
    // Store on the class
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("__dataclass_fields__"), PyObject::tuple(fields_info));
        // Mark it as a dataclass
        ns.insert(CompactString::from("__dataclass__"), PyObject::bool_val(true));
    }
    
    Ok(cls.clone())
}

// ── struct module ──


pub fn create_copy_module() -> PyObjectRef {
    make_module("copy", vec![
        ("copy", make_builtin(copy_copy)),
        ("deepcopy", make_builtin(copy_deepcopy)),
    ])
}

fn copy_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("copy() requires 1 argument")); }
    shallow_copy(&args[0])
}

fn copy_deepcopy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("deepcopy() requires 1 argument")); }
    deep_copy(&args[0])
}

fn shallow_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => Ok(PyObject::tuple(items.clone())),
        PyObjectPayload::List(items) => Ok(PyObject::list(items.read().clone())),
        PyObjectPayload::Dict(map) => Ok(PyObject::dict(map.read().clone())),
        PyObjectPayload::Set(set) => Ok(PyObject::set(set.read().clone())),
        PyObjectPayload::Instance(inst) => {
            // Create new instance with same class, shallow copy of attrs
            Ok(PyObject::wrap(PyObjectPayload::Instance(InstanceData {
                class: inst.class.clone(),
                attrs: Arc::new(RwLock::new(inst.attrs.read().clone())),
            })))
        }
        _ => Ok(obj.clone()),
    }
}

fn deep_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => {
            let new_items: Result<Vec<_>, _> = items.iter().map(|x| deep_copy(x)).collect();
            Ok(PyObject::tuple(new_items?))
        }
        PyObjectPayload::List(items) => {
            let new_items: Result<Vec<_>, _> = items.read().iter().map(|x| deep_copy(x)).collect();
            Ok(PyObject::list(new_items?))
        }
        PyObjectPayload::Dict(map) => {
            let mut new_map = IndexMap::new();
            for (k, v) in map.read().iter() {
                new_map.insert(k.clone(), deep_copy(v)?);
            }
            Ok(PyObject::dict(new_map))
        }
        PyObjectPayload::Set(set) => {
            Ok(PyObject::set(set.read().clone()))
        }
        _ => Ok(obj.clone()),
    }
}

// ── operator module ──


pub fn create_operator_module() -> PyObjectRef {
    make_module("operator", vec![
        ("add", make_builtin(|args| {
            check_args_min("add", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a + b));
                }
            }
            if let (Ok(a), Ok(b)) = (args[0].to_float(), args[1].to_float()) {
                Ok(PyObject::float(a + b))
            } else {
                let a = args[0].py_to_string();
                let b = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(format!("{}{}", a, b))))
            }
        })),
        ("sub", make_builtin(|args| {
            check_args_min("sub", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a - b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a - b))
        })),
        ("mul", make_builtin(|args| {
            check_args_min("mul", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a * b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a * b))
        })),
        ("truediv", make_builtin(|args| {
            check_args_min("truediv", args, 2)?;
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
            Ok(PyObject::float(a / b))
        })),
        ("floordiv", make_builtin(|args| {
            check_args_min("floordiv", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.div_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("float floor division by zero")); }
            Ok(PyObject::float((a / b).floor()))
        })),
        ("mod_", make_builtin(|args| {
            check_args_min("mod_", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        ("neg", make_builtin(|args| {
            check_args_min("neg", args, 1)?;
            if matches!(&args[0].payload, PyObjectPayload::Float(_)) {
                Ok(PyObject::float(-args[0].to_float()?))
            } else if let Ok(n) = args[0].to_int() {
                Ok(PyObject::int(-n))
            } else {
                Ok(PyObject::float(-args[0].to_float()?))
            }
        })),
        ("pos", make_builtin(|args| {
            check_args_min("pos", args, 1)?;
            Ok(args[0].clone())
        })),
        ("not_", make_builtin(|args| {
            check_args_min("not_", args, 1)?;
            Ok(PyObject::bool_val(!args[0].is_truthy()))
        })),
        ("eq", make_builtin(|args| {
            check_args_min("eq", args, 2)?;
            args[0].compare(&args[1], CompareOp::Eq)
        })),
        ("ne", make_builtin(|args| {
            check_args_min("ne", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ne)
        })),
        ("lt", make_builtin(|args| {
            check_args_min("lt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Lt)
        })),
        ("le", make_builtin(|args| {
            check_args_min("le", args, 2)?;
            args[0].compare(&args[1], CompareOp::Le)
        })),
        ("gt", make_builtin(|args| {
            check_args_min("gt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Gt)
        })),
        ("ge", make_builtin(|args| {
            check_args_min("ge", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ge)
        })),
        ("abs", make_builtin(|args| {
            check_args_min("abs", args, 1)?;
            check_args("abs", args, 1)?;
            args[0].py_abs()
        })),
        ("contains", make_builtin(|args| {
            check_args_min("contains", args, 2)?;
            Ok(PyObject::bool_val(args[0].contains(&args[1])?))
        })),
        ("getitem", make_builtin(|args| {
            check_args_min("getitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.read().get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("list index out of range"))
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.read().get(&key).cloned()
                        .ok_or_else(|| PyException::key_error(args[1].repr()))
                }
                PyObjectPayload::Tuple(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("tuple index out of range"))
                }
                _ => Err(PyException::type_error("object is not subscriptable")),
            }
        })),
        ("itemgetter", make_builtin(|args| {
            // Returns a Module-like callable that extracts an item
            check_args_min("itemgetter", args, 1)?;
            let key = args[0].clone();
            let mut attrs = vec![
                ("_key", key),
            ];
            attrs.push(("_bind_methods", PyObject::bool_val(true)));
            Ok(make_module("itemgetter", attrs))
        })),
        ("attrgetter", make_builtin(|args| {
            check_args_min("attrgetter", args, 1)?;
            let attr_name = args[0].clone();
            let mut attrs = vec![
                ("_attr", attr_name),
            ];
            attrs.push(("_bind_methods", PyObject::bool_val(true)));
            Ok(make_module("attrgetter", attrs))
        })),
    ])
}

// ── typing module (stub) ──


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

fn make_hash_object(name: &str, digest_hex: String, digest_bytes: Vec<u8>, block_size: i64, digest_size: i64) -> PyObjectRef {
    let class = PyObject::class(CompactString::from(name), vec![], IndexMap::new());
    let attrs = IndexMap::new();
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class: class.clone(),
        attrs: Arc::new(RwLock::new(attrs)),
    }));
    {
        let a = if let PyObjectPayload::Instance(ref d) = inst.payload { d.attrs.clone() } else { unreachable!() };
        let mut w = a.write();
        w.insert(CompactString::from("_hexdigest"), PyObject::str_val(CompactString::from(&digest_hex)));
        w.insert(CompactString::from("_digest"), PyObject::bytes(digest_bytes));
        w.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name)));
        w.insert(CompactString::from("block_size"), PyObject::int(block_size));
        w.insert(CompactString::from("digest_size"), PyObject::int(digest_size));
    }
    inst
}

fn hashlib_md5(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use md5::Md5;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Md5::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("md5", hex, result.to_vec(), 64, 16))
}

fn hashlib_sha1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha1::Sha1;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha1::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha1", hex, result.to_vec(), 64, 20))
}

fn hashlib_sha256(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha256;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha256", hex, result.to_vec(), 64, 32))
}

fn hashlib_sha224(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha224;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha224::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha224", hex, result.to_vec(), 64, 28))
}

fn hashlib_sha384(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha384;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha384::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha384", hex, result.to_vec(), 128, 48))
}

fn hashlib_sha512(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha512;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha512::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha512", hex, result.to_vec(), 128, 64))
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

// ── copy module ──


pub fn create_logging_module() -> PyObjectRef {
    // Logging levels
    let debug_level = PyObject::int(10);
    let info_level = PyObject::int(20);
    let warning_level = PyObject::int(30);
    let error_level = PyObject::int(40);
    let critical_level = PyObject::int(50);

    make_module("logging", vec![
        ("DEBUG", debug_level),
        ("INFO", info_level),
        ("WARNING", warning_level.clone()),
        ("ERROR", error_level),
        ("CRITICAL", critical_level),
        ("NOTSET", PyObject::int(0)),
        ("basicConfig", make_builtin(|_args| { Ok(PyObject::none()) })),
        ("getLogger", make_builtin(logging_get_logger)),
        ("debug", make_builtin(|args| { logging_log(10, args) })),
        ("info", make_builtin(|args| { logging_log(20, args) })),
        ("warning", make_builtin(|args| { logging_log(30, args) })),
        ("error", make_builtin(|args| { logging_log(40, args) })),
        ("critical", make_builtin(|args| { logging_log(50, args) })),
        ("log", make_builtin(|args| {
            if args.len() >= 2 {
                let level = args[0].as_int().unwrap_or(20);
                logging_log(level, &args[1..])
            } else {
                Ok(PyObject::none())
            }
        })),
        ("StreamHandler", make_builtin(|_| Ok(PyObject::none()))),
        ("FileHandler", make_builtin(|_| Ok(PyObject::none()))),
        ("Formatter", make_builtin(|_| Ok(PyObject::none()))),
        ("Handler", make_builtin(|_| Ok(PyObject::none()))),
        ("Logger", make_builtin(logging_get_logger)),
    ])
}

fn logging_log(level: i64, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::none()); }
    let level_name = match level {
        10 => "DEBUG",
        20 => "INFO",
        30 => "WARNING",
        40 => "ERROR",
        50 => "CRITICAL",
        _ => "UNKNOWN",
    };
    let msg = args[0].py_to_string();
    eprintln!("{}:root:{}", level_name, msg);
    Ok(PyObject::none())
}

fn logging_get_logger(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let logger_name = if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
        CompactString::from("root")
    } else {
        CompactString::from(args[0].py_to_string())
    };
    // Return a logger object (Instance of a Logger class)
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("name"), PyObject::str_val(logger_name.clone()));
    ns.insert(CompactString::from("level"), PyObject::int(30)); // WARNING default
    // Logger methods — stored as NativeFunction attrs
    ns.insert(CompactString::from("debug"), make_builtin(move |args| logging_log(10, args)));
    ns.insert(CompactString::from("info"), make_builtin(move |args| logging_log(20, args)));
    ns.insert(CompactString::from("warning"), make_builtin(move |args| logging_log(30, args)));
    ns.insert(CompactString::from("error"), make_builtin(move |args| logging_log(40, args)));
    ns.insert(CompactString::from("critical"), make_builtin(move |args| logging_log(50, args)));
    ns.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
    ns.insert(CompactString::from("addHandler"), make_builtin(|_| Ok(PyObject::none())));
    ns.insert(CompactString::from("removeHandler"), make_builtin(|_| Ok(PyObject::none())));
    ns.insert(CompactString::from("hasHandlers"), make_builtin(|_| Ok(PyObject::bool_val(false))));
    ns.insert(CompactString::from("isEnabledFor"), make_builtin(|_| Ok(PyObject::bool_val(true))));
    ns.insert(CompactString::from("getEffectiveLevel"), make_builtin(|_| Ok(PyObject::int(30))));
    
    let cls = PyObject::class(CompactString::from("Logger"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        for (k, v) in ns {
            attrs.insert(k, v);
        }
    }
    Ok(inst)
}

// ── subprocess module (basic) ──


pub fn create_warnings_module() -> PyObjectRef {
    make_module("warnings", vec![
        ("warn", make_builtin(|_| Ok(PyObject::none()))),
        ("filterwarnings", make_builtin(|_| Ok(PyObject::none()))),
        ("simplefilter", make_builtin(|_| Ok(PyObject::none()))),
        ("resetwarnings", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── decimal module (stub) ──


pub fn create_traceback_module() -> PyObjectRef {
    make_module("traceback", vec![
        ("format_exc", make_builtin(|_| Ok(PyObject::str_val(CompactString::from(""))))),
        ("print_exc", make_builtin(|_| Ok(PyObject::none()))),
        ("format_exception", make_builtin(|_| Ok(PyObject::list(vec![])))),
        ("print_stack", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── warnings module (stub) ──


pub fn create_inspect_module() -> PyObjectRef {
    make_module("inspect", vec![
        ("isfunction", make_builtin(|args| {
            check_args("inspect.isfunction", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Function(_))))
        })),
        ("isclass", make_builtin(|args| {
            check_args("inspect.isclass", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Class(_))))
        })),
        ("ismethod", make_builtin(|args| {
            check_args("inspect.ismethod", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::BoundMethod { .. })))
        })),
        ("ismodule", make_builtin(|args| {
            check_args("inspect.ismodule", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Module(_))))
        })),
        ("isbuiltin", make_builtin(|args| {
            check_args("inspect.isbuiltin", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::NativeFunction { .. } | PyObjectPayload::BuiltinFunction(_) | PyObjectPayload::BuiltinType(_))))
        })),
        ("getmembers", make_builtin(|args| {
            check_args("inspect.getmembers", args, 1)?;
            let dir_names = args[0].dir();
            let dir_list: Vec<PyObjectRef> = dir_names.into_iter().map(|n| PyObject::str_val(n)).collect();
            let names = PyObject::list(dir_list);
            let mut result = Vec::new();
            if let PyObjectPayload::List(items) = &names.payload {
                for item in items.read().iter() {
                    let name_str = item.py_to_string();
                    if let Some(val) = args[0].get_attr(&name_str) {
                        result.push(PyObject::tuple(vec![item.clone(), val]));
                    }
                }
            }
            Ok(PyObject::list(result))
        })),
    ])
}

// ── dis module (stub) ──


pub fn create_dis_module() -> PyObjectRef {
    make_module("dis", vec![
        ("dis", make_builtin(|_| { Ok(PyObject::none()) })),
    ])
}

// ── logging module ──


pub fn create_threading_module() -> PyObjectRef {
    make_module("threading", vec![
        ("Thread", make_builtin(|_| Ok(PyObject::none()))),
        ("Lock", make_builtin(|_| Ok(PyObject::none()))),
        ("RLock", make_builtin(|_| Ok(PyObject::none()))),
        ("Event", make_builtin(|_| Ok(PyObject::none()))),
        ("Semaphore", make_builtin(|_| Ok(PyObject::none()))),
        ("BoundedSemaphore", make_builtin(|_| Ok(PyObject::none()))),
        ("Condition", make_builtin(|_| Ok(PyObject::none()))),
        ("Barrier", make_builtin(|_| Ok(PyObject::none()))),
        ("Timer", make_builtin(|_| Ok(PyObject::none()))),
        ("current_thread", make_builtin(|_| {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("MainThread")));
            ns.insert(CompactString::from("ident"), PyObject::int(1));
            ns.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            ns.insert(CompactString::from("is_alive"), make_builtin(|_| Ok(PyObject::bool_val(true))));
            ns.insert(CompactString::from("getName"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("MainThread")))));
            let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(i) = &inst.payload {
                let mut attrs = i.attrs.write();
                for (k, v) in ns { attrs.insert(k, v); }
            }
            Ok(inst)
        })),
        ("active_count", make_builtin(|_| Ok(PyObject::int(1)))),
        ("enumerate", make_builtin(|_| Ok(PyObject::list(vec![])))),
        ("main_thread", make_builtin(|_| Ok(PyObject::none()))),
        ("local", make_builtin(|_| {
            // Thread-local storage — return a simple object
            let cls = PyObject::class(CompactString::from("local"), vec![], IndexMap::new());
            Ok(PyObject::instance(cls))
        })),
    ])
}

// ── csv module (basic) ──


pub fn create_unittest_module() -> PyObjectRef {
    // Create TestCase class
    let mut tc_ns = IndexMap::new();
    tc_ns.insert(CompactString::from("__unittest_testcase__"), PyObject::bool_val(true));
    let test_case = PyObject::class(CompactString::from("TestCase"), vec![], tc_ns);

    make_module("unittest", vec![
        ("TestCase", test_case),
        ("main", make_builtin(|_| Ok(PyObject::none()))),
        ("TestSuite", make_builtin(|_| Ok(PyObject::none()))),
        ("TestLoader", make_builtin(|_| Ok(PyObject::none()))),
        ("TextTestRunner", make_builtin(|_| Ok(PyObject::none()))),
        ("skip", make_builtin(|_args| {
            // Return identity decorator
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("skipIf", make_builtin(|_| {
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("expectedFailure", make_builtin(|args| {
            if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
        })),
    ])
}

// ── threading module (basic) ──


pub fn create_pprint_module() -> PyObjectRef {
    make_module("pprint", vec![
        ("pprint", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::none()); }
            println!("{}", args[0].py_to_string());
            Ok(PyObject::none())
        })),
        ("pformat", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
        })),
        ("PrettyPrinter", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── argparse module (basic) ──


pub fn create_argparse_module() -> PyObjectRef {
    let mut ap_ns = IndexMap::new();
    ap_ns.insert(CompactString::from("__argparse__"), PyObject::bool_val(true));
    let argument_parser = PyObject::class(CompactString::from("ArgumentParser"), vec![], ap_ns);

    make_module("argparse", vec![
        ("ArgumentParser", argument_parser),
        ("Namespace", make_builtin(|_| Ok(PyObject::none()))),
        ("Action", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── datetime module ──


pub fn create_weakref_module() -> PyObjectRef {
    make_module("weakref", vec![
        ("ref", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("ref requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("proxy", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("proxy requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("WeakValueDictionary", make_builtin(|_| Ok(PyObject::dict(IndexMap::new())))),
        ("WeakKeyDictionary", make_builtin(|_| Ok(PyObject::dict(IndexMap::new())))),
        ("WeakSet", make_builtin(|_| Ok(PyObject::set(IndexMap::new())))),
    ])
}

// ── gc module ──


pub fn create_gc_module() -> PyObjectRef {
    make_module("gc", vec![
        ("enable", make_builtin(|_| {
            ferrython_gc::enable();
            Ok(PyObject::none())
        })),
        ("disable", make_builtin(|_| {
            ferrython_gc::disable();
            Ok(PyObject::none())
        })),
        ("isenabled", make_builtin(|_| {
            Ok(PyObject::bool_val(ferrython_gc::is_enabled()))
        })),
        ("collect", make_builtin(|_| {
            let collected = ferrython_gc::collect();
            Ok(PyObject::int(collected as i64))
        })),
        ("get_threshold", make_builtin(|_| {
            let (g0, g1, g2) = ferrython_gc::get_threshold();
            Ok(PyObject::tuple(vec![
                PyObject::int(g0 as i64),
                PyObject::int(g1 as i64),
                PyObject::int(g2 as i64),
            ]))
        })),
        ("set_threshold", make_builtin(|args| {
            check_args_min("gc.set_threshold", args, 1)?;
            let g0 = args[0].as_int().ok_or_else(|| {
                PyException::type_error("threshold must be an integer")
            })? as u64;
            let g1 = args.get(1).and_then(|a| a.as_int()).unwrap_or(10) as u64;
            let g2 = args.get(2).and_then(|a| a.as_int()).unwrap_or(10) as u64;
            ferrython_gc::set_threshold(g0, g1, g2);
            Ok(PyObject::none())
        })),
        ("get_stats", make_builtin(|_| {
            let stats = ferrython_gc::get_stats();
            let entry = PyObject::dict({
                let mut m = IndexMap::new();
                m.insert(
                    HashableKey::Str(CompactString::from("collections")),
                    PyObject::int(stats.collections as i64),
                );
                m.insert(
                    HashableKey::Str(CompactString::from("collected")),
                    PyObject::int(0),
                );
                m.insert(
                    HashableKey::Str(CompactString::from("uncollectable")),
                    PyObject::int(0),
                );
                m
            });
            // CPython returns a list of 3 dicts, one per generation
            Ok(PyObject::list(vec![entry.clone(), entry.clone(), entry]))
        })),
        ("get_count", make_builtin(|_| {
            let stats = ferrython_gc::get_stats();
            Ok(PyObject::tuple(vec![
                PyObject::int(stats.allocations as i64),
                PyObject::int(0),
                PyObject::int(0),
            ]))
        })),
    ])
}


