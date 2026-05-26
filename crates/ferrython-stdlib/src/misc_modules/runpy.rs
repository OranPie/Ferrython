use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── runpy module ──

pub fn create_runpy_module() -> PyObjectRef {
    // run_path(path, init_globals=None, run_name=None) -> dict
    // Reads file, compiles it, stores code + globals to be executed by VM deferred mechanism
    let run_path = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("runpy.run_path", args, 1)?;
        let path = args[0].py_to_string();
        let source = std::fs::read_to_string(&*path)
            .map_err(|e| PyException::os_error(format!("Cannot read {}: {}", path, e)))?;
        // Build a globals dict for the executed module
        let run_name = if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::None) {
            args[2].py_to_string().to_string()
        } else {
            "<run_path>".to_string()
        };
        let mut ns = IndexMap::new();
        ns.insert(
            HashableKey::str_key(CompactString::from("__name__")),
            PyObject::str_val(CompactString::from(run_name.as_str())),
        );
        ns.insert(
            HashableKey::str_key(CompactString::from("__file__")),
            PyObject::str_val(CompactString::from(&*path)),
        );
        // Merge init_globals if provided
        if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
            if let PyObjectPayload::Dict(ref d) = args[1].payload {
                for (k, v) in d.read().iter() {
                    ns.insert(k.clone(), v.clone());
                }
            }
        }
        let result_dict = PyObject::dict(ns);

        // Use the compile + exec mechanism via deferred call
        // Since we can't directly invoke the VM here, we compile to code object
        // and attach it along with the namespace for the VM to pick up
        let _code_src = PyObject::str_val(CompactString::from(source.as_str()));
        let _filename = PyObject::str_val(CompactString::from(&*path));

        // Store (source, filename, globals_dict) for deferred execution
        // The caller should use exec() in Python land instead.
        // For simplicity, compile and push as deferred call to builtins.exec
        // Actually, we need to push compile+exec as a deferred pair.
        // Simpler approach: use DEFERRED_CALLS with a special marker.

        // For now: compile the source using the compiler and return the namespace.
        // The file gets parsed and compiled to bytecode.
        match ferrython_parser::parse(&source, &*path) {
            Ok(module) => match ferrython_compiler::compile(&module, &*path) {
                Ok(code) => {
                    let code_obj = PyObject::code(code);
                    crate::concurrency_modules::push_deferred_call(
                        PyObject::str_val(CompactString::from("__runpy_exec__")),
                        vec![code_obj, result_dict.clone()],
                    );
                    Ok(result_dict)
                }
                Err(e) => Err(PyException::syntax_error(format!(
                    "Failed to compile {}: {:?}",
                    path, e
                ))),
            },
            Err(e) => Err(PyException::syntax_error(format!(
                "Failed to parse {}: {:?}",
                path, e
            ))),
        }
    });

    // run_module(mod_name, run_name=None, alter_sys=False) -> dict
    let run_module = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("runpy.run_module", args, 1)?;
        let mod_name = args[0].py_to_string();
        // Try to find module file by checking current directory and common paths
        let search_paths: Vec<String> = {
            let mut paths = vec![".".to_string()];
            // Add PYTHONPATH entries if available
            if let Ok(pp) = std::env::var("PYTHONPATH") {
                for p in pp.split(':') {
                    if !p.is_empty() {
                        paths.push(p.to_string());
                    }
                }
            }
            paths
        };
        for dir in &search_paths {
            // Check module_name.py
            let file_path = format!("{}/{}.py", dir, mod_name);
            if std::path::Path::new(&file_path).exists() {
                let source = std::fs::read_to_string(&file_path).map_err(|e| {
                    PyException::os_error(format!("Cannot read {}: {}", file_path, e))
                })?;
                let run_name =
                    if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
                        args[1].py_to_string().to_string()
                    } else {
                        mod_name.to_string()
                    };
                let mut ns = IndexMap::new();
                ns.insert(
                    HashableKey::str_key(CompactString::from("__name__")),
                    PyObject::str_val(CompactString::from(run_name.as_str())),
                );
                ns.insert(
                    HashableKey::str_key(CompactString::from("__file__")),
                    PyObject::str_val(CompactString::from(file_path.as_str())),
                );
                let result_dict = PyObject::dict(ns);
                match ferrython_parser::parse(&source, &file_path) {
                    Ok(module) => match ferrython_compiler::compile(&module, &file_path) {
                        Ok(code) => {
                            let code_obj = PyObject::code(code);
                            crate::concurrency_modules::push_deferred_call(
                                PyObject::str_val(CompactString::from("__runpy_exec__")),
                                vec![code_obj, result_dict.clone()],
                            );
                            return Ok(result_dict);
                        }
                        Err(e) => {
                            return Err(PyException::syntax_error(format!(
                                "Failed to compile {}: {:?}",
                                file_path, e
                            )))
                        }
                    },
                    Err(e) => {
                        return Err(PyException::syntax_error(format!(
                            "Failed to parse {}: {:?}",
                            file_path, e
                        )))
                    }
                }
            }
            // Check module_name/__main__.py
            let pkg_main = format!("{}/{}/__main__.py", dir, mod_name);
            if std::path::Path::new(&pkg_main).exists() {
                let source = std::fs::read_to_string(&pkg_main).map_err(|e| {
                    PyException::os_error(format!("Cannot read {}: {}", pkg_main, e))
                })?;
                let mut ns = IndexMap::new();
                ns.insert(
                    HashableKey::str_key(CompactString::from("__name__")),
                    PyObject::str_val(CompactString::from("__main__")),
                );
                ns.insert(
                    HashableKey::str_key(CompactString::from("__file__")),
                    PyObject::str_val(CompactString::from(pkg_main.as_str())),
                );
                let result_dict = PyObject::dict(ns);
                match ferrython_parser::parse(&source, &pkg_main) {
                    Ok(module) => match ferrython_compiler::compile(&module, &pkg_main) {
                        Ok(code) => {
                            let code_obj = PyObject::code(code);
                            crate::concurrency_modules::push_deferred_call(
                                PyObject::str_val(CompactString::from("__runpy_exec__")),
                                vec![code_obj, result_dict.clone()],
                            );
                            return Ok(result_dict);
                        }
                        Err(e) => {
                            return Err(PyException::syntax_error(format!(
                                "Failed to compile {}: {:?}",
                                pkg_main, e
                            )))
                        }
                    },
                    Err(e) => {
                        return Err(PyException::syntax_error(format!(
                            "Failed to parse {}: {:?}",
                            pkg_main, e
                        )))
                    }
                }
            }
        }
        Err(PyException::import_error(format!(
            "No module named '{}'",
            mod_name
        )))
    });

    make_module(
        "runpy",
        vec![("run_module", run_module), ("run_path", run_path)],
    )
}
