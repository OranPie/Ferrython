use super::*;

type WeakSetStorage = Rc<PyCell<IndexMap<usize, PyWeakRef>>>;

pub(crate) fn make_weak_set_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__new__"),
        PyObject::native_function("WeakSet.__new__", weak_set_new),
    );
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_function("WeakSet.__init__", weak_set_init),
    );
    ns.insert(CompactString::from("__hash__"), PyObject::none());
    for name in [
        "__or__", "__and__", "__sub__", "__xor__", "__ior__", "__iand__", "__isub__", "__ixor__",
    ] {
        let method_name = name.to_string();
        ns.insert(
            CompactString::from(name),
            PyObject::native_closure(&format!("WeakSet.{name}"), move |args| {
                weak_set_call_instance_method(&method_name, args)
            }),
        );
    }
    PyObject::class(CompactString::from("WeakSet"), vec![], ns)
}

fn weak_set_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("WeakSet.__new__ requires a class"));
    }
    if args.len() > 2 {
        return Err(PyException::type_error(
            "WeakSet expected at most 1 argument",
        ));
    }
    install_weak_set_storage(&args[0], args.get(1))
}

fn weak_set_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("WeakSet.__init__ requires self"));
    }
    if args.len() > 2 {
        return Err(PyException::type_error(
            "WeakSet expected at most 1 argument",
        ));
    }
    let this = &args[0];
    if !weak_set_is_instance(this) {
        return Err(PyException::type_error(
            "WeakSet.__init__ requires a WeakSet instance",
        ));
    }
    if let Some(clear) = this.get_attr("clear") {
        call_callable(&clear, &[])?;
    }
    if let Some(data) = args.get(1) {
        if let Some(update) = this.get_attr("update") {
            call_callable(&update, &[data.clone()])?;
        }
    }
    Ok(PyObject::none())
}

