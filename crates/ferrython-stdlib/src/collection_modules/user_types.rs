use crate::introspection_modules::emit_deprecation_warning;
use compact_str::CompactString;
use ferrython_core::error::ExceptionKind;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    call_callable, is_hidden_dict_key, lookup_in_class_mro, new_fx_hashkey_map, repr_enter,
    repr_leave, BuiltinFn, CompareOp, FxHashKeyMap, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef, SharedFxAttrMap,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

mod user_dict;
mod user_list;
mod user_string;

pub(super) use user_dict::make_user_dict_class;
pub(super) use user_list::make_user_list_class;
pub(super) use user_string::make_user_string_class;

fn native_method(class_name: &str, method_name: &str, f: BuiltinFn) -> PyObjectRef {
    PyObject::native_function(&format!("{class_name}.{method_name}"), f)
}

fn copy_instance_attrs(src_attrs: &SharedFxAttrMap, dst_attrs: &SharedFxAttrMap, skip: &[&str]) {
    let src = src_attrs.read();
    let mut dst = dst_attrs.write();
    for (name, value) in src.iter() {
        if skip.iter().any(|s| *s == name.as_str()) {
            continue;
        }
        dst.insert(name.clone(), value.clone());
    }
}

fn get_user_data(obj: &PyObjectRef, attr: &str) -> PyResult<PyObjectRef> {
    if let PyObjectPayload::Instance(d) = &obj.payload {
        if let Some(v) = d.attrs.read().get(attr) {
            return Ok(v.clone());
        }
    }
    Err(PyException::attribute_error(format!(
        "'{}' object has no attribute '{}'",
        obj.type_name(),
        attr
    )))
}
