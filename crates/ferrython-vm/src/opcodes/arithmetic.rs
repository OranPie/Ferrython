use super::unwrap_int_enum;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::PyException;
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    lookup_in_class_mro, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use std::sync::Arc;

// ── Group 4: Unary operations ────────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_unary_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::UnaryPositive => {
                let v = self.vm_pop();
                if let Some(r) = self.try_call_dunder(&v, "__pos__", vec![])? {
                    self.vm_push(r);
                } else {
                    self.vm_push(v.positive()?);
                }
            }
            Opcode::UnaryNegative => {
                let v = self.vm_pop();
                if let Some(r) = self.try_call_dunder(&v, "__neg__", vec![])? {
                    self.vm_push(r);
                } else {
                    self.vm_push(v.negate()?);
                }
            }
            Opcode::UnaryNot => {
                let v = self.vm_pop();
                let truthy = self.vm_is_truthy(&v)?;
                self.vm_push(PyObject::bool_val(!truthy));
            }
            Opcode::UnaryInvert => {
                let v = self.vm_pop();
                if let Some(r) = self.try_call_dunder(&v, "__invert__", vec![])? {
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

// ── Group 5: Binary / inplace arithmetic ─────────────────────────────
impl VirtualMachine {
    pub(crate) fn try_binary_dunder(
        &mut self, a: &PyObjectRef, b: &PyObjectRef,
        dunder: &str, rdunder: Option<&str>,
    ) -> Result<Option<PyObjectRef>, PyException> {
        // Look up dunder via class MRO (not instance get_attr) for proper inheritance
        if let PyObjectPayload::Instance(inst) = &a.payload {
            if let Some(method) = lookup_in_class_mro(&inst.class, dunder) {
                let bound = self.bind_method(a, method);
                let result = self.call_object(bound, vec![b.clone()])?;
                // If method returns NotImplemented, try the reflected dunder
                if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                    return Ok(Some(result));
                }
            }
        }
        if let Some(rd) = rdunder {
            if let PyObjectPayload::Instance(inst) = &b.payload {
                if let Some(method) = lookup_in_class_mro(&inst.class, rd) {
                    let bound = self.bind_method(b, method);
                    let result = self.call_object(bound, vec![a.clone()])?;
                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                        return Ok(Some(result));
                    }
                }
            }
        }
        Ok(None)
    }

    fn try_inplace_dunder(
        &mut self, a: &PyObjectRef, b: &PyObjectRef,
        idunder: &str, dunder: &str,
    ) -> Result<Option<PyObjectRef>, PyException> {
        if let PyObjectPayload::Instance(inst) = &a.payload {
            let method = lookup_in_class_mro(&inst.class, idunder)
                .or_else(|| lookup_in_class_mro(&inst.class, dunder));
            if let Some(m) = method {
                let bound = self.bind_method(a, m);
                return Ok(Some(self.call_object(bound, vec![b.clone()])?));
            }
        }
        Ok(None)
    }

    /// Create a bound method from an instance receiver and an unbound method.
    pub(crate) fn bind_method(&self, receiver: &PyObjectRef, method: PyObjectRef) -> PyObjectRef {
        match &method.payload {
            PyObjectPayload::BoundMethod { .. } => method,
            _ => Arc::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: receiver.clone(),
                    method,
                }
            }),
        }
    }

    pub(crate) fn exec_binary_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let (a, b) = self.vm_pop2();

        // ── Fast paths for primitive types ──
        // Skip dunder dispatch and py_add/sub/mul overhead for the most common cases.
        // Only applies to BinaryAdd/Sub/Mul — the hottest arithmetic opcodes.
        macro_rules! fast_int_op {
            ($a:expr, $b:expr, $checked_op:ident, $big_op:tt) => {
                match (&$a.payload, &$b.payload) {
                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                        let result = match x.$checked_op(*y) {
                            Some(r) => PyObject::int(r),
                            None => {
                                use num_bigint::BigInt;
                                PyObject::big_int(BigInt::from(*x) $big_op BigInt::from(*y))
                            }
                        };
                        self.vm_push(result);
                        return Ok(None);
                    }
                    (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                        self.vm_push(PyObject::float(*x $big_op *y));
                        return Ok(None);
                    }
                    _ => {}
                }
            };
        }

        match instr.op {
            Opcode::BinaryAdd | Opcode::InplaceAdd => {
                fast_int_op!(a, b, checked_add, +);
                // Also fast-path str + str
                if let (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) = (&a.payload, &b.payload) {
                    let mut s = x.clone();
                    s.push_str(y);
                    self.vm_push(PyObject::str_val(s));
                    return Ok(None);
                }
            }
            Opcode::BinarySubtract | Opcode::InplaceSubtract => {
                fast_int_op!(a, b, checked_sub, -);
            }
            Opcode::BinaryMultiply | Opcode::InplaceMultiply => {
                fast_int_op!(a, b, checked_mul, *);
            }
            _ => {}
        }

        // ── Standard path: dunder dispatch + fallback ──
        // For IntEnum/IntFlag members, if the primitive op fails, retry with _value_
        macro_rules! with_enum_fallback {
            ($a:expr, $b:expr, $op:ident) => {
                match $a.$op(&$b) {
                    Ok(r) => r,
                    Err(_) => {
                        let ua = unwrap_int_enum(&$a);
                        let ub = unwrap_int_enum(&$b);
                        if !Arc::ptr_eq(&ua, &$a) || !Arc::ptr_eq(&ub, &$b) {
                            ua.$op(&ub)?
                        } else {
                            return Err($a.$op(&$b).unwrap_err());
                        }
                    }
                }
            };
        }
        let result = match instr.op {
            Opcode::BinaryAdd => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__add__", Some("__radd__"))? { r }
                else { with_enum_fallback!(a, b, add) }
            }
            Opcode::InplaceAdd => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__iadd__", "__add__")? { r }
                else { with_enum_fallback!(a, b, add) }
            }
            Opcode::BinarySubtract => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__sub__", Some("__rsub__"))? { r }
                else { with_enum_fallback!(a, b, sub) }
            }
            Opcode::InplaceSubtract => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__isub__", "__sub__")? { r }
                else { with_enum_fallback!(a, b, sub) }
            }
            Opcode::BinaryMultiply => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__mul__", Some("__rmul__"))? { r }
                else { with_enum_fallback!(a, b, mul) }
            }
            Opcode::InplaceMultiply => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__imul__", "__mul__")? { r }
                else { with_enum_fallback!(a, b, mul) }
            }
            Opcode::BinaryTrueDivide => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__truediv__", Some("__rtruediv__"))? { r }
                else { with_enum_fallback!(a, b, true_div) }
            }
            Opcode::InplaceTrueDivide => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__itruediv__", "__truediv__")? { r }
                else { with_enum_fallback!(a, b, true_div) }
            }
            Opcode::BinaryFloorDivide => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__floordiv__", Some("__rfloordiv__"))? { r }
                else { with_enum_fallback!(a, b, floor_div) }
            }
            Opcode::InplaceFloorDivide => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ifloordiv__", "__floordiv__")? { r }
                else { with_enum_fallback!(a, b, floor_div) }
            }
            Opcode::BinaryModulo | Opcode::InplaceModulo => {
                // str % val → Python printf-style formatting
                if let PyObjectPayload::Str(fmt_str) = &a.payload {
                    string_percent_format(fmt_str, &b)?
                } else if let Some(r) = self.try_binary_dunder(&a, &b, "__mod__", Some("__rmod__"))? { r }
                else { with_enum_fallback!(a, b, modulo) }
            }
            Opcode::BinaryPower | Opcode::InplacePower => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__pow__", Some("__rpow__"))? { r }
                else { with_enum_fallback!(a, b, power) }
            }
            Opcode::BinaryLshift | Opcode::InplaceLshift => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__lshift__", Some("__rlshift__"))? { r }
                else { with_enum_fallback!(a, b, lshift) }
            }
            Opcode::BinaryRshift | Opcode::InplaceRshift => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__rshift__", Some("__rrshift__"))? { r }
                else { with_enum_fallback!(a, b, rshift) }
            }
            Opcode::BinaryAnd | Opcode::InplaceAnd => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__and__", Some("__rand__"))? { r }
                else { with_enum_fallback!(a, b, bit_and) }
            }
            Opcode::BinaryOr | Opcode::InplaceOr => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__or__", Some("__ror__"))? { r }
                else { with_enum_fallback!(a, b, bit_or) }
            }
            Opcode::BinaryXor | Opcode::InplaceXor => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__xor__", Some("__rxor__"))? { r }
                else { with_enum_fallback!(a, b, bit_xor) }
            }
            Opcode::BinaryMatrixMultiply | Opcode::InplaceMatrixMultiply => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__matmul__", Some("__rmatmul__"))? { r }
                else {
                    return Err(PyException::type_error(format!(
                        "unsupported operand type(s) for @: '{}' and '{}'",
                        a.type_name(), b.type_name()
                    )));
                }
            }
            _ => unreachable!(),
        };
        self.vm_push(result);
        Ok(None)
    }

}

