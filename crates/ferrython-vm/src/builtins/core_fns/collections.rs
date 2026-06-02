use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::dict_storage_version;
use ferrython_core::object::{
    check_args, check_args_min, guarded_push, new_fx_hashkey_flatmap, new_fx_hashkey_map,
    DequeIterData, FxHashKeyMap, IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef, SyncUsize,
};
use ferrython_core::types::{take_pending_eq_error, HashableKey, PyInt};
use indexmap::IndexMap;
use std::rc::Rc;

use super::super::iter_advance;
use super::super::iter_helpers::deque_storage_len;

pub(crate) fn builtin_reversed(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("reversed", args, 1)?;
    let obj = &args[0];
    if let PyObjectPayload::List(items) = &obj.payload {
        let len = items.read().len();
        return Ok(PyObject::wrap(PyObjectPayload::RevRefIter {
            source: obj.clone(),
            index: SyncUsize::new(len),
        }));
    }
    match &obj.payload {
        PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => {
            let items: Vec<PyObjectRef> = map
                .read()
                .iter()
                .filter(|(key, _)| !ferrython_core::object::helpers::is_hidden_dict_key(key))
                .map(|(key, _)| key.to_object())
                .rev()
                .collect();
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::List { items, index: 0 }),
            ))));
        }
        PyObjectPayload::DictKeys { map, .. } => {
            let items: Vec<PyObjectRef> = map
                .read()
                .iter()
                .filter(|(key, _)| !ferrython_core::object::helpers::is_hidden_dict_key(key))
                .map(|(key, _)| key.to_object())
                .rev()
                .collect();
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::List { items, index: 0 }),
            ))));
        }
        PyObjectPayload::DictValues { map, .. } => {
            let items: Vec<PyObjectRef> = map
                .read()
                .iter()
                .filter(|(key, _)| !ferrython_core::object::helpers::is_hidden_dict_key(key))
                .map(|(_, value)| value.clone())
                .rev()
                .collect();
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::List { items, index: 0 }),
            ))));
        }
        PyObjectPayload::DictItems { map, .. } => {
            let items: Vec<PyObjectRef> = map
                .read()
                .iter()
                .filter(|(key, _)| !ferrython_core::object::helpers::is_hidden_dict_key(key))
                .map(|(key, value)| PyObject::tuple(vec![key.to_object(), value.clone()]))
                .rev()
                .collect();
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::List { items, index: 0 }),
            ))));
        }
        _ => {}
    }
    if let PyObjectPayload::Range(rd) = &obj.payload {
        let len = ferrython_core::object::helpers::range_data_len_bigint(rd);
        if len == num_bigint::BigInt::from(0) {
            return Ok(PyObject::range(0, 0, 1).get_iter()?);
        }
        let last_index = &len - num_bigint::BigInt::from(1);
        let last = ferrython_core::object::helpers::range_item_bigint(rd, &last_index);
        let (start, _, step) = ferrython_core::object::helpers::range_parts_bigint(rd);
        let stop = start - &step;
        let reverse_step = -step;
        let reversed = PyObject::wrap(PyObjectPayload::Range(Box::new(
            ferrython_core::object::helpers::range_data_from_bigints(last, stop, reverse_step),
        )));
        return reversed.get_iter();
    }
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if inst.attrs.read().contains_key("__deque__") {
            return Ok(PyObject::tracked(PyObjectPayload::DequeIter(Box::new(
                DequeIterData {
                    source: obj.clone(),
                    index: SyncUsize::new(0),
                    expected_len: deque_storage_len(obj).unwrap_or_default(),
                    reverse: true,
                },
            ))));
        }
        if obj.get_attr("__reversed__").is_none() {
            return Err(PyException::type_error(format!(
                "'{}' object is not reversible",
                obj.type_name()
            )));
        }
    }
    if let Some(reversed_attr) = obj.get_attr("__reversed__") {
        if !matches!(&reversed_attr.payload, PyObjectPayload::None) {
            return ferrython_core::object::helpers::call_callable(&reversed_attr, &[]);
        }
    }
    let builtin_reversible = matches!(
        obj.type_name(),
        "list"
            | "tuple"
            | "str"
            | "bytes"
            | "bytearray"
            | "range"
            | "dict"
            | "Counter"
            | "OrderedDict"
            | "dict_keys"
            | "dict_items"
            | "dict_values"
            | "memoryview"
    );
    if !builtin_reversible {
        return Err(PyException::type_error(format!(
            "'{}' object is not reversible",
            obj.type_name()
        )));
    }
    let mut items = obj.to_list()?;
    items.reverse();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
        PyCell::new(ferrython_core::object::IteratorData::List { items, index: 0 }),
    ))))
}

