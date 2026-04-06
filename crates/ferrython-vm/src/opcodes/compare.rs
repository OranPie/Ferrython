//! Comparison operations: ==, !=, <, <=, >, >=, is, is not, in, not in

use crate::vm::exception_kind_matches;
use crate::VirtualMachine;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{
    lookup_in_class_mro, CompareOp, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

impl VirtualMachine {
    /// Derive a missing comparison from total_ordering root method
    fn derive_total_ordering(
        &mut self, a: &PyObjectRef, b: &PyObjectRef, dunder: &str, root: &str
    ) -> Result<Option<PyObjectRef>, PyException> {
        // Helper: call a's dunder method via the VM
        let call_dunder = |vm: &mut Self, obj: &PyObjectRef, other: &PyObjectRef, method: &str|
            -> Result<Option<bool>, PyException>
        {
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let Some(m) = lookup_in_class_mro(&inst.class, method) {
                    let bound = vm.bind_method(obj, m);
                    let r = vm.call_object(bound, vec![other.clone()])?;
                    if !matches!(&r.payload, PyObjectPayload::NotImplemented) {
                        return Ok(Some(r.is_truthy()));
                    }
                }
            }
            Ok(None)
        };

        // Don't derive if we have the exact root (that should have been found already)
        if dunder == root { return Ok(None); }

        match (root, dunder) {
            ("__lt__", "__le__") => {
                // a <= b  =  a < b or a == b
                if let Some(lt) = call_dunder(self, a, b, "__lt__")? {
                    if lt { return Ok(Some(PyObject::bool_val(true))); }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(eq)));
                    }
                    return Ok(Some(PyObject::bool_val(false)));
                }
            }
            ("__lt__", "__gt__") => {
                // a > b  =  not (a < b) and not (a == b)
                if let Some(lt) = call_dunder(self, a, b, "__lt__")? {
                    if lt { return Ok(Some(PyObject::bool_val(false))); }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(!eq)));
                    }
                    return Ok(Some(PyObject::bool_val(true)));
                }
            }
            ("__lt__", "__ge__") => {
                // a >= b  =  not (a < b)
                if let Some(lt) = call_dunder(self, a, b, "__lt__")? {
                    return Ok(Some(PyObject::bool_val(!lt)));
                }
            }
            ("__gt__", "__ge__") => {
                if let Some(gt) = call_dunder(self, a, b, "__gt__")? {
                    if gt { return Ok(Some(PyObject::bool_val(true))); }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(eq)));
                    }
                    return Ok(Some(PyObject::bool_val(false)));
                }
            }
            ("__gt__", "__lt__") => {
                if let Some(gt) = call_dunder(self, a, b, "__gt__")? {
                    if gt { return Ok(Some(PyObject::bool_val(false))); }
                    if let Some(eq) = call_dunder(self, a, b, "__eq__")? {
                        return Ok(Some(PyObject::bool_val(!eq)));
                    }
                    return Ok(Some(PyObject::bool_val(true)));
                }
            }
            ("__gt__", "__le__") => {
                if let Some(gt) = call_dunder(self, a, b, "__gt__")? {
                    return Ok(Some(PyObject::bool_val(!gt)));
                }
            }
            _ => {}
        }
        Ok(None)
    }

    pub(crate) fn exec_compare_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let (a, b) = self.vm_pop2();
        self.exec_compare_op(instr.arg, a, b)
    }

    fn exec_compare_op(&mut self, op: u32, a: PyObjectRef, b: PyObjectRef) -> Result<Option<PyObjectRef>, PyException> {
        if let cmp @ 0..=5 = op {
            let (dunder, rdunder) = match cmp {
                0 => ("__lt__", "__gt__"),
                1 => ("__le__", "__ge__"),
                2 => ("__eq__", "__eq__"),
                3 => ("__ne__", "__ne__"),
                4 => ("__gt__", "__lt__"),
                5 => ("__ge__", "__le__"),
                _ => unreachable!()
            };
            // Try a's dunder via MRO walk
            if let PyObjectPayload::Instance(inst) = &a.payload {
                if let Some(method) = lookup_in_class_mro(&inst.class, dunder) {
                    let bound = self.bind_method(&a, method);
                    let r = self.call_object(bound, vec![b.clone()])?;
                    if !matches!(&r.payload, PyObjectPayload::NotImplemented) {
                        self.vm_push(r);
                        return Ok(None);
                    }
                }
                // total_ordering fallback: derive missing comparisons from root
                if let Some(root_marker) = lookup_in_class_mro(&inst.class, "__total_ordering_root__") {
                    let root = root_marker.py_to_string();
                    if let Some(result) = self.derive_total_ordering(&a, &b, dunder, &root)? {
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
            }
            // Try b's reflected dunder via MRO walk
            if let PyObjectPayload::Instance(inst) = &b.payload {
                if let Some(method) = lookup_in_class_mro(&inst.class, rdunder) {
                    let bound = self.bind_method(&b, method);
                    let r = self.call_object(bound, vec![a.clone()])?;
                    if !matches!(&r.payload, PyObjectPayload::NotImplemented) {
                        self.vm_push(r);
                        return Ok(None);
                    }
                }
            }
            // Dataclass auto-equality fallback
            if cmp == 2 || cmp == 3 {
                if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) = (&a.payload, &b.payload) {
                    let cls_a = &inst_a.class;
                    if cls_a.get_attr("__dataclass__").is_some() {
                        if let Some(fields) = cls_a.get_attr("__dataclass_fields__") {
                            if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                                let attrs_a = inst_a.attrs.read();
                                let attrs_b = inst_b.attrs.read();
                                let mut eq = true;
                                for ft in field_tuples {
                                    if let PyObjectPayload::Tuple(info) = &ft.payload {
                                        let name = info[0].py_to_string();
                                        let va = attrs_a.get(name.as_str());
                                        let vb = attrs_b.get(name.as_str());
                                        match (va, vb) {
                                            (Some(x), Some(y)) => {
                                                if let Ok(r) = x.compare(y, CompareOp::Eq) {
                                                    if !r.is_truthy() { eq = false; break; }
                                                } else { eq = false; break; }
                                            }
                                            _ => { eq = false; break; }
                                        }
                                    }
                                }
                                let result = if cmp == 2 { eq } else { !eq };
                                self.vm_push(PyObject::bool_val(result));
                                return Ok(None);
                            }
                        }
                    }
                }
            }
            // namedtuple equality: compare underlying _tuple
            if cmp == 2 || cmp == 3 {
                if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) = (&a.payload, &b.payload) {
                    if inst_a.class.get_attr("__namedtuple__").is_some() && inst_b.class.get_attr("__namedtuple__").is_some() {
                        let ta = inst_a.attrs.read().get("_tuple").cloned();
                        let tb = inst_b.attrs.read().get("_tuple").cloned();
                        if let (Some(tup_a), Some(tup_b)) = (ta, tb) {
                            let result = tup_a.compare(&tup_b, CompareOp::Eq)?;
                            let val = if cmp == 2 { result.is_truthy() } else { !result.is_truthy() };
                            self.vm_push(PyObject::bool_val(val));
                            return Ok(None);
                        }
                    }
                }
                // namedtuple == plain tuple: compare underlying _tuple with tuple
                if let PyObjectPayload::Instance(inst) = &a.payload {
                    if inst.class.get_attr("__namedtuple__").is_some() {
                        if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                            if matches!(b.payload, PyObjectPayload::Tuple(_)) {
                                let result = tup.compare(&b, CompareOp::Eq)?;
                                let val = if cmp == 2 { result.is_truthy() } else { !result.is_truthy() };
                                self.vm_push(PyObject::bool_val(val));
                                return Ok(None);
                            }
                        }
                    }
                }
                if let PyObjectPayload::Instance(inst) = &b.payload {
                    if inst.class.get_attr("__namedtuple__").is_some() {
                        if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                            if matches!(a.payload, PyObjectPayload::Tuple(_)) {
                                let result = a.compare(&tup, CompareOp::Eq)?;
                                let val = if cmp == 2 { result.is_truthy() } else { !result.is_truthy() };
                                self.vm_push(PyObject::bool_val(val));
                                return Ok(None);
                            }
                        }
                    }
                }
            }
            // IntEnum/enum value-based comparison fallback
            if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) = (&a.payload, &b.payload) {
                let va = inst_a.attrs.read().get("value").cloned();
                let vb = inst_b.attrs.read().get("value").cloned();
                if let (Some(av), Some(bv)) = (va, vb) {
                    if matches!(av.payload, PyObjectPayload::Int(_) | PyObjectPayload::Float(_))
                        && matches!(bv.payload, PyObjectPayload::Int(_) | PyObjectPayload::Float(_))
                    {
                        let cmp_op = match cmp {
                            0 => CompareOp::Lt,
                            1 => CompareOp::Le,
                            2 => CompareOp::Eq,
                            3 => CompareOp::Ne,
                            4 => CompareOp::Gt,
                            5 => CompareOp::Ge,
                            _ => unreachable!()
                        };
                        let result = av.compare(&bv, cmp_op)?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
            }
            // IntEnum vs plain int/float comparison
            {
                let (enum_val, other) = if let PyObjectPayload::Instance(inst) = &a.payload {
                    (inst.attrs.read().get("value").cloned(), Some(&b))
                } else if let PyObjectPayload::Instance(inst) = &b.payload {
                    (inst.attrs.read().get("value").cloned(), Some(&a))
                } else {
                    (None, None)
                };
                if let (Some(ev), Some(ov)) = (enum_val, other) {
                    if matches!(ev.payload, PyObjectPayload::Int(_) | PyObjectPayload::Float(_))
                        && matches!(ov.payload, PyObjectPayload::Int(_) | PyObjectPayload::Float(_))
                    {
                        let (left, right) = if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                            (ev, ov.clone())
                        } else {
                            (ov.clone(), ev)
                        };
                        let cmp_op = match cmp {
                            0 => CompareOp::Lt,
                            1 => CompareOp::Le,
                            2 => CompareOp::Eq,
                            3 => CompareOp::Ne,
                            4 => CompareOp::Gt,
                            5 => CompareOp::Ge,
                            _ => unreachable!()
                        };
                        let result = left.compare(&right, cmp_op)?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                }
            }
        }
        // 'in' / 'not in' with __contains__
        if op == 6 || op == 7 {
            // Handle Class with __contains__ (e.g., Enum: Color.RED in Color)
            if let PyObjectPayload::Class(cd) = &b.payload {
                // Look in own namespace and MRO
                let contains_fn = {
                    let ns = cd.namespace.read();
                    let mut found = ns.get("__contains__").cloned();
                    if found.is_none() {
                        for base in &cd.mro {
                            if let PyObjectPayload::Class(bcd) = &base.payload {
                                let bns = bcd.namespace.read();
                                if let Some(f) = bns.get("__contains__") {
                                    found = Some(f.clone());
                                    break;
                                }
                            }
                        }
                    }
                    found
                };
                if let Some(method) = contains_fn {
                    let r = self.call_object(method, vec![b.clone(), a.clone()])?;
                    let val = if op == 6 { r.is_truthy() } else { !r.is_truthy() };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
            }
            if let PyObjectPayload::Instance(inst) = &b.payload {
                // Check for user-defined __contains__ in the class (including dict subclasses)
                let custom_contains = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    cd.namespace.read().get("__contains__").cloned()
                } else { None };
                if let Some(method) = custom_contains {
                    let r = self.call_object(method, vec![b.clone(), a.clone()])?;
                    let val = if op == 6 { r.is_truthy() } else { !r.is_truthy() };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                // Dict subclass: use native contains() directly
                if inst.dict_storage.is_some() {
                    let val = if op == 6 { b.contains(&a)? } else { !b.contains(&a)? };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                if let Some(method) = b.get_attr("__contains__") {
                    let r = self.call_object(method, vec![a])?;
                    let val = if op == 6 { r.is_truthy() } else { !r.is_truthy() };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                // Fallback: iterate via __iter__ (CPython behavior)
                if let Some(iter_method) = b.get_attr("__iter__") {
                    let iterator = self.call_object(iter_method, vec![])?;
                    let mut found = false;
                    loop {
                        match crate::builtins::iter_advance(&iterator)? {
                            Some((_iter, item)) => {
                                if item.compare(&a, CompareOp::Eq)?.is_truthy() {
                                    found = true;
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                    let val = if op == 6 { found } else { !found };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
                // Fallback: iterate via __getitem__ with integer indices (CPython behavior)
                if b.get_attr("__getitem__").is_some() {
                    let mut found = false;
                    let mut idx = 0i64;
                    loop {
                        let getitem = b.get_attr("__getitem__").unwrap();
                        match self.call_object(getitem, vec![PyObject::int(idx)]) {
                            Ok(item) => {
                                if item.compare(&a, CompareOp::Eq)?.is_truthy() {
                                    found = true;
                                    break;
                                }
                                idx += 1;
                            }
                            Err(e) if e.kind == ferrython_core::error::ExceptionKind::IndexError => break,
                            Err(e) => return Err(e),
                        }
                    }
                    let val = if op == 6 { found } else { !found };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
            }
            // Module with __contains__ (e.g., os.environ)
            if let PyObjectPayload::Module(ref md) = &b.payload {
                let contains_fn = md.attrs.read().get("__contains__").cloned();
                if let Some(method) = contains_fn {
                    let r = self.call_object(method, vec![b.clone(), a.clone()])?;
                    let val = if op == 6 { r.is_truthy() } else { !r.is_truthy() };
                    self.vm_push(PyObject::bool_val(val));
                    return Ok(None);
                }
            }
        }
        let result = match op {
            0 => a.compare(&b, CompareOp::Lt)?,
            1 => a.compare(&b, CompareOp::Le)?,
            2 => a.compare(&b, CompareOp::Eq)?,
            3 => a.compare(&b, CompareOp::Ne)?,
            4 => a.compare(&b, CompareOp::Gt)?,
            5 => a.compare(&b, CompareOp::Ge)?,
            6 => PyObject::bool_val(b.contains(&a)?),
            7 => PyObject::bool_val(!b.contains(&a)?),
            8 => PyObject::bool_val(a.is_same(&b)),
            9 => PyObject::bool_val(!a.is_same(&b)),
            10 => {
                let match_one = |a_item: &PyObjectRef, b_item: &PyObjectRef| -> bool {
                    if let PyObjectPayload::Class(cls_a) = &a_item.payload {
                        if let PyObjectPayload::Class(cls_b) = &b_item.payload {
                            if cls_a.name == cls_b.name { return true; }
                            for base in &cls_a.mro {
                                if let PyObjectPayload::Class(bc) = &base.payload {
                                    if bc.name == cls_b.name { return true; }
                                }
                            }
                            for base in &cls_a.bases {
                                if let PyObjectPayload::Class(bc) = &base.payload {
                                    if bc.name == cls_b.name { return true; }
                                }
                            }
                            return false;
                        }
                        if let PyObjectPayload::ExceptionType(kind_b) = &b_item.payload {
                            let kind_a = Self::find_exception_kind(a_item);
                            return exception_kind_matches(&kind_a, kind_b);
                        }
                        return false;
                    }
                    if let PyObjectPayload::ExceptionType(kind_a) = &a_item.payload {
                        return match &b_item.payload {
                            PyObjectPayload::ExceptionType(kind_b) => {
                                exception_kind_matches(kind_a, kind_b)
                            }
                            PyObjectPayload::Class(_cls_b) => {
                                let kind_b = Self::find_exception_kind(b_item);
                                exception_kind_matches(kind_a, &kind_b)
                            }
                            PyObjectPayload::BuiltinType(name) => {
                                if let Some(kind_b) = ExceptionKind::from_name(name) {
                                    exception_kind_matches(kind_a, &kind_b)
                                } else {
                                    false
                                }
                            }
                            _ => false,
                        };
                    }
                    false
                };
                let matched = match &b.payload {
                    PyObjectPayload::Tuple(items) => items.iter().any(|item| match_one(&a, item)),
                    _ => match_one(&a, &b),
                };
                PyObject::bool_val(matched)
            }
            _ => return Err(PyException::runtime_error("invalid compare op")),
        };
        self.vm_push(result);
        Ok(None)
    }
}

