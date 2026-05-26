use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, ExceptionInstanceData, FxAttrMap, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::rc::Rc;

use super::base64_module::extract_bytes;
use crate::collection_modules::{
    create_collections_module, create_operator_module, namedtuple_rebuild_field,
    namedtuple_rebuild_instance,
};
use crate::text_modules::create_re_module;

// ── pickle module (CPython-compatible protocol 0 & 2) ──

// ── Helpers ──

fn hashable_key_to_pyobj(k: &HashableKey) -> PyObjectRef {
    match k {
        HashableKey::Str(s) => PyObject::str_val(s.to_compact_string()),
        HashableKey::Int(n) => PyObject::int(n.to_i64().unwrap_or(0)),
        HashableKey::Float(f) => PyObject::float(f.0),
        HashableKey::Bool(b) => PyObject::bool_val(*b),
        _ => PyObject::str_val(CompactString::from(format!("{:?}", k))),
    }
}

fn format_float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

fn operator_reduce_target(name: &str) -> Option<&'static str> {
    match name {
        "operator.attrgetter" => Some("attrgetter"),
        "operator.itemgetter" => Some("itemgetter"),
        "operator.methodcaller" => Some("methodcaller"),
        _ => None,
    }
}

fn pickle_exception_instance(kind: ExceptionKind, args: Vec<PyObjectRef>) -> PyObjectRef {
    let message = args
        .first()
        .map(|arg| CompactString::from(arg.py_to_string()))
        .unwrap_or_else(|| CompactString::from(""));
    let inst = PyObject::exception_instance_with_args(kind, message, args.clone());

    if kind.is_subclass_of(&ExceptionKind::ImportError) {
        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
            let mut attrs = ei.ensure_attrs().write();
            attrs.insert(CompactString::from("args"), PyObject::tuple(args.clone()));
            attrs.insert(
                CompactString::from("msg"),
                args.first().cloned().unwrap_or_else(PyObject::none),
            );
            attrs.insert(CompactString::from("name"), PyObject::none());
            attrs.insert(CompactString::from("path"), PyObject::none());
        }
    }

    inst
}

fn exception_pickle_state(ei: &ExceptionInstanceData) -> Option<PyObjectRef> {
    if !ei.kind.is_subclass_of(&ExceptionKind::ImportError) {
        return None;
    }

    let attrs = ei.get_attrs()?;
    let attrs = attrs.read();
    let mut pairs = Vec::new();
    for key in ["name", "path"] {
        if let Some(value) = attrs.get(key) {
            if !matches!(value.payload, PyObjectPayload::None) {
                pairs.push((PyObject::str_val(CompactString::from(key)), value.clone()));
            }
        }
    }
    if pairs.is_empty() {
        None
    } else {
        Some(PyObject::dict_from_pairs(pairs))
    }
}

fn pkl_apply_state(obj: &PyObjectRef, state: &PyObjectRef) -> PyResult<()> {
    let PyObjectPayload::Dict(map) = &state.payload else {
        return Ok(());
    };

    for (key, value) in map.read().iter() {
        let HashableKey::Str(name) = key else {
            continue;
        };
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                inst.attrs
                    .write()
                    .insert(name.to_compact_string(), value.clone());
            }
            PyObjectPayload::ExceptionInstance(ei) => {
                ei.ensure_attrs()
                    .write()
                    .insert(name.to_compact_string(), value.clone());
            }
            _ => {}
        }
    }
    Ok(())
}

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
struct PickleWriteMemo {
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

fn pickle_serialize_p0(
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

fn pickle_serialize_p2(
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

pub(super) fn pickle_serialize(obj: &PyObjectRef, buf: &mut Vec<u8>) -> PyResult<()> {
    buf.extend_from_slice(b"\x80\x02");
    let mut memo = PickleWriteMemo::default();
    pickle_serialize_p2(obj, buf, &mut memo)?;
    buf.push(b'.');
    Ok(())
}

// ── Protocol 0 (text) deserialization ──

#[derive(Clone)]
enum PklStackItem {
    Value(PyObjectRef),
    Mark,
    Global(String, String),
}

fn p0_read_line<'a>(data: &'a [u8], pos: &mut usize) -> &'a [u8] {
    let start = *pos;
    while *pos < data.len() && data[*pos] != b'\n' {
        *pos += 1;
    }
    let line = &data[start..*pos];
    if *pos < data.len() {
        *pos += 1;
    }
    line
}

fn p0_unescape_unicode(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => result.push('\\'),
            Some('n') => result.push('\n'),
            Some('r') => result.push('\r'),
            Some('t') => result.push('\t'),
            Some('0') => result.push('\0'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) {
                        result.push(ch);
                    }
                }
            }
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) {
                        result.push(ch);
                    }
                }
            }
            Some('U') => {
                let hex: String = chars.by_ref().take(8).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) {
                        result.push(ch);
                    }
                }
            }
            Some(other) => {
                result.push('\\');
                result.push(other);
            }
            None => result.push('\\'),
        }
    }
    result
}

fn p0_unescape_bytes(raw: &[u8]) -> Vec<u8> {
    if raw.len() < 2 {
        return Vec::new();
    }
    let quote = raw[0];
    if quote != b'\'' && quote != b'"' {
        return raw.to_vec();
    }
    let end = if raw.last() == Some(&quote) {
        raw.len() - 1
    } else {
        raw.len()
    };
    let inner = &raw[1..end];
    let mut result = Vec::new();
    let mut i = 0;
    while i < inner.len() {
        if inner[i] == b'\\' && i + 1 < inner.len() {
            match inner[i + 1] {
                b'\\' => {
                    result.push(b'\\');
                    i += 2;
                }
                b'\'' => {
                    result.push(b'\'');
                    i += 2;
                }
                b'"' => {
                    result.push(b'"');
                    i += 2;
                }
                b'n' => {
                    result.push(b'\n');
                    i += 2;
                }
                b'r' => {
                    result.push(b'\r');
                    i += 2;
                }
                b't' => {
                    result.push(b'\t');
                    i += 2;
                }
                b'x' if i + 3 < inner.len() => {
                    if let Ok(v) = u8::from_str_radix(
                        std::str::from_utf8(&inner[i + 2..i + 4]).unwrap_or("00"),
                        16,
                    ) {
                        result.push(v);
                    }
                    i += 4;
                }
                _ => {
                    result.push(inner[i]);
                    result.push(inner[i + 1]);
                    i += 2;
                }
            }
        } else {
            result.push(inner[i]);
            i += 1;
        }
    }
    result
}