fn install_weak_set_storage(
    cls: &PyObjectRef,
    data: Option<&PyObjectRef>,
) -> PyResult<PyObjectRef> {
    let storage: WeakSetStorage = Rc::new(PyCell::new(IndexMap::new()));
    let inst = PyObject::instance(cls.clone());
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__weakset__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("data"), weak_set_snapshot(&storage));

        let s1 = storage.clone();
        attrs.insert(
            CompactString::from("add"),
            PyObject::native_closure("WeakSet.add", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("add() requires an argument"));
                }
                weak_set_add_item(&s1, args[0].clone())?;
                Ok(PyObject::none())
            }),
        );

        let s_clear = storage.clone();
        attrs.insert(
            CompactString::from("clear"),
            PyObject::native_closure("WeakSet.clear", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("clear() takes no arguments"));
                }
                s_clear.write().clear();
                Ok(PyObject::none())
            }),
        );

        let s2 = storage.clone();
        attrs.insert(
            CompactString::from("discard"),
            PyObject::native_closure("WeakSet.discard", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("discard() requires an argument"));
                }
                weak_set_require_weakable(&args[0])?;
                weak_set_remove_equal(&s2, &args[0])?;
                Ok(PyObject::none())
            }),
        );

        let s3 = storage.clone();
        attrs.insert(
            CompactString::from("__contains__"),
            PyObject::native_closure("WeakSet.__contains__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__contains__ requires an argument"));
                }
                if weak_set_require_weakable(&args[0]).is_err() {
                    return Ok(PyObject::bool_val(false));
                }
                Ok(PyObject::bool_val(weak_set_contains_live(&s3, &args[0])?))
            }),
        );

        let s4 = storage.clone();
        attrs.insert(
            CompactString::from("__len__"),
            PyObject::native_closure("WeakSet.__len__", move |_| {
                let mut store = s4.write();
                store.retain(|_, w| w.upgrade().is_some());
                Ok(PyObject::int(store.len() as i64))
            }),
        );

        let s5 = storage.clone();
        attrs.insert(
            CompactString::from("__iter__"),
            PyObject::native_closure("WeakSet.__iter__", move |_| {
                Ok(weak_iter(weak_set_items(&s5)))
            }),
        );

        let s_repr = storage.clone();
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("WeakSet.__repr__", move |_| {
                Ok(PyObject::str_val(CompactString::from(weak_set_repr_text(
                    &s_repr,
                ))))
            }),
        );

        for (name, op) in [
            ("__eq__", "eq"),
            ("__ne__", "ne"),
            ("__le__", "le"),
            ("__lt__", "lt"),
            ("__ge__", "ge"),
            ("__gt__", "gt"),
            ("issubset", "le"),
            ("issuperset", "ge"),
        ] {
            let s = storage.clone();
            attrs.insert(
                CompactString::from(name),
                PyObject::native_closure(&format!("WeakSet.{name}"), move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "set comparison requires an argument",
                        ));
                    }
                    let left = weak_set_items(&s);
                    if !weak_set_is_instance(&args[0]) && matches!(op, "eq" | "ne") {
                        return Ok(PyObject::bool_val(op == "ne"));
                    }
                    let right = weak_set_iterable_items(&args[0])?;
                    let result = match op {
                        "eq" => weak_set_eq_items(&left, &right)?,
                        "ne" => !weak_set_eq_items(&left, &right)?,
                        "le" => weak_set_subset_items(&left, &right, true)?,
                        "lt" => weak_set_subset_items(&left, &right, false)?,
                        "ge" => weak_set_subset_items(&right, &left, true)?,
                        "gt" => weak_set_subset_items(&right, &left, false)?,
                        _ => false,
                    };
                    Ok(PyObject::bool_val(result))
                }),
            );
        }

        let s_disjoint = storage.clone();
        attrs.insert(
            CompactString::from("isdisjoint"),
            PyObject::native_closure("WeakSet.isdisjoint", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("isdisjoint() requires an argument"));
                }
                let left = weak_set_items(&s_disjoint);
                let right = weak_set_iterable_items(&args[0])?;
                for item in left.iter() {
                    if weak_set_contains_item(&right, item)? {
                        return Ok(PyObject::bool_val(false));
                    }
                }
                Ok(PyObject::bool_val(true))
            }),
        );

        let cls_for_copy = cls.clone();
        let s_copy = storage.clone();
        attrs.insert(
            CompactString::from("copy"),
            PyObject::native_closure("WeakSet.copy", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("copy() takes no arguments"));
                }
                let result = install_weak_set_storage(&cls_for_copy, None)?;
                weak_set_update_instance(&result, weak_set_items(&s_copy))?;
                Ok(result)
            }),
        );

        let cls_for_ops = cls.clone();
        for (name, op) in [
            ("union", "union"),
            ("__or__", "union"),
            ("intersection", "intersection"),
            ("__and__", "intersection"),
            ("difference", "difference"),
            ("__sub__", "difference"),
            ("symmetric_difference", "symmetric_difference"),
            ("__xor__", "symmetric_difference"),
        ] {
            let s = storage.clone();
            let cls = cls_for_ops.clone();
            attrs.insert(
                CompactString::from(name),
                PyObject::native_closure(&format!("WeakSet.{name}"), move |args| {
                    let mut result_items = weak_set_items(&s);
                    for other in args {
                        let other_items = weak_set_weakable_iterable_items(other)?;
                        result_items = weak_set_binary_items(result_items, other_items, op)?;
                    }
                    let result = install_weak_set_storage(&cls, None)?;
                    weak_set_update_instance(&result, result_items)?;
                    Ok(result)
                }),
            );
        }

        for (name, op) in [
            ("update", "union"),
            ("__ior__", "union"),
            ("intersection_update", "intersection"),
            ("__iand__", "intersection"),
            ("difference_update", "difference"),
            ("__isub__", "difference"),
            ("symmetric_difference_update", "symmetric_difference"),
            ("__ixor__", "symmetric_difference"),
        ] {
            let s = storage.clone();
            attrs.insert(
                CompactString::from(name),
                PyObject::native_closure(&format!("WeakSet.{name}"), move |args| {
                    let mut result_items = weak_set_items(&s);
                    for other in args {
                        let other_items = weak_set_weakable_iterable_items(other)?;
                        result_items = weak_set_binary_items(result_items, other_items, op)?;
                    }
                    weak_set_replace(&s, result_items)?;
                    Ok(PyObject::none())
                }),
            );
        }

        let s_remove = storage.clone();
        attrs.insert(
            CompactString::from("remove"),
            PyObject::native_closure("WeakSet.remove", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("remove() requires an argument"));
                }
                weak_set_require_weakable(&args[0])?;
                if weak_set_remove_equal(&s_remove, &args[0])? {
                    Ok(PyObject::none())
                } else {
                    Err(PyException::new(ExceptionKind::KeyError, args[0].repr()))
                }
            }),
        );

        let s_pop = storage.clone();
        attrs.insert(
            CompactString::from("pop"),
            PyObject::native_closure("WeakSet.pop", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("pop() takes no arguments"));
                }
                let mut store = s_pop.write();
                store.retain(|_, w| w.upgrade().is_some());
                let Some(ptr) = store.keys().next().cloned() else {
                    return Err(PyException::new(
                        ExceptionKind::KeyError,
                        "pop from empty WeakSet",
                    ));
                };
                let weak = store.shift_remove(&ptr).unwrap();
                Ok(weak.upgrade().unwrap_or_else(PyObject::none))
            }),
        );
    }
    if let Some(data) = data {
        weak_set_update_instance(&inst, data.to_list()?)?;
    }
    refresh_weak_set_data_attr(&inst, &storage);
    Ok(inst)
}

