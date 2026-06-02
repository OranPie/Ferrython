use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    call_callable, check_args, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

pub fn create_graphlib_module() -> PyObjectRef {
    let cycle_error = PyObject::class(
        CompactString::from("CycleError"),
        vec![PyObject::exception_type(ExceptionKind::ValueError)],
        IndexMap::new(),
    );
    make_module(
        "graphlib",
        vec![
            ("CycleError", cycle_error),
            ("TopologicalSorter", make_topological_sorter_class()),
        ],
    )
}

fn make_topological_sorter_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("TopologicalSorter.__init__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "TopologicalSorter.__init__ requires self",
                ));
            }
            let self_obj = &args[0];
            set_attr(self_obj, "_node2info", PyObject::dict(IndexMap::new()))?;
            set_attr(self_obj, "_ready_nodes", PyObject::none())?;
            set_attr(self_obj, "_npassedout", PyObject::int(0))?;
            set_attr(self_obj, "_nfinished", PyObject::int(0))?;
            set_attr(self_obj, "_prepared", PyObject::bool_val(false))?;
            if let Some(graph) = args.get(1) {
                if !matches!(graph.payload, PyObjectPayload::None) {
                    for (node, preds) in mapping_items(graph)? {
                        let mut add_args = vec![self_obj.clone(), node];
                        add_args.extend(preds.to_list().unwrap_or_default());
                        let _ = graphlib_add(&add_args)?;
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("add"),
        PyObject::native_function("TopologicalSorter.add", graphlib_add),
    );
    ns.insert(
        CompactString::from("prepare"),
        PyObject::native_function("TopologicalSorter.prepare", graphlib_prepare),
    );
    ns.insert(
        CompactString::from("is_active"),
        PyObject::native_function("TopologicalSorter.is_active", graphlib_is_active),
    );
    ns.insert(
        CompactString::from("get_ready"),
        PyObject::native_function("TopologicalSorter.get_ready", graphlib_get_ready),
    );
    ns.insert(
        CompactString::from("done"),
        PyObject::native_function("TopologicalSorter.done", graphlib_done),
    );
    ns.insert(
        CompactString::from("static_order"),
        PyObject::native_function("TopologicalSorter.static_order", graphlib_static_order),
    );
    PyObject::class(CompactString::from("TopologicalSorter"), vec![], ns)
}

fn graphlib_add(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("add requires node"));
    }
    let self_obj = &args[0];
    if get_attr(self_obj, "_prepared")?.is_truthy() {
        return Err(PyException::value_error(
            "Nodes cannot be added after a call to prepare()",
        ));
    }
    ensure_node(self_obj, &args[1])?;
    for pred in &args[2..] {
        ensure_node(self_obj, pred)?;
        add_predecessor(self_obj, &args[1], pred)?;
    }
    Ok(PyObject::none())
}

fn graphlib_prepare(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("TopologicalSorter.prepare", args, 1)?;
    let self_obj = &args[0];
    if get_attr(self_obj, "_prepared")?.is_truthy() {
        return Err(PyException::value_error("cannot prepare() more than once"));
    }
    detect_cycle(self_obj)?;
    set_attr(self_obj, "_prepared", PyObject::bool_val(true))?;
    let mut ready = Vec::new();
    for (node, info) in node_items(self_obj)? {
        if info_nremaining(&info)? == 0 {
            ready.push(node);
        }
    }
    set_attr(self_obj, "_ready_nodes", PyObject::list(ready))?;
    Ok(PyObject::none())
}

fn graphlib_is_active(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("TopologicalSorter.is_active", args, 1)?;
    ensure_prepared(&args[0])?;
    let total = node_items(&args[0])?.len() as i64;
    let finished = get_attr(&args[0], "_nfinished")?.to_int()?;
    Ok(PyObject::bool_val(finished < total))
}