pub(crate) fn builtin_enumerate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("enumerate", args, 1)?;
    let start = if args.len() > 1 {
        args[1].as_int().unwrap_or(0)
    } else {
        0
    };
    // Get an iterator from the source
    let source = get_iter_from_obj(&args[0])?;
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
        PyCell::new(IteratorData::Enumerate {
            source,
            index: start,
            cached_tuple: None,
        }),
    ))))
}

pub(crate) fn builtin_zip(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: vec![],
                index: 0,
            }),
        ))));
    }
    // Check for trailing kwargs dict with strict=True
    let mut strict = false;
    let iter_args = if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("strict"))) {
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
    let sources: Vec<PyObjectRef> = iter_args
        .iter()
        .map(|a| get_iter_from_obj(a))
        .collect::<PyResult<Vec<_>>>()?;
    let n = sources.len();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
        PyCell::new(IteratorData::Zip {
            sources,
            strict,
            cached_tuple: None,
            items_buf: Vec::with_capacity(n),
        }),
    ))))
}

/// Get an iterator from any iterable object.
pub(crate) fn get_iter_from_obj(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::Iterator(_)
        | PyObjectPayload::RangeIter(..)
        | PyObjectPayload::VecIter(_)
        | PyObjectPayload::DictValueIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::DequeIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. }
        | PyObjectPayload::Generator(_)
        | PyObjectPayload::AsyncGenerator(_) => Ok(obj.clone()),
        PyObjectPayload::Range(rd) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(ferrython_core::object::helpers::range_iterator_from_data(
                rd,
            )),
        )))),
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
            Ok(PyObject::wrap(PyObjectPayload::RefIter {
                source: obj.clone(),
                index: SyncUsize::new(0),
            }))
        }
        PyObjectPayload::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::Str { chars, index: 0 }),
            ))))
        }
        PyObjectPayload::Set(m) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::SetRefs {
                source: m.clone(),
                index: 0,
                expected_len: m.read().len(),
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
        PyObjectPayload::Instance(_) => match obj.get_iter() {
            Ok(iter) => Ok(iter),
            Err(_) if obj.get_attr("__next__").is_some() => Ok(obj.clone()),
            Err(_) => Err(PyException::type_error(format!(
                "'{}' object is not iterable",
                obj.type_name()
            ))),
        },
        // Module with __iter__ (file objects, module_with_attrs with _bind_methods)
        // Need to call __iter__ method to get the iterable result
        PyObjectPayload::Module(_) => {
            if let Some(iter_attr) = obj.get_attr("__iter__") {
                let result = match &iter_attr.payload {
                    // __iter__ returned a list/iterator directly (stored as attr)
                    PyObjectPayload::List(_)
                    | PyObjectPayload::Tuple(_)
                    | PyObjectPayload::Iterator(_)
                    | PyObjectPayload::RangeIter(..)
                    | PyObjectPayload::VecIter(_)
                    | PyObjectPayload::DictValueIter(_)
                    | PyObjectPayload::WeakValueIter(_)
                    | PyObjectPayload::WeakKeyIter(_)
                    | PyObjectPayload::DequeIter(_)
                    | PyObjectPayload::RefIter { .. }
                    | PyObjectPayload::RevRefIter { .. } => Some(iter_attr.clone()),
                    // __iter__ is a bound method — call it
                    PyObjectPayload::BoundMethod { receiver, method } => {
                        if let PyObjectPayload::NativeClosure(nc) = &method.payload {
                            Some((nc.func)(&[receiver.clone()])?)
                        } else if let PyObjectPayload::NativeFunction(nf) = &method.payload {
                            Some((nf.func)(&[receiver.clone()])?)
                        } else {
                            None
                        }
                    }
                    // __iter__ is a native closure/function to call with self
                    PyObjectPayload::NativeClosure(nc) => Some((nc.func)(&[obj.clone()])?),
                    PyObjectPayload::NativeFunction(nf) => Some((nf.func)(&[obj.clone()])?),
                    _ => None,
                };
                if let Some(result) = result {
                    // Guard against self-iter (module __iter__ returning self) which would infinite-recurse.
                    if PyObjectRef::ptr_eq(&result, obj) {
                        // Treat module as already-an-iterator if it has __next__.
                        if obj.get_attr("__next__").is_some() {
                            return Ok(obj.clone());
                        }
                        return Err(PyException::type_error(format!(
                            "'{}' object is not iterable",
                            obj.type_name()
                        )));
                    }
                    return get_iter_from_obj(&result);
                }
            }
            // No __iter__ but has __next__ — treat as already iterator
            if obj.get_attr("__next__").is_some() {
                return Ok(obj.clone());
            }
            Err(PyException::type_error(format!(
                "'{}' object is not iterable",
                obj.type_name()
            )))
        }
        // Delegate all other payload types to the core get_iter (handles DictKeys, DictValues,
        // DictItems, Bytes, ByteArray, FrozenSet, MappingProxy, etc.)
        // For Module payloads (file objects etc.) with __iter__, the __iter__ returns
        // a list — use that directly.
        _ => obj.get_iter().map_err(|_| {
            PyException::type_error(format!("'{}' object is not iterable", obj.type_name()))
        }),
    }
}

