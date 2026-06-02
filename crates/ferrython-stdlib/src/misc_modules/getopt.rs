use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::rc::Rc;

const PUBLIC_NAMES: &[&str] = &[
    "GetoptError",
    "error",
    "getopt",
    "gnu_getopt",
    "do_longs",
    "do_shorts",
    "long_has_args",
    "short_has_arg",
];

thread_local! {
    static GETOPT_ERROR_CLASS: RefCell<Option<PyObjectRef>> = const { RefCell::new(None) };
}

pub fn create_getopt_module() -> PyObjectRef {
    let error_cls = get_or_create_getopt_error_class();
    make_module(
        "getopt",
        vec![
            ("GetoptError", error_cls.clone()),
            ("error", error_cls.clone()),
            ("getopt", make_builtin(getopt_getopt)),
            ("gnu_getopt", make_builtin(getopt_gnu_getopt)),
            ("do_longs", make_builtin(getopt_do_longs)),
            ("do_shorts", make_builtin(getopt_do_shorts)),
            ("long_has_args", make_builtin(getopt_long_has_args)),
            ("short_has_arg", make_builtin(getopt_short_has_arg)),
            ("__all__", string_list(PUBLIC_NAMES)),
        ],
    )
}

fn get_or_create_getopt_error_class() -> PyObjectRef {
    if let Some(class) = GETOPT_ERROR_CLASS.with(|slot| slot.borrow().clone()) {
        return class;
    }
    let class = build_getopt_error_class();
    GETOPT_ERROR_CLASS.with(|slot| *slot.borrow_mut() = Some(class.clone()));
    class
}

fn build_getopt_error_class() -> PyObjectRef {
    let class_slot: Rc<parking_lot::RwLock<Option<PyObjectRef>>> =
        Rc::new(parking_lot::RwLock::new(None));
    let init_class = class_slot.clone();
    let str_class = class_slot.clone();
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("GetoptError.__init__", move |args| {
            let real = bound_args(args, &init_class);
            if real.len() < 2 {
                return Err(PyException::type_error(
                    "GetoptError.__init__ requires self and msg",
                ));
            }
            if real.len() > 3 {
                return Err(PyException::type_error(
                    "GetoptError.__init__ expected at most 3 arguments",
                ));
            }
            set_error_attrs(&real[0], real[1].clone(), real.get(2).cloned())?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__str__"),
        PyObject::native_closure("GetoptError.__str__", move |args| {
            let real = bound_args(args, &str_class);
            let self_obj = real
                .first()
                .ok_or_else(|| PyException::type_error("__str__ requires self"))?;
            Ok(
                get_error_msg(self_obj)
                    .unwrap_or_else(|| PyObject::str_val(CompactString::new(""))),
            )
        }),
    );
    let class = PyObject::class(
        CompactString::from("GetoptError"),
        vec![PyObject::exception_type(ExceptionKind::Exception)],
        ns,
    );
    *class_slot.write() = Some(class.clone());
    class
}

fn bound_args<'a>(
    args: &'a [PyObjectRef],
    class_slot: &Rc<parking_lot::RwLock<Option<PyObjectRef>>>,
) -> &'a [PyObjectRef] {
    if args.len() > 1 {
        if let Some(class) = class_slot.read().as_ref() {
            if PyObjectRef::ptr_eq(&args[0], class) {
                return &args[1..];
            }
        }
    }
    args
}

fn set_error_attrs(obj: &PyObjectRef, msg: PyObjectRef, opt: Option<PyObjectRef>) -> PyResult<()> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return Err(PyException::type_error(
            "GetoptError.__init__ requires self",
        ));
    };
    let opt = opt.unwrap_or_else(|| PyObject::str_val(CompactString::new("")));
    let mut attrs = inst.attrs.write();
    attrs.insert(CompactString::from("msg"), msg.clone());
    attrs.insert(CompactString::from("opt"), opt);
    attrs.insert(CompactString::from("args"), PyObject::tuple(vec![msg]));
    Ok(())
}

fn get_error_msg(obj: &PyObjectRef) -> Option<PyObjectRef> {
    obj.get_attr("msg")
}

fn make_getopt_error(message: String, opt: String) -> PyException {
    let class = get_or_create_getopt_error_class();
    let inst = PyObject::instance(class);
    let msg_obj = PyObject::str_val(CompactString::from(message.clone()));
    let opt_obj = PyObject::str_val(CompactString::from(opt));
    let _ = set_error_attrs(&inst, msg_obj.clone(), Some(opt_obj));
    PyException::with_original(ExceptionKind::Exception, message, inst)
}

fn string_list(items: &[&str]) -> PyObjectRef {
    PyObject::list(
        items
            .iter()
            .map(|item| PyObject::str_val(CompactString::from(*item)))
            .collect(),
    )
}