fn graphlib_get_ready(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("TopologicalSorter.get_ready", args, 1)?;
    let self_obj = &args[0];
    ensure_prepared(self_obj)?;
    let ready_obj = get_attr(self_obj, "_ready_nodes")?;
    let ready = ready_obj.to_list().unwrap_or_default();
    let passed = get_attr(self_obj, "_npassedout")?.to_int()? + ready.len() as i64;
    set_attr(self_obj, "_npassedout", PyObject::int(passed))?;
    set_attr(self_obj, "_ready_nodes", PyObject::list(vec![]))?;
    Ok(PyObject::tuple(ready))
}

fn graphlib_done(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("done requires self"));
    }
    let self_obj = &args[0];
    ensure_prepared(self_obj)?;
    for node in &args[1..] {
        if get_info(self_obj, node).is_none() {
            return Err(PyException::value_error(format!(
                "node {} was not added using add()",
                node.repr()
            )));
        }
        let finished = get_attr(self_obj, "_nfinished")?.to_int()? + 1;
        set_attr(self_obj, "_nfinished", PyObject::int(finished))?;
        for (_other_node, info) in node_items(self_obj)? {
            if predecessor_contains(&info, node)? {
                decrement_info(&info)?;
                if info_nremaining(&info)? == 0 {
                    append_ready(self_obj, _other_node)?;
                }
            }
        }
    }
    Ok(PyObject::none())
}

fn graphlib_static_order(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("TopologicalSorter.static_order", args, 1)?;
    let self_obj = args[0].clone();
    let _ = graphlib_prepare(&[self_obj.clone()])?;
    let mut result = Vec::new();
    while graphlib_is_active(&[self_obj.clone()])?.is_truthy() {
        let ready_obj = graphlib_get_ready(&[self_obj.clone()])?;
        let ready = ready_obj.to_list().unwrap_or_default();
        result.extend(ready.iter().cloned());
        let mut done_args = vec![self_obj.clone()];
        done_args.extend(ready);
        let _ = graphlib_done(&done_args)?;
    }
    Ok(PyObject::list(result))
}

fn set_attr(obj: &PyObjectRef, name: &str, value: PyObjectRef) -> PyResult<()> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs.write().insert(CompactString::from(name), value);
        Ok(())
    } else {
        Err(PyException::type_error("expected instance"))
    }
}

fn get_attr(obj: &PyObjectRef, name: &str) -> PyResult<PyObjectRef> {
    obj.get_attr(name)
        .ok_or_else(|| PyException::attribute_error(name))
}

fn mapping_items(obj: &PyObjectRef) -> PyResult<Vec<(PyObjectRef, PyObjectRef)>> {
    if let PyObjectPayload::Dict(map) = &obj.payload {
        return Ok(map
            .read()
            .iter()
            .map(|(k, v)| (k.to_object(), v.clone()))
            .collect());
    }
    if let Some(items_fn) = obj.get_attr("items") {
        let items = call_callable(&items_fn, &[])?;
        let mut out = Vec::new();
        for pair_obj in items.to_list()? {
            let pair = pair_obj.to_list()?;
            if pair.len() >= 2 {
                out.push((pair[0].clone(), pair[1].clone()));
            }
        }
        return Ok(out);
    }
    Err(PyException::type_error("expected mapping"))
}

fn node_key(node: &PyObjectRef) -> PyResult<HashableKey> {
    node.to_hashable_key()
}

fn node_dict(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    get_attr(obj, "_node2info")
}

fn node_items(obj: &PyObjectRef) -> PyResult<Vec<(PyObjectRef, PyObjectRef)>> {
    let dict = node_dict(obj)?;
    if let PyObjectPayload::Dict(map) = &dict.payload {
        Ok(map
            .read()
            .iter()
            .map(|(key, value)| (key.to_object(), value.clone()))
            .collect())
    } else {
        Err(PyException::type_error("expected node dict"))
    }
}

fn get_info(obj: &PyObjectRef, node: &PyObjectRef) -> Option<PyObjectRef> {
    let dict = node_dict(obj).ok()?;
    let PyObjectPayload::Dict(map) = &dict.payload else {
        return None;
    };
    map.read().get(&node_key(node).ok()?).cloned()
}

