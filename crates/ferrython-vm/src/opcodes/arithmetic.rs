use super::unwrap_int_enum;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{PyException, PyResult};
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
                if let Some(r) = resolved.or_else(|| self.try_call_dunder(&v, "__invert__", vec![]).ok().flatten()) {
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
                else if let PyObjectPayload::List(items) = &a.payload {
                    // list += iterable → extend in-place (same identity)
                    let new_items = b.to_list()?;
                    items.write().extend(new_items);
                    a.clone()
                }
                else if let PyObjectPayload::Set(set) = &a.payload {
                    // set |= iterable → update in-place
                    if let PyObjectPayload::Set(other) = &b.payload {
                        let other_items: Vec<_> = other.read().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                        set.write().extend(other_items);
                    }
                    a.clone()
                }
                else { with_enum_fallback!(a, b, add) }
            }
            Opcode::BinarySubtract => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__sub__", Some("__rsub__"))? { r }
                else { with_enum_fallback!(a, b, sub) }
            }
            Opcode::InplaceSubtract => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__isub__", "__sub__")? { r }
                else if let (PyObjectPayload::Set(set), PyObjectPayload::Set(other)) = (&a.payload, &b.payload) {
                    let keys_to_remove: Vec<_> = other.read().keys().cloned().collect();
                    let mut w = set.write();
                    for k in keys_to_remove {
                        w.shift_remove(&k);
                    }
                    drop(w);
                    a.clone()
                }
                else { with_enum_fallback!(a, b, sub) }
            }
            Opcode::BinaryMultiply => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__mul__", Some("__rmul__"))? { r }
                else { with_enum_fallback!(a, b, mul) }
            }
            Opcode::InplaceMultiply => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__imul__", "__mul__")? { r }
                else if let (PyObjectPayload::List(items), PyObjectPayload::Int(n)) = (&a.payload, &b.payload) {
                    // list *= n → repeat in-place
                    let n = n.to_i64().unwrap_or(0).max(0) as usize;
                    let mut w = items.write();
                    let orig: Vec<_> = w.clone();
                    w.clear();
                    for _ in 0..n {
                        w.extend_from_slice(&orig);
                    }
                    drop(w);
                    a.clone()
                }
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
                    self.vm_string_percent_format(fmt_str, &b)?
                } else if let PyObjectPayload::Bytes(fmt_bytes) = &a.payload {
                    self.vm_bytes_percent_format(fmt_bytes, &b)?
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
            Opcode::BinaryAnd => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__and__", Some("__rand__"))? { r }
                else { with_enum_fallback!(a, b, bit_and) }
            }
            Opcode::InplaceAnd => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__iand__", "__and__")? { r }
                else if let (PyObjectPayload::Set(set), PyObjectPayload::Set(other)) = (&a.payload, &b.payload) {
                    let other_keys: indexmap::IndexSet<_> = other.read().keys().cloned().collect();
                    set.write().retain(|k, _| other_keys.contains(k));
                    a.clone()
                }
                else { with_enum_fallback!(a, b, bit_and) }
            }
            Opcode::BinaryOr => {
                // PEP 604: type | type → UnionType for isinstance checks
                if Self::is_type_like(&a) && Self::is_type_like(&b)
                {
                    self.make_union_type(&a, &b)?
                }
                else if let Some(r) = self.try_binary_dunder(&a, &b, "__or__", Some("__ror__"))? { r }
                else if let (PyObjectPayload::Dict(_), PyObjectPayload::Dict(_)) = (&a.payload, &b.payload) {
                    // Delegate to py_bit_or which handles Counter union (max) vs regular dict merge
                    a.bit_or(&b)?
                }
                else { with_enum_fallback!(a, b, bit_or) }
            }
            Opcode::InplaceOr => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ior__", "__or__")? { r }
                else if let (PyObjectPayload::Dict(_), PyObjectPayload::Dict(_)) = (&a.payload, &b.payload) {
                    a.bit_or(&b)?
                }
                else if let (PyObjectPayload::Set(set), PyObjectPayload::Set(other)) = (&a.payload, &b.payload) {
                    let items: Vec<_> = other.read().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    set.write().extend(items);
                    a.clone()
                }
                else { with_enum_fallback!(a, b, bit_or) }
            }
            Opcode::BinaryXor => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__xor__", Some("__rxor__"))? { r }
                else { with_enum_fallback!(a, b, bit_xor) }
            }
            Opcode::InplaceXor => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ixor__", "__xor__")? { r }
                else if let (PyObjectPayload::Set(set), PyObjectPayload::Set(other)) = (&a.payload, &b.payload) {
                    let other_read = other.read();
                    let other_keys: indexmap::IndexSet<_> = other_read.keys().cloned().collect();
                    let mut s = set.write();
                    let my_keys: indexmap::IndexSet<_> = s.keys().cloned().collect();
                    // Remove items in both, add items only in other
                    s.retain(|k, _| !other_keys.contains(k));
                    for (k, v) in other_read.iter() {
                        if !my_keys.contains(k) {
                            s.insert(k.clone(), v.clone());
                        }
                    }
                    drop(s);
                    a.clone()
                }
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
            // LoadFastLoadConstBinarySub fallback: operands already on stack, treat as subtract
            Opcode::LoadFastLoadConstBinarySub => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__sub__", Some("__rsub__"))? { r }
                else { with_enum_fallback!(a, b, sub) }
            }
            // LoadFastLoadConstBinaryAdd fallback: operands already on stack, treat as add
            Opcode::LoadFastLoadConstBinaryAdd | Opcode::LoadFastLoadFastBinaryAdd => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__add__", Some("__radd__"))? { r }
                else { with_enum_fallback!(a, b, add) }
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
                    // Enum-style __getitem__: Color["RED"] — only if key is a string
                    // (not for generic subscript like MyClass[int])
                    if matches!(&key.payload, PyObjectPayload::Str(_)) {
                        if let Some(gi) = obj.get_attr("__getitem__") {
                            let result = self.call_object(gi, vec![obj.clone(), key])?;
                            self.vm_push(result);
                            return Ok(None);
                        }
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
                // BuiltinType subscript: list[int], dict[str, int] → GenericAlias (PEP 585)
                if let PyObjectPayload::BuiltinType(bt) = &obj.payload {
                    let type_name = bt.as_str();
                    let params = match &key.payload {
                        PyObjectPayload::Tuple(items) => {
                            items.iter().map(|i| self.format_type_param(i)).collect::<Vec<_>>().join(", ")
                        }
                        _ => self.format_type_param(&key),
                    };
                    let repr = format!("{}[{}]", type_name, params);
                    let alias_cls = PyObject::class(CompactString::from("types.GenericAlias"), vec![], IndexMap::new());
                    let mut attrs = IndexMap::new();
                    attrs.insert(intern_or_new("__origin__"), obj.clone());
                    attrs.insert(intern_or_new("__args__"), key.clone());
                    attrs.insert(intern_or_new("__typing_repr__"), PyObject::str_val(CompactString::from(&repr)));
                    let alias = PyObject::instance_with_attrs(alias_cls, attrs);
                    self.vm_push(alias);
                    return Ok(None);
                }
                // Dict subclass: Instance with dict_storage
                // Typing aliases: _GenericAlias[X] or types.GenericAlias[X] → new alias
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        if cd.name.contains("GenericAlias") || cd.name.contains("_GenericAlias") {
                            let base_repr = inst.attrs.read().get("__typing_repr__")
                                .map(|r| r.py_to_string())
                                .unwrap_or_else(|| obj.py_to_string());
                            let params = match &key.payload {
                                PyObjectPayload::Tuple(items) => {
                                    items.iter().map(|i| self.format_type_param(i)).collect::<Vec<_>>().join(", ")
                                }
                                _ => self.format_type_param(&key),
                            };
                            let repr = format!("{}[{}]", base_repr, params);
                            let alias_cls = PyObject::class(CompactString::from("_GenericAlias"), vec![], IndexMap::new());
                            let mut attrs = IndexMap::new();
                            attrs.insert(intern_or_new("__typing_repr__"), PyObject::str_val(CompactString::from(&repr)));
                            if let Some(origin) = inst.attrs.read().get("__origin__").cloned() {
                                attrs.insert(intern_or_new("__origin__"), origin);
                            }
                            attrs.insert(intern_or_new("__args__"), key.clone());
                            self.vm_push(PyObject::instance_with_attrs(alias_cls, attrs));
                            return Ok(None);
                        }
                    }
                }
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
                } else if let PyObjectPayload::InstanceDict(attrs) = &obj.payload {
                    let key_str = CompactString::from(key.py_to_string());
                    if let Some(val) = attrs.read().get(&key_str).cloned() {
                        self.vm_push(val);
                    } else {
                        return Err(PyException::key_error(key.py_to_string()));
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
                        if let PyObjectPayload::Slice { start, stop, step } = &key.payload {
                            // Slice assignment on bytearray
                            let len = bytes.len() as i64;
                            let step_val = step.as_ref().map(|v| v.as_int().unwrap_or(1)).unwrap_or(1);
                            let new_bytes: Vec<u8> = if let PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) = &value.payload {
                                b.clone()
                            } else if let Some(n) = value.as_int() {
                                vec![0u8; n.max(0) as usize]
                            } else {
                                // Try to collect as list of ints
                                let items = value.to_list()?;
                                items.iter().map(|v| v.to_int().unwrap_or(0) as u8).collect()
                            };
                            
                            if step_val == 1 || step_val == 0 {
                                let s_val = start.as_ref().map(|v| v.as_int().unwrap_or(0)).unwrap_or(0);
                                let e_val = stop.as_ref().map(|v| v.as_int().unwrap_or(len)).unwrap_or(len);
                                let s = (if s_val < 0 { (len + s_val).max(0) } else { s_val.min(len) }) as usize;
                                let e = (if e_val < 0 { (len + e_val).max(0) } else { e_val.min(len) }) as usize;
                                let e = e.max(s);
                                // Build new bytearray and replace contents
                                let mut result = bytes[..s].to_vec();
                                result.extend_from_slice(&new_bytes);
                                result.extend_from_slice(&bytes[e..]);
                                // Overwrite using pointer manipulation (matching existing pattern)
                                let ptr = bytes.as_ptr() as *mut u8;
                                unsafe {
                                    // Resize the backing buffer if needed
                                    if result.len() == bytes.len() {
                                        std::ptr::copy_nonoverlapping(result.as_ptr(), ptr, result.len());
                                    } else {
                                        // For now, just copy what fits (bytearray slice assign with same-length replacement)
                                        let copy_len = result.len().min(bytes.len());
                                        std::ptr::copy_nonoverlapping(result.as_ptr(), ptr, copy_len);
                                    }
                                }
                            } else {
                                // Extended slice: collect indices
                                let mut indices = Vec::new();
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
                                let mut i = if s_val < 0 { (len + s_val).max(0) } else { s_val.min(len) };
                                let end = if e_val < 0 { (len + e_val).max(-1) } else { e_val.min(len) };
                                if step_val > 0 {
                                    while i < end { indices.push(i as usize); i += step_val; }
                                } else {
                                    while i > end { indices.push(i as usize); i += step_val; }
                                }
                                if indices.len() != new_bytes.len() {
                                    return Err(PyException::value_error(format!(
                                        "attempt to assign bytes of size {} to extended slice of size {}",
                                        new_bytes.len(), indices.len()
                                    )));
                                }
                                unsafe {
                                    let ptr = bytes.as_ptr() as *mut u8;
                                    for (idx, &val) in indices.iter().zip(new_bytes.iter()) {
                                        *ptr.add(*idx) = val;
                                    }
                                }
                            }
                        } else {
                            let idx = key.to_int()?;
                            let byte_val = value.to_int()? as u8;
                            let len = bytes.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error("bytearray index out of range"));
                            }
                            unsafe {
                                let ptr = bytes.as_ptr() as *mut u8;
                                *ptr.add(actual as usize) = byte_val;
                            }
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
                    PyObjectPayload::Module(ref md) => {
                        // Module with __setitem__ (e.g., os.environ)
                        let setitem = md.attrs.read().get("__setitem__").cloned();
                        if let Some(m) = setitem {
                            self.call_object(m, vec![key, value])?;
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
                    PyObjectPayload::InstanceDict(attrs) => {
                        let key_str = CompactString::from(key.py_to_string());
                        if attrs.write().shift_remove(&key_str).is_none() {
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
                    PyObjectPayload::Module(ref md) => {
                        let delitem = md.attrs.read().get("__delitem__").cloned();
                        if let Some(m) = delitem {
                            self.call_object(m, vec![key])?;
                        } else {
                            return Err(PyException::type_error(format!(
                                "'{}' object does not support item deletion", obj.type_name())));
                        }
                    }
                    _ => return Err(PyException::type_error(format!(
                        "'{}' object does not support item deletion", obj.type_name()))),
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }

    fn format_type_param(&self, obj: &PyObjectRef) -> String {
        match &obj.payload {
            PyObjectPayload::BuiltinType(bt) => bt.as_str().to_string(),
            PyObjectPayload::Class(cls) => cls.name.to_string(),
            PyObjectPayload::None => "None".to_string(),
            _ => obj.type_name().to_string(),
        }
    }

    fn is_type_like(obj: &PyObjectRef) -> bool {
        matches!(&obj.payload, PyObjectPayload::BuiltinType(_) | PyObjectPayload::Class(_) | PyObjectPayload::None)
            || obj.get_attr("__union_params__").map_or(false, |f| f.is_truthy())
            // PEP 604: GenericAlias (e.g. tuple[str, str]) supports | for union types
            || if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    cd.name.contains("GenericAlias") || cd.name.contains("_GenericAlias")
                } else { false }
            } else { false }
    }

    fn make_union_type(&self, a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        // Collect args from both sides (flatten nested unions)
        let mut args = Vec::new();
        Self::collect_union_args(a, &mut args);
        Self::collect_union_args(b, &mut args);
        let repr = args.iter().map(|a| {
            match &a.payload {
                PyObjectPayload::BuiltinType(bt) => bt.as_str().to_string(),
                PyObjectPayload::Class(cls) => cls.name.to_string(),
                PyObjectPayload::None => "None".to_string(),
                _ => a.type_name().to_string(),
            }
        }).collect::<Vec<_>>().join(" | ");
        let args_tuple = PyObject::tuple(args);
        let union_cls = PyObject::class(CompactString::from("types.UnionType"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(intern_or_new("__args__"), args_tuple);
        attrs.insert(intern_or_new("__typing_repr__"), PyObject::str_val(CompactString::from(&repr)));
        attrs.insert(intern_or_new("__union_params__"), PyObject::bool_val(true));
        Ok(PyObject::instance_with_attrs(union_cls, attrs))
    }

    fn collect_union_args(obj: &PyObjectRef, out: &mut Vec<PyObjectRef>) {
        if let Some(args) = obj.get_attr("__union_params__") {
            if args.is_truthy() {
                if let Some(inner) = obj.get_attr("__args__") {
                    if let PyObjectPayload::Tuple(items) = &inner.payload {
                        out.extend(items.iter().cloned());
                        return;
                    }
                }
            }
        }
        out.push(obj.clone());
    }
}

/// Python printf-style string formatting: "hello %s, %d items" % (name, count)
impl VirtualMachine {
    /// VM-aware string % formatting. Uses vm_repr/vm_str to properly call user
    /// __repr__/__str__ dunders that need VM context.
    fn vm_string_percent_format(&mut self, fmt: &str, args: &PyObjectRef) -> Result<PyObjectRef, PyException> {
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
                    // Check for %(name) dict-keyed format
                    let dict_key = if chars.peek() == Some(&'(') {
                        chars.next();
                        let mut key = String::new();
                        while let Some(&c) = chars.peek() {
                            if c == ')' { chars.next(); break; }
                            key.push(c);
                            chars.next();
                        }
                        Some(key)
                    } else {
                        None
                    };

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

                    let arg = if let Some(ref key) = dict_key {
                        let key_obj = PyObject::str_val(CompactString::from(key.as_str()));
                        args.get_item(&key_obj)?
                    } else {
                        if arg_idx >= arg_list.len() {
                            return Err(PyException::type_error("not enough arguments for format string"));
                        }
                        let a = arg_list[arg_idx].clone();
                        arg_idx += 1;
                        a
                    };

                    let formatted = match spec {
                        's' => self.vm_str(&arg)?,
                        'r' => self.vm_repr(&arg)?,
                        'd' | 'i' => format!("{}", arg.as_int().unwrap_or(0)),
                        'f' | 'F' => {
                            let v = arg.to_float().unwrap_or(0.0);
                            let p = precision.unwrap_or(6);
                            format!("{:.prec$}", v, prec = p)
                        }
                        'e' | 'E' => {
                            let v = arg.to_float().unwrap_or(0.0);
                            let p = precision.unwrap_or(6);
                            let raw = if spec == 'e' { format!("{:.prec$e}", v, prec = p) }
                            else { format!("{:.prec$E}", v, prec = p) };
                            normalize_sci_exp(&raw, spec)
                        }
                        'g' | 'G' => {
                            let v = arg.to_float().unwrap_or(0.0);
                            let p = precision.unwrap_or(6);
                            let abs_v = v.abs();
                            let use_sci = abs_v != 0.0 && (abs_v >= 10f64.powi(p as i32) || abs_v < 1e-4);
                            if use_sci {
                                let sp = if p > 0 { p - 1 } else { 0 };
                                let ec = if spec == 'g' { 'e' } else { 'E' };
                                let raw = if ec == 'e' { format!("{:.prec$e}", v, prec = sp) }
                                else { format!("{:.prec$E}", v, prec = sp) };
                                normalize_sci_exp(&raw, ec)
                            } else {
                                let s = format!("{:.prec$}", v, prec = p);
                                if s.contains('.') {
                                    s.trim_end_matches('0').trim_end_matches('.').to_string()
                                } else { s }
                            }
                        }
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

    /// Bytes % formatting (PEP 461)
    fn vm_bytes_percent_format(&mut self, fmt: &[u8], args: &PyObjectRef) -> Result<PyObjectRef, PyException> {
        let arg_list: Vec<PyObjectRef> = match &args.payload {
            PyObjectPayload::Tuple(items) => items.clone(),
            _ => vec![args.clone()],
        };

        let mut result = Vec::with_capacity(fmt.len() + 32);
        let mut i = 0;
        let mut arg_idx = 0;

        while i < fmt.len() {
            if fmt[i] != b'%' {
                result.push(fmt[i]);
                i += 1;
                continue;
            }
            i += 1;
            if i >= fmt.len() { break; }
            if fmt[i] == b'%' { result.push(b'%'); i += 1; continue; }

            // Parse flags
            let mut zero_pad = false;
            let mut left_align = false;
            while i < fmt.len() && matches!(fmt[i], b'-' | b'+' | b'0' | b' ' | b'#') {
                if fmt[i] == b'0' { zero_pad = true; }
                if fmt[i] == b'-' { left_align = true; }
                i += 1;
            }
            // Parse width
            let mut width = 0usize;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                width = width * 10 + (fmt[i] - b'0') as usize;
                i += 1;
            }
            // Parse precision
            let mut _precision: Option<usize> = None;
            if i < fmt.len() && fmt[i] == b'.' {
                i += 1;
                let mut p = 0usize;
                while i < fmt.len() && fmt[i].is_ascii_digit() {
                    p = p * 10 + (fmt[i] - b'0') as usize;
                    i += 1;
                }
                _precision = Some(p);
            }

            if i >= fmt.len() { break; }
            let spec = fmt[i];
            i += 1;

            if arg_idx >= arg_list.len() {
                return Err(PyException::type_error("not enough arguments for format string"));
            }
            let arg = &arg_list[arg_idx];
            arg_idx += 1;

            let formatted: Vec<u8> = match spec {
                b's' | b'b' => {
                    match &arg.payload {
                        PyObjectPayload::Bytes(b) => b.clone(),
                        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                        _ => {
                            let s = self.vm_str(arg)?;
                            s.into_bytes()
                        }
                    }
                }
                b'r' | b'a' => {
                    let s = self.vm_repr(arg)?;
                    s.into_bytes()
                }
                b'd' | b'i' | b'u' => {
                    let v = arg.as_int().unwrap_or(0);
                    format!("{}", v).into_bytes()
                }
                b'x' => {
                    let v = arg.as_int().unwrap_or(0);
                    format!("{:x}", v).into_bytes()
                }
                b'X' => {
                    let v = arg.as_int().unwrap_or(0);
                    format!("{:X}", v).into_bytes()
                }
                b'o' => {
                    let v = arg.as_int().unwrap_or(0);
                    format!("{:o}", v).into_bytes()
                }
                b'c' => {
                    let v = arg.as_int().unwrap_or(0) as u8;
                    vec![v]
                }
                _ => {
                    let mut fallback = vec![b'%'];
                    fallback.push(spec);
                    fallback
                }
            };

            // Apply width/padding
            if width > 0 && formatted.len() < width {
                let pad_len = width - formatted.len();
                let pad_byte = if zero_pad && !left_align { b'0' } else { b' ' };
                if left_align {
                    result.extend_from_slice(&formatted);
                    result.extend(std::iter::repeat(b' ').take(pad_len));
                } else {
                    result.extend(std::iter::repeat(pad_byte).take(pad_len));
                    result.extend_from_slice(&formatted);
                }
            } else {
                result.extend_from_slice(&formatted);
            }
        }

        Ok(PyObject::bytes(result))
    }
}

/// Normalize Rust scientific notation to CPython format.
/// Rust: "1.23e3" → Python: "1.23e+03"
fn normalize_sci_exp(raw: &str, e_char: char) -> String {
    if let Some(e_pos) = raw.rfind(e_char) {
        let mantissa = &raw[..e_pos];
        let exp_str = &raw[e_pos + 1..];
        let exp_val: i64 = exp_str.parse().unwrap_or(0);
        if exp_val >= 0 {
            format!("{}{}+{:02}", mantissa, e_char, exp_val)
        } else {
            format!("{}{}-{:02}", mantissa, e_char, -exp_val)
        }
    } else {
        raw.to_string()
    }
}
