use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    PartialData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_partial_object(
        &mut self,
        partial: &PartialData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        let partial_func = partial.func.clone();
        let mut combined_args = partial.args.clone();
        combined_args.extend(args);
        if !partial.kwargs.is_empty() {
            let kwargs: Vec<(CompactString, PyObjectRef)> = partial
                .kwargs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            self.call_object_kw(partial_func, combined_args, kwargs)
        } else {
            self.call_object(partial_func, combined_args)
        }
    }

    pub(super) fn call_instance_object(
        &mut self,
        func: PyObjectRef,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if let Some(result) = self.call_lru_cache_instance(&func, &args)? {
            return Ok(result);
        }
        if func.get_attr("__singledispatch__").is_some() {
            return self.vm_singledispatch_call_instance(&func, &args);
        }
        if let Some(method) = func.get_attr("__call__") {
            let _dispatch_guard = self.enter_frameless_call_dispatch()?;
            let result = self.call_object(method, args);
            drop(func);
            result
        } else {
            Err(PyException::type_error(format!(
                "'{}' object is not callable",
                func.type_name()
            )))
        }
    }

    fn call_lru_cache_instance(
        &mut self,
        func: &PyObjectRef,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        let Some(cache_obj) = func.get_attr("_cache") else {
            return Ok(None);
        };
        let Some(wrapped) = func.get_attr("__wrapped__") else {
            return Ok(None);
        };
        let PyObjectPayload::Dict(cache_map) = &cache_obj.payload else {
            return Ok(None);
        };

        let key_str = args.iter().map(|a| a.repr()).collect::<Vec<_>>().join(",");
        let cache_key = HashableKey::str_key(CompactString::from(&key_str));
        if let Some(cached) = cache_map.read().get(&cache_key).cloned() {
            {
                let mut cache_write = cache_map.write();
                cache_write.shift_remove(&cache_key);
                cache_write.insert(cache_key, cached.clone());
            }
            increment_instance_counter(func, "_hits");
            return Ok(Some(cached));
        }

        increment_instance_counter(func, "_misses");
        let result = self.call_object(wrapped, args.to_vec())?;
        {
            let mut cache_write = cache_map.write();
            if let Some(max) = lru_cache_maxsize(func) {
                if max >= 0 {
                    while cache_write.len() >= max as usize {
                        cache_write.shift_remove_index(0);
                    }
                }
            }
            cache_write.insert(cache_key, result.clone());
        }
        Ok(Some(result))
    }
}

fn increment_instance_counter(obj: &PyObjectRef, name: &str) {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let mut attrs = inst.attrs.write();
        let key = intern_or_new(name);
        let value = attrs.get(&key).and_then(|v| v.as_int()).unwrap_or(0);
        attrs.insert(key, PyObject::int(value + 1));
    }
}

fn lru_cache_maxsize(obj: &PyObjectRef) -> Option<i64> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs
            .read()
            .get(&intern_or_new("_maxsize"))
            .and_then(|v| v.as_int())
    } else {
        None
    }
}
