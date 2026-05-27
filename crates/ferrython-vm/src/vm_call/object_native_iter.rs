use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    NativeFunctionData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_iter_regex_or_path_native_object(
        &mut self,
        nf_data: &NativeFunctionData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
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
                return self
                    .re_sub_with_callable(args, nf_data.name.as_str() == "re.subn")
                    .map(Some);
            }
        }
        if nf_data.name.as_str() == "itertools.groupby" {
            return self.call_itertools_groupby_native(args).map(Some);
        }
        if nf_data.name.as_str() == "itertools.filterfalse" && args.len() >= 2 {
            return self.vm_itertools_filterfalse(args).map(Some);
        }
        if nf_data.name.as_str() == "itertools.starmap" && args.len() >= 2 {
            return self.vm_itertools_starmap(args).map(Some);
        }
        if nf_data.name.as_str() == "itertools.accumulate" && args.len() >= 2 {
            return self.vm_itertools_accumulate(args).map(Some);
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
            return (nf_data.func)(&resolved).map(Some);
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
                        return self.call_object(method, ca).map(Some);
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
                    return self.call_object(method, ca).map(Some);
                }
            }
        }
        if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Generator(_)) {
            let mut resolved = Vec::with_capacity(args.len());
            resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
            resolved.extend_from_slice(&args[1..]);
            return (nf_data.func)(&resolved).map(Some);
        }
        Ok(None)
    }

    pub(super) fn call_itertools_groupby_native(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<PyObjectRef> {
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
}
