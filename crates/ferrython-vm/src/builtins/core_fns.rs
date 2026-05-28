//! Core builtin function implementations (print, len, type, etc.)

mod fundamental;
mod numeric;

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min, guard_eager_allocation, guarded_push, new_fx_hashkey_flatmap,
    new_fx_hashkey_map, FxHashKeyMap, IteratorData, PropertyData, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef, SyncUsize,
};
use ferrython_core::types::{take_pending_eq_error, HashableKey};
use indexmap::IndexMap;
use std::cell::Cell;
use std::rc::Rc;

use super::iter_advance;
pub(super) use fundamental::{builtin_bool, builtin_float, builtin_int, builtin_str, builtin_type};
pub(crate) use numeric::builtin_abs;
pub(super) use numeric::{
    builtin_bin, builtin_callable, builtin_chr, builtin_divmod, builtin_hash, builtin_hex,
    builtin_input, builtin_max, builtin_min, builtin_oct, builtin_ord, builtin_pow, builtin_round,
    builtin_sum,
};

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

pub(super) fn builtin_id(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("id", args, 1)?;
    let ptr = if let PyObjectPayload::Code(code) = &args[0].payload {
        std::rc::Rc::as_ptr(code) as usize
    } else {
        PyObjectRef::as_ptr(&args[0]) as usize
    };
    Ok(PyObject::int(ptr as i64))
}

pub(super) fn builtin_sorted(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("sorted", args, 1)?;
    // For temporary lists (refcount==1), steal contents instead of cloning
    let mut items = if let PyObjectPayload::List(ref cell) = args[0].payload {
        if PyObjectRef::strong_count(&args[0]) == 1 {
            std::mem::take(&mut *cell.write())
        } else {
            cell.read().clone()
        }
    } else {
        args[0].to_list()?
    };
    // Homogeneous small-int sort: extract i64 values, sort natively, avoid repeated
    // enum matching in the comparator. Detection is O(n) matches vs O(n log n) match-pairs.
    let all_small_int = items.iter().all(|x| {
        matches!(
            &x.payload,
            PyObjectPayload::Int(ferrython_core::types::PyInt::Small(_))
        )
    });
    if all_small_int {
        items.sort_unstable_by(|a, b| {
            let av =
                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(v)) = &a.payload {
                    *v
                } else {
                    0
                };
            let bv =
                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(v)) = &b.payload {
                    *v
                } else {
                    0
                };
            av.cmp(&bv)
        });
    } else if items.len() > 1 {
        items.sort_unstable_by(|a, b| {
            ferrython_core::object::helpers::partial_cmp_objects(a, b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    Ok(PyObject::list(items))
}

pub(super) fn builtin_reversed(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("reversed", args, 1)?;
    let obj = &args[0];
    if let PyObjectPayload::List(items) = &obj.payload {
        let len = items.read().len();
        return Ok(PyObject::wrap(PyObjectPayload::RevRefIter {
            source: obj.clone(),
            index: SyncUsize::new(len),
        }));
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

pub(super) fn builtin_enumerate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn builtin_zip(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
pub(super) fn get_iter_from_obj(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::Iterator(_)
        | PyObjectPayload::RangeIter(..)
        | PyObjectPayload::VecIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. }
        | PyObjectPayload::Generator(_)
        | PyObjectPayload::AsyncGenerator(_) => Ok(obj.clone()),
        PyObjectPayload::Range(rd) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::Range {
                current: rd.start,
                stop: rd.stop,
                step: rd.step,
            }),
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
        PyObjectPayload::Set(m) => {
            let items: Vec<PyObjectRef> = m.read().values().cloned().collect();
            Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(
                ferrython_core::object::VecIterData {
                    items,
                    index: SyncUsize::new(0),
                },
            ))))
        }
        PyObjectPayload::Dict(_) | PyObjectPayload::MappingProxy(_) => {
            Ok(PyObject::wrap(PyObjectPayload::RefIter {
                source: obj.clone(),
                index: SyncUsize::new(0),
            }))
        }
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
                    | PyObjectPayload::WeakValueIter(_)
                    | PyObjectPayload::WeakKeyIter(_)
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

pub(super) fn builtin_range(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (start, stop, step, start_obj, stop_obj, step_obj) = match args.len() {
        1 => {
            let stop = args[0]
                .as_int()
                .ok_or_else(|| PyException::type_error("range() integer expected"))?;
            (
                0i64,
                stop,
                1i64,
                PyObject::int(0),
                args[0].clone(),
                PyObject::int(1),
            )
        }
        2 => {
            let start = args[0]
                .as_int()
                .ok_or_else(|| PyException::type_error("range() integer expected"))?;
            let stop = args[1]
                .as_int()
                .ok_or_else(|| PyException::type_error("range() integer expected"))?;
            (
                start,
                stop,
                1,
                args[0].clone(),
                args[1].clone(),
                PyObject::int(1),
            )
        }
        3 => {
            let start = args[0]
                .as_int()
                .ok_or_else(|| PyException::type_error("range() integer expected"))?;
            let stop = args[1]
                .as_int()
                .ok_or_else(|| PyException::type_error("range() integer expected"))?;
            let step = args[2]
                .as_int()
                .ok_or_else(|| PyException::type_error("range() integer expected"))?;
            if step == 0 {
                return Err(PyException::value_error("range() arg 3 must not be zero"));
            }
            (
                start,
                stop,
                step,
                args[0].clone(),
                args[1].clone(),
                args[2].clone(),
            )
        }
        _ => return Err(PyException::type_error("range expected 1 to 3 arguments")),
    };
    Ok(PyObject::range_with_objects(
        start, stop, step, start_obj, stop_obj, step_obj,
    ))
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
                Some((_new_iter, value)) => guarded_push(&mut items, value, "list()")?,
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
                Some((_new_iter, value)) => guarded_push(&mut items, value, "tuple()")?,
                None => break,
            }
        }
        return Ok(PyObject::tuple(items));
    }
    let items = args[0].to_list()?;
    Ok(PyObject::tuple(items))
}

