//! Introspection stdlib modules (warnings, traceback, inspect, dis)

use compact_str::CompactString;
use ferrython_core::object::{
    PyObject, PyObjectPayload, PyObjectRef, PyObjectMethods,
    make_module, make_builtin, check_args,
};
use indexmap::IndexMap;

// ── subprocess module (basic) ──


pub fn create_warnings_module() -> PyObjectRef {
    // warn(message, category=UserWarning, stacklevel=1)
    let warn_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        let message = args[0].py_to_string();
        let category = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
            let cat = &args[1];
            if let PyObjectPayload::Class(cd) = &cat.payload {
                cd.name.to_string()
            } else {
                cat.py_to_string()
            }
        } else {
            "UserWarning".to_string()
        };
        // Print warning in CPython format: filename:lineno: category: message
        eprintln!("<stdin>:1: {}: {}", category, message);
        Ok(PyObject::none())
    });

    // filterwarnings(action, message="", category=Warning, module="", lineno=0, append=False)
    let filter_warnings_fn = make_builtin(|_args: &[PyObjectRef]| {
        // Store filter — basic implementation accepts but doesn't enforce
        Ok(PyObject::none())
    });

    // simplefilter(action, category=Warning, append=False)
    let simple_filter_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    // catch_warnings(record=False) — context manager that saves/restores warning filters
    // When record=True, __enter__ returns a list that collects WarningMessage objects
    let catch_warnings_fn = make_builtin(|args: &[PyObjectRef]| {
        // Check for record=True (positional arg or kwarg)
        let record = if !args.is_empty() {
            args[0].is_truthy()
        } else {
            false
        };
        let cls = PyObject::class(CompactString::from("catch_warnings"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let warning_list = PyObject::list(vec![]);
        attrs.insert(CompactString::from("_record"), PyObject::bool_val(record));
        attrs.insert(CompactString::from("_warnings"), warning_list.clone());
        if record {
            // __enter__ returns the warning list for `with ... as w:`
            let wl = warning_list.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "catch_warnings.__enter__", move |_args: &[PyObjectRef]| {
                    Ok(wl.clone())
                }
            ));
        } else {
            attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
                "catch_warnings.__enter__", |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    Ok(args[0].clone())
                }
            ));
        }
        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "catch_warnings.__exit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }
        ));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    make_module("warnings", vec![
        ("warn", warn_fn),
        ("filterwarnings", filter_warnings_fn),
        ("simplefilter", simple_filter_fn),
        ("resetwarnings", make_builtin(|_| Ok(PyObject::none()))),
        ("catch_warnings", catch_warnings_fn),
    ])
}

// ── decimal module (stub) ──


pub fn create_traceback_module() -> PyObjectRef {
    // format_exc() — return formatted exception string (empty when no active exception)
    let format_exc_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::str_val(CompactString::from("")))
    });

    // format_exception(etype, value, tb) — format exception into list of strings
    let format_exception_fn = make_builtin(|args: &[PyObjectRef]| {
        let mut lines = Vec::new();
        if args.len() >= 2 {
            let etype = &args[0];
            let value = &args[1];
            let type_name = if let PyObjectPayload::Class(cd) = &etype.payload {
                cd.name.to_string()
            } else if let PyObjectPayload::ExceptionType(kind) = &etype.payload {
                format!("{:?}", kind)
            } else {
                etype.py_to_string()
            };
            let msg = value.py_to_string();
            if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::None) {
                lines.push(PyObject::str_val(CompactString::from("Traceback (most recent call last):\n")));
                lines.push(PyObject::str_val(CompactString::from("  File \"<unknown>\", line 0, in <module>\n")));
            }
            lines.push(PyObject::str_val(CompactString::from(
                format!("{}: {}\n", type_name, msg)
            )));
        }
        Ok(PyObject::list(lines))
    });

    // print_exc() — print exception info to stderr
    let print_exc_fn = make_builtin(|_args: &[PyObjectRef]| {
        eprintln!("NoneType: None");
        Ok(PyObject::none())
    });

    // format_tb(tb) — format traceback entries as list of strings
    let format_tb_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
            return Ok(PyObject::list(vec![]));
        }
        // Return a basic traceback entry
        Ok(PyObject::list(vec![
            PyObject::str_val(CompactString::from("  File \"<unknown>\", line 0, in <module>\n"))
        ]))
    });

    // extract_tb(tb) — extract FrameSummary-like tuples from traceback
    let extract_tb_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
            return Ok(PyObject::list(vec![]));
        }
        // Return list of (filename, lineno, name, line) tuples
        Ok(PyObject::list(vec![
            PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("<unknown>")),
                PyObject::int(0),
                PyObject::str_val(CompactString::from("<module>")),
                PyObject::none(),
            ])
        ]))
    });

    make_module("traceback", vec![
        ("format_exc", format_exc_fn),
        ("print_exc", print_exc_fn),
        ("format_exception", format_exception_fn),
        ("print_stack", make_builtin(|_| Ok(PyObject::none()))),
        ("format_tb", format_tb_fn),
        ("extract_tb", extract_tb_fn),
    ])
}

// ── warnings module (stub) ──


pub fn create_inspect_module() -> PyObjectRef {
    make_module("inspect", vec![
        ("isfunction", make_builtin(|args| {
            check_args("inspect.isfunction", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Function(_))))
        })),
        ("isclass", make_builtin(|args| {
            check_args("inspect.isclass", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Class(_))))
        })),
        ("ismethod", make_builtin(|args| {
            check_args("inspect.ismethod", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::BoundMethod { .. })))
        })),
        ("ismodule", make_builtin(|args| {
            check_args("inspect.ismodule", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Module(_))))
        })),
        ("isbuiltin", make_builtin(|args| {
            check_args("inspect.isbuiltin", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::NativeFunction { .. } | PyObjectPayload::BuiltinFunction(_) | PyObjectPayload::BuiltinType(_))))
        })),
        ("getmembers", make_builtin(|args| {
            check_args("inspect.getmembers", args, 1)?;
            let dir_names = args[0].dir();
            let dir_list: Vec<PyObjectRef> = dir_names.into_iter().map(|n| PyObject::str_val(n)).collect();
            let names = PyObject::list(dir_list);
            let mut result = Vec::new();
            if let PyObjectPayload::List(items) = &names.payload {
                for item in items.read().iter() {
                    let name_str = item.py_to_string();
                    if let Some(val) = args[0].get_attr(&name_str) {
                        result.push(PyObject::tuple(vec![item.clone(), val]));
                    }
                }
            }
            Ok(PyObject::list(result))
        })),
    ])
}

// ── dis module (stub) ──


pub fn create_dis_module() -> PyObjectRef {
    make_module("dis", vec![
        ("dis", make_builtin(|_| { Ok(PyObject::none()) })),
    ])
}
