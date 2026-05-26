use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::collections::HashMap;

use super::shared::{
    exception_pickle_state, format_float_repr, hashable_key_to_pyobj, operator_reduce_target,
};

fn pickle_serialize_operator_reduce_p0(
    func_name: &str,
    args: &[PyObjectRef],
    buf: &mut Vec<u8>,
    memo: &mut PickleWriteMemo,
) -> PyResult<()> {
    buf.extend_from_slice(b"coperator\n");
    buf.extend_from_slice(func_name.as_bytes());
    buf.extend_from_slice(b"\n(");
    for arg in args {
        pickle_serialize_p0(arg, buf, memo)?;
    }
    buf.extend_from_slice(b"tR");
    Ok(())
}

fn pickle_serialize_operator_reduce_p2(
    func_name: &str,
    args: &[PyObjectRef],
    buf: &mut Vec<u8>,
    memo: &mut PickleWriteMemo,
) -> PyResult<()> {
    buf.extend_from_slice(b"coperator\n");
    buf.extend_from_slice(func_name.as_bytes());
    buf.extend_from_slice(b"\n");
    pickle_serialize_p2(&PyObject::tuple(args.to_vec()), buf, memo)?;
    buf.push(b'R');
    Ok(())
}

// ── Protocol 0 (text) serialization ──

#[derive(Default)]
pub(super) struct PickleWriteMemo {
    next: u32,
    seen: HashMap<usize, u32>,
}

impl std::ops::Deref for PickleWriteMemo {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        &self.next
    }
}

impl std::ops::DerefMut for PickleWriteMemo {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.next
    }
}

fn pickle_identity_key(obj: &PyObjectRef) -> Option<usize> {
    match &obj.payload {
        PyObjectPayload::List(_)
        | PyObjectPayload::Dict(_)
        | PyObjectPayload::MappingProxy(_)
        | PyObjectPayload::Instance(_) => Some(PyObjectRef::as_ptr(obj) as usize),
        _ => None,
    }
}

fn p0_emit_get(buf: &mut Vec<u8>, id: u32) {
    buf.push(b'g');
    buf.extend_from_slice(format!("{}\n", id).as_bytes());
}

fn p0_try_emit_get(obj: &PyObjectRef, buf: &mut Vec<u8>, memo: &PickleWriteMemo) -> bool {
    let Some(key) = pickle_identity_key(obj) else {
        return false;
    };
    let Some(id) = memo.seen.get(&key) else {
        return false;
    };
    p0_emit_get(buf, *id);
    true
}

fn p0_emit_put_obj(obj: &PyObjectRef, buf: &mut Vec<u8>, memo: &mut PickleWriteMemo) {
    let id = **memo;
    **memo += 1;
    if let Some(key) = pickle_identity_key(obj) {
        memo.seen.insert(key, id);
    }
    buf.push(b'p');
    buf.extend_from_slice(format!("{}\n", id).as_bytes());
}

fn p2_emit_get(buf: &mut Vec<u8>, id: u32) {
    if id <= 0xff {
        buf.push(b'h');
        buf.push(id as u8);
    } else {
        buf.push(b'j');
        buf.extend_from_slice(&id.to_le_bytes());
    }
}

fn p2_try_emit_get(obj: &PyObjectRef, buf: &mut Vec<u8>, memo: &PickleWriteMemo) -> bool {
    let Some(key) = pickle_identity_key(obj) else {
        return false;
    };
    let Some(id) = memo.seen.get(&key) else {
        return false;
    };
    p2_emit_get(buf, *id);
    true
}

fn p2_emit_put_obj(obj: &PyObjectRef, buf: &mut Vec<u8>, memo: &mut PickleWriteMemo) {
    let id = **memo;
    **memo += 1;
    if let Some(key) = pickle_identity_key(obj) {
        memo.seen.insert(key, id);
    }
    if id <= 0xff {
        buf.push(b'q');
        buf.push(id as u8);
    } else {
        buf.push(b'r');
        buf.extend_from_slice(&id.to_le_bytes());
    }
}

fn p0_escape_unicode(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            '\0' => out.extend_from_slice(b"\\x00"),
            c if c.is_ascii() => out.push(c as u8),
            c if (c as u32) <= 0xff => {
                out.extend_from_slice(format!("\\x{:02x}", c as u32).as_bytes());
            }
            c if (c as u32) <= 0xffff => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes());
            }
            c => {
                out.extend_from_slice(format!("\\U{:08x}", c as u32).as_bytes());
            }
        }
    }
    out
}

