use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};

/// pickletools — Pickle protocol analysis tools (stub)
pub fn create_pickletools_module() -> PyObjectRef {
    make_module(
        "pickletools",
        vec![
            (
                "genops",
                make_builtin(|args| {
                    // genops(pickle) → iterator of (opcode, arg, pos) tuples
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "genops requires a pickle bytes argument",
                        ));
                    }
                    Ok(PyObject::list(vec![]))
                }),
            ),
            ("dis", make_builtin(|_args| Ok(PyObject::none()))),
            (
                "optimize",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("optimize requires an argument"));
                    }
                    Ok(args[0].clone())
                }),
            ),
        ],
    )
}
