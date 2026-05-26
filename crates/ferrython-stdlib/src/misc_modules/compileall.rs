use ferrython_core::object::{
    check_args_min, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef,
};

// ── compileall module ──

pub fn create_compileall_module() -> PyObjectRef {
    // compile_file(fullname, ddir=None, force=False, rx=None, quiet=0)
    let compile_file = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("compileall.compile_file", args, 1)?;
        let path = args[0].py_to_string();
        if !path.ends_with(".py") {
            return Ok(PyObject::bool_val(true));
        }
        let source = match std::fs::read_to_string(&*path) {
            Ok(s) => s,
            Err(_) => return Ok(PyObject::bool_val(false)),
        };
        match ferrython_parser::parse(&source, &*path) {
            Ok(module) => match ferrython_compiler::compile(&module, &*path) {
                Ok(_) => Ok(PyObject::bool_val(true)),
                Err(_) => Ok(PyObject::bool_val(false)),
            },
            Err(_) => Ok(PyObject::bool_val(false)),
        }
    });

    // compile_dir(dir, maxlevels=10, ddir=None, force=False, rx=None, quiet=0)
    let compile_dir = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("compileall.compile_dir", args, 1)?;
        let dir = args[0].py_to_string();
        let max_levels = if args.len() >= 2 {
            args[1].to_int().unwrap_or(10)
        } else {
            10
        };
        fn compile_dir_recursive(dir: &str, levels: i64) -> bool {
            if levels < 0 {
                return true;
            }
            let entries = match std::fs::read_dir(dir) {
                Ok(e) => e,
                Err(_) => return false,
            };
            let mut ok = true;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "py" {
                            let p = path.to_string_lossy().to_string();
                            let source = match std::fs::read_to_string(&p) {
                                Ok(s) => s,
                                Err(_) => {
                                    ok = false;
                                    continue;
                                }
                            };
                            if ferrython_parser::parse(&source, &p)
                                .map_err(|_| ())
                                .and_then(|m| ferrython_compiler::compile(&m, &p).map_err(|_| ()))
                                .is_err()
                            {
                                ok = false;
                            }
                        }
                    }
                } else if path.is_dir() && levels > 0 {
                    if !compile_dir_recursive(&path.to_string_lossy(), levels - 1) {
                        ok = false;
                    }
                }
            }
            ok
        }
        Ok(PyObject::bool_val(compile_dir_recursive(&dir, max_levels)))
    });

    make_module(
        "compileall",
        vec![
            ("compile_dir", compile_dir),
            ("compile_file", compile_file),
            (
                "compile_path",
                make_builtin(|_| Ok(PyObject::bool_val(true))),
            ),
        ],
    )
}
