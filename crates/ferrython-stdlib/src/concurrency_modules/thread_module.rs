use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

// ── _thread module ──

pub fn create_thread_module() -> PyObjectRef {
    make_module(
        "_thread",
        vec![
            (
                "allocate_lock",
                make_builtin(|_| {
                    let locked = Arc::new(std::sync::Mutex::new(false));
                    let cls = PyObject::class(CompactString::from("lock"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut w = d.attrs.write();
                        let l1 = locked.clone();
                        w.insert(
                            CompactString::from("acquire"),
                            PyObject::native_closure("acquire", move |args: &[PyObjectRef]| {
                                let blocking = args.first().map(|a| a.is_truthy()).unwrap_or(true);
                                let mut guard = l1.lock().unwrap();
                                if *guard {
                                    if !blocking {
                                        return Ok(PyObject::bool_val(false));
                                    }
                                    // In single-threaded context, can't block — return false
                                    return Ok(PyObject::bool_val(false));
                                }
                                *guard = true;
                                Ok(PyObject::bool_val(true))
                            }),
                        );
                        let l2 = locked.clone();
                        w.insert(
                            CompactString::from("release"),
                            PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                                let mut guard = l2.lock().unwrap();
                                if !*guard {
                                    return Err(PyException::runtime_error(
                                        "release unlocked lock",
                                    ));
                                }
                                *guard = false;
                                Ok(PyObject::none())
                            }),
                        );
                        let l3 = locked.clone();
                        w.insert(
                            CompactString::from("locked"),
                            PyObject::native_closure("locked", move |_: &[PyObjectRef]| {
                                Ok(PyObject::bool_val(*l3.lock().unwrap()))
                            }),
                        );
                        let l4 = locked.clone();
                        w.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                                let mut guard = l4.lock().unwrap();
                                *guard = true;
                                Ok(PyObject::bool_val(true))
                            }),
                        );
                        let l5 = locked;
                        w.insert(
                            CompactString::from("__exit__"),
                            PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                                let mut guard = l5.lock().unwrap();
                                *guard = false;
                                Ok(PyObject::none())
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "LockType",
                PyObject::class(CompactString::from("lock"), vec![], IndexMap::new()),
            ),
            (
                "RLock",
                make_builtin(|_| {
                    let state = Rc::new(PyCell::new(0u32));
                    let cls =
                        PyObject::class(CompactString::from("RLock"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut w = d.attrs.write();
                        let s1 = state.clone();
                        w.insert(
                            CompactString::from("acquire"),
                            PyObject::native_closure("RLock.acquire", move |_args| {
                                *s1.write() += 1;
                                Ok(PyObject::bool_val(true))
                            }),
                        );
                        let s2 = state.clone();
                        w.insert(
                            CompactString::from("release"),
                            PyObject::native_closure("RLock.release", move |_args| {
                                let mut depth = s2.write();
                                if *depth == 0 {
                                    return Err(PyException::runtime_error(
                                        "cannot release un-acquired lock",
                                    ));
                                }
                                *depth -= 1;
                                Ok(PyObject::none())
                            }),
                        );
                        let s3 = state.clone();
                        w.insert(
                            CompactString::from("locked"),
                            PyObject::native_closure("RLock.locked", move |_args| {
                                Ok(PyObject::bool_val(*s3.read() > 0))
                            }),
                        );
                        let s4 = state.clone();
                        let inst_ref = inst.clone();
                        w.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_closure("RLock.__enter__", move |_args| {
                                *s4.write() += 1;
                                Ok(inst_ref.clone())
                            }),
                        );
                        let s5 = state.clone();
                        w.insert(
                            CompactString::from("__exit__"),
                            PyObject::native_closure("RLock.__exit__", move |_args| {
                                let mut depth = s5.write();
                                if *depth > 0 {
                                    *depth -= 1;
                                }
                                Ok(PyObject::none())
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "start_new_thread",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "start_new_thread requires a callable",
                        ));
                    }
                    let func = args[0].clone();
                    let call_args: Vec<PyObjectRef> = if args.len() > 1 {
                        args[1].to_list().unwrap_or_default()
                    } else {
                        vec![]
                    };
                    // Spawn a real OS thread for native closures/functions
                    let closure: Box<dyn FnOnce()> = Box::new(move || {
                        match &func.payload {
                            PyObjectPayload::NativeClosure(nc) => {
                                let _ = (nc.func)(&call_args);
                            }
                            PyObjectPayload::NativeFunction(nf) => {
                                let _ = (nf.func)(&call_args);
                            }
                            _ => {} // Python-defined functions need VM — can't call from here
                        }
                    });
                    let send_closure: Box<dyn FnOnce() + Send> =
                        unsafe { std::mem::transmute(closure) };
                    let handle = std::thread::spawn(move || {
                        send_closure();
                    });
                    // Return thread ID
                    let tid = format!("{:?}", handle.thread().id());
                    let id_num: i64 = tid
                        .chars()
                        .filter(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .parse()
                        .unwrap_or(1);
                    Ok(PyObject::int(id_num))
                }),
            ),
            (
                "get_ident",
                make_builtin(|_| {
                    let tid = format!("{:?}", std::thread::current().id());
                    let id_num: i64 = tid
                        .chars()
                        .filter(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .parse()
                        .unwrap_or(1);
                    Ok(PyObject::int(id_num))
                }),
            ),
            ("stack_size", make_builtin(|_| Ok(PyObject::int(0)))),
            ("TIMEOUT_MAX", PyObject::float(f64::MAX)),
        ],
    )
}
