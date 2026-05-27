use super::*;

// ── email.contentmanager module ────────────────────────────────────────

pub fn create_email_contentmanager_module() -> PyObjectRef {
    let content_manager_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(
            CompactString::from("ContentManager"),
            vec![],
            IndexMap::new(),
        );
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("get_content"),
                make_builtin(|args| {
                    if let Some(msg) = args.first() {
                        if let Some(payload) = msg.get_attr("_payload") {
                            return Ok(payload);
                        }
                    }
                    Ok(PyObject::none())
                }),
            );
            w.insert(
                CompactString::from("set_content"),
                make_builtin(|_| Ok(PyObject::none())),
            );
        }
        Ok(inst)
    });

    let raw_mgr = content_manager_fn.clone();
    // Create a default ContentManager instance
    let default_mgr = match &raw_mgr.payload {
        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[]).unwrap_or_else(|_| PyObject::none()),
        _ => PyObject::none(),
    };

    make_module(
        "email.contentmanager",
        vec![
            ("ContentManager", content_manager_fn),
            ("raw_data_manager", default_mgr),
        ],
    )
}
