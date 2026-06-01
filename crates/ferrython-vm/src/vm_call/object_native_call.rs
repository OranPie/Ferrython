use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    NativeFunctionData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_native_function_object(
        &mut self,
        nf_data: &NativeFunctionData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if let Some(result) = self.call_ast_or_type_native_object(nf_data, &args)? {
            return Ok(result);
        }
        if nf_data.name.as_str() == "dict.fromkeys"
            && args.len() >= 2
            && matches!(&args[0].payload, PyObjectPayload::Class(_))
        {
            let value = args.get(2).cloned().unwrap_or_else(PyObject::none);
            return self.dict_fromkeys_for_class(&args[0], &args[1], value);
        }
        if nf_data.name.as_str() == "property.__get__" {
            return self.call_property_get_native(&args);
        }
        if nf_data.name.as_str() == "type.__new__" {
            let bases = if args.len() == 4 {
                args[2].to_list().unwrap_or_default()
            } else if args.len() == 3 {
                args[1].to_list().unwrap_or_default()
            } else {
                vec![]
            };
            let result = (nf_data.func)(&args)?;
            if matches!(&result.payload, PyObjectPayload::Class(_)) {
                self.finish_pep487_class(&result, &bases, &[])?;
                if let PyObjectPayload::Class(cd) = &result.payload {
                    cd.namespace.write().insert(
                        CompactString::from("__ferrython_pep487_done__"),
                        PyObject::bool_val(true),
                    );
                }
            }
            return Ok(result);
        }
        if nf_data.name.as_str() == "functools.reduce" {
            return self.vm_functools_reduce(&args);
        }
        if nf_data.name.as_str() == "itertools.islice" {
            return self.vm_itertools_islice(&args);
        }
        if nf_data.name.as_str() == "singledispatch.register" {
            return self.vm_singledispatch_register(&args);
        }
        if nf_data.name.as_str() == "collections.deque" {
            let resolved = self.resolve_deque_constructor_args(&args, &[])?;
            return (nf_data.func)(&resolved);
        }
        if nf_data.name.as_str() == "UserList.__init__" && args.len() > 1 {
            let mut resolved = Vec::with_capacity(args.len());
            resolved.push(args[0].clone());
            resolved.push(PyObject::list(self.collect_iterable(&args[1])?));
            resolved.extend_from_slice(&args[2..]);
            return (nf_data.func)(&resolved);
        }
        if let Some(result) = self.call_iter_regex_or_path_native_object(nf_data, &args)? {
            return Ok(result);
        }

        let result = (nf_data.func)(&args)?;
        self.finish_native_callable_result(result, false)
    }
}
