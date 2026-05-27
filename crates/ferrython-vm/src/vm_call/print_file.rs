use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    /// Write text to a file-like object, handling both BoundMethod (e.g. StringIO)
    /// and NativeFunction (e.g. default sys.stdout) cases.
    pub(super) fn write_to_file_object(
        &mut self,
        target: &PyObjectRef,
        text: &str,
    ) -> PyResult<()> {
        if let Some(write_fn) = target.get_attr("write") {
            let text_obj = PyObject::str_val(CompactString::from(text));
            match &write_fn.payload {
                // Bound methods already include self in dispatch
                PyObjectPayload::BoundMethod { .. } | PyObjectPayload::BuiltinBoundMethod(_) => {
                    self.call_object(write_fn, vec![text_obj])?;
                }
                // NativeClosure (e.g. StringIO.write): instance method stored on instance dict
                PyObjectPayload::NativeClosure(_) => {
                    self.call_object(write_fn, vec![text_obj])?;
                }
                // Raw NativeFunction (e.g. default stdio): prepend self
                _ => {
                    self.call_object(write_fn, vec![target.clone(), text_obj])?;
                }
            }
            Ok(())
        } else {
            Err(PyException::attribute_error(format!(
                "'{}' object has no attribute 'write'",
                target.type_name()
            )))
        }
    }

    /// Resolve the output target for print(): file= kwarg > sys.stdout > native stdout.
    pub(super) fn resolve_print_target(
        &self,
        explicit_file: Option<PyObjectRef>,
    ) -> Option<PyObjectRef> {
        explicit_file
            .or_else(|| ferrython_stdlib::get_stdout_override())
            .or_else(|| self.modules.get("sys").and_then(|s| s.get_attr("stdout")))
    }

    pub(super) fn print_text_arg(
        &mut self,
        value: Option<PyObjectRef>,
        default: &str,
        name: &str,
    ) -> PyResult<String> {
        match value {
            None => Ok(default.to_string()),
            Some(value) if matches!(&value.payload, PyObjectPayload::None) => {
                Ok(default.to_string())
            }
            Some(value) => {
                if let PyObjectPayload::Str(s) = &value.payload {
                    Ok(s.to_string())
                } else {
                    Err(PyException::type_error(format!(
                        "{} must be None or a string, not {}",
                        name,
                        value.type_name()
                    )))
                }
            }
        }
    }

    pub(super) fn vm_print(
        &mut self,
        args: &[PyObjectRef],
        sep: Option<PyObjectRef>,
        end: Option<PyObjectRef>,
        file: Option<PyObjectRef>,
        flush: bool,
    ) -> PyResult<PyObjectRef> {
        let sep = self.print_text_arg(sep, " ", "sep")?;
        let end = self.print_text_arg(end, "\n", "end")?;
        let file = file.filter(|f| !matches!(&f.payload, PyObjectPayload::None));
        let mut parts = Vec::with_capacity(args.len());
        for arg in args {
            parts.push(self.vm_str(arg)?);
        }
        let output = format!("{}{}", parts.join(&sep), end);
        if let Some(target) = self.resolve_print_target(file) {
            self.write_to_file_object(&target, &output)?;
            if flush {
                let flush_fn = target.get_attr("flush").ok_or_else(|| {
                    PyException::attribute_error(format!(
                        "'{}' object has no attribute 'flush'",
                        target.type_name()
                    ))
                })?;
                self.call_object(flush_fn, vec![])?;
            }
        } else {
            print!("{}", output);
            if flush {
                use std::io::Write;
                std::io::stdout()
                    .flush()
                    .map_err(|e| PyException::runtime_error(e.to_string()))?;
            }
        }
        Ok(PyObject::none())
    }
}
