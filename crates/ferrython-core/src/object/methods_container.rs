//! Container operation methods (len, getitem, contains, iter).

use crate::error::{PyException, PyResult};
use crate::intern::intern_or_new;
use crate::types::{take_pending_eq_error, FrozenSetKeyData, HashableKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::rc::Rc;

use super::helpers::*;
use super::methods::{CompareOp, PyObjectMethods};
use super::payload::*;

fn instance_special_method(
    obj: &PyObjectRef,
    name: &str,
) -> Option<Result<PyObjectRef, PyException>> {
    obj.get_attr(name).map(|method| {
        if matches!(&method.payload, PyObjectPayload::None) {
            Err(PyException::type_error(format!(
                "'{}' object does not support {}",
                obj.type_name(),
                name
            )))
        } else {
            Ok(method)
        }
    })
}

fn ensure_iterator_result(owner: &PyObjectRef, iter: PyObjectRef) -> PyResult<PyObjectRef> {
    if iter.get_attr("__next__").is_some() {
        Ok(iter)
    } else {
        Err(PyException::type_error(format!(
            "iter() returned non-iterator of type '{}'",
            owner.type_name()
        )))
    }
}

fn element_matches(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
    if PyObjectRef::ptr_eq(a, b) {
        return Ok(true);
    }
    Ok(a.compare(b, CompareOp::Eq)?.is_truthy())
}

fn set_membership_key(obj: &PyObjectRef) -> PyResult<HashableKey> {
    match &obj.payload {
        PyObjectPayload::Set(items) => {
            let read = items.read();
            let mut keys: Vec<HashableKey> = read.keys().cloned().collect();
            keys.sort_by(|a, b| a.hash_key().cmp(&b.hash_key()));
            Ok(HashableKey::FrozenSet(Rc::new(FrozenSetKeyData::new(keys))))
        }
        PyObjectPayload::Instance(inst) => {
            let has_user_hash = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                cd.namespace.read().contains_key("__hash__")
            } else {
                false
            };
            if has_user_hash {
                if let Ok(key) = obj.to_hashable_key() {
                    return Ok(key);
                }
            }
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                if matches!(
                    &value.payload,
                    PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                ) {
                    return set_membership_key(&value);
                }
            }
            obj.to_hashable_key()
        }
        _ => obj.to_hashable_key(),
    }
}

fn visible_dict_storage_len(map: &FxHashKeyMap) -> usize {
    map.keys().filter(|k| !is_hidden_dict_key(k)).count()
}

fn chainmap_builtin_value(inst: &InstanceData) -> PyResult<Option<PyObjectRef>> {
    if !inst.attrs.read().contains_key("__chainmap__") {
        return Ok(None);
    }
    if std::env::var("FERR_DEBUG_CHAINMAP").is_ok() {
        eprintln!("chainmap: entering computed value path");
    }
    let maps = inst
        .attrs
        .read()
        .get("maps")
        .cloned()
        .ok_or_else(|| PyException::type_error("ChainMap missing maps"))?;
    let maps = maps.to_list()?;
    let mut combined = IndexMap::new();
    let mut seen = IndexMap::<HashableKey, ()>::new();
    for mapping in maps.iter().rev() {
        for key in mapping.to_list()? {
            let hk = key.to_hashable_key()?;
            if seen.insert(hk.clone(), ()).is_none() {
                let value = mapping.get_item(&key)?;
                combined.insert(hk, value);
            }
        }
    }
    Ok(Some(PyObject::dict(combined)))
}