// ── Group 6: Subscript operations ────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_subscript_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::BinarySubscr => {
                let raw_key = self.vm_pop();
                let obj = self.vm_pop();
                // Resolve __index__ on the key if it's an Instance with __index__
                let key = if matches!(&raw_key.payload, PyObjectPayload::Instance(_)) {
                    if let Some(r) = self.try_call_dunder(&raw_key, "__index__", vec![])? {
                        r
                    } else {
                        raw_key
                    }
                } else {
                    raw_key
                };
                // __class_getitem__: MyClass[int] → MyClass.__class_getitem__(cls, int)
                if matches!(&obj.payload, PyObjectPayload::Class(_)) {
                    if let Some(cgi) = obj.get_attr("__class_getitem__") {
                        let result = self.call_object(cgi, vec![obj.clone(), key])?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                    // Enum-style __getitem__: Color["RED"]
                    if let Some(gi) = obj.get_attr("__getitem__") {
                        let result = self.call_object(gi, vec![obj.clone(), key])?;
                        self.vm_push(result);
                        return Ok(None);
                    }
                    // typing generic alias: List[int] → _GenericAlias with str
                    if let Some(typing_name) = obj.get_attr("__typing_name__") {
                        let name = typing_name.py_to_string();
                        let params = match &key.payload {
                            PyObjectPayload::Tuple(items) => {
                                items.iter().map(|i| i.type_name().to_string()).collect::<Vec<_>>().join(", ")
                            }
                            _ => key.type_name().to_string(),
                        };
                        let repr = format!("{}[{}]", name, params);
                        let alias_cls = PyObject::class(CompactString::from("_GenericAlias"), vec![], IndexMap::new());
                        let mut attrs = IndexMap::new();
                        attrs.insert(intern_or_new("__typing_repr__"), PyObject::str_val(CompactString::from(&repr)));
                        attrs.insert(intern_or_new("__str__"), PyObject::str_val(CompactString::from(&repr)));
                        self.vm_push(PyObject::instance_with_attrs(alias_cls, attrs));
                        return Ok(None);
                    }
                    // Generic fallback: Class[X] returns the class itself (PEP 585)
                    self.vm_push(obj.clone());
                    return Ok(None);
                }
                // BuiltinType subscript: list[int], dict[str, int] → returns the type (PEP 585)
                if matches!(&obj.payload, PyObjectPayload::BuiltinType(_)) {
                    self.vm_push(obj.clone());
                    return Ok(None);
                }
                // Dict subclass: Instance with dict_storage
                // If the subclass defines its own __getitem__, call it instead of dict_storage
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let Some(ref ds) = inst.dict_storage {
                        let has_user_getitem = Self::class_has_user_override(&inst.class, "__getitem__");
                        if has_user_getitem {
                            // Let dunder dispatch handle it below
                        } else {
                            let hk = self.vm_to_hashable_key(&key)?;
                            let existing = ds.read().get(&hk).cloned();
                            if let Some(val) = existing {
                                self.vm_push(val);
                            } else {
                                // Check for __missing__
                                if let Some(missing) = obj.get_attr("__missing__") {
                                    let result = self.call_object(missing, vec![key])?;
                                    self.vm_push(result);
                                } else {
                                    return Err(PyException::key_error(key.py_to_string()));
                                }
                            }
                            return Ok(None);
                        }
                    }
                }
                if let Some(r) = self.try_call_dunder(&obj, "__getitem__", vec![key.clone()])? {
                    self.vm_push(r);
                    return Ok(None);
                }
                // Builtin base type subclass: delegate to __builtin_value__
                if let PyObjectPayload::Instance(_) = &obj.payload {
                    if let Some(bv) = obj.get_attr("__builtin_value__") {
                        self.vm_push(bv.get_item(&key)?);
                        return Ok(None);
                    }
                    if let Some(tup) = obj.get_attr("_tuple") {
                        self.vm_push(tup.get_item(&key)?);
                        return Ok(None);
                    }
                }
                if let PyObjectPayload::Dict(map) = &obj.payload {
                    let hk = self.vm_to_hashable_key(&key)?;
                    let existing = map.read().get(&hk).cloned();
                    if let Some(val) = existing {
                        self.vm_push(val);
                    } else {
                        let factory_key = HashableKey::Str(intern_or_new("__defaultdict_factory__"));
                        let factory = map.read().get(&factory_key).cloned();
                        if let Some(factory) = factory {
                            let default = self.call_object(factory, vec![])?;
                            map.write().insert(hk, default.clone());
                            self.vm_push(default);
                            return Ok(None);
                        } else {
                            return Err(PyException::key_error(key.py_to_string()));
                        }
                    }
                } else {
                    self.vm_push(obj.get_item(&key)?);
                }
            }
            Opcode::StoreSubscr => {
                let key = self.vm_pop();
                let obj = self.vm_pop();
                let value = self.vm_pop();
                match &obj.payload {
                    PyObjectPayload::List(items) => {
                        if let PyObjectPayload::Slice { start, stop, step } = &key.payload {
                            let step_val = step.as_ref().map(|v| v.as_int().unwrap_or(1)).unwrap_or(1);
                            let new_items = value.to_list()?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            
                            if step_val == 1 || step_val == 0 {
                                // Contiguous slice assignment: a[s:e] = items
                                let s_val = start.as_ref().map(|v| v.as_int().unwrap_or(0)).unwrap_or(0);
                                let e_val = stop.as_ref().map(|v| v.as_int().unwrap_or(len)).unwrap_or(len);
                                let s = (if s_val < 0 { (len + s_val).max(0) } else { s_val.min(len) }) as usize;
                                let e = (if e_val < 0 { (len + e_val).max(0) } else { e_val.min(len) }) as usize;
                                let e = e.max(s);
                                w.splice(s..e, new_items);
                            } else {
                                // Extended slice assignment: a[s:e:step] = items
                                let s_val = if step_val > 0 {
                                    start.as_ref().map(|v| v.as_int().unwrap_or(0)).unwrap_or(0)
                                } else {
                                    start.as_ref().map(|v| v.as_int().unwrap_or(len - 1)).unwrap_or(len - 1)
                                };
                                let e_val = if step_val > 0 {
                                    stop.as_ref().map(|v| v.as_int().unwrap_or(len)).unwrap_or(len)
                                } else {
                                    stop.as_ref().map(|v| v.as_int().unwrap_or(-len - 1)).unwrap_or(-len - 1)
                                };
                                // Collect indices
                                let mut indices = Vec::new();
                                let mut i = if s_val < 0 { (len + s_val).max(0) } else { s_val.min(len) };
                                let end = if e_val < 0 { (len + e_val).max(-1) } else { e_val.min(len) };
                                if step_val > 0 {
                                    while i < end {
                                        indices.push(i as usize);
                                        i += step_val;
                                    }
                                } else {
                                    while i > end {
                                        indices.push(i as usize);
                                        i += step_val;
                                    }
                                }
                                if indices.len() != new_items.len() {
                                    return Err(PyException::value_error(format!(
                                        "attempt to assign sequence of size {} to extended slice of size {}",
                                        new_items.len(), indices.len()
                                    )));
                                }
                                for (idx, val) in indices.iter().zip(new_items.iter()) {
                                    w[*idx] = val.clone();
                                }
                            }
                        } else {
                            let idx = key.to_int()?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error("list assignment index out of range"));
                            }
                            w[actual as usize] = value;
                        }
                    }
                    PyObjectPayload::Dict(map) => {
                        let hk = self.vm_to_hashable_key(&key)?;
                        map.write().insert(hk, value);
                    }
                    PyObjectPayload::ByteArray(ref bytes) => {
                        let idx = key.to_int()?;
                        let byte_val = value.to_int()? as u8;
                        // ByteArray is immutable-shared via Arc, so we need unsafe or a wrapper.
                        // Actually, ByteArray uses Vec<u8> directly in the payload.
                        // We need a mutable reference. Since PyObjectPayload::ByteArray wraps Vec<u8>,
                        // we can't mutate through Arc. Let's handle this via a workaround.
                        let len = bytes.len() as i64;
                        let actual = if idx < 0 { len + idx } else { idx };
                        if actual < 0 || actual >= len {
                            return Err(PyException::index_error("bytearray index out of range"));
                        }
                        // Use unsafe to mutate the inner bytes through the Arc
                        unsafe {
                            let ptr = bytes.as_ptr() as *mut u8;
                            *ptr.add(actual as usize) = byte_val;
                        }
                    }
                    PyObjectPayload::InstanceDict(attrs) => {
                        let key_str = CompactString::from(key.py_to_string());
                        attrs.write().insert(key_str, value);
                    }
                    PyObjectPayload::Instance(inst) => {
                        // Dict subclass: use dict_storage if no user override
                        if let Some(ref ds) = inst.dict_storage {
                            let has_user_setitem = Self::class_has_user_override(&inst.class, "__setitem__");
                            if has_user_setitem {
                                if let Some(m) = obj.get_attr("__setitem__") {
                                    self.call_object(m, vec![key, value])?;
                                    return Ok(None);
                                }
                            }
                            let hk = self.vm_to_hashable_key(&key)?;
                            ds.write().insert(hk, value);
                        } else if let Some(m) = obj.get_attr("__setitem__") {
                            self.call_object(m, vec![key, value])?;
                            return Ok(None);
                        } else {
                            return Err(PyException::type_error(format!(
                                "'{}' object does not support item assignment", obj.type_name())));
                        }
                    }
                    _ => return Err(PyException::type_error(format!(
                        "'{}' object does not support item assignment", obj.type_name()))),
                }
            }
            Opcode::DeleteSubscr => {
                let key = self.vm_pop();
                let obj = self.vm_pop();
                match &obj.payload {
                    PyObjectPayload::List(items) => {
                        if let PyObjectPayload::Slice { start, stop, step } = &key.payload {
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let s = start.as_ref().map(|v| v.to_int().unwrap_or(0)).unwrap_or(0);
                            let e = stop.as_ref().map(|v| v.to_int().unwrap_or(len)).unwrap_or(len);
                            let st = step.as_ref().map(|v| v.to_int().unwrap_or(1)).unwrap_or(1);
                            let s = if s < 0 { (len + s).max(0) } else { s.min(len) };
                            let e = if e < 0 { (len + e).max(0) } else { e.min(len) };
                            if st == 1 && s <= e {
                                w.drain(s as usize..e as usize);
                            } else if st == -1 && s >= e {
                                let mut indices: Vec<usize> = ((e + 1) as usize..=(s) as usize).collect();
                                indices.reverse();
                                for idx in indices {
                                    if idx < w.len() { w.remove(idx); }
                                }
                            } else if st > 1 {
                                let mut indices = Vec::new();
                                let mut i = s;
                                while i < e { indices.push(i as usize); i += st; }
                                indices.reverse();
                                for idx in indices { if idx < w.len() { w.remove(idx); } }
                            }
                        } else {
                            let idx = key.to_int()?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error("list assignment index out of range"));
                            }
                            w.remove(actual as usize);
                        }
                    }
                    PyObjectPayload::Dict(map) => {
                        let hk = self.vm_to_hashable_key(&key)?;
                        if map.write().shift_remove(&hk).is_none() {
                            return Err(PyException::key_error(key.repr()));
                        }
                    }
                    PyObjectPayload::Instance(inst) => {
                        if let Some(method) = obj.get_attr("__delitem__") {
                            self.call_object(method, vec![key])?;
                            return Ok(None);
                        }
                        if let Some(ref ds) = inst.dict_storage {
                            let hk = self.vm_to_hashable_key(&key)?;
                            if ds.write().shift_remove(&hk).is_none() {
                                return Err(PyException::key_error(key.repr()));
                            }
                            return Ok(None);
                        }
                        return Err(PyException::type_error(format!(
                            "'{}'  object does not support item deletion", obj.type_name())));
                    }
                    _ => return Err(PyException::type_error(format!(
                        "'{}' object does not support item deletion", obj.type_name()))),
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}

