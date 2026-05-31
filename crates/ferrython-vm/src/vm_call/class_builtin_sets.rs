use ferrython_core::error::PyResult;
use ferrython_core::object::{
    new_fx_hashkey_flatmap, new_fx_hashkey_map, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::take_pending_eq_error;

use crate::VirtualMachine;

pub(super) fn set_builtin_value(
    vm: &mut VirtualMachine,
    arg: &PyObjectRef,
) -> PyResult<PyObjectRef> {
    if let PyObjectPayload::Dict(items) = &arg.payload {
        let read = items.read();
        let mut map = new_fx_hashkey_flatmap();
        map.reserve(read.len());
        for key in read.keys() {
            map.insert(key.clone(), key.to_object());
        }
        return Ok(PyObject::set_from_flatmap(map));
    }

    let mut map = new_fx_hashkey_flatmap();
    for item in vm.collect_iterable(arg)? {
        let key = item.to_hashable_key()?;
        map.insert(key, item);
        if let Some(err) = take_pending_eq_error() {
            return Err(err);
        }
    }
    Ok(PyObject::set_from_flatmap(map))
}

pub(super) fn frozenset_builtin_value(
    vm: &mut VirtualMachine,
    arg: &PyObjectRef,
) -> PyResult<PyObjectRef> {
    if matches!(&arg.payload, PyObjectPayload::FrozenSet(_)) {
        return Ok(arg.clone());
    }

    if let PyObjectPayload::Dict(items) = &arg.payload {
        let read = items.read();
        let mut map = new_fx_hashkey_map();
        for key in read.keys() {
            map.insert(key.clone(), key.to_object());
        }
        return Ok(PyObject::frozenset(map));
    }

    let mut map = new_fx_hashkey_map();
    for item in vm.collect_iterable(arg)? {
        let key = item.to_hashable_key()?;
        map.insert(key, item);
        if let Some(err) = take_pending_eq_error() {
            return Err(err);
        }
    }
    Ok(PyObject::frozenset(map))
}
