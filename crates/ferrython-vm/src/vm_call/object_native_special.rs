use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    NativeFunctionData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_ast_or_type_native_object(
        &mut self,
        nf_data: &NativeFunctionData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if nf_data.name.as_str() == "_ast.AST.__init__" {
            if args.is_empty() {
                return Err(PyException::type_error("__init__ requires self"));
            }
            let (pos_args, kwargs) = Self::split_trailing_kwargs_dict(args);
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
            return Ok(Some(PyObject::none()));
        }
        if nf_data.name.as_str() == "_ast.AST.__new__" {
            if args.is_empty() {
                return Err(PyException::type_error("__new__ requires cls"));
            }
            let (pos_args, kwargs) = Self::split_trailing_kwargs_dict(args);
            if pos_args.is_empty() {
                return Err(PyException::type_error("__new__ requires cls"));
            }
            let cls = pos_args[0].clone();
            let pos_args = pos_args[1..].to_vec();
            return Ok(Some(
                self.try_instantiate_ast_node(&cls, pos_args, kwargs)?
                    .unwrap_or_else(|| PyObject::instance(cls)),
            ));
        }
        if nf_data.name.as_str() == "__type_call__" {
            if args.is_empty() {
                return Err(PyException::type_error("type.__call__ requires cls"));
            }
            let cls = args[0].clone();
            let rest = args[1..].to_vec();
            return self.instantiate_class(&cls, rest, vec![]).map(Some);
        }
        Ok(None)
    }

    pub(super) fn call_property_get_native(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<PyObjectRef> {
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
            if ferrython_core::object::is_dynamic_class_attribute(prop) {
                let is_abstract = self.property_isabstractmethod(prop)?;
                if is_abstract.is_truthy() {
                    return Ok(prop.clone());
                }
                return Err(PyException::attribute_error(""));
            }
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
}
