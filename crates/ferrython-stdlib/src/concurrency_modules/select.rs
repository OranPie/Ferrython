use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;

// concurrent.futures is now implemented in pure Python: stdlib/Lib/concurrent/futures.py

// ── select module ──

pub fn create_select_module() -> PyObjectRef {
    // select.select(rlist, wlist, xlist[, timeout])
    let select_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 3 {
            return Err(PyException::type_error(
                "select() requires at least 3 arguments",
            ));
        }
        // Extract file descriptors from the lists
        let extract_fds = |obj: &PyObjectRef| -> Vec<(i32, PyObjectRef)> {
            match &obj.payload {
                PyObjectPayload::List(items) => items
                    .read()
                    .iter()
                    .map(|item| {
                        let fd = if let Some(fileno) = item.get_attr("fileno") {
                            match &fileno.payload {
                                PyObjectPayload::NativeFunction(nf) => (nf.func)(&[item.clone()])
                                    .ok()
                                    .and_then(|v| v.as_int())
                                    .unwrap_or(-1)
                                    as i32,
                                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[item.clone()])
                                    .ok()
                                    .and_then(|v| v.as_int())
                                    .unwrap_or(-1)
                                    as i32,
                                _ => item.as_int().unwrap_or(-1) as i32,
                            }
                        } else {
                            item.as_int().unwrap_or(-1) as i32
                        };
                        (fd, item.clone())
                    })
                    .collect(),
                _ => vec![],
            }
        };

        let rlist_fds = extract_fds(&args[0]);
        let wlist_fds = extract_fds(&args[1]);
        let xlist_fds = extract_fds(&args[2]);

        // Timeout in milliseconds (None = -1 = block forever)
        let timeout_ms: i32 =
            if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::None) {
                if let Some(t) = args[3].as_int() {
                    (t * 1000) as i32
                } else if let Ok(t) = args[3].to_float() {
                    (t * 1000.0) as i32
                } else {
                    -1
                }
            } else {
                -1
            };

        #[cfg(unix)]
        {
            // Use libc::poll for real fd polling
            let mut pollfds: Vec<libc::pollfd> = Vec::new();
            let mut fd_map: Vec<(usize, PyObjectRef)> = Vec::new(); // index -> original object

            // rlist fds -> POLLIN
            for (fd, obj) in &rlist_fds {
                if *fd >= 0 {
                    pollfds.push(libc::pollfd {
                        fd: *fd,
                        events: libc::POLLIN,
                        revents: 0,
                    });
                    fd_map.push((pollfds.len() - 1, obj.clone()));
                }
            }
            let rlist_count = pollfds.len();
            // wlist fds -> POLLOUT
            for (fd, obj) in &wlist_fds {
                if *fd >= 0 {
                    pollfds.push(libc::pollfd {
                        fd: *fd,
                        events: libc::POLLOUT,
                        revents: 0,
                    });
                    fd_map.push((pollfds.len() - 1, obj.clone()));
                }
            }
            let wlist_count = pollfds.len() - rlist_count;
            // xlist fds -> POLLPRI
            for (fd, obj) in &xlist_fds {
                if *fd >= 0 {
                    pollfds.push(libc::pollfd {
                        fd: *fd,
                        events: libc::POLLPRI,
                        revents: 0,
                    });
                    fd_map.push((pollfds.len() - 1, obj.clone()));
                }
            }

            if pollfds.is_empty() {
                // No valid fds — sleep for timeout if given
                if timeout_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
                }
                return Ok(PyObject::tuple(vec![
                    PyObject::list(vec![]),
                    PyObject::list(vec![]),
                    PyObject::list(vec![]),
                ]));
            }

            let ret = unsafe {
                libc::poll(
                    pollfds.as_mut_ptr(),
                    pollfds.len() as libc::nfds_t,
                    timeout_ms,
                )
            };
            if ret < 0 {
                return Err(PyException::os_error("select.select: poll() failed"));
            }

            let mut readable = Vec::new();
            let mut writable = Vec::new();
            let mut exceptional = Vec::new();

            for (i, pfd) in pollfds.iter().enumerate() {
                if pfd.revents != 0 {
                    // Find original object for this fd
                    let obj = fd_map
                        .iter()
                        .find(|(idx, _)| *idx == i)
                        .map(|(_, o)| o.clone());
                    if let Some(o) = obj {
                        if i < rlist_count
                            && (pfd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR)) != 0
                        {
                            readable.push(o);
                        } else if i >= rlist_count
                            && i < rlist_count + wlist_count
                            && (pfd.revents & libc::POLLOUT) != 0
                        {
                            writable.push(o);
                        } else if i >= rlist_count + wlist_count
                            && (pfd.revents & libc::POLLPRI) != 0
                        {
                            exceptional.push(o);
                        }
                    }
                }
            }

            return Ok(PyObject::tuple(vec![
                PyObject::list(readable),
                PyObject::list(writable),
                PyObject::list(exceptional),
            ]));
        }

        #[cfg(not(unix))]
        {
            // Fallback: return rlist as readable (same as before)
            let rlist: Vec<PyObjectRef> = rlist_fds.into_iter().map(|(_, obj)| obj).collect();
            Ok(PyObject::tuple(vec![
                PyObject::list(rlist),
                PyObject::list(vec![]),
                PyObject::list(vec![]),
            ]))
        }
    });

    let poll_cls = {
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("register"),
            make_builtin(|_args| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("unregister"),
            make_builtin(|_args| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("modify"),
            make_builtin(|_args| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("poll"),
            make_builtin(|_args| Ok(PyObject::list(vec![]))),
        );
        PyObject::class(CompactString::from("poll"), vec![], ns)
    };

    let poll_fn = {
        let poll_cls = poll_cls.clone();
        PyObject::native_closure("poll", move |_args: &[PyObjectRef]| {
            // Create a poll instance with shared fd registry
            let registered_fds: Rc<PyCell<Vec<(i32, i16)>>> = Rc::new(PyCell::new(Vec::new()));
            let inst = PyObject::instance(poll_cls.clone());
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                let fds = registered_fds.clone();
                attrs.insert(
                    CompactString::from("register"),
                    PyObject::native_closure("register", move |args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Ok(PyObject::none());
                        }
                        let fd = args[0].as_int().unwrap_or(-1) as i32;
                        let events = if args.len() > 1 {
                            args[1].as_int().unwrap_or(0x001) as i16
                        } else {
                            0x001 | 0x002 | 0x004
                        };
                        let mut fds_w = fds.write();
                        fds_w.retain(|(f, _)| *f != fd);
                        fds_w.push((fd, events));
                        Ok(PyObject::none())
                    }),
                );
                let fds2 = registered_fds.clone();
                attrs.insert(
                    CompactString::from("unregister"),
                    PyObject::native_closure("unregister", move |args: &[PyObjectRef]| {
                        if let Some(fd) = args.first().and_then(|a| a.as_int()) {
                            fds2.write().retain(|(f, _)| *f != fd as i32);
                        }
                        Ok(PyObject::none())
                    }),
                );
                let fds3 = registered_fds.clone();
                attrs.insert(
                    CompactString::from("modify"),
                    PyObject::native_closure("modify", move |args: &[PyObjectRef]| {
                        if args.len() >= 2 {
                            let fd = args[0].as_int().unwrap_or(-1) as i32;
                            let events = args[1].as_int().unwrap_or(0x001) as i16;
                            let mut fds_w = fds3.write();
                            if let Some(entry) = fds_w.iter_mut().find(|(f, _)| *f == fd) {
                                entry.1 = events;
                            }
                        }
                        Ok(PyObject::none())
                    }),
                );
                let fds4 = registered_fds.clone();
                attrs.insert(
                    CompactString::from("poll"),
                    PyObject::native_closure("poll", move |args: &[PyObjectRef]| {
                        let timeout_ms: i32 = if !args.is_empty()
                            && !matches!(&args[0].payload, PyObjectPayload::None)
                        {
                            args[0].as_int().unwrap_or(-1) as i32
                        } else {
                            -1
                        };
                        let fds_r = fds4.read();
                        if fds_r.is_empty() {
                            return Ok(PyObject::list(vec![]));
                        }
                        #[cfg(unix)]
                        {
                            let mut pollfds: Vec<libc::pollfd> = fds_r
                                .iter()
                                .map(|(fd, events)| libc::pollfd {
                                    fd: *fd,
                                    events: *events,
                                    revents: 0,
                                })
                                .collect();
                            let ret = unsafe {
                                libc::poll(
                                    pollfds.as_mut_ptr(),
                                    pollfds.len() as libc::nfds_t,
                                    timeout_ms,
                                )
                            };
                            if ret <= 0 {
                                return Ok(PyObject::list(vec![]));
                            }
                            let results: Vec<PyObjectRef> = pollfds
                                .iter()
                                .filter(|pfd| pfd.revents != 0)
                                .map(|pfd| {
                                    PyObject::tuple(vec![
                                        PyObject::int(pfd.fd as i64),
                                        PyObject::int(pfd.revents as i64),
                                    ])
                                })
                                .collect();
                            return Ok(PyObject::list(results));
                        }
                        #[cfg(not(unix))]
                        Ok(PyObject::list(vec![]))
                    }),
                );
            }
            Ok(inst)
        })
    };

    make_module(
        "select",
        vec![
            ("error", PyObject::exception_type(ExceptionKind::OSError)),
            ("select", select_fn),
            ("poll", poll_fn),
            ("POLLIN", PyObject::int(0x001)),
            ("POLLPRI", PyObject::int(0x002)),
            ("POLLOUT", PyObject::int(0x004)),
            ("POLLERR", PyObject::int(0x008)),
            ("POLLHUP", PyObject::int(0x010)),
            ("POLLNVAL", PyObject::int(0x020)),
            // epoll constants (Linux)
            ("EPOLLIN", PyObject::int(0x001)),
            ("EPOLLOUT", PyObject::int(0x004)),
            ("EPOLLERR", PyObject::int(0x008)),
            ("EPOLLHUP", PyObject::int(0x010)),
            ("EPOLLET", PyObject::int(1 << 31)),
            ("EPOLLONESHOT", PyObject::int(1 << 30)),
            ("EPOLLRDHUP", PyObject::int(0x2000)),
        ],
    )
}