pub(super) fn py_len(obj: &PyObjectRef) -> PyResult<usize> {
    match &obj.payload {
        PyObjectPayload::Str(s) => Ok(s.chars().count()),
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.len()),
        PyObjectPayload::List(v) => Ok(v.read().len()),
        PyObjectPayload::Deque(v) => Ok(v.read().len()),
        PyObjectPayload::Tuple(v) => Ok(v.len()),
        PyObjectPayload::Set(m) => Ok(m.read().len()),
        PyObjectPayload::FrozenSet(m) => Ok(m.len()),
        PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => {
            let map = m.read();
            let hidden = map.keys().filter(|k| is_hidden_dict_key(k)).count();
            Ok(map.len() - hidden)
        }
        PyObjectPayload::Instance(inst) => {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                    return py_len(&(nc.func)(&[])?);
                }
            }
            if inst.attrs.read().contains_key("__chainmap__") {
                if let Some(len_method) = inst.class.get_attr("__len__") {
                    let result = match &len_method.payload {
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[obj.clone()])?,
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[obj.clone()])?,
                        PyObjectPayload::BoundMethod { receiver, method } => {
                            match &method.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    (nc.func)(&[receiver.clone()])?
                                }
                                PyObjectPayload::NativeFunction(nf) => {
                                    (nf.func)(&[receiver.clone()])?
                                }
                                _ => PyObject::none(),
                            }
                        }
                        _ => PyObject::none(),
                    };
                    if let Some(n) = result.as_int() {
                        return Ok(n as usize);
                    }
                }
            }
            if inst.attrs.read().contains_key("__deque__") {
                if let Some(data) = inst.attrs.read().get("_data").cloned() {
                    return py_len(&data);
                }
                return Ok(0);
            }
            if inst.class.get_attr("__namedtuple__").is_some() {
                if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                    return py_len(&tup);
                }
            }
            if let Some(ref ds) = inst.dict_storage {
                return Ok(visible_dict_storage_len(&ds.read()));
            }
            if let Some(method) = instance_special_method(obj, "__len__") {
                let method = method?;
                if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                    let result = call_callable(&method, &[])?;
                    if let Some(n) = result.as_int() {
                        return Ok(n as usize);
                    }
                }
            }
            if let Some(bv) = chainmap_builtin_value(inst)? {
                return py_len(&bv);
            }
            // Builtin base type subclass: delegate to __builtin_value__
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                return py_len(&bv);
            }
            Err(PyException::type_error(format!(
                "object of type '{}' has no len()",
                obj.type_name()
            )))
        }
        PyObjectPayload::Class(cd) => {
            // Support len() on classes with __len__ (e.g., Enum)
            // Check own namespace and MRO
            let len_fn = {
                let ns = cd.namespace.read();
                let mut found = ns.get("__len__").cloned();
                if found.is_none() {
                    for base in &cd.mro {
                        if let PyObjectPayload::Class(bcd) = &base.payload {
                            let bns = bcd.namespace.read();
                            if let Some(f) = bns.get("__len__") {
                                found = Some(f.clone());
                                break;
                            }
                        }
                    }
                }
                found
            };
            if let Some(len_method) = len_fn {
                let result = call_callable(&len_method, &[obj.clone()])?;
                if let Some(n) = result.as_int() {
                    return Ok(n as usize);
                }
            }
            if let Some(meta) = &cd.metaclass {
                if let PyObjectPayload::Class(meta_cd) = &meta.payload {
                    if let Some(len_method) = meta_cd.namespace.read().get("__len__").cloned() {
                        let bound = PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: obj.clone(),
                                method: len_method,
                            },
                        });
                        let result = call_callable(&bound, &[])?;
                        if let Some(n) = result.as_int() {
                            return Ok(n as usize);
                        }
                    }
                }
            }
            Err(PyException::type_error(format!(
                "object of type '{}' has no len()",
                obj.type_name()
            )))
        }
        PyObjectPayload::Range(rd) => {
            let len = range_data_len_bigint(rd);
            if len > BigInt::from(isize::MAX) {
                Err(PyException::overflow_error(
                    "Python int too large to convert to C ssize_t",
                ))
            } else {
                Ok(len.to_usize().unwrap_or(0))
            }
        }
        PyObjectPayload::Iterator(iter_data) => {
            let data = iter_data.read();
            match &*data {
                IteratorData::Range {
                    current,
                    stop,
                    step,
                } => {
                    let len = range_len_i128(*current, *stop, *step);
                    if len > isize::MAX as i128 {
                        Err(PyException::overflow_error(
                            "Python int too large to convert to C ssize_t",
                        ))
                    } else {
                        Ok(len as usize)
                    }
                }
                IteratorData::BigRange(iter) => {
                    let len = range_iter_len_bigint(iter);
                    if len > BigInt::from(isize::MAX) {
                        Err(PyException::overflow_error(
                            "Python int too large to convert to C ssize_t",
                        ))
                    } else {
                        Ok(len.to_usize().unwrap_or(0))
                    }
                }
                IteratorData::List { items, index } => Ok(items.len() - index),
                IteratorData::Tuple { items, index } => Ok(items.len() - index),
                IteratorData::Str { chars, index } => Ok(chars.len() - index),
                IteratorData::SetRefs {
                    source,
                    items,
                    index,
                    expected_len,
                } => {
                    let len = source.read().len();
                    if len != *expected_len {
                        Err(PyException::runtime_error(
                            "Set changed size during iteration",
                        ))
                    } else {
                        Ok(items.len().saturating_sub(*index))
                    }
                }
                IteratorData::FrozenSetItems { items, index } => Ok(items.len() - index),
                _ => Err(PyException::type_error(
                    "object of type 'iterator' has no len()",
                )),
            }
        }
        PyObjectPayload::RangeIter(ri) => {
            let len = range_len_i128(ri.current.get(), ri.stop, ri.step);
            if len > isize::MAX as i128 {
                Err(PyException::overflow_error(
                    "Python int too large to convert to C ssize_t",
                ))
            } else {
                Ok(len as usize)
            }
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            Ok(if idx < data.items.len() {
                data.items.len() - idx
            } else {
                0
            })
        }
        PyObjectPayload::DictValueIter(data) => {
            let map = data.source.read();
            if map.len() != data.expected_len
                || dict_storage_version(&data.source) != data.expected_version
            {
                return Err(PyException::runtime_error(
                    "dictionary changed size during iteration",
                ));
            }
            let idx = data.index.get();
            Ok(if idx < map.len() { map.len() - idx } else { 0 })
        }
        PyObjectPayload::WeakValueIter(data) => {
            let idx = data.index.get();
            Ok(if idx < data.entries.len() {
                data.entries.len() - idx
            } else {
                0
            })
        }
        PyObjectPayload::WeakKeyIter(data) => {
            let idx = data.index.get();
            Ok(if idx < data.entries.len() {
                data.entries.len() - idx
            } else {
                0
            })
        }
        PyObjectPayload::DequeIter(data) => {
            let idx = data.index.get();
            if idx == usize::MAX || idx >= data.expected_len {
                Ok(0)
            } else {
                Ok(data.expected_len - idx)
            }
        }
        PyObjectPayload::RefIter { source, index, .. } => {
            if index.get() == usize::MAX {
                return Ok(0);
            }
            let idx = index.get();
            let total = match &source.payload {
                PyObjectPayload::List(cell) => unsafe { &*cell.data_ptr() }.len(),
                PyObjectPayload::Tuple(items) => items.len(),
                _ => 0,
            };
            Ok(if idx < total { total - idx } else { 0 })
        }
        PyObjectPayload::RevRefIter { .. } => Err(PyException::type_error(
            "object of type 'list_reverseiterator' has no len()",
        )),
        PyObjectPayload::DictKeys { map: m, .. }
        | PyObjectPayload::DictValues { map: m, .. }
        | PyObjectPayload::DictItems { map: m, .. } => {
            let map = m.read();
            let hidden = map.keys().filter(|k| is_hidden_dict_key(k)).count();
            Ok(map.len() - hidden)
        }
        PyObjectPayload::InstanceDict(attrs) => Ok(instance_dict_visible_len(attrs)),
        _ => Err(PyException::type_error(format!(
            "object of type '{}' has no len()",
            obj.type_name()
        ))),
    }
}

