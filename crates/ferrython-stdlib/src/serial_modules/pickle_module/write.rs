use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::{py_int_from_bigint, range_iter_item_bigint};
use ferrython_core::object::{
    lookup_in_class_mro, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

mod memo;

pub(super) use memo::PickleWriteMemo;
use memo::{
    p0_emit_put_obj, p0_escape_bytes, p0_escape_unicode, p0_try_emit_get, p2_emit_put_obj,
    p2_try_emit_get, pickle_identity_key,
};

use super::shared::{
    exception_pickle_state, format_float_repr, hashable_key_to_pyobj, operator_reduce_target,
};

fn range_pickle_args(rd: &ferrython_core::object::RangeData) -> Vec<PyObjectRef> {
    let (start, stop, step) = ferrython_core::object::helpers::range_parts_bigint(rd);
    vec![
        ferrython_core::object::helpers::py_int_from_bigint(start),
        ferrython_core::object::helpers::py_int_from_bigint(stop),
        ferrython_core::object::helpers::py_int_from_bigint(step),
    ]
}

fn pickle_global_function_parts(
    obj: &PyObjectRef,
    func: &ferrython_core::types::PyFunction,
) -> Option<(String, String)> {
    let module = func
        .globals
        .read()
        .get("__name__")
        .map(|value| value.py_to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "__main__".to_string());
    let name = func.qualname.to_string();
    if name.contains("<locals>") {
        return None;
    }
    let module_obj = if module == "__main__" {
        None
    } else {
        crate::get_current_sys_module()
            .and_then(|sys| sys.get_attr("modules"))
            .and_then(|modules| {
                if let PyObjectPayload::Dict(map) = &modules.payload {
                    map.read()
                        .get(&HashableKey::str_key(CompactString::from(module.as_str())))
                        .cloned()
                } else {
                    None
                }
            })
    };
    let resolved = if let Some(module_obj) = module_obj {
        let mut current = module_obj;
        let mut ok = true;
        for part in name.split('.') {
            if let Some(next) = current.get_attr(part) {
                current = next;
            } else {
                ok = false;
                break;
            }
        }
        ok.then_some(current)
    } else {
        func.globals.read().get(name.as_str()).cloned()
    };
    resolved
        .filter(|candidate| PyObjectRef::ptr_eq(candidate, obj))
        .map(|_| (module, name))
}

fn pickle_serialize_function_global_p0(module: &str, name: &str, buf: &mut Vec<u8>) {
    buf.push(b'c');
    buf.extend_from_slice(module.as_bytes());
    buf.push(b'\n');
    buf.extend_from_slice(name.as_bytes());
    buf.push(b'\n');
}

fn pickle_serialize_function_global_p2(module: &str, name: &str, buf: &mut Vec<u8>) {
    pickle_serialize_function_global_p0(module, name, buf);
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

fn instance_state_dict(data_pairs: &[(CompactString, PyObjectRef)]) -> PyObjectRef {
    let mut pairs = Vec::with_capacity(data_pairs.len());
    for (key, value) in data_pairs {
        pairs.push((PyObject::str_val(key.clone()), value.clone()));
    }
    PyObject::dict_from_pairs(pairs)
}

fn class_override_method(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    let method = lookup_in_class_mro(&inst.class, name)?;
    Some(PyObjectRef::new(PyObject {
        payload: PyObjectPayload::BoundMethod {
            receiver: obj.clone(),
            method,
        },
    }))
}

fn deque_pickle_items(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    if let Some(method) = class_override_method(obj, "__iter__") {
        let iter = ferrython_core::object::call_callable(&method, &[])?;
        if iter.get_attr("__next__").is_none() {
            return Err(PyException::type_error(format!(
                "iter() returned non-iterator of type '{}'",
                iter.type_name()
            )));
        }
        let mut items = Vec::new();
        loop {
            let next = iter.get_attr("__next__").ok_or_else(|| {
                PyException::type_error(format!("'{}' object is not an iterator", iter.type_name()))
            })?;
            match ferrython_core::object::call_callable(&next, &[]) {
                Ok(value) => items.push(value),
                Err(err) if err.kind == ExceptionKind::StopIteration => break,
                Err(err) => return Err(err),
            }
        }
        return Ok(items);
    }

    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return Ok(vec![]);
    };
    let data = inst.attrs.read().get("_data").cloned();
    if let Some(data) = data {
        data.to_list()
    } else {
        Ok(vec![])
    }
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
        PyObjectPayload::Function(func) => {
            if let Some((module, name)) = pickle_global_function_parts(obj, func) {
                pickle_serialize_function_global_p0(&module, &name, buf);
            } else {
                return Err(PyException::runtime_error(format!(
                    "PicklingError: can't pickle object of type {}",
                    obj.type_name()
                )));
            }
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
        PyObjectPayload::Range(rd) => {
            buf.extend_from_slice(b"cbuiltins\nrange\n(");
            for arg in range_pickle_args(rd) {
                pickle_serialize_p0(&arg, buf, memo)?;
            }
            buf.extend_from_slice(b"tR");
        }
        PyObjectPayload::Instance(inst) => {
            if inst.attrs.read().contains_key("__csv_dialect__") {
                return Err(PyException::type_error("cannot pickle 'Dialect' instances"));
            }
            if let Some((factory, items)) = defaultdict_pickle_parts(inst) {
                pickle_serialize_defaultdict_p0(&factory, &items, buf, memo)?;
                return Ok(());
            }
            let (module_name, class_name, data_pairs) = pickle_extract_instance(obj, inst)?;
            buf.push(b'c');
            buf.extend_from_slice(module_name.as_bytes());
            buf.push(b'\n');
            buf.extend_from_slice(class_name.as_bytes());
            buf.push(b'\n');
            buf.extend_from_slice(b"(tR");
            p0_emit_put_obj(obj, buf, memo);
            let state = instance_state_dict(&data_pairs);
            pickle_serialize_p0(&state, buf, memo)?;
            buf.push(b'b');
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
        PyObjectPayload::DequeIter(data) => {
            let idx = data.index.get();
            buf.extend_from_slice(b"cbuiltins\n__ferrython_dequeiter__\n(");
            pickle_serialize_p0(&data.source, buf, memo)?;
            pickle_serialize_p0(&PyObject::int(idx as i64), buf, memo)?;
            pickle_serialize_p0(&PyObject::bool_val(idx == usize::MAX), buf, memo)?;
            pickle_serialize_p0(&PyObject::bool_val(data.reverse), buf, memo)?;
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
            buf.extend_from_slice(b"cbuiltins\n__ferrython_rangeiter__\n(");
            pickle_serialize_p0(&PyObject::int(ri.current.get()), buf, memo)?;
            pickle_serialize_p0(&PyObject::int(ri.stop), buf, memo)?;
            pickle_serialize_p0(&PyObject::int(ri.step), buf, memo)?;
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
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_rangeiter__\n(");
                    pickle_serialize_p0(&PyObject::int(*current), buf, memo)?;
                    pickle_serialize_p0(&PyObject::int(*stop), buf, memo)?;
                    pickle_serialize_p0(&PyObject::int(*step), buf, memo)?;
                    buf.extend_from_slice(b"tR");
                    return Ok(());
                }
                IteratorData::BigRange(iter) => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_rangeiter__\n(");
                    pickle_serialize_p0(
                        &py_int_from_bigint(range_iter_item_bigint(iter)),
                        buf,
                        memo,
                    )?;
                    pickle_serialize_p0(&py_int_from_bigint(iter.stop.clone()), buf, memo)?;
                    pickle_serialize_p0(&py_int_from_bigint(iter.step.clone()), buf, memo)?;
                    buf.extend_from_slice(b"tR");
                    return Ok(());
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
                IteratorData::Islice {
                    source,
                    index,
                    next_yield,
                    stop,
                    step,
                } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_islice__\n(");
                    pickle_serialize_p0(source, buf, memo)?;
                    pickle_serialize_p0(&PyObject::int(*index as i64), buf, memo)?;
                    pickle_serialize_p0(&PyObject::int(*next_yield as i64), buf, memo)?;
                    pickle_serialize_p0(&PyObject::int(*stop as i64), buf, memo)?;
                    pickle_serialize_p0(&PyObject::int(*step as i64), buf, memo)?;
                    buf.extend_from_slice(b"tR");
                    return Ok(());
                }
                IteratorData::ZipLongest {
                    sources,
                    active,
                    fillvalue,
                    ..
                } => {
                    let active_flags: Vec<PyObjectRef> = active
                        .iter()
                        .map(|flag| PyObject::bool_val(*flag))
                        .collect();
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_ziplongest__\n(");
                    pickle_serialize_p0(&PyObject::list(sources.clone()), buf, memo)?;
                    pickle_serialize_p0(&PyObject::list(active_flags), buf, memo)?;
                    pickle_serialize_p0(fillvalue, buf, memo)?;
                    buf.extend_from_slice(b"tR");
                    return Ok(());
                }
                IteratorData::TakeWhile { func, source, done } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_takewhile__\n(");
                    pickle_serialize_p0(func, buf, memo)?;
                    pickle_serialize_p0(source, buf, memo)?;
                    pickle_serialize_p0(&PyObject::bool_val(*done), buf, memo)?;
                    buf.extend_from_slice(b"tR");
                    return Ok(());
                }
                IteratorData::DropWhile {
                    func,
                    source,
                    dropping,
                } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_dropwhile__\n(");
                    pickle_serialize_p0(func, buf, memo)?;
                    pickle_serialize_p0(source, buf, memo)?;
                    pickle_serialize_p0(&PyObject::bool_val(*dropping), buf, memo)?;
                    buf.extend_from_slice(b"tR");
                    return Ok(());
                }
                IteratorData::Tee {
                    source,
                    buffer,
                    index,
                    ..
                } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_tee__\n(");
                    pickle_serialize_p0(&source.read().clone(), buf, memo)?;
                    pickle_serialize_p0(&PyObject::list(buffer.read().clone()), buf, memo)?;
                    pickle_serialize_p0(&PyObject::int(*index as i64), buf, memo)?;
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
        let is_deque = inst.attrs.read().contains_key("__deque__");
        let deque_items = if is_deque {
            Some(deque_pickle_items(obj)?)
        } else {
            None
        };
        let attrs_r = inst.attrs.read();
        for (k, v) in attrs_r.iter() {
            match &v.payload {
                PyObjectPayload::NativeFunction(_)
                | PyObjectPayload::NativeClosure(_)
                | PyObjectPayload::Function(_)
                | PyObjectPayload::Class(_) => continue,
                PyObjectPayload::List(_) if is_deque && k.as_str() == "_data" => {
                    let items = deque_items.clone().unwrap_or_default();
                    data_pairs.push((k.clone(), PyObject::list(items)));
                }
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
        PyObjectPayload::Function(func) => {
            if let Some((module, name)) = pickle_global_function_parts(obj, func) {
                pickle_serialize_function_global_p2(&module, &name, buf);
            } else {
                return Err(PyException::runtime_error(format!(
                    "PicklingError: can't pickle object of type {}",
                    obj.type_name()
                )));
            }
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
        PyObjectPayload::Range(rd) => {
            buf.extend_from_slice(b"cbuiltins\nrange\n");
            pickle_serialize_p2(&PyObject::tuple(range_pickle_args(rd)), buf, memo)?;
            buf.push(b'R');
        }
        PyObjectPayload::Instance(inst) => {
            if inst.attrs.read().contains_key("__csv_dialect__") {
                return Err(PyException::type_error("cannot pickle 'Dialect' instances"));
            }
            if let Some((factory, items)) = defaultdict_pickle_parts(inst) {
                pickle_serialize_defaultdict_p2(&factory, &items, buf, memo)?;
                return Ok(());
            }
            let (module_name, class_name, data_pairs) = pickle_extract_instance(obj, inst)?;
            buf.push(b'c');
            buf.extend_from_slice(module_name.as_bytes());
            buf.push(b'\n');
            buf.extend_from_slice(class_name.as_bytes());
            buf.push(b'\n');
            pickle_serialize_p2(&PyObject::tuple(vec![]), buf, memo)?;
            buf.push(b'R');
            p2_emit_put_obj(obj, buf, memo);
            let state = instance_state_dict(&data_pairs);
            pickle_serialize_p2(&state, buf, memo)?;
            buf.push(b'b');
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
        PyObjectPayload::DequeIter(data) => {
            let idx = data.index.get();
            buf.extend_from_slice(b"cbuiltins\n__ferrython_dequeiter__\n");
            pickle_serialize_p2(
                &PyObject::tuple(vec![
                    data.source.clone(),
                    PyObject::int(idx as i64),
                    PyObject::bool_val(idx == usize::MAX),
                    PyObject::bool_val(data.reverse),
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
            buf.extend_from_slice(b"cbuiltins\n__ferrython_rangeiter__\n");
            pickle_serialize_p2(
                &PyObject::tuple(vec![
                    PyObject::int(ri.current.get()),
                    PyObject::int(ri.stop),
                    PyObject::int(ri.step),
                ]),
                buf,
                memo,
            )?;
            buf.push(b'R');
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
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_rangeiter__\n");
                    pickle_serialize_p2(
                        &PyObject::tuple(vec![
                            PyObject::int(*current),
                            PyObject::int(*stop),
                            PyObject::int(*step),
                        ]),
                        buf,
                        memo,
                    )?;
                    buf.push(b'R');
                    return Ok(());
                }
                IteratorData::BigRange(iter) => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_rangeiter__\n");
                    pickle_serialize_p2(
                        &PyObject::tuple(vec![
                            py_int_from_bigint(range_iter_item_bigint(iter)),
                            py_int_from_bigint(iter.stop.clone()),
                            py_int_from_bigint(iter.step.clone()),
                        ]),
                        buf,
                        memo,
                    )?;
                    buf.push(b'R');
                    return Ok(());
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
                IteratorData::Islice {
                    source,
                    index,
                    next_yield,
                    stop,
                    step,
                } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_islice__\n");
                    pickle_serialize_p2(
                        &PyObject::tuple(vec![
                            source.clone(),
                            PyObject::int(*index as i64),
                            PyObject::int(*next_yield as i64),
                            PyObject::int(*stop as i64),
                            PyObject::int(*step as i64),
                        ]),
                        buf,
                        memo,
                    )?;
                    buf.push(b'R');
                    return Ok(());
                }
                IteratorData::ZipLongest {
                    sources,
                    active,
                    fillvalue,
                    ..
                } => {
                    let active_flags: Vec<PyObjectRef> = active
                        .iter()
                        .map(|flag| PyObject::bool_val(*flag))
                        .collect();
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_ziplongest__\n");
                    pickle_serialize_p2(
                        &PyObject::tuple(vec![
                            PyObject::list(sources.clone()),
                            PyObject::list(active_flags),
                            fillvalue.clone(),
                        ]),
                        buf,
                        memo,
                    )?;
                    buf.push(b'R');
                    return Ok(());
                }
                IteratorData::TakeWhile { func, source, done } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_takewhile__\n");
                    pickle_serialize_p2(
                        &PyObject::tuple(vec![
                            func.clone(),
                            source.clone(),
                            PyObject::bool_val(*done),
                        ]),
                        buf,
                        memo,
                    )?;
                    buf.push(b'R');
                    return Ok(());
                }
                IteratorData::DropWhile {
                    func,
                    source,
                    dropping,
                } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_dropwhile__\n");
                    pickle_serialize_p2(
                        &PyObject::tuple(vec![
                            func.clone(),
                            source.clone(),
                            PyObject::bool_val(*dropping),
                        ]),
                        buf,
                        memo,
                    )?;
                    buf.push(b'R');
                    return Ok(());
                }
                IteratorData::Tee {
                    source,
                    buffer,
                    index,
                    ..
                } => {
                    buf.extend_from_slice(b"cbuiltins\n__ferrython_tee__\n");
                    pickle_serialize_p2(
                        &PyObject::tuple(vec![
                            source.read().clone(),
                            PyObject::list(buffer.read().clone()),
                            PyObject::int(*index as i64),
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