fn object_string_list(obj: &PyObjectRef) -> PyResult<Vec<String>> {
    if let Some(text) = obj.as_str() {
        return Ok(vec![text.to_string()]);
    }
    obj.to_list()
        .map(|items| items.iter().map(|item| item.py_to_string()).collect())
}

fn string_vec_obj(items: Vec<String>) -> PyObjectRef {
    PyObject::list(
        items
            .into_iter()
            .map(|item| PyObject::str_val(CompactString::from(item)))
            .collect(),
    )
}

fn option_tuple(option: String, value: String) -> PyObjectRef {
    PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(option)),
        PyObject::str_val(CompactString::from(value)),
    ])
}

fn option_list_obj(opts: &[(String, String)]) -> PyObjectRef {
    PyObject::list(
        opts.iter()
            .map(|(opt, val)| option_tuple(opt.clone(), val.clone()))
            .collect(),
    )
}

fn parse_option_pairs(obj: &PyObjectRef) -> PyResult<Vec<(String, String)>> {
    let mut result = Vec::new();
    for item in obj.to_list()? {
        let parts = item.to_list()?;
        if parts.len() != 2 {
            return Err(PyException::type_error(
                "option entries must be 2-sequences",
            ));
        }
        result.push((parts[0].py_to_string(), parts[1].py_to_string()));
    }
    Ok(result)
}

fn getopt_getopt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "getopt() requires args and shortopts",
        ));
    }
    let argv: Vec<String> = args[0]
        .to_list()?
        .iter()
        .map(|item| item.py_to_string())
        .collect();
    let shortopts = args[1].py_to_string();
    let longopts = args
        .get(2)
        .map(object_string_list)
        .transpose()?
        .unwrap_or_default();
    let (opts, rest) = parse_getopt(argv, &shortopts, &longopts)?;
    Ok(PyObject::tuple(vec![
        option_list_obj(&opts),
        string_vec_obj(rest),
    ]))
}

fn getopt_gnu_getopt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "gnu_getopt() requires args and shortopts",
        ));
    }
    let argv: Vec<String> = args[0]
        .to_list()?
        .iter()
        .map(|item| item.py_to_string())
        .collect();
    let mut shortopts = args[1].py_to_string();
    let longopts = args
        .get(2)
        .map(object_string_list)
        .transpose()?
        .unwrap_or_default();
    let all_options_first = if let Some(stripped) = shortopts.strip_prefix('+') {
        shortopts = stripped.to_string();
        true
    } else {
        std::env::var("POSIXLY_CORRECT").is_ok()
    };
    let (opts, rest) = parse_gnu_getopt(argv, &shortopts, &longopts, all_options_first)?;
    Ok(PyObject::tuple(vec![
        option_list_obj(&opts),
        string_vec_obj(rest),
    ]))
}

fn getopt_short_has_arg(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("getopt.short_has_arg", args, 2)?;
    let opt = args[0].py_to_string();
    let ch = opt.chars().next().unwrap_or('\0');
    Ok(PyObject::bool_val(short_has_arg(
        ch,
        &args[1].py_to_string(),
    )?))
}

fn getopt_long_has_args(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("getopt.long_has_args", args, 2)?;
    let longopts = object_string_list(&args[1])?;
    let (has_arg, option) = long_has_args(&args[0].py_to_string(), &longopts)?;
    Ok(PyObject::tuple(vec![
        PyObject::bool_val(has_arg),
        PyObject::str_val(CompactString::from(option)),
    ]))
}

fn getopt_do_longs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("getopt.do_longs", args, 4)?;
    let mut opts = parse_option_pairs(&args[0])?;
    let longopts = object_string_list(&args[2])?;
    let rest: Vec<String> = args[3]
        .to_list()?
        .iter()
        .map(|item| item.py_to_string())
        .collect();
    let (new_opts, new_rest) = do_longs(&mut opts, &args[1].py_to_string(), &longopts, rest)?;
    Ok(PyObject::tuple(vec![
        option_list_obj(new_opts),
        string_vec_obj(new_rest.clone()),
    ]))
}

fn getopt_do_shorts(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("getopt.do_shorts", args, 4)?;
    let mut opts = parse_option_pairs(&args[0])?;
    let rest: Vec<String> = args[3]
        .to_list()?
        .iter()
        .map(|item| item.py_to_string())
        .collect();
    let (new_opts, new_rest) = do_shorts(
        &mut opts,
        &args[1].py_to_string(),
        &args[2].py_to_string(),
        rest,
    )?;
    Ok(PyObject::tuple(vec![
        option_list_obj(new_opts),
        string_vec_obj(new_rest.clone()),
    ]))
}

