use compact_str::CompactString;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

// ── site module ──

pub fn create_site_module() -> PyObjectRef {
    let layout = ferrython_toolchain::paths::InstallLayout::discover();
    let site_packages_str = layout.site_packages.to_string_lossy().to_string();
    let prefix_str = layout.prefix.to_string_lossy().to_string();

    let sp_clone = site_packages_str.clone();
    let getsitepackages =
        PyObject::native_closure("getsitepackages", move |_args: &[PyObjectRef]| {
            Ok(PyObject::list(vec![PyObject::str_val(
                CompactString::from(sp_clone.as_str()),
            )]))
        });

    let getusersitepackages = make_builtin(|_args: &[PyObjectRef]| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let user_site = format!("{}/.local/lib/ferrython/site-packages", home);
        Ok(PyObject::str_val(CompactString::from(user_site)))
    });

    let addsitedir = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(ferrython_core::error::PyException::type_error(
                "addsitedir requires 1 argument",
            ));
        }
        let _dir = args[0].py_to_string();
        Ok(PyObject::none())
    });

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let user_base = format!("{}/.local", home);
    let user_site = format!("{}/lib/ferrython/site-packages", user_base);

    make_module(
        "site",
        vec![
            ("ENABLE_USER_SITE", PyObject::bool_val(true)),
            (
                "USER_SITE",
                PyObject::str_val(CompactString::from(user_site.as_str())),
            ),
            (
                "USER_BASE",
                PyObject::str_val(CompactString::from(user_base.as_str())),
            ),
            (
                "PREFIXES",
                PyObject::list(vec![PyObject::str_val(CompactString::from(
                    prefix_str.as_str(),
                ))]),
            ),
            ("getusersitepackages", getusersitepackages),
            ("getsitepackages", getsitepackages),
            ("addsitedir", addsitedir),
        ],
    )
}

// ── sched module ──
