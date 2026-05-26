use super::*;

pub(crate) fn make_weak_set(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let storage: Rc<PyCell<IndexMap<usize, PyWeakRef>>> = Rc::new(PyCell::new(IndexMap::new()));

    let cls = PyObject::class(CompactString::from("WeakSet"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();

        let s1 = storage.clone();
        attrs.insert(
            CompactString::from("add"),
            PyObject::native_closure("WeakSet.add", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("add() requires an argument"));
                }
                let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                let weak = PyObjectRef::downgrade(&args[0]);
                s1.write().insert(ptr, weak);
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
                let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                s2.write().shift_remove(&ptr);
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
                let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                let mut store = s3.write();
                match store.get(&ptr) {
                    Some(weak) => {
                        if weak.upgrade().is_some() {
                            Ok(PyObject::bool_val(true))
                        } else {
                            store.shift_remove(&ptr);
                            Ok(PyObject::bool_val(false))
                        }
                    }
                    None => Ok(PyObject::bool_val(false)),
                }
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
                let mut store = s5.write();
                store.retain(|_, w| w.upgrade().is_some());
                let items: Vec<PyObjectRef> = store.values().filter_map(|w| w.upgrade()).collect();
                Ok(PyObject::list(items))
            }),
        );
    }
    Ok(inst)
}