fn pkl_pop_to_mark(stack: &mut Vec<PklStackItem>) -> PyResult<Vec<PyObjectRef>> {
    let mut items = Vec::new();
    loop {
        match stack.pop() {
            Some(PklStackItem::Mark) => break,
            Some(PklStackItem::Value(v)) => items.push(v),
            Some(PklStackItem::Global(..)) => {
                return Err(PyException::runtime_error(
                    "UnpicklingError: unexpected global on stack",
                ));
            }
            None => {
                return Err(PyException::runtime_error(
                    "UnpicklingError: MARK not found on stack",
                ))
            }
        }
    }
    items.reverse();
    Ok(items)
}

fn pkl_stack_top_value(stack: &[PklStackItem]) -> PyResult<PyObjectRef> {
    match stack.last() {
        Some(PklStackItem::Value(v)) => Ok(v.clone()),
        _ => Err(PyException::runtime_error(
            "UnpicklingError: expected value on stack top",
        )),
    }
}

fn ast_empty_fields_node_names(name: &str) -> bool {
    matches!(
        name,
        "Load"
            | "Store"
            | "Del"
            | "And"
            | "Or"
            | "Add"
            | "Sub"
            | "Mult"
            | "MatMult"
            | "Div"
            | "Mod"
            | "Pow"
            | "LShift"
            | "RShift"
            | "BitOr"
            | "BitXor"
            | "BitAnd"
            | "FloorDiv"
            | "Invert"
            | "Not"
            | "UAdd"
            | "USub"
            | "Eq"
            | "NotEq"
            | "Lt"
            | "LtE"
            | "Gt"
            | "GtE"
            | "Is"
            | "IsNot"
            | "In"
            | "NotIn"
            | "Pass"
            | "Break"
            | "Continue"
    )
}

fn maybe_add_ast_empty_fields(name: &str, attrs: &mut IndexMap<CompactString, PyObjectRef>) {
    if ast_empty_fields_node_names(name) && !attrs.contains_key("_fields") {
        attrs.insert(CompactString::from("_fields"), PyObject::tuple(vec![]));
    }
}

fn pkl_class_candidate(obj: PyObjectRef) -> Option<PyObjectRef> {
    if matches!(
        &obj.payload,
        PyObjectPayload::Class(_) | PyObjectPayload::BuiltinType(_)
    ) {
        Some(obj)
    } else {
        None
    }
}

fn pkl_resolve_dotted_attr(root: PyObjectRef, path: &str) -> Option<PyObjectRef> {
    let mut obj = root;
    for part in path.split('.') {
        if part.is_empty() {
            return None;
        }
        obj = obj.get_attr(part)?;
    }
    Some(obj)
}

fn pkl_resolve_attr_map_name(attrs: &FxAttrMap, name: &str) -> Option<PyObjectRef> {
    if let Some(obj) = attrs.get(name).cloned() {
        return Some(obj);
    }
    let (root_name, rest) = name.split_once('.')?;
    let root = attrs.get(root_name).cloned()?;
    pkl_resolve_dotted_attr(root, rest)
}

fn pkl_resolve_sys_module(module: &str) -> Option<PyObjectRef> {
    let sys = crate::get_current_sys_module()?;
    let modules = sys.get_attr("modules")?;
    let PyObjectPayload::Dict(map) = &modules.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(module)))
        .cloned()
}

fn pkl_resolve_global_class(module: &str, name: &str) -> Option<PyObjectRef> {
    if let Some(module_obj) = pkl_resolve_sys_module(module) {
        if let Some(cls) = pkl_resolve_dotted_attr(module_obj, name).and_then(pkl_class_candidate) {
            return Some(cls);
        }
    }

    let globals = crate::get_current_globals()?;
    let globals_r = globals.read();
    let current_module = globals_r.get("__name__").map(|value| value.py_to_string());
    if module == "__main__" || current_module.as_deref() == Some(module) {
        if let Some(cls) = pkl_resolve_attr_map_name(&globals_r, name).and_then(pkl_class_candidate)
        {
            return Some(cls);
        }
    }

    if let Some(module_obj) = pkl_resolve_attr_map_name(&globals_r, module) {
        if let Some(cls) = pkl_resolve_dotted_attr(module_obj, name).and_then(pkl_class_candidate) {
            return Some(cls);
        }
    }

    None
}