pub(super) fn builtin_dict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. }
        | PyObjectPayload::Set(_) => Ok(PyObject::dict(dict_pair_items(&args[0])?)),
        _ => {
            // Try to handle instances with dict_storage (OrderedDict, dict subclasses)
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
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

pub(super) fn builtin_set(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn builtin_frozenset(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
            return Ok(PyObject::frozenset(items.items.clone()));
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

pub(super) fn builtin_all(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("all", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if !item.is_truthy() {
            return Ok(PyObject::bool_val(false));
        }
    }
    Ok(PyObject::bool_val(true))
}

pub(super) fn builtin_any(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("any", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if item.is_truthy() {
            return Ok(PyObject::bool_val(true));
        }
    }
    Ok(PyObject::bool_val(false))
}

pub(super) fn builtin_iter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn builtin_next(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn builtin_hasattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hasattr", args, 2)?;
    let name = args[1]
        .as_str()
        .ok_or_else(|| PyException::type_error("hasattr(): attribute name must be string"))?;
    Ok(PyObject::bool_val(ferrython_core::object::py_has_attr(
        &args[0], name,
    )))
}

pub(super) fn builtin_getattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("getattr", args, 2)?;
    let name = args[1]
        .as_str()
        .ok_or_else(|| PyException::type_error("getattr(): attribute name must be string"))?;
    match args[0].get_attr(name) {
        Some(v) => Ok(v),
        None => {
            if args.len() > 2 {
                Ok(args[2].clone())
            } else {
                Err(PyException::attribute_error(format!(
                    "'{}' object has no attribute '{}'",
                    args[0].type_name(),
                    name
                )))
            }
        }
    }
}

pub(crate) fn builtin_dir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    let target = if let PyObjectPayload::Instance(inst) = &args[0].payload {
        if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
            if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                Some((nc.func)(&[])?)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    let names = target.as_ref().unwrap_or(&args[0]).dir();
    let items: Vec<PyObjectRef> = names.into_iter().map(|n| PyObject::str_val(n)).collect();
    Ok(PyObject::list(items))
}

pub(super) fn builtin_format(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("format", args, 1)?;
    if args.len() >= 2 {
        let spec = args[1].py_to_string();
        if !spec.is_empty() {
            return args[0]
                .format_value(&spec)
                .map(|s| PyObject::str_val(CompactString::from(s)));
        }
    }
    Ok(PyObject::str_val(CompactString::from(
        args[0].py_to_string(),
    )))
}

pub(super) fn builtin_ascii(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("ascii", args, 1)?;
    let repr = args[0].repr();
    // ascii() takes repr() and escapes non-ASCII characters
    let escaped: String = repr
        .chars()
        .map(|c| {
            if c.is_ascii() {
                c.to_string()
            } else if (c as u32) <= 0xff {
                format!("\\x{:02x}", c as u32)
            } else if (c as u32) <= 0xffff {
                format!("\\u{:04x}", c as u32)
            } else {
                format!("\\U{:08x}", c as u32)
            }
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(escaped)))
}

pub(super) fn builtin_property(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let property_func = |idx: usize| {
        args.get(idx).and_then(|arg| {
            if matches!(&arg.payload, PyObjectPayload::None) {
                None
            } else {
                Some(arg.clone())
            }
        })
    };
    let fget_raw = property_func(0);
    let fset = property_func(1);
    let fdel = property_func(2);
    let (doc, doc_from_getter) =
        ferrython_core::object::property_init_doc(fget_raw.as_ref(), args.get(3).cloned());
    // If fget is an abstract marker ("__abstract__", func), keep it as-is.
    // is_abstract_marker() detects Property.fget abstract markers.
    // unwrap_abstract_fget() unwraps the marker when actually calling the getter.
    Ok(PyObjectRef::new(PyObject {
        payload: PyObjectPayload::Property(Box::new(PropertyData {
            fget: fget_raw,
            fset,
            fdel,
            doc: PyCell::new(doc),
            doc_from_getter: Cell::new(doc_from_getter),
        })),
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
    Ok(PyObjectRef::new(PyObject {
        payload: PyObjectPayload::StaticMethod(args[0].clone()),
    }))
}

pub(super) fn builtin_classmethod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("classmethod", args, 1)?;
    Ok(PyObjectRef::new(PyObject {
        payload: PyObjectPayload::ClassMethod(args[0].clone()),
    }))
}

pub(super) fn builtin_setattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 3 {
        return Err(PyException::type_error(
            "setattr() takes exactly 3 arguments",
        ));
    }
    let name = args[1].py_to_string();
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            inst.attrs
                .write()
                .insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::Class(cd) => {
            cd.namespace
                .write()
                .insert(CompactString::from(name), args[2].clone());
            cd.invalidate_cache();
        }
        PyObjectPayload::Module(m) => {
            m.attrs
                .write()
                .insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::ExceptionInstance(ei) => {
            ei.ensure_attrs()
                .write()
                .insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::Function(f) => {
            f.attrs
                .write()
                .insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::Property(_) if name == "__doc__" => {
            ferrython_core::object::property_set_doc(&args[0], args[2].clone())?;
        }
        PyObjectPayload::NativeFunction(_)
        | PyObjectPayload::NativeClosure(_)
        | PyObjectPayload::BuiltinFunction(_) => {
            // Silently accept — native functions don't have persistent attrs
        }
        _ => {
            return Err(PyException::attribute_error(format!(
                "'{}' object does not support attribute assignment",
                args[0].type_name()
            )))
        }
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
        _ => {
            return Err(PyException::attribute_error(format!(
                "'{}' object does not support attribute deletion",
                args[0].type_name()
            )))
        }
    }
    Ok(PyObject::none())
}

pub(super) fn builtin_vars(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::dict_from_pairs(vec![]));
    }
    if let Some(dict) = args[0].get_attr("__dict__") {
        Ok(dict)
    } else {
        Err(PyException::type_error(
            "vars() argument must have __dict__ attribute",
        ))
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
        if matches!(a.payload, PyObjectPayload::None) {
            None
        } else {
            Some(a.clone())
        }
    };
    match args.len() {
        0 => Err(PyException::type_error(
            "slice expected at least 1 argument, got 0",
        )),
        1 => Ok(PyObject::slice(None, to_opt(&args[0]), None)),
        2 => Ok(PyObject::slice(to_opt(&args[0]), to_opt(&args[1]), None)),
        _ => Ok(PyObject::slice(
            to_opt(&args[0]),
            to_opt(&args[1]),
            to_opt(&args[2]),
        )),
    }
}

pub(super) fn builtin_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bytes(vec![]));
    }
    match &args[0].payload {
        PyObjectPayload::Bytes(b) => Ok(PyObject::bytes((**b).clone())),
        PyObjectPayload::ByteArray(b) => Ok(PyObject::bytes((**b).clone())),
        PyObjectPayload::Str(s) => {
            // bytes(string, encoding) — require encoding argument
            if args.len() >= 2 {
                Ok(PyObject::bytes(s.as_bytes().to_vec()))
            } else {
                Err(PyException::type_error(
                    "string argument without an encoding",
                ))
            }
        }
        PyObjectPayload::Int(n) => {
            let size = n.to_i64().unwrap_or(0);
            if size < 0 {
                return Err(PyException::value_error("negative count"));
            }
            let size = size as usize;
            guard_eager_allocation(size, "bytes()")?;
            Ok(PyObject::bytes(vec![0u8; size]))
        }
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
            let items = args[0].to_list()?;
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                let v = item
                    .to_int()
                    .map_err(|_| PyException::type_error("an integer is required"))?;
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
                    PyObjectPayload::NativeFunction(nf) => return (nf.func)(&[args[0].clone()]),
                    PyObjectPayload::NativeClosure(nc) => return (nc.func)(&[args[0].clone()]),
                    _ => {}
                }
            }
            // Try as general iterable (range, generator, etc.)
            if let Ok(items) = args[0].to_list() {
                let mut result = Vec::with_capacity(items.len());
                for item in items {
                    let v = item
                        .to_int()
                        .map_err(|_| PyException::type_error("an integer is required"))?;
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
        PyObjectPayload::Bytes(b) => Ok(PyObject::bytearray((**b).clone())),
        PyObjectPayload::ByteArray(b) => Ok(PyObject::bytearray((**b).clone())),
        PyObjectPayload::Str(s) => {
            if args.len() >= 2 {
                Ok(PyObject::bytearray(s.as_bytes().to_vec()))
            } else {
                Err(PyException::type_error(
                    "string argument without an encoding",
                ))
            }
        }
        PyObjectPayload::Int(n) => {
            let size = n.to_i64().unwrap_or(0);
            if size < 0 {
                return Err(PyException::value_error("negative count"));
            }
            let size = size as usize;
            guard_eager_allocation(size, "bytearray()")?;
            Ok(PyObject::bytearray(vec![0u8; size]))
        }
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
            let items = args[0].to_list()?;
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                let v = item
                    .to_int()
                    .map_err(|_| PyException::type_error("an integer is required"))?;
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

pub(crate) fn builtin_complex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Unwrap Instance subclasses of complex/int/float via __builtin_value__
    let unwrap = |o: &PyObjectRef| -> PyObjectRef {
        if let PyObjectPayload::Instance(inst) = &o.payload {
            if let Some(v) = inst.attrs.read().get("__builtin_value__").cloned() {
                return v;
            }
        }
        o.clone()
    };
    let a0 = args.get(0).map(unwrap);
    let a1 = args.get(1).map(unwrap);
    if args.len() == 1 {
        let arg = a0.as_ref().unwrap();
        if let PyObjectPayload::Str(s) = &arg.payload {
            let s = s.trim();
            return parse_complex_string(s);
        }
        if let PyObjectPayload::Complex { .. } = &arg.payload {
            return Ok(arg.clone());
        }
        if let Some(dunder) = arg.get_attr("__complex__") {
            if let PyObjectPayload::NativeFunction(nf) = &dunder.payload {
                let res = (nf.func)(&[])?;
                return match &res.payload {
                    PyObjectPayload::Complex { real, imag } => Ok(PyObject::complex(*real, *imag)),
                    _ => Err(PyException::type_error("__complex__ returned non-complex")),
                };
            }
        }
        if matches!(
            &arg.payload,
            PyObjectPayload::Int(_) | PyObjectPayload::Float(_) | PyObjectPayload::Bool(_)
        ) {
            // OK, fall through to numeric conversion
        } else {
            return Err(PyException::type_error(format!(
                "complex() first argument must be a string or a number, not '{}'",
                arg.type_name()
            )));
        }
    }
    // With two args: complex(a, b) = a + b*1j
    // If b is complex, real/imag extraction: result = (a.real - b.imag) + (a.imag + b.real)j
    if let (Some(a), Some(b)) = (&a0, &a1) {
        if matches!(&a.payload, PyObjectPayload::Str(_)) {
            return Err(PyException::type_error(
                "complex() can't take second arg if first is a string",
            ));
        }
        if matches!(&b.payload, PyObjectPayload::Str(_)) {
            return Err(PyException::type_error(
                "complex() second arg can't be a string",
            ));
        }
        // Reject dicts/lists/etc for either arg with helpful messages
        let is_num = |o: &PyObjectRef| {
            matches!(
                &o.payload,
                PyObjectPayload::Int(_)
                    | PyObjectPayload::Float(_)
                    | PyObjectPayload::Bool(_)
                    | PyObjectPayload::Complex { .. }
            )
        };
        if !is_num(a) {
            return Err(PyException::type_error(format!(
                "complex() first argument must be a string or a number, not '{}'",
                a.type_name()
            )));
        }
        if !is_num(b) {
            return Err(PyException::type_error(format!(
                "complex() second argument must be a number, not '{}'",
                b.type_name()
            )));
        }
        let a_is_complex = matches!(&a.payload, PyObjectPayload::Complex { .. });
        let b_is_complex = matches!(&b.payload, PyObjectPayload::Complex { .. });
        let af = a.to_float().unwrap_or(0.0);
        let bf = b.to_float().unwrap_or(0.0);
        if !a_is_complex && matches!(&a.payload, PyObjectPayload::Int(_)) && af.is_infinite() {
            return Err(PyException::overflow_error(
                "int too large to convert to float",
            ));
        }
        if !b_is_complex && matches!(&b.payload, PyObjectPayload::Int(_)) && bf.is_infinite() {
            return Err(PyException::overflow_error(
                "int too large to convert to float",
            ));
        }
        let (ar, ai) = match &a.payload {
            PyObjectPayload::Complex { real, imag } => (*real, *imag),
            _ => (af, 0.0),
        };
        let (br, bi) = match &b.payload {
            PyObjectPayload::Complex { real, imag } => (*real, *imag),
            _ => (bf, 0.0),
        };
        let real = if b_is_complex { ar - bi } else { ar };
        let imag = if a_is_complex { ai + br } else { br };
        return Ok(PyObject::complex(real, imag));
    }
    let real = a0
        .as_ref()
        .map(|v| v.to_float().unwrap_or(0.0))
        .unwrap_or(0.0);
    let imag = a1
        .as_ref()
        .map(|v| v.to_float().unwrap_or(0.0))
        .unwrap_or(0.0);
    Ok(PyObject::complex(real, imag))
}

fn parse_complex_string(raw: &str) -> PyResult<PyObjectRef> {
    let trimmed = raw.trim();
    // Strip matching surrounding parens
    let trimmed = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        trimmed[1..trimmed.len() - 1].trim()
    } else {
        trimmed
    };
    if trimmed.is_empty() {
        return Err(PyException::value_error(format!(
            "complex() arg is a malformed string: '{}'",
            raw
        )));
    }
    // Remove all underscores (validated later that no double-underscore creeps in via strtod)
    let no_ws: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();

    // Handle pure imaginary: "2j", "-3j", or "j" / "+j" / "-j"
    if no_ws.ends_with('j') || no_ws.ends_with('J') {
        let body = &no_ws[..no_ws.len() - 1];
        // Handle "j", "+j", "-j"
        let imag_body = if body.is_empty() {
            "1"
        } else if body == "+" {
            "1"
        } else if body == "-" {
            "-1"
        } else {
            body
        };
        // Pure imaginary: full body parses as float
        if let Some(f) = parse_py_float(imag_body) {
            return Ok(PyObject::complex(0.0, f));
        }
        // Split into real+imag. Find last '+' or '-' that is not part of an exponent
        // and not at position 0.
        if let Some(split_pos) = find_complex_split(body) {
            let real_s = &body[..split_pos];
            let imag_s_raw = &body[split_pos..];
            let imag_s = if imag_s_raw == "+" {
                "1".to_string()
            } else if imag_s_raw == "-" {
                "-1".to_string()
            } else if imag_s_raw == "++" || imag_s_raw == "+" {
                "1".to_string()
            } else if imag_s_raw.starts_with('+') {
                let rest = &imag_s_raw[1..];
                if rest.is_empty() {
                    "1".to_string()
                } else {
                    rest.to_string()
                }
            } else if imag_s_raw == "-" {
                "-1".to_string()
            } else if imag_s_raw.starts_with('-') && imag_s_raw.len() == 1 {
                "-1".to_string()
            } else {
                imag_s_raw.to_string()
            };
            if let (Some(r), Some(i)) = (parse_py_float(real_s), parse_py_float(&imag_s)) {
                return Ok(PyObject::complex(r, i));
            }
        }
    }
    // Pure real
    if let Some(r) = parse_py_float(&no_ws) {
        return Ok(PyObject::complex(r, 0.0));
    }
    Err(PyException::value_error(format!(
        "complex() arg is a malformed string: '{}'",
        raw
    )))
}

