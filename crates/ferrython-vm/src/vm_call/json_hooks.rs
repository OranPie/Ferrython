use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    /// Post-process parsed JSON: apply object_hook, parse_float, parse_int
    /// by calling Python functions via the VM.
    pub(super) fn json_apply_hooks(
        &mut self,
        value: &PyObjectRef,
        object_hook: &Option<PyObjectRef>,
        parse_float: &Option<PyObjectRef>,
        parse_int: &Option<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match &value.payload {
            PyObjectPayload::Dict(map) => {
                // Recursively apply hooks to values first
                let entries: Vec<_> = map
                    .read()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut new_map = IndexMap::new();
                for (k, v) in entries {
                    new_map.insert(
                        k,
                        self.json_apply_hooks(&v, object_hook, parse_float, parse_int)?,
                    );
                }
                let new_dict = PyObject::dict(new_map);
                // Apply object_hook to the dict
                if let Some(hook) = object_hook {
                    self.call_object(hook.clone(), vec![new_dict])
                } else {
                    Ok(new_dict)
                }
            }
            PyObjectPayload::List(items) => {
                let items: Vec<_> = items.read().clone();
                let mut result = Vec::with_capacity(items.len());
                for item in &items {
                    result.push(self.json_apply_hooks(
                        item,
                        object_hook,
                        parse_float,
                        parse_int,
                    )?);
                }
                Ok(PyObject::list(result))
            }
            PyObjectPayload::Float(_) => {
                if let Some(pf) = parse_float {
                    let s = PyObject::str_val(CompactString::from(value.py_to_string()));
                    self.call_object(pf.clone(), vec![s])
                } else {
                    Ok(value.clone())
                }
            }
            PyObjectPayload::Int(_) => {
                if let Some(pi) = parse_int {
                    let s = PyObject::str_val(CompactString::from(value.py_to_string()));
                    self.call_object(pi.clone(), vec![s])
                } else {
                    Ok(value.clone())
                }
            }
            _ => Ok(value.clone()),
        }
    }

    /// Pre-process an object tree for json.dumps: replace non-JSON-serializable
    /// values by calling `default(obj)` (a user Python function). Basic types
    /// (dict, list, tuple, str, int, float, bool, None) are passed through.
    pub(super) fn json_prepare_with_default(
        &mut self,
        obj: &PyObjectRef,
        default_fn: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        match &obj.payload {
            PyObjectPayload::Dict(map) => {
                let entries: Vec<_> = map
                    .read()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut new_map = IndexMap::new();
                for (k, v) in entries {
                    new_map.insert(k, self.json_prepare_with_default(&v, default_fn)?);
                }
                Ok(PyObject::dict(new_map))
            }
            PyObjectPayload::InstanceDict(map) => {
                // Instance __dict__ uses CompactString keys, convert to HashableKey
                let entries: Vec<_> = map
                    .read()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut new_map = IndexMap::new();
                for (k, v) in entries {
                    let prepared = self.json_prepare_with_default(&v, default_fn)?;
                    new_map.insert(HashableKey::str_key(k), prepared);
                }
                Ok(PyObject::dict(new_map))
            }
            PyObjectPayload::List(items) => {
                let items: Vec<_> = items.read().clone();
                let mut prepared = Vec::with_capacity(items.len());
                for item in &items {
                    prepared.push(self.json_prepare_with_default(item, default_fn)?);
                }
                Ok(PyObject::list(prepared))
            }
            PyObjectPayload::Tuple(items) => {
                let mut prepared = Vec::with_capacity(items.len());
                for item in items.iter() {
                    prepared.push(self.json_prepare_with_default(item, default_fn)?);
                }
                Ok(PyObject::tuple(prepared))
            }
            PyObjectPayload::Str(_)
            | PyObjectPayload::Int(_)
            | PyObjectPayload::Float(_)
            | PyObjectPayload::Bool(_)
            | PyObjectPayload::None => Ok(obj.clone()),
            _ => {
                // Call default(obj) and recursively prepare the result
                let result = self.call_object(default_fn.clone(), vec![obj.clone()])?;
                self.json_prepare_with_default(&result, default_fn)
            }
        }
    }
}
