use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    call_callable, check_args, make_builtin, make_module, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::RefCell;

thread_local! {
    static WEBBROWSERS: RefCell<Vec<(String, PyObjectRef)>> = const { RefCell::new(Vec::new()) };
    static TRYORDER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static GENERIC_BROWSER_CLASS: RefCell<Option<PyObjectRef>> = const { RefCell::new(None) };
}

pub fn create_webbrowser_module() -> PyObjectRef {
    let error = PyObject::class(
        CompactString::from("Error"),
        vec![PyObject::exception_type(ExceptionKind::Exception)],
        IndexMap::new(),
    );
    let base = make_base_browser_class();
    let generic = make_generic_browser_class(base.clone());
    GENERIC_BROWSER_CLASS.with(|cell| {
        *cell.borrow_mut() = Some(generic.clone());
    });
    let browsers = PyObject::dict(IndexMap::new());
    let tryorder = PyObject::list(vec![]);
    make_module(
        "webbrowser",
        vec![
            ("Error", error),
            ("BaseBrowser", base),
            ("GenericBrowser", generic),
            ("_browsers", browsers),
            ("_tryorder", tryorder),
            ("_escape_url", make_builtin(webbrowser_escape_url)),
            ("register", make_builtin(webbrowser_register)),
            ("get", make_builtin(webbrowser_get)),
            ("open", make_builtin(webbrowser_open)),
            ("open_new", make_builtin(webbrowser_open_new)),
            ("open_new_tab", make_builtin(webbrowser_open_new_tab)),
        ],
    )
}

fn make_base_browser_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("BaseBrowser.__init__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "BaseBrowser.__init__ requires self",
                ));
            }
            let name = args
                .get(1)
                .map(|obj| obj.py_to_string())
                .unwrap_or_default();
            set_attr(
                &args[0],
                "name",
                PyObject::str_val(CompactString::from(&name)),
            )?;
            set_attr(
                &args[0],
                "basename",
                PyObject::str_val(CompactString::from(name)),
            )?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("open"),
        PyObject::native_function("BaseBrowser.open", |_args| {
            Err(PyException::not_implemented_error("open"))
        }),
    );
    ns.insert(
        CompactString::from("open_new"),
        PyObject::native_function("BaseBrowser.open_new", browser_open_new_method),
    );
    ns.insert(
        CompactString::from("open_new_tab"),
        PyObject::native_function("BaseBrowser.open_new_tab", browser_open_new_tab_method),
    );
    PyObject::class(CompactString::from("BaseBrowser"), vec![], ns)
}

fn make_generic_browser_class(base: PyObjectRef) -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("GenericBrowser.__init__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("GenericBrowser requires name"));
            }
            let self_obj = &args[0];
            let (name, argv) = match &args[1].payload {
                PyObjectPayload::Str(_) => {
                    let name = args[1].py_to_string();
                    (
                        name.clone(),
                        vec![PyObject::str_val(CompactString::from(name))],
                    )
                }
                _ => {
                    let items = args[1].to_list().unwrap_or_else(|_| vec![args[1].clone()]);
                    let name = items.first().map(|v| v.py_to_string()).unwrap_or_default();
                    (name, items)
                }
            };
            set_attr(
                self_obj,
                "name",
                PyObject::str_val(CompactString::from(&name)),
            )?;
            set_attr(
                self_obj,
                "basename",
                PyObject::str_val(CompactString::from(name)),
            )?;
            set_attr(self_obj, "args", PyObject::list(argv))?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("open"),
        PyObject::native_function("GenericBrowser.open", generic_browser_open),
    );
    PyObject::class(CompactString::from("GenericBrowser"), vec![base], ns)
}

fn browser_open_new_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("open_new requires url"));
    }
    call_browser_open(&args[0], &args[1], 1)
}

fn browser_open_new_tab_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("open_new_tab requires url"));
    }
    call_browser_open(&args[0], &args[1], 2)
}

fn generic_browser_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("open requires url"));
    }
    let argv = get_attr(&args[0], "args")
        .ok()
        .and_then(|obj| obj.to_list().ok())
        .unwrap_or_default();
    let mut command = argv
        .iter()
        .map(|obj| obj.py_to_string())
        .collect::<Vec<_>>()
        .join(" ");
    if !command.is_empty() {
        command.push(' ');
    }
    command.push_str(&escape_url(&args[1].py_to_string()));
    run_shell_command(&format!("{} 2>/dev/null &", command));
    Ok(PyObject::bool_val(true))
}

fn webbrowser_escape_url(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("webbrowser._escape_url", args, 1)?;
    Ok(PyObject::str_val(CompactString::from(escape_url(
        &args[0].py_to_string(),
    ))))
}