fn pkl_reduce(callable: &PklStackItem, args: &PyObjectRef) -> PyResult<PyObjectRef> {
    if let PklStackItem::Global(module, name) = callable {
        let arg_list = match &args.payload {
            PyObjectPayload::Tuple(items) => (**items).clone(),
            _ => vec![args.clone()],
        };
        if matches!(module.as_str(), "__builtin__" | "builtins" | "exceptions") {
            if let Some(kind) = ExceptionKind::from_name(name.as_str()) {
                return Ok(pickle_exception_instance(kind, arg_list));
            }
        }
        match (module.as_str(), name.as_str()) {
            ("__builtin__" | "builtins", "set") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::List(items) = &first.payload {
                        let items = items.read();
                        let mut map = IndexMap::new();
                        for item in items.iter() {
                            if let Ok(hk) = HashableKey::from_object(item) {
                                map.insert(hk, item.clone());
                            }
                        }
                        return Ok(PyObject::set(map));
                    }
                }
                Ok(PyObject::set(IndexMap::new()))
            }
            ("__builtin__" | "builtins", "frozenset") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::List(items) = &first.payload {
                        let items = items.read();
                        let mut map = IndexMap::new();
                        for item in items.iter() {
                            if let Ok(hk) = HashableKey::from_object(item) {
                                map.insert(hk, item.clone());
                            }
                        }
                        return Ok(PyObject::frozenset(map));
                    }
                }
                Ok(PyObject::frozenset(IndexMap::new()))
            }
            ("__builtin__" | "builtins", "iter") => {
                // Reconstruct iter(list_or_tuple) as a VecIter.
                if let Some(first) = arg_list.first() {
                    let items: Vec<PyObjectRef> = match &first.payload {
                        PyObjectPayload::List(cell) => cell.read().clone(),
                        PyObjectPayload::Tuple(items) => (**items).clone(),
                        _ => vec![first.clone()],
                    };
                    use ferrython_core::object::{SyncUsize, VecIterData};
                    return Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(
                        VecIterData {
                            items,
                            index: SyncUsize::new(0),
                        },
                    ))));
                }
                use ferrython_core::object::{SyncUsize, VecIterData};
                Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(
                    VecIterData {
                        items: vec![],
                        index: SyncUsize::new(0),
                    },
                ))))
            }
            (
                "__builtin__" | "builtins",
                "int" | "float" | "str" | "list" | "dict" | "tuple" | "bool",
            ) => Ok(PyObject::builtin_type(CompactString::from(name.as_str()))),
            ("__builtin__" | "builtins", "__ferrython_seqiter__") => {
                use ferrython_core::object::IteratorData;
                let source = arg_list.first().cloned().unwrap_or_else(PyObject::none);
                let index = arg_list.get(1).and_then(|v| v.as_int()).unwrap_or(0);
                let exhausted = arg_list.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::SeqIter {
                        obj: source,
                        index,
                        exhausted,
                    }),
                ))))
            }
            ("__builtin__" | "builtins", "__ferrython_refiter__") => {
                let source = arg_list.first().cloned().unwrap_or_else(PyObject::none);
                let index = arg_list.get(1).and_then(|v| v.as_int()).unwrap_or(0);
                let exhausted = arg_list.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                let index = if exhausted || index < 0 {
                    usize::MAX
                } else {
                    index as usize
                };
                Ok(PyObject::wrap(PyObjectPayload::RefIter {
                    source,
                    index: ferrython_core::object::SyncUsize::new(index),
                }))
            }
            ("__builtin__" | "builtins", "__ferrython_revrefiter__") => {
                let source = arg_list.first().cloned().unwrap_or_else(PyObject::none);
                let index = arg_list.get(1).and_then(|v| v.as_int()).unwrap_or(0);
                let exhausted = arg_list.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                let index = if exhausted || index < 0 {
                    usize::MAX
                } else {
                    index as usize
                };
                Ok(PyObject::wrap(PyObjectPayload::RevRefIter {
                    source,
                    index: ferrython_core::object::SyncUsize::new(index),
                }))
            }
            ("collections", "defaultdict") | ("__main__", "defaultdict") => {
                let collections = create_collections_module();
                let cls = collections.get_attr("defaultdict").ok_or_else(|| {
                    PyException::runtime_error("UnpicklingError: missing collections.defaultdict")
                })?;
                let result = PyObject::instance(cls);
                let raw_factory = arg_list.first().cloned().unwrap_or_else(PyObject::none);
                let factory = if let PyObjectPayload::Str(s) = &raw_factory.payload {
                    if let Some(name) = s.as_str().strip_prefix("__ferrython_builtin_type__:") {
                        PyObject::builtin_type(CompactString::from(name))
                    } else {
                        raw_factory.clone()
                    }
                } else {
                    raw_factory.clone()
                };
                if let PyObjectPayload::Instance(inst) = &result.payload {
                    if !matches!(&factory.payload, PyObjectPayload::None) {
                        if let Some(dst) = inst.dict_storage.as_ref() {
                            dst.write().insert(
                                HashableKey::str_key(CompactString::from(
                                    "__defaultdict_factory__",
                                )),
                                factory.clone(),
                            );
                        }
                    }
                    inst.attrs
                        .write()
                        .insert(CompactString::from("default_factory"), factory);
                    if let Some(data) = arg_list.get(1) {
                        if let PyObjectPayload::Dict(map) = &data.payload {
                            if let Some(dst) = inst.dict_storage.as_ref() {
                                for (k, v) in map.read().iter() {
                                    dst.write().insert(k.clone(), v.clone());
                                }
                            }
                        }
                    }
                }
                Ok(result)
            }
            ("operator", "attrgetter" | "itemgetter" | "methodcaller") => {
                let module = create_operator_module();
                let constructor = module.get_attr(name.as_str()).ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: unsupported operator constructor {}",
                        name
                    ))
                })?;
                ferrython_core::object::call_callable(&constructor, &arg_list)
            }
            ("__main__" | "re", "Pattern") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::Dict(map) = &first.payload {
                        let map_r = map.read();
                        let pattern = map_r
                            .get(&HashableKey::str_key(CompactString::from("pattern")))
                            .cloned()
                            .unwrap_or_else(|| PyObject::str_val(CompactString::from("")));
                        let flags = map_r
                            .get(&HashableKey::str_key(CompactString::from("flags")))
                            .cloned()
                            .unwrap_or_else(|| PyObject::int(0));
                        let module = create_re_module();
                        let compile = module.get_attr("compile").ok_or_else(|| {
                            PyException::runtime_error("UnpicklingError: missing re.compile")
                        })?;
                        return ferrython_core::object::call_callable(&compile, &[pattern, flags]);
                    }
                }
                Err(PyException::runtime_error(
                    "UnpicklingError: invalid Pattern state",
                ))
            }
            (_, "namedtuple_field") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::Dict(map) = &first.payload {
                        let map_r = map.read();
                        let idx = map_r
                            .get(&HashableKey::str_key(CompactString::from(
                                "__tuple_index__",
                            )))
                            .and_then(|v| v.as_int())
                            .unwrap_or(0);
                        let field_name = map_r
                            .get(&HashableKey::str_key(CompactString::from("__field_name__")))
                            .map(|v| v.py_to_string())
                            .unwrap_or_default();
                        let doc = map_r
                            .get(&HashableKey::str_key(CompactString::from("__doc__")))
                            .map(|v| v.py_to_string())
                            .unwrap_or_default();
                        return namedtuple_rebuild_field(&[
                            PyObject::int(idx),
                            PyObject::str_val(CompactString::from(field_name)),
                            PyObject::str_val(CompactString::from(doc)),
                        ]);
                    }
                }
                Err(PyException::runtime_error(
                    "UnpicklingError: invalid namedtuple_field state",
                ))
            }
            (_, "Counter") => {
                let class_obj = crate::get_current_globals()
                    .and_then(|globals| globals.read().get("Counter").cloned())
                    .or_else(|| {
                        crate::get_current_globals().and_then(|globals| {
                            globals
                                .read()
                                .get("collections")
                                .cloned()
                                .and_then(|collections_mod| collections_mod.get_attr("Counter"))
                        })
                    });
                let counter_cls = class_obj.unwrap_or_else(|| {
                    PyObject::class(
                        CompactString::from("Counter"),
                        vec![PyObject::builtin_type(CompactString::from("dict"))],
                        IndexMap::new(),
                    )
                });
                let result = PyObject::instance(counter_cls);
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::Dict(map) = &first.payload {
                        if let Some(dst) = match &result.payload {
                            PyObjectPayload::Instance(inst) => inst.dict_storage.as_ref(),
                            _ => None,
                        } {
                            let mut w = dst.write();
                            for (k, v) in map.read().iter() {
                                if let HashableKey::Str(s) = k {
                                    if s.as_str() == "__counter_kwargs__" {
                                        continue;
                                    }
                                }
                                w.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
                return Ok(result);
            }
            _ => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::Dict(map) = &first.payload {
                        let map_r = map.read();
                        if map_r
                            .get(&HashableKey::str_key(CompactString::from("__namedtuple__")))
                            .is_some()
                        {
                            let field_names = map_r
                                .get(&HashableKey::str_key(CompactString::from("_fields")))
                                .and_then(|v| v.to_list().ok())
                                .unwrap_or_default();
                            let defaults = map_r
                                .get(&HashableKey::str_key(CompactString::from(
                                    "_field_defaults",
                                )))
                                .and_then(|v| {
                                    if let PyObjectPayload::Dict(d) = &v.payload {
                                        Some(
                                            d.read()
                                                .values()
                                                .cloned()
                                                .collect::<Vec<PyObjectRef>>(),
                                        )
                                    } else {
                                        None
                                    }
                                });
                            let module_obj = map_r
                                .get(&HashableKey::str_key(CompactString::from("__module__")))
                                .cloned()
                                .unwrap_or_else(|| PyObject::str_val(CompactString::from(module)));
                            let tuple_values = map_r
                                .get(&HashableKey::str_key(CompactString::from("_tuple")))
                                .cloned()
                                .unwrap_or_else(|| PyObject::tuple(vec![]));
                            let rebuilt = namedtuple_rebuild_instance(&[
                                PyObject::str_val(CompactString::from(name.as_str())),
                                PyObject::tuple(field_names),
                                defaults.map(PyObject::tuple).unwrap_or_else(PyObject::none),
                                module_obj,
                                tuple_values,
                            ])?;
                            if let PyObjectPayload::Instance(ref inst_data) = rebuilt.payload {
                                let mut attrs = inst_data.attrs.write();
                                for (k, v) in map_r.iter() {
                                    if let HashableKey::Str(key) = k {
                                        if matches!(
                                            key.as_str(),
                                            "__namedtuple__"
                                                | "_fields"
                                                | "_field_defaults"
                                                | "__module__"
                                                | "_tuple"
                                        ) {
                                            continue;
                                        }
                                        attrs.insert(key.to_compact_string(), v.clone());
                                    }
                                }
                            }
                            return Ok(rebuilt);
                        }
                        let mut attrs = IndexMap::new();
                        for (k, v) in map_r.iter() {
                            if let HashableKey::Str(s) = k {
                                attrs.insert(s.to_compact_string(), v.clone());
                            }
                        }
                        maybe_add_ast_empty_fields(name, &mut attrs);
                        let cls = pkl_resolve_global_class(module, name).unwrap_or_else(|| {
                            let mut class_namespace = IndexMap::new();
                            class_namespace.insert(
                                CompactString::from("__module__"),
                                PyObject::str_val(CompactString::from(module.as_str())),
                            );
                            PyObject::class(
                                CompactString::from(name.as_str()),
                                vec![],
                                class_namespace,
                            )
                        });
                        return Ok(PyObject::instance_with_attrs(cls, attrs));
                    }
                }
                Err(PyException::runtime_error(format!(
                    "UnpicklingError: unsupported global {}.{}",
                    module, name
                )))
            }
        }
    } else {
        Err(PyException::runtime_error(
            "UnpicklingError: REDUCE requires a callable",
        ))
    }
}

