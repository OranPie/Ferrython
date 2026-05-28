//! Attribute lookup helper functions shared by methods_attr.

use crate::error::PyException;
use crate::intern;
use compact_str::CompactString;

use super::helpers::*;
use super::methods::PyObjectMethods;
use super::payload::*;
use super::ClassData;

pub(super) fn bytes_fromhex_data(obj: &PyObjectRef) -> Result<Vec<u8>, PyException> {
    let s = match &obj.payload {
        PyObjectPayload::Str(s) => s,
        _ => {
            return Err(PyException::type_error(format!(
                "fromhex() argument must be str, not {}",
                obj.type_name()
            )))
        }
    };
    let bytes = s.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() / 2);
    let mut hi: Option<(usize, u8)> = None;
    for (pos, &byte) in bytes.iter().enumerate() {
        if matches!(byte, b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r' | b' ') {
            if hi.is_some() {
                return Err(PyException::value_error(format!(
                    "non-hexadecimal number found in fromhex() arg at position {}",
                    pos
                )));
            }
            continue;
        }
        let Some(value) = (byte as char).to_digit(16).map(|v| v as u8) else {
            return Err(PyException::value_error(format!(
                "non-hexadecimal number found in fromhex() arg at position {}",
                pos
            )));
        };
        if let Some((_, high)) = hi.take() {
            result.push((high << 4) | value);
        } else {
            hi = Some((pos, value));
        }
    }
    if let Some((pos, _)) = hi {
        return Err(PyException::value_error(format!(
            "non-hexadecimal number found in fromhex() arg at position {}",
            pos + 1
        )));
    }
    Ok(result)
}

pub(super) fn code_object_co_code(code: &ferrython_bytecode::CodeObject) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(code.instructions.len() * 2);
    for instruction in &code.instructions {
        let mut ext_args = Vec::new();
        let mut remaining = instruction.arg >> 8;
        while remaining > 0 {
            ext_args.push((remaining & 0xff) as u8);
            remaining >>= 8;
        }
        for arg in ext_args.iter().rev() {
            bytes.push(ferrython_bytecode::Opcode::ExtendedArg as u8);
            bytes.push(*arg);
        }
        bytes.push(instruction.op as u8);
        bytes.push((instruction.arg & 0xff) as u8);
    }
    bytes
}

pub(super) fn iterator_supports_setstate(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Iterator(iter_data) => {
            let data = iter_data.read();
            matches!(
                &*data,
                IteratorData::List { .. }
                    | IteratorData::Tuple { .. }
                    | IteratorData::Str { .. }
                    | IteratorData::SeqIter { .. }
            )
        }
        PyObjectPayload::RefIter { source, .. } => {
            matches!(
                &source.payload,
                PyObjectPayload::List(_) | PyObjectPayload::Tuple(_)
            )
        }
        PyObjectPayload::RevRefIter { .. } => true,
        _ => false,
    }
}

/// Walk a class and its base classes (MRO) to find an attribute.
/// Uses pre-computed vtable for O(1) lookup, falling back to cache+MRO.
pub fn lookup_in_class_mro(class: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    if let PyObjectPayload::Class(cd) = &class.payload {
        // Fastest path: check pre-computed method vtable (single hash probe)
        {
            let vt = cd.method_vtable.read();
            if !vt.is_empty() {
                if let Some(v) = vt.get(name) {
                    return Some(v.clone());
                }
                // Vtable miss: fall through to namespace/MRO lookup.
                // The vtable can be stale (e.g., decorator added methods after class creation).
            }
        }

        // Fallback for classes with empty vtable (e.g., builtin types)
        // Fast path: check method cache first
        {
            let cache = cd.method_cache.read();
            if let Some(cached) = cache.get(name) {
                return cached.clone();
            }
        }

        // Slow path: linear MRO scan
        let result = lookup_in_class_mro_uncached(cd, name);

        // Populate cache (cache both hits and misses)
        let key = intern::try_intern(name).unwrap_or_else(|| CompactString::from(name));
        cd.method_cache.write().insert(key, result.clone());

        return result;
    }
    None
}

