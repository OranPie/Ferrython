//! Core builtin function implementations (print, len, type, etc.)

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min,
    IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use rustc_hash::FxHashMap;
use parking_lot::RwLock;
use std::sync::Arc;

use super::iter_advance;

pub(super) fn builtin_print(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let parts: Vec<String> = args.iter().map(|a| a.py_to_string()).collect();
    println!("{}", parts.join(" "));
    Ok(PyObject::none())
}

pub(super) fn builtin_len(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("len", args, 1)?;
    let n = args[0].py_len()?;
    Ok(PyObject::int(n as i64))
}

pub(super) fn builtin_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("repr", args, 1)?;
    // Check for user-defined __repr__
    if let Some(repr_method) = args[0].get_attr("__repr__") {
        if matches!(&repr_method.payload, PyObjectPayload::BoundMethod { .. }) {
            // We can't call it here (no VM reference), so use py_to_string on the method
            // Actually, let's extract the result from the repr method
            // For now, fall through to default
        }
    }
    Ok(PyObject::str_val(CompactString::from(args[0].repr())))
}

pub(super) fn builtin_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    // str(bytes_or_bytearray, encoding[, errors])
    if args.len() >= 2 {
        match &args[0].payload {
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                // encoding arg is present (ignore it for now, treat as utf-8)
                let s = String::from_utf8_lossy(b);
                return Ok(PyObject::str_val(CompactString::from(s.as_ref())));
            }
            _ => {}
        }
    }
    Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
}

pub(super) fn builtin_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::int(0));
    }
    if args.len() >= 2 {
        // int(string, base)
        let s = args[0].as_str().ok_or_else(||
            PyException::type_error("int() can't convert non-string with explicit base"))?;
        let mut base = args[1].to_int()? as u32;
        let s = s.trim();
        // Handle base 0: auto-detect from prefix
        let s = if base == 0 {
            if s.starts_with("0x") || s.starts_with("0X") {
                base = 16; &s[2..]
            } else if s.starts_with("0o") || s.starts_with("0O") {
                base = 8; &s[2..]
            } else if s.starts_with("0b") || s.starts_with("0B") {
                base = 2; &s[2..]
            } else {
                base = 10; s
            }
        } else if base == 16 && (s.starts_with("0x") || s.starts_with("0X")) {
            &s[2..]
        } else if base == 8 && (s.starts_with("0o") || s.starts_with("0O")) {
            &s[2..]
        } else if base == 2 && (s.starts_with("0b") || s.starts_with("0B")) {
            &s[2..]
        } else {
            s
        };
        let val = i64::from_str_radix(s, base).map_err(|_|
            PyException::value_error(format!("invalid literal for int() with base {}: '{}'", base, args[0].as_str().unwrap())))?;
        return Ok(PyObject::int(val));
    }
    Ok(PyObject::int(args[0].to_int()?))
}

pub(super) fn builtin_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::float(0.0));
    }
    Ok(PyObject::float(args[0].to_float()?))
}

pub(super) fn builtin_bool(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(args[0].is_truthy()))
}

pub(super) fn builtin_type(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // type.__new__(mcs, name, bases, dict) — called from metaclass __new__
    if args.len() == 4 {
        // First arg is the metaclass (mcs), use it; pass name, bases, dict
        let mcs = &args[0];
        let cls = builtin_type_create(&args[1], &args[2], &args[3])?;
        // Inject metaclass reference if mcs is a user-defined metaclass (not plain 'type')
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if cd.metaclass.is_none() {
                let is_plain_type = matches!(&mcs.payload, PyObjectPayload::BuiltinType(n) if n.as_str() == "type");
                if !is_plain_type {
                    // Re-create with metaclass set
                    return Ok(PyObject::wrap(PyObjectPayload::Class(ferrython_core::object::ClassData {
                        name: cd.name.clone(),
                        bases: cd.bases.clone(),
                        namespace: cd.namespace.clone(),
                        mro: cd.mro.clone(),
                        metaclass: Some(mcs.clone()),
                        method_cache: Arc::new(RwLock::new(FxHashMap::default())),
                        subclasses: Arc::new(RwLock::new(Vec::new())),
                        slots: cd.slots.clone(),
                        has_getattribute: cd.has_getattribute,
                        has_descriptors: cd.has_descriptors,
                    })));
                }
            }
        }
        return Ok(cls);
    }
    if args.len() == 3 {
        // type(name, bases, dict) → dynamic class creation
        return builtin_type_create(&args[0], &args[1], &args[2]);
    }
    check_args("type", args, 1)?;
    let name = args[0].type_name();
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            Ok(inst.class.clone())
        }
        PyObjectPayload::ExceptionInstance { kind, .. } => {
            Ok(PyObject::exception_type(kind.clone()))
        }
        // For classes with a custom metaclass, return the metaclass
        PyObjectPayload::Class(cd) => {
            if let Some(ref mcs) = cd.metaclass {
                Ok(mcs.clone())
            } else {
                Ok(PyObject::builtin_type(CompactString::from("type")))
            }
        }
        _ => Ok(PyObject::builtin_type(CompactString::from(name)))
    }
}

fn builtin_type_create(name_obj: &PyObjectRef, bases_obj: &PyObjectRef, dict_obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let name = name_obj.as_str().ok_or_else(||
        PyException::type_error("type() argument 1 must be str"))?;
    let bases = bases_obj.to_list()?;
    let namespace = match &dict_obj.payload {
        PyObjectPayload::Dict(m) => {
            let r = m.read();
            let mut ns = IndexMap::new();
            for (k, v) in r.iter() {
                let key_str = match k {
                    HashableKey::Str(s) => s.clone(),
                    _ => CompactString::from(k.to_object().py_to_string()),
                };
                ns.insert(key_str, v.clone());
            }
            ns
        }
        _ => return Err(PyException::type_error("type() argument 3 must be dict")),
    };
    let mut mro = Vec::new();
    for base in &bases {
        mro.push(base.clone());
        if let PyObjectPayload::Class(cd) = &base.payload {
            for m in &cd.mro {
                if !mro.iter().any(|existing| Arc::ptr_eq(existing, m)) {
                    mro.push(m.clone());
                }
            }
        }
    }
    Ok(PyObject::wrap(PyObjectPayload::Class(ferrython_core::object::ClassData::new(
        CompactString::from(name),
        bases,
        namespace,
        mro,
        None,
    ))))
}

pub(super) fn builtin_id(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("id", args, 1)?;
    let ptr = std::sync::Arc::as_ptr(&args[0]) as usize;
    Ok(PyObject::int(ptr as i64))
}

pub(crate) fn builtin_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("abs", args, 1)?;
    args[0].py_abs()
}

pub(super) fn builtin_min(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("min expected at least 1 argument, got 0"));
    }
    let items = if args.len() == 1 { args[0].to_list()? } else { args.to_vec() };
    if items.is_empty() {
        return Err(PyException::value_error("min() arg is an empty sequence"));
    }
    let mut best = items[0].clone();
    for item in &items[1..] {
        if item.compare(&best, ferrython_core::object::CompareOp::Lt)?.is_truthy() {
            best = item.clone();
        }
    }
    Ok(best)
}

pub(super) fn builtin_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("max expected at least 1 argument, got 0"));
    }
    let items = if args.len() == 1 { args[0].to_list()? } else { args.to_vec() };
    if items.is_empty() {
        return Err(PyException::value_error("max() arg is an empty sequence"));
    }
    let mut best = items[0].clone();
    for item in &items[1..] {
        if item.compare(&best, ferrython_core::object::CompareOp::Gt)?.is_truthy() {
            best = item.clone();
        }
    }
    Ok(best)
}

pub(super) fn builtin_sum(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("sum expected at least 1 argument, got 0"));
    }
    let items = args[0].to_list()?;
    let start = if args.len() > 1 { args[1].clone() } else { PyObject::int(0) };
    let mut total = start;
    for item in items {
        total = total.add(&item)?;
    }
    Ok(total)
}

