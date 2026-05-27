use ferrython_core::object::{
    new_fx_hashkey_flatmap, new_fx_hashkey_map, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};

use crate::VirtualMachine;

pub(super) fn set_builtin_value(vm: &mut VirtualMachine, arg: &PyObjectRef) -> PyObjectRef {
    if let PyObjectPayload::Dict(items) = &arg.payload {
        let read = items.read();
        let mut map = new_fx_hashkey_flatmap();
        map.reserve(read.len());
        for key in read.keys() {
            map.insert(key.clone(), key.to_object());
        }
        return PyObject::set_from_flatmap(map);
    }

    let mut map = new_fx_hashkey_flatmap();
    for item in vm.collect_iterable(arg).unwrap_or_default() {
        if let Ok(key) = item.to_hashable_key() {
            map.insert(key, item);
        }
    }
    PyObject::set_from_flatmap(map)
}

pub(super) fn frozenset_builtin_value(vm: &mut VirtualMachine, arg: &PyObjectRef) -> PyObjectRef {
    if let PyObjectPayload::Dict(items) = &arg.payload {
        let read = items.read();
        let mut map = new_fx_hashkey_map();
        for key in read.keys() {
            map.insert(key.clone(), key.to_object());
        }
        return PyObject::frozenset(map);
    }

    let mut map = new_fx_hashkey_map();
    for item in vm.collect_iterable(arg).unwrap_or_default() {
        if let Ok(key) = item.to_hashable_key() {
            map.insert(key, item);
        }
    }
    PyObject::frozenset(map)
}
