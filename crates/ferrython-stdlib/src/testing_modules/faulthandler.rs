use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};

// ── faulthandler module ──

pub fn create_faulthandler_module() -> PyObjectRef {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ENABLED: AtomicBool = AtomicBool::new(false);

    let enable = PyObject::native_closure("faulthandler.enable", move |_args: &[PyObjectRef]| {
        ENABLED.store(true, Ordering::Relaxed);
        Ok(PyObject::none())
    });
    let disable = PyObject::native_closure("faulthandler.disable", move |_: &[PyObjectRef]| {
        ENABLED.store(false, Ordering::Relaxed);
        Ok(PyObject::none())
    });
    let is_enabled =
        PyObject::native_closure("faulthandler.is_enabled", move |_: &[PyObjectRef]| {
            Ok(PyObject::bool_val(ENABLED.load(Ordering::Relaxed)))
        });
    let dump_traceback = make_builtin(|_args: &[PyObjectRef]| {
        eprintln!("Current thread (main thread):");
        eprintln!("  File \"<unknown>\", line 0 in <module>");
        Ok(PyObject::none())
    });
    let register_fn = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));
    let unregister_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
    let dump_traceback_later = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));
    let cancel_dump = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));

    make_module(
        "faulthandler",
        vec![
            ("enable", enable),
            ("disable", disable),
            ("is_enabled", is_enabled),
            ("dump_traceback", dump_traceback),
            ("dump_traceback_later", dump_traceback_later),
            ("cancel_dump_traceback_later", cancel_dump),
            ("register", register_fn),
            ("unregister", unregister_fn),
        ],
    )
}