pub(super) fn builtin_round(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("round", args, 1)?;
    let ndigits = if args.len() >= 2 { Some(args[1].to_int()?) } else { None };
    match &args[0].payload {
        PyObjectPayload::Int(i) => {
            if let Some(n) = ndigits {
                if n < 0 {
                    let f = i.to_f64();
                    let factor = 10f64.powi((-n) as i32);
                    let rounded = ((f / factor).round() * factor) as i64;
                    Ok(PyObject::int(rounded))
                } else {
                    Ok(args[0].clone())
                }
            } else {
                Ok(args[0].clone())
            }
        }
        PyObjectPayload::Float(f) => {
            if let Some(n) = ndigits {
                if n >= 0 {
                    // Use string formatting to match CPython's rounding behavior
                    let formatted = format!("{:.prec$}", f, prec = n as usize);
                    let rounded: f64 = formatted.parse().unwrap_or(*f);
                    Ok(PyObject::float(rounded))
                } else {
                    let factor = 10f64.powi((-n) as i32);
                    let rounded = (f / factor).round() * factor;
                    Ok(PyObject::float(rounded))
                }
            } else {
                Ok(PyObject::int(round_half_to_even(*f) as i64))
            }
        }
        PyObjectPayload::Bool(b) => Ok(PyObject::int(if *b { 1 } else { 0 })),
        _ => {
            // Check for __round__ dunder method
            if let Some(round_method) = args[0].get_attr("__round__") {
                match &round_method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => {
                        let mut call_args = vec![args[0].clone()];
                        if args.len() >= 2 { call_args.push(args[1].clone()); }
                        return func(&call_args);
                    }
                    PyObjectPayload::NativeClosure { func, .. } => {
                        let mut call_args = vec![args[0].clone()];
                        if args.len() >= 2 { call_args.push(args[1].clone()); }
                        return func(&call_args);
                    }
                    _ => {}
                }
            }
            Err(PyException::type_error(format!(
                "type '{}' doesn't define __round__ method", args[0].type_name()
            )))
        }
    }
}

/// IEEE 754 round-half-to-even (banker's rounding).
/// When the value is exactly halfway between two integers, round to the nearest even integer.
fn round_half_to_even(x: f64) -> f64 {
    let rounded = x.round();
    // Check if we're exactly at a .5 boundary (use strict f64 comparison)
    let frac = (x - x.floor()).abs();
    if (frac - 0.5).abs() < f64::EPSILON * x.abs().max(1.0) {
        // Exactly halfway — round to even
        if rounded as i64 % 2 != 0 {
            if x > 0.0 { rounded - 1.0 } else { rounded + 1.0 }
        } else {
            rounded
        }
    } else {
        rounded
    }
}

pub(super) fn builtin_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("pow", args, 2)?;
    if args.len() >= 3 {
        // Three-argument pow(base, exp, mod) — modular exponentiation
        let modulus = &args[2];
        if matches!(&modulus.payload, PyObjectPayload::None) {
            return Ok(args[0].power(&args[1])?);
        }
        let base_i = args[0].as_int().ok_or_else(||
            PyException::type_error("pow() 1st argument not allowed for 3-argument pow() unless all arguments are integers"))?;
        let exp_i = args[1].as_int().ok_or_else(||
            PyException::type_error("pow() 2nd argument cannot be negative when 3rd argument specified"))?;
        let mod_i = modulus.as_int().ok_or_else(||
            PyException::type_error("pow() 3rd argument not allowed unless all arguments are integers"))?;
        if mod_i == 0 {
            return Err(PyException::value_error("pow() 3rd argument cannot be 0"));
        }
        if exp_i < 0 {
            // Modular inverse: pow(a, -1, m) = modular inverse of a mod m (Python 3.8+)
            // Use extended Euclidean algorithm
            let a = ((base_i % mod_i) + mod_i) % mod_i;
            let (g, x, _) = extended_gcd(a, mod_i);
            if g != 1 {
                return Err(PyException::value_error(format!(
                    "base is not invertible for the given modulus"
                )));
            }
            let inv = ((x % mod_i) + mod_i) % mod_i;
            // For exponents < -1, compute pow(inv, -exp, mod)
            let pos_exp = (-exp_i) as u64;
            if pos_exp == 1 {
                return Ok(PyObject::int(inv));
            }
            let result = mod_pow(inv, pos_exp, mod_i);
            return Ok(PyObject::int(result));
        }
        let result = mod_pow(base_i, exp_i as u64, mod_i);
        Ok(PyObject::int(result))
    } else {
        Ok(args[0].power(&args[1])?)
    }
}

/// Modular exponentiation: (base^exp) % modulus using repeated squaring
fn mod_pow(base: i64, mut exp: u64, modulus: i64) -> i64 {
    let m = modulus.unsigned_abs() as u128;
    let mut result: u128 = 1;
    let mut b = ((base as i128 % modulus as i128 + modulus as i128) % modulus as i128) as u128;
    while exp > 0 {
        if exp & 1 == 1 {
            result = result * b % m;
        }
        b = b * b % m;
        exp >>= 1;
    }
    let r = result as i64;
    if modulus < 0 && r > 0 { r + modulus } else { r }
}

/// Extended Euclidean algorithm: returns (gcd, x, y) such that a*x + b*y = gcd
fn extended_gcd(a: i64, b: i64) -> (i64, i64, i64) {
    if a == 0 {
        return (b, 0, 1);
    }
    let (g, x1, y1) = extended_gcd(b % a, a);
    (g, y1 - (b / a) * x1, x1)
}

pub(super) fn builtin_divmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("divmod", args, 2)?;
    let q = args[0].floor_div(&args[1])?;
    let r = args[0].modulo(&args[1])?;
    Ok(PyObject::tuple(vec![q, r]))
}

pub(super) fn builtin_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hash", args, 1)?;
    let key = args[0].to_hashable_key()?;
    let h = match key {
        HashableKey::Int(n) => n.to_i64().unwrap_or(0),
        HashableKey::Bool(b) => b as i64,
        HashableKey::Str(ref s) => {
            let mut h: u64 = 5381;
            for c in s.bytes() { h = h.wrapping_mul(33).wrapping_add(c as u64); }
            h as i64
        }
        HashableKey::Float(f) => {
            // Match hash consistency: integer-valued floats hash like their int
            let fv = f.0;
            if fv.is_finite() && fv == fv.trunc() && fv.abs() < (i64::MAX as f64) {
                fv as i64
            } else {
                f.0.to_bits() as i64
            }
        }
        HashableKey::None => 0,
        HashableKey::Tuple(items) => {
            // CPython tuple hash: xxHash-based mixing
            let mut h: u64 = 0x345678;
            let mult: u64 = 1000003;
            for item in items {
                let item_hash = builtin_hash(&[item.to_object()])
                    .map(|v| v.as_int().unwrap_or(0) as u64)
                    .unwrap_or(0);
                h = h.wrapping_mul(mult) ^ item_hash;
            }
            h as i64
        }
        HashableKey::FrozenSet(items) => {
            // CPython frozenset hash algorithm (order-independent, collision-resistant)
            let mask: u64 = u64::MAX;
            let n = items.len() as u64;
            let mut h: u64 = 1927868237u64.wrapping_mul(n.wrapping_add(1)) & mask;
            for item in items {
                let hx = builtin_hash(&[item.to_object()])
                    .map(|v| v.as_int().unwrap_or(0) as u64)
                    .unwrap_or(0);
                h ^= (hx ^ (hx << 16) ^ 89869747).wrapping_mul(3644798167) & mask;
            }
            h = h.wrapping_mul(69069).wrapping_add(907133923) & mask;
            let result = h as i64;
            if result == -1 { 590923713 } else { result }
        }
        HashableKey::Bytes(b) => { let mut h: u64 = 5381; for x in b { h = h.wrapping_mul(33).wrapping_add(x as u64); } h as i64 }
        HashableKey::Identity(ptr, _) => ptr as i64,
        HashableKey::Custom { hash_value, .. } => hash_value,
    };
    Ok(PyObject::int(h))
}

