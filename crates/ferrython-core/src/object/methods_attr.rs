//! Attribute lookup methods and descriptor protocol helpers.

use crate::error::{PyException, ExceptionKind};
use crate::types::{HashableKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use std::sync::Arc;

use super::payload::*;
use super::helpers::*;
use super::methods::PyObjectMethods;

/// Walk a class and its base classes (MRO) to find an attribute.
pub fn lookup_in_class_mro(class: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    if let PyObjectPayload::Class(cd) = &class.payload {
        // Check own namespace first
        if let Some(v) = cd.namespace.read().get(name).cloned() {
            return Some(v);
        }
        // Use computed MRO if available, otherwise walk bases recursively
        if !cd.mro.is_empty() {
            for base in &cd.mro {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    if let Some(v) = bcd.namespace.read().get(name).cloned() {
                        return Some(v);
                    }
                }
            }
        } else {
            for base in &cd.bases {
                if let Some(v) = lookup_in_class_mro(base, name) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Check if an object is a data descriptor (has __set__ or __delete__).
/// Data descriptors take priority over instance __dict__ in attribute lookup.
pub fn is_data_descriptor(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Property { .. } => true,
        PyObjectPayload::Instance(inst) => {
            has_method_in_class(&inst.class, "__set__")
                || has_method_in_class(&inst.class, "__delete__")
        }
        _ => false,
    }
}

/// Check if an object has __get__ (i.e. is any kind of descriptor).
pub fn has_descriptor_get(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Property { .. } => true,
        PyObjectPayload::Instance(inst) => {
            has_method_in_class(&inst.class, "__get__")
        }
        _ => false,
    }
}

/// Check if a class (or its MRO) has a method by name.
fn has_method_in_class(class: &PyObjectRef, name: &str) -> bool {
    lookup_in_class_mro(class, name).is_some()
}



/// Returns a BuiltinBoundMethod if the method name matches, None otherwise.
fn instance_builtin_method(obj: &PyObjectRef, inst: &InstanceData, name: &str) -> Option<PyObjectRef> {
    let make_bound = |name: &str| -> PyObjectRef {
        Arc::new(PyObject {
            payload: PyObjectPayload::BuiltinBoundMethod {
                receiver: obj.clone(),
                method_name: CompactString::from(name),
            }
        })
    };

    // Dict subclass: expose dict methods bound to the instance
    if inst.dict_storage.is_some() {
        if matches!(name, "keys" | "values" | "items" | "get" | "pop" | "update"
            | "setdefault" | "clear" | "copy" | "popitem" | "fromkeys" | "move_to_end")
        {
            return Some(make_bound(name));
        }
    }

    // Namedtuple
    if inst.class.get_attr("__namedtuple__").is_some() {
        if name == "_fields" {
            return inst.class.get_attr("_fields");
        }
        if matches!(name, "_asdict" | "_replace" | "_make" | "__len__" | "__iter__") {
            return Some(make_bound(name));
        }
    }

    // Deque
    if inst.attrs.read().contains_key("__deque__") {
        // maxlen is a property, not a method — return the value directly
        if name == "maxlen" {
            return Some(inst.attrs.read().get("__maxlen__").cloned().unwrap_or_else(PyObject::none));
        }
        if matches!(name, "append" | "appendleft" | "pop" | "popleft" | "extend"
            | "extendleft" | "rotate" | "clear" | "copy" | "count" | "index"
            | "insert" | "remove" | "reverse"
            | "__iter__" | "__len__" | "__contains__" | "__getitem__")
        {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    // StringIO
    if inst.attrs.read().contains_key("__stringio__") {
        if matches!(name, "write" | "read" | "getvalue" | "seek" | "tell" | "close" | "closed"
            | "readline" | "readlines" | "writelines" | "truncate" | "readable" | "writable" | "seekable"
            | "__iter__" | "__next__" | "__enter__" | "__exit__")
        {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    // BytesIO
    if inst.attrs.read().contains_key("__bytesio__") {
        if matches!(name, "write" | "read" | "getvalue" | "seek" | "tell" | "close"
            | "readline" | "readlines" | "truncate" | "readable" | "writable" | "seekable")
        {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    // pathlib.Path
    if inst.attrs.read().contains_key("__pathlib_path__") {
        // Check instance attrs first for properties (name, stem, suffix, suffixes, parts, parent)
        if let Some(v) = inst.attrs.read().get(name).cloned() {
            return Some(v);
        }
        // Methods are returned as bound methods
        if matches!(name, "exists" | "is_file" | "is_dir" | "is_absolute" | "is_symlink"
            | "__str__" | "__fspath__" | "__repr__"
            | "resolve" | "absolute" | "as_posix" | "relative_to"
            | "with_suffix" | "with_name"
            | "read_text" | "read_bytes" | "write_text" | "write_bytes"
            | "mkdir" | "rmdir" | "unlink" | "iterdir" | "glob" | "stat"
            | "joinpath" | "__truediv__"
            | "touch" | "rglob" | "chmod" | "match" | "samefile" | "rename" | "replace" | "open")
        {
            return Some(make_bound(name));
        }
        return None;
    }

    // Hashlib hash objects
    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.as_str() } else { "" };
    if matches!(class_name, "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512") {
        if matches!(name, "hexdigest" | "digest" | "update" | "copy") {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    // CSV writer
    if inst.attrs.read().contains_key("__csv_writer__") {
        if matches!(name, "writerow" | "writerows") {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    // CSV DictWriter
    if inst.attrs.read().contains_key("__csv_dictwriter__") {
        if matches!(name, "writeheader" | "writerow" | "writerows") {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    // datetime instances
    if inst.attrs.read().contains_key("__datetime__") {
        if matches!(name, "strftime" | "isoformat" | "timestamp" | "replace" | "date" | "time"
            | "timetuple" | "weekday" | "isoweekday" | "toordinal" | "ctime" | "__str__" | "__repr__"
            | "astimezone" | "utcoffset" | "tzname")
        {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    // timedelta instances
    if inst.attrs.read().contains_key("__timedelta__") {
        if matches!(name, "total_seconds" | "__str__" | "__repr__") {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    // timezone instances
    if inst.attrs.read().contains_key("__timezone__") {
        return inst.attrs.read().get(name).cloned();
    }

    // queue instances
    if inst.attrs.read().contains_key("__queue__") {
        if matches!(name, "put" | "get" | "empty" | "full" | "qsize" | "get_nowait" | "put_nowait"
            | "task_done" | "join")
        {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    None
}

pub(super) fn py_get_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                // Special instance attributes
                if name == "__class__" {
                    return Some(inst.class.clone());
                }
                if name == "__dict__" {
                    // If __slots__ is defined without __dict__ in it, raise AttributeError
                    let has_slots_no_dict = {
                        let mut found_slots = false;
                        let mut dict_in_slots = false;
                        let classes: Vec<PyObjectRef> = {
                            let mut v = vec![inst.class.clone()];
                            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                                v.extend(cd.mro.clone());
                                v.extend(cd.bases.clone());
                            }
                            v
                        };
                        for cls in &classes {
                            if let PyObjectPayload::Class(cd) = &cls.payload {
                                if let Some(slots) = cd.namespace.read().get("__slots__").cloned() {
                                    if matches!(&slots.payload, PyObjectPayload::List(_) | PyObjectPayload::Tuple(_)) {
                                        found_slots = true;
                                        if let Ok(items) = slots.to_list() {
                                            for item in &items {
                                                if item.py_to_string() == "__dict__" {
                                                    dict_in_slots = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        found_slots && !dict_in_slots
                    };
                    if has_slots_no_dict {
                        return None; // Will trigger AttributeError
                    }
                    return Some(PyObject::wrap(PyObjectPayload::InstanceDict(inst.attrs.clone())));
                }
                // Dict subclass: intercept dict method lookups before MRO returns BuiltinType methods
                if inst.dict_storage.is_some() {
                    if let Some(result) = instance_builtin_method(obj, inst, name) {
                        return Some(result);
                    }
                }
                // CPython descriptor protocol:
                // 1. Data descriptors (has __set__ or __delete__) from class MRO
                // 2. Instance __dict__
                // 3. Non-data descriptors (has __get__ only) and other class attrs
                let class_attr = lookup_in_class_mro(&inst.class, name);
                if let Some(ref v) = class_attr {
                    match &v.payload {
                        PyObjectPayload::Property { .. } => {
                            // Property is always a data descriptor — VM calls fget
                            return Some(v.clone());
                        }
                        PyObjectPayload::StaticMethod(func) => {
                            return Some(func.clone());
                        }
                        PyObjectPayload::ClassMethod(func) => {
                            return Some(Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: inst.class.clone(),
                                    method: func.clone(),
                                }
                            }));
                        }
                        _ => {
                            // Check if this is a custom data descriptor (has __set__ or __delete__)
                            if is_data_descriptor(v) {
                                // Data descriptor takes priority over instance dict
                                // Return it; VM's LoadAttr will call __get__
                                return Some(v.clone());
                            }
                        }
                    }
                }
                // Instance attributes (between data and non-data descriptors)
                if let Some(v) = inst.attrs.read().get(name) { return Some(v.clone()); }
                // Non-data descriptors and other class attrs
                if let Some(v) = class_attr {
                    // cached_property: if instance has the cached value, return it
                    if let PyObjectPayload::Instance(ref cp_inst) = v.payload {
                        if cp_inst.attrs.read().contains_key("__cached_property_func__") {
                            // This is a cached_property descriptor — return marker for VM
                            // The VM's LoadAttr handler will call the function and cache the result
                            return Some(v.clone());
                        }
                    }
                    if matches!(&v.payload, PyObjectPayload::Function(_)) {
                        return Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: obj.clone(),
                                method: v.clone(),
                            }
                        }));
                    }
                    // NativeFunction from class namespace → wrap to pass self
                    if matches!(&v.payload, PyObjectPayload::NativeFunction { .. }) {
                        let receiver = obj.clone();
                        let func = v.clone();
                        return Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver,
                                method: func,
                            }
                        }));
                    }
                    return Some(v.clone());
                }
                // Check for built-in instance methods (namedtuple, deque, hashlib)
                if let Some(result) = instance_builtin_method(obj, inst, name) {
                    return Some(result);
                }
                // Builtin type subclass: delegate to the underlying value's get_attr
                if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                    if let Some(result) = py_get_attr(&val, name) {
                        return Some(result);
                    }
                }
                None
            }
            PyObjectPayload::Class(cd) => {
                // Special class attributes
                if name == "__class__" {
                    // In CPython, a class's __class__ is its metaclass (usually 'type')
                    if let Some(meta) = &cd.metaclass {
                        return Some(meta.clone());
                    }
                    return Some(PyObject::builtin_type(CompactString::from("type")));
                }
                if name == "__name__" {
                    return Some(PyObject::str_val(cd.name.clone()));
                }
                if name == "__bases__" {
                    return Some(PyObject::tuple(cd.bases.clone()));
                }
                if name == "__mro__" {
                    let mut mro_list = vec![obj.clone()];
                    mro_list.extend(cd.mro.iter().cloned());
                    // Append 'object' as the universal base (like CPython)
                    mro_list.push(PyObject::builtin_type(CompactString::from("object")));
                    return Some(PyObject::tuple(mro_list));
                }
                if name == "__dict__" {
                    let ns = cd.namespace.read();
                    let mut map = IndexMap::new();
                    for (k, v) in ns.iter() {
                        if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                            map.insert(hk, v.clone());
                        }
                    }
                    return Some(PyObject::dict(map));
                }
                if name == "__module__" {
                    // Check namespace first for explicitly set __module__
                    if let Some(v) = cd.namespace.read().get("__module__") {
                        return Some(v.clone());
                    }
                    return Some(PyObject::str_val(CompactString::from("__main__")));
                }
                if name == "__qualname__" {
                    // Check namespace first
                    if let Some(v) = cd.namespace.read().get("__qualname__") {
                        return Some(v.clone());
                    }
                    return Some(PyObject::str_val(cd.name.clone()));
                }
                // Check own namespace first, then bases
                if let Some(v) = cd.namespace.read().get(name).cloned() {
                    match &v.payload {
                        PyObjectPayload::StaticMethod(func) => return Some(func.clone()),
                        PyObjectPayload::ClassMethod(func) => {
                            return Some(Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj.clone(),
                                    method: func.clone(),
                                }
                            }));
                        }
                        _ => return Some(v),
                    }
                }
                for base in &cd.bases {
                    if let Some(v) = base.get_attr(name) {
                        // Rebind classmethods to the current class (Sub), not the base (Base)
                        if let PyObjectPayload::BoundMethod { method, receiver } = &v.payload {
                            if matches!(&receiver.payload, PyObjectPayload::Class(_)) {
                                // Check if original in base namespace was a ClassMethod
                                let is_cm = if let PyObjectPayload::Class(bcd) = &base.payload {
                                    bcd.namespace.read().get(name)
                                        .map(|v| matches!(&v.payload, PyObjectPayload::ClassMethod(_)))
                                        .unwrap_or(false)
                                } else { false };
                                if is_cm {
                                    return Some(Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: obj.clone(),
                                            method: method.clone(),
                                        }
                                    }));
                                }
                            }
                        }
                        return Some(v);
                    }
                }
                // If class has a metaclass, look in metaclass namespace too
                // (e.g., cls._instances where _instances is a metaclass class attribute)
                if let Some(meta) = &cd.metaclass {
                    if let PyObjectPayload::Class(mcd) = &meta.payload {
                        if let Some(v) = mcd.namespace.read().get(name).cloned() {
                            return Some(v);
                        }
                    }
                }
                // Fallback: synthesize object-level attributes for user classes
                if name == "__new__" {
                    return Some(PyObject::native_function("__new__", |args| {
                        if args.is_empty() { return Err(PyException::type_error("__new__ requires cls")); }
                        Ok(PyObject::instance(args[0].clone()))
                    }));
                }
                if name == "__init_subclass__" {
                    return Some(PyObject::native_function("__init_subclass__", |_args| {
                        Ok(PyObject::none())
                    }));
                }
                None
            }
            PyObjectPayload::Module(m) => m.attrs.read().get(name).cloned(),
            PyObjectPayload::Slice { start, stop, step } => {
                match name {
                    "start" => Some(start.clone().unwrap_or_else(PyObject::none)),
                    "stop" => Some(stop.clone().unwrap_or_else(PyObject::none)),
                    "step" => Some(step.clone().unwrap_or_else(PyObject::none)),
                    _ => None,
                }
            }
            PyObjectPayload::Complex { real, imag } => {
                match name {
                    "real" => Some(PyObject::float(*real)),
                    "imag" => Some(PyObject::float(*imag)),
                    "__class__" => Some(PyObject::builtin_type(CompactString::from("complex"))),
                    "conjugate" => Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: obj.clone(),
                            method_name: CompactString::from("conjugate"),
                        }
                    })),
                    _ => None,
                }
            }
            PyObjectPayload::BuiltinType(n) => {
                match name {
                    "__name__" | "__qualname__" => Some(PyObject::str_val(n.clone())),
                    "__bases__" => {
                        let parent = match n.as_str() {
                            "object" => vec![],
                            "bool" => vec![PyObject::builtin_type(CompactString::from("int"))],
                            "bytearray" => vec![PyObject::builtin_type(CompactString::from("object"))],
                            _ => vec![PyObject::builtin_type(CompactString::from("object"))],
                        };
                        Some(PyObject::tuple(parent))
                    }
                    "__mro__" => {
                        let mro = match n.as_str() {
                            "object" => vec![obj.clone()],
                            "bool" => vec![
                                obj.clone(),
                                PyObject::builtin_type(CompactString::from("int")),
                                PyObject::builtin_type(CompactString::from("object")),
                            ],
                            _ => vec![
                                obj.clone(),
                                PyObject::builtin_type(CompactString::from("object")),
                            ],
                        };
                        Some(PyObject::tuple(mro))
                    }
                    "fromkeys" if n.as_str() == "dict" => {
                        Some(PyObject::native_function("dict.fromkeys", |args| {
                            if args.is_empty() { return Err(PyException::type_error("fromkeys() requires at least 1 argument")); }
                            let keys = args[0].to_list()?;
                            let value = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
                            let mut map = IndexMap::new();
                            for k in keys {
                                let hk = k.to_hashable_key()?;
                                map.insert(hk, value.clone());
                            }
                            Ok(PyObject::dict(map))
                        }))
                    }
                    "maketrans" if n.as_str() == "str" => {
                        Some(PyObject::native_function("str.maketrans", |args| {
                            if args.is_empty() { return Err(PyException::type_error("maketrans() requires at least 1 argument")); }
                            let mut result_map = IndexMap::new();
                            if args.len() == 1 {
                                if let PyObjectPayload::Dict(map) = &args[0].payload {
                                    for (k, v) in map.read().iter() {
                                        let key = match k {
                                            HashableKey::Int(n) => n.clone(),
                                            HashableKey::Str(s) => {
                                                if let Some(c) = s.chars().next() { PyInt::Small(c as i64) } else { continue; }
                                            }
                                            _ => continue,
                                        };
                                        result_map.insert(HashableKey::Int(key), v.clone());
                                    }
                                }
                            } else {
                                let x = args[0].py_to_string();
                                let y = args[1].py_to_string();
                                for (cx, cy) in x.chars().zip(y.chars()) {
                                    result_map.insert(HashableKey::Int(PyInt::Small(cx as i64)), PyObject::int(cy as i64));
                                }
                                if args.len() > 2 {
                                    let z = args[2].py_to_string();
                                    for cz in z.chars() {
                                        result_map.insert(HashableKey::Int(PyInt::Small(cz as i64)), PyObject::none());
                                    }
                                }
                            }
                            Ok(PyObject::dict(result_map))
                        }))
                    }
                    "fromhex" if n.as_str() == "bytes" => {
                        Some(PyObject::native_function("bytes.fromhex", |args| {
                            if args.is_empty() { return Err(PyException::type_error("fromhex() requires a string")); }
                            let s = args[0].py_to_string();
                            let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
                            if clean.len() % 2 != 0 {
                                return Err(PyException::value_error("non-hexadecimal number found in fromhex() arg"));
                            }
                            let mut bytes = Vec::with_capacity(clean.len() / 2);
                            for i in (0..clean.len()).step_by(2) {
                                let byte = u8::from_str_radix(&clean[i..i+2], 16)
                                    .map_err(|_| PyException::value_error("non-hexadecimal number found in fromhex() arg"))?;
                                bytes.push(byte);
                            }
                            Ok(PyObject::bytes(bytes))
                        }))
                    }
                    // object.__setattr__(instance, name, value) — bypass custom __setattr__
                    "__setattr__" if n.as_str() == "object" => {
                        Some(PyObject::native_function("object.__setattr__", |args| {
                            if args.len() != 3 {
                                return Err(PyException::type_error(
                                    "object.__setattr__() takes exactly 3 arguments"));
                            }
                            let name = args[1].as_str().ok_or_else(||
                                PyException::type_error("attribute name must be string"))?;
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                inst.attrs.write().insert(CompactString::from(name), args[2].clone());
                            }
                            Ok(PyObject::none())
                        }))
                    }
                    // object.__getattribute__(instance, name) — bypass custom __getattribute__
                    "__getattribute__" if n.as_str() == "object" => {
                        Some(PyObject::native_function("object.__getattribute__", |args| {
                            if args.len() != 2 {
                                return Err(PyException::type_error(
                                    "object.__getattribute__() takes exactly 2 arguments"));
                            }
                            let name = args[1].as_str().ok_or_else(||
                                PyException::type_error("attribute name must be string"))?;
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                if let Some(val) = inst.attrs.read().get(name) {
                                    return Ok(val.clone());
                                }
                            }
                            args[0].get_attr(name).ok_or_else(||
                                PyException::attribute_error(&format!(
                                    "'{}' object has no attribute '{}'", args[0].type_name(), name)))
                        }))
                    }
                    // object.__delattr__(instance, name) — bypass custom __delattr__
                    "__delattr__" if n.as_str() == "object" => {
                        Some(PyObject::native_function("object.__delattr__", |args| {
                            if args.len() != 2 {
                                return Err(PyException::type_error(
                                    "object.__delattr__() takes exactly 2 arguments"));
                            }
                            let name = args[1].as_str().ok_or_else(||
                                PyException::type_error("attribute name must be string"))?;
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                inst.attrs.write().shift_remove(name);
                            }
                            Ok(PyObject::none())
                        }))
                    }
                    _ => {
                        if name.starts_with("__") && name.ends_with("__") {
                            // Whitelist of dunders that BuiltinTypes actually support.
                            // Unknown dunders return None to avoid false positives
                            // (e.g. __namedtuple__, __dataclass__, __slots__ markers).
                            static BUILTIN_DUNDERS: &[&str] = &[
                                "__init__", "__new__", "__str__", "__repr__", "__hash__",
                                "__eq__", "__ne__", "__lt__", "__le__", "__gt__", "__ge__",
                                "__bool__", "__len__", "__getitem__", "__setitem__",
                                "__delitem__", "__contains__", "__iter__", "__next__",
                                "__call__", "__add__", "__sub__", "__mul__", "__truediv__",
                                "__floordiv__", "__mod__", "__pow__", "__and__", "__or__",
                                "__xor__", "__neg__", "__pos__", "__abs__", "__invert__",
                                "__radd__", "__rsub__", "__rmul__", "__rtruediv__",
                                "__rfloordiv__", "__rmod__", "__rpow__", "__rand__",
                                "__ror__", "__rxor__", "__iadd__", "__isub__", "__imul__",
                                "__itruediv__", "__ifloordiv__", "__imod__", "__ipow__",
                                "__iand__", "__ior__", "__ixor__", "__lshift__", "__rshift__",
                                "__rlshift__", "__rrshift__", "__ilshift__", "__irshift__",
                                "__enter__", "__exit__", "__format__", "__index__",
                                "__int__", "__float__", "__complex__", "__round__",
                                "__reversed__", "__missing__", "__del__", "__copy__",
                                "__deepcopy__", "__reduce__", "__sizeof__", "__class__",
                                "__subclasses__", "__subclasshook__",
                            ];
                            if BUILTIN_DUNDERS.contains(&name) {
                                // Check if resolve_builtin_type_method has a real implementation
                                if let Some(native) = resolve_builtin_type_method(n, name) {
                                    return Some(native);
                                }
                                Some(Arc::new(PyObject {
                                    payload: PyObjectPayload::BuiltinBoundMethod {
                                        receiver: obj.clone(),
                                        method_name: CompactString::from(name),
                                    }
                                }))
                            } else {
                                None
                            }
                        } else {
                            // Unbound method access: str.upper, list.append, etc.
                            Some(Arc::new(PyObject {
                                payload: PyObjectPayload::BuiltinBoundMethod {
                                    receiver: obj.clone(),
                                    method_name: CompactString::from(name),
                                }
                            }))
                        }
                    }
                }
            }
            PyObjectPayload::Property { fget, fset, fdel } => {
                match name {
                    "setter" | "getter" | "deleter" | "fget" | "fset" | "fdel" => {
                        match name {
                            "fget" => return fget.clone().or_else(|| Some(PyObject::none())),
                            "fset" => return fset.clone().or_else(|| Some(PyObject::none())),
                            "fdel" => return fdel.clone().or_else(|| Some(PyObject::none())),
                            _ => {}
                        }
                        // Return a BuiltinBoundMethod that the VM will handle
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: obj.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::Partial { func, args, kwargs } => {
                match name {
                    "func" => Some(func.clone()),
                    "args" => Some(PyObject::tuple(args.clone())),
                    "keywords" => {
                        let mut map = indexmap::IndexMap::new();
                        for (k, v) in kwargs {
                            map.insert(crate::types::HashableKey::Str(k.clone()), v.clone());
                        }
                        Some(PyObject::dict(map))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::ExceptionType(kind) => {
                match name {
                    "__name__" | "__qualname__" => Some(PyObject::str_val(CompactString::from(format!("{:?}", kind)))),
                    "__bases__" => {
                        // Return the parent exception type in the hierarchy
                        use crate::error::ExceptionKind;
                        let parent = match kind {
                            ExceptionKind::BaseException => None,
                            ExceptionKind::Exception | ExceptionKind::SystemExit |
                            ExceptionKind::KeyboardInterrupt | ExceptionKind::GeneratorExit => {
                                Some(ExceptionKind::BaseException)
                            }
                            ExceptionKind::ArithmeticError | ExceptionKind::LookupError |
                            ExceptionKind::OSError | ExceptionKind::ValueError |
                            ExceptionKind::Warning | ExceptionKind::ImportError |
                            ExceptionKind::RuntimeError | ExceptionKind::SyntaxError |
                            ExceptionKind::NameError | ExceptionKind::TypeError |
                            ExceptionKind::AttributeError | ExceptionKind::AssertionError |
                            ExceptionKind::BufferError | ExceptionKind::EOFError |
                            ExceptionKind::MemoryError | ExceptionKind::ReferenceError |
                            ExceptionKind::SystemError | ExceptionKind::StopIteration |
                            ExceptionKind::StopAsyncIteration => {
                                Some(ExceptionKind::Exception)
                            }
                            ExceptionKind::FloatingPointError | ExceptionKind::OverflowError |
                            ExceptionKind::ZeroDivisionError => Some(ExceptionKind::ArithmeticError),
                            ExceptionKind::IndexError | ExceptionKind::KeyError => Some(ExceptionKind::LookupError),
                            ExceptionKind::FileExistsError | ExceptionKind::FileNotFoundError |
                            ExceptionKind::PermissionError | ExceptionKind::TimeoutError |
                            ExceptionKind::IsADirectoryError | ExceptionKind::NotADirectoryError |
                            ExceptionKind::ProcessLookupError | ExceptionKind::ConnectionError |
                            ExceptionKind::InterruptedError | ExceptionKind::ChildProcessError |
                            ExceptionKind::BlockingIOError | ExceptionKind::BrokenPipeError => {
                                Some(ExceptionKind::OSError)
                            }
                            ExceptionKind::ConnectionResetError | ExceptionKind::ConnectionAbortedError |
                            ExceptionKind::ConnectionRefusedError => Some(ExceptionKind::ConnectionError),
                            ExceptionKind::UnicodeError | ExceptionKind::UnicodeDecodeError |
                            ExceptionKind::UnicodeEncodeError => Some(ExceptionKind::ValueError),
                            ExceptionKind::ModuleNotFoundError => Some(ExceptionKind::ImportError),
                            ExceptionKind::NotImplementedError | ExceptionKind::RecursionError => {
                                Some(ExceptionKind::RuntimeError)
                            }
                            ExceptionKind::UnboundLocalError => Some(ExceptionKind::NameError),
                            ExceptionKind::IndentationError => Some(ExceptionKind::SyntaxError),
                            ExceptionKind::TabError => Some(ExceptionKind::IndentationError),
                            ExceptionKind::DeprecationWarning | ExceptionKind::RuntimeWarning |
                            ExceptionKind::UserWarning | ExceptionKind::SyntaxWarning |
                            ExceptionKind::FutureWarning | ExceptionKind::ImportWarning |
                            ExceptionKind::UnicodeWarning | ExceptionKind::BytesWarning |
                            ExceptionKind::ResourceWarning | ExceptionKind::PendingDeprecationWarning => {
                                Some(ExceptionKind::Warning)
                            }
                        };
                        let bases = match parent {
                            Some(p) => vec![PyObject::exception_type(p)],
                            None => vec![PyObject::builtin_type(CompactString::from("object"))],
                        };
                        Some(PyObject::tuple(bases))
                    }
                    "__mro__" => {
                        // Build the MRO chain by walking up the hierarchy
                        use crate::error::ExceptionKind;
                        let mut mro = vec![obj.clone()];
                        let mut current = kind.clone();
                        loop {
                            let parent = match &current {
                                ExceptionKind::BaseException => break,
                                ExceptionKind::Exception | ExceptionKind::SystemExit |
                                ExceptionKind::KeyboardInterrupt | ExceptionKind::GeneratorExit => ExceptionKind::BaseException,
                                ExceptionKind::ArithmeticError | ExceptionKind::LookupError |
                                ExceptionKind::OSError | ExceptionKind::ValueError |
                                ExceptionKind::Warning | ExceptionKind::ImportError |
                                ExceptionKind::RuntimeError | ExceptionKind::SyntaxError |
                                ExceptionKind::NameError | ExceptionKind::TypeError |
                                ExceptionKind::AttributeError | ExceptionKind::AssertionError |
                                ExceptionKind::BufferError | ExceptionKind::EOFError |
                                ExceptionKind::MemoryError | ExceptionKind::ReferenceError |
                                ExceptionKind::SystemError | ExceptionKind::StopIteration |
                                ExceptionKind::StopAsyncIteration => ExceptionKind::Exception,
                                ExceptionKind::FloatingPointError | ExceptionKind::OverflowError |
                                ExceptionKind::ZeroDivisionError => ExceptionKind::ArithmeticError,
                                ExceptionKind::IndexError | ExceptionKind::KeyError => ExceptionKind::LookupError,
                                ExceptionKind::FileExistsError | ExceptionKind::FileNotFoundError |
                                ExceptionKind::PermissionError | ExceptionKind::TimeoutError |
                                ExceptionKind::IsADirectoryError | ExceptionKind::NotADirectoryError |
                                ExceptionKind::ProcessLookupError | ExceptionKind::ConnectionError |
                                ExceptionKind::InterruptedError | ExceptionKind::ChildProcessError |
                                ExceptionKind::BlockingIOError | ExceptionKind::BrokenPipeError => ExceptionKind::OSError,
                                ExceptionKind::ConnectionResetError | ExceptionKind::ConnectionAbortedError |
                                ExceptionKind::ConnectionRefusedError => ExceptionKind::ConnectionError,
                                ExceptionKind::UnicodeError | ExceptionKind::UnicodeDecodeError |
                                ExceptionKind::UnicodeEncodeError => ExceptionKind::ValueError,
                                ExceptionKind::ModuleNotFoundError => ExceptionKind::ImportError,
                                ExceptionKind::NotImplementedError | ExceptionKind::RecursionError => ExceptionKind::RuntimeError,
                                ExceptionKind::UnboundLocalError => ExceptionKind::NameError,
                                ExceptionKind::IndentationError => ExceptionKind::SyntaxError,
                                ExceptionKind::TabError => ExceptionKind::IndentationError,
                                ExceptionKind::DeprecationWarning | ExceptionKind::RuntimeWarning |
                                ExceptionKind::UserWarning | ExceptionKind::SyntaxWarning |
                                ExceptionKind::FutureWarning | ExceptionKind::ImportWarning |
                                ExceptionKind::UnicodeWarning | ExceptionKind::BytesWarning |
                                ExceptionKind::ResourceWarning | ExceptionKind::PendingDeprecationWarning => ExceptionKind::Warning,
                            };
                            mro.push(PyObject::exception_type(parent.clone()));
                            current = parent;
                        }
                        mro.push(PyObject::builtin_type(CompactString::from("object")));
                        Some(PyObject::tuple(mro))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::ExceptionInstance { kind, message, args, attrs } => {
                match name {
                    "args" => {
                        if args.is_empty() {
                            if message.is_empty() {
                                Some(PyObject::tuple(vec![]))
                            } else {
                                Some(PyObject::tuple(vec![PyObject::str_val(message.clone())]))
                            }
                        } else {
                            Some(PyObject::tuple(args.clone()))
                        }
                    }
                    "__class__" => Some(PyObject::exception_type(kind.clone())),
                    "code" if *kind == ExceptionKind::SystemExit => {
                        // SystemExit.code: first arg or message
                        if !args.is_empty() {
                            Some(args[0].clone())
                        } else if !message.is_empty() {
                            // Try to parse as int, otherwise return as string
                            if let Ok(n) = message.parse::<i64>() {
                                Some(PyObject::int(n))
                            } else {
                                Some(PyObject::str_val(message.clone()))
                            }
                        } else {
                            Some(PyObject::none())
                        }
                    }
                    "value" => {
                        // StopIteration.value — check attrs first, then args[0], then None
                        if let Some(v) = attrs.read().get("value").cloned() {
                            Some(v)
                        } else if !args.is_empty() {
                            Some(args[0].clone())
                        } else {
                            Some(PyObject::none())
                        }
                    }
                    "__cause__" => {
                        attrs.read().get("__cause__").cloned().or_else(|| Some(PyObject::none()))
                    }
                    "__context__" => {
                        attrs.read().get("__context__").cloned().or_else(|| Some(PyObject::none()))
                    }
                    "__suppress_context__" => {
                        attrs.read().get("__suppress_context__").cloned().or_else(|| Some(PyObject::bool_val(false)))
                    }
                    "__traceback__" => {
                        attrs.read().get("__traceback__").cloned().or_else(|| Some(PyObject::none()))
                    }
                    _ => {
                        // Check user-set attrs (e.g., __cause__)
                        attrs.read().get(name).cloned()
                    }
                }
            }
            // Function attributes
            PyObjectPayload::Function(f) => {
                // Check user-set attrs first (allows overriding __name__ etc.)
                if let Some(v) = f.attrs.read().get(name).cloned() {
                    return Some(v);
                }
                match name {
                    "__name__" => Some(PyObject::str_val(f.name.clone())),
                    "__qualname__" => Some(PyObject::str_val(f.qualname.clone())),
                    "__defaults__" => {
                        if f.defaults.is_empty() { Some(PyObject::none()) }
                        else { Some(PyObject::tuple(f.defaults.clone())) }
                    }
                    "__module__" => Some(PyObject::str_val(CompactString::from("__main__"))),
                    "__doc__" => {
                        // Check attrs first (set by functools.wraps etc.)
                        if let Some(doc) = f.attrs.read().get("__doc__").cloned() {
                            return Some(doc);
                        }
                        // Extract docstring from first constant if it's a string
                        if let Some(ferrython_bytecode::ConstantValue::Str(s)) = f.code.constants.first() {
                            Some(PyObject::str_val(s.clone()))
                        } else {
                            Some(PyObject::none())
                        }
                    }
                    "__dict__" => Some(PyObject::wrap(PyObjectPayload::InstanceDict(f.attrs.clone()))),
                    "__annotations__" => {
                        let mut map = IndexMap::new();
                        for (k, v) in &f.annotations {
                            if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                                map.insert(hk, v.clone());
                            }
                        }
                        Some(PyObject::dict(map))
                    }
                    "__closure__" => {
                        if f.closure.is_empty() {
                            Some(PyObject::none())
                        } else {
                            let cells: Vec<PyObjectRef> = f.closure.iter().map(|cell| {
                                cell.read().clone().unwrap_or_else(PyObject::none)
                            }).collect();
                            Some(PyObject::tuple(cells))
                        }
                    }
                    "__code__" => Some(PyObject::none()),
                    _ => None,
                }
            }
            PyObjectPayload::NativeFunction { name: fname, .. } => match name {
                "__name__" => Some(PyObject::str_val(CompactString::from(fname.as_str()))),
                "__qualname__" => Some(PyObject::str_val(CompactString::from(fname.as_str()))),
                "__module__" => Some(PyObject::str_val(CompactString::from("builtins"))),
                "__doc__" => Some(PyObject::none()),
                _ => None,
            }
            PyObjectPayload::BoundMethod { method, .. } => {
                method.get_attr(name)
            }
            // Int property-like attributes (return values, not bound methods)
            PyObjectPayload::Int(_n) => match name {
                "real" | "numerator" => Some(PyObject::wrap(obj.payload.clone())),
                "imag" => Some(PyObject::int(0)),
                "denominator" => Some(PyObject::int(1)),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("int"))),
                "bit_length" | "bit_count" | "to_bytes" | "conjugate" | "__abs__" |
                "__int__" | "__float__" | "__index__" | "__bool__" |
                "__repr__" | "__str__" | "__hash__" | "__format__" |
                "__ceil__" | "__floor__" | "__round__" | "__trunc__" |
                "as_integer_ratio" => Some(Arc::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod {
                        receiver: obj.clone(),
                        method_name: CompactString::from(name),
                    }
                })),
                _ => None,
            },
            // Float property-like attributes
            PyObjectPayload::Float(f) => match name {
                "real" => Some(PyObject::float(*f)),
                "imag" => Some(PyObject::float(0.0)),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("float"))),
                "is_integer" | "conjugate" | "hex" | "__abs__" |
                "__int__" | "__float__" | "__bool__" |
                "__repr__" | "__str__" | "__hash__" | "__format__" |
                "__ceil__" | "__floor__" | "__round__" | "__trunc__" |
                "as_integer_ratio" | "fromhex" => Some(Arc::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod {
                        receiver: obj.clone(),
                        method_name: CompactString::from(name),
                    }
                })),
                _ => None,
            },
            // Bool property-like attributes (bool is subtype of int)
            PyObjectPayload::Bool(b) => match name {
                "real" | "numerator" => Some(PyObject::int(if *b { 1 } else { 0 })),
                "imag" => Some(PyObject::int(0)),
                "denominator" => Some(PyObject::int(1)),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("bool"))),
                "bit_length" | "bit_count" | "to_bytes" | "conjugate" | "__abs__" |
                "__int__" | "__float__" | "__index__" | "__bool__" |
                "__repr__" | "__str__" | "__hash__" | "__format__" => Some(Arc::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod {
                        receiver: obj.clone(),
                        method_name: CompactString::from(name),
                    }
                })),
                _ => None,
            },
            // Built-in type methods — return bound method for KNOWN methods only
            PyObjectPayload::Str(_) => {
                if name == "__class__" {
                    return Some(PyObject::builtin_type(CompactString::from("str")));
                }
                if matches!(name,
                    "upper" | "lower" | "strip" | "lstrip" | "rstrip" | "split" | "rsplit"
                    | "join" | "replace" | "find" | "rfind" | "index" | "rindex" | "count"
                    | "startswith" | "endswith" | "isdigit" | "isalpha" | "isalnum" | "isspace"
                    | "isupper" | "islower" | "istitle" | "isprintable" | "isidentifier"
                    | "isascii" | "isdecimal" | "isnumeric" | "title" | "capitalize" | "swapcase"
                    | "center" | "ljust" | "rjust" | "zfill" | "expandtabs" | "encode"
                    | "partition" | "rpartition" | "casefold" | "removeprefix" | "removesuffix"
                    | "splitlines" | "format" | "format_map" | "translate" | "maketrans"
                    | "__len__" | "__contains__" | "__iter__" | "__getitem__" | "__hash__"
                    | "__eq__" | "__ne__" | "__lt__" | "__le__" | "__gt__" | "__ge__"
                    | "__repr__" | "__str__" | "__format__" | "__add__" | "__mul__" | "__rmul__"
                    | "__mod__" | "__bool__"
                ) {
                    return Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: obj.clone(),
                            method_name: CompactString::from(name),
                        }
                    }));
                }
                None
            }
            PyObjectPayload::List(_) => {
                if name == "__class__" {
                    return Some(PyObject::builtin_type(CompactString::from("list")));
                }
                if matches!(name,
                    "append" | "extend" | "insert" | "pop" | "remove" | "reverse" | "sort"
                    | "clear" | "copy" | "count" | "index"
                    | "__len__" | "__contains__" | "__iter__" | "__getitem__" | "__setitem__"
                    | "__delitem__" | "__eq__" | "__ne__" | "__lt__" | "__le__" | "__gt__" | "__ge__"
                    | "__repr__" | "__str__" | "__add__" | "__mul__" | "__iadd__" | "__imul__"
                    | "__reversed__" | "__bool__" | "__hash__"
                ) {
                    return Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: obj.clone(),
                            method_name: CompactString::from(name),
                        }
                    }));
                }
                None
            }
            PyObjectPayload::Dict(_) | PyObjectPayload::InstanceDict(_) => {
                if name == "__class__" {
                    let type_name = obj.type_name();
                    return Some(PyObject::builtin_type(CompactString::from(type_name)));
                }
                if matches!(name,
                    "keys" | "values" | "items" | "get" | "copy" | "update" | "subtract"
                    | "pop" | "setdefault" | "clear" | "popitem" | "most_common" | "elements"
                    | "move_to_end"
                    | "__len__" | "__contains__" | "__iter__" | "__getitem__" | "__setitem__"
                    | "__delitem__" | "__eq__" | "__ne__" | "__repr__" | "__str__"
                    | "__or__" | "__ior__" | "__bool__" | "__hash__"
                ) {
                    return Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: obj.clone(),
                            method_name: CompactString::from(name),
                        }
                    }));
                }
                None
            }
            PyObjectPayload::Tuple(_) => {
                if name == "__class__" {
                    return Some(PyObject::builtin_type(CompactString::from("tuple")));
                }
                if matches!(name,
                    "count" | "index"
                    | "__len__" | "__contains__" | "__iter__" | "__getitem__" | "__hash__"
                    | "__eq__" | "__ne__" | "__lt__" | "__le__" | "__gt__" | "__ge__"
                    | "__repr__" | "__str__" | "__add__" | "__mul__" | "__bool__"
                ) {
                    return Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: obj.clone(),
                            method_name: CompactString::from(name),
                        }
                    }));
                }
                None
            }
            PyObjectPayload::Set(_) => {
                if name == "__class__" {
                    return Some(PyObject::builtin_type(CompactString::from("set")));
                }
                if matches!(name,
                    "add" | "remove" | "discard" | "pop" | "clear" | "copy" | "update"
                    | "union" | "intersection" | "difference" | "symmetric_difference"
                    | "issubset" | "issuperset" | "isdisjoint"
                    | "intersection_update" | "difference_update" | "symmetric_difference_update"
                    | "__len__" | "__contains__" | "__iter__" | "__or__" | "__and__"
                    | "__sub__" | "__xor__" | "__eq__" | "__ne__" | "__lt__" | "__le__"
                    | "__gt__" | "__ge__" | "__repr__" | "__str__" | "__bool__" | "__hash__"
                ) {
                    return Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: obj.clone(),
                            method_name: CompactString::from(name),
                        }
                    }));
                }
                None
            }
            PyObjectPayload::FrozenSet(_) => {
                if name == "__class__" {
                    return Some(PyObject::builtin_type(CompactString::from("frozenset")));
                }
                if matches!(name,
                    "copy" | "union" | "intersection" | "difference" | "symmetric_difference"
                    | "issubset" | "issuperset" | "isdisjoint"
                    | "__len__" | "__contains__" | "__iter__" | "__or__" | "__and__"
                    | "__sub__" | "__xor__" | "__eq__" | "__ne__" | "__hash__"
                    | "__repr__" | "__str__" | "__bool__"
                ) {
                    return Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: obj.clone(),
                            method_name: CompactString::from(name),
                        }
                    }));
                }
                None
            }
            PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => {
                if name == "__class__" {
                    let type_name = obj.type_name();
                    return Some(PyObject::builtin_type(CompactString::from(type_name)));
                }
                if matches!(name,
                    "decode" | "hex" | "count" | "find" | "rfind" | "index" | "rindex"
                    | "startswith" | "endswith" | "upper" | "lower" | "strip" | "lstrip" | "rstrip"
                    | "split" | "join" | "replace" | "isdigit" | "isalpha" | "isalnum" | "isspace"
                    | "islower" | "isupper" | "istitle" | "swapcase" | "title" | "capitalize"
                    | "center" | "ljust" | "rjust" | "zfill" | "expandtabs"
                    | "partition" | "rpartition"
                    | "append" | "extend" | "pop" | "insert" | "clear" | "reverse" | "copy"
                    | "__len__" | "__contains__" | "__iter__" | "__getitem__" | "__setitem__"
                    | "__eq__" | "__ne__" | "__repr__" | "__str__" | "__add__" | "__mul__"
                    | "__bool__" | "__hash__"
                ) {
                    return Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: obj.clone(),
                            method_name: CompactString::from(name),
                        }
                    }));
                }
                None
            }
            PyObjectPayload::Generator(_) => {
                match name {
                    // Generator protocol: send, throw, close, __next__, __iter__
                    "send" | "throw" | "close" | "__next__" => {
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: obj.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    "__iter__" => Some(obj.clone()),
                    "gi_frame" | "gi_code" => Some(PyObject::none()),
                    "gi_running" => Some(PyObject::bool_val(false)),
                    "gi_yieldfrom" => Some(PyObject::none()),
                    _ => None,
                }
            }
            PyObjectPayload::Coroutine(_) => {
                match name {
                    // Coroutine protocol: send, throw, close
                    "send" | "throw" | "close" => {
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: obj.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    // __await__ on a coroutine returns self (the coroutine IS its own iterator for await)
                    "__await__" => Some(obj.clone()),
                    "cr_frame" | "cr_code" => Some(PyObject::none()),
                    "cr_running" => Some(PyObject::bool_val(false)),
                    "cr_await" | "cr_origin" => Some(PyObject::none()),
                    _ => None,
                }
            }
            PyObjectPayload::AsyncGenerator(_) => {
                match name {
                    // Sync methods that BuiltinBoundMethod dispatches in vm_call
                    "send" | "throw" | "close" => {
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: obj.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    // Async iteration protocol — __aiter__ returns self when called
                    "__aiter__" | "__anext__" | "asend" | "athrow" | "aclose" => {
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: obj.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    "ag_frame" | "ag_code" => Some(PyObject::none()),
                    "ag_running" => Some(PyObject::bool_val(false)),
                    "ag_await" => Some(PyObject::none()),
                    _ => None,
                }
            }
            // AsyncGenAwaitable is an awaitable: __await__ returns self, send/throw/close delegate
            PyObjectPayload::AsyncGenAwaitable { .. } => {
                match name {
                    "__await__" => Some(obj.clone()),
                    "send" | "throw" | "close" => {
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: obj.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::Super { cls, instance } => {
                // super() proxy: look up in the RUNTIME class MRO, skipping up to and including cls
                let runtime_cls = match &instance.payload {
                    PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
                    PyObjectPayload::Class(cd) => {
                        // For metaclass methods: walk metaclass MRO, not class MRO
                        if let Some(meta) = &cd.metaclass {
                            Some(meta.clone())
                        } else {
                            Some(instance.clone())
                        }
                    }
                    _ => None,
                };
                if let Some(rt_cls) = runtime_cls {
                    if let PyObjectPayload::Class(cd) = &rt_cls.payload {
                        let mro = &cd.mro;
                        // If cls IS the runtime class itself, start from index 0.
                        // Otherwise, skip entries up to and including cls in the MRO.
                        let cls_is_self = std::sync::Arc::ptr_eq(cls, &rt_cls);
                        let mut found_cls = cls_is_self;
                        for base in mro {
                            if !found_cls {
                                if std::sync::Arc::ptr_eq(base, cls) {
                                    found_cls = true;
                                }
                                continue;
                            }
                            // Look in this base's namespace directly
                            if let PyObjectPayload::Class(bcd) = &base.payload {
                                if let Some(v) = bcd.namespace.read().get(name) {
                                    if matches!(&v.payload, PyObjectPayload::Function(_)) {
                                        return Some(Arc::new(PyObject {
                                            payload: PyObjectPayload::BoundMethod {
                                                receiver: instance.clone(),
                                                method: v.clone(),
                                            }
                                        }));
                                    }
                                    return Some(v.clone());
                                }
                            }
                            // ExceptionType base: provide synthetic __init__/__str__
                            if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                                if let Some(resolved) = resolve_exception_type_method(name, instance) {
                                    // Bind to instance so obj is prepended
                                    return Some(Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: instance.clone(),
                                            method: resolved,
                                        }
                                    }));
                                }
                            }
                            // BuiltinType base in MRO
                            if let PyObjectPayload::BuiltinType(bt_name) = &base.payload {
                                // type.__call__ needs VM access; return a BoundMethod marker
                                if bt_name.as_str() == "type" && name == "__call__" {
                                    return Some(Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: instance.clone(),
                                            method: PyObject::native_function("__type_call__", |_| Ok(PyObject::none())),
                                        }
                                    }));
                                }
                                if let Some(resolved) = resolve_builtin_type_method(bt_name.as_str(), name) {
                                    // __new__ is a static method: don't bind obj
                                    if name == "__new__" {
                                        return Some(resolved);
                                    }
                                    // Wrap as BoundMethod so obj is prepended
                                    return Some(Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: instance.clone(),
                                            method: resolved,
                                        }
                                    }));
                                }
                            }
                        }
                        // Fallback: if cls not found in MRO, look in cls's own bases
                        if !found_cls {
                            if let PyObjectPayload::Class(ccd) = &cls.payload {
                                for base in &ccd.bases {
                                    if let PyObjectPayload::Class(bcd) = &base.payload {
                                        if let Some(v) = bcd.namespace.read().get(name) {
                                            if matches!(&v.payload, PyObjectPayload::Function(_)) {
                                                return Some(Arc::new(PyObject {
                                                    payload: PyObjectPayload::BoundMethod {
                                                        receiver: instance.clone(),
                                                        method: v.clone(),
                                                    }
                                                }));
                                            }
                                            return Some(v.clone());
                                        }
                                    }
                                    // Check BuiltinType bases (e.g., type, object)
                                    if let PyObjectPayload::BuiltinType(bt_name) = &base.payload {
                                        if let Some(resolved) = resolve_builtin_type_method(bt_name.as_str(), name) {
                                            return Some(Arc::new(PyObject {
                                                payload: PyObjectPayload::BoundMethod {
                                                    receiver: instance.clone(),
                                                    method: resolved,
                                                }
                                            }));
                                        }
                                    }
                                    // Check ExceptionType bases
                                    if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                                        if let Some(resolved) = resolve_exception_type_method(name, instance) {
                                            return Some(Arc::new(PyObject {
                                                payload: PyObjectPayload::BoundMethod {
                                                    receiver: instance.clone(),
                                                    method: resolved,
                                                }
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                        // Builtin __new__: object.__new__(cls) creates a new instance
                        if name == "__new__" {
                            return Some(PyObject::native_function("__new__", |args| {
                                if args.is_empty() { return Err(PyException::type_error("__new__ requires cls")); }
                                Ok(PyObject::instance(args[0].clone()))
                            }));
                        }
                        // Builtin __init__: object.__init__() is a no-op
                        if name == "__init__" {
                            return Some(PyObject::native_function("__init__", |_args| {
                                Ok(PyObject::none())
                            }));
                        }
                        // Builtin __init_subclass__: object.__init_subclass__() is a no-op
                        if name == "__init_subclass__" {
                            return Some(PyObject::native_function("__init_subclass__", |_args| {
                                Ok(PyObject::none())
                            }));
                        }
                        // Builtin __setattr__: object.__setattr__(self, name, value)
                        if name == "__setattr__" {
                            let inst = instance.clone();
                            return Some(PyObject::native_closure("__setattr__", move |args: &[PyObjectRef]| {
                                if args.len() < 2 {
                                    return Err(PyException::type_error("__setattr__ requires name and value"));
                                }
                                let attr_name = args[0].py_to_string();
                                let value = args[1].clone();
                                if let PyObjectPayload::Instance(data) = &inst.payload {
                                    data.attrs.write().insert(CompactString::from(attr_name.as_str()), value);
                                }
                                Ok(PyObject::none())
                            }));
                        }
                    }
                }
                None
            }
            _ => None,
        }
}

/// Resolve methods on builtin ExceptionType bases (e.g. Exception.__init__).
/// Used by super() proxy when the parent class is a builtin exception type.
fn resolve_exception_type_method(name: &str, _instance: &PyObjectRef) -> Option<PyObjectRef> {
    match name {
        "__init__" => {
            Some(PyObject::native_function("__init__", |args| {
                // Exception.__init__(self, *args) — only set self.args (CPython behavior)
                if args.is_empty() { return Ok(PyObject::none()); }
                let target = &args[0];
                if let PyObjectPayload::Instance(idata) = &target.payload {
                    let exc_args: Vec<PyObjectRef> = if args.len() > 1 {
                        args[1..].to_vec()
                    } else {
                        vec![]
                    };
                    idata.attrs.write().insert(CompactString::from("args"), PyObject::tuple(exc_args));
                }
                Ok(PyObject::none())
            }))
        }
        "__str__" => {
            Some(PyObject::native_function("__str__", |args| {
                if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
                let target = &args[0];
                if let Some(a) = target.get_attr("args") {
                    if let PyObjectPayload::Tuple(items) = &a.payload {
                        if items.len() == 1 {
                            return Ok(PyObject::str_val(CompactString::from(items[0].py_to_string())));
                        } else if items.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        return Ok(PyObject::str_val(CompactString::from(a.repr())));
                    }
                }
                Ok(PyObject::str_val(CompactString::from(String::new())))
            }))
        }
        "__repr__" => {
            Some(PyObject::native_function("__repr__", |args| {
                if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("Exception()"))); }
                let target = &args[0];
                let cls_name = if let PyObjectPayload::Instance(idata) = &target.payload {
                    if let PyObjectPayload::Class(cd) = &idata.class.payload {
                        cd.name.to_string()
                    } else { "Exception".to_string() }
                } else { "Exception".to_string() };
                if let Some(a) = target.get_attr("args") {
                    Ok(PyObject::str_val(CompactString::from(format!("{}({})", cls_name, a.py_to_string()))))
                } else {
                    Ok(PyObject::str_val(CompactString::from(format!("{}()", cls_name))))
                }
            }))
        }
        _ => None,
    }
}

