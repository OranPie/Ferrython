use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    lookup_in_class_mro, FxHashKeyMap, PyCell, PyObject, PyObjectMethods, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use std::rc::Rc;

use crate::VirtualMachine;

impl VirtualMachine {
    /// str.format_map() with dict subclass mapping, supporting __missing__ via VM call dispatch.
    pub(super) fn vm_format_map(
        &mut self,
        template: &str,
        mapping: &PyObjectRef,
        dict_storage: &Rc<PyCell<FxHashKeyMap>>,
        mapping_class: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let mut result = String::new();
        let mut chars = template.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    result.push('{');
                } else {
                    let mut field = String::new();
                    for c in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                        field.push(c);
                    }
                    let key = HashableKey::str_key(CompactString::from(&field));
                    if let Some(val) = dict_storage.read().get(&key) {
                        result.push_str(&val.py_to_string());
                    } else if let Some(missing_fn) =
                        lookup_in_class_mro(mapping_class, "__missing__")
                    {
                        // Call __missing__(self, key) via VM dispatch
                        let key_obj = PyObject::str_val(CompactString::from(&field));
                        let val = self.call_object(missing_fn, vec![mapping.clone(), key_obj])?;
                        result.push_str(&val.py_to_string());
                    } else {
                        return Err(PyException::key_error(field));
                    }
                }
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                result.push('}');
            } else {
                result.push(c);
            }
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }

    /// str.format_map() for defaultdict (Dict payload with __defaultdict_factory__).
    pub(super) fn vm_format_map_dict(
        &mut self,
        template: &str,
        _mapping: &PyObjectRef,
        dict: &Rc<PyCell<FxHashKeyMap>>,
    ) -> PyResult<PyObjectRef> {
        let factory_key = HashableKey::str_key(CompactString::from("__defaultdict_factory__"));
        let mut result = String::new();
        let mut chars = template.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    result.push('{');
                } else {
                    let mut field = String::new();
                    for c in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                        field.push(c);
                    }
                    let key = HashableKey::str_key(CompactString::from(&field));
                    let guard = dict.read();
                    if let Some(val) = guard.get(&key) {
                        result.push_str(&val.py_to_string());
                    } else if let Some(factory) = guard.get(&factory_key).cloned() {
                        drop(guard);
                        let val = self.call_object(factory, vec![])?;
                        dict.write().insert(key, val.clone());
                        result.push_str(&val.py_to_string());
                    } else {
                        return Err(PyException::key_error(field));
                    }
                }
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                result.push('}');
            } else {
                result.push(c);
            }
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }
}