pub(super) fn builtin_isinstance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("isinstance", args, 2)?;
    let obj = &args[0];
    let cls = &args[1];
    // Handle tuple of types: isinstance(x, (int, str))
    if let PyObjectPayload::Tuple(types) = &cls.payload {
        for t in types {
            if is_instance_of(obj, t) {
                return Ok(PyObject::bool_val(true));
            }
        }
        return Ok(PyObject::bool_val(false));
    }
    // Handle PEP 604 union types: isinstance(x, int | str)
    if let Some(union_flag) = cls.get_attr("__union_params__") {
        if union_flag.is_truthy() {
            if let Some(args_tuple) = cls.get_attr("__args__") {
                if let PyObjectPayload::Tuple(types) = &args_tuple.payload {
                    for t in types {
                        if is_instance_of(obj, t) {
                            return Ok(PyObject::bool_val(true));
                        }
                    }
                    return Ok(PyObject::bool_val(false));
                }
            }
        }
    }
    Ok(PyObject::bool_val(is_instance_of(obj, cls)))
}

/// Check if obj is an instance of cls (including inheritance).
pub(crate) fn is_instance_of(obj: &PyObjectRef, cls: &PyObjectRef) -> bool {
    match &cls.payload {
        PyObjectPayload::BuiltinFunction(type_name) | PyObjectPayload::BuiltinType(type_name) => {
            // Everything is an instance of object
            if type_name.as_str() == "object" {
                return true;
            }
            let obj_type = obj.type_name();
            if obj_type == type_name.as_str() {
                return true;
            }
            // Built-in subtype relationships: bool is subclass of int
            if type_name.as_str() == "int" && obj_type == "bool" {
                return true;
            }
            // IntEnum members are also int instances
            if type_name.as_str() == "int" {
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        let ns = cd.namespace.read();
                        if ns.contains_key("__int_enum__") || ns.contains_key("_value_") {
                            for base in &cd.bases {
                                if class_is_subclass_of(base, "IntEnum") || class_is_subclass_of(base, "int") {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            // collections.abc structural checks (duck typing)
            if check_abc_structural(obj, type_name.as_str()) {
                return true;
            }
            // Check user-defined classes that inherit from builtins
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                return class_is_subclass_of(&inst.class, type_name.as_str());
            }
            false
        }
        PyObjectPayload::Class(target_cd) => {
            // Check _abc_builtin_types registry (collections.abc uses this)
            let obj_type = obj.type_name();
            if let Some(registry) = target_cd.namespace.read().get("_abc_builtin_types") {
                if let PyObjectPayload::Set(set) = &registry.payload {
                    let key = HashableKey::Str(CompactString::from(obj_type));
                    if set.read().contains_key(&key) {
                        return true;
                    }
                }
            }
            // Check collections.abc structural typing for Class-based ABCs
            if check_abc_structural(obj, target_cd.name.as_str()) {
                return true;
            }
            // Check _abc_registry for ABCMeta.register() virtual subclasses
            // Walk the class and its bases (MRO) to find registries
            {
                let mut classes_to_check: Vec<PyObjectRef> = vec![cls.clone()];
                classes_to_check.extend(target_cd.bases.iter().cloned());
                for check_cls in &classes_to_check {
                    if let PyObjectPayload::Class(ref check_cd) = check_cls.payload {
                        if let Some(registry) = check_cd.namespace.read().get("_abc_registry").cloned() {
                            if let PyObjectPayload::Dict(map) = &registry.payload {
                                let obj_class = match &obj.payload {
                                    PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
                                    PyObjectPayload::Class(_) => Some(obj.clone()),
                                    _ => None,
                                };
                                if let Some(oc) = obj_class {
                                    let obj_class_name = match &oc.payload {
                                        PyObjectPayload::Class(cd) => Some(cd.name.clone()),
                                        _ => None,
                                    };
                                    for (k, _) in map.read().iter() {
                                        if let HashableKey::Identity(_, registered) = k {
                                            if Arc::ptr_eq(registered, &oc) {
                                                return true;
                                            }
                                            if let Some(ref ocn) = obj_class_name {
                                                if let PyObjectPayload::Class(rc) = &registered.payload {
                                                    if rc.name == *ocn {
                                                        return true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // runtime_checkable Protocol check
            if let Some(flag) = target_cd.namespace.read().get("_is_runtime_checkable") {
                if flag.is_truthy() {
                    if let Some(attrs) = target_cd.namespace.read().get("__protocol_attrs__") {
                        if let PyObjectPayload::Tuple(required) = &attrs.payload {
                            return required.iter().all(|attr_name| {
                                let name = attr_name.py_to_string();
                                obj.get_attr(&name).is_some()
                            });
                        }
                    }
                }
            }
            // User-defined class check: walk the instance's class MRO
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                class_is_subclass_of(&inst.class, &target_cd.name)
            } else if let PyObjectPayload::Class(obj_cd) = &obj.payload {
                // Metaclass check: isinstance(MyClass, Meta) where Meta is a metaclass
                if let Some(ref mcs) = obj_cd.metaclass {
                    if let PyObjectPayload::Class(mcs_cd) = &mcs.payload {
                        if mcs_cd.name == target_cd.name {
                            return true;
                        }
                        // Check MRO of the metaclass
                        return class_is_subclass_of(mcs, &target_cd.name);
                    }
                }
                // All classes are instances of 'type'
                target_cd.name.as_str() == "type"
            } else {
                false
            }
        }
        PyObjectPayload::ExceptionType(kind) => {
            // Check if obj is an exception instance of this type
            if let PyObjectPayload::ExceptionInstance { kind: obj_kind, .. } = &obj.payload {
                if obj_kind == kind {
                    return true;
                }
                // Check exception hierarchy
                return exception_is_subclass_of(obj_kind.clone(), &format!("{:?}", kind));
            }
            // Check if obj is a user-defined class instance that inherits from this exception
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                let kind_name = format!("{:?}", kind);
                return class_is_subclass_of(&inst.class, &kind_name);
            }
            false
        }
        // NativeFunction/NativeClosure used as constructor (e.g., ChainMap, OrderedDict):
        // Check if the instance's class name matches
        PyObjectPayload::NativeFunction { name: func_name, .. } |
        PyObjectPayload::NativeClosure { name: func_name, .. } => {
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    let cls_name = cd.name.as_str();
                    if !func_name.is_empty() && cls_name == func_name.as_str() {
                        return true;
                    }
                    return class_is_subclass_of(&inst.class, func_name.as_str());
                }
            }
            false
        }
        _ => false,
    }
}
pub(crate) fn class_is_subclass_of(cls: &PyObjectRef, target_name: &str) -> bool {
    match &cls.payload {
        PyObjectPayload::Class(cd) => {
            if cd.name.as_str() == target_name {
                return true;
            }
            for base in &cd.bases {
                if class_is_subclass_of(base, target_name) {
                    return true;
                }
            }
            false
        }
        // Handle builtin type bases (e.g., class MyList(list))
        PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
            name.as_str() == target_name
        }
        // Handle exception type bases (e.g., class AppError(Exception))
        PyObjectPayload::ExceptionType(kind) => {
            let kind_name = format!("{:?}", kind);
            if kind_name == target_name {
                return true;
            }
            // Walk up the exception hierarchy
            exception_is_subclass_of(kind.clone(), target_name)
        }
        _ => false,
    }
}

/// Check if an ExceptionKind is a subclass of a target by name.
fn exception_is_subclass_of(kind: ExceptionKind, target_name: &str) -> bool {
    if let Some(target_kind) = ExceptionKind::from_name(target_name) {
        is_exception_subclass(&kind, &target_kind)
    } else {
        false
    }
}

/// Structural (duck-type) check for collections.abc ABCs.
fn check_abc_structural(obj: &PyObjectRef, abc_name: &str) -> bool {
    match abc_name {
        "Iterable" => {
            matches!(obj.type_name(), "list" | "tuple" | "str" | "dict" | "set" | "frozenset" | "bytes" | "bytearray" | "range" | "iterator" | "generator")
                || obj.get_attr("__iter__").is_some()
        }
        "Iterator" => {
            matches!(obj.type_name(), "iterator" | "generator")
                || obj.get_attr("__next__").is_some()
        }
        "Mapping" | "MutableMapping" => {
            matches!(obj.type_name(), "dict")
                || (obj.get_attr("__getitem__").is_some() && obj.get_attr("keys").is_some())
        }
        "Sequence" | "MutableSequence" => {
            matches!(obj.type_name(), "list" | "tuple" | "str" | "bytes" | "bytearray" | "range")
        }
        "Set" | "MutableSet" => {
            matches!(obj.type_name(), "set" | "frozenset")
        }
        "Callable" => {
            obj.is_callable()
        }
        "Hashable" => {
            !matches!(obj.type_name(), "list" | "dict" | "set" | "bytearray")
        }
        "Sized" => {
            matches!(obj.type_name(), "list" | "tuple" | "str" | "dict" | "set" | "frozenset" | "bytes" | "bytearray" | "range")
                || obj.get_attr("__len__").is_some()
        }
        "Collection" => {
            check_abc_structural(obj, "Sized") && check_abc_structural(obj, "Iterable")
                && obj.get_attr("__contains__").is_some()
                || matches!(obj.type_name(), "list" | "tuple" | "str" | "dict" | "set" | "frozenset" | "bytes" | "bytearray" | "range")
        }
        "Reversible" => {
            matches!(obj.type_name(), "list" | "tuple" | "str" | "dict" | "bytes" | "bytearray" | "range")
        }
        "Container" => {
            matches!(obj.type_name(), "list" | "tuple" | "str" | "dict" | "set" | "frozenset" | "bytes" | "bytearray" | "range")
                || obj.get_attr("__contains__").is_some()
        }
        "Number" | "Complex" => {
            matches!(obj.type_name(), "int" | "float" | "complex" | "bool")
        }
        "Real" => {
            matches!(obj.type_name(), "int" | "float" | "bool")
        }
        "Rational" | "Integral" => {
            matches!(obj.type_name(), "int" | "bool")
        }
        _ => false,
    }
}

pub(super) fn builtin_callable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("callable", args, 1)?;
    Ok(PyObject::bool_val(args[0].is_callable()))
}

pub(super) fn builtin_input(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if !args.is_empty() {
        print!("{}", args[0].py_to_string());
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).map_err(|e|
        PyException::runtime_error(format!("input error: {}", e))
    )?;
    if buf.ends_with('\n') { buf.pop(); }
    if buf.ends_with('\r') { buf.pop(); }
    Ok(PyObject::str_val(CompactString::from(buf)))
}

pub(super) fn builtin_ord(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("ord", args, 1)?;
    // Accept both str and bytes (CPython: ord('a') == ord(b'a') == 97)
    match &args[0].payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            if b.len() != 1 {
                return Err(PyException::type_error(format!(
                    "ord() expected a character, but bytes of length {} found", b.len()
                )));
            }
            return Ok(PyObject::int(b[0] as i64));
        }
        PyObjectPayload::Int(n) => {
            // bytearray indexing returns int in Python 3
            let v = n.to_i64().unwrap_or(0);
            return Ok(PyObject::int(v));
        }
        _ => {}
    }
    let s = args[0].as_str().ok_or_else(|| PyException::type_error(
        "ord() expected string of length 1, but found non-string"
    ))?;
    let mut chars = s.chars();
    let c = chars.next().ok_or_else(|| PyException::type_error("ord() expected a character"))?;
    if chars.next().is_some() {
        return Err(PyException::type_error("ord() expected a character, but string of length > 1 found"));
    }
    Ok(PyObject::int(c as i64))
}

pub(super) fn builtin_chr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("chr", args, 1)?;
    let n = args[0].as_int().ok_or_else(|| PyException::type_error("chr() expects int"))?;
    if n < 0 || n > 0x10FFFF {
        return Err(PyException::value_error(
            format!("chr() arg not in range(0x110000): {}", n)));
    }
    // Rust char doesn't allow surrogates (0xD800-0xDFFF), but CPython does
    let s = if let Some(c) = char::from_u32(n as u32) {
        c.to_string()
    } else {
        // Surrogate codepoint — encode as replacement char
        String::from('\u{FFFD}')
    };
    Ok(PyObject::str_val(CompactString::from(s)))
}

/// Resolve an integer from an object, trying `as_int()` first then `__index__`.
fn resolve_index(obj: &PyObjectRef, func_name: &str) -> PyResult<i64> {
    if let Some(n) = obj.as_int() {
        return Ok(n);
    }
    // Try __index__ protocol on instances
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        // Look up __index__ in the instance's class or attrs
        let index_fn = {
            let attrs = inst.attrs.read();
            attrs.get("__index__").cloned()
        }.or_else(|| obj.get_attr("__index__"));
        if let Some(func) = index_fn {
            let result = match &func.payload {
                PyObjectPayload::NativeClosure { func, .. } => func(&[])?,
                PyObjectPayload::NativeFunction { func, .. } => func(&[])?,
                PyObjectPayload::BoundMethod { receiver: _, method } => {
                    // Call the bound method — for simple __index__ methods that
                    // just return an int, we can try NativeFunction/NativeClosure
                    match &method.payload {
                        PyObjectPayload::NativeClosure { func, .. } => func(&[obj.clone()])?,
                        PyObjectPayload::NativeFunction { func, .. } => func(&[obj.clone()])?,
                        // Python-defined __index__ needs VM; we can't call it here.
                        // Fall through to error.
                        _ => return Err(PyException::type_error(format!(
                            "'{}'() integer argument expected, got '{}'", func_name, obj.type_name()))),
                    }
                }
                PyObjectPayload::Function(_) => {
                    // Python function needs VM to call — can't do it from here.
                    // But for the common case of __index__ defined in the class,
                    // it'll be accessed as BoundMethod via get_attr, handled above.
                    return Err(PyException::type_error(format!(
                        "'{}'() integer argument expected, got '{}'", func_name, obj.type_name())));
                }
                _ => return Err(PyException::type_error(format!(
                    "'{}'() integer argument expected, got '{}'", func_name, obj.type_name()))),
            };
            if let Some(n) = result.as_int() {
                return Ok(n);
            }
        }
    }
    Err(PyException::type_error(format!(
        "'{}'() integer argument expected, got '{}'", func_name, obj.type_name())))
}

pub(super) fn builtin_hex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hex", args, 1)?;
    let n = resolve_index(&args[0], "hex")?;
    let s = if n < 0 { format!("-0x{:x}", -n) } else { format!("0x{:x}", n) };
    Ok(PyObject::str_val(CompactString::from(s)))
}

pub(super) fn builtin_oct(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("oct", args, 1)?;
    let n = resolve_index(&args[0], "oct")?;
    let s = if n < 0 { format!("-0o{:o}", -n) } else { format!("0o{:o}", n) };
    Ok(PyObject::str_val(CompactString::from(s)))
}

pub(super) fn builtin_bin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("bin", args, 1)?;
    let n = resolve_index(&args[0], "bin")?;
    let s = if n < 0 { format!("-0b{:b}", -n) } else { format!("0b{:b}", n) };
    Ok(PyObject::str_val(CompactString::from(s)))
}

pub(super) fn builtin_sorted(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("sorted", args, 1)?;
    let mut items = args[0].to_list()?;
    items.sort_by(|a, b| {
        if let Ok(r) = a.compare(b, ferrython_core::object::CompareOp::Lt) {
            if r.is_truthy() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }
        } else {
            std::cmp::Ordering::Equal
        }
    });
    Ok(PyObject::list(items))
}

pub(super) fn builtin_reversed(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("reversed", args, 1)?;
    let mut items = args[0].to_list()?;
    items.reverse();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(parking_lot::Mutex::new(ferrython_core::object::IteratorData::List { items, index: 0 }))
    )))
}

