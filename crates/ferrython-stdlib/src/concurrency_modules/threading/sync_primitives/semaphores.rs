use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;
use std::rc::Rc;

pub(super) fn create_semaphore_primitives() -> (PyObjectRef, PyObjectRef) {
    // Semaphore — counting semaphore
    let sem_cls = PyObject::class(CompactString::from("Semaphore"), vec![], IndexMap::new());
    let sc = sem_cls.clone();
    let semaphore_fn = PyObject::native_closure("Semaphore", move |args: &[PyObjectRef]| {
        let initial = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else {
            1
        };
        let inst = PyObject::instance(sc.clone());
        let counter = Rc::new(PyCell::new(initial));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let c1 = counter.clone();
            attrs.insert(
                CompactString::from("acquire"),
                PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                    let mut c = c1.write();
                    if *c > 0 {
                        *c -= 1;
                        Ok(PyObject::bool_val(true))
                    } else {
                        Ok(PyObject::bool_val(false))
                    }
                }),
            );
            let c2 = counter.clone();
            attrs.insert(
                CompactString::from("release"),
                PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    *c2.write() += 1;
                    Ok(PyObject::none())
                }),
            );
            let c3 = counter.clone();
            attrs.insert(
                CompactString::from("_value"),
                PyObject::native_closure("_value", move |_: &[PyObjectRef]| {
                    Ok(PyObject::int(*c3.read()))
                }),
            );
            let c4 = counter.clone();
            let ir = inst_ref.clone();
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                    let mut c = c4.write();
                    if *c > 0 {
                        *c -= 1;
                    }
                    Ok(ir.clone())
                }),
            );
            let c5 = counter.clone();
            attrs.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                    *c5.write() += 1;
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    // BoundedSemaphore — same as Semaphore with upper bound check
    let bsem_cls = PyObject::class(
        CompactString::from("BoundedSemaphore"),
        vec![],
        IndexMap::new(),
    );
    let bsc = bsem_cls.clone();
    let bounded_semaphore_fn =
        PyObject::native_closure("BoundedSemaphore", move |args: &[PyObjectRef]| {
            let initial = if !args.is_empty() {
                args[0].as_int().unwrap_or(1)
            } else {
                1
            };
            let inst = PyObject::instance(bsc.clone());
            let counter = Rc::new(PyCell::new(initial));
            let bound = initial;
            let inst_ref = inst.clone();
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();
                let c1 = counter.clone();
                attrs.insert(
                    CompactString::from("acquire"),
                    PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                        let mut c = c1.write();
                        if *c > 0 {
                            *c -= 1;
                            Ok(PyObject::bool_val(true))
                        } else {
                            Ok(PyObject::bool_val(false))
                        }
                    }),
                );
                let c2 = counter.clone();
                attrs.insert(
                    CompactString::from("release"),
                    PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                        let mut c = c2.write();
                        if *c >= bound {
                            return Err(PyException::value_error(
                                "Semaphore released too many times",
                            ));
                        }
                        *c += 1;
                        Ok(PyObject::none())
                    }),
                );
                let c3 = counter.clone();
                attrs.insert(
                    CompactString::from("_value"),
                    PyObject::native_closure("_value", move |_: &[PyObjectRef]| {
                        Ok(PyObject::int(*c3.read()))
                    }),
                );
                let c4 = counter.clone();
                let ir = inst_ref.clone();
                attrs.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                        let mut c = c4.write();
                        if *c > 0 {
                            *c -= 1;
                        }
                        Ok(ir.clone())
                    }),
                );
                let c5 = counter.clone();
                attrs.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                        *c5.write() += 1;
                        Ok(PyObject::bool_val(false))
                    }),
                );
            }
            Ok(inst)
        });

    (semaphore_fn, bounded_semaphore_fn)
}