fn weak_set_is_instance(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::Instance(inst) if inst.attrs.read().contains_key("__weakset__"))
}

fn weak_set_call_instance_method(method_name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(format!(
            "{method_name} requires self"
        )));
    }
    if !weak_set_is_instance(&args[0]) {
        return Ok(PyObject::not_implemented());
    }
    let method = args[0].get_attr(method_name).ok_or_else(|| {
        PyException::attribute_error(format!("WeakSet has no attribute '{method_name}'"))
    })?;
    let result = call_callable(&method, &args[1..])?;
    if matches!(
        method_name,
        "__ior__" | "__iand__" | "__isub__" | "__ixor__"
    ) {
        Ok(args[0].clone())
    } else {
        Ok(result)
    }
}

fn weak_set_snapshot(storage: &WeakSetStorage) -> PyObjectRef {
    let mut map = IndexMap::new();
    for item in weak_set_items(storage) {
        map.insert(
            PyObjectRef::as_ptr(&item) as usize,
            PyObjectRef::downgrade(&item),
        );
    }
    let cls = PyObject::class(CompactString::from("WeakSetData"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref data) = inst.payload {
        let text = weak_set_repr_text(storage);
        data.attrs.write().insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("WeakSetData.__repr__", move |_| {
                Ok(PyObject::str_val(CompactString::from(text.as_str())))
            }),
        );
    }
    inst
}

fn weak_set_repr_text(storage: &WeakSetStorage) -> String {
    let mut live: Vec<String> = weak_set_items(storage)
        .iter()
        .map(|item| item.repr())
        .collect();
    live.sort();
    format!("{{{}}}", live.join(", "))
}

fn refresh_weak_set_data_attr(inst: &PyObjectRef, storage: &WeakSetStorage) {
    if let PyObjectPayload::Instance(data) = &inst.payload {
        data.attrs
            .write()
            .insert(CompactString::from("data"), weak_set_snapshot(storage));
    }
}

fn weak_set_items(storage: &WeakSetStorage) -> Vec<PyObjectRef> {
    let mut store = storage.write();
    store.retain(|_, w| w.upgrade().is_some());
    store.values().filter_map(|w| w.upgrade()).collect()
}

fn weak_set_find_equal(storage: &WeakSetStorage, needle: &PyObjectRef) -> PyResult<Option<usize>> {
    let live: Vec<(usize, PyObjectRef)> = {
        let mut store = storage.write();
        store.retain(|_, w| w.upgrade().is_some());
        store
            .iter()
            .filter_map(|(ptr, weak)| weak.upgrade().map(|item| (*ptr, item)))
            .collect()
    };
    for (ptr, item) in live {
        if PyObjectRef::ptr_eq(&item, needle) || item.compare(needle, CompareOp::Eq)?.is_truthy() {
            return Ok(Some(ptr));
        }
    }
    Ok(None)
}

fn weak_set_contains_live(storage: &WeakSetStorage, needle: &PyObjectRef) -> PyResult<bool> {
    Ok(weak_set_find_equal(storage, needle)?.is_some())
}

