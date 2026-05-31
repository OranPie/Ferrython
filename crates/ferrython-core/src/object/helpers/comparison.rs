//! Object comparison helpers.

use super::super::payload::*;
use super::{instance_dict_as_hashkey_map, is_hidden_dict_key, range_canonical_parts};
use crate::object::methods::PyObjectMethods;
use crate::types::{float_as_integer_ratio, PyInt};
use compact_str::CompactString;
use ferrython_bytecode::{CodeObject, ConstantValue};
use num_bigint::BigInt;
use num_traits::{One, Zero};

fn code_objects_equal(a: &CodeObject, b: &CodeObject) -> bool {
    a.instructions == b.instructions
        && code_constant_values_equal(&a.constants, &b.constants)
        && a.names == b.names
        && a.varnames == b.varnames
        && a.freevars == b.freevars
        && a.cellvars == b.cellvars
        && a.name == b.name
        && a.qualname == b.qualname
        && a.first_line_number == b.first_line_number
        && a.docstring == b.docstring
        && a.line_number_table == b.line_number_table
        && a.flags == b.flags
        && a.arg_count == b.arg_count
        && a.posonlyarg_count == b.posonlyarg_count
        && a.kwonlyarg_count == b.kwonlyarg_count
        && a.num_locals == b.num_locals
        && a.max_stack_size == b.max_stack_size
}

fn code_constant_values_equal(a: &[ConstantValue], b: &[ConstantValue]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(left, right)| code_constant_value_equal(left, right))
}

fn code_constant_value_equal(a: &ConstantValue, b: &ConstantValue) -> bool {
    match (a, b) {
        (ConstantValue::Code(a), ConstantValue::Code(b)) => code_objects_equal(a, b),
        (ConstantValue::Tuple(a), ConstantValue::Tuple(b))
        | (ConstantValue::FrozenSet(a), ConstantValue::FrozenSet(b)) => {
            code_constant_values_equal(a, b)
        }
        _ => a.bit_exact_eq(b),
    }
}

fn dict_maps_equal(a: &FxHashKeyMap, b: &FxHashKeyMap) -> bool {
    let od_key = crate::types::HashableKey::str_key(CompactString::from("__ordered_dict__"));
    let a_is_od = a.contains_key(&od_key);
    let b_is_od = b.contains_key(&od_key);
    if a_is_od && b_is_od {
        let a_items: Vec<_> = a.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
        let b_items: Vec<_> = b.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
        if a_items.len() != b_items.len() {
            return false;
        }
        for ((ak, av), (bk, bv)) in a_items.iter().zip(b_items.iter()) {
            if ak != bk {
                return false;
            }
            if partial_cmp_objects(av, bv) != Some(std::cmp::Ordering::Equal) {
                return false;
            }
        }
        true
    } else {
        let a_effective: Vec<_> = a.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
        let b_effective: Vec<_> = b.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
        if a_effective.len() != b_effective.len() {
            return false;
        }
        for (k, v1) in &a_effective {
            match b.get(*k) {
                Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                _ => return false,
            }
        }
        true
    }
}

