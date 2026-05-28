//! Object comparison helpers.

use super::super::payload::*;
use super::{is_hidden_dict_key, range_len};
use crate::object::methods::PyObjectMethods;
use crate::types::PyInt;
use compact_str::CompactString;
use ferrython_bytecode::{CodeObject, ConstantValue};

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

#[inline]
pub fn partial_cmp_objects(a: &PyObjectRef, b: &PyObjectRef) -> Option<std::cmp::Ordering> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::None, PyObjectPayload::None) => Some(std::cmp::Ordering::Equal),
        (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => a.partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => a.to_f64().partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => a.partial_cmp(&b.to_f64()),
        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a.partial_cmp(b),
        (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => {
            PyInt::Small(*a as i64).partial_cmp(b)
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => {
            a.partial_cmp(&PyInt::Small(*b as i64))
        }
        (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => (*a as i64 as f64).partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => a.partial_cmp(&(*b as i64 as f64)),
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
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(n))
        | (PyObjectPayload::Int(n), PyObjectPayload::Complex { real, imag }) => {
            if *imag == 0.0
                && *real == n.to_f64()
                && n.to_i64().map(|i| (*real as i64) == i).unwrap_or(false)
            {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
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
            let bf = if *b { 1.0 } else { 0.0 };
            if *imag == 0.0 && *real == bf {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
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
            // CPython: ranges are equal if they produce the same sequence
            // Simple shortcut: normalize empty ranges
            let len1 = range_len(r1.start, r1.stop, r1.step);
            let len2 = range_len(r2.start, r2.stop, r2.step);
            if len1 == 0 && len2 == 0 {
                return Some(std::cmp::Ordering::Equal);
            }
            if len1 != len2 {
                return None;
            }
            if r1.start != r2.start {
                return None;
            }
            if len1 == 1 {
                return Some(std::cmp::Ordering::Equal);
            }
            if r1.step == r2.step {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::InstanceDict(a), PyObjectPayload::InstanceDict(b)) => {
            let a = a.read();
            let b = b.read();
            if a.len() != b.len() {
                return None;
            }
            for (k, v1) in a.iter() {
                match b.get(k.as_str()) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // Cross-type: InstanceDict == Dict
        (PyObjectPayload::InstanceDict(a), PyObjectPayload::Dict(b)) => {
            let a = a.read();
            let b = b.read();
            if a.len() != b.len() {
                return None;
            }
            for (k, v1) in a.iter() {
                let hk = match PyObject::str_val(CompactString::from(k.as_str())).to_hashable_key()
                {
                    Ok(hk) => hk,
                    Err(_) => return None,
                };
                match b.get(&hk) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Dict(a_dict), PyObjectPayload::InstanceDict(b_idict)) => {
            let a_r = a_dict.read();
            let b_r = b_idict.read();
            if a_r.len() != b_r.len() {
                return None;
            }
            for (k, v1) in b_r.iter() {
                let hk = match PyObject::str_val(CompactString::from(k.as_str())).to_hashable_key()
                {
                    Ok(hk) => hk,
                    Err(_) => return None,
                };
                match a_r.get(&hk) {
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