fn pickle_loads_p0(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos: usize = 0;
    let mut stack: Vec<PklStackItem> = Vec::new();
    let mut memo: std::collections::HashMap<u32, PyObjectRef> = std::collections::HashMap::new();

    while pos < data.len() {
        let opcode = data[pos];
        pos += 1;
        match opcode {
            b'.' => break, // STOP
            b'N' => stack.push(PklStackItem::Value(PyObject::none())),
            b'I' => {
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| {
                        PyException::runtime_error("UnpicklingError: invalid INT encoding")
                    })?
                    .trim();
                if s == "01" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(true)));
                } else if s == "00" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(false)));
                } else {
                    let val: i64 = s.parse().map_err(|_| {
                        PyException::runtime_error(format!(
                            "UnpicklingError: invalid INT value '{}'",
                            s
                        ))
                    })?;
                    stack.push(PklStackItem::Value(PyObject::int(val)));
                }
            }
            b'L' => {
                // LONG — like I but for big ints, trailing L
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| {
                        PyException::runtime_error("UnpicklingError: invalid LONG encoding")
                    })?
                    .trim()
                    .trim_end_matches('L');
                let val: i64 = s.parse().unwrap_or(0);
                stack.push(PklStackItem::Value(PyObject::int(val)));
            }
            b'F' => {
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| {
                        PyException::runtime_error("UnpicklingError: invalid FLOAT encoding")
                    })?
                    .trim();
                let val: f64 = match s {
                    "nan" | "NaN" => f64::NAN,
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => s.parse().map_err(|_| {
                        PyException::runtime_error(format!(
                            "UnpicklingError: invalid FLOAT value '{}'",
                            s
                        ))
                    })?,
                };
                stack.push(PklStackItem::Value(PyObject::float(val)));
            }
            b'V' => {
                // UNICODE — read raw-unicode-escape line
                let line = p0_read_line(data, &mut pos);
                let s = p0_unescape_unicode(line);
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
            }
            b'S' => {
                // STRING — read quoted string line (bytes)
                let line = p0_read_line(data, &mut pos);
                let bytes = p0_unescape_bytes(line);
                stack.push(PklStackItem::Value(PyObject::bytes(bytes)));
            }
            b'(' => stack.push(PklStackItem::Mark),
            b'l' => {
                // LIST — pop to mark, build list
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::list(items)));
            }
            b't' => {
                // TUPLE — pop to mark, build tuple
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::tuple(items)));
            }
            b'd' => {
                // DICT — pop to mark, build dict from pairs
                let items = pkl_pop_to_mark(&mut stack)?;
                let mut pairs = Vec::new();
                for chunk in items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PklStackItem::Value(PyObject::dict_from_pairs(pairs)));
            }
            b'a' => {
                // APPEND — pop item, append to list on stack
                let item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: APPEND expects value",
                        ))
                    }
                };
                // Find the list on top of the remaining stack
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().push(item);
                    }
                }
            }
            b's' => {
                // SETITEM — pop value, pop key, set on dict
                let val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects value",
                        ))
                    }
                };
                let key = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects key",
                        ))
                    }
                };
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        if let Ok(hk) = HashableKey::from_object(&key) {
                            dict_map.write().insert(hk, val);
                        }
                    }
                }
            }
            b'p' => {
                // PUT — memoize top of stack
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'g' => {
                // GET — recall from memo
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'c' => {
                // GLOBAL — read module\nqualname\n
                let mod_line = p0_read_line(data, &mut pos);
                let name_line = p0_read_line(data, &mut pos);
                let module = String::from_utf8_lossy(mod_line).to_string();
                let name = String::from_utf8_lossy(name_line).to_string();
                stack.push(PklStackItem::Global(module, name));
            }
            b'R' => {
                // REDUCE — pop args tuple, pop callable, call
                let args_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects args",
                        ))
                    }
                };
                let callable = match stack.pop() {
                    Some(item) => item,
                    None => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects callable",
                        ))
                    }
                };
                let result = pkl_reduce(&callable, &args_item)?;
                stack.push(PklStackItem::Value(result));
            }
            b'b' => {
                // BUILD — apply a state dict to the object on top of the stack.
                let state = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects state",
                        ))
                    }
                };
                let obj = match stack.last() {
                    Some(PklStackItem::Value(v)) => v.clone(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects object",
                        ))
                    }
                };
                pkl_apply_state(&obj, &state)?;
            }
            b'\n' | b'\r' | b' ' => {} // skip whitespace
            _ => {
                return Err(PyException::runtime_error(format!(
                    "UnpicklingError: unknown protocol 0 opcode 0x{:02x} ('{}')",
                    opcode,
                    if opcode.is_ascii_graphic() {
                        opcode as char
                    } else {
                        '?'
                    }
                )));
            }
        }
    }

    // Return top of stack
    for item in stack.iter().rev() {
        if let PklStackItem::Value(v) = item {
            return Ok(v.clone());
        }
    }
    Err(PyException::runtime_error(
        "UnpicklingError: empty pickle data",
    ))
}