pub(super) fn builtin_enumerate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("enumerate", args, 1)?;
    let start = if args.len() > 1 {
        args[1].as_int().unwrap_or(0)
    } else { 0 };
    // Get an iterator from the source
    let source = get_iter_from_obj(&args[0])?;
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(parking_lot::Mutex::new(IteratorData::Enumerate { source, index: start }))
    )))
}

pub(super) fn builtin_zip(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
            Arc::new(parking_lot::Mutex::new(IteratorData::List { items: vec![], index: 0 }))
        )));
    }
    // Check for trailing kwargs dict with strict=True
    let mut strict = false;
    let iter_args = if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("strict"))) {
                strict = v.is_truthy();
                &args[..args.len() - 1]
            } else {
                args
            }
        } else {
            args
        }
    } else {
        args
    };
    let sources: Vec<PyObjectRef> = iter_args.iter()
        .map(|a| get_iter_from_obj(a))
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(parking_lot::Mutex::new(IteratorData::Zip { sources, strict }))
    )))
}

/// Get an iterator from any iterable object.
pub(super) fn get_iter_from_obj(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::Iterator(_) | PyObjectPayload::Generator(_) | PyObjectPayload::AsyncGenerator(_) => Ok(obj.clone()),
        PyObjectPayload::Range { start, stop, step } => {
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(parking_lot::Mutex::new(IteratorData::Range { current: *start, stop: *stop, step: *step }))
            )))
        }
        PyObjectPayload::List(items) => {
            let items = items.read().clone();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(parking_lot::Mutex::new(IteratorData::List { items, index: 0 }))
            )))
        }
        PyObjectPayload::Tuple(items) => {
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(parking_lot::Mutex::new(IteratorData::Tuple { items: items.clone(), index: 0 }))
            )))
        }
        PyObjectPayload::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(parking_lot::Mutex::new(IteratorData::Str { chars, index: 0 }))
            )))
        }
        PyObjectPayload::Set(m) => {
            let items: Vec<PyObjectRef> = m.read().values().cloned().collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(parking_lot::Mutex::new(IteratorData::List { items, index: 0 }))
            )))
        }
        PyObjectPayload::Dict(m) => {
            let items: Vec<PyObjectRef> = m.read().keys().map(|k| k.to_object()).collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(parking_lot::Mutex::new(IteratorData::List { items, index: 0 }))
            )))
        }
        PyObjectPayload::Instance(_) => {
            // For builtins without VM access, check if it's already an iterator
            if obj.get_attr("__next__").is_some() || obj.get_attr("__iter__").is_some() {
                // Try core get_iter (handles dict_storage, namedtuple, etc.)
                match obj.get_iter() {
                    Ok(iter) => Ok(iter),
                    Err(_) => Ok(obj.clone()),
                }
            } else {
                Err(PyException::type_error(format!("'{}' object is not iterable", obj.type_name())))
            }
        }
        // Module with __iter__ (file objects, module_with_attrs with _bind_methods)
        // Need to call __iter__ method to get the iterable result
        PyObjectPayload::Module(_) => {
            if let Some(iter_attr) = obj.get_attr("__iter__") {
                match &iter_attr.payload {
                    // __iter__ returned a list/iterator directly
                    PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) | PyObjectPayload::Iterator(_) => {
                        return get_iter_from_obj(&iter_attr);
                    }
                    // __iter__ is a bound method — call it
                    PyObjectPayload::BoundMethod { receiver, method } => {
                        if let PyObjectPayload::NativeClosure { func, .. } = &method.payload {
                            let result = func(&[receiver.clone()])?;
                            return get_iter_from_obj(&result);
                        }
                        if let PyObjectPayload::NativeFunction { func, .. } = &method.payload {
                            let result = func(&[receiver.clone()])?;
                            return get_iter_from_obj(&result);
                        }
                    }
                    // __iter__ is a native closure/function to call with self
                    PyObjectPayload::NativeClosure { func, .. } => {
                        let result = func(&[obj.clone()])?;
                        return get_iter_from_obj(&result);
                    }
                    PyObjectPayload::NativeFunction { func, .. } => {
                        let result = func(&[obj.clone()])?;
                        return get_iter_from_obj(&result);
                    }
                    _ => {}
                }
            }
            Err(PyException::type_error(format!("'{}' object is not iterable", obj.type_name())))
        }
        // Delegate all other payload types to the core get_iter (handles DictKeys, DictValues,
        // DictItems, Bytes, ByteArray, FrozenSet, MappingProxy, etc.)
        // For Module payloads (file objects etc.) with __iter__, the __iter__ returns
        // a list — use that directly.
        PyObjectPayload::Module(_) => {
            if let Some(iter_attr) = obj.get_attr("__iter__") {
                match &iter_attr.payload {
                    // __iter__ returned a list/iterator directly (not a method)
                    PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) | PyObjectPayload::Iterator(_) => {
                        return get_iter_from_obj(&iter_attr);
                    }
                    // __iter__ is a bound method — it's a NativeClosure that needs calling
                    // which we can't do from builtins. But we know file __iter__ returns
                    // a list, so try calling it via the bound method pattern:
                    PyObjectPayload::BoundMethod { receiver, method } => {
                        if let PyObjectPayload::NativeClosure { func, .. } = &method.payload {
                            let result = func(&[receiver.clone()])?;
                            return get_iter_from_obj(&result);
                        }
                        if let PyObjectPayload::NativeFunction { func, .. } = &method.payload {
                            let result = func(&[receiver.clone()])?;
                            return get_iter_from_obj(&result);
                        }
                    }
                    PyObjectPayload::NativeClosure { func, .. } => {
                        let result = func(&[obj.clone()])?;
                        return get_iter_from_obj(&result);
                    }
                    PyObjectPayload::NativeFunction { func, .. } => {
                        let result = func(&[obj.clone()])?;
                        return get_iter_from_obj(&result);
                    }
                    _ => {}
                }
            }
            Err(PyException::type_error(format!("'{}' object is not iterable", obj.type_name())))
        }
        _ => obj.get_iter().map_err(|_| {
            PyException::type_error(format!("'{}' object is not iterable", obj.type_name()))
        }),
    }
}