fn parse_getopt(
    mut args: Vec<String>,
    shortopts: &str,
    longopts: &[String],
) -> PyResult<(Vec<(String, String)>, Vec<String>)> {
    let mut opts = Vec::new();
    while !args.is_empty() && args[0].starts_with('-') && args[0] != "-" {
        if args[0] == "--" {
            args.remove(0);
            break;
        }
        let current = args.remove(0);
        if let Some(long) = current.strip_prefix("--") {
            let rest = args;
            let (_, new_rest) = do_longs(&mut opts, long, longopts, rest)?;
            args = new_rest;
        } else {
            let rest = args;
            let (_, new_rest) = do_shorts(&mut opts, &current[1..], shortopts, rest)?;
            args = new_rest;
        }
    }
    Ok((opts, args))
}

fn parse_gnu_getopt(
    mut args: Vec<String>,
    shortopts: &str,
    longopts: &[String],
    all_options_first: bool,
) -> PyResult<(Vec<(String, String)>, Vec<String>)> {
    let mut opts = Vec::new();
    let mut prog_args = Vec::new();
    while !args.is_empty() {
        if args[0] == "--" {
            prog_args.extend(args.into_iter().skip(1));
            break;
        }
        if args[0].starts_with("--") {
            let current = args.remove(0);
            let rest = args;
            let (_, new_rest) = do_longs(&mut opts, &current[2..], longopts, rest)?;
            args = new_rest;
        } else if args[0].starts_with('-') && args[0] != "-" {
            let current = args.remove(0);
            let rest = args;
            let (_, new_rest) = do_shorts(&mut opts, &current[1..], shortopts, rest)?;
            args = new_rest;
        } else if all_options_first {
            prog_args.extend(args);
            break;
        } else {
            prog_args.push(args.remove(0));
        }
    }
    Ok((opts, prog_args))
}

fn short_has_arg(opt: char, shortopts: &str) -> PyResult<bool> {
    let chars: Vec<char> = shortopts.chars().collect();
    for (idx, shortopt) in chars.iter().enumerate() {
        if opt == *shortopt && opt != ':' {
            return Ok(chars.get(idx + 1).is_some_and(|next| *next == ':'));
        }
    }
    Err(make_getopt_error(
        format!("option -{} not recognized", opt),
        opt.to_string(),
    ))
}

fn long_has_args(opt: &str, longopts: &[String]) -> PyResult<(bool, String)> {
    let possibilities: Vec<&str> = longopts
        .iter()
        .map(String::as_str)
        .filter(|candidate| candidate.starts_with(opt))
        .collect();
    if possibilities.is_empty() {
        return Err(make_getopt_error(
            format!("option --{} not recognized", opt),
            opt.to_string(),
        ));
    }
    if possibilities.contains(&opt) {
        return Ok((false, opt.to_string()));
    }
    let arg_form = format!("{}=", opt);
    if possibilities.iter().any(|candidate| *candidate == arg_form) {
        return Ok((true, opt.to_string()));
    }
    if possibilities.len() > 1 {
        return Err(make_getopt_error(
            format!("option --{} not a unique prefix", opt),
            opt.to_string(),
        ));
    }
    let mut matched = possibilities[0].to_string();
    let has_arg = matched.ends_with('=');
    if has_arg {
        matched.pop();
    }
    Ok((has_arg, matched))
}

fn do_longs<'a>(
    opts: &'a mut Vec<(String, String)>,
    opt: &str,
    longopts: &[String],
    mut args: Vec<String>,
) -> PyResult<(&'a Vec<(String, String)>, Vec<String>)> {
    let (name, optarg) = opt
        .split_once('=')
        .map(|(name, value)| (name.to_string(), Some(value.to_string())))
        .unwrap_or_else(|| (opt.to_string(), None));
    let (has_arg, canonical) = long_has_args(&name, longopts)?;
    let value = if has_arg {
        if let Some(value) = optarg {
            value
        } else if args.is_empty() {
            return Err(make_getopt_error(
                format!("option --{} requires argument", canonical),
                canonical,
            ));
        } else {
            args.remove(0)
        }
    } else if optarg.is_some() {
        return Err(make_getopt_error(
            format!("option --{} must not have an argument", canonical),
            canonical,
        ));
    } else {
        String::new()
    };
    opts.push((format!("--{}", canonical), value));
    Ok((opts, args))
}

fn do_shorts<'a>(
    opts: &'a mut Vec<(String, String)>,
    optstring: &str,
    shortopts: &str,
    mut args: Vec<String>,
) -> PyResult<(&'a Vec<(String, String)>, Vec<String>)> {
    let chars: Vec<char> = optstring.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let opt = chars[i];
        i += 1;
        if short_has_arg(opt, shortopts)? {
            let value = if i < chars.len() {
                let value: String = chars[i..].iter().collect();
                i = chars.len();
                value
            } else if args.is_empty() {
                return Err(make_getopt_error(
                    format!("option -{} requires argument", opt),
                    opt.to_string(),
                ));
            } else {
                args.remove(0)
            };
            opts.push((format!("-{}", opt), value));
        } else {
            opts.push((format!("-{}", opt), String::new()));
        }
    }
    Ok((opts, args))
}