// ── Protocol 2 (binary) deserialization ──

fn pickle_loads_p2(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos: usize = 0;
    let mut stack: Vec<PklStackItem> = Vec::new();
    let mut memo: std::collections::HashMap<u32, PyObjectRef> = std::collections::HashMap::new();

    // Skip protocol header
    if pos + 1 < data.len() && data[pos] == 0x80 {
        pos += 2;
    }

    while pos < data.len() {
        let opcode = data[pos];
        pos += 1;
        match opcode {
            b'.' => break, // STOP
            b'N' => stack.push(PklStackItem::Value(PyObject::none())),
            0x88 => stack.push(PklStackItem::Value(PyObject::bool_val(true))),
            0x89 => stack.push(PklStackItem::Value(PyObject::bool_val(false))),
            b'K' => {
                // BININT1 — 1-byte unsigned int
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT1",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::int(data[pos] as i64)));
                pos += 1;
            }
            b'M' => {
                // BININT2 — 2-byte LE unsigned short
                if pos + 2 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT2",
                    ));
                }
                let val = u16::from_le_bytes([data[pos], data[pos + 1]]) as i64;
                stack.push(PklStackItem::Value(PyObject::int(val)));
                pos += 2;
            }
            b'J' => {
                // BININT — 4-byte LE signed int
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT",
                    ));
                }
                let val =
                    i32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as i64;
                stack.push(PklStackItem::Value(PyObject::int(val)));
                pos += 4;
            }
            b'I' => {
                // INT (text fallback) — read to newline
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line).unwrap_or("0").trim();
                if s == "01" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(true)));
                } else if s == "00" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(false)));
                } else {
                    let val: i64 = s.parse().unwrap_or(0);
                    stack.push(PklStackItem::Value(PyObject::int(val)));
                }
            }
            b'G' => {
                // BINFLOAT — 8-byte BE IEEE 754 double
                if pos + 8 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINFLOAT",
                    ));
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[pos..pos + 8]);
                let val = f64::from_be_bytes(bytes);
                stack.push(PklStackItem::Value(PyObject::float(val)));
                pos += 8;
            }
            b'X' => {
                // BINUNICODE — 4-byte LE len + UTF-8
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINUNICODE length",
                    ));
                }
                let len =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINUNICODE data",
                    ));
                }
                let s = std::str::from_utf8(&data[pos..pos + len]).map_err(|_| {
                    PyException::runtime_error("UnpicklingError: invalid utf-8 in BINUNICODE")
                })?;
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
                pos += len;
            }
            0x8c => {
                // SHORT_BINUNICODE — 1-byte len + UTF-8
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINUNICODE",
                    ));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINUNICODE data",
                    ));
                }
                let s = std::str::from_utf8(&data[pos..pos + len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid utf-8"))?;
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
                pos += len;
            }
            b'T' => {
                // BINSTRING — 4-byte LE len + bytes
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINSTRING length",
                    ));
                }
                let len =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINSTRING data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b'U' => {
                // SHORT_BINSTRING — 1-byte len + bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINSTRING",
                    ));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINSTRING data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b'B' => {
                // BINBYTES — 4-byte LE len + raw bytes
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINBYTES length",
                    ));
                }
                let len =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINBYTES data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b'C' => {
                // SHORT_BINBYTES — 1-byte len + raw bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINBYTES",
                    ));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINBYTES data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b']' => stack.push(PklStackItem::Value(PyObject::list(vec![]))),
            b'}' => stack.push(PklStackItem::Value(PyObject::dict_from_pairs(vec![]))),
            b')' => stack.push(PklStackItem::Value(PyObject::tuple(vec![]))),
            b'(' => stack.push(PklStackItem::Mark),
            b'l' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::list(items)));
            }
            b't' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::tuple(items)));
            }
            b'd' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                let mut pairs = Vec::new();
                for chunk in items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PklStackItem::Value(PyObject::dict_from_pairs(pairs)));
            }
            0x85 => {
                // TUPLE1
                let v = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE1 stack underflow",
                        ))
                    }
                };
                stack.push(PklStackItem::Value(PyObject::tuple(vec![v])));
            }
            0x86 => {
                // TUPLE2
                let b_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE2 stack underflow",
                        ))
                    }
                };
                let a_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE2 stack underflow",
                        ))
                    }
                };
                stack.push(PklStackItem::Value(PyObject::tuple(vec![a_val, b_val])));
            }
            0x87 => {
                // TUPLE3
                let c_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE3 stack underflow",
                        ))
                    }
                };
                let b_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE3 stack underflow",
                        ))
                    }
                };
                let a_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE3 stack underflow",
                        ))
                    }
                };
                stack.push(PklStackItem::Value(PyObject::tuple(vec![
                    a_val, b_val, c_val,
                ])));
            }
            b'a' => {
                // APPEND
                let item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: APPEND expects value",
                        ))
                    }
                };
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().push(item);
                    }
                }
            }
            b'e' => {
                // APPENDS — pop items to mark, extend list
                let items = pkl_pop_to_mark(&mut stack)?;
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().extend(items);
                    }
                }
            }
            b's' => {
                // SETITEM
                let val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects value",
                        ))
                    }
                };
                let key = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects key",
                        ))
                    }
                };
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        if let Ok(hk) = HashableKey::from_object(&key) {
                            dict_map.write().insert(hk, val);
                        }
                    }
                }
            }
            b'u' => {
                // SETITEMS — pop pairs to mark, update dict
                let items = pkl_pop_to_mark(&mut stack)?;
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        let mut map = dict_map.write();
                        for chunk in items.chunks_exact(2) {
                            if let Ok(hk) = HashableKey::from_object(&chunk[0]) {
                                map.insert(hk, chunk[1].clone());
                            }
                        }
                    }
                }
            }
            b'q' => {
                // BINPUT — 1-byte memo index
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINPUT",
                    ));
                }
                let id = data[pos] as u32;
                pos += 1;
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'h' => {
                // BINGET — 1-byte memo index
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINGET",
                    ));
                }
                let id = data[pos] as u32;
                pos += 1;
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'r' => {
                // LONG_BINPUT — 4-byte LE memo index
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG_BINPUT",
                    ));
                }
                let id =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                pos += 4;
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'j' => {
                // LONG_BINGET — 4-byte LE memo index
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG_BINGET",
                    ));
                }
                let id =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                pos += 4;
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'p' => {
                // PUT (text) — read id to newline
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'g' => {
                // GET (text) — read id to newline
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'c' => {
                // GLOBAL — module\nqualname\n
                let mod_line = p0_read_line(data, &mut pos);
                let name_line = p0_read_line(data, &mut pos);
                let module = String::from_utf8_lossy(mod_line).to_string();
                let name = String::from_utf8_lossy(name_line).to_string();
                stack.push(PklStackItem::Global(module, name));
            }
            0x93 => {
                // STACK_GLOBAL — pop name, pop module, push global
                let name_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v.py_to_string(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: STACK_GLOBAL expects name",
                        ))
                    }
                };
                let mod_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v.py_to_string(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: STACK_GLOBAL expects module",
                        ))
                    }
                };
                stack.push(PklStackItem::Global(mod_item, name_item));
            }
            b'R' => {
                // REDUCE
                let args_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects args",
                        ))
                    }
                };
                let callable = match stack.pop() {
                    Some(item) => item,
                    None => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects callable",
                        ))
                    }
                };
                let result = pkl_reduce(&callable, &args_item)?;
                stack.push(PklStackItem::Value(result));
            }
            b'b' => {
                // BUILD — apply a state dict to the object on top of the stack.
                let state = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects state",
                        ))
                    }
                };
                let obj = match stack.last() {
                    Some(PklStackItem::Value(v)) => v.clone(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects object",
                        ))
                    }
                };
                pkl_apply_state(&obj, &state)?;
            }
            0x8a => {
                // LONG1 — 1-byte count + little-endian 2's complement bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG1",
                    ));
                }
                let count = data[pos] as usize;
                pos += 1;
                if pos + count > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG1 data",
                    ));
                }
                let bytes = &data[pos..pos + count];
                pos += count;
                if count == 0 {
                    stack.push(PklStackItem::Value(PyObject::int(0)));
                } else {
                    // Little-endian 2's complement
                    let mut val: i64 = 0;
                    for (i, &b) in bytes.iter().enumerate() {
                        val |= (b as i64) << (i * 8);
                    }
                    // Sign extend if high bit set
                    if bytes[count - 1] & 0x80 != 0 {
                        for i in count..8 {
                            val |= 0xffi64 << (i * 8);
                        }
                    }
                    stack.push(PklStackItem::Value(PyObject::int(val)));
                }
            }
            b'V' => {
                // UNICODE (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let s = p0_unescape_unicode(line);
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
            }
            b'S' => {
                // STRING (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let bytes = p0_unescape_bytes(line);
                stack.push(PklStackItem::Value(PyObject::bytes(bytes)));
            }
            b'F' => {
                // FLOAT (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line).unwrap_or("0").trim();
                let val: f64 = match s {
                    "nan" | "NaN" => f64::NAN,
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => s.parse().unwrap_or(0.0),
                };
                stack.push(PklStackItem::Value(PyObject::float(val)));
            }
            b'0' => {} // POP — discard top (used after PUT sometimes)
            b'1' => {} // POP_MARK — discard stack to mark
            b'2' => {
                // DUP — duplicate top of stack
                let val = pkl_stack_top_value(&stack)?;
                stack.push(PklStackItem::Value(val));
            }
            _ => {
                return Err(PyException::runtime_error(format!(
                    "UnpicklingError: unknown opcode 0x{:02x}",
                    opcode
                )));
            }
        }
    }

    for item in stack.iter().rev() {
        if let PklStackItem::Value(v) = item {
            return Ok(v.clone());
        }
    }
    Err(PyException::runtime_error(
        "UnpicklingError: empty pickle data",
    ))
}