fn p0_escape_bytes(b: &[u8]) -> Vec<u8> {
    let mut out = vec![b'\''];
    for &byte in b {
        match byte {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\'' => out.extend_from_slice(b"\\'"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x20..=0x7e => out.push(byte),
            _ => out.extend_from_slice(format!("\\x{:02x}", byte).as_bytes()),
        }
    }
    out.push(b'\'');
    out
}

pub(super) fn pickle_serialize_p0(
    obj: &PyObjectRef,
    buf: &mut Vec<u8>,
    memo: &mut PickleWriteMemo,
) -> PyResult<()> {
    if p0_try_emit_get(obj, buf, memo) {
        return Ok(());
    }
    match &obj.payload {
        PyObjectPayload::None => buf.push(b'N'),
        PyObjectPayload::Bool(b) => {
            buf.extend_from_slice(if *b { b"I01\n" } else { b"I00\n" });
        }
        PyObjectPayload::Int(n) => {
            buf.extend_from_slice(format!("I{}\n", n).as_bytes());
        }
        PyObjectPayload::Float(f) => {
            buf.extend_from_slice(format!("F{}\n", format_float_repr(*f)).as_bytes());
        }
        PyObjectPayload::Str(s) => {
            buf.push(b'V');
            buf.extend_from_slice(&p0_escape_unicode(s));
            buf.push(b'\n');
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            buf.push(b'S');
            buf.extend_from_slice(&p0_escape_bytes(b));
            buf.push(b'\n');
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            buf.extend_from_slice(b"(lp");
            let id = **memo;
            **memo += 1;
            if let Some(key) = pickle_identity_key(obj) {
                memo.seen.insert(key, id);
            }
            buf.extend_from_slice(format!("{}\n", id).as_bytes());
            for item in items.iter() {
                pickle_serialize_p0(item, buf, memo)?;
                buf.push(b'a');
            }
        }
        PyObjectPayload::Tuple(items) => {
            buf.push(b'(');
            for item in items.iter() {
                pickle_serialize_p0(item, buf, memo)?;
            }
            buf.push(b't');
        }
        PyObjectPayload::Dict(map) => {
            let map = map.read();
            buf.extend_from_slice(b"(dp");
            let id = **memo;
            **memo += 1;
            if let Some(key) = pickle_identity_key(obj) {
                memo.seen.insert(key, id);
            }
            buf.extend_from_slice(format!("{}\n", id).as_bytes());
            for (k, v) in map.iter() {
                pickle_serialize_p0(&hashable_key_to_pyobj(k), buf, memo)?;
                pickle_serialize_p0(v, buf, memo)?;
                buf.push(b's');
            }
        }
        PyObjectPayload::MappingProxy(map) => {
            let map = map.read();
            buf.extend_from_slice(b"(dp");
            let id = **memo;
            **memo += 1;
            if let Some(key) = pickle_identity_key(obj) {
                memo.seen.insert(key, id);
            }
            buf.extend_from_slice(format!("{}\n", id).as_bytes());
            for (k, v) in map.iter() {
                pickle_serialize_p0(&hashable_key_to_pyobj(k), buf, memo)?;
                pickle_serialize_p0(v, buf, memo)?;
                buf.push(b's');
            }
        }
        PyObjectPayload::Set(items) => {
            buf.extend_from_slice(b"c__builtin__\nset\n(");
            let items_r = items.read();
            let list_items: Vec<PyObjectRef> = items_r.values().cloned().collect();
            pickle_serialize_p0(&PyObject::list(list_items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::FrozenSet(items) => {
            buf.extend_from_slice(b"c__builtin__\nfrozenset\n(");
            let list_items: Vec<PyObjectRef> = items.values().cloned().collect();
            pickle_serialize_p0(&PyObject::list(list_items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::Instance(inst) => {
            if let Some((factory, items)) = defaultdict_pickle_parts(inst) {
                pickle_serialize_defaultdict_p0(&factory, &items, buf, memo)?;
                return Ok(());
            }
            let (module_name, class_name, data_pairs) = pickle_extract_instance(obj, inst)?;
            // Serialize as dict via GLOBAL + REDUCE pattern:
            // cmodule\nClassName\n( {state_dict} tR
            buf.push(b'c');
            buf.extend_from_slice(module_name.as_bytes());
            buf.push(b'\n');
            buf.extend_from_slice(class_name.as_bytes());
            buf.push(b'\n');
            buf.push(b'(');
            // Build state dict
            buf.extend_from_slice(b"(d");
            let id = **memo;
            **memo += 1;
            buf.extend_from_slice(format!("p{}\n", id).as_bytes());
            for (k, v) in &data_pairs {
                pickle_serialize_p0(&PyObject::str_val(k.clone()), buf, memo)?;
                pickle_serialize_p0(v, buf, memo)?;
                buf.push(b's');
            }
            buf.extend_from_slice(b"tR");
            p0_emit_put_obj(obj, buf, memo);
        }
        PyObjectPayload::RefIter { source, index } => {
            let idx = index.get();
            match &source.payload {
                PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_refiter__\n(");
                    pickle_serialize_p0(source, buf, memo)?;
                    pickle_serialize_p0(&PyObject::int(idx as i64), buf, memo)?;
                    pickle_serialize_p0(&PyObject::bool_val(idx == usize::MAX), buf, memo)?;
                    buf.extend_from_slice(b"tR");
                    return Ok(());
                }
                _ => {}
            }
            let items: Vec<PyObjectRef> = match &source.payload {
                PyObjectPayload::Dict(cell)
                | PyObjectPayload::MappingProxy(cell)
                | PyObjectPayload::DictKeys { map: cell, .. } => cell
                    .read()
                    .iter()
                    .skip(idx)
                    .map(|(k, _)| k.to_object())
                    .collect(),
                _ => {
                    return Err(PyException::runtime_error(format!(
                        "PicklingError: can't pickle object of type {}",
                        obj.type_name()
                    )));
                }
            };
            buf.extend_from_slice(b"cbuiltins\niter\n(");
            pickle_serialize_p0(&PyObject::list(items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::RevRefIter { source, index } => {
            let idx = index.get();
            buf.extend_from_slice(b"cbuiltins\n__ferrython_revrefiter__\n(");
            pickle_serialize_p0(source, buf, memo)?;
            pickle_serialize_p0(&PyObject::int(idx as i64), buf, memo)?;
            pickle_serialize_p0(&PyObject::bool_val(idx == usize::MAX), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            let items: Vec<PyObjectRef> = data.items.iter().skip(idx).cloned().collect();
            buf.extend_from_slice(b"cbuiltins\niter\n(");
            pickle_serialize_p0(&PyObject::list(items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::RangeIter(ri) => {
            let mut items = Vec::new();
            let mut c = ri.current.get();
            if ri.step > 0 {
                while c < ri.stop {
                    items.push(PyObject::int(c));
                    c += ri.step;
                }
            } else if ri.step < 0 {
                while c > ri.stop {
                    items.push(PyObject::int(c));
                    c += ri.step;
                }
            }
            buf.extend_from_slice(b"cbuiltins\niter\n(");
            pickle_serialize_p0(&PyObject::list(items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::Iterator(arc) => {
            use ferrython_core::object::IteratorData;
            let data = arc.read();
            let items: Vec<PyObjectRef> = match &*data {
                IteratorData::List { items, index } => items.iter().skip(*index).cloned().collect(),
                IteratorData::Tuple { items, index } => {
                    items.iter().skip(*index).cloned().collect()
                }
                IteratorData::Str { chars, index } => chars
                    .iter()
                    .skip(*index)
                    .map(|c| PyObject::str_val(CompactString::from(c.to_string())))
                    .collect(),
                IteratorData::Range {
                    current,
                    stop,
                    step,
                } => {
                    let mut items = Vec::new();
                    let mut c = *current;
                    if *step > 0 {
                        while c < *stop {
                            items.push(PyObject::int(c));
                            c += step;
                        }
                    } else if *step < 0 {
                        while c > *stop {
                            items.push(PyObject::int(c));
                            c += step;
                        }
                    }
                    items
                }
                IteratorData::DictKeys { keys, index } => {
                    keys.iter().skip(*index).cloned().collect()
                }
                IteratorData::DictEntries { source, index, .. } => {
                    let map = source.read();
                    map.iter()
                        .skip(*index)
                        .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()]))
                        .collect()
                }
                IteratorData::SeqIter {
                    obj: source,
                    index,
                    exhausted,
                } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_seqiter__\n(");
                    pickle_serialize_p0(source, buf, memo)?;
                    pickle_serialize_p0(&PyObject::int(*index), buf, memo)?;
                    pickle_serialize_p0(&PyObject::bool_val(*exhausted), buf, memo)?;
                    buf.extend_from_slice(b"tR");
                    return Ok(());
                }
                _ => {
                    return Err(PyException::runtime_error(format!(
                        "PicklingError: can't pickle object of type {}",
                        obj.type_name()
                    )));
                }
            };
            buf.extend_from_slice(b"cbuiltins\niter\n(");
            pickle_serialize_p0(&PyObject::list(items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::ExceptionInstance(ei) => {
            let type_name = format!("{}", ei.kind);
            buf.extend_from_slice(b"cbuiltins\n");
            buf.extend_from_slice(type_name.as_bytes());
            buf.push(b'\n');
            buf.push(b'(');
            if ei.args.is_empty() {
                // Use the message as the sole arg
                pickle_serialize_p0(
                    &PyObject::str_val(CompactString::from(ei.message.as_str())),
                    buf,
                    memo,
                )?;
            } else {
                for arg in &ei.args {
                    pickle_serialize_p0(arg, buf, memo)?;
                }
            }
            buf.extend_from_slice(b"tR");
            if let Some(state) = exception_pickle_state(ei) {
                pickle_serialize_p0(&state, buf, memo)?;
                buf.push(b'b');
            }
        }
        PyObjectPayload::NativeClosure(nc) => {
            if let (Some(func_name), Some(args)) = (
                operator_reduce_target(nc.name.as_str()),
                nc.pickle_args.as_ref(),
            ) {
                pickle_serialize_operator_reduce_p0(func_name, args, buf, memo)?;
            } else {
                return Err(PyException::runtime_error(format!(
                    "PicklingError: can't pickle object of type {}",
                    obj.type_name()
                )));
            }
        }
        _ => {
            return Err(PyException::runtime_error(format!(
                "PicklingError: can't pickle object of type {}",
                obj.type_name()
            )));
        }
    }
    Ok(())
}

fn pickle_extract_instance(
    obj: &PyObjectRef,
    inst: &ferrython_core::object::InstanceData,
) -> PyResult<(String, String, Vec<(CompactString, PyObjectRef)>)> {
    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
        cd.name.to_string()
    } else {
        "object".to_string()
    };
    let module_name = inst
        .class
        .get_attr("__module__")
        .map(|module| module.py_to_string())
        .filter(|module| !module.is_empty())
        .unwrap_or_else(|| "__main__".to_string());
    let state_dict = if class_name == "Counter" {
        if let Some(ref ds) = inst.dict_storage {
            let mut map = IndexMap::new();
            for (k, v) in ds.read().iter() {
                if let HashableKey::Str(s) = k {
                    if s.as_str() == "__counter_kwargs__" {
                        continue;
                    }
                }
                map.insert(k.clone(), v.clone());
            }
            Some(PyObject::dict(map))
        } else {
            None
        }
    } else if let Some(getstate) = obj.get_attr("__getstate__") {
        match &getstate.payload {
            PyObjectPayload::NativeFunction(nf) => (nf.func)(&[obj.clone()]).ok(),
            PyObjectPayload::NativeClosure(nc) => (nc.func)(&[obj.clone()]).ok(),
            _ => None,
        }
    } else {
        None
    };
    let is_namedtuple = obj.get_attr("__namedtuple__").is_some();
    let mut data_pairs: Vec<(CompactString, PyObjectRef)> = Vec::new();
    if let Some(state) = &state_dict {
        if let PyObjectPayload::Dict(map) = &state.payload {
            for (k, v) in map.read().iter() {
                if let HashableKey::Str(name) = k {
                    data_pairs.push((name.to_compact_string(), v.clone()));
                }
            }
        }
    } else {
        let attrs_r = inst.attrs.read();
        for (k, v) in attrs_r.iter() {
            match &v.payload {
                PyObjectPayload::NativeFunction(_)
                | PyObjectPayload::NativeClosure(_)
                | PyObjectPayload::Function(_)
                | PyObjectPayload::Class(_) => continue,
                _ => data_pairs.push((k.clone(), v.clone())),
            }
        }
    }
    if is_namedtuple {
        data_pairs.push((
            CompactString::from("__namedtuple__"),
            PyObject::bool_val(true),
        ));
        if let Some(fields) = obj.get_attr("_fields") {
            data_pairs.push((CompactString::from("_fields"), fields));
        }
        if let Some(defaults) = obj.get_attr("_field_defaults") {
            data_pairs.push((CompactString::from("_field_defaults"), defaults));
        }
        if let Some(module) = inst.class.get_attr("__module__") {
            data_pairs.push((CompactString::from("__module__"), module));
        }
    }
    Ok((module_name, class_name, data_pairs))
}

fn defaultdict_pickle_parts(
    inst: &ferrython_core::object::InstanceData,
) -> Option<(PyObjectRef, Vec<(HashableKey, PyObjectRef)>)> {
    let class_name = match &inst.class.payload {
        PyObjectPayload::Class(cd) => cd.name.as_str(),
        _ => return None,
    };
    if class_name != "defaultdict" {
        return None;
    }
    let storage = inst.dict_storage.as_ref()?;
    let factory = inst
        .attrs
        .read()
        .get("default_factory")
        .cloned()
        .unwrap_or_else(PyObject::none);
    let mut items = Vec::new();
    for (k, v) in storage.read().iter() {
        if !ferrython_core::object::is_hidden_dict_key(k) {
            items.push((k.clone(), v.clone()));
        }
    }
    Some((factory, items))
}

fn pickle_serialize_builtin_factory_p0(factory: &PyObjectRef, buf: &mut Vec<u8>) -> PyResult<()> {
    match &factory.payload {
        PyObjectPayload::None => {
            let mut nested_memo = PickleWriteMemo::default();
            pickle_serialize_p0(factory, buf, &mut nested_memo)
        }
        PyObjectPayload::BuiltinType(name)
            if matches!(
                name.as_str(),
                "int" | "float" | "str" | "list" | "dict" | "set" | "tuple" | "bool"
            ) =>
        {
            let marker = PyObject::str_val(CompactString::from(format!(
                "__ferrython_builtin_type__:{}",
                name.as_str()
            )));
            let mut nested_memo = PickleWriteMemo::default();
            pickle_serialize_p0(&marker, buf, &mut nested_memo)
        }
        _ => Err(PyException::runtime_error(format!(
            "PicklingError: can't pickle object of type {}",
            factory.type_name()
        ))),
    }
}

fn pickle_serialize_builtin_factory_p2(factory: &PyObjectRef, buf: &mut Vec<u8>) -> PyResult<()> {
    match &factory.payload {
        PyObjectPayload::None => {
            let mut nested_memo = PickleWriteMemo::default();
            pickle_serialize_p2(factory, buf, &mut nested_memo)
        }
        PyObjectPayload::BuiltinType(name)
            if matches!(
                name.as_str(),
                "int" | "float" | "str" | "list" | "dict" | "set" | "tuple" | "bool"
            ) =>
        {
            let marker = PyObject::str_val(CompactString::from(format!(
                "__ferrython_builtin_type__:{}",
                name.as_str()
            )));
            let mut nested_memo = PickleWriteMemo::default();
            pickle_serialize_p2(&marker, buf, &mut nested_memo)
        }
        _ => Err(PyException::runtime_error(format!(
            "PicklingError: can't pickle object of type {}",
            factory.type_name()
        ))),
    }
}

fn pickle_serialize_defaultdict_p0(
    factory: &PyObjectRef,
    items: &[(HashableKey, PyObjectRef)],
    buf: &mut Vec<u8>,
    memo: &mut PickleWriteMemo,
) -> PyResult<()> {
    buf.extend_from_slice(b"ccollections\ndefaultdict\n(");
    pickle_serialize_builtin_factory_p0(factory, buf)?;
    buf.extend_from_slice(b"(d");
    let id = **memo;
    **memo += 1;
    buf.extend_from_slice(format!("p{}\n", id).as_bytes());
    for (k, v) in items {
        pickle_serialize_p0(&hashable_key_to_pyobj(k), buf, memo)?;
        pickle_serialize_p0(v, buf, memo)?;
        buf.push(b's');
    }
    buf.extend_from_slice(b"tR");
    Ok(())
}

fn pickle_serialize_defaultdict_p2(
    factory: &PyObjectRef,
    items: &[(HashableKey, PyObjectRef)],
    buf: &mut Vec<u8>,
    memo: &mut PickleWriteMemo,
) -> PyResult<()> {
    buf.push(b'c');
    buf.extend_from_slice(b"collections\ndefaultdict\n");
    buf.push(b'(');
    pickle_serialize_builtin_factory_p2(factory, buf)?;
    buf.push(b'}');
    p2_emit_put(buf, memo);
    if !items.is_empty() {
        buf.push(b'(');
        for (k, v) in items {
            pickle_serialize_p2(&hashable_key_to_pyobj(k), buf, memo)?;
            pickle_serialize_p2(v, buf, memo)?;
        }
        buf.push(b'u');
    }
    buf.extend_from_slice(b"tR");
    Ok(())
}

// ── Protocol 2 (binary) serialization ──

fn p2_emit_put(buf: &mut Vec<u8>, memo: &mut PickleWriteMemo) {
    let id = **memo;
    **memo += 1;
    if id <= 0xff {
        buf.push(b'q');
        buf.push(id as u8);
    } else {
        buf.push(b'r');
        buf.extend_from_slice(&id.to_le_bytes());
    }
}

pub(super) fn pickle_serialize_p2(
    obj: &PyObjectRef,
    buf: &mut Vec<u8>,
    memo: &mut PickleWriteMemo,
) -> PyResult<()> {
    if p2_try_emit_get(obj, buf, memo) {
        return Ok(());
    }
    match &obj.payload {
        PyObjectPayload::None => buf.push(b'N'),
        PyObjectPayload::Bool(b) => buf.push(if *b { 0x88 } else { 0x89 }),
        PyObjectPayload::Int(n) => {
            if let Some(val) = n.to_i64() {
                if val >= 0 && val <= 0xff {
                    buf.push(b'K');
                    buf.push(val as u8);
                } else if val >= 0 && val <= 0xffff {
                    buf.push(b'M');
                    buf.extend_from_slice(&(val as u16).to_le_bytes());
                } else if val >= i32::MIN as i64 && val <= i32::MAX as i64 {
                    buf.push(b'J');
                    buf.extend_from_slice(&(val as i32).to_le_bytes());
                } else {
                    buf.extend_from_slice(format!("I{}\n", n).as_bytes());
                }
            } else {
                buf.extend_from_slice(format!("I{}\n", n).as_bytes());
            }
        }
        PyObjectPayload::Float(f) => {
            buf.push(b'G');
            buf.extend_from_slice(&f.to_be_bytes());
        }
        PyObjectPayload::Str(s) => {
            let bytes = s.as_bytes();
            buf.push(b'X');
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            buf.push(b'B');
            buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
            buf.extend_from_slice(b);
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            buf.push(b']');
            p2_emit_put_obj(obj, buf, memo);
            if !items.is_empty() {
                buf.push(b'(');
                for item in items.iter() {
                    pickle_serialize_p2(item, buf, memo)?;
                }
                buf.push(b'e');
            }
        }
        PyObjectPayload::Tuple(items) => match items.len() {
            0 => buf.push(b')'),
            1 => {
                pickle_serialize_p2(&items[0], buf, memo)?;
                buf.push(0x85);
            }
            2 => {
                pickle_serialize_p2(&items[0], buf, memo)?;
                pickle_serialize_p2(&items[1], buf, memo)?;
                buf.push(0x86);
            }
            3 => {
                pickle_serialize_p2(&items[0], buf, memo)?;
                pickle_serialize_p2(&items[1], buf, memo)?;
                pickle_serialize_p2(&items[2], buf, memo)?;
                buf.push(0x87);
            }
            _ => {
                buf.push(b'(');
                for item in items.iter() {
                    pickle_serialize_p2(item, buf, memo)?;
                }
                buf.push(b't');
            }
        },
        PyObjectPayload::Dict(map) => {
            let map = map.read();
            buf.push(b'}');
            p2_emit_put_obj(obj, buf, memo);
            if !map.is_empty() {
                buf.push(b'(');
                for (k, v) in map.iter() {
                    pickle_serialize_p2(&hashable_key_to_pyobj(k), buf, memo)?;
                    pickle_serialize_p2(v, buf, memo)?;
                }
                buf.push(b'u');
            }
        }
        PyObjectPayload::MappingProxy(map) => {
            let map = map.read();
            buf.push(b'}');
            p2_emit_put_obj(obj, buf, memo);
            if !map.is_empty() {
                buf.push(b'(');
                for (k, v) in map.iter() {
                    pickle_serialize_p2(&hashable_key_to_pyobj(k), buf, memo)?;
                    pickle_serialize_p2(v, buf, memo)?;
                }
                buf.push(b'u');
            }
        }
        PyObjectPayload::Set(items) => {
            buf.extend_from_slice(b"c__builtin__\nset\n(");
            let items_r = items.read();
            let list_items: Vec<PyObjectRef> = items_r.values().cloned().collect();
            pickle_serialize_p2(&PyObject::list(list_items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::FrozenSet(items) => {
            buf.extend_from_slice(b"c__builtin__\nfrozenset\n(");
            let list_items: Vec<PyObjectRef> = items.values().cloned().collect();
            pickle_serialize_p2(&PyObject::list(list_items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::Instance(inst) => {
            if let Some((factory, items)) = defaultdict_pickle_parts(inst) {
                pickle_serialize_defaultdict_p2(&factory, &items, buf, memo)?;
                return Ok(());
            }
            let (module_name, class_name, data_pairs) = pickle_extract_instance(obj, inst)?;
            // cmodule\nClassName\n( {state_dict} t R
            buf.push(b'c');
            buf.extend_from_slice(module_name.as_bytes());
            buf.push(b'\n');
            buf.extend_from_slice(class_name.as_bytes());
            buf.push(b'\n');
            buf.push(b'(');
            // Build state dict
            buf.push(b'}');
            p2_emit_put(buf, memo);
            if !data_pairs.is_empty() {
                buf.push(b'(');
                for (k, v) in &data_pairs {
                    pickle_serialize_p2(&PyObject::str_val(k.clone()), buf, memo)?;
                    pickle_serialize_p2(v, buf, memo)?;
                }
                buf.push(b'u');
            }
            buf.extend_from_slice(b"tR");
            p2_emit_put_obj(obj, buf, memo);
        }
        PyObjectPayload::RefIter { source, index } => {
            let idx = index.get();
            match &source.payload {
                PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_refiter__\n");
                    pickle_serialize_p2(
                        &PyObject::tuple(vec![
                            source.clone(),
                            PyObject::int(idx as i64),
                            PyObject::bool_val(idx == usize::MAX),
                        ]),
                        buf,
                        memo,
                    )?;
                    buf.push(b'R');
                    return Ok(());
                }
                _ => {}
            }
            let items: Vec<PyObjectRef> = match &source.payload {
                PyObjectPayload::Dict(cell)
                | PyObjectPayload::MappingProxy(cell)
                | PyObjectPayload::DictKeys { map: cell, .. } => cell
                    .read()
                    .iter()
                    .skip(idx)
                    .map(|(k, _)| k.to_object())
                    .collect(),
                _ => {
                    return Err(PyException::runtime_error(format!(
                        "PicklingError: can't pickle object of type {}",
                        obj.type_name()
                    )))
                }
            };
            buf.extend_from_slice(b"cbuiltins\niter\n(");
            pickle_serialize_p2(&PyObject::list(items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::RevRefIter { source, index } => {
            let idx = index.get();
            buf.extend_from_slice(b"cbuiltins\n__ferrython_revrefiter__\n");
            pickle_serialize_p2(
                &PyObject::tuple(vec![
                    source.clone(),
                    PyObject::int(idx as i64),
                    PyObject::bool_val(idx == usize::MAX),
                ]),
                buf,
                memo,
            )?;
            buf.push(b'R');
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            let items: Vec<PyObjectRef> = data.items.iter().skip(idx).cloned().collect();
            buf.extend_from_slice(b"cbuiltins\niter\n(");
            pickle_serialize_p2(&PyObject::list(items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::RangeIter(ri) => {
            let mut items = Vec::new();
            let mut c = ri.current.get();
            if ri.step > 0 {
                while c < ri.stop {
                    items.push(PyObject::int(c));
                    c += ri.step;
                }
            } else if ri.step < 0 {
                while c > ri.stop {
                    items.push(PyObject::int(c));
                    c += ri.step;
                }
            }
            buf.extend_from_slice(b"cbuiltins\niter\n(");
            pickle_serialize_p2(&PyObject::list(items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::Iterator(arc) => {
            use ferrython_core::object::IteratorData;
            let data = arc.read();
            let items: Vec<PyObjectRef> = match &*data {
                IteratorData::List { items, index } => items.iter().skip(*index).cloned().collect(),
                IteratorData::Tuple { items, index } => {
                    items.iter().skip(*index).cloned().collect()
                }
                IteratorData::Str { chars, index } => chars
                    .iter()
                    .skip(*index)
                    .map(|c| PyObject::str_val(CompactString::from(c.to_string())))
                    .collect(),
                IteratorData::Range {
                    current,
                    stop,
                    step,
                } => {
                    let mut items = Vec::new();
                    let mut c = *current;
                    if *step > 0 {
                        while c < *stop {
                            items.push(PyObject::int(c));
                            c += step;
                        }
                    } else if *step < 0 {
                        while c > *stop {
                            items.push(PyObject::int(c));
                            c += step;
                        }
                    }
                    items
                }
                IteratorData::DictKeys { keys, index } => {
                    keys.iter().skip(*index).cloned().collect()
                }
                IteratorData::DictEntries { source, index, .. } => {
                    let map = source.read();
                    map.iter()
                        .skip(*index)
                        .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()]))
                        .collect()
                }
                IteratorData::SeqIter {
                    obj: source,
                    index,
                    exhausted,
                } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_seqiter__\n");
                    pickle_serialize_p2(
                        &PyObject::tuple(vec![
                            source.clone(),
                            PyObject::int(*index),
                            PyObject::bool_val(*exhausted),
                        ]),
                        buf,
                        memo,
                    )?;
                    buf.push(b'R');
                    return Ok(());
                }
                _ => {
                    return Err(PyException::runtime_error(format!(
                        "PicklingError: can't pickle object of type {}",
                        obj.type_name()
                    )))
                }
            };
            buf.extend_from_slice(b"cbuiltins\niter\n(");
            pickle_serialize_p2(&PyObject::list(items), buf, memo)?;
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::ExceptionInstance(ei) => {
            let type_name = format!("{}", ei.kind);
            buf.extend_from_slice(b"cbuiltins\n");
            buf.extend_from_slice(type_name.as_bytes());
            buf.push(b'\n');
            buf.push(b'(');
            if ei.args.is_empty() {
                pickle_serialize_p2(
                    &PyObject::str_val(CompactString::from(ei.message.as_str())),
                    buf,
                    memo,
                )?;
            } else {
                for arg in &ei.args {
                    pickle_serialize_p2(arg, buf, memo)?;
                }
            }
            buf.extend_from_slice(b"tR");
            if let Some(state) = exception_pickle_state(ei) {
                pickle_serialize_p2(&state, buf, memo)?;
                buf.push(b'b');
            }
        }
        PyObjectPayload::NativeClosure(nc) => {
            if let (Some(func_name), Some(args)) = (
                operator_reduce_target(nc.name.as_str()),
                nc.pickle_args.as_ref(),
            ) {
                pickle_serialize_operator_reduce_p2(func_name, args, buf, memo)?;
            } else {
                return Err(PyException::runtime_error(format!(
                    "PicklingError: can't pickle object of type {}",
                    obj.type_name()
                )));
            }
        }
        _ => {
            return Err(PyException::runtime_error(format!(
                "PicklingError: can't pickle object of type {}",
                obj.type_name()
            )));
        }
    }
    Ok(())
}

// ── Unified serializer (protocol 2 by default, used by shelve) ──

pub(in crate::serial_modules) fn pickle_serialize(
    obj: &PyObjectRef,
    buf: &mut Vec<u8>,
) -> PyResult<()> {
    buf.extend_from_slice(b"\x80\x02");
    let mut memo = PickleWriteMemo::default();
    pickle_serialize_p2(obj, buf, &mut memo)?;
    buf.push(b'.');
    Ok(())
}
