use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_predicate_builtin(
        &mut self,
        name: &str,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name {
            "any" => {
                if !args.is_empty() {
                    let iter_obj = builtins::get_iter_from_obj_pub(&args[0])?;
                    loop {
                        match self.vm_iter_next(&iter_obj)? {
                            Some(item) => {
                                if item.is_truthy() {
                                    return Ok(PyObject::bool_val(true));
                                }
                            }
                            None => return Ok(PyObject::bool_val(false)),
                        }
                    }
                }
            }
            "all" => {
                if !args.is_empty() {
                    let iter_obj = builtins::get_iter_from_obj_pub(&args[0])?;
                    loop {
                        match self.vm_iter_next(&iter_obj)? {
                            Some(item) => {
                                if !item.is_truthy() {
                                    return Ok(PyObject::bool_val(false));
                                }
                            }
                            None => return Ok(PyObject::bool_val(true)),
                        }
                    }
                }
            }
            "isinstance" => {
                if args.len() == 2 {
                    let cls = &args[1];
                    if let PyObjectPayload::Class(cd) = &cls.payload {
                        if let Some(ref metaclass) = cd.metaclass {
                            if let Some(ic) = metaclass.get_attr("__instancecheck__") {
                                let result =
                                    self.call_object(ic, vec![cls.clone(), args[0].clone()])?;
                                return Ok(PyObject::bool_val(result.is_truthy()));
                            }
                        }
                        if let Some(hook) = cls.get_attr("__subclasshook__") {
                            let obj = &args[0];
                            let obj_type = match &obj.payload {
                                PyObjectPayload::Instance(inst) => inst.class.clone(),
                                _ => PyObject::builtin_type(CompactString::from(obj.type_name())),
                            };
                            if let Ok(result) = self.call_object(hook, vec![obj_type]) {
                                if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                    return Ok(PyObject::bool_val(result.is_truthy()));
                                }
                            }
                        }
                        let ns = cd.namespace.read();
                        if ns
                            .get("_is_runtime_checkable")
                            .map_or(false, |v| v.is_truthy())
                        {
                            if let Some(protocol_attrs) = ns.get("__protocol_attrs__") {
                                if let PyObjectPayload::Tuple(required) = &protocol_attrs.payload {
                                    let obj = &args[0];
                                    let has_all = required.iter().all(|attr_name| {
                                        let name = attr_name.py_to_string();
                                        obj.get_attr(&name).is_some()
                                    });
                                    return Ok(PyObject::bool_val(has_all));
                                }
                            }
                        }
                    }
                }
            }
            "issubclass" => {
                if args.len() == 2 {
                    let sup = &args[1];
                    if let PyObjectPayload::Class(cd) = &sup.payload {
                        if let Some(ref metaclass) = cd.metaclass {
                            if let Some(sc) = metaclass.get_attr("__subclasscheck__") {
                                let result =
                                    self.call_object(sc, vec![sup.clone(), args[0].clone()])?;
                                return Ok(PyObject::bool_val(result.is_truthy()));
                            }
                        }
                        if let Some(hook) = sup.get_attr("__subclasshook__") {
                            if let Ok(result) = self.call_object(hook, vec![args[0].clone()]) {
                                if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                    return Ok(PyObject::bool_val(result.is_truthy()));
                                }
                            }
                        }
                    }
                }
            }
            _ => unreachable!("non-predicate builtin routed to predicate dispatch"),
        }
        match builtins::get_builtin_fn(name) {
            Some(f) => f(&args),
            None => unreachable!("predicate builtin missing fallback"),
        }
    }
}
