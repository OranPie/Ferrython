use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn builtin_type_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if !kwargs.is_empty() && pos_args.len() >= 3 {
            return self.call_object(func, pos_args);
        }
        let mut all_args = pos_args;
        let mut kw_map = IndexMap::new();
        for (k, v) in kwargs {
            kw_map.insert(HashableKey::str_key(k), v);
        }
        if !kw_map.is_empty() {
            all_args.push(PyObject::dict(kw_map));
        }
        self.call_object(func, all_args)
    }

    pub(super) fn call_builtin_kw_fallback(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if !kwargs.is_empty() {
            let mut all_args = pos_args;
            let mut kw_map = IndexMap::new();
            for (k, v) in kwargs {
                kw_map.insert(HashableKey::str_key(k), v);
            }
            if matches!(&func.payload, PyObjectPayload::NativeFunction(nf)
                    if nf.name.as_str() == "weakref.__new__")
            {
                kw_map.insert(
                    HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__")),
                    PyObject::bool_val(true),
                );
            }
            all_args.push(PyObject::dict(kw_map));
            return self.call_object(func, all_args);
        }
        self.call_object(func, pos_args)
    }

    pub(super) fn call_builtin_kw_trailing_dict(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if !kwargs.is_empty() {
            let mut all_args = pos_args;
            let mut kw_map = IndexMap::new();
            for (k, v) in kwargs {
                kw_map.insert(HashableKey::str_key(k), v);
            }
            all_args.push(PyObject::dict(kw_map));
            return self.call_object(func, all_args);
        }
        self.call_object(func, pos_args)
    }
}