fn weak_set_remove_equal(storage: &WeakSetStorage, needle: &PyObjectRef) -> PyResult<bool> {
    let Some(ptr) = weak_set_find_equal(storage, needle)? else {
        return Ok(false);
    };
    Ok(storage.write().shift_remove(&ptr).is_some())
}

fn weak_set_require_weakable(item: &PyObjectRef) -> PyResult<()> {
    match &item.payload {
        PyObjectPayload::Int(_)
        | PyObjectPayload::Bool(_)
        | PyObjectPayload::Float(_)
        | PyObjectPayload::Complex { .. }
        | PyObjectPayload::Str(_)
        | PyObjectPayload::Bytes(_)
        | PyObjectPayload::ByteArray(_)
        | PyObjectPayload::Tuple(_)
        | PyObjectPayload::List(_)
        | PyObjectPayload::Dict(_)
        | PyObjectPayload::Set(_)
        | PyObjectPayload::FrozenSet(_) => Err(PyException::type_error(format!(
            "cannot create weak reference to '{}' object",
            item.type_name()
        ))),
        _ => Ok(()),
    }
}

fn weak_set_add_item(storage: &WeakSetStorage, item: PyObjectRef) -> PyResult<()> {
    weak_set_require_weakable(&item)?;
    let ptr = PyObjectRef::as_ptr(&item) as usize;
    storage.write().insert(ptr, PyObjectRef::downgrade(&item));
    Ok(())
}

fn weak_set_replace(storage: &WeakSetStorage, items: Vec<PyObjectRef>) -> PyResult<()> {
    storage.write().clear();
    for item in items {
        weak_set_add_item(storage, item)?;
    }
    Ok(())
}

fn weak_set_update_instance(inst: &PyObjectRef, items: Vec<PyObjectRef>) -> PyResult<()> {
    let Some(update) = inst.get_attr("update") else {
        return Ok(());
    };
    call_callable(&update, &[PyObject::list(items)])?;
    Ok(())
}

fn weak_set_iterable_items(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if inst.attrs.read().contains_key("__weakset__") {
            if let Some(iter_fn) = obj.get_attr("__iter__") {
                return call_callable(&iter_fn, &[])?.to_list();
            }
        }
    }
    obj.to_list()
}

fn weak_set_weakable_iterable_items(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    let items = weak_set_iterable_items(obj)?;
    for item in &items {
        weak_set_require_weakable(item)?;
    }
    Ok(items)
}

fn weak_set_contains_item(items: &[PyObjectRef], needle: &PyObjectRef) -> PyResult<bool> {
    for item in items {
        if PyObjectRef::ptr_eq(item, needle) || item.compare(needle, CompareOp::Eq)?.is_truthy() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn weak_set_eq_items(left: &[PyObjectRef], right: &[PyObjectRef]) -> PyResult<bool> {
    if left.len() != right.len() {
        return Ok(false);
    }
    weak_set_subset_items(left, right, true)
}

fn weak_set_subset_items(
    left: &[PyObjectRef],
    right: &[PyObjectRef],
    allow_equal: bool,
) -> PyResult<bool> {
    if (!allow_equal && left.len() >= right.len()) || (allow_equal && left.len() > right.len()) {
        return Ok(false);
    }
    for item in left {
        if !weak_set_contains_item(right, item)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn weak_set_binary_items(
    left: Vec<PyObjectRef>,
    right: Vec<PyObjectRef>,
    op: &str,
) -> PyResult<Vec<PyObjectRef>> {
    let mut result = Vec::new();
    match op {
        "union" => {
            result = left;
            for item in right {
                if !weak_set_contains_item(&result, &item)? {
                    result.push(item);
                }
            }
        }
        "intersection" => {
            for item in right {
                if weak_set_contains_item(&left, &item)? {
                    result.push(item);
                }
            }
        }
        "difference" => {
            for item in left {
                if !weak_set_contains_item(&right, &item)? {
                    result.push(item);
                }
            }
        }
        "symmetric_difference" => {
            for item in left.iter() {
                if !weak_set_contains_item(&right, item)? {
                    result.push(item.clone());
                }
            }
            for item in right {
                if !weak_set_contains_item(&left, &item)? {
                    result.push(item);
                }
            }
        }
        _ => {}
    }
    Ok(result)
}
