use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

// ── pdb module ──

pub fn create_pdb_module() -> PyObjectRef {
    let set_trace_fn = make_builtin(|_args: &[PyObjectRef]| {
        eprintln!("(Pdb) > <stdin>: breakpoint");
        Ok(PyObject::none())
    });

    let pm_fn = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));

    let run_fn = make_builtin(|args: &[PyObjectRef]| {
        let _ = args;
        Ok(PyObject::none())
    });

    let runeval_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("runeval requires an expression"));
        }
        Ok(PyObject::none())
    });

    let runcall_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("runcall requires a function"));
        }
        // Call the function with remaining args
        let func = &args[0];
        let call_args = if args.len() > 1 { &args[1..] } else { &[] };
        match &func.payload {
            PyObjectPayload::NativeFunction(nf) => (nf.func)(call_args),
            PyObjectPayload::NativeClosure(nc) => (nc.func)(call_args),
            _ => Ok(PyObject::none()),
        }
    });

    // Breakpoint class
    let bp_cls = PyObject::class(CompactString::from("Breakpoint"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = bp_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("bpbynumber"),
            PyObject::list(vec![PyObject::none()]),
        );
        ns.insert(
            CompactString::from("bplist"),
            PyObject::dict(IndexMap::new()),
        );
        let bp_init = make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 3 {
                return Err(PyException::type_error(
                    "Breakpoint() requires file and line",
                ));
            }
            let inst = &args[0];
            let file = args[1].py_to_string();
            let line = args[2].to_int().unwrap_or(0);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(
                    CompactString::from("file"),
                    PyObject::str_val(CompactString::from(&file)),
                );
                w.insert(CompactString::from("line"), PyObject::int(line));
                w.insert(CompactString::from("enabled"), PyObject::bool_val(true));
                w.insert(CompactString::from("temporary"), PyObject::bool_val(false));
                w.insert(CompactString::from("cond"), PyObject::none());
                w.insert(CompactString::from("hits"), PyObject::int(0));
                static BP_NUM: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
                let num = BP_NUM.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                w.insert(CompactString::from("number"), PyObject::int(num));
                w.insert(
                    CompactString::from("enable"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                w.insert(
                    CompactString::from("disable"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
            }
            Ok(PyObject::none())
        });
        ns.insert(CompactString::from("__init__"), bp_init);
        ns.insert(
            CompactString::from("clearBreakpoints"),
            make_builtin(|_| Ok(PyObject::none())),
        );
    }

    // Bdb class
    let bdb_cls = PyObject::class(CompactString::from("Bdb"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = bdb_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("set_break"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("clear_break"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("clear_all_breaks"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("set_step"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("set_next"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("set_return"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("set_continue"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("set_quit"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("get_all_breaks"),
            make_builtin(|_| Ok(PyObject::dict(IndexMap::new()))),
        );
    }

    // Pdb class
    let pdb_cls = PyObject::class(
        CompactString::from("Pdb"),
        vec![bdb_cls.clone()],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = pdb_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("prompt"),
            PyObject::str_val(CompactString::from("(Pdb) ")),
        );
        ns.insert(
            CompactString::from("set_trace"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("run"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("set_break"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("clear_all_breaks"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("get_all_breaks"),
            make_builtin(|_| Ok(PyObject::dict(IndexMap::new()))),
        );
    }

    make_module(
        "pdb",
        vec![
            ("set_trace", set_trace_fn),
            ("pm", pm_fn),
            ("run", run_fn),
            ("runeval", runeval_fn),
            ("runcall", runcall_fn),
            (
                "post_mortem",
                make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
            ),
            ("Pdb", pdb_cls),
            ("Bdb", bdb_cls),
            ("Breakpoint", bp_cls),
        ],
    )
}