pub(super) fn builtin_range(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (start, stop, step) = match args.len() {
        1 => {
            let stop = args[0].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            (0i64, stop, 1i64)
        }
        2 => {
            let start = args[0].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            let stop = args[1].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            (start, stop, 1)
        }
        3 => {
            let start = args[0].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            let stop = args[1].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            let step = args[2].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            if step == 0 {
                return Err(PyException::value_error("range() arg 3 must not be zero"));
            }
            (start, stop, step)
        }
        _ => return Err(PyException::type_error("range expected 1 to 3 arguments")),
    };
    Ok(PyObject::range(start, stop, step))
}

pub(super) fn builtin_list(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    // For Module payloads (e.g. file objects), use VM-level iteration that can call __iter__
    if matches!(&args[0].payload, PyObjectPayload::Module(_)) {
        let iter = get_iter_from_obj(&args[0])?;
        let mut items = Vec::new();
        loop {
            match iter_advance(&iter)? {
                Some((_new_iter, value)) => items.push(value),
                None => break,
            }
        }
        return Ok(PyObject::list(items));
    }
    let items = args[0].to_list()?;
    Ok(PyObject::list(items))
}

pub(super) fn builtin_tuple(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::tuple(vec![]));
    }
    if matches!(&args[0].payload, PyObjectPayload::Module(_)) {
        let iter = get_iter_from_obj(&args[0])?;
        let mut items = Vec::new();
        loop {
            match iter_advance(&iter)? {
                Some((_new_iter, value)) => items.push(value),
                None => break,
            }
        }
        return Ok(PyObject::tuple(items));
    }
    let items = args[0].to_list()?;
    Ok(PyObject::tuple(items))
}

pub(super) fn builtin_dict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::dict(IndexMap::new()));
    }
    match &args[0].payload {
        PyObjectPayload::Dict(m) => {
            let mut new_map = m.read().clone();
            new_map.shift_remove(&HashableKey::Str(CompactString::from("__defaultdict_factory__")));
            new_map.shift_remove(&HashableKey::Str(CompactString::from("__counter__")));
            Ok(PyObject::dict(new_map))
        },
        PyObjectPayload::MappingProxy(m) => {
            Ok(PyObject::dict(m.read().clone()))
        },
        PyObjectPayload::InstanceDict(m) => {
            let read = m.read();
            let mut map = IndexMap::new();
            for (k, v) in read.iter() {
                if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                    map.insert(hk, v.clone());
                }
            }
            Ok(PyObject::dict(map))
        },
        // dict from iterable of (key, value) pairs
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) | PyObjectPayload::Iterator(_) | PyObjectPayload::Set(_) => {
            let pairs = args[0].to_list()?;
            let mut map = IndexMap::new();
            for pair in &pairs {
                let kv = pair.to_list()?;
                if kv.len() != 2 {
                    return Err(PyException::value_error(
                        format!("dictionary update sequence element has length {}; 2 is required", kv.len())));
                }
                let key = kv[0].to_hashable_key()?;
                map.insert(key, kv[1].clone());
            }
            Ok(PyObject::dict(map))
        }
        _ => {
            // Try to handle instances with dict_storage (OrderedDict, dict subclasses)
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let read = ds.read();
                    return Ok(PyObject::dict(read.clone()));
                }
            }
            // Fall back to iterating as pairs
            let pairs = args[0].to_list()?;
            let mut map: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
            for pair in &pairs {
                let kv = pair.to_list()?;
                if kv.len() != 2 {
                    return Err(PyException::value_error(
                        format!("dictionary update sequence element has length {}; 2 is required", kv.len())));
                }
                let key = kv[0].to_hashable_key()?;
                map.insert(key, kv[1].clone());
            }
            Ok(PyObject::dict(map))
        }
    }
}

