use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use std::rc::Rc;

// ── readline module ──

pub fn create_readline_module() -> PyObjectRef {
    // Shared readline state
    let history: Rc<PyCell<Vec<String>>> = Rc::new(PyCell::new(Vec::new()));
    let history_max_len: Rc<PyCell<i64>> = Rc::new(PyCell::new(-1)); // -1 = unlimited
    let completer: Rc<PyCell<Option<PyObjectRef>>> = Rc::new(PyCell::new(None));
    let completer_delims: Rc<PyCell<String>> = Rc::new(PyCell::new(
        " \t\n`~!@#$%^&*()-=+[{]}\\|;:'\",<>/?".to_string(),
    ));
    let startup_hook: Rc<PyCell<Option<PyObjectRef>>> = Rc::new(PyCell::new(None));
    let pre_input_hook: Rc<PyCell<Option<PyObjectRef>>> = Rc::new(PyCell::new(None));

    let h = history.clone();
    let add_history_fn = PyObject::native_closure("add_history", move |args: &[PyObjectRef]| {
        if !args.is_empty() {
            let line = args[0].py_to_string();
            h.write().push(line);
        }
        Ok(PyObject::none())
    });

    let h = history.clone();
    let clear_history_fn = PyObject::native_closure("clear_history", move |_: &[PyObjectRef]| {
        h.write().clear();
        Ok(PyObject::none())
    });

    let h = history.clone();
    let get_current_history_length_fn =
        PyObject::native_closure("get_current_history_length", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(h.read().len() as i64))
        });

    let ml = history_max_len.clone();
    let get_history_length_fn =
        PyObject::native_closure("get_history_length", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(*ml.read()))
        });

    let ml = history_max_len.clone();
    let set_history_length_fn =
        PyObject::native_closure("set_history_length", move |args: &[PyObjectRef]| {
            if !args.is_empty() {
                let n = args[0].as_int().unwrap_or(-1);
                *ml.write() = n;
            }
            Ok(PyObject::none())
        });

    let h = history.clone();
    let read_history_file_fn =
        PyObject::native_closure("read_history_file", move |args: &[PyObjectRef]| {
            let path = if !args.is_empty() {
                args[0].py_to_string()
            } else {
                "~/.history".to_string()
            };
            let expanded = if path.starts_with('~') {
                if let Ok(home) = std::env::var("HOME") {
                    path.replacen('~', &home, 1)
                } else {
                    path
                }
            } else {
                path
            };
            if let Ok(contents) = std::fs::read_to_string(&expanded) {
                let mut hist = h.write();
                for line in contents.lines() {
                    if !line.is_empty() {
                        hist.push(line.to_string());
                    }
                }
            }
            Ok(PyObject::none())
        });

    let h = history.clone();
    let write_history_file_fn =
        PyObject::native_closure("write_history_file", move |args: &[PyObjectRef]| {
            let path = if !args.is_empty() {
                args[0].py_to_string()
            } else {
                "~/.history".to_string()
            };
            let expanded = if path.starts_with('~') {
                if let Ok(home) = std::env::var("HOME") {
                    path.replacen('~', &home, 1)
                } else {
                    path
                }
            } else {
                path
            };
            let hist = h.read();
            let contents = hist.join("\n");
            let _ = std::fs::write(&expanded, contents);
            Ok(PyObject::none())
        });

    let c = completer.clone();
    let set_completer_fn =
        PyObject::native_closure("set_completer", move |args: &[PyObjectRef]| {
            if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                *c.write() = Some(args[0].clone());
            } else {
                *c.write() = None;
            }
            Ok(PyObject::none())
        });

    let c = completer.clone();
    let get_completer_fn = PyObject::native_closure("get_completer", move |_: &[PyObjectRef]| {
        Ok(c.read().clone().unwrap_or_else(PyObject::none))
    });

    let d = completer_delims.clone();
    let set_completer_delims_fn =
        PyObject::native_closure("set_completer_delims", move |args: &[PyObjectRef]| {
            if !args.is_empty() {
                *d.write() = args[0].py_to_string();
            }
            Ok(PyObject::none())
        });

    let d = completer_delims.clone();
    let get_completer_delims_fn =
        PyObject::native_closure("get_completer_delims", move |_: &[PyObjectRef]| {
            Ok(PyObject::str_val(CompactString::from(d.read().as_str())))
        });

    let sh = startup_hook.clone();
    let set_startup_hook_fn =
        PyObject::native_closure("set_startup_hook", move |args: &[PyObjectRef]| {
            if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                *sh.write() = Some(args[0].clone());
            } else {
                *sh.write() = None;
            }
            Ok(PyObject::none())
        });

    let pih = pre_input_hook.clone();
    let set_pre_input_hook_fn =
        PyObject::native_closure("set_pre_input_hook", move |args: &[PyObjectRef]| {
            if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                *pih.write() = Some(args[0].clone());
            } else {
                *pih.write() = None;
            }
            Ok(PyObject::none())
        });

    let h = history.clone();
    let get_history_item_fn =
        PyObject::native_closure("get_history_item", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "get_history_item requires an index",
                ));
            }
            let idx = args[0].as_int().unwrap_or(0) as usize;
            let hist = h.read();
            // readline uses 1-based indexing
            if idx >= 1 && idx <= hist.len() {
                Ok(PyObject::str_val(CompactString::from(
                    hist[idx - 1].as_str(),
                )))
            } else {
                Ok(PyObject::none())
            }
        });

    let h = history.clone();
    let remove_history_item_fn =
        PyObject::native_closure("remove_history_item", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "remove_history_item requires an index",
                ));
            }
            let idx = args[0].as_int().unwrap_or(0) as usize;
            let mut hist = h.write();
            if idx < hist.len() {
                hist.remove(idx);
            }
            Ok(PyObject::none())
        });

    make_module(
        "readline",
        vec![
            ("parse_and_bind", make_builtin(|_| Ok(PyObject::none()))),
            ("set_completer", set_completer_fn),
            ("get_completer", get_completer_fn),
            ("set_completer_delims", set_completer_delims_fn),
            ("get_completer_delims", get_completer_delims_fn),
            ("add_history", add_history_fn),
            ("clear_history", clear_history_fn),
            ("get_history_length", get_history_length_fn),
            ("set_history_length", set_history_length_fn),
            ("get_current_history_length", get_current_history_length_fn),
            ("get_history_item", get_history_item_fn),
            ("remove_history_item", remove_history_item_fn),
            ("read_history_file", read_history_file_fn),
            ("write_history_file", write_history_file_fn),
            ("set_startup_hook", set_startup_hook_fn),
            ("set_pre_input_hook", set_pre_input_hook_fn),
        ],
    )
}
