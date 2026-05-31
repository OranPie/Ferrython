use super::unwrap_int_enum;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::helpers::{checked_repeat_len, index_to_usize_repeat};
use ferrython_core::object::{
    lookup_in_class_mro, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;
use indexmap::IndexMap;

mod percent_format;
mod subscript;
mod unary;

// ── Group 5: Binary / inplace arithmetic ─────────────────────────────
impl VirtualMachine {
    fn is_builtin_print_function(obj: &PyObjectRef) -> bool {
        matches!(&obj.payload, PyObjectPayload::BuiltinFunction(name) if name.as_str() == "print")
    }

    fn py2_print_redirection_type_error(a: &PyObjectRef, b: &PyObjectRef) -> PyException {
        PyException::type_error(format!(
            "unsupported operand type(s) for >>: '{}' and '{}'. Did you mean \"print(<message>, file=<output_stream>)\"?",
            a.type_name(),
            b.type_name()
        ))
    }

    pub(crate) fn try_binary_dunder(
        &mut self,
        a: &PyObjectRef,
        b: &PyObjectRef,
        dunder: &str,
        rdunder: Option<&str>,
    ) -> Result<Option<PyObjectRef>, PyException> {
        if let Some(unwrapped_a) = self.unwrap_weak_proxy_for_arithmetic(a)? {
            return self.try_binary_dunder(&unwrapped_a, b, dunder, rdunder);
        }
        if let Some(unwrapped_b) = self.unwrap_weak_proxy_for_arithmetic(b)? {
            return self.try_binary_dunder(a, &unwrapped_b, dunder, rdunder);
        }
        // Look up dunder via class MRO (not instance get_attr) for proper inheritance
        if let PyObjectPayload::Instance(inst) = &a.payload {
            if inst.attrs.read().contains_key("__deque__") {
                if let Some(method) = a.get_attr(dunder) {
                    let result = self.call_object(method, vec![b.clone()])?;
                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                        return Ok(Some(result));
                    }
                }
            }
            if let Some(method) = lookup_in_class_mro(&inst.class, dunder) {
                let bound = self.bind_method(a, method);
                let result = self.call_object(bound, vec![b.clone()])?;
                // If method returns NotImplemented, try the reflected dunder
                if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                    return Ok(Some(result));
                }
            }
            if matches!(
                dunder,
                "__add__"
                    | "__and__"
                    | "__or__"
                    | "__sub__"
                    | "__xor__"
                    | "__radd__"
                    | "__rand__"
                    | "__ror__"
                    | "__rsub__"
                    | "__rxor__"
            ) {
                if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                    let result = match dunder {
                        "__add__" | "__radd__" => bv.add(b),
                        "__and__" | "__rand__" => bv.bit_and(b),
                        "__or__" | "__ror__" => bv.bit_or(b),
                        "__sub__" | "__rsub__" => bv.sub(b),
                        "__xor__" | "__rxor__" => bv.bit_xor(b),
                        _ => unreachable!(),
                    }?;
                    return Ok(Some(result));
                }
            }
        }
        if let Some(rd) = rdunder {
            if let PyObjectPayload::Instance(inst) = &b.payload {
                if inst.attrs.read().contains_key("__deque__") {
                    if let Some(method) = b.get_attr(rd) {
                        let result = self.call_object(method, vec![a.clone()])?;
                        if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                            return Ok(Some(result));
                        }
                    }
                }
                if let Some(method) = lookup_in_class_mro(&inst.class, rd) {
                    let bound = self.bind_method(b, method);
                    let result = self.call_object(bound, vec![a.clone()])?;
                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                        return Ok(Some(result));
                    }
                }
                if matches!(
                    rd,
                    "__add__"
                        | "__and__"
                        | "__or__"
                        | "__sub__"
                        | "__xor__"
                        | "__radd__"
                        | "__rand__"
                        | "__ror__"
                        | "__rsub__"
                        | "__rxor__"
                ) {
                    if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                        let result = match rd {
                            "__radd__" | "__add__" => a.add(&bv),
                            "__rand__" | "__and__" => a.bit_and(&bv),
                            "__ror__" | "__or__" => a.bit_or(&bv),
                            "__rsub__" | "__sub__" => a.sub(&bv),
                            "__rxor__" | "__xor__" => a.bit_xor(&bv),
                            _ => unreachable!(),
                        }?;
                        return Ok(Some(result));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Python-aware addition: dispatches `__add__`/`__radd__` for Instance types,
    /// falls back to Rust-level `py_add` for primitives. Used by `sum()` and other
    /// builtins that need to support non-numeric `__add__`.
    pub(crate) fn vm_add(
        &mut self,
        a: &PyObjectRef,
        b: &PyObjectRef,
    ) -> Result<PyObjectRef, PyException> {
        if let Some(result) = self.try_binary_dunder(a, b, "__add__", Some("__radd__"))? {
            return Ok(result);
        }
        a.add(b)
    }

    fn try_inplace_dunder(
        &mut self,
        a: &PyObjectRef,
        b: &PyObjectRef,
        idunder: &str,
        dunder: &str,
    ) -> Result<Option<PyObjectRef>, PyException> {
        if let Some(unwrapped_a) = self.unwrap_weak_proxy_for_arithmetic(a)? {
            return self.try_inplace_dunder(&unwrapped_a, b, idunder, dunder);
        }
        if let Some(unwrapped_b) = self.unwrap_weak_proxy_for_arithmetic(b)? {
            return self.try_inplace_dunder(a, &unwrapped_b, idunder, dunder);
        }
        if let PyObjectPayload::Instance(inst) = &a.payload {
            if inst.attrs.read().contains_key("__deque__") {
                if let Some(method) = a.get_attr(idunder).or_else(|| a.get_attr(dunder)) {
                    let result = self.call_object(method, vec![b.clone()])?;
                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                        return Ok(Some(a.clone()));
                    }
                }
            }
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                match (idunder, &bv.payload) {
                    ("__iadd__", PyObjectPayload::List(items)) => {
                        let other = b.to_list()?;
                        items.write().extend(other);
                        return Ok(Some(a.clone()));
                    }
                    ("__imul__", PyObjectPayload::List(items)) => {
                        let count = index_to_usize_repeat(b)?;
                        let mut w = items.write();
                        let orig = w.clone();
                        checked_repeat_len(orig.len(), count, "list repeat")?;
                        w.clear();
                        for _ in 0..count {
                            w.extend_from_slice(&orig);
                        }
                        return Ok(Some(a.clone()));
                    }
                    ("__ior__", PyObjectPayload::Set(_))
                    | ("__iand__", PyObjectPayload::Set(_))
                    | ("__isub__", PyObjectPayload::Set(_))
                    | ("__ixor__", PyObjectPayload::Set(_)) => {
                        let result = match idunder {
                            "__ior__" => bv.bit_or(b),
                            "__iand__" => bv.bit_and(b),
                            "__isub__" => bv.sub(b),
                            "__ixor__" => bv.bit_xor(b),
                            _ => unreachable!(),
                        }?;
                        inst.attrs
                            .write()
                            .insert(intern_or_new("__builtin_value__"), result);
                        return Ok(Some(a.clone()));
                    }
                    _ => {}
                }
            }
            let method = lookup_in_class_mro(&inst.class, idunder)
                .or_else(|| lookup_in_class_mro(&inst.class, dunder));
            if let Some(m) = method {
                let bound = self.bind_method(a, m);
                let result = self.call_object(bound, vec![b.clone()])?;
                if inst.attrs.read().contains_key("__builtin_value__")
                    && matches!(
                        idunder,
                        "__iadd__" | "__imul__" | "__iand__" | "__ior__" | "__isub__" | "__ixor__"
                    )
                {
                    if !PyObjectRef::ptr_eq(&result, a)
                        && matches!(
                            &result.payload,
                            PyObjectPayload::List(_)
                                | PyObjectPayload::Set(_)
                                | PyObjectPayload::FrozenSet(_)
                                | PyObjectPayload::Tuple(_)
                                | PyObjectPayload::Str(_)
                        )
                    {
                        inst.attrs
                            .write()
                            .insert(intern_or_new("__builtin_value__"), result);
                    }
                    return Ok(Some(a.clone()));
                }
                return Ok(Some(result));
            }
            if matches!(idunder, "__iand__" | "__ior__" | "__isub__" | "__ixor__") {
                if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                    let result = match idunder {
                        "__iand__" => bv.bit_and(b),
                        "__ior__" => bv.bit_or(b),
                        "__isub__" => bv.sub(b),
                        "__ixor__" => bv.bit_xor(b),
                        _ => unreachable!(),
                    }?;
                    inst.attrs
                        .write()
                        .insert(intern_or_new("__builtin_value__"), result);
                    return Ok(Some(a.clone()));
                }
            }
        }
        Ok(None)
    }

    /// Create a bound method from an instance receiver and an unbound method.
    pub(crate) fn bind_method(&self, receiver: &PyObjectRef, method: PyObjectRef) -> PyObjectRef {
        match &method.payload {
            PyObjectPayload::BoundMethod { .. } => method,
            _ => PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: receiver.clone(),
                    method,
                },
            }),
        }
    }

    pub(crate) fn exec_binary_ops(
        &mut self,
        instr: Instruction,
    ) -> Result<Option<PyObjectRef>, PyException> {
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
                if let (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) = (&a.payload, &b.payload)
                {
                    let mut s = x.to_compact_string();
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
            // Fast paths for bitwise/shift/power on small ints
            Opcode::BinaryAnd | Opcode::InplaceAnd => {
                if let (
                    PyObjectPayload::Int(PyInt::Small(x)),
                    PyObjectPayload::Int(PyInt::Small(y)),
                ) = (&a.payload, &b.payload)
                {
                    self.vm_push(PyObject::int(*x & *y));
                    return Ok(None);
                }
            }
            Opcode::BinaryOr | Opcode::InplaceOr => {
                if let (
                    PyObjectPayload::Int(PyInt::Small(x)),
                    PyObjectPayload::Int(PyInt::Small(y)),
                ) = (&a.payload, &b.payload)
                {
                    self.vm_push(PyObject::int(*x | *y));
                    return Ok(None);
                }
            }
            Opcode::BinaryXor | Opcode::InplaceXor => {
                if let (
                    PyObjectPayload::Int(PyInt::Small(x)),
                    PyObjectPayload::Int(PyInt::Small(y)),
                ) = (&a.payload, &b.payload)
                {
                    self.vm_push(PyObject::int(*x ^ *y));
                    return Ok(None);
                }
            }
            Opcode::BinaryLshift | Opcode::InplaceLshift => {
                if let (
                    PyObjectPayload::Int(PyInt::Small(x)),
                    PyObjectPayload::Int(PyInt::Small(y)),
                ) = (&a.payload, &b.payload)
                {
                    if let Some(r) = PyInt::checked_small_lshift(*x, *y) {
                        self.vm_push(PyObject::int(r));
                        return Ok(None);
                    }
                }
            }
            Opcode::BinaryRshift | Opcode::InplaceRshift => {
                if let (
                    PyObjectPayload::Int(PyInt::Small(x)),
                    PyObjectPayload::Int(PyInt::Small(y)),
                ) = (&a.payload, &b.payload)
                {
                    if *y >= 0 && *y < 64 {
                        self.vm_push(PyObject::int(*x >> *y as u32));
                        return Ok(None);
                    }
                }
            }
            Opcode::BinaryPower | Opcode::InplacePower => {
                if let (
                    PyObjectPayload::Int(PyInt::Small(x)),
                    PyObjectPayload::Int(PyInt::Small(y)),
                ) = (&a.payload, &b.payload)
                {
                    if *y >= 0 && *y <= 63 {
                        let mut r: i64 = 1;
                        let mut overflow = false;
                        for _ in 0..*y {
                            match r.checked_mul(*x) {
                                Some(v) => r = v,
                                None => {
                                    overflow = true;
                                    break;
                                }
                            }
                        }
                        if !overflow {
                            self.vm_push(PyObject::int(r));
                            return Ok(None);
                        }
                    }
                }
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
                        if !PyObjectRef::ptr_eq(&ua, &$a) || !PyObjectRef::ptr_eq(&ub, &$b) {
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
                if let Some(r) = self.try_binary_dunder(&a, &b, "__add__", Some("__radd__"))? {
                    r
                } else {
                    with_enum_fallback!(a, b, add)
                }
            }
            Opcode::InplaceAdd => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__iadd__", "__add__")? {
                    r
                } else if let PyObjectPayload::List(items) = &a.payload {
                    // list += iterable → extend in-place (same identity)
                    let new_items = b.to_list()?;
                    items.write().extend(new_items);
                    a.clone()
                } else if let PyObjectPayload::Set(set) = &a.payload {
                    // set |= iterable → update in-place
                    if let PyObjectPayload::Set(other) = &b.payload {
                        let other_items: Vec<_> = other
                            .read()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        set.write().extend(other_items);
                    }
                    a.clone()
                } else {
                    with_enum_fallback!(a, b, add)
                }
            }
            Opcode::BinarySubtract => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__sub__", Some("__rsub__"))? {
                    r
                } else {
                    with_enum_fallback!(a, b, sub)
                }
            }
            Opcode::InplaceSubtract => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__isub__", "__sub__")? {
                    r
                } else if let (PyObjectPayload::Set(set), PyObjectPayload::Set(other)) =
                    (&a.payload, &b.payload)
                {
                    let keys_to_remove: Vec<_> = other.read().keys().cloned().collect();
                    let mut w = set.write();
                    for k in keys_to_remove {
                        w.remove(&k);
                    }
                    drop(w);
                    a.clone()
                } else {
                    with_enum_fallback!(a, b, sub)
                }
            }
            Opcode::BinaryMultiply => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__mul__", Some("__rmul__"))? {
                    r
                } else {
                    with_enum_fallback!(a, b, mul)
                }
            }
            Opcode::InplaceMultiply => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__imul__", "__mul__")? {
                    r
                } else if let (PyObjectPayload::List(items), PyObjectPayload::Int(n)) =
                    (&a.payload, &b.payload)
                {
                    // list *= n → repeat in-place
                    let count = index_to_usize_repeat(&n.to_object())?;
                    let mut w = items.write();
                    let orig: Vec<_> = w.clone();
                    checked_repeat_len(orig.len(), count, "list repeat")?;
                    w.clear();
                    for _ in 0..count {
                        w.extend_from_slice(&orig);
                    }
                    drop(w);
                    a.clone()
                } else {
                    with_enum_fallback!(a, b, mul)
                }
            }
            Opcode::BinaryTrueDivide => {
                if let Some(r) =
                    self.try_binary_dunder(&a, &b, "__truediv__", Some("__rtruediv__"))?
                {
                    r
                } else {
                    with_enum_fallback!(a, b, true_div)
                }
            }
            Opcode::InplaceTrueDivide => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__itruediv__", "__truediv__")? {
                    r
                } else {
                    with_enum_fallback!(a, b, true_div)
                }
            }
            Opcode::BinaryFloorDivide => {
                if let Some(r) =
                    self.try_binary_dunder(&a, &b, "__floordiv__", Some("__rfloordiv__"))?
                {
                    r
                } else {
                    with_enum_fallback!(a, b, floor_div)
                }
            }
            Opcode::InplaceFloorDivide => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ifloordiv__", "__floordiv__")? {
                    r
                } else {
                    with_enum_fallback!(a, b, floor_div)
                }
            }
            Opcode::BinaryModulo => {
                // str % val → Python printf-style formatting
                if let PyObjectPayload::Str(fmt_str) = &a.payload {
                    self.vm_string_percent_format(fmt_str, &b)?
                } else if let PyObjectPayload::Bytes(fmt_bytes) = &a.payload {
                    self.vm_bytes_percent_format(fmt_bytes, &b)?
                } else if let Some(r) =
                    self.try_binary_dunder(&a, &b, "__mod__", Some("__rmod__"))?
                {
                    r
                } else {
                    with_enum_fallback!(a, b, modulo)
                }
            }
            Opcode::InplaceModulo => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__imod__", "__mod__")? {
                    r
                } else if let PyObjectPayload::Str(fmt_str) = &a.payload {
                    self.vm_string_percent_format(fmt_str, &b)?
                } else if let PyObjectPayload::Bytes(fmt_bytes) = &a.payload {
                    self.vm_bytes_percent_format(fmt_bytes, &b)?
                } else {
                    with_enum_fallback!(a, b, modulo)
                }
            }
            Opcode::BinaryPower => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__pow__", Some("__rpow__"))? {
                    r
                } else {
                    with_enum_fallback!(a, b, power)
                }
            }
            Opcode::InplacePower => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ipow__", "__pow__")? {
                    r
                } else {
                    with_enum_fallback!(a, b, power)
                }
            }
            Opcode::BinaryLshift => {
                if let Some(r) =
                    self.try_binary_dunder(&a, &b, "__lshift__", Some("__rlshift__"))?
                {
                    r
                } else {
                    with_enum_fallback!(a, b, lshift)
                }
            }
            Opcode::InplaceLshift => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ilshift__", "__lshift__")? {
                    r
                } else {
                    with_enum_fallback!(a, b, lshift)
                }
            }
            Opcode::BinaryRshift => {
                if let Some(r) =
                    self.try_binary_dunder(&a, &b, "__rshift__", Some("__rrshift__"))?
                {
                    r
                } else if Self::is_builtin_print_function(&a) {
                    return Err(Self::py2_print_redirection_type_error(&a, &b));
                } else {
                    with_enum_fallback!(a, b, rshift)
                }
            }
            Opcode::InplaceRshift => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__irshift__", "__rshift__")? {
                    r
                } else {
                    with_enum_fallback!(a, b, rshift)
                }
            }
            Opcode::BinaryAnd => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__and__", Some("__rand__"))? {
                    r
                } else {
                    with_enum_fallback!(a, b, bit_and)
                }
            }
            Opcode::InplaceAnd => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__iand__", "__and__")? {
                    r
                } else if let (PyObjectPayload::Set(set), PyObjectPayload::Set(other)) =
                    (&a.payload, &b.payload)
                {
                    let other_keys: indexmap::IndexSet<_> = other.read().keys().cloned().collect();
                    set.write().retain(|k, _| other_keys.contains(k));
                    a.clone()
                } else {
                    with_enum_fallback!(a, b, bit_and)
                }
            }
            Opcode::BinaryOr => {
                // PEP 604: type | type → UnionType for isinstance checks
                if Self::is_type_like(&a) && Self::is_type_like(&b) {
                    self.make_union_type(&a, &b)?
                } else if let Some(r) = self.try_binary_dunder(&a, &b, "__or__", Some("__ror__"))? {
                    r
                } else if let (PyObjectPayload::Dict(_), PyObjectPayload::Dict(_)) =
                    (&a.payload, &b.payload)
                {
                    // Delegate to py_bit_or which handles Counter union (max) vs regular dict merge
                    a.bit_or(&b)?
                } else {
                    with_enum_fallback!(a, b, bit_or)
                }
            }
            Opcode::InplaceOr => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ior__", "__or__")? {
                    r
                } else if let (PyObjectPayload::Dict(_), PyObjectPayload::Dict(_)) =
                    (&a.payload, &b.payload)
                {
                    a.bit_or(&b)?
                } else if let (PyObjectPayload::Set(set), PyObjectPayload::Set(other)) =
                    (&a.payload, &b.payload)
                {
                    let items: Vec<_> = other
                        .read()
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    set.write().extend(items);
                    a.clone()
                } else {
                    with_enum_fallback!(a, b, bit_or)
                }
            }
            Opcode::BinaryXor => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__xor__", Some("__rxor__"))? {
                    r
                } else {
                    with_enum_fallback!(a, b, bit_xor)
                }
            }
            Opcode::InplaceXor => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__ixor__", "__xor__")? {
                    r
                } else if let (PyObjectPayload::Set(set), PyObjectPayload::Set(other)) =
                    (&a.payload, &b.payload)
                {
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
                } else {
                    with_enum_fallback!(a, b, bit_xor)
                }
            }
            Opcode::BinaryMatrixMultiply => {
                if let Some(r) =
                    self.try_binary_dunder(&a, &b, "__matmul__", Some("__rmatmul__"))?
                {
                    r
                } else {
                    return Err(PyException::type_error(format!(
                        "unsupported operand type(s) for @: '{}' and '{}'",
                        a.type_name(),
                        b.type_name()
                    )));
                }
            }
            Opcode::InplaceMatrixMultiply => {
                if let Some(r) = self.try_inplace_dunder(&a, &b, "__imatmul__", "__matmul__")? {
                    r
                } else {
                    return Err(PyException::type_error(format!(
                        "unsupported operand type(s) for @=: '{}' and '{}'",
                        a.type_name(),
                        b.type_name()
                    )));
                }
            }
            // LoadFastLoadConstBinarySub fallback: operands already on stack, treat as subtract
            Opcode::LoadFastLoadConstBinarySub => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__sub__", Some("__rsub__"))? {
                    r
                } else {
                    with_enum_fallback!(a, b, sub)
                }
            }
            // LoadFastLoadConstBinaryAdd fallback: operands already on stack, treat as add
            Opcode::LoadFastLoadConstBinaryAdd
            | Opcode::LoadFastLoadFastBinaryAdd
            | Opcode::LoadFastLoadFastBinaryAddStoreFast
            | Opcode::LoadFastLoadConstBinaryAddStoreFast => {
                if let Some(r) = self.try_binary_dunder(&a, &b, "__add__", Some("__radd__"))? {
                    r
                } else {
                    with_enum_fallback!(a, b, add)
                }
            }
            _ => unreachable!(),
        };
        self.vm_push(result);
        Ok(None)
    }
}

impl VirtualMachine {
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
        let repr = args
            .iter()
            .map(|a| match &a.payload {
                PyObjectPayload::BuiltinType(bt) => bt.as_str().to_string(),
                PyObjectPayload::Class(cls) => cls.name.to_string(),
                PyObjectPayload::None => "None".to_string(),
                _ => a.type_name().to_string(),
            })
            .collect::<Vec<_>>()
            .join(" | ");
        let args_tuple = PyObject::tuple(args);
        let union_cls = PyObject::class(
            CompactString::from("types.UnionType"),
            vec![],
            IndexMap::new(),
        );
        let mut attrs = IndexMap::new();
        attrs.insert(intern_or_new("__args__"), args_tuple);
        attrs.insert(
            intern_or_new("__typing_repr__"),
            PyObject::str_val(CompactString::from(&repr)),
        );
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