fn object_rational_parts(obj: &PyObjectRef) -> Option<(BigInt, BigInt)> {
    match &obj.payload {
        PyObjectPayload::Bool(flag) => {
            Some((BigInt::from(if *flag { 1 } else { 0 }), BigInt::one()))
        }
        PyObjectPayload::Int(PyInt::Small(n)) => Some((BigInt::from(*n), BigInt::one())),
        PyObjectPayload::Int(PyInt::Big(n)) => Some((n.as_ref().clone(), BigInt::one())),
        PyObjectPayload::Float(f) if f.is_finite() => Some(float_as_integer_ratio(*f)),
        PyObjectPayload::Float(f) if f.is_infinite() => Some((
            if f.is_sign_negative() {
                -BigInt::one()
            } else {
                BigInt::one()
            },
            BigInt::zero(),
        )),
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            if attrs.contains_key("__fraction__") {
                let n = attrs.get("numerator").and_then(object_int_to_bigint)?;
                let d = attrs.get("denominator").and_then(object_int_to_bigint)?;
                Some((n, d))
            } else if attrs.contains_key("__decimal__") {
                attrs
                    .get("_value")
                    .and_then(|v| v.as_str())
                    .and_then(decimal_ratio)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn object_int_to_bigint(obj: &PyObjectRef) -> Option<BigInt> {
    match &obj.payload {
        PyObjectPayload::Bool(flag) => Some(BigInt::from(if *flag { 1 } else { 0 })),
        PyObjectPayload::Int(PyInt::Small(n)) => Some(BigInt::from(*n)),
        PyObjectPayload::Int(PyInt::Big(n)) => Some(n.as_ref().clone()),
        _ => None,
    }
}

fn decimal_ratio(s: &str) -> Option<(BigInt, BigInt)> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("nan") {
        return None;
    }
    if s.eq_ignore_ascii_case("infinity")
        || s.eq_ignore_ascii_case("+infinity")
        || s.eq_ignore_ascii_case("inf")
        || s.eq_ignore_ascii_case("+inf")
    {
        return Some((BigInt::one(), BigInt::zero()));
    }
    if s.eq_ignore_ascii_case("-infinity") || s.eq_ignore_ascii_case("-inf") {
        return Some((-BigInt::one(), BigInt::zero()));
    }
    let (negative, body) = if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else {
        (false, s)
    };
    let (mantissa, exp) =
        if let Some((m, e)) = body.split_once('e').or_else(|| body.split_once('E')) {
            (m, e.parse::<i64>().ok()?)
        } else {
            (body, 0)
        };
    let mut digits = String::new();
    let mut scale = 0i64;
    if let Some((int_part, frac_part)) = mantissa.split_once('.') {
        digits.push_str(int_part);
        digits.push_str(frac_part);
        scale = frac_part.len() as i64;
    } else {
        digits.push_str(mantissa);
    }
    if digits.is_empty() || digits.chars().all(|c| c == '0') {
        return Some((BigInt::zero(), BigInt::one()));
    }
    let mut numerator = digits.parse::<BigInt>().ok()?;
    if negative {
        numerator = -numerator;
    }
    let power = scale - exp;
    if power.abs() > 10_000 {
        if numerator.is_zero() {
            return Some((BigInt::zero(), BigInt::one()));
        }
        return None;
    }
    if power >= 0 {
        Some((numerator, BigInt::from(10u8).pow(power as u32)))
    } else {
        Some((
            numerator * BigInt::from(10u8).pow((-power) as u32),
            BigInt::one(),
        ))
    }
}

fn compare_rational_parts(
    left: (BigInt, BigInt),
    right: (BigInt, BigInt),
) -> Option<std::cmp::Ordering> {
    let (an, ad) = left;
    let (bn, bd) = right;
    if ad.is_zero() || bd.is_zero() {
        return if ad.is_zero() && bd.is_zero() {
            an.sign().partial_cmp(&bn.sign())
        } else if ad.is_zero() {
            if an.sign() == num_bigint::Sign::Minus {
                Some(std::cmp::Ordering::Less)
            } else {
                Some(std::cmp::Ordering::Greater)
            }
        } else if bn.sign() == num_bigint::Sign::Minus {
            Some(std::cmp::Ordering::Greater)
        } else {
            Some(std::cmp::Ordering::Less)
        };
    }
    Some((an * bd).cmp(&(bn * ad)))
}

fn complex_equal_rational(real: f64, imag: f64, other: &PyObjectRef) -> Option<std::cmp::Ordering> {
    if imag != 0.0 || real.is_nan() {
        return None;
    }
    let parts = object_rational_parts(other)?;
    compare_rational_parts(float_as_integer_ratio(real), parts)
        .filter(|ordering| *ordering == std::cmp::Ordering::Equal)
}

fn builtin_value(obj: &PyObjectRef) -> Option<PyObjectRef> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if inst.attrs.read().contains_key("__weakref_ref__") {
            return None;
        }
        return inst.attrs.read().get("__builtin_value__").cloned();
    }
    None
}

