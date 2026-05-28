use compact_str::CompactString;
use ferrython_core::object::{PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

pub(super) fn create_lock_primitives() -> (PyObjectRef, PyObjectRef) {
    // Lock — real mutex for cross-thread synchronization
    let lock_cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
    let lc = lock_cls.clone();
    let lock_fn = PyObject::native_closure("Lock", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(lc.clone());
        let mutex = Arc::new(parking_lot::Mutex::new(()));
        let locked_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let m1 = mutex.clone();
            let lf1 = locked_flag.clone();
            attrs.insert(
                CompactString::from("acquire"),
                PyObject::native_closure("acquire", move |args: &[PyObjectRef]| {
                    let mut blocking = true;
                    for a in args {
                        match &a.payload {
                            PyObjectPayload::Bool(b) => {
                                blocking = *b;
                            }
                            PyObjectPayload::Dict(map) => {
                                let r = map.read();
                                if let Some(v) =
                                    r.get(&HashableKey::str_key(CompactString::from("blocking")))
                                {
                                    blocking = v.is_truthy();
                                }
                            }
                            _ => {}
                        }
                    }
                    if blocking {
                        let guard = m1.lock();
                        lf1.store(true, std::sync::atomic::Ordering::Release);
                        // Leak the guard to keep the mutex locked until release()
                        std::mem::forget(guard);
                        Ok(PyObject::bool_val(true))
                    } else {
                        match m1.try_lock() {
                            Some(guard) => {
                                lf1.store(true, std::sync::atomic::Ordering::Release);
                                std::mem::forget(guard);
                                Ok(PyObject::bool_val(true))
                            }
                            None => Ok(PyObject::bool_val(false)),
                        }
                    }
                }),
            );
            let m2 = mutex.clone();
            let lf2 = locked_flag.clone();
            attrs.insert(
                CompactString::from("release"),
                PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    if lf2.swap(false, std::sync::atomic::Ordering::AcqRel) {
                        // Safety: we know the mutex was locked by acquire()
                        unsafe {
                            m2.force_unlock();
                        }
                    }
                    Ok(PyObject::none())
                }),
            );
            let lf3 = locked_flag.clone();
            attrs.insert(
                CompactString::from("locked"),
                PyObject::native_closure("locked", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(
                        lf3.load(std::sync::atomic::Ordering::Acquire),
                    ))
                }),
            );
            let m4 = mutex.clone();
            let lf4 = locked_flag.clone();
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                    let guard = m4.lock();
                    lf4.store(true, std::sync::atomic::Ordering::Release);
                    std::mem::forget(guard);
                    Ok(inst_ref.clone())
                }),
            );
            let m5 = mutex.clone();
            let lf5 = locked_flag.clone();
            attrs.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                    if lf5.swap(false, std::sync::atomic::Ordering::AcqRel) {
                        unsafe {
                            m5.force_unlock();
                        }
                    }
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    // RLock — reentrant lock with count tracking
    let rlock_cls = PyObject::class(CompactString::from("RLock"), vec![], IndexMap::new());
    let rlc = rlock_cls.clone();
    let rlock_fn = PyObject::native_closure("RLock", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(rlc.clone());
        // (locked, reentrant_count)
        let state = Rc::new(PyCell::new((false, 0u32)));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let s1 = state.clone();
            attrs.insert(
                CompactString::from("acquire"),
                PyObject::native_closure("acquire", move |args: &[PyObjectRef]| {
                    let mut blocking = true;
                    for a in args {
                        match &a.payload {
                            PyObjectPayload::Bool(b) => {
                                blocking = *b;
                            }
                            PyObjectPayload::Dict(map) => {
                                let r = map.read();
                                if let Some(v) =
                                    r.get(&HashableKey::str_key(CompactString::from("blocking")))
                                {
                                    blocking = v.is_truthy();
                                }
                            }
                            _ => {}
                        }
                    }
                    let mut s = s1.write();
                    if s.0 && blocking {
                        // Reentrant — always succeeds
                        s.1 += 1;
                        Ok(PyObject::bool_val(true))
                    } else if s.0 && !blocking {
                        // Non-blocking on already-locked: RLock is reentrant, so succeed
                        s.1 += 1;
                        Ok(PyObject::bool_val(true))
                    } else {
                        s.0 = true;
                        s.1 += 1;
                        Ok(PyObject::bool_val(true))
                    }
                }),
            );
            let s2 = state.clone();
            attrs.insert(
                CompactString::from("release"),
                PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    let mut s = s2.write();
                    if s.1 > 0 {
                        s.1 -= 1;
                    }
                    if s.1 == 0 {
                        s.0 = false;
                    }
                    Ok(PyObject::none())
                }),
            );
            let s3 = state.clone();
            attrs.insert(
                CompactString::from("locked"),
                PyObject::native_closure("locked", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(s3.read().0))
                }),
            );
            let s4 = state.clone();
            let ir = inst_ref.clone();
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                    let mut s = s4.write();
                    s.0 = true;
                    s.1 += 1;
                    Ok(ir.clone())
                }),
            );
            let s5 = state.clone();
            attrs.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                    let mut s = s5.write();
                    if s.1 > 0 {
                        s.1 -= 1;
                    }
                    if s.1 == 0 {
                        s.0 = false;
                    }
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    (lock_fn, rlock_fn)
}
