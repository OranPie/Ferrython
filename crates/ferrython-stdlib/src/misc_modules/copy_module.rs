use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    call_callable, make_builtin, make_module, new_fx_hashkey_map, FxAttrMap, InstanceData,
    IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use std::rc::Rc;

// ── struct module ──

pub fn create_copy_module() -> PyObjectRef {
    make_module(
        "copy",
        vec![
            ("copy", make_builtin(copy_copy)),
            ("deepcopy", make_builtin(copy_deepcopy)),
        ],
    )
}

fn copy_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("copy() requires 1 argument"));
    }
    shallow_copy(&args[0])
}

fn copy_deepcopy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("deepcopy() requires 1 argument"));
    }
    if args.len() >= 2 {
        return deep_copy_with_memo_object(&args[0], &args[1]);
    }
    let mut memo = std::collections::HashMap::new();
    deep_copy_with_memo(&args[0], &mut memo)
}

fn deep_copy_with_memo_object(obj: &PyObjectRef, memo_obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let mut memo = std::collections::HashMap::new();
    if let PyObjectPayload::Dict(map) = &memo_obj.payload {
        for (key, value) in map.read().iter() {
            if let HashableKey::Int(n) = key {
                if let Some(ptr) = n.to_i64() {
                    memo.insert(ptr as usize, value.clone());
                }
            }
        }
    }
    let result = deep_copy_with_memo(obj, &mut memo)?;
    if let PyObjectPayload::Dict(map) = &memo_obj.payload {
        let mut write = map.write();
        for (ptr, value) in memo {
            write.insert(HashableKey::Int(PyInt::Small(ptr as i64)), value);
        }
    }
    Ok(result)
}

fn shallow_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::None
        | PyObjectPayload::Bool(_)
        | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_)
        | PyObjectPayload::Str(_)
        | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => Ok(PyObject::tuple((**items).clone())),
        PyObjectPayload::List(items) => Ok(PyObject::list(items.read().clone())),
        PyObjectPayload::Dict(map) => Ok(PyObject::dict(map.read().clone())),
        PyObjectPayload::Set(set) => Ok(PyObject::set_from_flatmap(set.read().clone())),
        PyObjectPayload::Iterator(iter_data) => {
            let data = iter_data.read();
            if let IteratorData::Islice {
                source,
                index,
                next_yield,
                stop,
                step,
            } = &*data
            {
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::Islice {
                        source: source.clone(),
                        index: *index,
                        next_yield: *next_yield,
                        stop: *stop,
                        step: *step,
                    }),
                ))));
            }
            Ok(obj.clone())
        }
        PyObjectPayload::Instance(inst) => {
            if let Some(copy_fn) = obj.get_attr("__copy__") {
                return call_callable(&copy_fn, &[]);
            }
            // Create new instance with same class, shallow copy of attrs
            Ok(PyObject::wrap(PyObjectPayload::Instance(
                std::mem::ManuallyDrop::new(Box::new(InstanceData {
                    class: inst.class.clone(),
                    attrs: Rc::new(PyCell::new(inst.attrs.read().clone())),
                    is_special: true,
                    dict_storage: inst
                        .dict_storage
                        .as_ref()
                        .map(|ds| Rc::new(PyCell::new(ds.read().clone()))),
                    class_flags: InstanceData::compute_flags(&inst.class),
                    finalizer_state: std::cell::Cell::new(0),
                })),
            )))
        }
        _ => Ok(obj.clone()),
    }
}

#[allow(dead_code)]
fn deep_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let mut memo = std::collections::HashMap::new();
    deep_copy_with_memo(obj, &mut memo)
}