/// Check if a class (or its MRO) has an attribute, without cloning the value.
pub(super) fn has_in_class_mro(class: &PyObjectRef, name: &str) -> bool {
    if let PyObjectPayload::Class(cd) = &class.payload {
        // Check vtable first
        {
            let vt = cd.method_vtable.read();
            if !vt.is_empty() {
                if vt.contains_key(name) {
                    return true;
                }
                // Vtable miss: fall through (vtable may be stale)
            }
        }
        // Check cache
        {
            let cache = cd.method_cache.read();
            if let Some(cached) = cache.get(name) {
                return cached.is_some();
            }
        }
        // Slow path
        return lookup_in_class_mro_uncached(cd, name).is_some();
    }
    false
}

/// Uncached MRO lookup — scans own namespace then bases.
fn lookup_in_class_mro_uncached(cd: &ClassData, name: &str) -> Option<PyObjectRef> {
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
            if let PyObjectPayload::Class(bcd) = &base.payload {
                if let Some(v) = lookup_in_class_mro_uncached(bcd, name) {
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
        PyObjectPayload::Property(_) => true,
        PyObjectPayload::Instance(inst) => {
            if is_property_subclass_class(&inst.class) {
                return true;
            }
            has_method_in_class(&inst.class, "__set__")
                || has_method_in_class(&inst.class, "__delete__")
        }
        _ => false,
    }
}

/// Check if an object has __get__ (i.e. is any kind of descriptor).
pub fn has_descriptor_get(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Property(_) => true,
        PyObjectPayload::Instance(inst) => {
            is_property_subclass_class(&inst.class) || has_method_in_class(&inst.class, "__get__")
        }
        _ => false,
    }
}

/// Check if a class (or its MRO) has a method by name.
fn has_method_in_class(class: &PyObjectRef, name: &str) -> bool {
    lookup_in_class_mro(class, name).is_some()
}

fn native_function_binds_to_class(class: &PyObjectRef, attr_name: &str, native_name: &str) -> bool {
    fn matches_class_name(class_name: &CompactString, attr_name: &str, native_name: &str) -> bool {
        let expected_len = class_name.len() + attr_name.len() + 1;
        native_name.len() == expected_len
            && native_name.starts_with(class_name.as_str())
            && native_name.as_bytes().get(class_name.len()) == Some(&b'.')
            && &native_name[class_name.len() + 1..] == attr_name
    }

    if let PyObjectPayload::Class(cd) = &class.payload {
        if matches_class_name(&cd.name, attr_name, native_name) {
            return true;
        }
        for base in cd.mro.iter().chain(cd.bases.iter()) {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                if matches_class_name(&bcd.name, attr_name, native_name) {
                    return true;
                }
            }
        }
    }
    false
}

pub(super) fn weakdict_class_attr(native_name: &str, attr_name: &str) -> Option<PyObjectRef> {
    if !matches!(native_name, "WeakValueDictionary" | "WeakKeyDictionary") {
        return None;
    }
    match attr_name {
        "__init__" | "update" => {
            let owner_name = native_name.to_string();
            let method_name = attr_name.to_string();
            let qualname = format!("{}.{}", owner_name, method_name);
            let message = format!(
                "{}() missing 1 required positional argument: 'self'",
                qualname
            );
            Some(PyObject::native_closure(&qualname, move |args| {
                if args.is_empty() {
                    Err(PyException::type_error(message.clone()))
                } else if args[0]
                    .get_attr(match owner_name.as_str() {
                        "WeakValueDictionary" => "__weakvaluedict__",
                        "WeakKeyDictionary" => "__weakkeydict__",
                        _ => "",
                    })
                    .is_none()
                {
                    Err(PyException::type_error(format!(
                        "descriptor '{}' for '{}' objects does not apply to '{}'",
                        method_name,
                        owner_name,
                        args[0].type_name()
                    )))
                } else if method_name == "__init__" {
                    if let Some(clear) = args[0].get_attr("clear") {
                        call_callable(&clear, &[])?;
                    }
                    if args.len() > 1 {
                        if let Some(update) = args[0].get_attr("update") {
                            call_callable(&update, &args[1..])?;
                        }
                    }
                    Ok(PyObject::none())
                } else if let Some(method) = args[0].get_attr(&method_name) {
                    call_callable(&method, &args[1..])
                } else {
                    Err(PyException::type_error(format!(
                        "descriptor '{}' for '{}' objects does not apply to '{}'",
                        method_name,
                        owner_name,
                        args[0].type_name()
                    )))
                }
            }))
        }
        _ => None,
    }
}

