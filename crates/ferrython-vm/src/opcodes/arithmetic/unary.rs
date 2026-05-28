use crate::VirtualMachine;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    lookup_in_class_mro, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;

// ── Group 4: Unary operations ────────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn unwrap_weak_proxy_for_arithmetic(
        &mut self,
        obj: &PyObjectRef,
    ) -> Result<Option<PyObjectRef>, PyException> {
        let PyObjectPayload::Instance(inst) = &obj.payload else {
            return Ok(None);
        };
        let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() else {
            return Ok(None);
        };
        Ok(Some(self.call_object(target_fn, vec![])?))
    }

    pub(crate) fn exec_unary_ops(
        &mut self,
        instr: Instruction,
    ) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::UnaryPositive => {
                let v = self.vm_pop();
                // bool: +True/+False must return int, not bool (preserve identity test)
                if let PyObjectPayload::Bool(b) = &v.payload {
                    self.vm_push(PyObject::int(if *b { 1 } else { 0 }));
                } else if let Some(r) = self.try_call_dunder(&v, "__pos__", vec![])? {
                    self.vm_push(r);
                } else {
                    self.vm_push(v.positive()?);
                }
            }
            Opcode::UnaryNegative => {
                let v = self.vm_pop();
                // Inline fast path for int/float
                let fast = match &v.payload {
                    PyObjectPayload::Int(PyInt::Small(n)) => Some(match n.checked_neg() {
                        Some(r) => PyObject::int(r),
                        None => {
                            use num_bigint::BigInt;
                            PyObject::big_int(-BigInt::from(*n))
                        }
                    }),
                    PyObjectPayload::Float(f) => Some(PyObject::float(-f)),
                    _ => None,
                };
                if let Some(r) = fast {
                    self.vm_push(r);
                } else if let Some(r) = self.try_call_dunder(&v, "__neg__", vec![])? {
                    self.vm_push(r);
                } else {
                    self.vm_push(v.negate()?);
                }
            }
            Opcode::UnaryNot => {
                let v = self.vm_pop();
                // Inline fast path for bool/int/None
                let fast = match &v.payload {
                    PyObjectPayload::Bool(b) => Some(!b),
                    PyObjectPayload::Int(PyInt::Small(n)) => Some(*n == 0),
                    PyObjectPayload::None => Some(true),
                    _ => None,
                };
                if let Some(r) = fast {
                    self.vm_push(PyObject::bool_val(r));
                } else {
                    let truthy = self.vm_is_truthy(&v)?;
                    self.vm_push(PyObject::bool_val(!truthy));
                }
            }
            Opcode::UnaryInvert => {
                let v = self.vm_pop();
                // Try class MRO lookup + bind (like try_binary_dunder does)
                let resolved = if let PyObjectPayload::Instance(inst) = &v.payload {
                    if let Some(method) = lookup_in_class_mro(&inst.class, "__invert__") {
                        let bound = self.bind_method(&v, method);
                        Some(self.call_object(bound, vec![])?)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(r) = resolved.or_else(|| {
                    self.try_call_dunder(&v, "__invert__", vec![])
                        .ok()
                        .flatten()
                }) {
                    self.vm_push(r);
                } else {
                    self.vm_push(v.invert()?);
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}
