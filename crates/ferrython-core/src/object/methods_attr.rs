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
        PyObjectPayload::Class(cd) => class_attrs::class_attr(obj, cd, name),
        PyObjectPayload::Module(m) => {
            if name == "__class__" {
                return Some(PyObject::builtin_type(CompactString::from("module")));
            }
            if name == "__dict__" {
                Some(PyObject::wrap(PyObjectPayload::InstanceDict(
                    m.attrs.clone(),
                )))
            } else {
                m.attrs.read().get(name).cloned()
            }
        }
        PyObjectPayload::Slice(sd) => match name {
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
                            "slice.indices() requires a length argument",
                        ));
                    }
                    let length = index_to_i64(&args[0]).map_err(|_| {
                        crate::error::PyException::type_error("length must be an integer")
                    })?;
                    if length < 0 {
                        return Err(crate::error::PyException::value_error(
                            "length should not be negative",
                        ));
                    }
                    let (start_val, stop_val, step_val) =
                        resolve_slice_i128(&s_start, &s_stop, &s_step, length as i128)?;
                    let int_obj = |value: i128| {
                        i64::try_from(value)
                            .map(PyObject::int)
                            .unwrap_or_else(|_| PyObject::big_int(value.into()))
                    };
                    Ok(PyObject::tuple(vec![
                        int_obj(start_val),
                        int_obj(stop_val),
                        int_obj(step_val),
                    ]))
                }))
            }
            _ => None,
        },
        PyObjectPayload::Complex { real, imag } => match name {
            "real" => Some(PyObject::float(*real)),
            "imag" => Some(PyObject::float(*imag)),
            "__class__" => Some(PyObject::builtin_type(CompactString::from("complex"))),
            "conjugate" => Some(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(
                    obj.clone(),
                    CompactString::from("conjugate"),
                )),
            })),
            "__abs__" | "__neg__" | "__pos__" | "__bool__" | "__repr__" | "__str__"
            | "__hash__" | "__format__" | "__eq__" | "__ne__" | "__lt__" | "__le__" | "__gt__"
            | "__ge__" | "__add__" | "__sub__" | "__mul__" | "__truediv__" | "__floordiv__"
            | "__mod__" | "__pow__" | "__divmod__" | "__radd__" | "__rsub__" | "__rmul__"
            | "__rtruediv__" | "__rfloordiv__" | "__rmod__" | "__rpow__" | "__complex__"
            | "__getnewargs__" => Some(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(
                    obj.clone(),
                    CompactString::from(name),
                )),
            })),
            _ => None,
        },
        PyObjectPayload::BuiltinType(n) => builtin_type::builtin_type_attr(obj, n, name),
        PyObjectPayload::Property(pd) => {
            match name {
                "__doc__" => {
                    return Some(pd.doc.read().clone().unwrap_or_else(PyObject::none));
                }
                "__isabstractmethod__" => {
                    for func in [&pd.fget, &pd.fset, &pd.fdel].into_iter().flatten() {
                        if let Some(flag) = func.get_attr("__isabstractmethod__") {
                            if flag.is_truthy() {
                                return Some(PyObject::bool_val(true));
                            }
                        }
                    }
                    return Some(PyObject::bool_val(false));
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
                        payload: PyObjectPayload::BuiltinBoundMethod(
                            super::constructors::alloc_bbm_box(
                                obj.clone(),
                                CompactString::from(name),
                            ),
                        ),
                    }))
                }
                _ => None,
            }
        }
        PyObjectPayload::Partial(pd) => match name {
            "func" => Some(pd.func.clone()),
            "args" => Some(PyObject::tuple(pd.args.clone())),
            "keywords" => {
                let mut map = new_fx_hashkey_map();
                for (k, v) in &pd.kwargs {
                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                }
                Some(PyObject::dict(map))
            }
            _ => None,
        },
        PyObjectPayload::ExceptionType(kind) => {
            exception_attrs::exception_type_attr(obj, *kind, name)
        }
        PyObjectPayload::ExceptionInstance(ei) => {
            exception_attrs::exception_instance_attr(obj, ei, name)
        }
        PyObjectPayload::Function(f) => callable_attrs::function_attr(obj, f, name),
        PyObjectPayload::NativeFunction(nf) => callable_attrs::native_function_attr(obj, nf, name),
        PyObjectPayload::BuiltinFunction(fname) => {
            callable_attrs::builtin_function_attr(obj, fname, name)
        }
        PyObjectPayload::ClassMethod(func) => callable_attrs::classmethod_attr(func, name),
        PyObjectPayload::StaticMethod(func) => callable_attrs::staticmethod_attr(func, name),
        PyObjectPayload::BoundMethod { receiver, method } => {
            callable_attrs::bound_method_attr(receiver, method, name)
        }
        // Int property-like attributes (return values, not bound methods)
        PyObjectPayload::Int(_n) => match name {
            "real" | "numerator" => Some(PyObject::wrap(obj.payload.clone())),
            "imag" => Some(PyObject::int(0)),
            "denominator" => Some(PyObject::int(1)),
            "__class__" => Some(PyObject::builtin_type(CompactString::from("int"))),
            "bit_length" | "bit_count" | "to_bytes" | "conjugate" | "__abs__" | "__int__"
            | "__float__" | "__index__" | "__bool__" | "__neg__" | "__pos__" | "__invert__"
            | "__repr__" | "__str__" | "__hash__" | "__format__" | "__eq__" | "__ne__"
            | "__lt__" | "__le__" | "__gt__" | "__ge__" | "__add__" | "__sub__" | "__mul__"
            | "__truediv__" | "__floordiv__" | "__mod__" | "__pow__" | "__divmod__"
            | "__lshift__" | "__rshift__" | "__and__" | "__or__" | "__xor__" | "__ceil__"
            | "__floor__" | "__round__" | "__trunc__" | "__sizeof__" | "as_integer_ratio" => {
                Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }))
            }
            _ => None,
        },
        // Float property-like attributes
        PyObjectPayload::Float(f) => match name {
            "real" => Some(PyObject::float(*f)),
            "imag" => Some(PyObject::float(0.0)),
            "__class__" => Some(PyObject::builtin_type(CompactString::from("float"))),
            "is_integer" | "conjugate" | "hex" | "__abs__" | "__int__" | "__float__"
            | "__bool__" | "__index__" | "__neg__" | "__pos__" | "__repr__" | "__str__"
            | "__hash__" | "__format__" | "__eq__" | "__ne__" | "__lt__" | "__le__" | "__gt__"
            | "__ge__" | "__add__" | "__sub__" | "__mul__" | "__truediv__" | "__floordiv__"
            | "__mod__" | "__pow__" | "__divmod__" | "__round__" | "__ceil__" | "__floor__"
            | "__trunc__" | "__sizeof__" | "as_integer_ratio" | "fromhex" => {
                Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }))
            }
            _ => None,
        },
        // Bool property-like attributes (bool is subtype of int)
        PyObjectPayload::Bool(b) => match name {
            "real" | "numerator" => Some(PyObject::int(if *b { 1 } else { 0 })),
            "imag" => Some(PyObject::int(0)),
            "denominator" => Some(PyObject::int(1)),
            "__class__" => Some(PyObject::builtin_type(CompactString::from("bool"))),
            "bit_length" | "bit_count" | "to_bytes" | "conjugate" | "__abs__" | "__int__"
            | "__float__" | "__index__" | "__bool__" | "__repr__" | "__str__" | "__hash__"
            | "__format__" | "__sizeof__" => Some(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(
                    obj.clone(),
                    CompactString::from(name),
                )),
            })),
            _ => None,
        },
        // Built-in type methods — return bound method for KNOWN methods only
        PyObjectPayload::Range(rd) => match name {
            "start" => Some(
                rd.start_obj
                    .clone()
                    .unwrap_or_else(|| PyObject::int(rd.start)),
            ),
            "stop" => Some(
                rd.stop_obj
                    .clone()
                    .unwrap_or_else(|| PyObject::int(rd.stop)),
            ),
            "step" => Some(
                rd.step_obj
                    .clone()
                    .unwrap_or_else(|| PyObject::int(rd.step)),
            ),
            "__class__" => Some(PyObject::builtin_type(CompactString::from("range"))),
            "count" | "index" | "__contains__" | "__iter__" | "__reversed__" | "__len__" => {
                Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }))
            }
            "__getitem__" => Some(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(
                    obj.clone(),
                    CompactString::from(name),
                )),
            })),
            _ => None,
        },
        PyObjectPayload::Str(_) => {
            if name == "__class__" {
                return Some(PyObject::builtin_type(CompactString::from("str")));
            }
            if matches!(
                name,
                "upper"
                    | "lower"
                    | "strip"
                    | "lstrip"
                    | "rstrip"
                    | "split"
                    | "rsplit"
                    | "join"
                    | "replace"
                    | "find"
                    | "rfind"
                    | "index"
                    | "rindex"
                    | "count"
                    | "startswith"
                    | "endswith"
                    | "isdigit"
                    | "isalpha"
                    | "isalnum"
                    | "isspace"
                    | "isupper"
                    | "islower"
                    | "istitle"
                    | "isprintable"
                    | "isidentifier"
                    | "isascii"
                    | "isdecimal"
                    | "isnumeric"
                    | "title"
                    | "capitalize"
                    | "swapcase"
                    | "center"
                    | "ljust"
                    | "rjust"
                    | "zfill"
                    | "expandtabs"
                    | "encode"
                    | "partition"
                    | "rpartition"
                    | "casefold"
                    | "removeprefix"
                    | "removesuffix"
                    | "splitlines"
                    | "format"
                    | "format_map"
                    | "translate"
                    | "maketrans"
                    | "__len__"
                    | "__contains__"
                    | "__iter__"
                    | "__getitem__"
                    | "__hash__"
                    | "__eq__"
                    | "__ne__"
                    | "__lt__"
                    | "__le__"
                    | "__gt__"
                    | "__ge__"
                    | "__repr__"
                    | "__str__"
                    | "__format__"
                    | "__add__"
                    | "__mul__"
                    | "__rmul__"
                    | "__mod__"
                    | "__bool__"
                    | "__sizeof__"
            ) {
                return Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }));
            }
            None
        }
        PyObjectPayload::List(_) => {
            if name == "__class__" {
                return Some(PyObject::builtin_type(CompactString::from("list")));
            }
            if matches!(
                name,
                "append"
                    | "extend"
                    | "insert"
                    | "pop"
                    | "remove"
                    | "reverse"
                    | "sort"
                    | "clear"
                    | "copy"
                    | "count"
                    | "index"
                    | "__len__"
                    | "__contains__"
                    | "__iter__"
                    | "__getitem__"
                    | "__setitem__"
                    | "__delitem__"
                    | "__eq__"
                    | "__ne__"
                    | "__lt__"
                    | "__le__"
                    | "__gt__"
                    | "__ge__"
                    | "__repr__"
                    | "__str__"
                    | "__add__"
                    | "__mul__"
                    | "__rmul__"
                    | "__iadd__"
                    | "__imul__"
                    | "__reversed__"
                    | "__bool__"
                    | "__hash__"
                    | "__sizeof__"
            ) {
                return Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }));
            }
            None
        }
        PyObjectPayload::Dict(_)
        | PyObjectPayload::InstanceDict(_)
        | PyObjectPayload::MappingProxy(_) => {
            if name == "__class__" {
                let type_name = obj.type_name();
                return Some(PyObject::builtin_type(CompactString::from(type_name)));
            }
            if matches!(
                name,
                "keys"
                    | "values"
                    | "items"
                    | "get"
                    | "copy"
                    | "update"
                    | "subtract"
                    | "pop"
                    | "setdefault"
                    | "clear"
                    | "popitem"
                    | "most_common"
                    | "elements"
                    | "move_to_end"
                    | "__len__"
                    | "__contains__"
                    | "__iter__"
                    | "__getitem__"
                    | "__setitem__"
                    | "__delitem__"
                    | "__eq__"
                    | "__ne__"
                    | "__repr__"
                    | "__str__"
                    | "__or__"
                    | "__ior__"
                    | "__bool__"
                    | "__hash__"
                    | "__sizeof__"
            ) {
                return Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }));
            }
            None
        }
        PyObjectPayload::Tuple(_) => {
            if name == "__class__" {
                return Some(PyObject::builtin_type(CompactString::from("tuple")));
            }
            if matches!(
                name,
                "count"
                    | "index"
                    | "__len__"
                    | "__contains__"
                    | "__iter__"
                    | "__getitem__"
                    | "__hash__"
                    | "__eq__"
                    | "__ne__"
                    | "__lt__"
                    | "__le__"
                    | "__gt__"
                    | "__ge__"
                    | "__repr__"
                    | "__str__"
                    | "__add__"
                    | "__mul__"
                    | "__rmul__"
                    | "__bool__"
                    | "__sizeof__"
            ) {
                return Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }));
            }
            None
        }
        PyObjectPayload::Set(_) => {
            if name == "__class__" {
                return Some(PyObject::builtin_type(CompactString::from("set")));
            }
            if matches!(
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
                    | "isdisjoint"
                    | "intersection_update"
                    | "difference_update"
                    | "symmetric_difference_update"
                    | "__init__"
                    | "__len__"
                    | "__contains__"
                    | "__iter__"
                    | "__or__"
                    | "__and__"
                    | "__sub__"
                    | "__xor__"
                    | "__eq__"
                    | "__ne__"
                    | "__lt__"
                    | "__le__"
                    | "__gt__"
                    | "__ge__"
                    | "__repr__"
                    | "__str__"
                    | "__bool__"
                    | "__hash__"
                    | "__sizeof__"
            ) {
                return Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }));
            }
            None
        }
        PyObjectPayload::FrozenSet(_) => {
            if name == "__class__" {
                return Some(PyObject::builtin_type(CompactString::from("frozenset")));
            }
            if matches!(
                name,
                "copy"
                    | "union"
                    | "intersection"
                    | "difference"
                    | "symmetric_difference"
                    | "issubset"
                    | "issuperset"
                    | "isdisjoint"
                    | "__init__"
                    | "__len__"
                    | "__contains__"
                    | "__iter__"
                    | "__or__"
                    | "__and__"
                    | "__sub__"
                    | "__xor__"
                    | "__eq__"
                    | "__ne__"
                    | "__hash__"
                    | "__repr__"
                    | "__str__"
                    | "__bool__"
                    | "__sizeof__"
            ) {
                return Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }));
            }
            None
        }
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => {
            if name == "__class__" {
                let type_name = obj.type_name();
                return Some(PyObject::builtin_type(CompactString::from(type_name)));
            }
            if matches!(
                name,
                "decode"
                    | "hex"
                    | "count"
                    | "find"
                    | "rfind"
                    | "index"
                    | "rindex"
                    | "startswith"
                    | "endswith"
                    | "upper"
                    | "lower"
                    | "strip"
                    | "lstrip"
                    | "rstrip"
                    | "split"
                    | "join"
                    | "replace"
                    | "isdigit"
                    | "isalpha"
                    | "isalnum"
                    | "isspace"
                    | "islower"
                    | "isupper"
                    | "istitle"
                    | "swapcase"
                    | "title"
                    | "capitalize"
                    | "center"
                    | "ljust"
                    | "rjust"
                    | "zfill"
                    | "expandtabs"
                    | "partition"
                    | "rpartition"
                    | "removeprefix"
                    | "removesuffix"
                    | "rsplit"
                    | "splitlines"
                    | "translate"
                    | "tobytes"
                    | "tolist"
                    | "release"
                    | "append"
                    | "extend"
                    | "pop"
                    | "insert"
                    | "clear"
                    | "reverse"
                    | "copy"
                    | "__len__"
                    | "__contains__"
                    | "__iter__"
                    | "__getitem__"
                    | "__setitem__"
                    | "__eq__"
                    | "__ne__"
                    | "__repr__"
                    | "__str__"
                    | "__add__"
                    | "__mul__"
                    | "__rmul__"
                    | "__rmod__"
                    | "__bool__"
                    | "__hash__"
                    | "__sizeof__"
            ) {
                return Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }));
            }
            None
        }
        PyObjectPayload::None => match name {
            "__class__" => Some(PyObject::builtin_type(CompactString::from("NoneType"))),
            "__bool__" | "__repr__" | "__str__" | "__hash__" | "__eq__" | "__ne__" => {
                Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                }))
            }
            _ => None,
        },
        PyObjectPayload::Generator(_) => {
            match name {
                // Generator protocol: send, throw, close, __next__, __iter__
                "send" | "throw" | "close" | "__next__" => Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                })),
                // Context manager protocol: generators from @contextmanager
                // __enter__ calls next(gen), __exit__ calls gen.close()/gen.throw()
                "__enter__" | "__exit__" => Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                })),
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
                "send" | "throw" | "close" => Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                })),
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
                "send" | "throw" | "close" => Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod(
                        super::constructors::alloc_bbm_box(obj.clone(), CompactString::from(name)),
                    ),
                })),
                // Async iteration protocol — __aiter__ returns self when called
                "__aiter__" | "__anext__" | "asend" | "athrow" | "aclose" => {
                    Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(
                            super::constructors::alloc_bbm_box(
                                obj.clone(),
                                CompactString::from(name),
                            ),
                        ),
                    }))
                }
                "ag_frame" | "ag_code" => Some(PyObject::none()),
                "ag_running" => Some(PyObject::bool_val(false)),
                "ag_await" => Some(PyObject::none()),
                _ => None,
            }
        }
        // AsyncGenAwaitable is an awaitable: __await__ returns self, send/throw/close delegate
        PyObjectPayload::AsyncGenAwaitable { .. } => match name {
            "__await__" => Some(obj.clone()),
            "send" | "throw" | "close" => Some(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(
                    obj.clone(),
                    CompactString::from(name),
                )),
            })),
            _ => None,
        },
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
                        pickle_args: None,
                        func: std::rc::Rc::new(move |args: &[PyObjectRef]| {
                            if args.is_empty() {
                                return Err(PyException::type_error(
                                    "__getattribute__() requires at least 1 argument",
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
                                "'super' object has no attribute '{}'",
                                attr_name
                            )))
                        }),
                    })),
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
                                if matches!(
                                    &v.payload,
                                    PyObjectPayload::Function(_)
                                        | PyObjectPayload::NativeClosure(_)
                                        | PyObjectPayload::NativeFunction(_)
                                ) {
                                    return Some(PyObjectRef::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: instance.clone(),
                                            method: v.clone(),
                                        },
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
                                        },
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
                            if let Some(resolved) =
                                exception_attrs::resolve_exception_type_method(name, instance)
                            {
                                // Bind to instance so obj is prepended
                                return Some(PyObjectRef::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod {
                                        receiver: instance.clone(),
                                        method: resolved,
                                    },
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
                                        method: PyObject::native_function("__type_call__", |_| {
                                            Ok(PyObject::none())
                                        }),
                                    },
                                }));
                            }
                            if let Some(resolved) =
                                resolve_builtin_type_method(bt_name.as_str(), name)
                            {
                                // __new__ is a static method: don't bind obj
                                if name == "__new__" {
                                    return Some(resolved);
                                }
                                // Wrap as BoundMethod so obj is prepended
                                return Some(PyObjectRef::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod {
                                        receiver: instance.clone(),
                                        method: resolved,
                                    },
                                }));
                            }
                            // For builtin type methods (list.append, dict.update, etc.)
                            // that aren't in resolve_builtin_type_method, return a
                            // BuiltinBoundMethod that the VM dispatches via __builtin_value__
                            let known_methods = match bt_name.as_str() {
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
                                        | "__len__"
                                        | "__iter__"
                                        | "__contains__"
                                        | "__getitem__"
                                        | "__setitem__"
                                        | "__delitem__"
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
                                        | "__len__"
                                        | "__iter__"
                                        | "__contains__"
                                        | "__getitem__"
                                        | "__setitem__"
                                        | "__delitem__"
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
                                        | "intersection_update"
                                        | "difference_update"
                                        | "symmetric_difference_update"
                                        | "union"
                                        | "intersection"
                                        | "difference"
                                        | "symmetric_difference"
                                        | "issubset"
                                        | "issuperset"
                                        | "__len__"
                                        | "__iter__"
                                        | "__contains__"
                                ),
                                "str" => matches!(
                                    name,
                                    "upper"
                                        | "lower"
                                        | "strip"
                                        | "lstrip"
                                        | "rstrip"
                                        | "split"
                                        | "rsplit"
                                        | "join"
                                        | "replace"
                                        | "startswith"
                                        | "endswith"
                                        | "find"
                                        | "rfind"
                                        | "index"
                                        | "rindex"
                                        | "count"
                                        | "encode"
                                        | "format"
                                        | "center"
                                        | "ljust"
                                        | "rjust"
                                        | "zfill"
                                        | "title"
                                        | "capitalize"
                                        | "swapcase"
                                        | "partition"
                                        | "rpartition"
                                        | "expandtabs"
                                        | "__len__"
                                        | "__iter__"
                                        | "__contains__"
                                        | "__getitem__"
                                ),
                                "int" => matches!(
                                    name,
                                    "bit_length"
                                        | "to_bytes"
                                        | "from_bytes"
                                        | "__int__"
                                        | "__float__"
                                        | "__index__"
                                ),
                                "tuple" => matches!(
                                    name,
                                    "count"
                                        | "index"
                                        | "__len__"
                                        | "__iter__"
                                        | "__contains__"
                                        | "__getitem__"
                                ),
                                _ => false,
                            };
                            if known_methods {
                                return Some(PyObjectRef::new(PyObject {
                                    payload: PyObjectPayload::BuiltinBoundMethod(
                                        super::constructors::alloc_bbm_box(
                                            instance.clone(),
                                            CompactString::from(name),
                                        ),
                                    ),
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
                                                },
                                            }));
                                        }
                                        if let PyObjectPayload::ClassMethod(func) = &v.payload {
                                            let bound_cls = match &instance.payload {
                                                PyObjectPayload::Instance(inst) => {
                                                    inst.class.clone()
                                                }
                                                _ => instance.clone(),
                                            };
                                            return Some(PyObjectRef::new(PyObject {
                                                payload: PyObjectPayload::BoundMethod {
                                                    receiver: bound_cls,
                                                    method: func.clone(),
                                                },
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
                                    if let Some(resolved) =
                                        resolve_builtin_type_method(bt_name.as_str(), name)
                                    {
                                        return Some(PyObjectRef::new(PyObject {
                                            payload: PyObjectPayload::BoundMethod {
                                                receiver: instance.clone(),
                                                method: resolved,
                                            },
                                        }));
                                    }
                                }
                                // Check ExceptionType bases
                                if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                                    if let Some(resolved) =
                                        exception_attrs::resolve_exception_type_method(
                                            name, instance,
                                        )
                                    {
                                        return Some(PyObjectRef::new(PyObject {
                                            payload: PyObjectPayload::BoundMethod {
                                                receiver: instance.clone(),
                                                method: resolved,
                                            },
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    // Builtin __new__: object.__new__(cls) creates a new instance
                    if name == "__new__" {
                        return Some(PyObject::native_function("__new__", |args| {
                            if args.is_empty() {
                                return Err(PyException::type_error("__new__ requires cls"));
                            }
                            Ok(PyObject::instance(args[0].clone()))
                        }));
                    }
                    // Fallback: check instance attrs for methods installed by
                    // parent __init__ (e.g., BytesIO.__init__ installs write/read
                    // as NativeClosure on the instance, not in the class namespace)
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        if let Some(v) = inst.attrs.read().get(name).cloned() {
                            if matches!(
                                &v.payload,
                                PyObjectPayload::NativeClosure(_)
                                    | PyObjectPayload::NativeFunction(_)
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
                        return Some(PyObject::native_closure(
                            "__setattr__",
                            move |args: &[PyObjectRef]| {
                                if args.len() < 2 {
                                    return Err(PyException::type_error(
                                        "__setattr__ requires name and value",
                                    ));
                                }
                                let attr_name = args[0].py_to_string();
                                let value = args[1].clone();
                                if let PyObjectPayload::Instance(data) = &inst.payload {
                                    data.attrs
                                        .write()
                                        .insert(CompactString::from(attr_name.as_str()), value);
                                }
                                Ok(PyObject::none())
                            },
                        ));
                    }
                    // Builtin __delattr__: object.__delattr__(self, name)
                    if name == "__delattr__" {
                        let inst = instance.clone();
                        return Some(PyObject::native_closure(
                            "__delattr__",
                            move |args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "__delattr__ requires name argument",
                                    ));
                                }
                                let attr_name = args[0].py_to_string();
                                if let PyObjectPayload::Instance(data) = &inst.payload {
                                    let removed =
                                        data.attrs.write().shift_remove(attr_name.as_str());
                                    if removed.is_none() {
                                        return Err(PyException::attribute_error(format!(
                                            "'{}' object has no attribute '{}'",
                                            data.class.py_to_string(),
                                            attr_name
                                        )));
                                    }
                                }
                                Ok(PyObject::none())
                            },
                        ));
                    }
                    // Builtin __eq__: object.__eq__ is identity comparison
                    if name == "__eq__" {
                        let inst = instance.clone();
                        return Some(PyObject::native_closure(
                            "__eq__",
                            move |args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "__eq__ requires an argument",
                                    ));
                                }
                                Ok(PyObject::bool_val(PyObjectRef::ptr_eq(&inst, &args[0])))
                            },
                        ));
                    }
                    // Builtin __ne__: object.__ne__ is negated identity
                    if name == "__ne__" {
                        let inst = instance.clone();
                        return Some(PyObject::native_closure(
                            "__ne__",
                            move |args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "__ne__ requires an argument",
                                    ));
                                }
                                Ok(PyObject::bool_val(!PyObjectRef::ptr_eq(&inst, &args[0])))
                            },
                        ));
                    }
                    // Builtin __repr__ / __str__: default object repr
                    if name == "__repr__" || name == "__str__" {
                        let inst = instance.clone();
                        return Some(PyObject::native_closure(
                            name,
                            move |_args: &[PyObjectRef]| {
                                let cls_name =
                                    if let PyObjectPayload::Instance(data) = &inst.payload {
                                        data.class.py_to_string()
                                    } else {
                                        "object".into()
                                    };
                                Ok(PyObject::str_val(CompactString::from(format!(
                                    "<{} object>",
                                    cls_name
                                ))))
                            },
                        ));
                    }
                    // Builtin __hash__: default hash from object id
                    if name == "__hash__" {
                        let inst = instance.clone();
                        return Some(PyObject::native_closure(
                            "__hash__",
                            move |_args: &[PyObjectRef]| {
                                let ptr = PyObjectRef::as_ptr(&inst) as usize;
                                Ok(PyObject::int(ptr as i64))
                            },
                        ));
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
            "co_code" => Some(PyObject::bytes(code_object_co_code(code))),
            "co_lnotab" => Some(PyObject::bytes(Vec::new())),
            "co_varnames" => Some(PyObject::tuple(
                code.varnames
                    .iter()
                    .map(|s| PyObject::str_val(s.clone()))
                    .collect(),
            )),
            "co_names" => Some(PyObject::tuple(
                code.names
                    .iter()
                    .map(|s| PyObject::str_val(s.clone()))
                    .collect(),
            )),
            "co_freevars" => Some(PyObject::tuple(
                code.freevars
                    .iter()
                    .map(|s| PyObject::str_val(s.clone()))
                    .collect(),
            )),
            "co_cellvars" => Some(PyObject::tuple(
                code.cellvars
                    .iter()
                    .map(|s| PyObject::str_val(s.clone()))
                    .collect(),
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
                        ConstantValue::Code(co) => {
                            PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::clone(co)))
                        }
                        ConstantValue::Tuple(items) => {
                            PyObject::tuple(items.iter().map(|i| cv_to_obj(i)).collect())
                        }
                        ConstantValue::FrozenSet(items) => {
                            let mut set = crate::object::new_fx_hashkey_map();
                            for item in items {
                                let obj = cv_to_obj(item);
                                if let Ok(key) = obj.to_hashable_key() {
                                    set.insert(key, obj);
                                }
                            }
                            PyObject::frozenset(set)
                        }
                    }
                }
                Some(PyObject::tuple(
                    code.constants.iter().map(|c| cv_to_obj(c)).collect(),
                ))
            }
            "__class__" => Some(PyObject::builtin_type(CompactString::from("code"))),
            _ => None,
        },
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
        },
        PyObjectPayload::Iterator(_)
        | PyObjectPayload::RangeIter(..)
        | PyObjectPayload::VecIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. } => match name {
            "__next__" | "__iter__" | "__length_hint__" => Some(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(
                    obj.clone(),
                    CompactString::from(name),
                )),
            })),
            "__setstate__" if iterator_supports_setstate(obj) => Some(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BuiltinBoundMethod(super::constructors::alloc_bbm_box(
                    obj.clone(),
                    CompactString::from(name),
                )),
            })),
            "__class__" => Some(PyObject::builtin_type(CompactString::from("iterator"))),
            _ => None,
        },
        PyObjectPayload::BuiltinBoundMethod(_) => match name {
            "__class__" => Some(PyObject::builtin_type(CompactString::from(
                "builtin_function_or_method",
            ))),
            "__name__" => {
                if let PyObjectPayload::BuiltinBoundMethod(bbm) = &obj.payload {
                    Some(PyObject::str_val(bbm.method_name.clone()))
                } else {
                    None
                }
            }
            "__self__" => {
                if let PyObjectPayload::BuiltinBoundMethod(bbm) = &obj.payload {
                    Some(bbm.receiver.clone())
                } else {
                    None
                }
            }
            _ => None,
        },
        PyObjectPayload::NativeClosure(nc) => match name {
            "__name__" | "__qualname__" => Some(PyObject::str_val(nc.name.clone())),
            "__class__" => Some(PyObject::builtin_type(CompactString::from(
                "builtin_function_or_method",
            ))),
            "__doc__" => Some(PyObject::none()),
            "__call__" => Some(obj.clone()),
            "__get__" => {
                let func = obj.clone();
                Some(PyObject::native_closure("__get__", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "__get__ requires at least 1 argument",
                        ));
                    }
                    let instance = &args[0];
                    if matches!(&instance.payload, PyObjectPayload::None) {
                        return Ok(func.clone());
                    }
                    Ok(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: instance.clone(),
                            method: func.clone(),
                        },
                    }))
                }))
            }
            _ => None,
        },
        _ => None,
    }
}
