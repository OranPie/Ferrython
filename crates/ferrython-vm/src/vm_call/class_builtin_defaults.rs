use compact_str::CompactString;
use ferrython_core::object::{new_fx_hashkey_flatmap, new_fx_hashkey_map, PyObject, PyObjectRef};

pub(super) fn default_builtin_subclass_value(base_type: &str) -> Option<PyObjectRef> {
    match base_type {
        "list" => Some(PyObject::list(vec![])),
        "set" => Some(PyObject::set_from_flatmap(new_fx_hashkey_flatmap())),
        "frozenset" => Some(PyObject::frozenset(new_fx_hashkey_map())),
        "tuple" => Some(PyObject::tuple(vec![])),
        "int" => Some(PyObject::int(0)),
        "float" => Some(PyObject::float(0.0)),
        "str" => Some(PyObject::str_val(CompactString::from(""))),
        "bytes" => Some(PyObject::bytes(vec![])),
        "bytearray" => Some(PyObject::bytes(vec![])),
        "deque" => Some(PyObject::list(vec![])),
        _ => None,
    }
}
