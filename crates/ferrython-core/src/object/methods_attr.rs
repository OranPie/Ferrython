//! Attribute lookup methods and descriptor protocol helpers.

use crate::error::PyException;
use crate::types::HashableKey;
use compact_str::CompactString;

use super::helpers::*;
use super::methods::PyObjectMethods;
use super::payload::*;

use super::methods_attr_helpers::*;

mod builtin_type;
mod callable_attrs;
mod class_attrs;
mod exception_attrs;
mod non_instance_attrs;
mod super_attrs;

/// Check if an attribute exists without cloning its value.
/// Used by hasattr() to avoid unnecessary Rc clone+drop cycles.
pub fn py_has_attr(obj: &PyObjectRef, name: &str) -> bool {
    match &obj.payload {
        PyObjectPayload::Instance(inst) => {
            let is_dunder =
                name.as_bytes().first() == Some(&b'_') && name.as_bytes().get(1) == Some(&b'_');
            if !is_dunder
                && inst.class_flags & (CLASS_FLAG_HAS_DESCRIPTORS | CLASS_FLAG_HAS_GETATTRIBUTE)
                    == 0
                && inst.dict_storage.is_none()
                && !inst.attrs.read().contains_key("__deque__")
            {
                // 1. Instance dict
                if inst.attrs.read().contains_key(name) {
                    return true;
                }
                // 2. Class MRO (no clone needed)
                if has_in_class_mro(&inst.class, name) {
                    return true;
                }
                // 2b. Builtin base type methods
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    if let Some(ref bt_name) = cd.builtin_base_name {
                        if super::helpers::resolve_builtin_type_method(bt_name.as_str(), name)
                            .is_some()
                        {
                            return true;
                        }
                        let known = match bt_name.as_str() {
                            "list" => matches!(
                                name,
                                "__init__"
                                    | "append"
                                    | "extend"
                                    | "insert"
                                    | "remove"
                                    | "pop"
                                    | "clear"
                                    | "reverse"
                                    | "sort"
                                    | "copy"
                                    | "count"
                                    | "index"
                            ),
                            "dict" => matches!(
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
                            ),
                            "set" => matches!(
                                name,
                                "add"
                                    | "remove"
                                    | "discard"
                                    | "pop"
                                    | "clear"
                                    | "copy"
                                    | "union"
                                    | "intersection"
                                    | "difference"
                                    | "symmetric_difference"
                                    | "update"
                                    | "intersection_update"
                                    | "difference_update"
                                    | "symmetric_difference_update"
                                    | "issubset"
                                    | "issuperset"
                                    | "isdisjoint"
                            ),
                            "str" => matches!(
                                name,
                                "upper"
                                    | "lower"
                                    | "strip"
                                    | "lstrip"
                                    | "rstrip"
                                    | "split"
                                    | "join"
                                    | "replace"
                                    | "startswith"
                                    | "endswith"
                                    | "find"
                                    | "count"
                                    | "format"
                                    | "encode"
                            ),
                            _ => false,
                        };
                        if known {
                            return true;
                        }
                    }
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
            // Standard path: fall through to get_attr.  Descriptor attributes
            // need their getter invoked so hasattr(x, "prop") matches Python.
            if let Some(v) = py_get_attr(obj, name) {
                if is_property_like(&v) {
                    if let Some(getter) = property_field(&v, "fget") {
                        if !matches!(&getter.payload, PyObjectPayload::None) {
                            return call_callable(&getter, &[obj.clone()]).is_ok();
                        }
                    }
                    return false;
                }
                true
            } else {
                false
            }
        }
        // For non-Instance types, delegate to get_attr (less hot path)
        _ => py_get_attr(obj, name).is_some(),
    }
}

pub(super) fn py_get_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::Instance(inst) => {
            if is_property_subclass_class(&inst.class) {
                match name {
                    "__doc__" => {
                        return Some(
                            inst.attrs
                                .read()
                                .get("__doc__")
                                .cloned()
                                .unwrap_or_else(PyObject::none),
                        );
                    }
                    "__isabstractmethod__" => {
                        for field in ["fget", "fset", "fdel"] {
                            if let Some(func) = inst.attrs.read().get(field).cloned() {
                                if !matches!(&func.payload, PyObjectPayload::None) {
                                    if let Some(flag) = func.get_attr("__isabstractmethod__") {
                                        if flag.is_truthy() {
                                            return Some(PyObject::bool_val(true));
                                        }
                                    }
                                }
                            }
                        }
                        return Some(PyObject::bool_val(false));
                    }
                    "fget" | "fset" | "fdel" => {
                        return Some(
                            inst.attrs
                                .read()
                                .get(name)
                                .cloned()
                                .unwrap_or_else(PyObject::none),
                        );
                    }
                    "setter" | "getter" | "deleter" => {
                        return Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(
                                super::constructors::alloc_bbm_box(
                                    obj.clone(),
                                    CompactString::from(name),
                                ),
                            ),
                        }));
                    }
                    "__get__" => {
                        if let Some(method) = resolve_builtin_type_method("property", "__get__") {
                            return Some(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj.clone(),
                                    method,
                                },
                            }));
                        }
                    }
                    _ => {}
                }
            }
            // ── ULTRA-FAST PATH ──
            // For simple instances: no descriptors, no __getattribute__, no
            // dict_storage, and name is NOT a dunder. Skips the expensive
            // __class__-override attr-map lookup that the standard path does
            // on every single call.
            let is_dunder =
                name.as_bytes().first() == Some(&b'_') && name.as_bytes().get(1) == Some(&b'_');
            if !is_dunder
                && inst.class_flags & (CLASS_FLAG_HAS_DESCRIPTORS | CLASS_FLAG_HAS_GETATTRIBUTE)
                    == 0
                && inst.dict_storage.is_none()
                && !inst.attrs.read().contains_key("__deque__")
            {
                // 1. Instance dict first (data attributes)
                if let Some(v) = inst.attrs.read().get(name) {
                    return Some(v.clone());
                }
                if let Some(v) = ast_constant_alias_attr(inst, name) {
                    return Some(v);
                }
                if inst.class.get_attr("__namedtuple__").is_some() {
                    if let Some(fields) = inst.class.get_attr("_fields") {
                        if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                            if let Some((idx, _)) = field_names
                                .iter()
                                .enumerate()
                                .find(|(_, field)| field.py_to_string() == name)
                            {
                                if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                                    if let PyObjectPayload::Tuple(items) = &tup.payload {
                                        if let Some(val) = items.get(idx) {
                                            return Some(val.clone());
                                        }
                                    }
                                }
                                if let Some(v) = inst.attrs.read().get(name) {
                                    return Some(v.clone());
                                }
                            }
                        }
                    }
                }
                // 2. Class + MRO via vtable/cache (methods & class attrs)
                if let Some(v) = lookup_in_class_mro(&inst.class, name) {
                    return Some(wrap_class_attr_for_instance(obj, inst, name, v));
                }
                // 2b. Builtin base type methods (list.append, tuple.__len__, etc.)
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    if let Some(ref bt_name) = cd.builtin_base_name {
                        if let Some(resolved) =
                            super::helpers::resolve_builtin_type_method(bt_name.as_str(), name)
                        {
                            return Some(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj.clone(),
                                    method: resolved,
                                },
                            }));
                        }
                        // Known builtin methods dispatched via __builtin_value__
                        let known = match bt_name.as_str() {
                            "list" => matches!(
                                name,
                                "append"
                                    | "extend"
                                    | "insert"
                                    | "remove"
                                    | "pop"
                                    | "clear"
                                    | "reverse"
                                    | "sort"
                                    | "copy"
                                    | "count"
                                    | "index"
                            ),
                            "dict" => matches!(
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
                            ),
                            "set" => matches!(
                                name,
                                "add"
                                    | "remove"
                                    | "discard"
                                    | "pop"
                                    | "clear"
                                    | "copy"
                                    | "update"
                                    | "union"
                                    | "intersection"
                                    | "difference"
                                    | "symmetric_difference"
                                    | "issubset"
                                    | "issuperset"
                            ),
                            "str" => matches!(
                                name,
                                "upper"
                                    | "lower"
                                    | "strip"
                                    | "lstrip"
                                    | "rstrip"
                                    | "split"
                                    | "join"
                                    | "replace"
                                    | "startswith"
                                    | "endswith"
                                    | "find"
                                    | "count"
                                    | "format"
                                    | "encode"
                            ),
                            _ => false,
                        };
                        if known {
                            return Some(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BuiltinBoundMethod(
                                    super::constructors::alloc_bbm_box(
                                        obj.clone(),
                                        CompactString::from(name),
                                    ),
                                ),
                            }));
                        }
                    }
                }
                // Builtin base subclass: delegate non-dunder attr lookups (e.g. .real, .imag)
                // to the underlying __builtin_value__ when all else fails.
                if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                    if let Some(result) = py_get_attr(&val, name) {
                        return Some(result);
                    }
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
                if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                    if !inst.attrs.read().contains_key("__weakref_ref__") {
                        if let Ok(referent) = call_callable(&target_fn, &[]) {
                            return py_get_attr(&referent, name);
                        }
                    }
                }
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
                if inst.class.get_attr("__namedtuple__").is_some() {
                    let mut filtered = new_fx_hashkey_map();
                    {
                        let attrs = inst.attrs.read();
                        for (k, v) in attrs.iter() {
                            if k.as_str() != "_tuple" {
                                filtered.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                        }
                    }
                    return Some(PyObject::dict(filtered));
                }
                return Some(PyObject::wrap(PyObjectPayload::InstanceDict(
                    inst.attrs.clone(),
                )));
            }
            // Dict subclass: intercept dict method lookups
            if inst.dict_storage.is_some() {
                if let Some(result) = instance_builtin_method(obj, inst, name) {
                    return Some(result);
                }
            }
            // Deque marker instances keep compatibility state in marker attrs.
            // Prefer the marker dispatcher over constructor-installed closures
            // so reinitialization and maxlen changes use the same backing list.
            if inst.attrs.read().contains_key("__deque__") {
                if let Some(result) = instance_builtin_method(obj, inst, name) {
                    return Some(result);
                }
            }

            // Resolve effective class: honour `__class__` override (if any)
            let effective_class = inst
                .attrs
                .read()
                .get("__class__")
                .filter(|c| matches!(c.payload, PyObjectPayload::Class(_)))
                .cloned()
                .unwrap_or_else(|| inst.class.clone());

            if inst.class.get_attr("__namedtuple__").is_some() {
                if let Some(fields) = inst.class.get_attr("_fields") {
                    if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                        if let Some((idx, _)) = field_names
                            .iter()
                            .enumerate()
                            .find(|(_, field)| field.py_to_string() == name)
                        {
                            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                                if let PyObjectPayload::Tuple(items) = &tup.payload {
                                    if let Some(val) = items.get(idx) {
                                        return Some(val.clone());
                                    }
                                }
                            }
                            if let Some(v) = inst.attrs.read().get(name) {
                                return Some(v.clone());
                            }
                        }
                    }
                }
            }

            // Determine if this class has data descriptors
            let class_has_descriptors = inst.class_flags & CLASS_FLAG_HAS_DESCRIPTORS != 0;

            if !class_has_descriptors {
                // ── FAST PATH: no data descriptors ──
                // Check instance dict first, then class MRO
                if let Some(v) = inst.attrs.read().get(name) {
                    return Some(v.clone());
                }
                if let Some(v) = ast_constant_alias_attr(inst, name) {
                    return Some(v);
                }
                if let Some(v) = lookup_in_class_mro(&effective_class, name) {
                    return Some(wrap_class_attr_for_instance(obj, inst, name, v));
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
                if let Some(v) = inst.attrs.read().get(name) {
                    return Some(v.clone());
                }
                if let Some(v) = ast_constant_alias_attr(inst, name) {
                    return Some(v);
                }
                // 3. Non-data descriptors and other class attrs
                if let Some(v) = class_attr {
                    return Some(wrap_class_attr_for_instance(obj, inst, name, v));
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
            if name == "__eq__" {
                let inst_obj = obj.clone();
                return Some(PyObject::native_closure(
                    "__eq__",
                    move |args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Err(PyException::type_error("__eq__ requires an argument"));
                        }
                        if PyObjectRef::ptr_eq(&inst_obj, &args[0]) {
                            Ok(PyObject::bool_val(true))
                        } else {
                            Ok(PyObject::not_implemented())
                        }
                    },
                ));
            }
            if name == "__ne__" {
                let inst_obj = obj.clone();
                return Some(PyObject::native_closure(
                    "__ne__",
                    move |args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Err(PyException::type_error("__ne__ requires an argument"));
                        }
                        if let Some(eq_method) = inst_obj.get_attr("__eq__") {
                            let result = call_callable(&eq_method, &[args[0].clone()])?;
                            if matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                return Ok(PyObject::not_implemented());
                            }
                            return Ok(PyObject::bool_val(!result.is_truthy()));
                        }
                        Ok(PyObject::not_implemented())
                    },
                ));
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
        _ => non_instance_attrs::non_instance_attr(obj, name),
    }
}
