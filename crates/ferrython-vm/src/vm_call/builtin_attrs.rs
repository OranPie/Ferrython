use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    has_descriptor_get, is_data_descriptor, lookup_in_class_mro, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef, CLASS_FLAG_HAS_DESCRIPTORS, CLASS_FLAG_HAS_SETATTR,
    CLASS_FLAG_HAS_SLOTS,
};

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_attr_builtin(
        &mut self,
        name: &str,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name {
            "getattr" => self.call_getattr_builtin(args),
            "setattr" => self.call_setattr_builtin(args),
            "delattr" => self.call_delattr_builtin(args),
            _ => unreachable!("non-attribute builtin routed to attribute dispatch"),
        }
    }

    fn call_getattr_builtin(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.len() < 2 || args.len() > 3 {
            return Err(PyException::type_error("getattr expected 2 or 3 arguments"));
        }
        let attr_name = args[1]
            .as_str()
            .ok_or_else(|| PyException::type_error("getattr(): attribute name must be string"))?;
        if attr_name == "__isabstractmethod__" && ferrython_core::object::is_property_like(&args[0])
        {
            return self.property_isabstractmethod(&args[0]);
        }
        match args[0].get_attr(attr_name) {
            Some(v) => {
                if ferrython_core::object::is_property_like(&v) {
                    if matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                        return Ok(v);
                    }
                    if let Some(getter) = ferrython_core::object::property_field(&v, "fget") {
                        if matches!(&getter.payload, PyObjectPayload::None) {
                            return Err(PyException::attribute_error(format!(
                                "unreadable attribute '{}'",
                                attr_name
                            )));
                        }
                        let getter = crate::builtins::unwrap_abstract_fget(&getter);
                        return self.call_object(getter, vec![args[0].clone()]);
                    }
                    return Err(PyException::attribute_error(format!(
                        "unreadable attribute '{}'",
                        attr_name
                    )));
                }
                if has_descriptor_get(&v) {
                    if let Some(get_method) = v.get_attr("__get__") {
                        let (inst_arg, owner_arg) = match &args[0].payload {
                            PyObjectPayload::Instance(inst) => {
                                (args[0].clone(), inst.class.clone())
                            }
                            PyObjectPayload::Class(_) => (PyObject::none(), args[0].clone()),
                            _ => (args[0].clone(), PyObject::none()),
                        };
                        return self.call_object(get_method, vec![inst_arg, owner_arg]);
                    }
                }
                Ok(v)
            }
            None => {
                if let PyObjectPayload::Instance(_) = &args[0].payload {
                    if let Some(ga) = args[0].get_attr("__getattr__") {
                        let name_arg = PyObject::str_val(CompactString::from(attr_name));
                        return self.call_object(ga, vec![name_arg]);
                    }
                }
                if args.len() > 2 {
                    return Ok(args[2].clone());
                }
                Err(PyException::attribute_error(format!(
                    "'{}' object has no attribute '{}'",
                    args[0].type_name(),
                    attr_name
                )))
            }
        }
    }

    fn call_setattr_builtin(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.len() != 3 {
            return Err(PyException::type_error(
                "setattr() takes exactly 3 arguments",
            ));
        }
        let attr_name = args[1].py_to_string();
        let value = args[2].clone();
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            if inst.class_flags
                & (CLASS_FLAG_HAS_SETATTR | CLASS_FLAG_HAS_DESCRIPTORS | CLASS_FLAG_HAS_SLOTS)
                == 0
            {
                inst.attrs
                    .write()
                    .insert(CompactString::from(attr_name.as_str()), value);
                return Ok(PyObject::none());
            }
            if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                if ferrython_core::object::is_property_like(&desc) {
                    if let Some(setter) = ferrython_core::object::property_field(&desc, "fset") {
                        if matches!(&setter.payload, PyObjectPayload::None) {
                            return Err(PyException::attribute_error(format!(
                                "can't set attribute '{}'",
                                attr_name
                            )));
                        }
                        self.call_object(setter, vec![args[0].clone(), value])?;
                        return Ok(PyObject::none());
                    } else {
                        return Err(PyException::attribute_error(format!(
                            "can't set attribute '{}'",
                            attr_name
                        )));
                    }
                }
                if is_data_descriptor(&desc) {
                    if let Some(set_method) = desc.get_attr("__set__") {
                        self.call_object(set_method, vec![args[0].clone(), value])?;
                        return Ok(PyObject::none());
                    }
                }
            }
            if let Some(sa) = lookup_in_class_mro(&inst.class, "__setattr__") {
                if matches!(&sa.payload, PyObjectPayload::Function(_)) {
                    let method = PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: args[0].clone(),
                            method: sa,
                        },
                    });
                    self.call_object(
                        method,
                        vec![PyObject::str_val(CompactString::from(&attr_name)), value],
                    )?;
                    return Ok(PyObject::none());
                }
            }
        }
        builtins::dispatch("setattr", &args)
    }

    fn call_delattr_builtin(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.len() != 2 {
            return Err(PyException::type_error(
                "delattr() takes exactly 2 arguments",
            ));
        }
        let attr_name = args[1].py_to_string();
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                if ferrython_core::object::is_property_like(&desc) {
                    if let Some(deleter) = ferrython_core::object::property_field(&desc, "fdel") {
                        if matches!(&deleter.payload, PyObjectPayload::None) {
                            return Err(PyException::attribute_error(format!(
                                "can't delete attribute '{}'",
                                attr_name
                            )));
                        }
                        self.call_object(deleter, vec![args[0].clone()])?;
                        return Ok(PyObject::none());
                    }
                    return Err(PyException::attribute_error(format!(
                        "can't delete attribute '{}'",
                        attr_name
                    )));
                }
            }
        }
        builtins::dispatch("delattr", &args)
    }
}
