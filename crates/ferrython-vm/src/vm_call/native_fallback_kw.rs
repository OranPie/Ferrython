use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PartialData, PyObject, PyObjectMethods, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::vm_call::exception_build::build_builtin_exception_instance;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_partial_kw(
        &mut self,
        partial: &PartialData,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        let partial_func = partial.func.clone();
        let mut combined_args = partial.args.clone();
        combined_args.extend(pos_args);
        let mut combined_kw = partial.kwargs.clone();
        combined_kw.extend(kwargs);
        if combined_kw.is_empty() {
            self.call_object(partial_func, combined_args)
        } else {
            self.call_object_kw(partial_func, combined_args, combined_kw)
        }
    }

    pub(super) fn call_exception_type_kw(
        &mut self,
        kind: ExceptionKind,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        build_builtin_exception_instance(kind, pos_args, kwargs)
    }

    pub(super) fn call_instance_with_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if func.get_attr("__singledispatch__").is_some() {
            return self.vm_singledispatch_call_instance(&func, &pos_args);
        }
        if let Some(method) = func.get_attr("__call__") {
            let _dispatch_guard = self.enter_frameless_call_dispatch()?;
            return self.call_object_kw(method, pos_args, kwargs);
        }
        Err(PyException::type_error(format!(
            "'{}' object is not callable",
            func.type_name()
        )))
    }

    pub(super) fn call_object_with_trailing_kwargs(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if kwargs.is_empty() {
            return self.call_object(func, pos_args);
        }

        let mut all_args = pos_args;
        let mut kw_map = IndexMap::new();
        for (k, v) in kwargs {
            kw_map.insert(HashableKey::str_key(k), v);
        }
        all_args.push(PyObject::dict(kw_map));
        self.call_object(func, all_args)
    }
}
