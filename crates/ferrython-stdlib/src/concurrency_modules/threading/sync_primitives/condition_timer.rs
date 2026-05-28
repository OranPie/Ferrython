use crate::concurrency_modules::push_deferred_call;
use compact_str::CompactString;
use ferrython_core::object::{PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

pub(super) fn create_condition_timer_primitives() -> (PyObjectRef, PyObjectRef) {
    // Condition — real condition variable using std::sync::Condvar
    let cond_cls = PyObject::class(CompactString::from("Condition"), vec![], IndexMap::new());
    let cc = cond_cls.clone();
    let condition_fn = PyObject::native_closure("Condition", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(cc.clone());
        let mutex = Arc::new(std::sync::Mutex::new(false));
        let condvar = Arc::new(std::sync::Condvar::new());
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let m1 = mutex.clone();
            attrs.insert(
                CompactString::from("acquire"),
                PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                    let _guard = m1.lock().unwrap();
                    Ok(PyObject::bool_val(true))
                }),
            );
            let m2 = mutex.clone();
            attrs.insert(
                CompactString::from("release"),
                PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    // Release is implicit when guard drops
                    let _guard = m2.lock();
                    Ok(PyObject::none())
                }),
            );
            // wait(timeout=None) — release lock, wait for notify, re-acquire lock
            let m3 = mutex.clone();
            let c3 = condvar.clone();
            attrs.insert(
                CompactString::from("wait"),
                PyObject::native_closure("wait", move |args: &[PyObjectRef]| {
                    let timeout = args
                        .get(0)
                        .and_then(|a| {
                            if matches!(&a.payload, PyObjectPayload::None) {
                                None
                            } else {
                                Some(a)
                            }
                        })
                        .and_then(|a| a.to_float().ok());
                    let guard = m3.lock().unwrap();
                    if let Some(secs) = timeout {
                        let dur = std::time::Duration::from_secs_f64(secs);
                        let _result = c3.wait_timeout(guard, dur).unwrap();
                    } else {
                        let _result = c3.wait(guard).unwrap();
                    }
                    Ok(PyObject::bool_val(true))
                }),
            );
            // wait_for(predicate, timeout=None) — simplified: evaluates predicate, waits if false
            let m3b = mutex.clone();
            let c3b = condvar.clone();
            attrs.insert(
                CompactString::from("wait_for"),
                PyObject::native_closure("wait_for", move |args: &[PyObjectRef]| {
                    // In CPython, wait_for calls wait() in a loop until predicate() is true.
                    // Since we can't call Python functions from native code without VM access,
                    // we do a single condvar wait then return True (like CPython's successful case).
                    let timeout = args
                        .get(1)
                        .and_then(|a| {
                            if matches!(&a.payload, PyObjectPayload::None) {
                                None
                            } else {
                                Some(a)
                            }
                        })
                        .and_then(|a| a.to_float().ok());
                    let guard = m3b.lock().unwrap();
                    if let Some(secs) = timeout {
                        let dur = std::time::Duration::from_secs_f64(secs);
                        let _result = c3b.wait_timeout(guard, dur).unwrap();
                    } else {
                        let _result = c3b.wait(guard).unwrap();
                    }
                    Ok(PyObject::bool_val(true))
                }),
            );
            let c4 = condvar.clone();
            attrs.insert(
                CompactString::from("notify"),
                PyObject::native_closure("notify", move |_: &[PyObjectRef]| {
                    c4.notify_one();
                    Ok(PyObject::none())
                }),
            );
            let c5 = condvar.clone();
            attrs.insert(
                CompactString::from("notify_all"),
                PyObject::native_closure("notify_all", move |_: &[PyObjectRef]| {
                    c5.notify_all();
                    Ok(PyObject::none())
                }),
            );
            let m6 = mutex.clone();
            let ir = inst_ref.clone();
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                    let _guard = m6.lock().unwrap();
                    Ok(ir.clone())
                }),
            );
            let m7 = mutex.clone();
            attrs.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                    let _guard = m7.lock();
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    // Timer — subclass of Thread with cancel()
    let timer_cls = PyObject::class(CompactString::from("Timer"), vec![], IndexMap::new());
    let tmc = timer_cls.clone();
    let timer_fn = PyObject::native_closure("Timer", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(tmc.clone());
        let cancelled = Rc::new(PyCell::new(false));
        let alive = Rc::new(PyCell::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            // interval
            let interval = if !args.is_empty() {
                args[0].to_float().unwrap_or(0.0)
            } else {
                0.0
            };
            attrs.insert(CompactString::from("interval"), PyObject::float(interval));
            // Parse target/args from kwargs
            let mut target = PyObject::none();
            let mut fn_args = PyObject::tuple(vec![]);
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    if let Some(t) = r.get(&HashableKey::str_key(CompactString::from("target"))) {
                        target = t.clone();
                    }
                    if let Some(a) = r.get(&HashableKey::str_key(CompactString::from("args"))) {
                        fn_args = a.clone();
                    }
                }
            }
            // Also check positional: Timer(interval, function, args)
            if args.len() >= 2 && matches!(&target.payload, PyObjectPayload::None) {
                target = args[1].clone();
            }
            if args.len() >= 3
                && matches!(&fn_args.payload, PyObjectPayload::Tuple(t) if t.is_empty())
            {
                fn_args = args[2].clone();
            }
            attrs.insert(CompactString::from("function"), target.clone());
            attrs.insert(CompactString::from("args"), fn_args.clone());
            attrs.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from("Timer")),
            );
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));

            let c1 = cancelled.clone();
            attrs.insert(
                CompactString::from("cancel"),
                PyObject::native_closure("cancel", move |_: &[PyObjectRef]| {
                    *c1.write() = true;
                    Ok(PyObject::none())
                }),
            );

            let c2 = cancelled.clone();
            let a1 = alive.clone();
            let tgt = target.clone();
            let targs = fn_args.clone();
            attrs.insert(
                CompactString::from("start"),
                PyObject::native_closure("start", move |_: &[PyObjectRef]| {
                    if *c2.read() {
                        return Ok(PyObject::none());
                    }
                    *a1.write() = true;
                    if !matches!(&tgt.payload, PyObjectPayload::None) {
                        let call_args: Vec<PyObjectRef> = match &targs.payload {
                            PyObjectPayload::Tuple(items) => (**items).clone(),
                            PyObjectPayload::List(items) => items.read().clone(),
                            _ => vec![],
                        };
                        match &tgt.payload {
                            PyObjectPayload::NativeFunction(nf) => {
                                let _ = (nf.func)(&call_args);
                            }
                            PyObjectPayload::NativeClosure(nc) => {
                                let _ = (nc.func)(&call_args);
                            }
                            _ => {
                                push_deferred_call(tgt.clone(), call_args);
                            }
                        }
                    }
                    *a1.write() = false;
                    Ok(PyObject::none())
                }),
            );
            let a2 = alive.clone();
            attrs.insert(
                CompactString::from("join"),
                PyObject::native_closure("join", move |args: &[PyObjectRef]| {
                    // Timer runs synchronously, so if alive, spin-wait with optional timeout
                    let timeout = args.first().and_then(|a| {
                        if matches!(&a.payload, PyObjectPayload::None) {
                            None
                        } else {
                            a.to_float().ok()
                        }
                    });
                    if *a2.read() {
                        if let Some(t) = timeout {
                            std::thread::sleep(std::time::Duration::from_secs_f64(t));
                        }
                    }
                    Ok(PyObject::none())
                }),
            );
            let a3 = alive.clone();
            let c3 = cancelled.clone();
            attrs.insert(
                CompactString::from("is_alive"),
                PyObject::native_closure("is_alive", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*a3.read() && !*c3.read()))
                }),
            );
            attrs.insert(CompactString::from("ident"), PyObject::none());
        }
        Ok(inst)
    });

    (condition_fn, timer_fn)
}