fn deep_copy_with_memo(
    obj: &PyObjectRef,
    memo: &mut std::collections::HashMap<usize, PyObjectRef>,
) -> PyResult<PyObjectRef> {
    // Check memo for already-copied objects (handles circular references)
    let ptr = PyObjectRef::as_ptr(obj) as usize;
    if let Some(existing) = memo.get(&ptr) {
        return Ok(existing.clone());
    }

    match &obj.payload {
        PyObjectPayload::None
        | PyObjectPayload::Bool(_)
        | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_)
        | PyObjectPayload::Str(_)
        | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => {
            let new_items: Vec<_> = items
                .iter()
                .map(|x| deep_copy_with_memo(x, memo))
                .collect::<PyResult<Vec<_>>>()?;
            if let Some(existing) = memo.get(&ptr) {
                return Ok(existing.clone());
            }
            let result = if items
                .iter()
                .zip(new_items.iter())
                .all(|(original, copied)| PyObjectRef::ptr_eq(original, copied))
            {
                obj.clone()
            } else {
                PyObject::tuple(new_items)
            };
            memo.insert(ptr, result.clone());
            Ok(result)
        }
        PyObjectPayload::List(items) => {
            // Pre-insert empty list to handle circular refs
            let result = PyObject::list(vec![]);
            memo.insert(ptr, result.clone());
            let new_items: Result<Vec<_>, _> = items
                .read()
                .iter()
                .map(|x| deep_copy_with_memo(x, memo))
                .collect();
            if let PyObjectPayload::List(new_list) = &result.payload {
                *new_list.write() = new_items?;
            }
            Ok(result)
        }
        PyObjectPayload::Dict(map) => {
            let result = PyObject::dict(IndexMap::new());
            memo.insert(ptr, result.clone());
            let mut new_map = new_fx_hashkey_map();
            for (k, v) in map.read().iter() {
                new_map.insert(k.clone(), deep_copy_with_memo(v, memo)?);
            }
            if let PyObjectPayload::Dict(new_dict) = &result.payload {
                *new_dict.write() = new_map;
            }
            Ok(result)
        }
        PyObjectPayload::Set(set) => {
            let mut new_set = new_fx_hashkey_map();
            for v in set.read().values() {
                let copied = deep_copy_with_memo(v, memo)?;
                let key = copied.to_hashable_key()?;
                new_set.entry(key).or_insert(copied);
            }
            let result = PyObject::set(new_set);
            memo.insert(ptr, result.clone());
            Ok(result)
        }
        PyObjectPayload::Iterator(iter_data) => {
            let data = iter_data.read();
            if let IteratorData::Islice {
                source,
                index,
                next_yield,
                stop,
                step,
            } = &*data
            {
                let source = deep_copy_with_memo(source, memo)?;
                let result = PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
                    IteratorData::Islice {
                        source,
                        index: *index,
                        next_yield: *next_yield,
                        stop: *stop,
                        step: *step,
                    },
                ))));
                memo.insert(ptr, result.clone());
                return Ok(result);
            }
            Ok(obj.clone())
        }
        PyObjectPayload::Instance(inst) => {
            if let Some(deepcopy_fn) = obj.get_attr("__deepcopy__") {
                let mut memo_map = new_fx_hashkey_map();
                for (ptr, value) in memo.iter() {
                    memo_map.insert(HashableKey::Int(PyInt::Small(*ptr as i64)), value.clone());
                }
                let memo_obj = PyObject::dict(memo_map);
                let copied = call_callable(&deepcopy_fn, &[memo_obj.clone()])?;
                memo.insert(ptr, copied.clone());
                if let PyObjectPayload::Dict(updated) = &memo_obj.payload {
                    for (key, value) in updated.read().iter() {
                        if let HashableKey::Int(n) = key {
                            if let Some(ptr) = n.to_i64() {
                                memo.insert(ptr as usize, value.clone());
                            }
                        }
                    }
                }
                return Ok(copied);
            }
            // Pre-insert placeholder instance to handle circular refs
            let result = PyObject::instance_with_attrs(inst.class.clone(), IndexMap::new());
            memo.insert(ptr, result.clone());
            let mut new_attrs = FxAttrMap::default();
            for (k, v) in inst.attrs.read().iter() {
                new_attrs.insert(k.clone(), deep_copy_with_memo(v, memo)?);
            }
            if let PyObjectPayload::Instance(new_inst) = &result.payload {
                *new_inst.attrs.write() = new_attrs;
                if let (Some(src_ds), Some(dst_ds)) = (&inst.dict_storage, &new_inst.dict_storage) {
                    let mut new_map = new_fx_hashkey_map();
                    for (k, v) in src_ds.read().iter() {
                        new_map.insert(k.clone(), deep_copy_with_memo(v, memo)?);
                    }
                    *dst_ds.write() = new_map;
                }
            }
            Ok(result)
        }
        _ => Ok(obj.clone()),
    }
}