pub(super) fn py_get_item(obj: &PyObjectRef, key: &PyObjectRef) -> PyResult<PyObjectRef> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if inst.attrs.read().contains_key("__memoryview__") {
            if let Some(base) = inst.attrs.read().get("obj").cloned() {
                return py_get_item(&base, key);
            }
        }
    }
    // Check for slice key first
    if let PyObjectPayload::Slice(sd) = &key.payload {
        return get_slice_impl(obj, &sd.start, &sd.stop, &sd.step);
    }
    match &obj.payload {
        PyObjectPayload::List(items) => {
            let items = items.read();
            let idx = index_to_i64(key).map_err(|e| {
                if e.kind == crate::error::ExceptionKind::OverflowError {
                    PyException::index_error(e.message)
                } else {
                    PyException::type_error("list indices must be integers or slices")
                }
            })?;
            let len = items.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("list index out of range"));
            }
            Ok(items[actual as usize].clone())
        }
        PyObjectPayload::Deque(items) => {
            let items = items.read();
            let idx = index_to_i64(key).map_err(|e| {
                if e.kind == crate::error::ExceptionKind::OverflowError {
                    PyException::index_error(e.message)
                } else {
                    PyException::type_error("deque indices must be integers or slices")
                }
            })?;
            let len = items.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("deque index out of range"));
            }
            Ok(items[actual as usize].clone())
        }
        PyObjectPayload::Tuple(items) => {
            let idx = index_to_i64(key).map_err(|e| {
                if e.kind == crate::error::ExceptionKind::OverflowError {
                    PyException::index_error(e.message)
                } else {
                    PyException::type_error("tuple indices must be integers or slices")
                }
            })?;
            let len = items.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("tuple index out of range"));
            }
            Ok(items[actual as usize].clone())
        }
        PyObjectPayload::Dict(map) => {
            let hk = key.to_hashable_key()?;
            if is_hidden_dict_key(&hk) {
                return Err(PyException::key_error_value(key.clone()));
            }
            let map_r = map.read();
            if let Some(val) = map_r.get(&hk) {
                if let Some(err) = take_pending_eq_error() {
                    if matches!(hk, HashableKey::Custom { .. }) {
                        return Err(err);
                    }
                }
                return Ok(val.clone());
            }
            if let Some(err) = take_pending_eq_error() {
                if matches!(hk, HashableKey::Custom { .. }) {
                    return Err(err);
                }
            }
            // Check for __defaultdict_factory__ (Counter / defaultdict)
            let factory_key = HashableKey::str_key(intern_or_new("__defaultdict_factory__"));
            if let Some(factory) = map_r.get(&factory_key) {
                let factory = factory.clone();
                drop(map_r);
                // Create default value by "calling" the factory
                // For common factories: int -> 0, list -> [], str -> "", float -> 0.0
                let default = match &factory.payload {
                    PyObjectPayload::BuiltinType(name) => match name.as_str() {
                        "int" => PyObject::int(0),
                        "float" => PyObject::float(0.0),
                        "str" => PyObject::str_val(CompactString::new("")),
                        "list" => PyObject::list(vec![]),
                        "bool" => PyObject::bool_val(false),
                        "tuple" => PyObject::tuple(vec![]),
                        "set" => PyObject::set(new_fx_hashkey_map()),
                        "dict" => PyObject::dict(new_fx_hashkey_map()),
                        _ => return Err(PyException::key_error_value(key.clone())),
                    },
                    _ => return Err(PyException::key_error_value(key.clone())),
                };
                // Store the default value
                map.write().insert(hk, default.clone());
                return Ok(default);
            }
            Err(PyException::key_error_value(key.clone()))
        }
        PyObjectPayload::MappingProxy(map) => {
            let hk = key.to_hashable_key()?;
            if let Some(val) = map.read().get(&hk) {
                if let Some(err) = take_pending_eq_error() {
                    if matches!(hk, HashableKey::Custom { .. }) {
                        return Err(err);
                    }
                }
                return Ok(val.clone());
            }
            if let Some(err) = take_pending_eq_error() {
                if matches!(hk, HashableKey::Custom { .. }) {
                    return Err(err);
                }
            }
            Err(PyException::key_error_value(key.clone()))
        }
        PyObjectPayload::Str(s) => {
            let idx = index_to_i64(key).map_err(|e| {
                if e.kind == crate::error::ExceptionKind::OverflowError {
                    PyException::index_error(e.message)
                } else {
                    e
                }
            })?;
            if s.as_str().is_ascii() {
                let len = s.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len {
                    return Err(PyException::index_error("string index out of range"));
                }
                return Ok(PyObject::str_char(s.as_str().as_bytes()[actual as usize]));
            }
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("string index out of range"));
            }
            Ok(PyObject::str_val(CompactString::from(
                chars[actual as usize].to_string(),
            )))
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            let idx = index_to_i64(key).map_err(|e| {
                if e.kind == crate::error::ExceptionKind::OverflowError {
                    PyException::index_error(e.message)
                } else if e.kind == crate::error::ExceptionKind::TypeError {
                    let message = if matches!(&obj.payload, PyObjectPayload::ByteArray(_)) {
                        "bytearray indices must be integers or slices"
                    } else {
                        "byte indices must be integers or slices"
                    };
                    PyException::type_error(message)
                } else {
                    e
                }
            })?;
            let len = b.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("index out of range"));
            }
            Ok(PyObject::int(b[actual as usize] as i64))
        }
        PyObjectPayload::InstanceDict(attrs) => {
            if let Some(value) = instance_dict_get_item(attrs, key)? {
                Ok(value)
            } else {
                Err(PyException::key_error(key.repr()))
            }
        }
        PyObjectPayload::Range(rd) => {
            let idx = match key.to_index()? {
                PyInt::Small(n) => BigInt::from(n),
                PyInt::Big(n) => n.as_ref().clone(),
            };
            let len = range_data_len_bigint(rd);
            let actual = if idx.is_negative() { &len + idx } else { idx };
            if actual.is_negative() || actual >= len {
                return Err(PyException::index_error("range object index out of range"));
            }
            let value = range_item_bigint(rd, &actual);
            if let Some(value) = value.to_i64() {
                Ok(PyObject::int(value))
            } else {
                Ok(PyObject::big_int(value))
            }
        }
        PyObjectPayload::Instance(inst) => {
            if inst.attrs.read().contains_key("__deque__") {
                if let Some(data) = inst.attrs.read().get("_data").cloned() {
                    return py_get_item(&data, key);
                }
                return Err(PyException::index_error("deque index out of range"));
            }
            if let Some(method) = instance_special_method(obj, "__getitem__") {
                let method = method?;
                if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                    return call_callable(&method, &[key.clone()]);
                }
            }
            if let Some(bv) = chainmap_builtin_value(inst)? {
                return py_get_item(&bv, key);
            }
            if inst.class.get_attr("__namedtuple__").is_some() {
                if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                    return py_get_item(&tup, key);
                }
            }
            if let Some(storage) = inst.dict_storage.as_ref() {
                let hk = key.to_hashable_key()?;
                if is_hidden_dict_key(&hk) {
                    return Err(PyException::key_error_value(key.clone()));
                }
                if let Some(value) = storage.read().get(&hk).cloned() {
                    return Ok(value);
                }
                if let Some(method) = instance_class_special_method(obj, inst, "__missing__") {
                    if !matches!(
                        &method.payload,
                        PyObjectPayload::None | PyObjectPayload::BuiltinBoundMethod(_)
                    ) {
                        return call_callable(&method, &[key.clone()]);
                    }
                }
                return Err(PyException::key_error(key.repr()));
            }
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                return py_get_item(&bv, key);
            }
            Err(PyException::type_error(format!(
                "'{}' object is not subscriptable",
                obj.type_name()
            )))
        }
        _ => Err(PyException::type_error(format!(
            "'{}' object is not subscriptable",
            obj.type_name()
        ))),
    }
}

