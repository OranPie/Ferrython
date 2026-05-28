use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use super::class_builtin_defaults::default_builtin_subclass_value;
use super::class_builtin_numeric::{complex_builtin_value, float_builtin_value, int_builtin_value};
use super::class_builtin_sets::{frozenset_builtin_value, set_builtin_value};
use crate::VirtualMachine;

pub(super) enum BuiltinSubclassStrMode {
    VmAware,
    Plain,
}

pub(super) enum BuiltinSubclassValue {
    Store(Option<PyObjectRef>),
    Return(PyObjectRef),
}

impl VirtualMachine {
    pub(super) fn build_builtin_subclass_value(
        &mut self,
        base_type: &str,
        pos_args: &[PyObjectRef],
        str_mode: BuiltinSubclassStrMode,
    ) -> PyResult<BuiltinSubclassValue> {
        if pos_args.is_empty() {
            return Ok(BuiltinSubclassValue::Store(default_builtin_subclass_value(
                base_type,
            )));
        }

        let value = match base_type {
            "int" => int_builtin_value(&pos_args[0]),
            "float" => float_builtin_value(&pos_args[0]),
            "str" => match str_mode {
                BuiltinSubclassStrMode::VmAware => {
                    if pos_args.len() >= 2 {
                        match &pos_args[0].payload {
                            PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
                                let s = String::from_utf8_lossy(bytes);
                                return Ok(BuiltinSubclassValue::Return(PyObject::str_val(
                                    CompactString::from(s.as_ref()),
                                )));
                            }
                            _ => {}
                        }
                    }
                    let value = match self.vm_str(&pos_args[0]) {
                        Ok(s) => PyObject::str_val(CompactString::from(s)),
                        Err(_) => {
                            PyObject::str_val(CompactString::from(pos_args[0].py_to_string()))
                        }
                    };
                    Some(value)
                }
                BuiltinSubclassStrMode::Plain => Some(PyObject::str_val(CompactString::from(
                    pos_args[0].py_to_string(),
                ))),
            },
            "complex" => complex_builtin_value(pos_args),
            "list" => Some(PyObject::list(
                self.collect_iterable(&pos_args[0]).unwrap_or_default(),
            )),
            "tuple" => {
                if pos_args.len() > 1 {
                    Some(PyObject::tuple(pos_args.to_vec()))
                } else {
                    Some(PyObject::tuple(
                        self.collect_iterable(&pos_args[0]).unwrap_or_default(),
                    ))
                }
            }
            "set" => Some(set_builtin_value(self, &pos_args[0])),
            "frozenset" => Some(frozenset_builtin_value(self, &pos_args[0])),
            "bytes" | "bytearray" => Some(pos_args[0].clone()),
            "deque" => Some(PyObject::list(
                self.collect_iterable(&pos_args[0]).unwrap_or_default(),
            )),
            _ => None,
        };

        Ok(BuiltinSubclassValue::Store(value))
    }
}
