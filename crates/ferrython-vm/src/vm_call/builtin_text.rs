use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{new_fx_hashkey_map, PyCell, PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use std::rc::Rc;

use crate::vm_call::str_fast::fast_exact_str;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_text_builtin(
        &mut self,
        name: &str,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name {
            "str" => self.call_str_builtin(args),
            "repr" => self.call_repr_builtin(args),
            "mappingproxy" => self.call_mappingproxy_builtin(args),
            _ => unreachable!("non-text builtin routed to text dispatch"),
        }
    }

    fn call_str_builtin(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Ok(PyObject::str_val(CompactString::from("")));
        }
        if args.len() >= 2 {
            match &args[0].payload {
                PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                    let s = String::from_utf8_lossy(b);
                    return Ok(PyObject::str_val(CompactString::from(s.as_ref())));
                }
                _ => {}
            }
        }
        if args.len() == 1 {
            if let Some(result) = fast_exact_str(&args[0]) {
                return Ok(result);
            }
        }
        self.vm_str(&args[0])
            .map(|s| PyObject::str_val(CompactString::from(s)))
    }

    fn call_repr_builtin(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Ok(PyObject::str_val(CompactString::from("")));
        }
        ferrython_core::object::helpers::repr_reset_overflow();
        let text = self.vm_repr(&args[0])?;
        if ferrython_core::object::helpers::repr_depth_exceeded() {
            return Err(PyException::recursion_error(
                "maximum recursion depth exceeded while getting the repr of an object",
            ));
        }
        Ok(PyObject::str_val(CompactString::from(text)))
    }

    fn call_mappingproxy_builtin(&self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.len() == 1 {
            let src = &args[0];
            let map = match &src.payload {
                PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => m.clone(),
                PyObjectPayload::InstanceDict(attrs) => {
                    let rd = attrs.read();
                    let mut m = new_fx_hashkey_map();
                    for (k, v) in rd.iter() {
                        m.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                    Rc::new(PyCell::new(m))
                }
                _ => {
                    return Err(PyException::type_error(
                        "mappingproxy() argument must be a mapping, not a non-mapping type",
                    ));
                }
            };
            return Ok(PyObject::wrap(PyObjectPayload::MappingProxy(map)));
        }
        if args.is_empty() {
            return Err(PyException::type_error(
                "mappingproxy() missing required argument: 'mapping'",
            ));
        }
        fallback_text_builtin("mappingproxy", &args)
    }
}

fn fallback_text_builtin(name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match crate::builtins::get_builtin_fn(name) {
        Some(f) => f(args),
        None => unreachable!("text builtin missing fallback"),
    }
}
