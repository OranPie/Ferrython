use super::*;

// ── email.charset module ──────────────────────────────────────────────

pub fn create_email_charset_module() -> PyObjectRef {
    let charset_fn = make_builtin(|args: &[PyObjectRef]| {
        let name = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            "us-ascii".to_string()
        };
        let cls = PyObject::class(CompactString::from("Charset"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("input_charset"),
                PyObject::str_val(CompactString::from(name.as_str())),
            );
            w.insert(
                CompactString::from("output_charset"),
                PyObject::str_val(CompactString::from(name.as_str())),
            );
            let n = name.clone();
            w.insert(
                CompactString::from("__str__"),
                PyObject::native_closure("__str__", move |_: &[PyObjectRef]| {
                    Ok(PyObject::str_val(CompactString::from(n.clone())))
                }),
            );
            w.insert(
                CompactString::from("get_body_encoding"),
                make_builtin(|_| Ok(PyObject::str_val(CompactString::from("base64")))),
            );
        }
        Ok(inst)
    });

    make_module("email.charset", vec![("Charset", charset_fn)])
}
