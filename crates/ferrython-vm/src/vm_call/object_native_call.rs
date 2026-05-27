use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    NativeClosureData, NativeFunctionData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_native_function_object(
        &mut self,
        nf_data: &NativeFunctionData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if nf_data.name.as_str() == "_ast.AST.__init__" {
            if args.is_empty() {
                return Err(PyException::type_error("__init__ requires self"));
            }
            let (pos_args, kwargs) = Self::split_trailing_kwargs_dict(&args);
            if pos_args.is_empty() {
                return Err(PyException::type_error("__init__ requires self"));
            }
            let instance = &pos_args[0];
            let cls = match &instance.payload {
                PyObjectPayload::Instance(inst) => inst.class.clone(),
                _ => {
                    return Err(PyException::type_error(
                        "AST.__init__ requires an AST instance",
                    ))
                }
            };
            Self::populate_ast_node_attrs(instance, &cls, &pos_args[1..], &kwargs)?;
            return Ok(PyObject::none());
        }
        if nf_data.name.as_str() == "_ast.AST.__new__" {
            if args.is_empty() {
                return Err(PyException::type_error("__new__ requires cls"));
            }
            let (pos_args, kwargs) = Self::split_trailing_kwargs_dict(&args);
            if pos_args.is_empty() {
                return Err(PyException::type_error("__new__ requires cls"));
            }
            let cls = pos_args[0].clone();
            let pos_args = pos_args[1..].to_vec();
            return Ok(self
                .try_instantiate_ast_node(&cls, pos_args, kwargs)?
                .unwrap_or_else(|| PyObject::instance(cls)));
        }
        if nf_data.name.as_str() == "property.__get__" {
            return self.call_property_get_native(&args);
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
        if nf_data.name.as_str() == "__type_call__" {
            if args.is_empty() {
                return Err(PyException::type_error("type.__call__ requires cls"));
            }
            let cls = args[0].clone();
            let rest = args[1..].to_vec();
            return self.instantiate_class(&cls, rest, vec![]);
        }
        if (nf_data.name.as_str() == "re.sub" || nf_data.name.as_str() == "re.subn")
            && args.len() >= 3
        {
            let repl = &args[1];
            let is_callable = matches!(
                &repl.payload,
                PyObjectPayload::Function(_)
                    | PyObjectPayload::BuiltinFunction(_)
                    | PyObjectPayload::NativeFunction(_)
                    | PyObjectPayload::NativeClosure(_)
                    | PyObjectPayload::Partial(_)
            );
            if is_callable {
                return self.re_sub_with_callable(&args, nf_data.name.as_str() == "re.subn");
            }
        }
        if nf_data.name.as_str() == "itertools.groupby" {
            return self.call_itertools_groupby_native(&args);
        }
        if nf_data.name.as_str() == "itertools.filterfalse" && args.len() >= 2 {
            return self.vm_itertools_filterfalse(&args);
        }
        if nf_data.name.as_str() == "itertools.starmap" && args.len() >= 2 {
            return self.vm_itertools_starmap(&args);
        }
        if nf_data.name.as_str() == "itertools.accumulate" && args.len() >= 2 {
            return self.vm_itertools_accumulate(&args);
        }
        if nf_data.name.as_str() == "dict.fromkeys"
            && !args.is_empty()
            && matches!(
                &args[0].payload,
                PyObjectPayload::Generator(_)
                    | PyObjectPayload::Instance(_)
                    | PyObjectPayload::Iterator(_)
            )
        {
            let mut resolved = Vec::with_capacity(args.len());
            resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
            resolved.extend_from_slice(&args[1..]);
            return (nf_data.func)(&resolved);
        }
        if args.len() == 1 {
            if let PyObjectPayload::Instance(_) = &args[0].payload {
                let dunder = match nf_data.name.as_str() {
                    "math.trunc" => Some("__trunc__"),
                    "math.floor" => Some("__floor__"),
                    "math.ceil" => Some("__ceil__"),
                    _ => None,
                };
                if let Some(dunder_name) = dunder {
                    if let Some(method) = args[0].get_attr(dunder_name) {
                        let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                            vec![]
                        } else {
                            vec![args[0].clone()]
                        };
                        return self.call_object(method, ca);
                    }
                }
            }
        }
        if nf_data.name.as_str() == "os.fspath" && args.len() == 1 {
            if let PyObjectPayload::Instance(_) = &args[0].payload {
                if let Some(method) = args[0].get_attr("__fspath__") {
                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                        vec![]
                    } else {
                        vec![args[0].clone()]
                    };
                    return self.call_object(method, ca);
                }
            }
        }
        if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Generator(_)) {
            let mut resolved = Vec::with_capacity(args.len());
            resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
            resolved.extend_from_slice(&args[1..]);
            return (nf_data.func)(&resolved);
        }

        let result = (nf_data.func)(&args)?;
        self.finish_native_callable_result(result, false)
    }

    pub(super) fn call_native_closure_object(
        &mut self,
        nc: &NativeClosureData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        let args = if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Generator(_))
        {
            let mut resolved = Vec::with_capacity(args.len());
            resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
            resolved.extend_from_slice(&args[1..]);
            resolved
        } else {
            args
        };
        let result = (nc.func)(&args)?;
        self.finish_native_callable_result(result, true)
    }

    fn call_property_get_native(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "descriptor '__get__' requires a property object",
            ));
        }
        let prop = &args[0];
        let obj = args.get(1);
        let is_none_obj = match obj {
            Some(o) => matches!(&o.payload, PyObjectPayload::None),
            None => true,
        };
        if is_none_obj {
            return Ok(prop.clone());
        }
        let obj = obj.unwrap();
        if let PyObjectPayload::Property(pd) = &prop.payload {
            if let Some(getter) = pd.fget.as_ref() {
                let getter = crate::builtins::unwrap_abstract_fget(getter);
                return self.call_object(getter, vec![obj.clone()]);
            }
            return Err(PyException::attribute_error("unreadable attribute"));
        }
        if let PyObjectPayload::Instance(inst) = &prop.payload {
            if let Some(fget) = inst.attrs.read().get("fget").cloned() {
                if !matches!(&fget.payload, PyObjectPayload::None) {
                    return self.call_object(fget, vec![obj.clone()]);
                }
            }
        }
        Err(PyException::attribute_error("unreadable attribute"))
    }

    fn call_itertools_groupby_native(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let mut key_fn = None;
        let mut iterable_end = args.len();
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(map) = &last.payload {
                let map_r = map.read();
                key_fn = map_r
                    .get(&HashableKey::str_key(CompactString::from("key")))
                    .cloned();
                if key_fn.is_some() {
                    iterable_end = args.len() - 1;
                }
            }
        }
        if key_fn.is_none() && iterable_end >= 2 {
            key_fn = Some(args[1].clone());
            iterable_end = 1;
        }
        self.vm_itertools_groupby(&args[..iterable_end], key_fn)
    }

    fn finish_native_callable_result(
        &mut self,
        result: PyObjectRef,
        check_asyncio_run: bool,
    ) -> PyResult<PyObjectRef> {
        let collect_mode = ferrython_core::error::take_collect_vm_call_results();
        if collect_mode {
            let mut collected = Vec::new();
            while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                collected.push(self.call_object(method, margs)?);
            }
            if !collected.is_empty() {
                return Ok(PyObject::list(collected));
            }
        }
        while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
            self.call_object(method, margs)?;
        }
        let deferred = ferrython_stdlib::drain_deferred_calls();
        for (dfunc, dargs) in deferred {
            self.call_object(dfunc, dargs)?;
        }
        if check_asyncio_run {
            if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
                return self.maybe_await_result(coro);
            }
        }
        Ok(result)
    }
}