// ── Unified deserialization (auto-detects protocol) ──

pub(super) fn pickle_loads_stack(data: &[u8]) -> PyResult<PyObjectRef> {
    if data.is_empty() {
        return Err(PyException::runtime_error(
            "UnpicklingError: empty pickle data",
        ));
    }
    if data[0] == 0x80 {
        pickle_loads_p2(data)
    } else {
        pickle_loads_p0(data)
    }
}

// ── Public API ──

pub fn create_pickle_module() -> PyObjectRef {
    let pickler_cls = {
        PyObject::native_closure("Pickler", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Pickler requires a file argument"));
            }
            let file = args[0].clone();
            let protocol = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
            let buf: Rc<PyCell<Vec<u8>>> = Rc::new(PyCell::new(Vec::new()));

            let cls_inner =
                PyObject::class(CompactString::from("Pickler"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls_inner);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_file"), file.clone());
                w.insert(CompactString::from("protocol"), PyObject::int(protocol));
                let b = buf.clone();
                let f = file.clone();
                w.insert(
                    CompactString::from("dump"),
                    PyObject::native_closure("dump", move |dargs| {
                        if dargs.is_empty() {
                            return Err(PyException::type_error("dump requires an object"));
                        }
                        let obj = &dargs[dargs.len() - 1];
                        let mut data = b.write();
                        data.clear();
                        pickle_serialize(obj, &mut data)?;
                        if let Some(write_fn) = f.get_attr("write") {
                            let bytes_obj = PyObject::bytes(data.clone());
                            ferrython_core::error::request_vm_call(write_fn, vec![bytes_obj]);
                        }
                        Ok(PyObject::none())
                    }),
                );
                w.insert(
                    CompactString::from("clear_memo"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
            }
            Ok(inst)
        })
    };

    let unpickler_cls = {
        PyObject::native_closure("Unpickler", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "Unpickler requires a file argument",
                ));
            }
            let file = args[0].clone();
            let cls_inner =
                PyObject::class(CompactString::from("Unpickler"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls_inner);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_file"), file.clone());
                let f = file.clone();
                w.insert(
                    CompactString::from("load"),
                    PyObject::native_closure("load", move |_largs| {
                        if let Some(read_fn) = f.get_attr("read") {
                            ferrython_core::error::request_vm_call(read_fn, vec![]);
                        }
                        Ok(PyObject::none())
                    }),
                );
            }
            Ok(inst)
        })
    };

    let pickling_error = PyObject::class(
        CompactString::from("PicklingError"),
        vec![],
        IndexMap::new(),
    );
    let unpickling_error = PyObject::class(
        CompactString::from("UnpicklingError"),
        vec![],
        IndexMap::new(),
    );

    make_module(
        "pickle",
        vec![
            ("dumps", make_builtin(pickle_dumps)),
            ("loads", make_builtin(pickle_loads)),
            ("dump", make_builtin(pickle_dump)),
            ("load", make_builtin(pickle_load)),
            ("_dumps", make_builtin(pickle_dumps)),
            ("_loads", make_builtin(pickle_loads)),
            ("_dump", make_builtin(pickle_dump)),
            ("Pickler", pickler_cls),
            ("Unpickler", unpickler_cls),
            ("HIGHEST_PROTOCOL", PyObject::int(5)),
            ("DEFAULT_PROTOCOL", PyObject::int(4)),
            ("PicklingError", pickling_error),
            ("UnpicklingError", unpickling_error),
            (
                "PickleError",
                PyObject::class(CompactString::from("PickleError"), vec![], IndexMap::new()),
            ),
            (
                "bytes_types",
                PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from("bytes")),
                    PyObject::str_val(CompactString::from("bytearray")),
                ]),
            ),
        ],
    )
}