pub(super) fn py_contains(obj: &PyObjectRef, item: &PyObjectRef) -> PyResult<bool> {
    match &obj.payload {
        PyObjectPayload::List(v) => {
            let items = v.read().clone();
            for candidate in items {
                if element_matches(&candidate, item)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        PyObjectPayload::Deque(v) => {
            let items: Vec<_> = v.read().iter().cloned().collect();
            for candidate in items {
                if element_matches(&candidate, item)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        PyObjectPayload::Tuple(v) => {
            for x in v.iter() {
                if element_matches(x, item)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        PyObjectPayload::Str(haystack) => {
            if let Some(needle) = item.as_str() {
                Ok(haystack.contains(needle))
            } else {
                Err(PyException::type_error(
                    "'in <string>' requires string as left operand",
                ))
            }
        }
        PyObjectPayload::Set(m) => {
            let hk = set_membership_key(item)?;
            let contains = m.read().contains_key(&hk);
            if let Some(err) = take_pending_eq_error() {
                return Err(err);
            }
            Ok(contains)
        }
        PyObjectPayload::FrozenSet(m) => {
            let hk = set_membership_key(item)?;
            let contains = m.contains_key(&hk);
            if let Some(err) = take_pending_eq_error() {
                return Err(err);
            }
            Ok(contains)
        }
        PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => {
            let hk = item.to_hashable_key()?;
            if is_hidden_dict_key(&hk) {
                return Ok(false);
            }
            let contains = m.read().contains_key(&hk);
            if let Some(err) = take_pending_eq_error() {
                if matches!(hk, HashableKey::Custom { .. }) {
                    return Err(err);
                }
            }
            Ok(contains)
        }
        PyObjectPayload::Instance(inst) => {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                    return py_contains(&(nc.func)(&[])?, item);
                }
            }
            if let Some(bv) = chainmap_builtin_value(inst)? {
                return py_contains(&bv, item);
            }
            if let Some(method) = instance_special_method(obj, "__contains__") {
                let method = method?;
                if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                    return Ok(call_callable(&method, &[item.clone()])?.is_truthy());
                }
            }
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                return py_contains(&bv, item);
            }
            if inst.dict_storage.is_some() {
                let hk = item.to_hashable_key()?;
                if is_hidden_dict_key(&hk) {
                    return Ok(false);
                }
                if let Some(storage) = inst.dict_storage.as_ref() {
                    return Ok(storage.read().contains_key(&hk));
                }
            }
            match py_get_iter(obj) {
                Ok(iter_obj) => {
                    for next in iter_obj.to_list()? {
                        if element_matches(&next, item)? {
                            return Ok(true);
                        }
                    }
                    return Ok(false);
                }
                Err(err) => return Err(err),
            }
        }
        PyObjectPayload::InstanceDict(attrs) => Ok(instance_dict_get_item(attrs, item)?.is_some()),
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            // Support: int in bytes (single byte) or bytes in bytes (subsequence)
            match &item.payload {
                PyObjectPayload::Int(n) => {
                    let val = n.to_i64().unwrap_or(-1);
                    if val < 0 || val > 255 {
                        return Ok(false);
                    }
                    Ok(b.contains(&(val as u8)))
                }
                PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) => {
                    if needle.is_empty() {
                        return Ok(true);
                    }
                    Ok(b.windows(needle.len()).any(|w| w == needle.as_slice()))
                }
                _ => Err(PyException::type_error("a bytes-like object is required")),
            }
        }
        PyObjectPayload::Range(rd) => {
            if let Some(val) = py_exact_numeric_bigint(item) {
                Ok(range_contains_bigint(rd, &val))
            } else {
                let len = range_data_len_bigint(rd);
                if len > BigInt::from(1024usize) {
                    return Ok(false);
                }
                let mut idx = BigInt::zero();
                while idx < len {
                    let value = range_item_bigint(rd, &idx);
                    let candidate = if let Some(value) = value.to_i64() {
                        PyObject::int(value)
                    } else {
                        PyObject::big_int(value)
                    };
                    if element_matches(&candidate, item)? {
                        return Ok(true);
                    }
                    idx += 1;
                }
                Ok(false)
            }
        }
        PyObjectPayload::Iterator(iter_data) => {
            let data = iter_data.read();
            match &*data {
                IteratorData::Range {
                    current,
                    stop,
                    step,
                } => {
                    if let Some(val) = item.as_int() {
                        let val = val as i128;
                        let current = *current as i128;
                        let stop = *stop as i128;
                        let step = *step as i128;
                        if step > 0 {
                            Ok(val >= current && val < stop && (val - current) % step == 0)
                        } else {
                            Ok(val <= current && val > stop && (current - val) % (-step) == 0)
                        }
                    } else {
                        Ok(false)
                    }
                }
                IteratorData::BigRange(iter) => {
                    if let Some(val) = py_exact_numeric_bigint(item) {
                        let current = range_iter_item_bigint(iter);
                        let rd =
                            range_data_from_bigints(current, iter.stop.clone(), iter.step.clone());
                        Ok(range_contains_bigint(&rd, &val))
                    } else {
                        Ok(false)
                    }
                }
                _ => {
                    drop(data);
                    let items = obj.to_list()?;
                    for x in items.iter() {
                        if element_matches(x, item)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
            }
        }
        PyObjectPayload::RangeIter(ri) => {
            if let Some(val) = item.as_int() {
                let val = val as i128;
                let cur = ri.current.get() as i128;
                let stop = ri.stop as i128;
                let step = ri.step as i128;
                if ri.step > 0 {
                    Ok(val >= cur && val < stop && (val - cur) % step == 0)
                } else {
                    Ok(val <= cur && val > stop && (cur - val) % (-step) == 0)
                }
            } else {
                Ok(false)
            }
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            if idx >= data.items.len() {
                return Ok(false);
            }
            for x in &data.items[idx..] {
                if element_matches(x, item)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        PyObjectPayload::RefIter { source, index, .. } => {
            if index.get() == usize::MAX {
                return Ok(false);
            }
            let idx = index.get();
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let mut pos = idx;
                    loop {
                        let candidate = {
                            let items = unsafe { &*cell.data_ptr() };
                            if pos >= items.len() {
                                return Ok(false);
                            }
                            items[pos].clone()
                        };
                        if element_matches(&candidate, item)? {
                            return Ok(true);
                        }
                        pos += 1;
                    }
                }
                PyObjectPayload::Tuple(items) => {
                    if idx >= items.len() {
                        return Ok(false);
                    }
                    for x in &items[idx..] {
                        if element_matches(x, item)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                _ => Ok(false),
            }
        }
        PyObjectPayload::RevRefIter { source, index, .. } => {
            let idx = index.get();
            if idx == usize::MAX || idx == 0 {
                return Ok(false);
            }
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let mut pos = idx;
                    while pos > 0 {
                        pos -= 1;
                        let candidate = {
                            let items = unsafe { &*cell.data_ptr() };
                            if pos >= items.len() {
                                continue;
                            }
                            items[pos].clone()
                        };
                        if element_matches(&candidate, item)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                _ => Ok(false),
            }
        }
        PyObjectPayload::DictKeys { map: m, .. } => {
            let hk = item.to_hashable_key()?;
            let contains = m.read().contains_key(&hk);
            if let Some(err) = take_pending_eq_error() {
                return Err(err);
            }
            Ok(contains)
        }
        PyObjectPayload::DictValues { map: m, .. } => {
            let values: Vec<PyObjectRef> = m.read().values().cloned().collect();
            for value in values {
                if element_matches(&value, item)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        PyObjectPayload::DictItems { map: m, .. } => {
            // item should be a (key, value) tuple
            if let PyObjectPayload::Tuple(pair) = &item.payload {
                if pair.len() == 2 {
                    let hk = pair[0].to_hashable_key()?;
                    let val = {
                        let r = m.read();
                        r.get(&hk).cloned()
                    };
                    if let Some(err) = take_pending_eq_error() {
                        return Err(err);
                    }
                    if let Some(val) = val {
                        return element_matches(&val, &pair[1]);
                    }
                }
            }
            Ok(false)
        }
        _ => Err(PyException::type_error(format!(
            "argument of type '{}' is not iterable",
            obj.type_name()
        ))),
    }
}

pub(super) fn py_get_iter(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
            Ok(PyObject::wrap(PyObjectPayload::RefIter {
                source: obj.clone(),
                index: SyncUsize::new(0),
            }))
        }
        PyObjectPayload::Str(s) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::Str {
                chars: s.chars().collect(),
                index: 0,
            }),
        )))),
        PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => Ok(PyObject::wrap(
            PyObjectPayload::Iterator(Rc::new(PyCell::new(IteratorData::DictKeyRefs {
                source: map.clone(),
                index: 0,
                expected_len: map.read().len(),
                expected_version: dict_storage_version(map),
            }))),
        )),
        PyObjectPayload::InstanceDict(attrs) => {
            let map = Rc::new(PyCell::new(instance_dict_as_hashkey_map(attrs)));
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::DictKeyRefs {
                    source: map.clone(),
                    index: 0,
                    expected_len: map.read().len(),
                    expected_version: dict_storage_version(&map),
                }),
            ))))
        }
        PyObjectPayload::Set(m) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::set_refs(m)),
        )))),
        PyObjectPayload::FrozenSet(m) => {
            let vals: Vec<PyObjectRef> = m.values().cloned().collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::FrozenSetItems {
                    items: vals,
                    index: 0,
                }),
            ))))
        }
        PyObjectPayload::Range(rd) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(range_iterator_from_data(rd)),
        )))),
        PyObjectPayload::Iterator(_)
        | PyObjectPayload::RangeIter(..)
        | PyObjectPayload::VecIter(_)
        | PyObjectPayload::DictValueIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::DequeIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. } => Ok(obj.clone()),
        PyObjectPayload::Generator(_) => Ok(obj.clone()), // generators are their own iterators
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            let items: Vec<PyObjectRef> =
                b.iter().map(|byte| PyObject::int(*byte as i64)).collect();
            Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(
                VecIterData {
                    items,
                    index: SyncUsize::new(0),
                },
            ))))
        }
        PyObjectPayload::Instance(inst) => {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                    return py_get_iter(&(nc.func)(&[])?);
                }
            }
            if inst.attrs.read().contains_key("__deque__") {
                if let Some(method) = instance_special_method(obj, "__iter__") {
                    let method = method?;
                    return ensure_iterator_result(obj, call_callable(&method, &[])?);
                }
            }
            if inst.attrs.read().contains_key("__chainmap__") {
                if let Some(iter_method) = inst.class.get_attr("__iter__") {
                    let result = match &iter_method.payload {
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[obj.clone()])?,
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[obj.clone()])?,
                        PyObjectPayload::BoundMethod { receiver, method } => {
                            match &method.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    (nc.func)(&[receiver.clone()])?
                                }
                                PyObjectPayload::NativeFunction(nf) => {
                                    (nf.func)(&[receiver.clone()])?
                                }
                                _ => PyObject::none(),
                            }
                        }
                        _ => PyObject::none(),
                    };
                    return py_get_iter(&result);
                }
            }
            if let Some(method) = instance_special_method(obj, "__iter__") {
                let method = method?;
                if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                    return ensure_iterator_result(obj, call_callable(&method, &[])?);
                }
            }
            if let Some(bv) = chainmap_builtin_value(inst)? {
                return py_get_iter(&bv);
            }
            if inst.class.get_attr("__namedtuple__").is_some() {
                if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                    return py_get_iter(&tup);
                }
            }
            if let Some(ds) = inst.dict_storage.as_ref() {
                let iter = PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
                    IteratorData::DictKeyRefs {
                        source: ds.clone(),
                        index: 0,
                        expected_len: ds.read().len(),
                        expected_version: dict_storage_version(ds),
                    },
                ))));
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::HeldIter {
                        iter,
                        owner: Some(obj.clone()),
                    }),
                ))));
            }
            // Builtin base type subclass: delegate to __builtin_value__
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                let iter = py_get_iter(&bv)?;
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::HeldIter {
                        iter,
                        owner: Some(obj.clone()),
                    }),
                ))));
            }
            if let Some(method) = instance_special_method(obj, "__getitem__") {
                method?;
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::SeqIter {
                        obj: obj.clone(),
                        index: 0,
                        exhausted: false,
                    }),
                ))));
            }
            Err(PyException::type_error(format!(
                "'{}' object is not iterable",
                obj.type_name()
            )))
        }
        PyObjectPayload::DictKeys { map, owner } => {
            let iter = PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
                IteratorData::DictKeyRefs {
                    source: map.clone(),
                    index: 0,
                    expected_len: map.read().len(),
                    expected_version: dict_storage_version(map),
                },
            ))));
            if let Some(owner) = owner.clone() {
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::HeldIter {
                        iter,
                        owner: Some(owner),
                    }),
                ))))
            } else {
                Ok(iter)
            }
        }
        PyObjectPayload::DictValues { map: m, owner } => {
            let iter = PyObject::wrap(PyObjectPayload::DictValueIter(Box::new(
                DictValueIterData {
                    source: m.clone(),
                    index: SyncUsize::new(0),
                    expected_len: m.read().len(),
                    expected_version: dict_storage_version(m),
                },
            )));
            if let Some(owner) = owner.clone() {
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::HeldIter {
                        iter,
                        owner: Some(owner),
                    }),
                ))))
            } else {
                Ok(iter)
            }
        }
        PyObjectPayload::DictItems { map: m, owner } => {
            let iter = PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
                IteratorData::DictEntries {
                    source: m.clone(),
                    owner: None,
                    index: 0,
                    expected_len: m.read().len(),
                    expected_version: dict_storage_version(m),
                    cached_tuple: None,
                },
            ))));
            if let Some(owner) = owner.clone() {
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::HeldIter {
                        iter,
                        owner: Some(owner),
                    }),
                ))))
            } else {
                Ok(iter)
            }
        }
        _ => Err(PyException::type_error(format!(
            "'{}' object is not iterable",
            obj.type_name()
        ))),
    }
}
