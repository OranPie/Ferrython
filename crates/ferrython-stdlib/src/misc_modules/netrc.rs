use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

pub fn create_netrc_module() -> PyObjectRef {
    make_module(
        "netrc",
        vec![
            (
                "NetrcParseError",
                PyObject::class(
                    CompactString::from("NetrcParseError"),
                    vec![PyObject::exception_type(ExceptionKind::Exception)],
                    IndexMap::new(),
                ),
            ),
            ("netrc", make_netrc_class()),
        ],
    )
}

fn make_netrc_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("netrc.__init__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("netrc.__init__ requires self"));
            }
            let self_obj = &args[0];
            set_attr(self_obj, "hosts", PyObject::dict(IndexMap::new()))?;
            set_attr(self_obj, "macros", PyObject::dict(IndexMap::new()))?;
            let file = args
                .get(1)
                .filter(|obj| !matches!(obj.payload, PyObjectPayload::None))
                .map(|obj| obj.py_to_string())
                .or_else(default_netrc_path);
            if let Some(path) = file {
                parse_netrc_file(self_obj, &path)?;
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("authenticators"),
        PyObject::native_function("netrc.authenticators", netrc_authenticators),
    );
    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_function("netrc.__repr__", netrc_repr),
    );
    PyObject::class(CompactString::from("netrc"), vec![], ns)
}

fn default_netrc_path() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|home| format!("{}/.netrc", home))
}

fn parse_netrc_file(obj: &PyObjectRef, path: &str) -> PyResult<()> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "machine" => {
                i += 1;
                if i >= tokens.len() {
                    break;
                }
                let machine = tokens[i].to_string();
                i += 1;
                let (login, account, password, next) = parse_login_fields(&tokens, i);
                i = next;
                set_host(obj, &machine, &login, &account, &password)?;
            }
            "default" => {
                i += 1;
                let (login, account, password, next) = parse_login_fields(&tokens, i);
                i = next;
                set_host(obj, "default", &login, &account, &password)?;
            }
            "macdef" => {
                i += 1;
                if i >= tokens.len() {
                    break;
                }
                let name = tokens[i].to_string();
                i += 1;
                let mut lines = Vec::new();
                while i < tokens.len() && !matches!(tokens[i], "machine" | "default" | "macdef") {
                    lines.push(tokens[i]);
                    i += 1;
                }
                set_macro(obj, &name, &lines.join("\n"))?;
            }
            _ => i += 1,
        }
    }
    Ok(())
}

fn parse_login_fields(tokens: &[&str], mut i: usize) -> (String, String, String, usize) {
    let mut login = String::new();
    let mut account = String::new();
    let mut password = String::new();
    while i < tokens.len() && !matches!(tokens[i], "machine" | "default" | "macdef") {
        match tokens[i] {
            "login" => {
                i += 1;
                if i < tokens.len() {
                    login = tokens[i].to_string();
                }
            }
            "account" => {
                i += 1;
                if i < tokens.len() {
                    account = tokens[i].to_string();
                }
            }
            "password" => {
                i += 1;
                if i < tokens.len() {
                    password = tokens[i].to_string();
                }
            }
            _ => {}
        }
        i += 1;
    }
    (login, account, password, i)
}

fn set_host(
    obj: &PyObjectRef,
    host: &str,
    login: &str,
    account: &str,
    password: &str,
) -> PyResult<()> {
    let hosts = get_attr(obj, "hosts")?;
    let PyObjectPayload::Dict(map) = &hosts.payload else {
        return Err(PyException::type_error("hosts must be a dict"));
    };
    map.write().insert(
        HashableKey::str_key(CompactString::from(host)),
        PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(login)),
            PyObject::str_val(CompactString::from(account)),
            PyObject::str_val(CompactString::from(password)),
        ]),
    );
    Ok(())
}

fn set_macro(obj: &PyObjectRef, name: &str, value: &str) -> PyResult<()> {
    let macros = get_attr(obj, "macros")?;
    let PyObjectPayload::Dict(map) = &macros.payload else {
        return Err(PyException::type_error("macros must be a dict"));
    };
    map.write().insert(
        HashableKey::str_key(CompactString::from(name)),
        PyObject::str_val(CompactString::from(value)),
    );
    Ok(())
}

fn netrc_authenticators(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("netrc.authenticators", args, 2)?;
    let hosts = get_attr(&args[0], "hosts")?;
    let host = args[1].py_to_string();
    if let Some(value) = dict_get(&hosts, &host) {
        return Ok(value);
    }
    if let Some(value) = dict_get(&hosts, "default") {
        return Ok(value);
    }
    Ok(PyObject::none())
}

fn netrc_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("netrc.__repr__", args, 1)?;
    let hosts = get_attr(&args[0], "hosts")?;
    let macros = get_attr(&args[0], "macros")?;
    let mut out = String::new();
    if let PyObjectPayload::Dict(map) = &hosts.payload {
        for (key, value) in map.read().iter() {
            let host = key.to_object().py_to_string();
            let vals = value.to_list().unwrap_or_default();
            let login = vals.first().map(|v| v.py_to_string()).unwrap_or_default();
            let account = vals.get(1).map(|v| v.py_to_string()).unwrap_or_default();
            let password = vals.get(2).map(|v| v.py_to_string()).unwrap_or_default();
            out.push_str(&format!("machine {}\n\tlogin {}\n", host, login));
            if !account.is_empty() {
                out.push_str(&format!("\taccount {}\n", account));
            }
            out.push_str(&format!("\tpassword {}\n", password));
        }
    }
    if let PyObjectPayload::Dict(map) = &macros.payload {
        for (key, value) in map.read().iter() {
            out.push_str(&format!(
                "macdef {}\n{}\n",
                key.to_object().py_to_string(),
                value.py_to_string()
            ));
        }
    }
    Ok(PyObject::str_val(CompactString::from(out)))
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
