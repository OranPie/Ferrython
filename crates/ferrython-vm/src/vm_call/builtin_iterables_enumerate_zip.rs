use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_enumerate_builtin(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if !args.is_empty() {
            let mut resolved = Vec::with_capacity(args.len());
            resolved.push(self.resolve_iterable(&args[0])?);
            resolved.extend_from_slice(&args[1..]);
            return builtins::dispatch("enumerate", &resolved);
        }
        builtins::dispatch("enumerate", args)
    }

    pub(super) fn call_zip_builtin(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let mut strict = false;
        let iter_end = if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw) = &last.payload {
                let read = kw.read();
                if let Some(value) = read.get(&HashableKey::str_key(CompactString::from("strict")))
                {
                    strict = value.is_truthy();
                }
                drop(read);
                args.len() - 1
            } else {
                args.len()
            }
        } else {
            args.len()
        };
        let resolved = self.resolve_iterables(&args[..iter_end])?;
        let mut full_args = resolved;
        if strict {
            let kw = PyObject::dict(indexmap::IndexMap::from([(
                HashableKey::str_key(CompactString::from("strict")),
                PyObject::bool_val(true),
            )]));
            full_args.push(kw);
        }
        builtins::dispatch("zip", &full_args)
    }
}
