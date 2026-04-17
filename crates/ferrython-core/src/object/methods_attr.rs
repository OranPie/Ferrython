//! Attribute lookup methods and descriptor protocol helpers.

use std::rc::Rc;
use crate::error::{PyException, ExceptionKind};
use crate::intern::{self, intern_or_new};
use crate::types::{DictKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;

use super::payload::*;
use super::helpers::*;
use super::methods::PyObjectMethods;

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
        let key = intern::try_intern(name)
            .unwrap_or_else(|| CompactString::from(name));
        cd.method_cache.write().insert(key, result.clone());

        return result;
    }
    None
}

/// Check if a class (or its MRO) has an attribute, without cloning the value.
fn has_in_class_mro(class: &PyObjectRef, name: &str) -> bool {
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
            has_method_in_class(&inst.class, "__get__")
        }
        _ => false,
    }
}

/// Check if a class (or its MRO) has a method by name.
fn has_method_in_class(class: &PyObjectRef, name: &str) -> bool {
    lookup_in_class_mro(class, name).is_some()
}

/// Wrap a class-level attribute for instance access: bind functions as BoundMethod,
/// unwrap StaticMethod/ClassMethod, handle cached_property/lru_cache wrappers.
/// Extracted to avoid duplicating this logic in fast-path and descriptor-protocol paths.
#[inline]
fn wrap_class_attr_for_instance(obj: &PyObjectRef, inst: &InstanceData, v: PyObjectRef) -> PyObjectRef {
    match &v.payload {
        PyObjectPayload::StaticMethod(func) => func.clone(),
        PyObjectPayload::ClassMethod(func) => PyObjectRef::new(PyObject {
            payload: PyObjectPayload::BoundMethod {
                receiver: inst.class.clone(),
                method: func.clone(),
            }
        }),
        PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction(_) => {
            PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: obj.clone(),
                    method: v,
                }
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
                    }
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
                    }
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
fn instance_builtin_method(obj: &PyObjectRef, inst: &InstanceData, name: &str) -> Option<PyObjectRef> {
    let make_bound = |name: &str| -> PyObjectRef {
        PyObjectRef::new(PyObject {
            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
        if matches!(name, "_asdict" | "_replace" | "_make" | "__len__" | "__iter__"
            | "__repr__" | "__str__" | "__eq__" | "__hash__" | "__contains__" | "__getitem__")
        {
            return Some(make_bound(name));
        }
    }

    // Single lock acquisition for all marker checks
    let attrs = inst.attrs.read();

    // Deque
    if attrs.contains_key("__deque__") {
        if name == "maxlen" {
            return Some(attrs.get("__maxlen__").cloned().unwrap_or_else(PyObject::none));
        }
        if matches!(name, "append" | "appendleft" | "pop" | "popleft" | "extend"
            | "extendleft" | "rotate" | "clear" | "copy" | "count" | "index"
            | "insert" | "remove" | "reverse"
            | "__iter__" | "__len__" | "__contains__" | "__getitem__")
        {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // StringIO
    if attrs.contains_key("__stringio__") {
        if matches!(name, "write" | "read" | "getvalue" | "seek" | "tell" | "close" | "closed"
            | "readline" | "readlines" | "writelines" | "truncate" | "readable" | "writable" | "seekable"
            | "__iter__" | "__next__" | "__enter__" | "__exit__")
        {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    // BytesIO
    if attrs.contains_key("__bytesio__") {
        if matches!(name, "write" | "read" | "getvalue" | "seek" | "tell" | "close"
            | "readline" | "readlines" | "truncate" | "readable" | "writable" | "seekable")
        {
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

    // Hashlib hash objects (check class name, not attrs)
    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.as_str() } else { "" };
    if matches!(class_name, "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512") {
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
        if matches!(name, "strftime" | "isoformat" | "timestamp" | "replace" | "date" | "time"
            | "timetuple" | "weekday" | "isoweekday" | "toordinal" | "ctime" | "__str__" | "__repr__"
            | "astimezone" | "utcoffset" | "tzname" | "isocalendar")
        {
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
        if matches!(name, "put" | "get" | "empty" | "full" | "qsize" | "get_nowait" | "put_nowait"
            | "task_done" | "join")
        {
            return Some(make_bound(name));
        }
        return attrs.get(name).cloned();
    }

    None
}

/// Check if an attribute exists without cloning its value.
/// Used by hasattr() to avoid unnecessary Rc clone+drop cycles.
pub fn py_has_attr(obj: &PyObjectRef, name: &str) -> bool {
    match &obj.payload {
        PyObjectPayload::Instance(inst) => {
            let is_dunder = name.as_bytes().first() == Some(&b'_')
                && name.as_bytes().get(1) == Some(&b'_');
            if !is_dunder
                && inst.class_flags & (CLASS_FLAG_HAS_DESCRIPTORS | CLASS_FLAG_HAS_GETATTRIBUTE) == 0
                && inst.dict_storage.is_none()
            {
                // 1. Instance dict
                if inst.attrs.read().contains_key(name) {
                    return true;
                }
                // 2. Class MRO (no clone needed)
                if has_in_class_mro(&inst.class, name) {
                    return true;
                }
                // 3. Builtin instance methods
                if inst.is_special {
                    if instance_builtin_method(obj, inst, name).is_some() {
                        return true;
                    }
                }
                // 4. __getattr__ fallback
                if inst.class_flags & CLASS_FLAG_HAS_GETATTR != 0 {
                    if let Some(getattr_fn) = lookup_in_class_mro(&inst.class, "__getattr__") {
                        let name_obj = PyObject::str_val(CompactString::from(name));
                        if call_callable(&getattr_fn, &[obj.clone(), name_obj]).is_ok() {
                            return true;
                        }
                    }
                }
                return false;
            }
            // Standard path: fall through to get_attr
            py_get_attr(obj, name).is_some()
        }
        // For non-Instance types, delegate to get_attr (less hot path)
        _ => py_get_attr(obj, name).is_some(),
    }
}

pub(super) fn py_get_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                // ── ULTRA-FAST PATH ──
                // For simple instances: no descriptors, no __getattribute__, no
                // dict_storage, and name is NOT a dunder. Skips the expensive
                // __class__-override attr-map lookup that the standard path does
                // on every single call.
                let is_dunder = name.as_bytes().first() == Some(&b'_')
                    && name.as_bytes().get(1) == Some(&b'_');
                if !is_dunder
                    && inst.class_flags & (CLASS_FLAG_HAS_DESCRIPTORS | CLASS_FLAG_HAS_GETATTRIBUTE) == 0
                    && inst.dict_storage.is_none()
                {
                    // 1. Instance dict first (data attributes)
                    if let Some(v) = inst.attrs.read().get(name) {
                        return Some(v.clone());
                    }
                    // 2. Class + MRO via vtable/cache (methods & class attrs)
                    if let Some(v) = lookup_in_class_mro(&inst.class, name) {
                        return Some(wrap_class_attr_for_instance(obj, inst, v));
                    }
                    // 3. Builtin instance methods (only for special instances)
                    if inst.is_special {
                        if let Some(result) = instance_builtin_method(obj, inst, name) {
                            return Some(result);
                        }
                    }
                    // 4. __getattr__ fallback (only if class defines it)
                    if inst.class_flags & CLASS_FLAG_HAS_GETATTR != 0 {
                        if let Some(getattr_fn) = lookup_in_class_mro(&inst.class, "__getattr__") {
                            let name_obj = PyObject::str_val(CompactString::from(name));
                            if let Ok(result) = call_callable(&getattr_fn, &[obj.clone(), name_obj]) {
                                return Some(result);
                            }
                        }
                    }
                    return None;
                }

                // ── STANDARD PATH ── (dunders, descriptors, __getattribute__, dict subclasses)
                // Special instance attributes
                if name == "__class__" {
                    if let Some(cls_override) = inst.attrs.read().get("__class__") {
                        return Some(cls_override.clone());
                    }
                    return Some(inst.class.clone());
                }
                if name == "__dict__" {
                    if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        if !cd.has_dict_slot() {
                            return None;
                        }
                    }
                    return Some(PyObject::wrap(PyObjectPayload::InstanceDict(inst.attrs.clone())));
                }
                // Dict subclass: intercept dict method lookups
                if inst.dict_storage.is_some() {
                    if let Some(result) = instance_builtin_method(obj, inst, name) {
                        return Some(result);
                    }
                }

                // Resolve effective class: honour `__class__` override (if any)
                let effective_class = inst.attrs.read().get("__class__")
                    .filter(|c| matches!(c.payload, PyObjectPayload::Class(_)))
                    .cloned()
                    .unwrap_or_else(|| inst.class.clone());

                // Determine if this class has data descriptors
                let class_has_descriptors = inst.class_flags & CLASS_FLAG_HAS_DESCRIPTORS != 0;

                if !class_has_descriptors {
                    // ── FAST PATH: no data descriptors ──
                    // Check instance dict first, then class MRO
                    if let Some(v) = inst.attrs.read().get(name) {
                        return Some(v.clone());
                    }
                    if let Some(v) = lookup_in_class_mro(&effective_class, name) {
                        return Some(wrap_class_attr_for_instance(obj, inst, v));
                    }
                } else {
                    // ── FULL DESCRIPTOR PROTOCOL ──
                    // 1. Data descriptors from class MRO take priority
                    let class_attr = lookup_in_class_mro(&effective_class, name);
                    if let Some(ref v) = class_attr {
                        match &v.payload {
                            PyObjectPayload::Property(_) => {
                                return Some(v.clone());
                            }
                            _ => {
                                if is_data_descriptor(v) {
                                    return Some(v.clone());
                                }
                            }
                        }
                    }
                    // 2. Instance attributes
                    if let Some(v) = inst.attrs.read().get(name) { return Some(v.clone()); }
                    // 3. Non-data descriptors and other class attrs
                    if let Some(v) = class_attr {
                        return Some(wrap_class_attr_for_instance(obj, inst, v));
                    }
                }

                // Fallback: built-in instance methods (namedtuple, deque, hashlib, etc.)
                if let Some(result) = instance_builtin_method(obj, inst, name) {
                    return Some(result);
                }
                // Builtin type subclass: delegate to underlying value
                if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                    if let Some(result) = py_get_attr(&val, name) {
                        return Some(result);
                    }
                }
                // Synthesized class-level attrs
                if name == "__new__" || name == "__init_subclass__" || name == "__subclasshook__" {
                    return py_get_attr(&effective_class, name);
                }
                // __getattr__ fallback: if the class defines __getattr__, invoke it
                if name != "__getattr__" {
                    if let Some(getattr_fn) = lookup_in_class_mro(&effective_class, "__getattr__") {
                        let name_obj = PyObject::str_val(CompactString::from(name));
                        if let Ok(result) = call_callable(&getattr_fn, &[obj.clone(), name_obj]) {
                            return Some(result);
                        }
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
                    let mut map = new_fx_hashkey_map();
                    for (k, v) in ns.iter() {
                        if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                            map.insert(hk, v.clone());
                        }
                    }
                    return Some(PyObject::wrap(PyObjectPayload::MappingProxy(
                        Rc::new(PyCell::new(map)),
                    )));
                }
                if name == "__module__" {
                    // Check namespace first for explicitly set __module__
                    if let Some(v) = cd.namespace.read().get("__module__") {
                        return Some(v.clone());
                    }
                    return Some(PyObject::str_val(intern_or_new("__main__")));
                }
                if name == "__qualname__" {
                    // Check namespace first
                    if let Some(v) = cd.namespace.read().get("__qualname__") {
                        return Some(v.clone());
                    }
                    return Some(PyObject::str_val(cd.name.clone()));
                }
                if name == "__subclasses__" {
                    let subs = cd.subclasses.clone();
                    return Some(PyObject::native_closure("__subclasses__", move |_args| {
                        let refs = subs.read();
                        let alive: Vec<PyObjectRef> = refs.iter()
                            .filter_map(|w| w.upgrade())
                            .collect();
                        Ok(PyObject::list(alive))
                    }));
                }
                if name == "mro" {
                    let self_cls = obj.clone();
                    let mro_data = cd.mro.clone();
                    return Some(PyObject::native_closure("mro", move |_args| {
                        let mut mro_list = vec![self_cls.clone()];
                        mro_list.extend(mro_data.iter().cloned());
                        Ok(PyObject::list(mro_list))
                    }));
                }
                // Check own namespace first, then bases
                if let Some(v) = cd.namespace.read().get(name).cloned() {
                    match &v.payload {
                        PyObjectPayload::StaticMethod(func) => return Some(func.clone()),
                        PyObjectPayload::ClassMethod(func) => {
                            return Some(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj.clone(),
                                    method: func.clone(),
                                }
                            }));
                        }
                        _ => return Some(v),
                    }
                }
                // Walk the computed MRO (C3 linearization) for correct diamond resolution
                let mro_chain: &[PyObjectRef] = if !cd.mro.is_empty() { &cd.mro } else { &cd.bases };
                for base in mro_chain {
                    if let PyObjectPayload::Class(bcd) = &base.payload {
                        if let Some(v) = bcd.namespace.read().get(name).cloned() {
                            match &v.payload {
                                PyObjectPayload::StaticMethod(func) => return Some(func.clone()),
                                PyObjectPayload::ClassMethod(func) => {
                                    return Some(PyObjectRef::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: obj.clone(),
                                            method: func.clone(),
                                        }
                                    }));
                                }
                                _ => return Some(v),
                            }
                        }
                        // If base has its own bases/MRO, recurse (for Rust-created classes with empty MRO)
                        if bcd.mro.is_empty() && !bcd.bases.is_empty() {
                            if let Some(v) = py_get_attr(base, name) {
                                return Some(v);
                            }
                        }
                    } else if let Some(v) = base.get_attr(name) {
                        return Some(v);
                    }
                }
                // If class has a metaclass, look in metaclass namespace too
                // (e.g., cls._instances where _instances is a metaclass class attribute)
                // But skip __new__/__init__ — those are type-level constructors,
                // not methods on instances of the metaclass.
                if let Some(meta) = &cd.metaclass {
                    if name != "__new__" && name != "__init__" {
                        if let PyObjectPayload::Class(mcd) = &meta.payload {
                            if let Some(v) = mcd.namespace.read().get(name).cloned() {
                                return Some(v);
                            }
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
                // Fallback: synthesize object-level dunder methods that all classes inherit
                if name == "__setattr__" {
                    return Some(PyObject::native_function("__setattr__", |args| {
                        if args.len() < 3 {
                            return Err(PyException::type_error("__setattr__ requires 3 arguments"));
                        }
                        let attr_name = args[1].py_to_string();
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            inst.attrs.write().insert(CompactString::from(attr_name.as_str()), args[2].clone());
                        }
                        Ok(PyObject::none())
                    }));
                }
                if name == "__delattr__" {
                    return Some(PyObject::native_function("__delattr__", |args| {
                        if args.len() < 2 {
                            return Err(PyException::type_error("__delattr__ requires 2 arguments"));
                        }
                        let attr_name = args[1].py_to_string();
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            inst.attrs.write().shift_remove(attr_name.as_str());
                        }
                        Ok(PyObject::none())
                    }));
                }
                if name == "__getattribute__" {
                    return Some(PyObject::native_function("__getattribute__", |args| {
                        if args.len() < 2 {
                            return Err(PyException::type_error("__getattribute__ requires 2 arguments"));
                        }
                        let attr_name = args[1].py_to_string();
                        args[0].get_attr(&attr_name).ok_or_else(||
                            PyException::attribute_error(format!(
                                "'{}' object has no attribute '{}'",
                                args[0].type_name(), attr_name
                            ))
                        )
                    }));
                }
                // Note: Do NOT add __init__ fallback here — it breaks
                // dataclass auto-init detection (which checks cls.get_attr("__init__")).
                if name == "__repr__" {
                    let cls_name = cd.name.clone();
                    return Some(PyObject::native_closure("__repr__", move |args| {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from(format!("<class '{}'>", cls_name))));
                        }
                        let addr = PyObjectRef::as_ptr(&args[0]) as usize;
                        Ok(PyObject::str_val(CompactString::from(format!("<{} object at 0x{:x}>", cls_name, addr))))
                    }));
                }
                if name == "__hash__" {
                    return Some(PyObject::native_function("__hash__", |args| {
                        if args.is_empty() {
                            return Err(PyException::type_error("__hash__ requires 1 argument"));
                        }
                        Ok(PyObject::int(PyObjectRef::as_ptr(&args[0]) as i64))
                    }));
                }
                if name == "__eq__" {
                    return Some(PyObject::native_function("__eq__", |args| {
                        if args.len() < 2 {
                            return Ok(PyObject::not_implemented());
                        }
                        Ok(PyObject::bool_val(PyObjectRef::ptr_eq(&args[0], &args[1])))
                    }));
                }
                if name == "__ne__" {
                    return Some(PyObject::native_function("__ne__", |args| {
                        if args.len() < 2 {
                            return Ok(PyObject::not_implemented());
                        }
                        Ok(PyObject::bool_val(!PyObjectRef::ptr_eq(&args[0], &args[1])))
                    }));
                }
                None
            }
            PyObjectPayload::Module(m) => {
                if name == "__class__" {
                    return Some(PyObject::builtin_type(CompactString::from("module")));
                }
                if name == "__dict__" {
                    // Return module attrs as a dict
                    let attrs = m.attrs.read();
                    let mut map = new_fx_hashkey_map();
                    for (k, v) in attrs.iter() {
                        map.insert(DictKey::str_key(k.clone()), v.clone());
                    }
                    Some(PyObject::dict(map))
                } else {
                    m.attrs.read().get(name).cloned()
                }
            }
            PyObjectPayload::Slice(sd) => {
                match name {
                    "start" => Some(sd.start.clone().unwrap_or_else(PyObject::none)),
                    "stop" => Some(sd.stop.clone().unwrap_or_else(PyObject::none)),
                    "step" => Some(sd.step.clone().unwrap_or_else(PyObject::none)),
                    "__class__" => Some(PyObject::builtin_type(CompactString::from("slice"))),
                    "indices" => {
                        let s_start = sd.start.clone();
                        let s_stop = sd.stop.clone();
                        let s_step = sd.step.clone();
                        Some(PyObject::native_closure("slice.indices", move |args| {
                            if args.is_empty() {
                                return Err(crate::error::PyException::type_error(
                                    "slice.indices() requires a length argument"));
                            }
                            let length = args[0].to_int().map_err(|_|
                                crate::error::PyException::type_error("length must be an integer"))? as i64;
                            if length < 0 {
                                return Err(crate::error::PyException::value_error(
                                    "length should not be negative"));
                            }
                            let step_val = match &s_step {
                                Some(s) => s.to_int().unwrap_or(1) as i64,
                                None => 1,
                            };
                            if step_val == 0 {
                                return Err(crate::error::PyException::value_error(
                                    "slice step cannot be zero"));
                            }
                            let start_val = match &s_start {
                                Some(s) => {
                                    let v = s.to_int().unwrap_or(0) as i64;
                                    if v < 0 {
                                        (v + length).max(if step_val < 0 { -1 } else { 0 })
                                    } else {
                                        v.min(length)
                                    }
                                }
                                None => if step_val < 0 { length - 1 } else { 0 },
                            };
                            let stop_val = match &s_stop {
                                Some(s) => {
                                    let v = s.to_int().unwrap_or(0) as i64;
                                    if v < 0 {
                                        (v + length).max(if step_val < 0 { -1 } else { 0 })
                                    } else {
                                        v.min(length)
                                    }
                                }
                                None => if step_val < 0 { -1 } else { length },
                            };
                            Ok(PyObject::tuple(vec![
                                PyObject::int(start_val),
                                PyObject::int(stop_val),
                                PyObject::int(step_val),
                            ]))
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::Complex { real, imag } => {
                match name {
                    "real" => Some(PyObject::float(*real)),
                    "imag" => Some(PyObject::float(*imag)),
                    "__class__" => Some(PyObject::builtin_type(CompactString::from("complex"))),
                    "conjugate" => Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from("conjugate")))
                    })),
                    _ => None,
                }
            }
            PyObjectPayload::BuiltinType(n) => {
                match name {
                    "__name__" | "__qualname__" => Some(PyObject::str_val((**n).clone())),
                    "__module__" => Some(PyObject::str_val(CompactString::from("builtins"))),
                    "__dict__" => {
                        // Return a mappingproxy with common type descriptors
                        let mut map = new_fx_hashkey_map();
                        if n.as_str() == "type" || n.as_str() == "object" {
                            // type.__dict__["__dict__"] is a getset_descriptor with __get__
                            // that returns obj.__dict__ when called as descriptor.__get__(obj)
                            let desc_cls = PyObject::class(CompactString::from("getset_descriptor"), vec![], IndexMap::new());
                            let desc = PyObject::instance(desc_cls);
                            if let PyObjectPayload::Instance(ref inst) = desc.payload {
                                inst.attrs.write().insert(CompactString::from("__get__"),
                                    PyObject::native_function("getset_descriptor.__get__", |args: &[PyObjectRef]| {
                                        // __get__(self, obj, objtype=None) → obj.__dict__
                                        if args.len() >= 2 {
                                            if let Some(d) = args[1].get_attr("__dict__") {
                                                return Ok(d);
                                            }
                                        }
                                        if args.len() >= 1 {
                                            if let Some(d) = args[0].get_attr("__dict__") {
                                                return Ok(d);
                                            }
                                        }
                                        Ok(PyObject::dict(new_fx_hashkey_map()))
                                    }),
                                );
                            }
                            map.insert(DictKey::str_key(CompactString::from("__dict__")), desc);
                            map.insert(DictKey::str_key(CompactString::from("__doc__")), PyObject::none());
                            map.insert(DictKey::str_key(CompactString::from("__repr__")), 
                                PyObject::builtin_type(CompactString::from("wrapper_descriptor")));
                            map.insert(DictKey::str_key(CompactString::from("__subclasshook__")),
                                PyObject::builtin_type(CompactString::from("method_descriptor")));
                        }
                        Some(PyObject::wrap(PyObjectPayload::MappingProxy(
                            Rc::new(PyCell::new(map)),
                        )))
                    }
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
                            let mut map = new_fx_hashkey_map();
                            for k in keys {
                                let dk = DictKey::try_new(&k)?;
                                map.insert(dk, value.clone());
                            }
                            Ok(PyObject::dict(map))
                        }))
                    }
                    "maketrans" if n.as_str() == "str" => {
                        Some(PyObject::native_function("str.maketrans", |args| {
                            if args.is_empty() { return Err(PyException::type_error("maketrans() requires at least 1 argument")); }
                            let mut result_map = new_fx_hashkey_map();
                            if args.len() == 1 {
                                if let PyObjectPayload::Dict(map) = &args[0].payload {
                                    for (k, v) in map.read().iter() {
                                        let key = if let Some(n) = k.as_int() {
                                            n.clone()
                                        } else if let Some(s) = k.as_str() {
                                            if let Some(c) = s.chars().next() { PyInt::Small(c as i64) } else { continue; }
                                        } else {
                                            continue;
                                        };
                                        result_map.insert(DictKey(key.to_object()), v.clone());
                                    }
                                }
                            } else {
                                let x = args[0].py_to_string();
                                let y = args[1].py_to_string();
                                for (cx, cy) in x.chars().zip(y.chars()) {
                                    result_map.insert(DictKey(PyObject::int(cx as i64)), PyObject::int(cy as i64));
                                }
                                if args.len() > 2 {
                                    let z = args[2].py_to_string();
                                    for cz in z.chars() {
                                        result_map.insert(DictKey(PyObject::int(cz as i64)), PyObject::none());
                                    }
                                }
                            }
                            Ok(PyObject::dict(result_map))
                        }))
                    }
                    "fromhex" if n.as_str() == "bytes" || n.as_str() == "bytearray" => {
                        let is_bytearray = n.as_str() == "bytearray";
                        Some(PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(NativeClosureData {
                            name: CompactString::from("fromhex"),
                            func: std::rc::Rc::new(move |args| {
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
                                if is_bytearray {
                                    Ok(PyObject::bytearray(bytes))
                                } else {
                                    Ok(PyObject::bytes(bytes))
                                }
                            }),
                        }))))
                    }
                    // object.__setattr__(instance, name, value) — bypass custom __setattr__
                    "__setattr__" => {
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
                    "__getattribute__" => {
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
                    "__delattr__" => {
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
                            // O(1) lookup for supported dunders on builtin types.
                            use std::collections::HashSet;
                            use std::sync::LazyLock;
                            static BUILTIN_DUNDERS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
                                [
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
                                ].into_iter().collect()
                            });
                            // Container-only dunders: not valid for numeric/NoneType
                            static CONTAINER_DUNDERS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
                                [
                                    "__len__", "__getitem__", "__setitem__", "__delitem__",
                                    "__contains__", "__iter__", "__next__", "__reversed__",
                                    "__missing__",
                                ].into_iter().collect()
                            });
                            if BUILTIN_DUNDERS.contains(name) {
                                // Exclude container dunders for non-container types
                                let is_non_container = matches!(n.as_str(),
                                    "int" | "float" | "complex" | "bool" | "NoneType" | "type");
                                if is_non_container && CONTAINER_DUNDERS.contains(name) {
                                    return None;
                                }
                                // Check if resolve_builtin_type_method has a real implementation
                                if let Some(native) = resolve_builtin_type_method(n, name) {
                                    return Some(native);
                                }
                                Some(PyObjectRef::new(PyObject {
                                    payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                                }))
                            } else {
                                None
                            }
                        } else {
                            // Unbound method access: str.upper, list.append, etc.
                            // Only return a bound method if the type actually has this method.
                            if let Some(native) = resolve_builtin_type_method(n, name) {
                                return Some(native);
                            }
                            // Check a known set of non-dunder methods per type
                            let has_method = match n.as_str() {
                                "str" => matches!(name,
                                    "upper" | "lower" | "strip" | "lstrip" | "rstrip" | "split" | "rsplit"
                                    | "join" | "replace" | "find" | "rfind" | "index" | "rindex" | "count"
                                    | "startswith" | "endswith" | "encode" | "decode" | "format"
                                    | "format_map" | "center" | "ljust" | "rjust" | "zfill"
                                    | "expandtabs" | "title" | "capitalize" | "swapcase" | "casefold"
                                    | "isalpha" | "isdigit" | "isalnum" | "isspace" | "isupper" | "islower"
                                    | "istitle" | "isnumeric" | "isdecimal" | "isidentifier" | "isprintable"
                                    | "isascii" | "partition" | "rpartition" | "splitlines" | "translate"
                                    | "removeprefix" | "removesuffix" | "maketrans"
                                ),
                                "list" => matches!(name,
                                    "append" | "extend" | "insert" | "remove" | "pop" | "clear"
                                    | "index" | "count" | "sort" | "reverse" | "copy"
                                ),
                                "dict" => matches!(name,
                                    "keys" | "values" | "items" | "get" | "pop" | "setdefault"
                                    | "update" | "clear" | "copy" | "fromkeys"
                                ),
                                "set" | "frozenset" => matches!(name,
                                    "add" | "remove" | "discard" | "pop" | "clear" | "copy"
                                    | "union" | "intersection" | "difference" | "symmetric_difference"
                                    | "update" | "intersection_update" | "difference_update"
                                    | "symmetric_difference_update" | "issubset" | "issuperset" | "isdisjoint"
                                ),
                                "tuple" => matches!(name, "count" | "index"),
                                "bytes" | "bytearray" => matches!(name,
                                    "decode" | "hex" | "count" | "find" | "rfind" | "index" | "rindex"
                                    | "split" | "rsplit" | "join" | "replace" | "strip" | "lstrip" | "rstrip"
                                    | "startswith" | "endswith" | "upper" | "lower" | "title" | "capitalize"
                                    | "swapcase" | "center" | "ljust" | "rjust" | "zfill" | "expandtabs"
                                    | "isalpha" | "isdigit" | "isalnum" | "isspace" | "isupper" | "islower"
                                    | "translate" | "partition" | "rpartition" | "splitlines" | "fromhex"
                                    | "extend" | "append" | "insert" | "pop" | "remove" | "reverse" | "copy" | "clear"
                                    | "maketrans"
                                ),
                                "int" => matches!(name,
                                    "bit_length" | "to_bytes" | "from_bytes" | "conjugate"
                                    | "real" | "imag" | "numerator" | "denominator"
                                ),
                                "float" => matches!(name,
                                    "is_integer" | "hex" | "fromhex" | "as_integer_ratio"
                                    | "conjugate" | "real" | "imag"
                                ),
                                "type" => matches!(name, "mro"),
                                _ => false,
                            };
                            if has_method {
                                Some(PyObjectRef::new(PyObject {
                                    payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                                }))
                            } else {
                                None
                            }
                        }
                    }
                }
            }
            PyObjectPayload::Property(pd) => {
                match name {
                    "__doc__" => {
                        // Return the docstring from pd.fget, if any
                        if let Some(fg) = &pd.fget {
                            if let Some(doc) = fg.get_attr("__doc__") {
                                return Some(doc);
                            }
                        }
                        return Some(PyObject::none());
                    }
                    "setter" | "getter" | "deleter" | "fget" | "fset" | "fdel" => {
                        match name {
                            "fget" => return pd.fget.clone().or_else(|| Some(PyObject::none())),
                            "fset" => return pd.fset.clone().or_else(|| Some(PyObject::none())),
                            "fdel" => return pd.fdel.clone().or_else(|| Some(PyObject::none())),
                            _ => {}
                        }
                        // Return a BuiltinBoundMethod that the VM will handle
                        Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::Partial(pd) => {
                match name {
                    "func" => Some(pd.func.clone()),
                    "args" => Some(PyObject::tuple(pd.args.clone())),
                    "keywords" => {
                        let mut map = new_fx_hashkey_map();
                        for (k, v) in &pd.kwargs {
                            map.insert(DictKey::str_key(k.clone()), v.clone());
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
                            ExceptionKind::JSONDecodeError => Some(ExceptionKind::ValueError),
                            ExceptionKind::ModuleNotFoundError => Some(ExceptionKind::ImportError),
                            ExceptionKind::NotImplementedError | ExceptionKind::RecursionError => {
                                Some(ExceptionKind::RuntimeError)
                            }
                            ExceptionKind::UnboundLocalError => Some(ExceptionKind::NameError),
                            ExceptionKind::IndentationError => Some(ExceptionKind::SyntaxError),
                            ExceptionKind::TabError => Some(ExceptionKind::IndentationError),
                            ExceptionKind::SubprocessError => Some(ExceptionKind::Exception),
                            ExceptionKind::CalledProcessError | ExceptionKind::TimeoutExpired => {
                                Some(ExceptionKind::SubprocessError)
                            }
                            ExceptionKind::DeprecationWarning | ExceptionKind::RuntimeWarning |
                            ExceptionKind::UserWarning | ExceptionKind::SyntaxWarning |
                            ExceptionKind::FutureWarning | ExceptionKind::ImportWarning |
                            ExceptionKind::UnicodeWarning | ExceptionKind::BytesWarning |
                            ExceptionKind::ResourceWarning | ExceptionKind::PendingDeprecationWarning => {
                                Some(ExceptionKind::Warning)
                            }
                            ExceptionKind::BaseExceptionGroup => Some(ExceptionKind::BaseException),
                            ExceptionKind::ExceptionGroup => Some(ExceptionKind::BaseExceptionGroup),
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
                        let mut current = *kind;
                        loop {
                            let parent = match current {
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
                                ExceptionKind::JSONDecodeError => ExceptionKind::ValueError,
                                ExceptionKind::ModuleNotFoundError => ExceptionKind::ImportError,
                                ExceptionKind::NotImplementedError | ExceptionKind::RecursionError => ExceptionKind::RuntimeError,
                                ExceptionKind::UnboundLocalError => ExceptionKind::NameError,
                                ExceptionKind::IndentationError => ExceptionKind::SyntaxError,
                                ExceptionKind::TabError => ExceptionKind::IndentationError,
                                ExceptionKind::SubprocessError => ExceptionKind::Exception,
                                ExceptionKind::CalledProcessError | ExceptionKind::TimeoutExpired => ExceptionKind::SubprocessError,
                                ExceptionKind::DeprecationWarning | ExceptionKind::RuntimeWarning |
                                ExceptionKind::UserWarning | ExceptionKind::SyntaxWarning |
                                ExceptionKind::FutureWarning | ExceptionKind::ImportWarning |
                                ExceptionKind::UnicodeWarning | ExceptionKind::BytesWarning |
                                ExceptionKind::ResourceWarning | ExceptionKind::PendingDeprecationWarning => ExceptionKind::Warning,
                                ExceptionKind::BaseExceptionGroup => ExceptionKind::BaseException,
                                ExceptionKind::ExceptionGroup => ExceptionKind::BaseExceptionGroup,
                            };
                            mro.push(PyObject::exception_type(parent));
                            current = parent;
                        }
                        mro.push(PyObject::builtin_type(CompactString::from("object")));
                        Some(PyObject::tuple(mro))
                    }
                    "__module__" => Some(PyObject::str_val(CompactString::from("builtins"))),
                    "__class__" => Some(PyObject::builtin_type(CompactString::from("type"))),
                    "__init__" => {
                        // Unbound __init__: ExcType.__init__(self, *args)
                        Some(PyObject::native_function("__init__", |args| {
                            if args.is_empty() { return Ok(PyObject::none()); }
                            let target = &args[0];
                            let init_args: Vec<PyObjectRef> = args[1..].to_vec();
                            match &target.payload {
                                PyObjectPayload::Instance(idata) => {
                                    idata.attrs.write().insert(
                                        CompactString::from("args"),
                                        PyObject::tuple(init_args),
                                    );
                                }
                                PyObjectPayload::ExceptionInstance(ei) => {
                                    ei.ensure_attrs().write().insert(
                                        CompactString::from("args"),
                                        PyObject::tuple(init_args),
                                    );
                                }
                                _ => {}
                            }
                            Ok(PyObject::none())
                        }))
                    }
                    "__new__" => {
                        // Only return __new__ for direct ExceptionType calls (ValueError()),
                        // not for user-defined subclasses (handled by normal Class instantiation)
                        None
                    }
                    "__str__" => {
                        Some(PyObject::native_function("__str__", |args| {
                            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
                            Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
                        }))
                    }
                    "__repr__" => {
                        let kind_clone = *kind;
                        Some(PyObject::native_closure("__repr__", move |args| {
                            if args.is_empty() {
                                return Ok(PyObject::str_val(CompactString::from(
                                    format!("{:?}()", kind_clone))));
                            }
                            let s = args[0].py_to_string();
                            Ok(PyObject::str_val(CompactString::from(
                                format!("{:?}({})", kind_clone, s))))
                        }))
                    }
                    "mro" => {
                        // mro() method: returns __mro__ as a list
                        let mro_val = obj.get_attr("__mro__");
                        Some(PyObject::native_closure("mro", move |_args| {
                            if let Some(ref mro_tuple) = mro_val {
                                if let PyObjectPayload::Tuple(items) = &mro_tuple.payload {
                                    return Ok(PyObject::list((**items).clone()));
                                }
                            }
                            Ok(PyObject::list(vec![]))
                        }))
                    }
                    "__subclasses__" => {
                        Some(PyObject::native_function("__subclasses__", |_args| {
                            Ok(PyObject::list(vec![]))
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::ExceptionInstance(ei) => {
                match name {
                    "args" => {
                        // Check attrs first (may be overwritten by __init__)
                        if let Some(v) = ei.get_attrs().and_then(|a| a.read().get("args").cloned()) {
                            return Some(v);
                        }
                        if ei.args.is_empty() {
                            if ei.message.is_empty() {
                                Some(PyObject::tuple(vec![]))
                            } else {
                                Some(PyObject::tuple(vec![PyObject::str_val(ei.message.clone())]))
                            }
                        } else {
                            Some(PyObject::tuple(ei.args.clone()))
                        }
                    }
                    "__class__" => Some(PyObject::exception_type(ei.kind)),
                    "code" if ei.kind == ExceptionKind::SystemExit => {
                        // SystemExit.code: first arg or message
                        if !ei.args.is_empty() {
                            Some(ei.args[0].clone())
                        } else if !ei.message.is_empty() {
                            // Try to parse as int, otherwise return as string
                            if let Ok(n) = ei.message.parse::<i64>() {
                                Some(PyObject::int(n))
                            } else {
                                Some(PyObject::str_val(ei.message.clone()))
                            }
                        } else {
                            Some(PyObject::none())
                        }
                    }
                    "value" => {
                        // StopIteration.value — check attrs first, then args[0], then None
                        if let Some(v) = ei.get_attrs().and_then(|a| a.read().get("value").cloned()) {
                            Some(v)
                        } else if !ei.args.is_empty() {
                            Some(ei.args[0].clone())
                        } else {
                            Some(PyObject::none())
                        }
                    }
                    "__cause__" => {
                        ei.get_attrs().and_then(|a| a.read().get("__cause__").cloned()).or_else(|| Some(PyObject::none()))
                    }
                    "__context__" => {
                        ei.get_attrs().and_then(|a| a.read().get("__context__").cloned()).or_else(|| Some(PyObject::none()))
                    }
                    "__suppress_context__" => {
                        ei.get_attrs().and_then(|a| a.read().get("__suppress_context__").cloned()).or_else(|| Some(PyObject::bool_val(false)))
                    }
                    "__traceback__" => {
                        ei.get_attrs().and_then(|a| a.read().get("__traceback__").cloned()).or_else(|| Some(PyObject::none()))
                    }
                    "__notes__" => {
                        ei.get_attrs().and_then(|a| a.read().get("__notes__").cloned())
                    }
                    "add_note" => {
                        let obj_ref = obj.clone();
                        Some(PyObject::native_closure("add_note", move |args| {
                            if args.is_empty() {
                                return Err(crate::error::PyException::type_error(
                                    "add_note() missing required argument: 'note'"));
                            }
                            let note = &args[0];
                            if let PyObjectPayload::ExceptionInstance(ref ei) = obj_ref.payload {
                                let mut w = ei.ensure_attrs().write();
                                let notes = w.entry(CompactString::from("__notes__"))
                                    .or_insert_with(|| PyObject::list(vec![]));
                                if let PyObjectPayload::List(list) = &notes.payload {
                                    list.write().push(note.clone());
                                }
                            }
                            Ok(PyObject::none())
                        }))
                    }
                    "with_traceback" => {
                        let obj_ref = obj.clone();
                        Some(PyObject::native_closure("with_traceback", move |args| {
                            if !args.is_empty() {
                                if let PyObjectPayload::ExceptionInstance(ref ei) = obj_ref.payload {
                                    ei.ensure_attrs().write().insert(
                                        CompactString::from("__traceback__"), args[0].clone());
                                }
                            }
                            Ok(obj_ref.clone())
                        }))
                    }
                    // OSError attributes: .errno, .strerror, .filename
                    "errno" | "strerror" | "filename" if ei.kind.is_subclass_of(&ExceptionKind::OSError) => {
                        ei.get_attrs().and_then(|a| a.read().get(name).cloned()).or_else(|| Some(PyObject::none()))
                    }
                    _ => {
                        // Check user-set attrs (e.g., __cause__)
                        ei.get_attrs().and_then(|a| a.read().get(name).cloned())
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
                    "__class__" => Some(PyObject::builtin_type(CompactString::from("function"))),
                    "__defaults__" => {
                        if f.defaults.is_empty() { Some(PyObject::none()) }
                        else { Some(PyObject::tuple(f.defaults.clone())) }
                    }
                    "__module__" => Some(PyObject::str_val(intern_or_new("__main__"))),
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
                        let mut map = new_fx_hashkey_map();
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
                                PyObject::cell(cell.clone())
                            }).collect();
                            Some(PyObject::tuple(cells))
                        }
                    }
                    "__code__" => Some(PyObject::wrap(PyObjectPayload::Code(Rc::clone(&f.code)))),
                    "__kwdefaults__" => {
                        if f.kw_defaults.is_empty() { Some(PyObject::none()) }
                        else {
                            let mut map = new_fx_hashkey_map();
                            for (k, v) in &f.kw_defaults {
                                if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                                    map.insert(hk, v.clone());
                                }
                            }
                            Some(PyObject::dict(map))
                        }
                    }
                    "__globals__" => {
                        let g = f.globals.read();
                        let mut map: FxHashKeyMap = new_fx_hashkey_map();
                        for (k, v) in g.iter() {
                            if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                                map.insert(hk, v.clone());
                            }
                        }
                        Some(PyObject::dict(map))
                    }
                    "__get__" => {
                        let func = obj.clone();
                        Some(PyObject::native_closure("__get__", move |args| {
                            if args.is_empty() {
                                return Err(PyException::type_error("__get__ requires at least 1 argument"));
                            }
                            let instance = &args[0];
                            if matches!(&instance.payload, PyObjectPayload::None) {
                                return Ok(func.clone());
                            }
                            Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: instance.clone(),
                                    method: func.clone(),
                                }
                            }))
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::NativeFunction(nf) => match name {
                "__name__" => Some(PyObject::str_val(CompactString::from(nf.name.as_str()))),
                "__qualname__" => Some(PyObject::str_val(CompactString::from(nf.name.as_str()))),
                "__module__" => Some(PyObject::str_val(CompactString::from("builtins"))),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("builtin_function_or_method"))),
                "__doc__" => Some(PyObject::none()),
                "__call__" => Some(obj.clone()),
                "__get__" => {
                    let func_obj = obj.clone();
                    Some(PyObject::native_closure("__get__", move |args| {
                        if args.is_empty() {
                            return Err(PyException::type_error("__get__ requires at least 1 argument"));
                        }
                        let instance = &args[0];
                        if matches!(&instance.payload, PyObjectPayload::None) {
                            return Ok(func_obj.clone());
                        }
                        Ok(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: instance.clone(),
                                method: func_obj.clone(),
                            }
                        }))
                    }))
                }
                _ => None,
            }
            PyObjectPayload::BuiltinFunction(fname) => match name {
                "__name__" | "__qualname__" => Some(PyObject::str_val((**fname).clone())),
                "__module__" => Some(PyObject::str_val(CompactString::from("builtins"))),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("builtin_function_or_method"))),
                "__doc__" => Some(PyObject::none()),
                "__call__" => Some(obj.clone()),
                _ => None,
            }
            PyObjectPayload::ClassMethod(func) => match name {
                "__class__" => Some(PyObject::builtin_type(CompactString::from("classmethod"))),
                "__func__" => Some(func.clone()),
                "__wrapped__" => Some(func.clone()),
                "__get__" => {
                    let func = func.clone();
                    Some(PyObject::native_closure("__get__", move |args| {
                        if args.len() < 2 {
                            return Err(PyException::type_error("__get__ requires 2 arguments"));
                        }
                        let owner = &args[1];
                        Ok(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: owner.clone(),
                                method: func.clone(),
                            }
                        }))
                    }))
                }
                _ => None,
            }
            PyObjectPayload::StaticMethod(func) => match name {
                "__class__" => Some(PyObject::builtin_type(CompactString::from("staticmethod"))),
                "__func__" => Some(func.clone()),
                "__wrapped__" => Some(func.clone()),
                "__get__" => {
                    let func = func.clone();
                    Some(PyObject::native_closure("__get__", move |_args| {
                        Ok(func.clone())
                    }))
                }
                _ => func.get_attr(name),
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
                "__neg__" | "__pos__" | "__invert__" |
                "__repr__" | "__str__" | "__hash__" | "__format__" |
                "__eq__" | "__ne__" | "__lt__" | "__le__" | "__gt__" | "__ge__" |
                "__add__" | "__sub__" | "__mul__" | "__truediv__" |
                "__floordiv__" | "__mod__" | "__pow__" | "__divmod__" |
                "__lshift__" | "__rshift__" | "__and__" | "__or__" | "__xor__" |
                "__ceil__" | "__floor__" | "__round__" | "__trunc__" |
                "__sizeof__" | "as_integer_ratio" => Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                })),
                _ => None,
            },
            // Float property-like attributes
            PyObjectPayload::Float(f) => match name {
                "real" => Some(PyObject::float(*f)),
                "imag" => Some(PyObject::float(0.0)),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("float"))),
                "is_integer" | "conjugate" | "hex" | "__abs__" |
                "__int__" | "__float__" | "__bool__" | "__index__" |
                "__neg__" | "__pos__" |
                "__repr__" | "__str__" | "__hash__" | "__format__" |
                "__eq__" | "__ne__" | "__lt__" | "__le__" | "__gt__" | "__ge__" |
                "__add__" | "__sub__" | "__mul__" | "__truediv__" |
                "__floordiv__" | "__mod__" | "__pow__" | "__divmod__" |
                "__round__" | "__ceil__" | "__floor__" | "__trunc__" |
                "__sizeof__" | "as_integer_ratio" | "fromhex" => Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
                "__repr__" | "__str__" | "__hash__" | "__format__" | "__sizeof__" => Some(PyObjectRef::new(PyObject {                    payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                })),
                _ => None,
            },
            // Built-in type methods — return bound method for KNOWN methods only
            PyObjectPayload::Range(rd) => match name {
                "start" => Some(PyObject::int(rd.start)),
                "stop" => Some(PyObject::int(rd.stop)),
                "step" => Some(PyObject::int(rd.step)),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("range"))),
                "count" | "index" | "__contains__" | "__iter__" | "__reversed__" | "__len__" => {
                    Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                    }))
                }
                _ => None,
            },
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
                    | "__mod__" | "__bool__" | "__sizeof__"
                ) {
                    return Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
                    | "__reversed__" | "__bool__" | "__hash__" | "__sizeof__"
                ) {
                    return Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                    }));
                }
                None
            }
            PyObjectPayload::Dict(_) | PyObjectPayload::InstanceDict(_) | PyObjectPayload::MappingProxy(_) => {
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
                    | "__or__" | "__ior__" | "__bool__" | "__hash__" | "__sizeof__"
                ) {
                    return Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
                    | "__repr__" | "__str__" | "__add__" | "__mul__" | "__bool__" | "__sizeof__"
                ) {
                    return Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
                    | "__gt__" | "__ge__" | "__repr__" | "__str__" | "__bool__" | "__hash__" | "__sizeof__"
                ) {
                    return Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
                    | "__repr__" | "__str__" | "__bool__" | "__sizeof__"
                ) {
                    return Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
                    | "partition" | "rpartition" | "removeprefix" | "removesuffix"
                    | "rsplit" | "splitlines" | "translate"
                    | "tobytes" | "tolist" | "release"
                    | "append" | "extend" | "pop" | "insert" | "clear" | "reverse" | "copy"
                    | "__len__" | "__contains__" | "__iter__" | "__getitem__" | "__setitem__"
                    | "__eq__" | "__ne__" | "__repr__" | "__str__" | "__add__" | "__mul__"
                    | "__bool__" | "__hash__" | "__sizeof__"
                ) {
                    return Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                    }));
                }
                None
            }
            PyObjectPayload::Generator(_) => {
                match name {
                    // Generator protocol: send, throw, close, __next__, __iter__
                    "send" | "throw" | "close" | "__next__" => {
                        Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                        }))
                    }
                    // Context manager protocol: generators from @contextmanager
                    // __enter__ calls next(gen), __exit__ calls gen.close()/gen.throw()
                    "__enter__" | "__exit__" => {
                        Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
                        Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
                        Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                        }))
                    }
                    // Async iteration protocol — __aiter__ returns self when called
                    "__aiter__" | "__anext__" | "asend" | "athrow" | "aclose" => {
                        Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
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
                        Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::Super { cls, instance } => {
                // super().__class__ → the 'super' type itself
                if name == "__class__" {
                    return Some(PyObject::builtin_type(CompactString::from("super")));
                }
                // super().__getattribute__(name) → behaves like object.__getattribute__(self, name)
                // Must check both MRO (from parent) AND instance __dict__ to match CPython.
                if name == "__getattribute__" {
                    let super_obj = obj.clone();
                    let inst_ref = instance.clone();
                    return Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::NativeClosure(Box::new(NativeClosureData {
                            name: CompactString::from("super.__getattribute__"),
                            func: std::rc::Rc::new(move |args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "__getattribute__() requires at least 1 argument"
                                    ));
                                }
                                let attr_name = args[0].py_to_string();
                                // First try MRO lookup through the super proxy
                                if let Some(v) = super_obj.get_attr(&attr_name) {
                                    return Ok(v);
                                }
                                // Fall back to instance __dict__ (like object.__getattribute__)
                                if let PyObjectPayload::Instance(inst) = &inst_ref.payload {
                                    if let Some(v) = inst.attrs.read().get(attr_name.as_str()) {
                                        return Ok(v.clone());
                                    }
                                }
                                Err(PyException::attribute_error(format!(
                                    "'super' object has no attribute '{}'", attr_name
                                )))
                            }),
                        }))
                    }));
                }
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
                        let cls_is_self = PyObjectRef::ptr_eq(cls, &rt_cls);
                        let mut found_cls = cls_is_self;
                        for base in mro {
                            if !found_cls {
                                if PyObjectRef::ptr_eq(base, cls) {
                                    found_cls = true;
                                }
                                continue;
                            }
                            // Look in this base's namespace directly
                            if let PyObjectPayload::Class(bcd) = &base.payload {
                                if let Some(v) = bcd.namespace.read().get(name) {
                                    if matches!(&v.payload, PyObjectPayload::Function(_) | PyObjectPayload::NativeClosure(_) | PyObjectPayload::NativeFunction(_)) {
                                        return Some(PyObjectRef::new(PyObject {
                                            payload: PyObjectPayload::BoundMethod {
                                                receiver: instance.clone(),
                                                method: v.clone(),
                                            }
                                        }));
                                    }
                                    // Unwrap descriptors: ClassMethod → bind to class,
                                    // StaticMethod → return raw function
                                    if let PyObjectPayload::ClassMethod(func) = &v.payload {
                                        let bound_cls = match &instance.payload {
                                            PyObjectPayload::Instance(inst) => inst.class.clone(),
                                            _ => instance.clone(),
                                        };
                                        return Some(PyObjectRef::new(PyObject {
                                            payload: PyObjectPayload::BoundMethod {
                                                receiver: bound_cls,
                                                method: func.clone(),
                                            }
                                        }));
                                    }
                                    if let PyObjectPayload::StaticMethod(func) = &v.payload {
                                        return Some(func.clone());
                                    }
                                    return Some(v.clone());
                                }
                            }
                            // ExceptionType base: provide synthetic __init__/__str__
                            if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                                if let Some(resolved) = resolve_exception_type_method(name, instance) {
                                    // Bind to instance so obj is prepended
                                    return Some(PyObjectRef::new(PyObject {
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
                                    return Some(PyObjectRef::new(PyObject {
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
                                    return Some(PyObjectRef::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: instance.clone(),
                                            method: resolved,
                                        }
                                    }));
                                }
                                // For builtin type methods (list.append, dict.update, etc.)
                                // that aren't in resolve_builtin_type_method, return a
                                // BuiltinBoundMethod that the VM dispatches via __builtin_value__
                                let known_methods = match bt_name.as_str() {
                                    "list" => matches!(name, "append" | "extend" | "insert" | "remove"
                                        | "pop" | "clear" | "reverse" | "sort" | "copy" | "count"
                                        | "index" | "__len__" | "__iter__" | "__contains__"
                                        | "__getitem__" | "__setitem__" | "__delitem__"),
                                    "dict" => matches!(name, "keys" | "values" | "items" | "get"
                                        | "pop" | "update" | "setdefault" | "clear" | "copy"
                                        | "popitem" | "__len__" | "__iter__" | "__contains__"
                                        | "__getitem__" | "__setitem__" | "__delitem__"),
                                    "set" => matches!(name, "add" | "remove" | "discard" | "pop"
                                        | "clear" | "copy" | "update" | "intersection_update"
                                        | "difference_update" | "symmetric_difference_update"
                                        | "union" | "intersection" | "difference" | "symmetric_difference"
                                        | "issubset" | "issuperset" | "__len__" | "__iter__" | "__contains__"),
                                    "str" => matches!(name, "upper" | "lower" | "strip" | "lstrip"
                                        | "rstrip" | "split" | "rsplit" | "join" | "replace"
                                        | "startswith" | "endswith" | "find" | "rfind" | "index"
                                        | "rindex" | "count" | "encode" | "format" | "center"
                                        | "ljust" | "rjust" | "zfill" | "title" | "capitalize"
                                        | "swapcase" | "partition" | "rpartition" | "expandtabs"
                                        | "__len__" | "__iter__" | "__contains__" | "__getitem__"),
                                    "int" => matches!(name, "bit_length" | "to_bytes" | "from_bytes"
                                        | "__int__" | "__float__" | "__index__"),
                                    "tuple" => matches!(name, "count" | "index" | "__len__"
                                        | "__iter__" | "__contains__" | "__getitem__"),
                                    _ => false,
                                };
                                if known_methods {
                                    return Some(PyObjectRef::new(PyObject {
                                        payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(instance.clone(), CompactString::from(name)))
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
                                                return Some(PyObjectRef::new(PyObject {
                                                    payload: PyObjectPayload::BoundMethod {
                                                        receiver: instance.clone(),
                                                        method: v.clone(),
                                                    }
                                                }));
                                            }
                                            if let PyObjectPayload::ClassMethod(func) = &v.payload {
                                                let bound_cls = match &instance.payload {
                                                    PyObjectPayload::Instance(inst) => inst.class.clone(),
                                                    _ => instance.clone(),
                                                };
                                                return Some(PyObjectRef::new(PyObject {
                                                    payload: PyObjectPayload::BoundMethod {
                                                        receiver: bound_cls,
                                                        method: func.clone(),
                                                    }
                                                }));
                                            }
                                            if let PyObjectPayload::StaticMethod(func) = &v.payload {
                                                return Some(func.clone());
                                            }
                                            return Some(v.clone());
                                        }
                                    }
                                    // Check BuiltinType bases (e.g., type, object)
                                    if let PyObjectPayload::BuiltinType(bt_name) = &base.payload {
                                        if let Some(resolved) = resolve_builtin_type_method(bt_name.as_str(), name) {
                                            return Some(PyObjectRef::new(PyObject {
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
                                            return Some(PyObjectRef::new(PyObject {
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
                        // Fallback: check instance attrs for methods installed by
                        // parent __init__ (e.g., BytesIO.__init__ installs write/read
                        // as NativeClosure on the instance, not in the class namespace)
                        if let PyObjectPayload::Instance(inst) = &instance.payload {
                            if let Some(v) = inst.attrs.read().get(name).cloned() {
                                if matches!(&v.payload,
                                    PyObjectPayload::NativeClosure(_) |
                                    PyObjectPayload::NativeFunction(_)
                                ) {
                                    return Some(v);
                                }
                                return Some(v);
                            }
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
                        // Builtin __delattr__: object.__delattr__(self, name)
                        if name == "__delattr__" {
                            let inst = instance.clone();
                            return Some(PyObject::native_closure("__delattr__", move |args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error("__delattr__ requires name argument"));
                                }
                                let attr_name = args[0].py_to_string();
                                if let PyObjectPayload::Instance(data) = &inst.payload {
                                    let removed = data.attrs.write().shift_remove(attr_name.as_str());
                                    if removed.is_none() {
                                        return Err(PyException::attribute_error(format!(
                                            "'{}' object has no attribute '{}'",
                                            data.class.py_to_string(), attr_name
                                        )));
                                    }
                                }
                                Ok(PyObject::none())
                            }));
                        }
                        // Builtin __eq__: object.__eq__ is identity comparison
                        if name == "__eq__" {
                            let inst = instance.clone();
                            return Some(PyObject::native_closure("__eq__", move |args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error("__eq__ requires an argument"));
                                }
                                Ok(PyObject::bool_val(PyObjectRef::ptr_eq(&inst, &args[0])))
                            }));
                        }
                        // Builtin __ne__: object.__ne__ is negated identity
                        if name == "__ne__" {
                            let inst = instance.clone();
                            return Some(PyObject::native_closure("__ne__", move |args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error("__ne__ requires an argument"));
                                }
                                Ok(PyObject::bool_val(!PyObjectRef::ptr_eq(&inst, &args[0])))
                            }));
                        }
                        // Builtin __repr__ / __str__: default object repr
                        if name == "__repr__" || name == "__str__" {
                            let inst = instance.clone();
                            return Some(PyObject::native_closure(name, move |_args: &[PyObjectRef]| {
                                let cls_name = if let PyObjectPayload::Instance(data) = &inst.payload {
                                    data.class.py_to_string()
                                } else {
                                    "object".into()
                                };
                                Ok(PyObject::str_val(CompactString::from(
                                    format!("<{} object>", cls_name)
                                )))
                            }));
                        }
                        // Builtin __hash__: default hash from object id
                        if name == "__hash__" {
                            let inst = instance.clone();
                            return Some(PyObject::native_closure("__hash__", move |_args: &[PyObjectRef]| {
                                let ptr = PyObjectRef::as_ptr(&inst) as usize;
                                Ok(PyObject::int(ptr as i64))
                            }));
                        }
                    }
                }
                None
            }
            PyObjectPayload::Code(code) => match name {
                "co_name" => Some(PyObject::str_val(code.name.clone())),
                "co_qualname" => Some(PyObject::str_val(code.qualname.clone())),
                "co_filename" => Some(PyObject::str_val(code.filename.clone())),
                "co_firstlineno" => Some(PyObject::int(code.first_line_number as i64)),
                "co_argcount" => Some(PyObject::int(code.arg_count as i64)),
                "co_posonlyargcount" => Some(PyObject::int(code.posonlyarg_count as i64)),
                "co_kwonlyargcount" => Some(PyObject::int(code.kwonlyarg_count as i64)),
                "co_nlocals" => Some(PyObject::int(code.num_locals as i64)),
                "co_stacksize" => Some(PyObject::int(code.max_stack_size as i64)),
                "co_flags" => Some(PyObject::int(code.flags.bits() as i64)),
                "co_varnames" => Some(PyObject::tuple(
                    code.varnames.iter().map(|s| PyObject::str_val(s.clone())).collect(),
                )),
                "co_names" => Some(PyObject::tuple(
                    code.names.iter().map(|s| PyObject::str_val(s.clone())).collect(),
                )),
                "co_freevars" => Some(PyObject::tuple(
                    code.freevars.iter().map(|s| PyObject::str_val(s.clone())).collect(),
                )),
                "co_cellvars" => Some(PyObject::tuple(
                    code.cellvars.iter().map(|s| PyObject::str_val(s.clone())).collect(),
                )),
                "co_consts" => {
                    use ferrython_bytecode::code::ConstantValue;
                    fn cv_to_obj(c: &ConstantValue) -> PyObjectRef {
                        match c {
                            ConstantValue::None => PyObject::none(),
                            ConstantValue::Bool(b) => PyObject::bool_val(*b),
                            ConstantValue::Integer(n) => PyObject::int(*n),
                            ConstantValue::BigInteger(n) => PyObject::big_int(n.as_ref().clone()),
                            ConstantValue::Float(f) => PyObject::float(*f),
                            ConstantValue::Complex { real, imag } => PyObject::complex(*real, *imag),
                            ConstantValue::Str(s) => PyObject::str_val(s.clone()),
                            ConstantValue::Bytes(b) => PyObject::bytes(b.clone()),
                            ConstantValue::Ellipsis => PyObject::ellipsis(),
                            ConstantValue::Code(co) => PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::clone(co))),
                            ConstantValue::Tuple(items) => {
                                PyObject::tuple(items.iter().map(|i| cv_to_obj(i)).collect())
                            }
                            ConstantValue::FrozenSet(_) => PyObject::str_val(CompactString::from("<frozenset>")),
                        }
                    }
                    Some(PyObject::tuple(
                        code.constants.iter().map(|c| cv_to_obj(c)).collect(),
                    ))
                }
                "__class__" => Some(PyObject::builtin_type(CompactString::from("code"))),
                _ => None,
            }
            PyObjectPayload::Cell(cell_ref) => match name {
                "cell_contents" => {
                    let guard = cell_ref.read();
                    match guard.as_ref() {
                        Some(v) => Some(v.clone()),
                        None => None, // empty cell → raise ValueError in Python
                    }
                }
                "__class__" => Some(PyObject::builtin_type(CompactString::from("cell"))),
                _ => None,
            }
            PyObjectPayload::Iterator(_) | PyObjectPayload::RangeIter(..) | PyObjectPayload::VecIter(_) | PyObjectPayload::RefIter { .. } => {
                match name {
                    "__next__" | "__iter__" | "__length_hint__" => {
                        Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)))
                        }))
                    }
                    "__class__" => Some(PyObject::builtin_type(CompactString::from("iterator"))),
                    _ => None,
                }
            }
            PyObjectPayload::BuiltinBoundMethod(_) => {
                match name {
                    "__class__" => Some(PyObject::builtin_type(CompactString::from("builtin_function_or_method"))),
                    "__name__" => {
                        if let PyObjectPayload::BuiltinBoundMethod(bbm) = &obj.payload {
                            Some(PyObject::str_val(bbm.method_name.clone()))
                        } else { None }
                    }
                    "__self__" => {
                        if let PyObjectPayload::BuiltinBoundMethod(bbm) = &obj.payload {
                            Some(bbm.receiver.clone())
                        } else { None }
                    }
                    _ => None,
                }
            }
            PyObjectPayload::NativeClosure(nc) => {
                match name {
                    "__name__" | "__qualname__" => Some(PyObject::str_val(nc.name.clone())),
                    "__class__" => Some(PyObject::builtin_type(CompactString::from("builtin_function_or_method"))),
                    "__doc__" => Some(PyObject::none()),
                    "__call__" => Some(obj.clone()),
                    "__get__" => {
                        let func = obj.clone();
                        Some(PyObject::native_closure("__get__", move |args| {
                            if args.is_empty() {
                                return Err(PyException::type_error("__get__ requires at least 1 argument"));
                            }
                            let instance = &args[0];
                            if matches!(&instance.payload, PyObjectPayload::None) {
                                return Ok(func.clone());
                            }
                            Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: instance.clone(),
                                    method: func.clone(),
                                }
                            }))
                        }))
                    }
                    _ => None,
                }
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
        "add_note" => {
            Some(PyObject::native_function("add_note", |args| {
                if args.len() < 2 {
                    return Err(crate::error::PyException::type_error(
                        "add_note() missing required argument: 'note'"));
                }
                let target = &args[0];
                let note = &args[1];
                if let PyObjectPayload::Instance(idata) = &target.payload {
                    let mut w = idata.attrs.write();
                    let notes = w.entry(CompactString::from("__notes__"))
                        .or_insert_with(|| PyObject::list(vec![]));
                    if let PyObjectPayload::List(list) = &notes.payload {
                        list.write().push(note.clone());
                    }
                } else if let PyObjectPayload::ExceptionInstance(ref ei) = target.payload {
                    let mut w = ei.ensure_attrs().write();
                    let notes = w.entry(CompactString::from("__notes__"))
                        .or_insert_with(|| PyObject::list(vec![]));
                    if let PyObjectPayload::List(list) = &notes.payload {
                        list.write().push(note.clone());
                    }
                }
                Ok(PyObject::none())
            }))
        }
        "with_traceback" => {
            let inst = _instance.clone();
            Some(PyObject::native_closure("with_traceback", move |args| {
                if !args.is_empty() {
                    if let PyObjectPayload::Instance(idata) = &inst.payload {
                        idata.attrs.write().insert(
                            CompactString::from("__traceback__"), args[0].clone());
                    } else if let PyObjectPayload::ExceptionInstance(ref ei) = inst.payload {
                        ei.ensure_attrs().write().insert(
                            CompactString::from("__traceback__"), args[0].clone());
                    }
                }
                Ok(inst.clone())
            }))
        }
        _ => None,
    }
}