#[inline]
pub fn partial_cmp_objects(a: &PyObjectRef, b: &PyObjectRef) -> Option<std::cmp::Ordering> {
    if let Some(left) = builtin_value(a) {
        if matches!(
            (&left.payload, &b.payload),
            (PyObjectPayload::List(_), PyObjectPayload::List(_))
                | (PyObjectPayload::Set(_), PyObjectPayload::Set(_))
                | (PyObjectPayload::Set(_), PyObjectPayload::FrozenSet(_))
                | (PyObjectPayload::FrozenSet(_), PyObjectPayload::Set(_))
                | (PyObjectPayload::FrozenSet(_), PyObjectPayload::FrozenSet(_))
        ) {
            return partial_cmp_objects(&left, b);
        }
    }
    if let Some(right) = builtin_value(b) {
        if matches!(
            (&a.payload, &right.payload),
            (PyObjectPayload::List(_), PyObjectPayload::List(_))
                | (PyObjectPayload::Set(_), PyObjectPayload::Set(_))
                | (PyObjectPayload::Set(_), PyObjectPayload::FrozenSet(_))
                | (PyObjectPayload::FrozenSet(_), PyObjectPayload::Set(_))
                | (PyObjectPayload::FrozenSet(_), PyObjectPayload::FrozenSet(_))
        ) {
            return partial_cmp_objects(a, &right);
        }
    }

    match (&a.payload, &b.payload) {
        (PyObjectPayload::None, PyObjectPayload::None) => Some(std::cmp::Ordering::Equal),
        (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => a.partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => {
            if b.is_finite() {
                compare_rational_parts((a.to_bigint(), BigInt::one()), float_as_integer_ratio(*b))
            } else {
                a.to_f64().partial_cmp(b)
            }
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => {
            if a.is_finite() {
                compare_rational_parts(float_as_integer_ratio(*a), (b.to_bigint(), BigInt::one()))
            } else {
                a.partial_cmp(&b.to_f64())
            }
        }
        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a.partial_cmp(b),
        (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => {
            PyInt::Small(*a as i64).partial_cmp(b)
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => {
            a.partial_cmp(&PyInt::Small(*b as i64))
        }
        (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => (*a as i64 as f64).partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => a.partial_cmp(&(*b as i64 as f64)),
        (PyObjectPayload::Float(f), PyObjectPayload::Instance(_)) if f.is_infinite() => {
            match object_rational_parts(b) {
                Some(parts) => compare_rational_parts(object_rational_parts(a)?, parts),
                None => {
                    if *f == f64::NEG_INFINITY {
                        Some(std::cmp::Ordering::Less)
                    } else {
                        Some(std::cmp::Ordering::Greater)
                    }
                }
            }
        }
        (PyObjectPayload::Instance(_), PyObjectPayload::Float(f)) if f.is_infinite() => {
            match object_rational_parts(a) {
                Some(parts) => compare_rational_parts(parts, object_rational_parts(b)?),
                None => {
                    if *f == f64::NEG_INFINITY {
                        Some(std::cmp::Ordering::Greater)
                    } else {
                        Some(std::cmp::Ordering::Less)
                    }
                }
            }
        }
        (PyObjectPayload::Instance(_), PyObjectPayload::Float(_))
        | (PyObjectPayload::Float(_), PyObjectPayload::Instance(_))
        | (PyObjectPayload::Instance(_), PyObjectPayload::Int(_))
        | (PyObjectPayload::Int(_), PyObjectPayload::Instance(_))
        | (PyObjectPayload::Instance(_), PyObjectPayload::Bool(_))
        | (PyObjectPayload::Bool(_), PyObjectPayload::Instance(_)) => {
            match (object_rational_parts(a), object_rational_parts(b)) {
                (Some(left), Some(right)) => compare_rational_parts(left, right),
                _ => None,
            }
        }
        (PyObjectPayload::List(a), PyObjectPayload::List(b)) => {
            let a = a.read();
            let b = b.read();
            for (x, y) in a.iter().zip(b.iter()) {
                match partial_cmp_objects(x, y) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            a.len().partial_cmp(&b.len())
        }
        (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
            for (x, y) in a.iter().zip(b.iter()) {
                match partial_cmp_objects(x, y) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            a.len().partial_cmp(&b.len())
        }
        (PyObjectPayload::Slice(a), PyObjectPayload::Slice(b)) => {
            let a_items = [
                a.start.clone().unwrap_or_else(PyObject::none),
                a.stop.clone().unwrap_or_else(PyObject::none),
                a.step.clone().unwrap_or_else(PyObject::none),
            ];
            let b_items = [
                b.start.clone().unwrap_or_else(PyObject::none),
                b.stop.clone().unwrap_or_else(PyObject::none),
                b.step.clone().unwrap_or_else(PyObject::none),
            ];
            for (x, y) in a_items.iter().zip(b_items.iter()) {
                match partial_cmp_objects(x, y) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::BuiltinFunction(a), PyObjectPayload::BuiltinFunction(b)) => {
            if a == b {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeFunction(a), PyObjectPayload::NativeFunction(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeClosure(a), PyObjectPayload::NativeClosure(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Code(a), PyObjectPayload::Code(b)) => {
            if code_objects_equal(a, b) {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::BuiltinFunction(a), PyObjectPayload::NativeFunction(b)) => {
            if a.as_ref() == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeFunction(a), PyObjectPayload::BuiltinFunction(b)) => {
            if a.name == b.as_ref() {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::BuiltinFunction(a), PyObjectPayload::NativeClosure(b)) => {
            if a.as_ref() == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeClosure(a), PyObjectPayload::BuiltinFunction(b)) => {
            if a.name == b.as_ref() {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeFunction(a), PyObjectPayload::NativeClosure(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeClosure(a), PyObjectPayload::NativeFunction(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Bytes(a), PyObjectPayload::Bytes(b)) => a.partial_cmp(b),
        (PyObjectPayload::ByteArray(a), PyObjectPayload::ByteArray(b)) => a.partial_cmp(b),
        (PyObjectPayload::Bytes(a), PyObjectPayload::ByteArray(b))
        | (PyObjectPayload::ByteArray(a), PyObjectPayload::Bytes(b)) => a.partial_cmp(b),
        (
            PyObjectPayload::Complex { real: ar, imag: ai },
            PyObjectPayload::Complex { real: br, imag: bi },
        ) => {
            if ar == br && ai == bi {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(_)) => {
            complex_equal_rational(*real, *imag, b)
        }
        (PyObjectPayload::Int(_), PyObjectPayload::Complex { real, imag }) => {
            complex_equal_rational(*real, *imag, a)
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(f))
        | (PyObjectPayload::Float(f), PyObjectPayload::Complex { real, imag }) => {
            if *imag == 0.0 && real == f {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Bool(b))
        | (PyObjectPayload::Bool(b), PyObjectPayload::Complex { real, imag }) => {
            complex_equal_rational(*real, *imag, &PyObject::bool_val(*b))
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Instance(_)) => {
            complex_equal_rational(*real, *imag, b)
        }
        (PyObjectPayload::Instance(_), PyObjectPayload::Complex { real, imag }) => {
            complex_equal_rational(*real, *imag, a)
        }
        (PyObjectPayload::BuiltinType(a), PyObjectPayload::BuiltinType(b)) => {
            if a == b {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
            let a = a.read();
            let b = b.read();
            if a.len() != b.len() {
                return None;
            }
            for k in a.keys() {
                if !b.contains_key(k) {
                    return None;
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
            // Set equality: same keys
            if a.len() != b.len() {
                return None;
            }
            for k in a.keys() {
                if !b.contains_key(k) {
                    return None;
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // frozenset == set cross-type
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
            let rb = b.read();
            if a.len() != rb.len() {
                return None;
            }
            for k in a.keys() {
                if !rb.contains_key(k) {
                    return None;
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
            let ra = a.read();
            if ra.len() != b.len() {
                return None;
            }
            for k in ra.keys() {
                if !b.contains_key(k) {
                    return None;
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Dict(a), PyObjectPayload::Dict(b)) => {
            let a = a.read();
            let b = b.read();
            if dict_maps_equal(&a, &b) {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Dict(a), PyObjectPayload::MappingProxy(b))
        | (PyObjectPayload::MappingProxy(a), PyObjectPayload::Dict(b))
        | (PyObjectPayload::MappingProxy(a), PyObjectPayload::MappingProxy(b)) => {
            let a = a.read();
            let b = b.read();
            if dict_maps_equal(&a, &b) {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        // Class identity comparison (same Arc pointer = same class)
        (PyObjectPayload::Class(a), PyObjectPayload::Class(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        // ExceptionType comparison
        (PyObjectPayload::ExceptionType(a), PyObjectPayload::ExceptionType(b)) => {
            if a == b {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Range(r1), PyObjectPayload::Range(r2)) => {
            if range_canonical_parts(r1) == range_canonical_parts(r2) {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::InstanceDict(a), PyObjectPayload::InstanceDict(b)) => {
            let a = instance_dict_as_hashkey_map(a);
            let b = instance_dict_as_hashkey_map(b);
            if a.len() != b.len() {
                return None;
            }
            for (k, v1) in a.iter() {
                match b.get(k) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // Cross-type: InstanceDict == Dict
        (PyObjectPayload::InstanceDict(a), PyObjectPayload::Dict(b)) => {
            let a = instance_dict_as_hashkey_map(a);
            let b = b.read();
            if a.len() != b.len() {
                return None;
            }
            for (k, v1) in a.iter() {
                match b.get(k) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Dict(a_dict), PyObjectPayload::InstanceDict(b_idict)) => {
            let a_r = a_dict.read();
            let b_r = instance_dict_as_hashkey_map(b_idict);
            if a_r.len() != b_r.len() {
                return None;
            }
            for (k, v1) in b_r.iter() {
                match a_r.get(k) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // Instance comparison: check __eq__ method on class (for dataclass, custom __eq__)
        (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) => {
            // Check if they are the same object
            if PyObjectRef::ptr_eq(a, b) {
                return Some(std::cmp::Ordering::Equal);
            }
            if inst_a.attrs.read().contains_key("__weakref_ref__")
                || inst_b.attrs.read().contains_key("__weakref_ref__")
            {
                return None;
            }
            // Dict subclass: compare dict_storage contents
            if let (Some(ref ds_a), Some(ref ds_b)) = (&inst_a.dict_storage, &inst_b.dict_storage) {
                let a_r = ds_a.read();
                let b_r = ds_b.read();
                if a_r.len() != b_r.len() {
                    return None;
                }
                for (k, v1) in a_r.iter() {
                    match b_r.get(k) {
                        Some(v2)
                            if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                        _ => return None,
                    }
                }
                return Some(std::cmp::Ordering::Equal);
            }
            // Look for __eq__ in the class hierarchy
            fn find_in_mro(cls: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    let ns = cd.namespace.read();
                    if let Some(f) = ns.get(name) {
                        return Some(f.clone());
                    }
                    for base in &cd.mro {
                        if let PyObjectPayload::Class(bcd) = &base.payload {
                            let bns = bcd.namespace.read();
                            if let Some(f) = bns.get(name) {
                                return Some(f.clone());
                            }
                        }
                    }
                }
                None
            }
            if let Some(eq_fn) = find_in_mro(&inst_a.class, "__eq__") {
                match &eq_fn.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        if let Ok(result) = (nf.func)(&[a.clone(), b.clone()]) {
                            return if result.is_truthy() {
                                Some(std::cmp::Ordering::Equal)
                            } else {
                                None
                            };
                        }
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        if let Ok(result) = (nc.func)(&[a.clone(), b.clone()]) {
                            return if result.is_truthy() {
                                Some(std::cmp::Ordering::Equal)
                            } else {
                                None
                            };
                        }
                    }
                    _ => {}
                }
            }
            // For __lt__ comparison (used by sorted), also check
            if let Some(lt_fn) = find_in_mro(&inst_a.class, "__lt__") {
                match &lt_fn.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        if let Ok(result) = (nf.func)(&[a.clone(), b.clone()]) {
                            if result.is_truthy() {
                                return Some(std::cmp::Ordering::Less);
                            }
                        }
                        if let Ok(result) = (nf.func)(&[b.clone(), a.clone()]) {
                            if result.is_truthy() {
                                return Some(std::cmp::Ordering::Greater);
                            }
                        }
                        return Some(std::cmp::Ordering::Equal);
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        if let Ok(result) = (nc.func)(&[a.clone(), b.clone()]) {
                            if result.is_truthy() {
                                return Some(std::cmp::Ordering::Less);
                            }
                        }
                        if let Ok(result) = (nc.func)(&[b.clone(), a.clone()]) {
                            if result.is_truthy() {
                                return Some(std::cmp::Ordering::Greater);
                            }
                        }
                        return Some(std::cmp::Ordering::Equal);
                    }
                    _ => {}
                }
            }
            None
        }
        // Dict subclass (Instance with dict_storage) vs Dict
        (PyObjectPayload::Instance(inst), PyObjectPayload::Dict(b_dict)) => {
            if let Some(ref ds) = inst.dict_storage {
                let a_r = ds.read();
                let b_r = b_dict.read();
                if a_r.len() != b_r.len() {
                    return None;
                }
                for (k, v1) in a_r.iter() {
                    match b_r.get(k) {
                        Some(v2)
                            if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                        _ => return None,
                    }
                }
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Dict(a_dict), PyObjectPayload::Instance(inst)) => {
            if let Some(ref ds) = inst.dict_storage {
                let a_r = a_dict.read();
                let b_r = ds.read();
                if a_r.len() != b_r.len() {
                    return None;
                }
                for (k, v1) in a_r.iter() {
                    match b_r.get(k) {
                        Some(v2)
                            if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                        _ => return None,
                    }
                }
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        _ => None,
    }
}
