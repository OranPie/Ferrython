use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::vm_call::exception_build::build_builtin_exception_instance;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(crate) fn call_object(
        &mut self,
        func: PyObjectRef,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        let frame_guard = self.enter_call_object_trace_frame(&func);
        let result = match &func.payload {
            PyObjectPayload::Function(pyfunc) => {
                // Borrow fields directly from the Arc-backed func instead of cloning
                // expensive Vec/IndexMap payloads. Only globals needs cloning (moved into frame).
                let globals = pyfunc.globals.clone();
                let defaults = pyfunc.defaults.read();
                let kw_defaults = pyfunc.kw_defaults.read();
                let attrs = pyfunc.attrs.read();
                let func_name = attrs
                    .get("__name__")
                    .and_then(|v| v.as_str())
                    .map(CompactString::from)
                    .unwrap_or_else(|| pyfunc.name.clone());
                let func_qualname = attrs
                    .get("__qualname__")
                    .and_then(|v| v.as_str())
                    .map(CompactString::from)
                    .unwrap_or_else(|| pyfunc.qualname.clone());
                drop(attrs);
                self.call_function(
                    &pyfunc.code,
                    func_name,
                    func_qualname,
                    args,
                    &defaults,
                    &kw_defaults,
                    globals,
                    &pyfunc.closure,
                    &pyfunc.constant_cache,
                )
            }
            PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
                self.call_builtin_or_type(&func, name, args)
            }
            PyObjectPayload::Class(cd) => self.call_class_object(&func, cd, args),
            PyObjectPayload::BoundMethod { receiver, method } => {
                // VM intercept: RawIOBase.read(size=-1) → calls self.readinto()
                if let PyObjectPayload::NativeFunction(nf) = &method.payload {
                    if nf.name.as_str() == "RawIOBase.read" {
                        let size: i64 = args.first().and_then(|a| a.as_int()).unwrap_or(-1);
                        return self.rawiobase_read(receiver, size);
                    }
                    if nf.name.as_str() == "RawIOBase.readall" {
                        return self.rawiobase_readall(receiver);
                    }
                }
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(args);
                self.call_object(method.clone(), bound_args)
            }
            PyObjectPayload::BuiltinBoundMethod(bbm) => self.call_builtin_bound_method(bbm, args),
            PyObjectPayload::ExceptionType(kind) => {
                build_builtin_exception_instance(*kind, args, &[])
            }
            PyObjectPayload::NativeFunction(nf_data) => {
                self.call_native_function_object(nf_data, args)
            }
            PyObjectPayload::NativeClosure(nc) => self.call_native_closure_object(nc, args),
            PyObjectPayload::Partial(pd) => self.call_partial_object(pd, args),
            PyObjectPayload::Instance(_inst) => self.call_instance_object(func, args),
            _ => Err(PyException::type_error(format!(
                "'{}' object is not callable",
                func.type_name()
            ))),
        };
        self.leave_call_object_trace_frame(frame_guard);
        result
    }
}