fn range_index_arg(obj: &PyObjectRef) -> PyResult<(i64, PyObjectRef)> {
    let index = obj.to_index().map_err(|err| {
        if err.kind == ExceptionKind::TypeError {
            PyException::type_error("range() integer expected")
        } else {
            err
        }
    })?;
    let saturated = match &index {
        PyInt::Small(n) => *n,
        PyInt::Big(n) if n.sign() == num_bigint::Sign::Minus => i64::MIN,
        PyInt::Big(_) => i64::MAX,
    };
    let bound_obj = match &obj.payload {
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__builtin_value__")
            .filter(|value| {
                matches!(
                    value.payload,
                    PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)
                )
            })
            .map(|_| obj.clone())
            .unwrap_or_else(|| index.to_object()),
        _ => index.to_object(),
    };
    Ok((saturated, bound_obj))
}

pub(crate) fn builtin_range(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (start, stop, step, start_obj, stop_obj, step_obj) = match args.len() {
        1 => {
            let (stop, stop_obj) = range_index_arg(&args[0])?;
            (
                0i64,
                stop,
                1i64,
                PyObject::int(0),
                stop_obj,
                PyObject::int(1),
            )
        }
        2 => {
            let (start, start_obj) = range_index_arg(&args[0])?;
            let (stop, stop_obj) = range_index_arg(&args[1])?;
            (start, stop, 1, start_obj, stop_obj, PyObject::int(1))
        }
        3 => {
            let (start, start_obj) = range_index_arg(&args[0])?;
            let (stop, stop_obj) = range_index_arg(&args[1])?;
            let (step, step_obj) = range_index_arg(&args[2])?;
            if step == 0 {
                return Err(PyException::value_error("range() arg 3 must not be zero"));
            }
            (start, stop, step, start_obj, stop_obj, step_obj)
        }
        _ => return Err(PyException::type_error("range expected 1 to 3 arguments")),
    };
    Ok(PyObject::range_with_objects(
        start, stop, step, start_obj, stop_obj, step_obj,
    ))
}

pub(crate) fn builtin_list(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    // For Module payloads (e.g. file objects), use VM-level iteration that can call __iter__
    if matches!(&args[0].payload, PyObjectPayload::Module(_)) {
        let iter = get_iter_from_obj(&args[0])?;
        let mut items = Vec::new();
        loop {
            match iter_advance(&iter)? {
                Some((_new_iter, value)) => guarded_push(&mut items, value, "list()")?,
                None => break,
            }
        }
        return Ok(PyObject::list(items));
    }
    let items = args[0].to_list()?;
    Ok(PyObject::list(items))
}

pub(crate) fn builtin_tuple(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::tuple(vec![]));
    }
    if matches!(&args[0].payload, PyObjectPayload::Module(_)) {
        let iter = get_iter_from_obj(&args[0])?;
        let mut items = Vec::new();
        loop {
            match iter_advance(&iter)? {
                Some((_new_iter, value)) => guarded_push(&mut items, value, "tuple()")?,
                None => break,
            }
        }
        return Ok(PyObject::tuple(items));
    }
    let items = args[0].to_list()?;
    Ok(PyObject::tuple(items))
}