/// Python printf-style string formatting: "hello %s, %d items" % (name, count)
fn string_percent_format(fmt: &str, args: &PyObjectRef) -> Result<PyObjectRef, PyException> {
    let arg_list: Vec<PyObjectRef> = match &args.payload {
        PyObjectPayload::Tuple(items) => items.clone(),
        _ => vec![args.clone()],
    };

    let mut result = String::with_capacity(fmt.len() + 32);
    let mut chars = fmt.chars().peekable();
    let mut arg_idx = 0;

    while let Some(ch) = chars.next() {
        if ch != '%' {
            result.push(ch);
            continue;
        }
        match chars.peek() {
            Some(&'%') => { chars.next(); result.push('%'); }
            Some(_) => {
                let mut flags = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '-' || c == '+' || c == '0' || c == ' ' || c == '#' {
                        flags.push(c); chars.next();
                    } else { break; }
                }
                let mut width = 0usize;
                if let Some(&'*') = chars.peek() {
                    chars.next();
                    if arg_idx < arg_list.len() {
                        width = arg_list[arg_idx].as_int().unwrap_or(0) as usize;
                        arg_idx += 1;
                    }
                } else {
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() { width = width * 10 + (c as usize - '0' as usize); chars.next(); }
                        else { break; }
                    }
                }
                let mut precision: Option<usize> = None;
                if let Some(&'.') = chars.peek() {
                    chars.next();
                    let mut p = 0usize;
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_digit() { p = p * 10 + (c as usize - '0' as usize); chars.next(); }
                        else { break; }
                    }
                    precision = Some(p);
                }
                let spec = chars.next().unwrap_or('s');
                if arg_idx >= arg_list.len() {
                    return Err(PyException::type_error("not enough arguments for format string"));
                }
                let arg = &arg_list[arg_idx];
                arg_idx += 1;

                let formatted = match spec {
                    's' => arg.py_to_string(),
                    'r' => arg.py_to_string(),
                    'd' | 'i' => format!("{}", arg.as_int().unwrap_or(0)),
                    'f' | 'F' => {
                        let v = arg.to_float().unwrap_or(0.0);
                        let p = precision.unwrap_or(6);
                        format!("{:.prec$}", v, prec = p)
                    }
                    'e' | 'E' => {
                        let v = arg.to_float().unwrap_or(0.0);
                        let p = precision.unwrap_or(6);
                        if spec == 'e' { format!("{:.prec$e}", v, prec = p) }
                        else { format!("{:.prec$E}", v, prec = p) }
                    }
                    'g' | 'G' => format!("{}", arg.to_float().unwrap_or(0.0)),
                    'x' => format!("{:x}", arg.as_int().unwrap_or(0)),
                    'X' => format!("{:X}", arg.as_int().unwrap_or(0)),
                    'o' => format!("{:o}", arg.as_int().unwrap_or(0)),
                    'c' => {
                        if let Some(n) = arg.as_int() {
                            char::from_u32(n as u32).map(|c| c.to_string()).unwrap_or_default()
                        } else {
                            arg.py_to_string().chars().next().map(|c| c.to_string()).unwrap_or_default()
                        }
                    }
                    _ => format!("%{}", spec),
                };

                if width > 0 && formatted.len() < width {
                    if flags.contains('-') {
                        result.push_str(&formatted);
                        for _ in 0..(width - formatted.len()) { result.push(' '); }
                    } else {
                        let pad = if flags.contains('0') { '0' } else { ' ' };
                        for _ in 0..(width - formatted.len()) { result.push(pad); }
                        result.push_str(&formatted);
                    }
                } else {
                    result.push_str(&formatted);
                }
            }
            None => { result.push('%'); }
        }
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}