pub(super) fn builtin_set(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::set(IndexMap::new()));
    }
    let items = args[0].to_list()?;
    let mut set = IndexMap::new();
    for item in items {
        if let Ok(key) = item.to_hashable_key() {
            set.insert(key, item);
        }
    }
    Ok(PyObject::set(set))
}

pub(super) fn builtin_frozenset(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::frozenset(IndexMap::new()));
    }
    let items = args[0].to_list()?;
    let mut set = IndexMap::new();
    for item in items {
        if let Ok(key) = item.to_hashable_key() {
            set.insert(key, item);
        }
    }
    Ok(PyObject::frozenset(set))
}

pub(super) fn builtin_all(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("all", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if !item.is_truthy() { return Ok(PyObject::bool_val(false)); }
    }
    Ok(PyObject::bool_val(true))
}

pub(super) fn builtin_any(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("any", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if item.is_truthy() { return Ok(PyObject::bool_val(true)); }
    }
    Ok(PyObject::bool_val(false))
}

pub(super) fn builtin_iter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() == 2 {
        // iter(callable, sentinel) — creates a lazy sentinel iterator
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
            Arc::new(parking_lot::Mutex::new(IteratorData::Sentinel {
                callable: args[0].clone(),
                sentinel: args[1].clone(),
            }))
        )));
    }
    check_args("iter", args, 1)?;
    args[0].get_iter()
}

pub(super) fn builtin_next(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("next", args, 1)?;
    match iter_advance(&args[0])? {
        Some((_new_iter, value)) => Ok(value),
        None => {
            if args.len() > 1 {
                Ok(args[1].clone())
            } else {
                Err(PyException::stop_iteration())
            }
        }
    }
}

pub(super) fn builtin_hasattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hasattr", args, 2)?;
    let name = args[1].as_str().ok_or_else(||
        PyException::type_error("hasattr(): attribute name must be string"))?;
    Ok(PyObject::bool_val(args[0].get_attr(name).is_some()))
}

pub(super) fn builtin_getattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("getattr", args, 2)?;
    let name = args[1].as_str().ok_or_else(||
        PyException::type_error("getattr(): attribute name must be string"))?;
    match args[0].get_attr(name) {
        Some(v) => Ok(v),
        None => {
            if args.len() > 2 {
                Ok(args[2].clone())
            } else {
                Err(PyException::attribute_error(format!(
                    "'{}' object has no attribute '{}'", args[0].type_name(), name
                )))
            }
        }
    }
}

pub(crate) fn builtin_dir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    let names = args[0].dir();
    let items: Vec<PyObjectRef> = names.into_iter().map(|n| PyObject::str_val(n)).collect();
    Ok(PyObject::list(items))
}

pub(super) fn builtin_format(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("format", args, 1)?;
    if args.len() >= 2 {
        let spec = args[1].py_to_string();
        if !spec.is_empty() {
            return args[0].format_value(&spec).map(|s| PyObject::str_val(CompactString::from(s)));
        }
    }
    Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
}

pub(super) fn builtin_ascii(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("ascii", args, 1)?;
    let repr = args[0].repr();
    // ascii() takes repr() and escapes non-ASCII characters
    let escaped: String = repr.chars().map(|c| {
        if c.is_ascii() { c.to_string() }
        else if (c as u32) <= 0xff { format!("\\x{:02x}", c as u32) }
        else if (c as u32) <= 0xffff { format!("\\u{:04x}", c as u32) }
        else { format!("\\U{:08x}", c as u32) }
    }).collect();
    Ok(PyObject::str_val(CompactString::from(escaped)))
}

pub(super) fn builtin_property(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fget_raw = args.first().cloned();
    let fset = args.get(1).cloned();
    let fdel = args.get(2).cloned();
    // If fget is an abstract marker ("__abstract__", func), keep it as-is.
    // is_abstract_marker() detects Property.fget abstract markers.
    // unwrap_abstract_fget() unwraps the marker when actually calling the getter.
    Ok(Arc::new(PyObject {
        payload: PyObjectPayload::Property { fget: fget_raw, fset, fdel },
    }))
}

/// Unwrap abstract marker from a property fget if present.
/// Returns the real callable function, whether it was abstract-wrapped or not.
pub(crate) fn unwrap_abstract_fget(fget: &PyObjectRef) -> PyObjectRef {
    if let PyObjectPayload::Tuple(items) = &fget.payload {
        if items.len() == 2 && items[0].as_str() == Some("__abstract__") {
            return items[1].clone();
        }
    }
    fget.clone()
}

pub(super) fn builtin_staticmethod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("staticmethod", args, 1)?;
    Ok(Arc::new(PyObject {
        payload: PyObjectPayload::StaticMethod(args[0].clone()),
    }))
}

pub(super) fn builtin_classmethod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("classmethod", args, 1)?;
    Ok(Arc::new(PyObject {
        payload: PyObjectPayload::ClassMethod(args[0].clone()),
    }))
}

pub(super) fn builtin_setattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 3 {
        return Err(PyException::type_error("setattr() takes exactly 3 arguments"));
    }
    let name = args[1].py_to_string();
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            inst.attrs.write().insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::Class(cd) => {
            cd.namespace.write().insert(CompactString::from(name), args[2].clone());
            cd.invalidate_cache();
        }
        PyObjectPayload::Module(m) => {
            m.attrs.write().insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::ExceptionInstance { attrs, .. } => {
            attrs.write().insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::Function(f) => {
            f.attrs.write().insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. } |
        PyObjectPayload::BuiltinFunction(_) => {
            // Silently accept — native functions don't have persistent attrs
        }
        _ => return Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute assignment", args[0].type_name()
        ))),
    }
    Ok(PyObject::none())
}

pub(super) fn builtin_delattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("delattr", args, 2)?;
    let name = args[1].py_to_string();
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            inst.attrs.write().shift_remove(name.as_str());
        }
        PyObjectPayload::Module(md) => {
            md.attrs.write().shift_remove(name.as_str());
        }
        _ => return Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute deletion", args[0].type_name()
        ))),
    }
    Ok(PyObject::none())
}

pub(super) fn builtin_vars(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::dict_from_pairs(vec![]));
    }
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = attrs.iter()
                .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
                .collect();
            Ok(PyObject::dict_from_pairs(pairs))
        }
        PyObjectPayload::Class(cd) => {
            let ns = cd.namespace.read();
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = ns.iter()
                .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
                .collect();
            Ok(PyObject::dict_from_pairs(pairs))
        }
        PyObjectPayload::Module(md) => {
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = md.attrs.read().iter()
                .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
                .collect();
            Ok(PyObject::dict_from_pairs(pairs))
        }
        _ => Err(PyException::type_error("vars() argument must have __dict__ attribute")),
    }
}

pub(super) fn builtin_globals(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::dict_from_pairs(vec![]))
}

pub(super) fn builtin_locals(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::dict_from_pairs(vec![]))
}

pub(super) fn builtin_slice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let to_opt = |a: &PyObjectRef| -> Option<PyObjectRef> {
        if matches!(a.payload, PyObjectPayload::None) { None } else { Some(a.clone()) }
    };
    match args.len() {
        0 => Err(PyException::type_error("slice expected at least 1 argument, got 0")),
        1 => Ok(PyObject::slice(None, to_opt(&args[0]), None)),
        2 => Ok(PyObject::slice(to_opt(&args[0]), to_opt(&args[1]), None)),
        _ => Ok(PyObject::slice(to_opt(&args[0]), to_opt(&args[1]), to_opt(&args[2]))),
    }
}