pub(crate) fn builtin_dict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    fn dict_pair_items(obj: &PyObjectRef) -> PyResult<FxHashKeyMap> {
        let pairs = obj.to_list()?;
        let mut map: FxHashKeyMap = new_fx_hashkey_map();
        for pair in &pairs {
            let kv = match &pair.payload {
                PyObjectPayload::Tuple(items) if items.len() == 2 => {
                    vec![items[0].clone(), items[1].clone()]
                }
                PyObjectPayload::List(items) if items.read().len() == 2 => {
                    let items = items.read();
                    vec![items[0].clone(), items[1].clone()]
                }
                _ => pair.to_list()?,
            };
            if kv.len() != 2 {
                return Err(PyException::value_error(format!(
                    "dictionary update sequence element has length {}; 2 is required",
                    kv.len()
                )));
            }
            let key = kv[0].to_hashable_key()?;
            map.insert(key, kv[1].clone());
        }
        Ok(map)
    }

    fn dict_from_mapping(obj: &PyObjectRef) -> PyResult<Option<FxHashKeyMap>> {
        let Some(keys_fn) = obj.get_attr("keys") else {
            return Ok(None);
        };
        let keys = ferrython_core::object::call_callable(&keys_fn, &[])?;
        let mut map: FxHashKeyMap = new_fx_hashkey_map();
        for key_obj in keys.to_list()? {
            let value = obj.get_item(&key_obj)?;
            map.insert(key_obj.to_hashable_key()?, value);
        }
        Ok(Some(map))
    }

    if args.len() > 1 {
        return Err(PyException::type_error(
            "dict expected at most 1 positional argument",
        ));
    }
    if args.is_empty() {
        return Ok(PyObject::dict(new_fx_hashkey_map()));
    }
    match &args[0].payload {
        PyObjectPayload::Dict(m) => {
            let mut new_map = m.read().clone();
            new_map.shift_remove(&HashableKey::str_key(CompactString::from(
                "__defaultdict_factory__",
            )));
            new_map.shift_remove(&HashableKey::str_key(CompactString::from("__counter__")));
            Ok(PyObject::dict(new_map))
        }
        PyObjectPayload::MappingProxy(m) => Ok(PyObject::dict(m.read().clone())),
        PyObjectPayload::InstanceDict(m) => {
            let read = m.read();
            let mut map = IndexMap::new();
            for (k, v) in read.iter() {
                if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                    map.insert(hk, v.clone());
                }
            }
            Ok(PyObject::dict(map))
        }
        // dict from iterable of (key, value) pairs
        PyObjectPayload::List(_)
        | PyObjectPayload::Tuple(_)
        | PyObjectPayload::Iterator(_)
        | PyObjectPayload::RangeIter(..)
        | PyObjectPayload::VecIter(_)
        | PyObjectPayload::DictValueIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::DequeIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. }
        | PyObjectPayload::Set(_) => Ok(PyObject::dict(dict_pair_items(&args[0])?)),
        _ => {
            // Dict subclasses may override keys()/__iter__; use the mapping
            // protocol before cloning their internal storage.
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(map) = dict_from_mapping(&args[0])? {
                    return Ok(PyObject::dict(map));
                }
                if let Some(ref ds) = inst.dict_storage {
                    let read = ds.read();
                    return Ok(PyObject::dict(read.clone()));
                }
            }
            if let Some(map) = dict_from_mapping(&args[0])? {
                return Ok(PyObject::dict(map));
            }
            // Fall back to iterating as pairs
            Ok(PyObject::dict(dict_pair_items(&args[0])?))
        }
    }
}

pub(crate) fn builtin_set(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() > 1 {
        return Err(PyException::type_error(format!(
            "set expected at most 1 argument, got {}",
            args.len()
        )));
    }
    if args.is_empty() {
        return Ok(PyObject::set(new_fx_hashkey_map()));
    }
    match &args[0].payload {
        PyObjectPayload::Dict(items) => {
            let read = items.read();
            let mut set = new_fx_hashkey_flatmap();
            set.reserve(read.len());
            for key in read.keys() {
                set.insert(key.clone(), key.to_object());
            }
            return Ok(PyObject::set_from_flatmap(set));
        }
        PyObjectPayload::Set(items) => {
            return Ok(PyObject::set_from_flatmap(items.read().clone()));
        }
        PyObjectPayload::FrozenSet(items) => {
            let mut set = new_fx_hashkey_flatmap();
            set.reserve(items.len());
            for (key, value) in items.iter() {
                set.insert(key.clone(), value.clone());
            }
            return Ok(PyObject::set_from_flatmap(set));
        }
        _ => {}
    }
    let items = args[0].to_list()?;
    let mut set = IndexMap::new();
    for item in items {
        let key = item.to_hashable_key()?;
        set.entry(key).or_insert(item);
        if let Some(err) = take_pending_eq_error() {
            return Err(err);
        }
    }
    Ok(PyObject::set(set))
}

