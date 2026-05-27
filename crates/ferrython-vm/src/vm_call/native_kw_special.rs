use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{NativeFunctionData, PyObject, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_special_native_kw(
        &mut self,
        nf_data: &NativeFunctionData,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<Option<PyObjectRef>> {
        if nf_data.name.as_str() == "_ast.AST.__init__" {
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
            Self::populate_ast_node_attrs(instance, &cls, &pos_args[1..], kwargs)?;
            return Ok(Some(PyObject::none()));
        }

        if nf_data.name.as_str() == "_ast.AST.__new__" {
            if pos_args.is_empty() {
                return Err(PyException::type_error("__new__ requires cls"));
            }
            let cls = pos_args[0].clone();
            let args = pos_args[1..].to_vec();
            let instance = self
                .try_instantiate_ast_node(&cls, args, kwargs.to_vec())?
                .unwrap_or_else(|| PyObject::instance(cls));
            return Ok(Some(instance));
        }

        if nf_data.name.as_str() == "property.__init__" {
            if pos_args.is_empty() {
                return Ok(Some(PyObject::none()));
            }
            Self::init_property_instance_attrs(&pos_args[0], &pos_args[1..], kwargs)?;
            return Ok(Some(PyObject::none()));
        }

        if nf_data.name.as_str() == "__type_call__" {
            if pos_args.is_empty() {
                return Err(PyException::type_error("type.__call__ requires cls"));
            }
            let cls = pos_args[0].clone();
            let rest = pos_args[1..].to_vec();
            return self
                .instantiate_class(&cls, rest, kwargs.to_vec())
                .map(Some);
        }

        Ok(None)
    }
}
