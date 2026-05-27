use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{NativeClosureData, PyObject, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_native_closure_kw(
        &mut self,
        nc: &NativeClosureData,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        let mut counter_kw_marker = false;
        let mut defaultdict_kw_marker = false;
        let mut weakdict_kw_marker = false;
        let mut finalize_kw_marker = false;
        let mut adjusted_kwargs = kwargs;
        if !adjusted_kwargs.is_empty() && nc.name.as_str().starts_with("Counter.") {
            counter_kw_marker = true;
            adjusted_kwargs.push((
                CompactString::from("__counter_kwargs__"),
                PyObject::bool_val(true),
            ));
        }
        if !adjusted_kwargs.is_empty() && nc.name.as_str().starts_with("defaultdict.") {
            defaultdict_kw_marker = true;
            adjusted_kwargs.push((
                CompactString::from("__defaultdict_kwargs__"),
                PyObject::bool_val(true),
            ));
        }
        if !adjusted_kwargs.is_empty()
            && (nc.name.as_str() == "WeakValueDictionary.update"
                || nc.name.as_str() == "WeakKeyDictionary.update")
        {
            weakdict_kw_marker = true;
            adjusted_kwargs.push((
                CompactString::from("__weakdict_kwargs__"),
                PyObject::bool_val(true),
            ));
        }
        if !adjusted_kwargs.is_empty()
            && (nc.name.as_str() == "finalize" || nc.name.as_str() == "finalize.__new__")
        {
            finalize_kw_marker = true;
            adjusted_kwargs.push((
                CompactString::from("__finalize_kwargs__"),
                PyObject::bool_val(true),
            ));
        }
        if !adjusted_kwargs.is_empty() && nc.name.as_str() == "weakref.__new__" {
            adjusted_kwargs.push((
                CompactString::from("__weakref_ref_kwargs__"),
                PyObject::bool_val(true),
            ));
        }

        let result = if !adjusted_kwargs.is_empty() {
            let mut all_args = pos_args;
            let mut kw_map = IndexMap::new();
            for (k, v) in adjusted_kwargs {
                kw_map.insert(HashableKey::str_key(k), v);
            }
            if counter_kw_marker {
                kw_map.insert(
                    HashableKey::str_key(CompactString::from("__counter_kwargs__")),
                    PyObject::bool_val(true),
                );
            }
            if defaultdict_kw_marker {
                kw_map.insert(
                    HashableKey::str_key(CompactString::from("__defaultdict_kwargs__")),
                    PyObject::bool_val(true),
                );
            }
            if weakdict_kw_marker {
                kw_map.insert(
                    HashableKey::str_key(CompactString::from("__weakdict_kwargs__")),
                    PyObject::bool_val(true),
                );
            }
            if finalize_kw_marker {
                kw_map.insert(
                    HashableKey::str_key(CompactString::from("__finalize_kwargs__")),
                    PyObject::bool_val(true),
                );
            }
            all_args.push(PyObject::dict(kw_map));
            (nc.func)(&all_args)?
        } else {
            (nc.func)(&pos_args)?
        };

        if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
            return self.maybe_await_result(coro);
        }
        Ok(result)
    }
}
