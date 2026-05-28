use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;
use std::rc::Rc;

pub(super) fn create_event_primitives() -> (PyObjectRef, PyObjectRef) {
    // Event — simple threading event using shared state
    let event_cls = PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());
    let ec = event_cls.clone();
    let event_fn = PyObject::native_closure("Event", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(ec.clone());
        let flag = Rc::new(PyCell::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let f1 = flag.clone();
            attrs.insert(
                CompactString::from("set"),
                PyObject::native_closure("set", move |_: &[PyObjectRef]| {
                    *f1.write() = true;
                    Ok(PyObject::none())
                }),
            );
            let f2 = flag.clone();
            attrs.insert(
                CompactString::from("clear"),
                PyObject::native_closure("clear", move |_: &[PyObjectRef]| {
                    *f2.write() = false;
                    Ok(PyObject::none())
                }),
            );
            let f3 = flag.clone();
            attrs.insert(
                CompactString::from("is_set"),
                PyObject::native_closure("is_set", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*f3.read()))
                }),
            );
            let f4 = flag.clone();
            attrs.insert(
                CompactString::from("wait"),
                PyObject::native_closure("wait", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*f4.read()))
                }),
            );
        }
        Ok(inst)
    });

    // Barrier — synchronization barrier
    let barrier_cls = PyObject::class(CompactString::from("Barrier"), vec![], IndexMap::new());
    let bc = barrier_cls.clone();
    let barrier_fn = PyObject::native_closure("Barrier", move |args: &[PyObjectRef]| {
        let parties = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else {
            1
        };
        let inst = PyObject::instance(bc.clone());
        let waiting = Rc::new(PyCell::new(0i64));
        let broken = Rc::new(PyCell::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("parties"), PyObject::int(parties));
            let w1 = waiting.clone();
            attrs.insert(
                CompactString::from("n_waiting"),
                PyObject::native_closure("n_waiting", move |_: &[PyObjectRef]| {
                    Ok(PyObject::int(*w1.read()))
                }),
            );
            let w2 = waiting.clone();
            let b2 = broken.clone();
            let p = parties;
            attrs.insert(
                CompactString::from("wait"),
                PyObject::native_closure("wait", move |_: &[PyObjectRef]| {
                    if *b2.read() {
                        return Err(PyException::runtime_error("BrokenBarrierError"));
                    }
                    let mut w = w2.write();
                    *w += 1;
                    if *w >= p {
                        *w = 0;
                    }
                    Ok(PyObject::int(0))
                }),
            );
            let w3 = waiting.clone();
            attrs.insert(
                CompactString::from("reset"),
                PyObject::native_closure("reset", move |_: &[PyObjectRef]| {
                    *w3.write() = 0;
                    Ok(PyObject::none())
                }),
            );
            let w4 = waiting.clone();
            let b4 = broken.clone();
            attrs.insert(
                CompactString::from("abort"),
                PyObject::native_closure("abort", move |_: &[PyObjectRef]| {
                    *b4.write() = true;
                    *w4.write() = 0;
                    Ok(PyObject::none())
                }),
            );
            let b5 = broken.clone();
            attrs.insert(
                CompactString::from("broken"),
                PyObject::native_closure("broken", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*b5.read()))
                }),
            );
        }
        Ok(inst)
    });

    (event_fn, barrier_fn)
}
