use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    index_to_i64, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── Group 6: Subscript operations ────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_subscript_ops(
        &mut self,
        instr: Instruction,
    ) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::BinarySubscr => {
                let key = self.vm_pop();
                let obj = self.vm_pop();
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                        if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                            let referent = (nc.func)(&[])?;
                            self.vm_push(referent);
                            self.vm_push(key);
                            return self.exec_subscript_ops(instr);
                        }
                    }
                }
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
                            PyObjectPayload::Tuple(items) => items
                                .iter()
                                .map(|i| i.type_name().to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                            _ => key.type_name().to_string(),
                        };
                        let repr = format!("{}[{}]", name, params);
                        let alias_cls = PyObject::class(
                            CompactString::from("_GenericAlias"),
                            vec![],
                            IndexMap::new(),
                        );
                        let mut attrs = IndexMap::new();
                        attrs.insert(
                            intern_or_new("__typing_repr__"),
                            PyObject::str_val(CompactString::from(&repr)),
                        );
                        attrs.insert(
                            intern_or_new("__str__"),
                            PyObject::str_val(CompactString::from(&repr)),
                        );
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
                        PyObjectPayload::Tuple(items) => items
                            .iter()
                            .map(|i| self.format_type_param(i))
                            .collect::<Vec<_>>()
                            .join(", "),
                        _ => self.format_type_param(&key),
                    };
                    let repr = format!("{}[{}]", type_name, params);
                    let alias_cls = PyObject::class(
                        CompactString::from("types.GenericAlias"),
                        vec![],
                        IndexMap::new(),
                    );
                    let mut attrs = IndexMap::new();
                    attrs.insert(intern_or_new("__origin__"), obj.clone());
                    attrs.insert(intern_or_new("__args__"), key.clone());
                    attrs.insert(
                        intern_or_new("__typing_repr__"),
                        PyObject::str_val(CompactString::from(&repr)),
                    );
                    let alias = PyObject::instance_with_attrs(alias_cls, attrs);
                    self.vm_push(alias);
                    return Ok(None);
                }
                // Dict subclass: Instance with dict_storage
                // Typing aliases: _GenericAlias[X] or types.GenericAlias[X] → new alias
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        if cd.name.contains("GenericAlias") || cd.name.contains("_GenericAlias") {
                            let base_repr = inst
                                .attrs
                                .read()
                                .get("__typing_repr__")
                                .map(|r| r.py_to_string())
                                .unwrap_or_else(|| obj.py_to_string());
                            let params = match &key.payload {
                                PyObjectPayload::Tuple(items) => items
                                    .iter()
                                    .map(|i| self.format_type_param(i))
                                    .collect::<Vec<_>>()
                                    .join(", "),
                                _ => self.format_type_param(&key),
                            };
                            let repr = format!("{}[{}]", base_repr, params);
                            let alias_cls = PyObject::class(
                                CompactString::from("_GenericAlias"),
                                vec![],
                                IndexMap::new(),
                            );
                            let mut attrs = IndexMap::new();
                            attrs.insert(
                                intern_or_new("__typing_repr__"),
                                PyObject::str_val(CompactString::from(&repr)),
                            );
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
                    if inst.attrs.read().contains_key("__deque__") {
                        if let Some(data) = inst.attrs.read().get("_data").cloned() {
                            self.vm_push(data.get_item(&key)?);
                            return Ok(None);
                        }
                        return Err(PyException::index_error("deque index out of range"));
                    }
                    let is_chainmap = inst.attrs.read().contains_key("__chainmap__");
                    if is_chainmap {
                        if let Some(maps_obj) = obj.get_attr("maps") {
                            let maps = maps_obj.to_list()?;
                            for mapping in maps {
                                match mapping.get_item(&key) {
                                    Ok(val) => {
                                        self.vm_push(val);
                                        return Ok(None);
                                    }
                                    Err(e) if e.kind == ExceptionKind::KeyError => continue,
                                    Err(e) => return Err(e),
                                }
                            }
                            if let Some(missing) = obj.get_attr("__missing__") {
                                let result = self.call_object(missing, vec![key])?;
                                self.vm_push(result);
                            } else {
                                return Err(PyException::key_error(key.py_to_string()));
                            }
                            return Ok(None);
                        }
                    }
                    if let Some(ref ds) = inst.dict_storage {
                        let has_user_getitem =
                            Self::class_has_user_override(&inst.class, "__getitem__");
                        if has_user_getitem || is_chainmap {
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
                        let factory_key =
                            HashableKey::str_key(intern_or_new("__defaultdict_factory__"));
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
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                        if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                            let referent = (nc.func)(&[])?;
                            self.vm_push(value);
                            self.vm_push(referent);
                            self.vm_push(key);
                            return self.exec_subscript_ops(instr);
                        }
                    }
                }
                match &obj.payload {
                    PyObjectPayload::List(items) => {
                        if let PyObjectPayload::Slice(sd) = &key.payload {
                            let step_val = sd
                                .step
                                .as_ref()
                                .map(|v| v.as_int().unwrap_or(1))
                                .unwrap_or(1);
                            let new_items = value.to_list()?;
                            let mut w = items.write();
                            let len = w.len() as i64;

                            if step_val == 1 || step_val == 0 {
                                // Contiguous slice assignment: a[s:e] = items
                                let s_val = sd
                                    .start
                                    .as_ref()
                                    .map(|v| v.as_int().unwrap_or(0))
                                    .unwrap_or(0);
                                let e_val = sd
                                    .stop
                                    .as_ref()
                                    .map(|v| v.as_int().unwrap_or(len))
                                    .unwrap_or(len);
                                let s = (if s_val < 0 {
                                    (len + s_val).max(0)
                                } else {
                                    s_val.min(len)
                                }) as usize;
                                let e = (if e_val < 0 {
                                    (len + e_val).max(0)
                                } else {
                                    e_val.min(len)
                                }) as usize;
                                let e = e.max(s);
                                w.splice(s..e, new_items);
                            } else {
                                // Extended slice assignment: a[s:e:step] = items
                                let s_val = if step_val > 0 {
                                    sd.start
                                        .as_ref()
                                        .map(|v| v.as_int().unwrap_or(0))
                                        .unwrap_or(0)
                                } else {
                                    sd.start
                                        .as_ref()
                                        .map(|v| v.as_int().unwrap_or(len - 1))
                                        .unwrap_or(len - 1)
                                };
                                let e_val = if step_val > 0 {
                                    sd.stop
                                        .as_ref()
                                        .map(|v| v.as_int().unwrap_or(len))
                                        .unwrap_or(len)
                                } else {
                                    sd.stop
                                        .as_ref()
                                        .map(|v| v.as_int().unwrap_or(-len - 1))
                                        .unwrap_or(-len - 1)
                                };
                                // Collect indices
                                let mut indices = Vec::new();
                                let mut i = if s_val < 0 {
                                    (len + s_val).max(0)
                                } else {
                                    s_val.min(len)
                                };
                                let end = if e_val < 0 {
                                    (len + e_val).max(-1)
                                } else {
                                    e_val.min(len)
                                };
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
                            let idx = index_to_i64(&key)?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error(
                                    "list assignment index out of range",
                                ));
                            }
                            w[actual as usize] = value;
                        }
                    }
                    PyObjectPayload::Dict(map) => {
                        let hk = self.vm_to_hashable_key(&key)?;
                        map.write().insert(hk, value);
                    }
                    PyObjectPayload::ByteArray(ref bytes) => {
                        if let PyObjectPayload::Slice(sd) = &key.payload {
                            // Slice assignment on bytearray
                            let len = bytes.len() as i64;
                            let step_val = sd
                                .step
                                .as_ref()
                                .map(|v| v.as_int().unwrap_or(1))
                                .unwrap_or(1);
                            let new_bytes: Vec<u8> = if let PyObjectPayload::Bytes(b)
                            | PyObjectPayload::ByteArray(b) =
                                &value.payload
                            {
                                (**b).clone()
                            } else if let Some(n) = value.as_int() {
                                vec![0u8; n.max(0) as usize]
                            } else {
                                // Try to collect as list of ints
                                let items = value.to_list()?;
                                items
                                    .iter()
                                    .map(|v| v.to_int().unwrap_or(0) as u8)
                                    .collect()
                            };

                            if step_val == 1 || step_val == 0 {
                                let s_val = sd
                                    .start
                                    .as_ref()
                                    .map(|v| v.as_int().unwrap_or(0))
                                    .unwrap_or(0);
                                let e_val = sd
                                    .stop
                                    .as_ref()
                                    .map(|v| v.as_int().unwrap_or(len))
                                    .unwrap_or(len);
                                let s = (if s_val < 0 {
                                    (len + s_val).max(0)
                                } else {
                                    s_val.min(len)
                                }) as usize;
                                let e = (if e_val < 0 {
                                    (len + e_val).max(0)
                                } else {
                                    e_val.min(len)
                                }) as usize;
                                let e = e.max(s);
                                unsafe {
                                    let vec_ptr = &obj.payload as *const PyObjectPayload;
                                    if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                                        let vp = &**v as *const Vec<u8> as *mut Vec<u8>;
                                        (*vp).splice(s..e, new_bytes);
                                    }
                                }
                            } else {
                                // Extended slice: collect indices
                                let mut indices = Vec::new();
                                let s_val = if step_val > 0 {
                                    sd.start
                                        .as_ref()
                                        .map(|v| v.as_int().unwrap_or(0))
                                        .unwrap_or(0)
                                } else {
                                    sd.start
                                        .as_ref()
                                        .map(|v| v.as_int().unwrap_or(len - 1))
                                        .unwrap_or(len - 1)
                                };
                                let e_val = if step_val > 0 {
                                    sd.stop
                                        .as_ref()
                                        .map(|v| v.as_int().unwrap_or(len))
                                        .unwrap_or(len)
                                } else {
                                    sd.stop
                                        .as_ref()
                                        .map(|v| v.as_int().unwrap_or(-len - 1))
                                        .unwrap_or(-len - 1)
                                };
                                let mut i = if s_val < 0 {
                                    (len + s_val).max(0)
                                } else {
                                    s_val.min(len)
                                };
                                let end = if e_val < 0 {
                                    (len + e_val).max(-1)
                                } else {
                                    e_val.min(len)
                                };
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
                            let idx = index_to_i64(&key)?;
                            let byte_val = value.to_int()? as u8;
                            let len = bytes.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error(
                                    "bytearray index out of range",
                                ));
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
                            let has_user_setitem =
                                Self::class_has_user_override(&inst.class, "__setitem__");
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
                                "'{}' object does not support item assignment",
                                obj.type_name()
                            )));
                        }
                    }
                    PyObjectPayload::Module(ref md) => {
                        // Module with __setitem__ (e.g., os.environ)
                        let setitem = md.attrs.read().get("__setitem__").cloned();
                        if let Some(m) = setitem {
                            self.call_object(m, vec![key, value])?;
                        } else {
                            return Err(PyException::type_error(format!(
                                "'{}' object does not support item assignment",
                                obj.type_name()
                            )));
                        }
                    }
                    _ => {
                        return Err(PyException::type_error(format!(
                            "'{}' object does not support item assignment",
                            obj.type_name()
                        )))
                    }
                }
            }
            Opcode::DeleteSubscr => {
                let key = self.vm_pop();
                let obj = self.vm_pop();
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                        if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                            let referent = (nc.func)(&[])?;
                            self.vm_push(referent);
                            self.vm_push(key);
                            return self.exec_subscript_ops(instr);
                        }
                    }
                }
                match &obj.payload {
                    PyObjectPayload::List(items) => {
                        if let PyObjectPayload::Slice(sd) = &key.payload {
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let s = sd
                                .start
                                .as_ref()
                                .map(|v| v.to_int().unwrap_or(0))
                                .unwrap_or(0);
                            let e = sd
                                .stop
                                .as_ref()
                                .map(|v| v.to_int().unwrap_or(len))
                                .unwrap_or(len);
                            let st = sd
                                .step
                                .as_ref()
                                .map(|v| v.to_int().unwrap_or(1))
                                .unwrap_or(1);
                            let s = if s < 0 { (len + s).max(0) } else { s.min(len) };
                            let e = if e < 0 { (len + e).max(0) } else { e.min(len) };
                            if st == 1 && s <= e {
                                w.drain(s as usize..e as usize);
                            } else if st == -1 && s >= e {
                                let mut indices: Vec<usize> =
                                    ((e + 1) as usize..=(s) as usize).collect();
                                indices.reverse();
                                for idx in indices {
                                    if idx < w.len() {
                                        w.remove(idx);
                                    }
                                }
                            } else if st > 1 {
                                let mut indices = Vec::new();
                                let mut i = s;
                                while i < e {
                                    indices.push(i as usize);
                                    i += st;
                                }
                                indices.reverse();
                                for idx in indices {
                                    if idx < w.len() {
                                        w.remove(idx);
                                    }
                                }
                            }
                        } else {
                            let idx = index_to_i64(&key)?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error(
                                    "list assignment index out of range",
                                ));
                            }
                            w.remove(actual as usize);
                        }
                    }
                    PyObjectPayload::Dict(map) => {
                        let hk = self.vm_to_hashable_key(&key)?;
                        if map.write().swap_remove(&hk).is_none() {
                            return Err(PyException::key_error(key.repr()));
                        }
                    }
                    PyObjectPayload::InstanceDict(attrs) => {
                        let key_str = CompactString::from(key.py_to_string());
                        if attrs.write().swap_remove(&key_str).is_none() {
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
                            if ds.write().swap_remove(&hk).is_none() {
                                return Err(PyException::key_error(key.repr()));
                            }
                            return Ok(None);
                        }
                        return Err(PyException::type_error(format!(
                            "'{}'  object does not support item deletion",
                            obj.type_name()
                        )));
                    }
                    PyObjectPayload::Module(ref md) => {
                        let delitem = md.attrs.read().get("__delitem__").cloned();
                        if let Some(m) = delitem {
                            self.call_object(m, vec![key])?;
                        } else {
                            return Err(PyException::type_error(format!(
                                "'{}' object does not support item deletion",
                                obj.type_name()
                            )));
                        }
                    }
                    _ => {
                        return Err(PyException::type_error(format!(
                            "'{}' object does not support item deletion",
                            obj.type_name()
                        )))
                    }
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}