fn pickle_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.dumps() missing 1 required positional argument: 'obj'",
        ));
    }
    // Extract protocol from positional arg 1, or from a trailing kwargs dict (protocol=N)
    let mut protocol: i64 = 0;
    if let Some(a) = args.get(1) {
        if let Some(n) = a.as_int() {
            protocol = n;
        } else if let PyObjectPayload::Dict(m) = &a.payload {
            let r = m.read();
            for (k, v) in r.iter() {
                if let HashableKey::Str(s) = k {
                    if s.as_str() == "protocol" {
                        if let Some(n) = v.as_int() {
                            protocol = n;
                        }
                    }
                }
            }
        }
    }
    // Also check a last-position kwargs dict (e.g., args[2] when args[1] is obj's second positional)
    if let Some(a) = args.last() {
        if args.len() > 1 {
            if let PyObjectPayload::Dict(m) = &a.payload {
                let r = m.read();
                for (k, v) in r.iter() {
                    if let HashableKey::Str(s) = k {
                        if s.as_str() == "protocol" {
                            if let Some(n) = v.as_int() {
                                protocol = n;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut buf = Vec::new();
    let mut memo = PickleWriteMemo::default();
    if protocol >= 2 {
        buf.extend_from_slice(b"\x80\x02");
        pickle_serialize_p2(&args[0], &mut buf, &mut memo)?;
    } else {
        pickle_serialize_p0(&args[0], &mut buf, &mut memo)?;
    }
    buf.push(b'.');
    Ok(PyObject::bytes(buf))
}

fn pickle_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.loads() missing 1 required positional argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;
    pickle_loads_stack(&data)
}

fn pickle_dump(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "pickle.dump() missing required arguments: 'obj' and 'file'",
        ));
    }
    let protocol = args.get(2).and_then(|a| a.as_int()).unwrap_or(0);
    let data = pickle_dumps(&[args[0].clone(), PyObject::int(protocol)])?;
    let data_bytes = extract_bytes(&data)?;

    // Try file path first (via .name attribute)
    if let Some(name) = args[1].get_attr("name") {
        let path = name.py_to_string();
        if !path.is_empty() {
            std::fs::write(&path, &data_bytes)
                .map_err(|e| PyException::runtime_error(format!("pickle.dump: {}", e)))?;
            return Ok(PyObject::none());
        }
    }
    // Try file-like object with write method (BytesIO, etc.)
    if let Some(write_method) = args[1].get_attr("write") {
        match &write_method.payload {
            PyObjectPayload::NativeFunction(nf) => {
                let _ = (nf.func)(&[PyObject::bytes(data_bytes.clone())]);
                return Ok(PyObject::none());
            }
            PyObjectPayload::NativeClosure(nc) => {
                let _ = (nc.func)(&[PyObject::bytes(data_bytes.clone())]);
                return Ok(PyObject::none());
            }
            _ => {}
        }
    }
    if let PyObjectPayload::Str(path) = &args[1].payload {
        std::fs::write(path.as_str(), &data_bytes)
            .map_err(|e| PyException::runtime_error(format!("pickle.dump: {}", e)))?;
    }
    Ok(PyObject::none())
}

fn pickle_load(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.load() missing 1 required positional argument: 'file'",
        ));
    }
    // Try file path first (via .name attribute)
    if let Some(name) = args[0].get_attr("name") {
        let path = name.py_to_string();
        if !path.is_empty() && std::path::Path::new(&path).exists() {
            let data = std::fs::read(&path)
                .map_err(|e| PyException::runtime_error(format!("pickle.load: {}", e)))?;
            return pickle_loads_stack(&data);
        }
    }
    // Try file-like object with read method (BytesIO, etc.)
    if let Some(read_method) = args[0].get_attr("read") {
        let read_result = match &read_method.payload {
            PyObjectPayload::NativeFunction(nf) => (nf.func)(&[]).ok(),
            PyObjectPayload::NativeClosure(nc) => (nc.func)(&[]).ok(),
            _ => None,
        };
        if let Some(data_obj) = read_result {
            let data = extract_bytes(&data_obj)?;
            if !data.is_empty() {
                return pickle_loads_stack(&data);
            }
        }
    }
    if let PyObjectPayload::Str(path) = &args[0].payload {
        let data = std::fs::read(path.as_str())
            .map_err(|e| PyException::runtime_error(format!("pickle.load: {}", e)))?;
        return pickle_loads_stack(&data);
    }
    Err(PyException::runtime_error(
        "pickle.load: expected a file path or file-like object",
    ))
}