pub(super) fn builtin_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bytes(vec![]));
    }
    match &args[0].payload {
        PyObjectPayload::Bytes(b) => Ok(PyObject::bytes(b.clone())),
        PyObjectPayload::ByteArray(b) => Ok(PyObject::bytes(b.clone())),
        PyObjectPayload::Str(s) => {
            // bytes(string, encoding) — require encoding argument
            if args.len() >= 2 {
                Ok(PyObject::bytes(s.as_bytes().to_vec()))
            } else {
                Err(PyException::type_error("string argument without an encoding"))
            }
        }
        PyObjectPayload::Int(n) => {
            let size = n.to_i64().unwrap_or(0) as usize;
            Ok(PyObject::bytes(vec![0u8; size]))
        }
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
            let items = args[0].to_list()?;
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                let v = item.to_int().map_err(|_| PyException::type_error("an integer is required"))?;
                if v < 0 || v > 255 {
                    return Err(PyException::value_error("bytes must be in range(0, 256)"));
                }
                result.push(v as u8);
            }
            Ok(PyObject::bytes(result))
        }
        _ => {
            // Check for __bytes__ dunder method
            if let Some(bytes_method) = args[0].get_attr("__bytes__") {
                match &bytes_method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => return func(&[args[0].clone()]),
                    PyObjectPayload::NativeClosure { func, .. } => return func(&[args[0].clone()]),
                    _ => {}
                }
            }
            // Try as general iterable (range, generator, etc.)
            if let Ok(items) = args[0].to_list() {
                let mut result = Vec::with_capacity(items.len());
                for item in items {
                    let v = item.to_int().map_err(|_| PyException::type_error("an integer is required"))?;
                    if v < 0 || v > 255 {
                        return Err(PyException::value_error("bytes must be in range(0, 256)"));
                    }
                    result.push(v as u8);
                }
                return Ok(PyObject::bytes(result));
            }
            Err(PyException::type_error("cannot convert to bytes"))
        }
    }
}

pub(super) fn builtin_bytearray(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bytearray(vec![]));
    }
    match &args[0].payload {
        PyObjectPayload::Bytes(b) => Ok(PyObject::bytearray(b.clone())),
        PyObjectPayload::ByteArray(b) => Ok(PyObject::bytearray(b.clone())),
        PyObjectPayload::Str(s) => {
            if args.len() >= 2 {
                Ok(PyObject::bytearray(s.as_bytes().to_vec()))
            } else {
                Err(PyException::type_error("string argument without an encoding"))
            }
        }
        PyObjectPayload::Int(n) => {
            let size = n.to_i64().unwrap_or(0) as usize;
            Ok(PyObject::bytearray(vec![0u8; size]))
        }
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
            let items = args[0].to_list()?;
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                let v = item.to_int().map_err(|_| PyException::type_error("an integer is required"))?;
                if v < 0 || v > 255 {
                    return Err(PyException::value_error("bytes must be in range(0, 256)"));
                }
                result.push(v as u8);
            }
            Ok(PyObject::bytearray(result))
        }
        _ => Err(PyException::type_error("cannot convert to bytearray")),
    }
}

pub(super) fn builtin_complex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() == 1 {
        if let PyObjectPayload::Str(s) = &args[0].payload {
            let s = s.trim().replace(" ", "");
            return parse_complex_string(&s);
        }
    }
    let real = if !args.is_empty() { args[0].to_float().unwrap_or(0.0) } else { 0.0 };
    let imag = if args.len() > 1 { args[1].to_float().unwrap_or(0.0) } else { 0.0 };
    Ok(PyObject::complex(real, imag))
}

fn parse_complex_string(s: &str) -> PyResult<PyObjectRef> {
    // Handle pure imaginary: "2j", "-3j"
    if s.ends_with('j') || s.ends_with('J') {
        let body = &s[..s.len()-1];
        // Pure imaginary like "2j"
        if let Ok(imag) = body.parse::<f64>() {
            return Ok(PyObject::complex(0.0, imag));
        }
        // "1+2j" or "1-2j"
        if let Some(pos) = body.rfind('+') {
            if pos > 0 {
                let real_s = &body[..pos];
                let imag_s = &body[pos+1..];
                if let (Ok(r), Ok(i)) = (real_s.parse::<f64>(), imag_s.parse::<f64>()) {
                    return Ok(PyObject::complex(r, i));
                }
            }
        }
        if let Some(pos) = body.rfind('-') {
            if pos > 0 {
                let real_s = &body[..pos];
                let imag_s = &body[pos..]; // includes the minus
                if let (Ok(r), Ok(i)) = (real_s.parse::<f64>(), imag_s.parse::<f64>()) {
                    return Ok(PyObject::complex(r, i));
                }
            }
        }
    }
    // Pure real
    if let Ok(r) = s.parse::<f64>() {
        return Ok(PyObject::complex(r, 0.0));
    }
    Err(PyException::value_error(format!("complex() arg is a malformed string: '{}'", s)))
}

pub(super) fn builtin_issubclass(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("issubclass", args, 2)?;
    let sub = &args[0];
    let sup = &args[1];
    // Handle tuple of types: issubclass(A, (B, C))
    if let PyObjectPayload::Tuple(types) = &sup.payload {
        for t in types {
            if check_subclass(sub, t) {
                return Ok(PyObject::bool_val(true));
            }
        }
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(check_subclass(sub, sup)))
}

