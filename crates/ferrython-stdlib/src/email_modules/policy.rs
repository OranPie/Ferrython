use super::*;

// ── email.policy module ────────────────────────────────────────────────

pub fn create_email_policy_module() -> PyObjectRef {
    // Build an EmailPolicy class that can be instantiated with kwargs
    let make_policy = |name: &str, utf8: bool, max_line_len: i64| -> PyObjectRef {
        let cls = PyObject::class(CompactString::from("EmailPolicy"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("utf8"), PyObject::bool_val(utf8));
        attrs.insert(
            CompactString::from("max_line_length"),
            PyObject::int(max_line_len),
        );
        attrs.insert(
            CompactString::from("raise_on_defect"),
            PyObject::bool_val(false),
        );
        attrs.insert(
            CompactString::from("cte_type"),
            PyObject::str_val(CompactString::from("8bit")),
        );
        attrs.insert(CompactString::from("header_source_parse"), PyObject::none());
        attrs.insert(CompactString::from("header_store_parse"), PyObject::none());
        attrs.insert(CompactString::from("header_factory"), PyObject::none());
        attrs.insert(CompactString::from("content_manager"), PyObject::none());
        let name_str = CompactString::from(name);
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("__repr__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "email.policy.{}",
                    name_str
                ))))
            }),
        );
        // clone(**kw) — return a copy with overrides
        attrs.insert(
            CompactString::from("clone"),
            PyObject::native_closure("clone", |_args: &[PyObjectRef]| {
                let cls =
                    PyObject::class(CompactString::from("EmailPolicy"), vec![], IndexMap::new());
                Ok(PyObject::instance(cls))
            }),
        );
        PyObject::instance_with_attrs(cls, attrs)
    };

    let default_policy = make_policy("default", false, 78);
    let smtp = make_policy("SMTP", false, 998);
    // SMTP policy uses 7bit cte_type
    if let PyObjectPayload::Instance(ref d) = smtp.payload {
        d.attrs.write().insert(
            CompactString::from("cte_type"),
            PyObject::str_val(CompactString::from("7bit")),
        );
    }
    let smtputf8 = make_policy("SMTPUTF8", true, 998);
    if let PyObjectPayload::Instance(ref d) = smtputf8.payload {
        d.attrs.write().insert(
            CompactString::from("cte_type"),
            PyObject::str_val(CompactString::from("8bit")),
        );
    }
    let http = make_policy("HTTP", false, 0);
    let strict = make_policy("strict", false, 78);
    let compat32 = make_policy("compat32", false, 78);

    // EmailPolicy constructor
    let email_policy_fn = make_builtin(|args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("EmailPolicy"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("utf8"), PyObject::bool_val(false));
            w.insert(CompactString::from("max_line_length"), PyObject::int(78));
            w.insert(
                CompactString::from("raise_on_defect"),
                PyObject::bool_val(false),
            );
            // Apply kwargs
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw) = &last.payload {
                    let r = kw.read();
                    for (k, v) in r.iter() {
                        if let ferrython_core::types::HashableKey::Str(key) = k {
                            w.insert(CompactString::from(key.as_str()), v.clone());
                        }
                    }
                }
            }
        }
        Ok(inst)
    });

    make_module(
        "email.policy",
        vec![
            ("EmailPolicy", email_policy_fn),
            ("default", default_policy),
            ("SMTP", smtp),
            ("SMTPUTF8", smtputf8),
            ("HTTP", http),
            ("strict", strict),
            ("compat32", compat32),
        ],
    )
}
