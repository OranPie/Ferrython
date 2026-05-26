use compact_str::CompactString;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

// ── sysconfig module ──

pub fn create_sysconfig_module() -> PyObjectRef {
    let layout = ferrython_toolchain::paths::InstallLayout::discover();

    let get_python_version =
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::str_val(CompactString::from("3.11"))));

    let get_platform = make_builtin(|_args: &[PyObjectRef]| {
        let platform = if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "windows") {
            "win32"
        } else {
            "unknown"
        };
        Ok(PyObject::str_val(CompactString::from(platform)))
    });

    let layout_path = layout.clone();
    let get_path = PyObject::native_closure("get_path", move |args: &[PyObjectRef]| {
        let name = if args.is_empty() {
            String::from("stdlib")
        } else {
            args[0].py_to_string()
        };
        let path_val = layout_path
            .get_path(&name)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(PyObject::str_val(CompactString::from(path_val)))
    });

    let layout_paths = layout.clone();
    let get_paths = PyObject::native_closure("get_paths", move |_args: &[PyObjectRef]| {
        let names = ["stdlib", "purelib", "platlib", "include", "scripts", "data"];
        let pairs: Vec<(PyObjectRef, PyObjectRef)> = names
            .iter()
            .map(|name| {
                let path_val = layout_paths
                    .get_path(name)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                (
                    PyObject::str_val(CompactString::from(*name)),
                    PyObject::str_val(CompactString::from(path_val)),
                )
            })
            .collect();
        Ok(PyObject::dict_from_pairs(pairs))
    });

    let layout_var = layout.clone();
    let get_config_var =
        PyObject::native_closure("get_config_var", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let name = args[0].py_to_string();
            match layout_var.get_config_var(&name) {
                Some(val) => Ok(PyObject::str_val(CompactString::from(val))),
                None => Ok(PyObject::none()),
            }
        });

    let layout_vars = layout.clone();
    let get_config_vars =
        PyObject::native_closure("get_config_vars", move |_args: &[PyObjectRef]| {
            let keys = [
                "prefix",
                "exec_prefix",
                "base_prefix",
                "BINDIR",
                "py_version_short",
                "SOABI",
                "EXT_SUFFIX",
                "SIZEOF_VOID_P",
                "installed_base",
            ];
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = keys
                .iter()
                .filter_map(|k| {
                    layout_vars.get_config_var(k).map(|v| {
                        (
                            PyObject::str_val(CompactString::from(*k)),
                            PyObject::str_val(CompactString::from(v)),
                        )
                    })
                })
                .collect();
            Ok(PyObject::dict_from_pairs(pairs))
        });

    let get_default_scheme = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::str_val(CompactString::from("posix_prefix")))
    });

    let get_scheme_names = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::list(vec![
            PyObject::str_val(CompactString::from("posix_prefix")),
            PyObject::str_val(CompactString::from("posix_user")),
            PyObject::str_val(CompactString::from("nt")),
            PyObject::str_val(CompactString::from("nt_user")),
        ]))
    });

    make_module(
        "sysconfig",
        vec![
            ("get_python_version", get_python_version),
            ("get_platform", get_platform),
            ("get_path", get_path),
            ("get_paths", get_paths),
            ("get_config_var", get_config_var),
            ("get_config_vars", get_config_vars),
            ("get_default_scheme", get_default_scheme),
            ("get_scheme_names", get_scheme_names),
        ],
    )
}

// ── grp module (Unix group database) ──
