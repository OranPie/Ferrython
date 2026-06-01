use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    fn class_is_subclass(child: &PyObjectRef, parent: &PyObjectRef) -> bool {
        if PyObjectRef::ptr_eq(child, parent) {
            return true;
        }
        match &child.payload {
            PyObjectPayload::Class(cd) => {
                cd.mro
                    .iter()
                    .any(|base| Self::class_is_subclass(base, parent))
                    || cd
                        .bases
                        .iter()
                        .any(|base| Self::class_is_subclass(base, parent))
            }
            PyObjectPayload::BuiltinType(child_name) => match &parent.payload {
                PyObjectPayload::BuiltinType(parent_name) => {
                    child_name.as_str() == parent_name.as_str()
                        || parent_name.as_str() == "object"
                        || (child_name.as_str() == "bool" && parent_name.as_str() == "int")
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn zero_arg_super_class(frame: &crate::frame::Frame) -> PyResult<PyObjectRef> {
        let n_cell = frame.code.cellvars.len();
        for (i, name) in frame.code.cellvars.iter().enumerate() {
            if name.as_str() == "__class__" {
                return Self::class_from_super_cell(frame.cells.get(i));
            }
        }
        for (i, name) in frame.code.freevars.iter().enumerate() {
            if name.as_str() == "__class__" {
                return Self::class_from_super_cell(frame.cells.get(n_cell + i));
            }
        }
        Err(PyException::runtime_error("super(): no current class"))
    }

    fn class_from_super_cell(cell: Option<&crate::frame::CellRef>) -> PyResult<PyObjectRef> {
        let Some(cell) = cell else {
            return Err(PyException::runtime_error("super(): no current class"));
        };
        match cell.read().as_ref() {
            Some(cls) if matches!(&cls.payload, PyObjectPayload::Class(_)) => Ok(cls.clone()),
            Some(_) => Err(PyException::runtime_error(
                "super(): __class__ is not a type",
            )),
            None => Err(PyException::runtime_error("super(): empty __class__ cell")),
        }
    }

    fn zero_arg_super_instance(cls: &PyObjectRef, self_obj: PyObjectRef) -> PyResult<PyObjectRef> {
        match &self_obj.payload {
            PyObjectPayload::Instance(inst) if Self::class_is_subclass(&inst.class, cls) => {
                Ok(self_obj.clone())
            }
            PyObjectPayload::Class(cd)
                if Self::class_is_subclass(&self_obj, cls)
                    || cd
                        .metaclass
                        .as_ref()
                        .map(|meta| Self::class_is_subclass(meta, cls))
                        .unwrap_or(false) =>
            {
                Ok(self_obj.clone())
            }
            PyObjectPayload::Super { instance, .. } => Ok(instance.clone()),
            _ => Err(PyException::type_error(
                "super(type, obj): obj must be an instance or subtype of type",
            )),
        }
    }

    /// Build a super() proxy from current call frame or explicit args.
    pub(crate) fn make_super(&self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            let frame = self.call_stack.last().unwrap();
            let cls = Self::zero_arg_super_class(frame)?;
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
                let instance_for_super = Self::zero_arg_super_instance(&cls, self_obj)?;
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