pub(crate) fn check_subclass(sub: &PyObjectRef, sup: &PyObjectRef) -> bool {
    match (&sub.payload, &sup.payload) {
        (PyObjectPayload::Class(sub_cd), PyObjectPayload::Class(sup_cd)) => {
            if sub_cd.name == sup_cd.name { return true; }
            // Walk full MRO
            for base in &sub_cd.mro {
                if let PyObjectPayload::Class(bc) = &base.payload {
                    if bc.name == sup_cd.name { return true; }
                }
            }
            // Also check direct bases
            for base in &sub_cd.bases {
                if let PyObjectPayload::Class(bc) = &base.payload {
                    if bc.name == sup_cd.name { return true; }
                }
            }
            // Check _abc_registry for virtual subclass registration
            {
                let mut classes_to_check: Vec<PyObjectRef> = vec![sup.clone()];
                classes_to_check.extend(sup_cd.bases.iter().cloned());
                for check_cls in &classes_to_check {
                    if let PyObjectPayload::Class(ref check_cd) = check_cls.payload {
                        if let Some(registry) = check_cd.namespace.read().get("_abc_registry").cloned() {
                            if let PyObjectPayload::Dict(map) = &registry.payload {
                                for (k, _) in map.read().iter() {
                                    if let HashableKey::Identity(_, registered) = k {
                                        if Arc::ptr_eq(registered, sub) {
                                            return true;
                                        }
                                        if let PyObjectPayload::Class(rc) = &registered.payload {
                                            if rc.name == sub_cd.name {
                                                return true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            false
        }
        // Class inheriting from ExceptionType (e.g. class MyError(ValueError))
        (PyObjectPayload::Class(sub_cd), PyObjectPayload::ExceptionType(target_kind)) => {
            let _target_name = format!("{:?}", target_kind);
            // Check bases: is any base an ExceptionType matching target?
            for base in &sub_cd.bases {
                if let PyObjectPayload::ExceptionType(bk) = &base.payload {
                    if bk == target_kind { return true; }
                    // Check exception hierarchy
                    if is_exception_subclass(bk, target_kind) { return true; }
                }
                // Recursively check class bases
                if check_subclass(base, sup) { return true; }
            }
            false
        }
        (PyObjectPayload::ExceptionType(a), PyObjectPayload::ExceptionType(b)) => {
            a == b || is_exception_subclass(a, b)
        }
        // BuiltinType subclass (bool is subclass of int)
        (PyObjectPayload::BuiltinType(a), PyObjectPayload::BuiltinType(b)) => {
            a == b
            || (a.as_str() == "bool" && b.as_str() == "int")
            || b.as_str() == "object"  // everything is a subclass of object
        }
        // Any type is subclass of object
        (_, PyObjectPayload::BuiltinType(b)) if b.as_str() == "object" => true,
        // Class checking against BuiltinType: walk MRO for matching BuiltinType
        (PyObjectPayload::Class(sub_cd), PyObjectPayload::BuiltinType(target)) => {
            for base in &sub_cd.mro {
                if let PyObjectPayload::BuiltinType(bt) = &base.payload {
                    if bt == target { return true; }
                }
            }
            for base in &sub_cd.bases {
                if let PyObjectPayload::BuiltinType(bt) = &base.payload {
                    if bt == target { return true; }
                }
            }
            false
        }
        // BuiltinType vs ABC Class: check _abc_builtin_types registry
        (PyObjectPayload::BuiltinType(type_name), PyObjectPayload::Class(sup_cd)) => {
            if let Some(registry) = sup_cd.namespace.read().get("_abc_builtin_types") {
                if let PyObjectPayload::Set(set) = &registry.payload {
                    let key = HashableKey::Str(CompactString::from(type_name.as_str()));
                    return set.read().contains_key(&key);
                }
            }
            false
        }
        _ => false,
    }
}

/// Check if exception kind `child` is a subclass of `parent` in the hierarchy.
pub(crate) fn is_exception_subclass(child: &ExceptionKind, parent: &ExceptionKind) -> bool {
    child.is_subclass_of(parent)
}

pub(super) fn builtin_object(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::instance(PyObject::builtin_type(CompactString::from("object"))))
}

pub(super) fn builtin_super(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Simplified: return None for now
    Ok(PyObject::none())
}

/// dict.fromkeys(iterable, value=None) — create dict with keys from iterable
pub(super) fn builtin_dict_fromkeys(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("dict.fromkeys", args, 1)?;
    let iterable = &args[0];
    let value = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
    let mut map = IndexMap::new();
    match &iterable.payload {
        PyObjectPayload::List(items) => {
            for item in items.read().iter() {
                let hk = item.to_hashable_key()?;
                map.insert(hk, value.clone());
            }
        }
        PyObjectPayload::Tuple(items) => {
            for item in items.iter() {
                let hk = item.to_hashable_key()?;
                map.insert(hk, value.clone());
            }
        }
        PyObjectPayload::Set(items) => {
            for (item, _) in items.read().iter() {
                map.insert(item.clone(), value.clone());
            }
        }
        PyObjectPayload::Str(s) => {
            for ch in s.chars() {
                let hk = HashableKey::Str(CompactString::from(ch.to_string()));
                map.insert(hk, value.clone());
            }
        }
        PyObjectPayload::Dict(d) => {
            for key in d.read().keys() {
                map.insert(key.clone(), value.clone());
            }
        }
        _ => {
            return Err(PyException::type_error(format!(
                "'{}' object is not iterable", iterable.type_name()
            )));
        }
    }
    Ok(PyObject::dict(map))
}

pub(super) fn builtin_breakpoint(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Signal to the VM that breakpoint() was called.
    // The VM checks BREAKPOINT_TRIGGERED after each BuiltinFunction call.
    BREAKPOINT_TRIGGERED.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(PyObject::none())
}

/// Global flag for breakpoint() → VM communication.
pub(crate) static BREAKPOINT_TRIGGERED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(super) fn builtin_help(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        println!("Welcome to Ferrython help!");
        println!("Type help(object) for help about an object.");
        println!("Type help('topic') for help on a topic.");
        println!();
        println!("See https://docs.python.org/3/ for full Python documentation.");
        return Ok(PyObject::none());
    }

    let obj = &args[0];
    let type_name = obj.type_name();

    // Get the object's name
    let _name = obj.get_attr("__name__")
        .map(|n| n.py_to_string())
        .unwrap_or_else(|| type_name.to_string());

    // Get docstring
    let doc = obj.get_attr("__doc__")
        .map(|d| d.py_to_string())
        .unwrap_or_default();

    // Print header
    match &obj.payload {
        PyObjectPayload::Class(cd) => {
            println!("Help on class {}:", cd.name);
            println!();
            println!("class {}({})", cd.name,
                cd.bases.iter()
                    .filter_map(|b| b.get_attr("__name__").map(|n| n.py_to_string()))
                    .collect::<Vec<_>>().join(", "));
        }
        PyObjectPayload::Module(md) => {
            println!("Help on module {}:", md.name);
        }
        PyObjectPayload::Function(fd) => {
            println!("Help on function {}:", fd.name);
        }
        PyObjectPayload::BuiltinFunction(n) => {
            println!("Help on built-in function {}:", n);
        }
        _ => {
            println!("Help on {} object:", type_name);
        }
    }

    // Print docstring
    if !doc.is_empty() && doc != "None" {
        println!(" |  {}", doc.replace('\n', "\n |  "));
    }

    // Print methods for classes and modules
    match &obj.payload {
        PyObjectPayload::Class(cd) => {
            println!(" |");
            println!(" |  Methods defined here:");
            let ns = cd.namespace.read();
            let mut names: Vec<_> = ns.keys().collect();
            names.sort();
            for name in names {
                if name.starts_with("__") && name.ends_with("__") && name.len() > 4 {
                    continue; // Skip dunder methods in default view
                }
                let val = &ns[name];
                let method_doc = val.get_attr("__doc__")
                    .map(|d| d.py_to_string())
                    .unwrap_or_default();
                println!(" |  {}(self, ...)", name);
                if !method_doc.is_empty() && method_doc != "None" {
                    println!(" |      {}", method_doc.lines().next().unwrap_or(""));
                }
            }
        }
        PyObjectPayload::Module(md) => {
            println!(" |");
            println!(" |  Functions and classes:");
            let attrs = md.attrs.read();
            let mut names: Vec<_> = attrs.keys().collect();
            names.sort();
            for name in names {
                if name.starts_with("_") { continue; }
                let val = &attrs[name];
                let desc = match &val.payload {
                    PyObjectPayload::Function(_) => "function",
                    PyObjectPayload::Class(_) => "class",
                    PyObjectPayload::BuiltinFunction(_) => "built-in function",
                    _ => continue,
                };
                println!(" |  {} - {}", name, desc);
            }
        }
        _ => {}
    }
    println!();
    Ok(PyObject::none())
}

#[allow(non_snake_case)]
pub(super) fn builtin___import__(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("__import__() requires at least 1 argument"));
    }
    let name = args[0].py_to_string();
    // Store the import request for the VM to process
    // __import__(name, globals=None, locals=None, fromlist=(), level=0)
    let level = if args.len() >= 5 {
        args[4].as_int().unwrap_or(0) as usize
    } else {
        0
    };
    IMPORT_REQUEST.with(|r| {
        *r.borrow_mut() = Some(ImportRequest {
            name: CompactString::from(name),
            level,
        });
    });
    ferrython_core::object::set_intercept_pending();
    // Return a placeholder — the VM will replace this with the actual module
    Ok(PyObject::none())
}

/// Import request stored by __import__ for the VM to process.
pub(crate) struct ImportRequest {
    pub name: CompactString,
    pub level: usize,
}

thread_local! {
    pub(crate) static IMPORT_REQUEST: std::cell::RefCell<Option<ImportRequest>> = std::cell::RefCell::new(None);
}

pub(crate) fn take_import_request() -> Option<ImportRequest> {
    IMPORT_REQUEST.with(|r| r.borrow_mut().take())
}

pub(super) fn builtin_memoryview(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("memoryview", args, 1)?;
    match &args[0].payload {
        PyObjectPayload::Bytes(_) => {
            // Read-only memoryview — wrap as bytes (immutable)
            Ok(PyObject::bytes(match &args[0].payload {
                PyObjectPayload::Bytes(b) => b.clone(),
                _ => unreachable!(),
            }))
        }
        PyObjectPayload::ByteArray(_) => {
            // Mutable memoryview — keep as bytearray so __setitem__ works
            Ok(PyObject::bytearray(match &args[0].payload {
                PyObjectPayload::ByteArray(b) => b.clone(),
                _ => unreachable!(),
            }))
        }
        _ => Err(PyException::type_error(format!(
            "memoryview: a bytes-like object is required, not '{}'",
            args[0].type_name()
        ))),
    }
}