pub(crate) fn builtin_frozenset(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() > 1 {
        return Err(PyException::type_error(format!(
            "frozenset expected at most 1 argument, got {}",
            args.len()
        )));
    }
    if args.is_empty() {
        return Ok(PyObject::frozenset(new_fx_hashkey_map()));
    }
    match &args[0].payload {
        PyObjectPayload::Dict(items) => {
            let read = items.read();
            let mut set = new_fx_hashkey_map();
            for key in read.keys() {
                set.insert(key.clone(), key.to_object());
            }
            return Ok(PyObject::frozenset(set));
        }
        PyObjectPayload::Set(items) => {
            let read = items.read();
            let mut set = new_fx_hashkey_map();
            for (key, value) in read.iter() {
                set.insert(key.clone(), value.clone());
            }
            return Ok(PyObject::frozenset(set));
        }
        PyObjectPayload::FrozenSet(items) => {
            let _ = items;
            return Ok(args[0].clone());
        }
        _ => {}
    }
    let items = args[0].to_list()?;
    let mut set = IndexMap::new();
    for item in items {
        let key = item.to_hashable_key()?;
        set.entry(key).or_insert(item);
        if let Some(err) = take_pending_eq_error() {
            return Err(err);
        }
    }
    Ok(PyObject::frozenset(set))
}

pub(crate) fn builtin_all(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("all", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if !item.is_truthy() {
            return Ok(PyObject::bool_val(false));
        }
    }
    Ok(PyObject::bool_val(true))
}

pub(crate) fn builtin_any(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("any", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if item.is_truthy() {
            return Ok(PyObject::bool_val(true));
        }
    }
    Ok(PyObject::bool_val(false))
}

pub(crate) fn builtin_iter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() == 2 {
        // iter(callable, sentinel) — creates a lazy sentinel iterator
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::Sentinel {
                callable: args[0].clone(),
                sentinel: args[1].clone(),
                done: false,
            }),
        ))));
    }
    check_args("iter", args, 1)?;
    get_iter_from_obj(&args[0])
}

pub(crate) fn builtin_next(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("next", args, 1)?;
    let advanced = match iter_advance(&args[0]) {
        Ok(value) => value,
        Err(err)
            if err.kind == ExceptionKind::TypeError
                && matches!(&args[0].payload, PyObjectPayload::Iterator(_)) =>
        {
            let next_method = args[0].get_attr("__next__").ok_or_else(|| {
                PyException::type_error(format!(
                    "'{}' object is not an iterator",
                    args[0].type_name()
                ))
            })?;
            match ferrython_core::object::helpers::call_callable(&next_method, &[]) {
                Ok(value) => return Ok(value),
                Err(err) if err.kind == ExceptionKind::StopIteration => None,
                Err(err) => return Err(err),
            }
        }
        Err(err) => return Err(err),
    };
    match advanced {
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

/// dict.fromkeys(iterable, value=None) — create dict with keys from iterable
pub(crate) fn builtin_dict_fromkeys(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("dict.fromkeys", args, 1)?;
    let iterable = &args[0];
    let value = if args.len() >= 2 {
        args[1].clone()
    } else {
        PyObject::none()
    };
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
        PyObjectPayload::FrozenSet(items) => {
            for key in items.keys() {
                map.insert(key.clone(), value.clone());
            }
        }
        PyObjectPayload::Str(s) => {
            for ch in s.chars() {
                let hk = HashableKey::str_key(CompactString::from(ch.to_string()));
                map.insert(hk, value.clone());
            }
        }
        PyObjectPayload::Dict(d) => {
            for key in d.read().keys() {
                map.insert(key.clone(), value.clone());
            }
        }
        _ => {
            for item in iterable.to_list()? {
                let hk = item.to_hashable_key()?;
                map.insert(hk, value.clone());
            }
        }
    }
    Ok(PyObject::dict(map))
}
