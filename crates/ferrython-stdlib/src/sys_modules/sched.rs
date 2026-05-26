use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

// ── sched module ──

pub fn create_sched_module() -> PyObjectRef {
    // Event namedtuple-like: (time, priority, sequence, action, argument, kwargs)
    let event_cls = PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());

    let event_cls2 = event_cls.clone();
    let scheduler_fn = PyObject::native_closure("scheduler", move |args: &[PyObjectRef]| {
        // scheduler(timefunc=time.monotonic, delayfunc=time.sleep)
        // We use std::time for the default implementation
        let _timefunc = args.first().cloned();
        let _delayfunc = args.get(1).cloned();

        // Internal priority queue: Vec of (time_f64, priority, sequence, action, args, kwargs)
        let queue: Rc<PyCell<Vec<(f64, i64, i64, PyObjectRef, PyObjectRef, PyObjectRef)>>> =
            Rc::new(PyCell::new(Vec::new()));
        let seq_counter: Arc<std::sync::atomic::AtomicI64> =
            Arc::new(std::sync::atomic::AtomicI64::new(0));

        let cls = PyObject::class(CompactString::from("scheduler"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);

        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();

            // enterabs(time, priority, action, argument=(), kwargs={})
            let q = queue.clone();
            let seq = seq_counter.clone();
            let ev_cls = event_cls2.clone();
            w.insert(
                CompactString::from("enterabs"),
                PyObject::native_closure("enterabs", move |args: &[PyObjectRef]| {
                    if args.len() < 3 {
                        return Err(PyException::type_error(
                            "enterabs() requires at least 3 arguments",
                        ));
                    }
                    let time_val = args[0].to_float()?;
                    let priority = args[1].to_int().unwrap_or(1);
                    let action = args[2].clone();
                    let argument = args
                        .get(3)
                        .cloned()
                        .unwrap_or_else(|| PyObject::tuple(vec![]));
                    let kwargs = args.get(4).cloned().unwrap_or_else(PyObject::none);
                    let s = seq.fetch_add(1, Ordering::SeqCst);

                    let event = PyObject::instance(ev_cls.clone());
                    if let PyObjectPayload::Instance(ref ed) = event.payload {
                        let mut ew = ed.attrs.write();
                        ew.insert(CompactString::from("time"), PyObject::float(time_val));
                        ew.insert(CompactString::from("priority"), PyObject::int(priority));
                        ew.insert(CompactString::from("sequence"), PyObject::int(s));
                        ew.insert(CompactString::from("action"), action.clone());
                        ew.insert(CompactString::from("argument"), argument.clone());
                        ew.insert(CompactString::from("kwargs"), kwargs.clone());
                    }

                    let mut qw = q.write();
                    qw.push((time_val, priority, s, action, argument, kwargs));
                    // Sort by (time, priority, sequence) ascending
                    qw.sort_by(|a, b| {
                        a.0.partial_cmp(&b.0)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(a.1.cmp(&b.1))
                            .then(a.2.cmp(&b.2))
                    });

                    Ok(event)
                }),
            );

            // enter(delay, priority, action, argument=(), kwargs={})
            let q2 = queue.clone();
            let seq2 = seq_counter.clone();
            let ev_cls2 = event_cls2.clone();
            w.insert(
                CompactString::from("enter"),
                PyObject::native_closure("enter", move |args: &[PyObjectRef]| {
                    if args.len() < 3 {
                        return Err(PyException::type_error(
                            "enter() requires at least 3 arguments",
                        ));
                    }
                    let delay = args[0].to_float()?;
                    let priority = args[1].to_int().unwrap_or(1);
                    let action = args[2].clone();
                    let argument = args
                        .get(3)
                        .cloned()
                        .unwrap_or_else(|| PyObject::tuple(vec![]));
                    let kwargs = args.get(4).cloned().unwrap_or_else(PyObject::none);
                    let s = seq2.fetch_add(1, Ordering::SeqCst);

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64();
                    let time_val = now + delay;

                    let event = PyObject::instance(ev_cls2.clone());
                    if let PyObjectPayload::Instance(ref ed) = event.payload {
                        let mut ew = ed.attrs.write();
                        ew.insert(CompactString::from("time"), PyObject::float(time_val));
                        ew.insert(CompactString::from("priority"), PyObject::int(priority));
                        ew.insert(CompactString::from("sequence"), PyObject::int(s));
                        ew.insert(CompactString::from("action"), action.clone());
                        ew.insert(CompactString::from("argument"), argument.clone());
                        ew.insert(CompactString::from("kwargs"), kwargs.clone());
                    }

                    let mut qw = q2.write();
                    qw.push((time_val, priority, s, action, argument, kwargs));
                    qw.sort_by(|a, b| {
                        a.0.partial_cmp(&b.0)
                            .unwrap_or(std::cmp::Ordering::Equal)
                            .then(a.1.cmp(&b.1))
                            .then(a.2.cmp(&b.2))
                    });

                    Ok(event)
                }),
            );

            // cancel(event) — remove matching event from queue
            let q3 = queue.clone();
            w.insert(
                CompactString::from("cancel"),
                PyObject::native_closure("cancel", move |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("cancel() requires 1 argument"));
                    }
                    let event = &args[0];
                    let ev_seq = event.get_attr("sequence").and_then(|s| s.as_int());
                    if let Some(seq_val) = ev_seq {
                        let mut qw = q3.write();
                        qw.retain(|e| e.2 != seq_val);
                        Ok(PyObject::none())
                    } else {
                        Err(PyException::runtime_error("event not in queue"))
                    }
                }),
            );

            // empty() -> bool
            let q4 = queue.clone();
            w.insert(
                CompactString::from("empty"),
                PyObject::native_closure("empty", move |_args: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(q4.read().is_empty()))
                }),
            );

            // run(blocking=True) — execute events in order
            let q5 = queue.clone();
            w.insert(
                CompactString::from("run"),
                PyObject::native_closure("run", move |args: &[PyObjectRef]| {
                    let blocking = args
                        .first()
                        .map(|a| !matches!(&a.payload, PyObjectPayload::Bool(false)))
                        .unwrap_or(true);

                    loop {
                        let next = {
                            let qr = q5.read();
                            if qr.is_empty() {
                                break;
                            }
                            qr[0].clone()
                        };

                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs_f64();

                        if next.0 > now {
                            if !blocking {
                                break;
                            }
                            let delay = next.0 - now;
                            if delay > 0.0 {
                                std::thread::sleep(std::time::Duration::from_secs_f64(delay));
                            }
                        }

                        // Pop the event
                        {
                            let mut qw = q5.write();
                            if !qw.is_empty() && qw[0].2 == next.2 {
                                qw.remove(0);
                            } else {
                                continue;
                            }
                        }

                        // Call action(*argument)
                        let action = &next.3;
                        let argument = &next.4;
                        let call_args: Vec<PyObjectRef> =
                            if let PyObjectPayload::Tuple(items) = &argument.payload {
                                (**items).clone()
                            } else {
                                vec![]
                            };
                        match &action.payload {
                            PyObjectPayload::NativeFunction(nf) => {
                                (nf.func)(&call_args)?;
                            }
                            PyObjectPayload::NativeClosure(nc) => {
                                (nc.func)(&call_args)?;
                            }
                            _ => {
                                // Python function — defer via request_vm_call
                                ferrython_core::error::request_vm_call(action.clone(), call_args);
                            }
                        }
                    }
                    Ok(PyObject::none())
                }),
            );

            // queue property — list of pending events (read-only snapshot)
            let q6 = queue.clone();
            let ev_cls3 = event_cls2.clone();
            w.insert(
                CompactString::from("queue"),
                PyObject::native_closure("queue", move |_args: &[PyObjectRef]| {
                    let qr = q6.read();
                    let events: Vec<PyObjectRef> = qr
                        .iter()
                        .map(|(t, p, s, act, arg, kw)| {
                            let event = PyObject::instance(ev_cls3.clone());
                            if let PyObjectPayload::Instance(ref ed) = event.payload {
                                let mut ew = ed.attrs.write();
                                ew.insert(CompactString::from("time"), PyObject::float(*t));
                                ew.insert(CompactString::from("priority"), PyObject::int(*p));
                                ew.insert(CompactString::from("sequence"), PyObject::int(*s));
                                ew.insert(CompactString::from("action"), act.clone());
                                ew.insert(CompactString::from("argument"), arg.clone());
                                ew.insert(CompactString::from("kwargs"), kw.clone());
                            }
                            event
                        })
                        .collect();
                    Ok(PyObject::list(events))
                }),
            );
        }

        Ok(inst)
    });

    make_module(
        "sched",
        vec![("scheduler", scheduler_fn), ("Event", event_cls)],
    )
}

// ── mmap module ──
