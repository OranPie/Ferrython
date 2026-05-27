use super::*;

pub(super) fn make_timezone_utc() -> PyObjectRef {
    let class = PyObject::class(CompactString::from("timezone"), vec![], IndexMap::new());
    let class_flags = InstanceData::compute_flags(&class);
    let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
        Box::new(InstanceData {
            class,
            attrs: new_shared_fx(),
            is_special: true,
            dict_storage: None,
            class_flags,
            finalizer_state: std::cell::Cell::new(0),
        }),
    )));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__timezone__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("_offset_seconds"), PyObject::float(0.0));
        w.insert(
            CompactString::from("_name"),
            PyObject::str_val(CompactString::from("UTC")),
        );
        w.insert(
            CompactString::from("__str__"),
            make_builtin(|_| Ok(PyObject::str_val(CompactString::from("UTC")))),
        );
        w.insert(
            CompactString::from("__repr__"),
            make_builtin(|_| {
                Ok(PyObject::str_val(CompactString::from(
                    "datetime.timezone.utc",
                )))
            }),
        );
        w.insert(
            CompactString::from("utcoffset"),
            make_builtin(|_| make_timedelta(0, 0, 0, 0.0)),
        );
        w.insert(
            CompactString::from("tzname"),
            make_builtin(|_| Ok(PyObject::str_val(CompactString::from("UTC")))),
        );
        w.insert(
            CompactString::from("dst"),
            make_builtin(|_| Ok(PyObject::none())),
        );
    }
    inst
}
