use super::*;

pub(super) fn register_exception_assertions(tc_ns: &mut IndexMap<CompactString, PyObjectRef>) {
    // assertRaises(exc_type) — returns a context manager
    tc_ns.insert(
        CompactString::from("assertRaises"),
        PyObject::native_closure("assertRaises", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "assertRaises requires an exception type",
                ));
            }
            let exc_type_name = match &args[0].payload {
                PyObjectPayload::Class(cd) => cd.name.clone(),
                PyObjectPayload::Str(s) => s.to_compact_string(),
                _ => CompactString::from(args[0].py_to_string()),
            };
            // Build a context-manager object with __enter__ / __exit__
            let cls = PyObject::class(
                CompactString::from("_AssertRaisesContext"),
                vec![],
                IndexMap::new(),
            );
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(
                    CompactString::from("expected"),
                    PyObject::str_val(exc_type_name.clone()),
                );
                w.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", |_args: &[PyObjectRef]| {
                        Ok(PyObject::none())
                    }),
                );
                let etype = exc_type_name.clone();
                w.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", move |args: &[PyObjectRef]| {
                        // args: exc_type, exc_val, exc_tb (or None if no exception)
                        let has_exc = if args.is_empty() {
                            false
                        } else {
                            !matches!(args[0].payload, PyObjectPayload::None)
                        };
                        if !has_exc {
                            return Err(PyException::assertion_error(format!(
                                "{} not raised",
                                etype
                            )));
                        }
                        // Suppress the exception
                        Ok(PyObject::bool_val(true))
                    }),
                );
            }
            Ok(inst)
        }),
    );
}