fn webbrowser_register(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("register requires name"));
    }
    let (pos_args, kwargs) = split_kwargs(args);
    let name = args[0].py_to_string();
    let klass = kwargs
        .as_ref()
        .and_then(|kw| dict_get(kw, "klass"))
        .or_else(|| pos_args.get(1).cloned())
        .unwrap_or_else(PyObject::none);
    let instance = kwargs
        .as_ref()
        .and_then(|kw| dict_get(kw, "instance"))
        .or_else(|| pos_args.get(2).cloned())
        .unwrap_or_else(PyObject::none);
    let preferred = kwargs
        .as_ref()
        .and_then(|kw| dict_get(kw, "preferred"))
        .or_else(|| pos_args.get(3).cloned())
        .is_some_and(|obj| obj.is_truthy());
    let entry = if !matches!(instance.payload, PyObjectPayload::None) {
        Some(instance)
    } else if !matches!(klass.payload, PyObjectPayload::None) {
        Some(klass)
    } else {
        None
    };
    if let Some(entry) = entry {
        WEBBROWSERS.with(|cell| {
            let mut browsers = cell.borrow_mut();
            browsers.retain(|(existing, _)| existing != &name);
            browsers.push((name.clone(), entry));
        });
    }
    TRYORDER.with(|cell| {
        let mut items = cell.borrow_mut();
        let exists = items.iter().any(|item| item == &name);
        if preferred {
            items.retain(|item| item != &name);
            items.insert(0, name);
        } else if !exists {
            items.push(name);
        }
    });
    Ok(PyObject::none())
}

fn webbrowser_get(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos_args, kwargs) = split_kwargs(args);
    let using = kwargs
        .as_ref()
        .and_then(|kw| dict_get(kw, "using"))
        .or_else(|| {
            pos_args
                .first()
                .filter(|obj| !matches!(obj.payload, PyObjectPayload::None))
                .cloned()
        })
        .map(|obj| obj.py_to_string());
    if let Some(name) = using {
        if let Some(browser) = find_browser(&name) {
            return browser_for_entry(&name, browser);
        }
        return Err(PyException::new(
            ExceptionKind::Exception,
            format!("could not locate runnable browser: {}", name),
        ));
    }
    for name in TRYORDER.with(|cell| cell.borrow().clone()) {
        if let Some(browser) = find_browser(&name) {
            return browser_for_entry(&name, browser);
        }
    }
    generic_browser("xdg-open")
}

fn webbrowser_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("open requires url"));
    }
    let url = escape_url(&args[0].py_to_string());
    let command = if cfg!(target_os = "macos") {
        format!("open {} 2>/dev/null &", url)
    } else if cfg!(target_os = "windows") {
        format!("start {}", url)
    } else {
        format!("xdg-open {} 2>/dev/null &", url)
    };
    run_shell_command(&command);
    Ok(PyObject::bool_val(true))
}

fn webbrowser_open_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("open_new requires url"));
    }
    webbrowser_open(&[args[0].clone(), PyObject::int(1)])
}

fn webbrowser_open_new_tab(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("open_new_tab requires url"));
    }
    webbrowser_open(&[args[0].clone(), PyObject::int(2)])
}

fn call_browser_open(browser: &PyObjectRef, url: &PyObjectRef, mode: i64) -> PyResult<PyObjectRef> {
    let open = browser
        .get_attr("open")
        .ok_or_else(|| PyException::attribute_error("open"))?;
    call_callable(&open, &[url.clone(), PyObject::int(mode)])
}

fn find_browser(name: &str) -> Option<PyObjectRef> {
    WEBBROWSERS.with(|cell| {
        cell.borrow()
            .iter()
            .find(|(existing, _)| existing == name)
            .map(|(_, value)| value.clone())
    })
}

fn split_kwargs(args: &[PyObjectRef]) -> (&[PyObjectRef], Option<PyObjectRef>) {
    if let Some(last) = args.last() {
        if matches!(last.payload, PyObjectPayload::Dict(_)) {
            return (&args[..args.len() - 1], Some(last.clone()));
        }
    }
    (args, None)
}

fn browser_for_entry(name: &str, entry: PyObjectRef) -> PyResult<PyObjectRef> {
    if matches!(entry.payload, PyObjectPayload::Class(_)) {
        call_callable(&entry, &[PyObject::str_val(CompactString::from(name))])
    } else {
        Ok(entry)
    }
}

fn generic_browser(name: &str) -> PyResult<PyObjectRef> {
    let cls = GENERIC_BROWSER_CLASS
        .with(|cell| cell.borrow().clone())
        .ok_or_else(|| PyException::runtime_error("webbrowser module unavailable"))?;
    call_callable(&cls, &[PyObject::str_val(CompactString::from(name))])
}

fn escape_url(url: &str) -> String {
    let mut out = String::new();
    for ch in url.chars() {
        if matches!(
            ch,
            '"' | '\'' | '\\' | ' ' | '(' | ')' | '&' | '|' | ';' | '$' | '`'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn run_shell_command(command: &str) {
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .status();
}

fn dict_get(dict: &PyObjectRef, key: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &dict.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(key)))
        .cloned()
}

fn set_attr(obj: &PyObjectRef, name: &str, value: PyObjectRef) -> PyResult<()> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs.write().insert(CompactString::from(name), value);
        Ok(())
    } else {
        Err(PyException::type_error("expected instance"))
    }
}

fn get_attr(obj: &PyObjectRef, name: &str) -> PyResult<PyObjectRef> {
    obj.get_attr(name)
        .ok_or_else(|| PyException::attribute_error(name))
}