/// Parse a Python-style float string (supports `_` separators, `inf`, `nan`).
fn parse_py_float(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    if s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return None;
    }
    // Each underscore must be surrounded by digits on both sides.
    let bytes = s.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if c == b'_' {
            if i == 0 || i + 1 >= bytes.len() {
                return None;
            }
            let prev = bytes[i - 1];
            let next = bytes[i + 1];
            if !prev.is_ascii_digit() || !next.is_ascii_digit() {
                return None;
            }
        }
    }
    let cleaned: String = s.chars().filter(|&c| c != '_').collect();
    let lower = cleaned.to_ascii_lowercase();
    match lower.as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => Some(f64::INFINITY),
        "-inf" | "-infinity" => Some(f64::NEG_INFINITY),
        "nan" | "+nan" | "-nan" => Some(f64::NAN),
        _ => cleaned.parse::<f64>().ok(),
    }
}

/// Find the split position between real and imag in a body like "1+2" or "3.14-5e-2".
/// Returns the index of the '+' or '-' separating the parts.
fn find_complex_split(body: &str) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        let c = bytes[i];
        if (c == b'+' || c == b'-') && i > 0 {
            let prev = bytes[i - 1];
            if prev == b'e' || prev == b'E' {
                continue;
            }
            // If previous char is also a sign (like `+-0j`), this sign is part of the imag
            // number, keep scanning to find the earlier one.
            if prev == b'+' || prev == b'-' {
                continue;
            }
            return Some(i);
        }
    }
    None
}

