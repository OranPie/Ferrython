use compact_str::CompactString;
use ferrython_core::object::{make_builtin, PyObject, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;

pub(super) fn create_module_functions() -> (
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
) {
    // current_thread() — return Thread-like object
    let current_thread_fn =
        PyObject::native_closure("current_thread", move |_: &[PyObjectRef]| {
            let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref i) = inst.payload {
                let mut attrs = i.attrs.write();
                attrs.insert(
                    CompactString::from("name"),
                    PyObject::str_val(CompactString::from("MainThread")),
                );
                attrs.insert(CompactString::from("ident"), PyObject::int(1));
                attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
                attrs.insert(
                    CompactString::from("is_alive"),
                    make_builtin(|_| Ok(PyObject::bool_val(true))),
                );
                attrs.insert(
                    CompactString::from("getName"),
                    make_builtin(|_| Ok(PyObject::str_val(CompactString::from("MainThread")))),
                );
            }
            Ok(inst)
        });

    // active_count() — return count of active threads
    let active_count_fn = make_builtin(|_| Ok(PyObject::int(1)));

    // enumerate() — return list of active threads
    let enumerate_fn = PyObject::native_closure("enumerate", move |_: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
        let main = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref i) = main.payload {
            let mut attrs = i.attrs.write();
            attrs.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from("MainThread")),
            );
            attrs.insert(CompactString::from("ident"), PyObject::int(1));
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            attrs.insert(
                CompactString::from("is_alive"),
                make_builtin(|_| Ok(PyObject::bool_val(true))),
            );
        }
        Ok(PyObject::list(vec![main]))
    });

    let main_thread_fn = make_builtin(|_| {
        let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref i) = inst.payload {
            let mut attrs = i.attrs.write();
            attrs.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from("MainThread")),
            );
            attrs.insert(CompactString::from("ident"), PyObject::int(1));
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            attrs.insert(
                CompactString::from("is_alive"),
                make_builtin(|_| Ok(PyObject::bool_val(true))),
            );
        }
        Ok(inst)
    });

    let local_fn = make_builtin(|_| {
        let cls = PyObject::class(CompactString::from("local"), vec![], IndexMap::new());
        Ok(PyObject::instance(cls))
    });

    let get_ident_fn = make_builtin(|_| {
        let tid = std::thread::current().id();
        let id_str = format!("{:?}", tid);
        let num: i64 = id_str
            .trim_start_matches("ThreadId(")
            .trim_end_matches(')')
            .parse()
            .unwrap_or(1);
        Ok(PyObject::int(num))
    });

    let get_native_id_fn = make_builtin(|_| Ok(PyObject::int(std::process::id() as i64)));

    let stack_size_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            Ok(PyObject::int(0))
        } else {
            Ok(PyObject::int(0))
        }
    });

    (
        current_thread_fn,
        active_count_fn,
        enumerate_fn,
        main_thread_fn,
        local_fn,
        get_ident_fn,
        get_native_id_fn,
        stack_size_fn,
    )
}
