use compact_str::CompactString;
use ferrython_core::error::ExceptionKind;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

/// Attach `split` and `subgroup` methods to an ExceptionGroup instance.
/// Reads `message` and `exceptions` from the instance attrs.
pub fn attach_eg_methods_pub(eg: &PyObjectRef) {
    attach_eg_methods(eg);
}

pub(super) fn attach_eg_methods(eg: &PyObjectRef) {
    if let PyObjectPayload::ExceptionInstance(ei) = &eg.payload {
        let (msg, exc_list) = {
            let a = ei.ensure_attrs().read();
            let msg = a
                .get(&CompactString::from("message"))
                .cloned()
                .unwrap_or_else(|| PyObject::str_val(CompactString::from("")));
            let exc_list = a
                .get(&CompactString::from("exceptions"))
                .cloned()
                .unwrap_or_else(|| PyObject::list(vec![]));
            (msg, exc_list)
        };
        let msg_sg = msg.clone();
        let exc_sg = exc_list.clone();
        let msg_sp = msg;
        let exc_sp = exc_list;
        let mut a = ei.ensure_attrs().write();
        a.insert(
            CompactString::from("subgroup"),
            PyObject::native_closure("ExceptionGroup.subgroup", move |sg_args| {
                let filter_type = if sg_args.len() > 1 {
                    &sg_args[1]
                } else if !sg_args.is_empty() {
                    &sg_args[0]
                } else {
                    return Ok(PyObject::none());
                };
                let filter_kind = match &filter_type.payload {
                    PyObjectPayload::ExceptionType(k) => Some(k.clone()),
                    _ => ExceptionKind::from_name(&filter_type.py_to_string()),
                };
                let items = exc_sg.to_list().unwrap_or_default();
                let matched: Vec<PyObjectRef> = items
                    .into_iter()
                    .filter(|exc| {
                        if let Some(ref fk) = filter_kind {
                            if let PyObjectPayload::ExceptionInstance(ei) = &exc.payload {
                                return ei.kind.is_subclass_of(fk);
                            }
                        }
                        false
                    })
                    .collect();
                if matched.is_empty() {
                    return Ok(PyObject::none());
                }
                let new_eg = PyObject::exception_instance(
                    ExceptionKind::ExceptionGroup,
                    msg_sg.py_to_string(),
                );
                if let PyObjectPayload::ExceptionInstance(ei) = &new_eg.payload {
                    let mut ew = ei.ensure_attrs().write();
                    ew.insert(CompactString::from("message"), msg_sg.clone());
                    ew.insert(CompactString::from("exceptions"), PyObject::list(matched));
                }
                attach_eg_methods(&new_eg);
                Ok(new_eg)
            }),
        );
        a.insert(
            CompactString::from("split"),
            PyObject::native_closure("ExceptionGroup.split", move |sp_args| {
                let filter_type = if sp_args.len() > 1 {
                    &sp_args[1]
                } else if !sp_args.is_empty() {
                    &sp_args[0]
                } else {
                    return Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none()]));
                };
                let filter_kind = match &filter_type.payload {
                    PyObjectPayload::ExceptionType(k) => Some(k.clone()),
                    _ => ExceptionKind::from_name(&filter_type.py_to_string()),
                };
                let items = exc_sp.to_list().unwrap_or_default();
                let mut matched = Vec::new();
                let mut rest = Vec::new();
                for exc in items {
                    let matches = if let Some(ref fk) = filter_kind {
                        if let PyObjectPayload::ExceptionInstance(ei) = &exc.payload {
                            ei.kind.is_subclass_of(fk)
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if matches {
                        matched.push(exc);
                    } else {
                        rest.push(exc);
                    }
                }
                let make_eg = |msg: &PyObjectRef, items: Vec<PyObjectRef>| -> PyObjectRef {
                    if items.is_empty() {
                        return PyObject::none();
                    }
                    let eg = PyObject::exception_instance(
                        ExceptionKind::ExceptionGroup,
                        msg.py_to_string(),
                    );
                    if let PyObjectPayload::ExceptionInstance(ei) = &eg.payload {
                        let mut ew = ei.ensure_attrs().write();
                        ew.insert(CompactString::from("message"), msg.clone());
                        ew.insert(CompactString::from("exceptions"), PyObject::list(items));
                    }
                    attach_eg_methods(&eg);
                    eg
                };
                Ok(PyObject::tuple(vec![
                    make_eg(&msg_sp, matched),
                    make_eg(&msg_sp, rest),
                ]))
            }),
        );
    }
}
