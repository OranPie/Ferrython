use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use std::rc::Rc;

use crate::VirtualMachine;

impl VirtualMachine {
    /// Build a super() proxy from current call frame or explicit args.
    pub(crate) fn make_super(&self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            let frame = self.call_stack.last().unwrap();
            // First check locals[0] for self; if moved to a cell (e.g. captured by
            // a comprehension), fall back to cellvars to find it (PEP 3135 compat).
            let self_obj = frame.locals.first().cloned().flatten().or_else(|| {
                // If self is in cellvars (common when method body has comprehensions
                // that reference self), look it up from cells
                for (i, cv) in frame.code.cellvars.iter().enumerate() {
                    if cv.as_str() == "self" || cv.as_str() == "cls" {
                        if let Some(cell) = frame.cells.get(i) {
                            if let Some(val) = cell.read().as_ref() {
                                return Some(val.clone());
                            }
                        }
                    }
                }
                None
            });
            if let Some(self_obj) = self_obj {
                let qualname = frame.code.qualname.as_str();
                let defining_class_name = qualname.rsplit_once('.').map(|(cls_part, _)| {
                    cls_part
                        .rsplit_once('.')
                        .map(|(_, c)| c)
                        .unwrap_or(cls_part)
                });

                let (runtime_cls, instance_for_super) = match &self_obj.payload {
                    PyObjectPayload::Instance(inst) => (inst.class.clone(), self_obj.clone()),
                    PyObjectPayload::Class(cd) => {
                        // For metaclass methods: if defining_class_name matches the metaclass,
                        // use the metaclass as runtime_cls (so super walks metaclass MRO)
                        if let Some(meta) = &cd.metaclass {
                            (meta.clone(), self_obj.clone())
                        } else {
                            (self_obj.clone(), self_obj.clone())
                        }
                    }
                    // Unwrap Super proxy — can happen if property getter receives
                    // a super proxy as self (shouldn't normally, but be defensive)
                    PyObjectPayload::Super { instance, .. } => match &instance.payload {
                        PyObjectPayload::Instance(inst) => (inst.class.clone(), instance.clone()),
                        _ => (instance.clone(), instance.clone()),
                    },
                    _ => return Err(PyException::runtime_error("super(): no current class")),
                };

                let mut cls = runtime_cls.clone();
                if let Some(def_name) = defining_class_name {
                    if let PyObjectPayload::Class(cd) = &runtime_cls.payload {
                        // Build full MRO including the runtime class itself
                        let mut full_mro = vec![runtime_cls.clone()];
                        full_mro.extend(cd.mro.iter().cloned());

                        // Strategy: find the class whose namespace contains the
                        // currently executing function (by matching Rc<CodeObject>
                        // pointers).  This is robust even when multiple classes
                        // share the same name (e.g. Flask Request vs werkzeug
                        // Request, or same-named EnvironBuilder subclasses).
                        let code_ptr = Rc::as_ptr(&frame.code);
                        let mut found_by_code = false;
                        for m in &full_mro {
                            if let PyObjectPayload::Class(mc) = &m.payload {
                                let ns = mc.namespace.read();
                                // Check method name from qualname (last segment)
                                let method_name = qualname
                                    .rsplit_once('.')
                                    .map(|(_, m)| m)
                                    .unwrap_or(qualname);
                                if let Some(val) = ns.get(method_name) {
                                    let matches = match &val.payload {
                                        PyObjectPayload::Function(f) => {
                                            Rc::as_ptr(&f.code) == code_ptr
                                        }
                                        PyObjectPayload::BoundMethod { method, .. } => {
                                            if let PyObjectPayload::Function(f) = &method.payload {
                                                Rc::as_ptr(&f.code) == code_ptr
                                            } else {
                                                false
                                            }
                                        }
                                        _ => false,
                                    };
                                    if matches {
                                        cls = m.clone();
                                        found_by_code = true;
                                        break;
                                    }
                                }
                            }
                        }

                        // Fallback: match by class name if code-pointer match failed
                        // (can happen with NativeFunction or wrapped methods)
                        if !found_by_code {
                            for m in &full_mro {
                                if let PyObjectPayload::Class(mc) = &m.payload {
                                    if mc.name.as_str() == def_name {
                                        cls = m.clone();
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                return Ok(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::Super {
                        cls,
                        instance: instance_for_super,
                    },
                }));
            }
            Err(PyException::runtime_error("super(): no current class"))
        } else if args.len() == 2 {
            Ok(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::Super {
                    cls: args[0].clone(),
                    instance: args[1].clone(),
                },
            }))
        } else {
            Err(PyException::type_error("super() takes 0 or 2 arguments"))
        }
    }
}