#[inline]
pub(super) fn ast_constant_alias_attr(inst: &InstanceData, name: &str) -> Option<PyObjectRef> {
    if !matches!(name, "n" | "s" | "kind") {
        return None;
    }
    let is_constant = match &inst.class.payload {
        PyObjectPayload::Class(cd) => {
            cd.name.as_str() == "Constant"
                || cd.mro.iter().any(|base| {
                    matches!(&base.payload, PyObjectPayload::Class(bcd) if bcd.name.as_str() == "Constant")
                })
        }
        _ => false,
    };
    if !is_constant {
        return None;
    }
    if name == "kind" {
        return Some(
            inst.attrs
                .read()
                .get("kind")
                .cloned()
                .unwrap_or_else(PyObject::none),
        );
    }
    inst.attrs.read().get("value").cloned()
}

/// Wrap a class-level attribute for instance access: bind functions as BoundMethod,
/// unwrap StaticMethod/ClassMethod, handle cached_property/lru_cache wrappers.
/// Extracted to avoid duplicating this logic in fast-path and descriptor-protocol paths.
#[inline]
pub(super) fn wrap_class_attr_for_instance(
    obj: &PyObjectRef,
    inst: &InstanceData,
    attr_name: &str,
    v: PyObjectRef,
) -> PyObjectRef {
    match &v.payload {
        PyObjectPayload::StaticMethod(func) => func.clone(),
        PyObjectPayload::ClassMethod(func) => PyObjectRef::new(PyObject {
            payload: PyObjectPayload::BoundMethod {
                receiver: inst.class.clone(),
                method: func.clone(),
            },
        }),
        PyObjectPayload::Function(_) => PyObjectRef::new(PyObject {
            payload: PyObjectPayload::BoundMethod {
                receiver: obj.clone(),
                method: v,
            },
        }),
        PyObjectPayload::NativeFunction(nf)
            if (nf.name.is_empty() && inst.is_special)
                || native_function_binds_to_class(&inst.class, attr_name, &nf.name) =>
        {
            PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: obj.clone(),
                    method: v,
                },
            })
        }
        PyObjectPayload::Instance(ref cp_inst) => {
            let cp_attrs = cp_inst.attrs.read();
            if cp_attrs.contains_key("__cached_property_func__") {
                return v.clone();
            }
            if cp_attrs.contains_key("__wrapped__") {
                return PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: obj.clone(),
                        method: v.clone(),
                    },
                });
            }
            drop(cp_attrs);
            v
        }
        PyObjectPayload::NativeClosure(ref nc) => {
            if nc.name.contains('.') {
                PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: obj.clone(),
                        method: v,
                    },
                })
            } else {
                v
            }
        }
        _ => v,
    }
}

