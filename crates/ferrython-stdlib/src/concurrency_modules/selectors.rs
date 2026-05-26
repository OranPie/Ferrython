use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;

// ── selectors module ────────────────────────────────────────────────

pub fn create_selectors_module() -> PyObjectRef {
    // SelectorKey namedtuple-like
    let selector_key_fn = make_builtin(|args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("SelectorKey"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(
                CompactString::from("fileobj"),
                args.first().cloned().unwrap_or_else(PyObject::none),
            );
            attrs.insert(
                CompactString::from("fd"),
                args.get(1).cloned().unwrap_or_else(|| PyObject::int(0)),
            );
            attrs.insert(
                CompactString::from("events"),
                args.get(2).cloned().unwrap_or_else(|| PyObject::int(0)),
            );
            attrs.insert(
                CompactString::from("data"),
                args.get(3).cloned().unwrap_or_else(PyObject::none),
            );
        }
        Ok(inst)
    });

    // Create selector constructor with register/unregister/select/close/get_map
    fn make_selector(name: &str) -> PyObjectRef {
        let cls_name = CompactString::from(name);
        let cls = PyObject::class(cls_name, vec![], IndexMap::new());
        let c = cls.clone();
        PyObject::native_closure(name, move |_args: &[PyObjectRef]| {
            let inst = PyObject::instance(c.clone());
            let registry: Rc<PyCell<IndexMap<i64, PyObjectRef>>> =
                Rc::new(PyCell::new(IndexMap::new()));
            let inst_ref = inst.clone();

            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();

                // register(fileobj, events, data=None) -> SelectorKey
                let reg1 = registry.clone();
                attrs.insert(
                    CompactString::from("register"),
                    PyObject::native_closure("register", move |args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Err(PyException::type_error(
                                "register() requires at least 1 argument",
                            ));
                        }
                        let fileobj = args[0].clone();
                        let events = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
                        let data = args.get(2).cloned().unwrap_or_else(PyObject::none);
                        let fd = fileobj.as_int().unwrap_or(0);

                        let key_cls = PyObject::class(
                            CompactString::from("SelectorKey"),
                            vec![],
                            IndexMap::new(),
                        );
                        let key = PyObject::instance(key_cls);
                        if let PyObjectPayload::Instance(ref d) = key.payload {
                            let mut ka = d.attrs.write();
                            ka.insert(CompactString::from("fileobj"), fileobj);
                            ka.insert(CompactString::from("fd"), PyObject::int(fd));
                            ka.insert(CompactString::from("events"), PyObject::int(events));
                            ka.insert(CompactString::from("data"), data);
                        }
                        reg1.write().insert(fd, key.clone());
                        Ok(key)
                    }),
                );

                // unregister(fileobj) -> SelectorKey
                let reg2 = registry.clone();
                attrs.insert(
                    CompactString::from("unregister"),
                    PyObject::native_closure("unregister", move |args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Err(PyException::type_error(
                                "unregister() requires 1 argument",
                            ));
                        }
                        let fd = args[0].as_int().unwrap_or(0);
                        let key = reg2.write().swap_remove(&fd).unwrap_or_else(PyObject::none);
                        Ok(key)
                    }),
                );

                // modify(fileobj, events, data=None) -> SelectorKey
                let reg2b = registry.clone();
                attrs.insert(
                    CompactString::from("modify"),
                    PyObject::native_closure("modify", move |args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Err(PyException::type_error(
                                "modify() requires at least 1 argument",
                            ));
                        }
                        let fileobj = args[0].clone();
                        let events = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
                        let data = args.get(2).cloned().unwrap_or_else(PyObject::none);
                        let fd = fileobj.as_int().unwrap_or(0);

                        let key_cls = PyObject::class(
                            CompactString::from("SelectorKey"),
                            vec![],
                            IndexMap::new(),
                        );
                        let key = PyObject::instance(key_cls);
                        if let PyObjectPayload::Instance(ref d) = key.payload {
                            let mut ka = d.attrs.write();
                            ka.insert(CompactString::from("fileobj"), fileobj);
                            ka.insert(CompactString::from("fd"), PyObject::int(fd));
                            ka.insert(CompactString::from("events"), PyObject::int(events));
                            ka.insert(CompactString::from("data"), data);
                        }
                        reg2b.write().insert(fd, key.clone());
                        Ok(key)
                    }),
                );

                // select(timeout=None) -> list of (key, events)
                let reg3 = registry.clone();
                attrs.insert(
                    CompactString::from("select"),
                    PyObject::native_closure("select", move |args: &[PyObjectRef]| {
                        let timeout_ms: i32 = if let Some(t_arg) = args.first() {
                            if matches!(&t_arg.payload, PyObjectPayload::None) {
                                -1 // block forever
                            } else if let Some(t) = t_arg.as_int() {
                                (t * 1000) as i32
                            } else if let Ok(t) = t_arg.to_float() {
                                (t * 1000.0) as i32
                            } else {
                                -1
                            }
                        } else {
                            -1
                        };

                        let r = reg3.read();

                        #[cfg(unix)]
                        {
                            if r.is_empty() {
                                return Ok(PyObject::list(vec![]));
                            }

                            // Build pollfd array from registered fds
                            let mut pollfds: Vec<libc::pollfd> = Vec::with_capacity(r.len());
                            let mut keys: Vec<(&i64, &PyObjectRef)> = Vec::with_capacity(r.len());

                            for (fd, key) in r.iter() {
                                let events_val =
                                    if let PyObjectPayload::Instance(ref d) = key.payload {
                                        d.attrs
                                            .read()
                                            .get(&CompactString::from("events"))
                                            .and_then(|e| e.as_int())
                                            .unwrap_or(0)
                                    } else {
                                        0
                                    };
                                // Map EVENT_READ (1) -> POLLIN, EVENT_WRITE (2) -> POLLOUT
                                let mut poll_events: i16 = 0;
                                if events_val & 1 != 0 {
                                    poll_events |= libc::POLLIN as i16;
                                }
                                if events_val & 2 != 0 {
                                    poll_events |= libc::POLLOUT as i16;
                                }
                                pollfds.push(libc::pollfd {
                                    fd: *fd as i32,
                                    events: poll_events,
                                    revents: 0,
                                });
                                keys.push((fd, key));
                            }

                            let ret = unsafe {
                                libc::poll(
                                    pollfds.as_mut_ptr(),
                                    pollfds.len() as libc::nfds_t,
                                    timeout_ms,
                                )
                            };

                            if ret < 0 {
                                return Err(PyException::os_error("select: poll() failed"));
                            }

                            // Only return keys where revents is non-zero
                            let results: Vec<PyObjectRef> = pollfds
                                .iter()
                                .enumerate()
                                .filter(|(_, pfd)| pfd.revents != 0)
                                .map(|(i, pfd)| {
                                    let key = keys[i].1.clone();
                                    // Map revents back to EVENT_READ/EVENT_WRITE
                                    let mut ready_events: i64 = 0;
                                    let rev = pfd.revents;
                                    if rev & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                                        ready_events |= 1; // EVENT_READ
                                    }
                                    if rev & libc::POLLOUT != 0 {
                                        ready_events |= 2; // EVENT_WRITE
                                    }
                                    PyObject::tuple(vec![key, PyObject::int(ready_events)])
                                })
                                .collect();
                            return Ok(PyObject::list(results));
                        }

                        #[cfg(not(unix))]
                        {
                            let _ = timeout_ms;
                            let results: Vec<PyObjectRef> = r
                                .values()
                                .map(|key| {
                                    let events =
                                        if let PyObjectPayload::Instance(ref d) = key.payload {
                                            d.attrs
                                                .read()
                                                .get(&CompactString::from("events"))
                                                .cloned()
                                                .unwrap_or_else(|| PyObject::int(0))
                                        } else {
                                            PyObject::int(0)
                                        };
                                    PyObject::tuple(vec![key.clone(), events])
                                })
                                .collect();
                            Ok(PyObject::list(results))
                        }
                    }),
                );

                // close()
                let reg4 = registry.clone();
                attrs.insert(
                    CompactString::from("close"),
                    PyObject::native_closure("close", move |_: &[PyObjectRef]| {
                        reg4.write().clear();
                        Ok(PyObject::none())
                    }),
                );

                // get_map()
                attrs.insert(
                    CompactString::from("get_map"),
                    PyObject::native_closure("get_map", move |_: &[PyObjectRef]| {
                        Ok(PyObject::dict(IndexMap::new()))
                    }),
                );

                // Context manager
                let ir = inst_ref.clone();
                attrs.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| Ok(ir.clone())),
                );
                let reg6 = registry.clone();
                attrs.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                        reg6.write().clear();
                        Ok(PyObject::bool_val(false))
                    }),
                );
            }
            Ok(inst)
        })
    }

    make_module(
        "selectors",
        vec![
            ("DefaultSelector", make_selector("DefaultSelector")),
            ("SelectSelector", make_selector("SelectSelector")),
            ("PollSelector", make_selector("PollSelector")),
            ("EpollSelector", make_selector("EpollSelector")),
            ("KqueueSelector", make_selector("KqueueSelector")),
            ("SelectorKey", selector_key_fn),
            ("EVENT_READ", PyObject::int(1)),
            ("EVENT_WRITE", PyObject::int(2)),
        ],
    )
}
