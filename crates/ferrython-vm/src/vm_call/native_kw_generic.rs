use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{NativeFunctionData, PyObject, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_native_function_trailing_kw(
        &mut self,
        nf_data: &NativeFunctionData,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if kwargs.is_empty() {
            return (nf_data.func)(&pos_args);
        }

        let mut all_args = pos_args;
        let mut kw_map = IndexMap::new();
        for (k, v) in kwargs {
            kw_map.insert(HashableKey::str_key(k), v);
        }
        if nf_data.name.as_str() == "UserDict.__init__" {
            kw_map.insert(
                HashableKey::str_key(CompactString::from("__userdict_kwargs__")),
                PyObject::bool_val(true),
            );
        }
        if matches!(
            nf_data.name.as_str(),
            "weakref.__new__" | "weakref.__init__"
        ) {
            kw_map.insert(
                HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__")),
                PyObject::bool_val(true),
            );
        }
        if matches!(
            nf_data.name.as_str(),
            "functools.cmp_to_key" | "cmp_to_key.__init__"
        ) {
            kw_map.insert(
                HashableKey::str_key(CompactString::from("__cmp_to_key_kwargs__")),
                PyObject::bool_val(true),
            );
        }
        all_args.push(PyObject::dict(kw_map));
        (nf_data.func)(&all_args)
    }
}