pub(super) fn builtin_object(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::instance(PyObject::builtin_type(
        CompactString::from("object"),
    )))
}

pub(super) fn builtin_super(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Simplified: return None for now
    Ok(PyObject::none())
}

/// dict.fromkeys(iterable, value=None) — create dict with keys from iterable
pub(super) fn builtin_dict_fromkeys(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
            return Err(PyException::type_error(format!(
                "'{}' object is not iterable",
                iterable.type_name()
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
    let _name = obj
        .get_attr("__name__")
        .map(|n| n.py_to_string())
        .unwrap_or_else(|| type_name.to_string());

    // Get docstring
    let doc = obj
        .get_attr("__doc__")
        .map(|d| d.py_to_string())
        .unwrap_or_default();

    // Print header
    match &obj.payload {
        PyObjectPayload::Class(cd) => {
            println!("Help on class {}:", cd.name);
            println!();
            println!(
                "class {}({})",
                cd.name,
                cd.bases
                    .iter()
                    .filter_map(|b| b.get_attr("__name__").map(|n| n.py_to_string()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
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
                let method_doc = val
                    .get_attr("__doc__")
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
                if name.starts_with("_") {
                    continue;
                }
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
        return Err(PyException::type_error(
            "__import__() requires at least 1 argument",
        ));
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
            let cls = PyObject::builtin_type(CompactString::from("memoryview"));
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(
                    CompactString::from("__memoryview__"),
                    PyObject::bool_val(true),
                );
                attrs.insert(
                    CompactString::from("__readonly__"),
                    PyObject::bool_val(true),
                );
                attrs.insert(CompactString::from("obj"), args[0].clone());
                attrs.insert(
                    CompactString::from("format"),
                    PyObject::str_val(CompactString::from("B")),
                );
                attrs.insert(CompactString::from("ndim"), PyObject::int(1));
                install_memoryview_cast_method(&inst);
            }
            Ok(inst)
        }
        PyObjectPayload::ByteArray(_) => {
            let cls = PyObject::builtin_type(CompactString::from("memoryview"));
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(
                    CompactString::from("__memoryview__"),
                    PyObject::bool_val(true),
                );
                attrs.insert(
                    CompactString::from("__readonly__"),
                    PyObject::bool_val(false),
                );
                attrs.insert(CompactString::from("obj"), args[0].clone());
                attrs.insert(
                    CompactString::from("format"),
                    PyObject::str_val(CompactString::from("B")),
                );
                attrs.insert(CompactString::from("ndim"), PyObject::int(1));
                install_memoryview_cast_method(&inst);
            }
            Ok(inst)
        }
        _ => Err(PyException::type_error(format!(
            "memoryview: a bytes-like object is required, not '{}'",
            args[0].type_name()
        ))),
    }
}

fn install_memoryview_cast_method(inst: &PyObjectRef) {
    if let PyObjectPayload::Instance(ref data) = inst.payload {
        let source = inst.clone();
        data.attrs.write().insert(
            CompactString::from("cast"),
            PyObject::native_closure("memoryview.cast", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "cast() missing required argument 'format'",
                    ));
                }
                let format = args[0].py_to_string();
                let ndim = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
                    match &args[1].payload {
                        PyObjectPayload::Tuple(items) => items.len() as i64,
                        PyObjectPayload::List(items) => items.read().len() as i64,
                        _ => 1,
                    }
                } else {
                    1
                };
                let cls = PyObject::builtin_type(CompactString::from("memoryview"));
                let view = PyObject::instance(cls);
                if let PyObjectPayload::Instance(ref view_data) = view.payload {
                    let base = source.get_attr("obj").unwrap_or_else(|| source.clone());
                    let readonly = source
                        .get_attr("__readonly__")
                        .unwrap_or_else(|| PyObject::bool_val(true));
                    let mut attrs = view_data.attrs.write();
                    attrs.insert(
                        CompactString::from("__memoryview__"),
                        PyObject::bool_val(true),
                    );
                    attrs.insert(CompactString::from("__readonly__"), readonly);
                    attrs.insert(CompactString::from("obj"), base);
                    attrs.insert(
                        CompactString::from("format"),
                        PyObject::str_val(CompactString::from(format.as_str())),
                    );
                    attrs.insert(CompactString::from("ndim"), PyObject::int(ndim));
                }
                install_memoryview_cast_method(&view);
                Ok(view)
            }),
        );
    }
}