/// Returns a BuiltinBoundMethod if the method name matches, None otherwise.
/// Uses a single lock acquisition to check all special instance markers.
pub(super) fn instance_builtin_method(
    obj: &PyObjectRef,
    inst: &InstanceData,
    name: &str,
) -> Option<PyObjectRef> {
    let make_bound = |name: &str| -> PyObjectRef {
        PyObjectRef::new(PyObject {
            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(
                obj.clone(),
                CompactString::from(name),
            )),
        })
    };

    if inst.attrs.read().contains_key("__memoryview__")
        && matches!(name, "hex" | "tobytes" | "tolist" | "release")
    {
        return Some(make_bound(name));
    }

    // Dict subclass: expose dict methods bound to the instance
    if inst.dict_storage.is_some() {
        if matches!(
            name,
            "keys"
                | "values"
                | "items"
                | "get"
                | "pop"
                | "update"
                | "setdefault"
                | "clear"
                | "copy"
                | "popitem"
                | "fromkeys"
                | "move_to_end"
        ) {
            if inst.attrs.read().contains_key(name)
                || lookup_in_class_mro(&inst.class, name).is_some()
            {
                return None;
            }
            return Some(make_bound(name));
        }
    }

    // Namedtuple
    if inst.class.get_attr("__namedtuple__").is_some() {
        if name == "_fields" {
            return inst.class.get_attr("_fields");
        }
        if matches!(
            name,
            "_asdict"
                | "_replace"
                | "_make"
                | "__len__"
                | "__iter__"
                | "__repr__"
                | "__str__"
                | "__eq__"
                | "__hash__"
                | "__contains__"
                | "__getitem__"
        ) {
            return Some(make_bound(name));
        }
    }

    // Single lock acquisition for all marker checks
    let attrs = inst.attrs.read();

    // Deque
    if attrs.contains_key("__deque__") {
        if name == "maxlen" {
            return Some(
                attrs
                    .get("__maxlen__")
                    .cloned()
                    .unwrap_or_else(PyObject::none),
            );
        }
        if matches!(
            name,
            "append"
                | "appendleft"
                | "pop"
                | "popleft"
                | "extend"
                | "extendleft"
                | "rotate"
                | "clear"
                | "copy"
                | "__copy__"
                | "count"
                | "index"
                | "insert"
                | "remove"
                | "reverse"
                | "__init__"
                | "__repr__"
                | "__str__"
                | "__eq__"
                | "__ne__"
                | "__lt__"
                | "__le__"
                | "__gt__"
                | "__ge__"
                | "__add__"
                | "__mul__"
                | "__rmul__"
                | "__iadd__"
                | "__imul__"
                | "__iter__"
                | "__len__"
                | "__contains__"
                | "__getitem__"
                | "__setitem__"
                | "__delitem__"
        ) {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // StringIO
    if attrs.contains_key("__stringio__") {
        if matches!(
            name,
            "write"
                | "read"
                | "getvalue"
                | "seek"
                | "tell"
                | "close"
                | "closed"
                | "readline"
                | "readlines"
                | "writelines"
                | "truncate"
                | "readable"
                | "writable"
                | "seekable"
                | "__iter__"
                | "__next__"
                | "__enter__"
                | "__exit__"
        ) {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // BytesIO
    if attrs.contains_key("__bytesio__") {
        if matches!(
            name,
            "write"
                | "read"
                | "getvalue"
                | "seek"
                | "tell"
                | "close"
                | "readline"
                | "readlines"
                | "truncate"
                | "readable"
                | "writable"
                | "seekable"
        ) {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // pathlib.Path
    if attrs.contains_key("__pathlib_path__") {
        if let Some(v) = attrs.get(name).cloned() {
            return Some(v);
        }
        drop(attrs); // Release before returning bound method
        if matches!(
            name,
            "exists"
                | "is_file"
                | "is_dir"
                | "is_absolute"
                | "is_symlink"
                | "__str__"
                | "__fspath__"
                | "__repr__"
                | "resolve"
                | "absolute"
                | "as_posix"
                | "relative_to"
                | "with_suffix"
                | "with_name"
                | "read_text"
                | "read_bytes"
                | "write_text"
                | "write_bytes"
                | "mkdir"
                | "rmdir"
                | "unlink"
                | "iterdir"
                | "glob"
                | "stat"
                | "joinpath"
                | "__truediv__"
                | "touch"
                | "rglob"
                | "chmod"
                | "match"
                | "samefile"
                | "rename"
                | "replace"
                | "open"
        ) {
            return Some(make_bound(name));
        }
        return None;
    }

    // Hashlib hash objects (check class name, not attrs)
    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
        cd.name.as_str()
    } else {
        ""
    };
    if matches!(
        class_name,
        "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512"
    ) {
        if matches!(name, "hexdigest" | "digest" | "update" | "copy") {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // CSV writer
    if attrs.contains_key("__csv_writer__") {
        if matches!(name, "writerow" | "writerows") {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // CSV DictWriter
    if attrs.contains_key("__csv_dictwriter__") {
        if matches!(name, "writeheader" | "writerow" | "writerows") {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // datetime instances
    if attrs.contains_key("__datetime__") {
        if matches!(
            name,
            "strftime"
                | "isoformat"
                | "timestamp"
                | "replace"
                | "date"
                | "time"
                | "timetuple"
                | "weekday"
                | "isoweekday"
                | "toordinal"
                | "ctime"
                | "__str__"
                | "__repr__"
                | "astimezone"
                | "utcoffset"
                | "tzname"
                | "isocalendar"
        ) {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // timedelta instances
    if attrs.contains_key("__timedelta__") {
        if matches!(name, "total_seconds" | "__str__" | "__repr__") {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // timezone instances
    if attrs.contains_key("__timezone__") {
        return attrs.get(name).cloned();
    }

    // queue instances
    if attrs.contains_key("__queue__") {
        if matches!(
            name,
            "put"
                | "get"
                | "empty"
                | "full"
                | "qsize"
                | "get_nowait"
                | "put_nowait"
                | "task_done"
                | "join"
        ) {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    None
}