fn ensure_node(obj: &PyObjectRef, node: &PyObjectRef) -> PyResult<PyObjectRef> {
    if let Some(info) = get_info(obj, node) {
        return Ok(info);
    }
    let info = PyObject::list(vec![PyObject::list(vec![]), PyObject::int(0)]);
    let dict = node_dict(obj)?;
    let PyObjectPayload::Dict(map) = &dict.payload else {
        return Err(PyException::type_error("expected node dict"));
    };
    map.write().insert(node_key(node)?, info.clone());
    Ok(info)
}

fn info_predecessors(info: &PyObjectRef) -> PyResult<PyObjectRef> {
    let items = info.to_list()?;
    items
        .first()
        .cloned()
        .ok_or_else(|| PyException::runtime_error("invalid node info"))
}

fn info_nremaining(info: &PyObjectRef) -> PyResult<i64> {
    let items = info.to_list()?;
    items
        .get(1)
        .ok_or_else(|| PyException::runtime_error("invalid node info"))?
        .to_int()
}

fn set_info_nremaining(info: &PyObjectRef, value: i64) -> PyResult<()> {
    if let PyObjectPayload::List(items) = &info.payload {
        let mut items = items.write();
        if items.len() < 2 {
            return Err(PyException::runtime_error("invalid node info"));
        }
        items[1] = PyObject::int(value);
        Ok(())
    } else {
        Err(PyException::runtime_error("invalid node info"))
    }
}

fn predecessor_contains(info: &PyObjectRef, pred: &PyObjectRef) -> PyResult<bool> {
    let preds = info_predecessors(info)?.to_list()?;
    let pred_key = node_key(pred)?;
    Ok(preds
        .iter()
        .filter_map(|item| node_key(item).ok())
        .any(|key| key == pred_key))
}

fn add_predecessor(obj: &PyObjectRef, node: &PyObjectRef, pred: &PyObjectRef) -> PyResult<()> {
    let info = ensure_node(obj, node)?;
    if predecessor_contains(&info, pred)? {
        return Ok(());
    }
    let preds = info_predecessors(&info)?;
    if let PyObjectPayload::List(items) = &preds.payload {
        items.write().push(pred.clone());
    }
    set_info_nremaining(&info, info_nremaining(&info)? + 1)
}

fn decrement_info(info: &PyObjectRef) -> PyResult<()> {
    set_info_nremaining(info, info_nremaining(info)? - 1)
}

fn append_ready(obj: &PyObjectRef, node: PyObjectRef) -> PyResult<()> {
    let ready = get_attr(obj, "_ready_nodes")?;
    if let PyObjectPayload::List(items) = &ready.payload {
        items.write().push(node);
    }
    Ok(())
}

fn ensure_prepared(obj: &PyObjectRef) -> PyResult<()> {
    if get_attr(obj, "_prepared")?.is_truthy() {
        Ok(())
    } else {
        Err(PyException::value_error("prepare() must be called first"))
    }
}

fn detect_cycle(obj: &PyObjectRef) -> PyResult<()> {
    let items = node_items(obj)?;
    let mut visiting: Vec<HashableKey> = Vec::new();
    let mut visited: Vec<HashableKey> = Vec::new();
    for (node, _) in items {
        dfs_cycle(obj, &node, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn dfs_cycle(
    obj: &PyObjectRef,
    node: &PyObjectRef,
    visiting: &mut Vec<HashableKey>,
    visited: &mut Vec<HashableKey>,
) -> PyResult<()> {
    let key = node_key(node)?;
    if visited.contains(&key) {
        return Ok(());
    }
    if visiting.contains(&key) {
        return Err(PyException::value_error("nodes are in a cycle"));
    }
    visiting.push(key.clone());
    if let Some(info) = get_info(obj, node) {
        for pred in info_predecessors(&info)?.to_list()? {
            dfs_cycle(obj, &pred, visiting, visited)?;
        }
    }
    visiting.retain(|existing| existing != &key);
    visited.push(key);
    Ok(())
}
