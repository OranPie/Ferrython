use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;

static CURRENT_CTYPE_LOCALE: std::sync::LazyLock<parking_lot::RwLock<(String, String)>> =
    std::sync::LazyLock::new(|| {
        parking_lot::RwLock::new(("C".to_string(), "ANSI_X3.4-1968".to_string()))
    });

pub fn get_current_ctype_locale() -> (String, String) {
    CURRENT_CTYPE_LOCALE.read().clone()
}

// ── locale module ──

pub fn create_locale_module() -> PyObjectRef {
    // Detect system locale from environment
    fn get_system_locale() -> (String, String) {
        let lang = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_CTYPE"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_else(|_| "C".to_string());
        if lang == "C" || lang == "POSIX" || lang.is_empty() {
            return ("C".to_string(), "ANSI_X3.4-1968".to_string());
        }
        // Parse "en_US.UTF-8" into ("en_US", "UTF-8")
        if let Some(dot) = lang.find('.') {
            let locale_name = &lang[..dot];
            let encoding =
                lang[dot + 1..].trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-');
            (locale_name.to_string(), encoding.to_string())
        } else {
            (lang.clone(), "UTF-8".to_string())
        }
    }

    let current_locale: Rc<PyCell<(String, String)>> = Rc::new(PyCell::new(get_system_locale()));
    *CURRENT_CTYPE_LOCALE.write() = current_locale.read().clone();

    let cl1 = current_locale.clone();
    let getlocale_fn = PyObject::native_closure("getlocale", move |_: &[PyObjectRef]| {
        let (name, enc) = cl1.read().clone();
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(name)),
            PyObject::str_val(CompactString::from(enc)),
        ]))
    });

    let cl2 = current_locale.clone();
    let setlocale_fn = PyObject::native_closure("setlocale", move |args: &[PyObjectRef]| {
        let _category = if !args.is_empty() {
            args[0].to_int().unwrap_or(0)
        } else {
            0
        };
        if args.len() >= 2 {
            let locale_str = args[1].py_to_string();
            if locale_str.is_empty() || locale_str == "C" || locale_str == "POSIX" {
                *cl2.write() = ("C".to_string(), "ANSI_X3.4-1968".to_string());
            } else if let Some(dot) = locale_str.find('.') {
                *cl2.write() = (
                    locale_str[..dot].to_string(),
                    locale_str[dot + 1..].to_string(),
                );
            } else {
                *cl2.write() = (locale_str.clone(), "UTF-8".to_string());
            }
            *CURRENT_CTYPE_LOCALE.write() = cl2.read().clone();
        }
        let (name, enc) = cl2.read().clone();
        Ok(PyObject::str_val(CompactString::from(format!(
            "{}.{}",
            name, enc
        ))))
    });

    let cl3 = current_locale.clone();
    let localeconv_fn = PyObject::native_closure("localeconv", move |_: &[PyObjectRef]| {
        let (name, _) = cl3.read().clone();
        let is_c = name == "C" || name == "POSIX";
        let mut conv = IndexMap::new();
        conv.insert(
            CompactString::from("decimal_point"),
            PyObject::str_val(CompactString::from(".")),
        );
        conv.insert(
            CompactString::from("thousands_sep"),
            PyObject::str_val(CompactString::from(if is_c { "" } else { "," })),
        );
        conv.insert(
            CompactString::from("grouping"),
            PyObject::list(if is_c {
                vec![]
            } else {
                vec![PyObject::int(3), PyObject::int(0)]
            }),
        );
        conv.insert(
            CompactString::from("int_curr_symbol"),
            PyObject::str_val(CompactString::from("")),
        );
        conv.insert(
            CompactString::from("currency_symbol"),
            PyObject::str_val(CompactString::from("")),
        );
        conv.insert(
            CompactString::from("mon_decimal_point"),
            PyObject::str_val(CompactString::from(".")),
        );
        conv.insert(
            CompactString::from("mon_thousands_sep"),
            PyObject::str_val(CompactString::from(if is_c { "" } else { "," })),
        );
        conv.insert(CompactString::from("p_sign_posn"), PyObject::int(1));
        conv.insert(CompactString::from("n_sign_posn"), PyObject::int(1));
        let dict = PyObject::dict_from_pairs(
            conv.into_iter()
                .map(|(k, v)| (PyObject::str_val(k), v))
                .collect(),
        );
        Ok(dict)
    });

    let normalize_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("normalize() requires 1 argument"));
        }
        let s = args[0].py_to_string();
        // Simple normalization: "en_US" → "en_US.UTF-8"
        if !s.contains('.') {
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}.UTF-8",
                s
            ))))
        } else {
            Ok(PyObject::str_val(CompactString::from(s)))
        }
    });

    make_module(
        "locale",
        vec![
            ("getlocale", getlocale_fn),
            ("setlocale", setlocale_fn),
            ("localeconv", localeconv_fn),
            ("normalize", normalize_fn),
            (
                "getpreferredencoding",
                make_builtin(|_| Ok(PyObject::str_val(CompactString::from("UTF-8")))),
            ),
            (
                "getdefaultlocale",
                make_builtin(|_| {
                    let lang = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string());
                    let (name, enc) = if let Some(dot) = lang.find('.') {
                        (lang[..dot].to_string(), lang[dot + 1..].to_string())
                    } else {
                        (lang, "UTF-8".to_string())
                    };
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(name)),
                        PyObject::str_val(CompactString::from(enc)),
                    ]))
                }),
            ),
            ("LC_CTYPE", PyObject::int(0)),
            ("LC_NUMERIC", PyObject::int(1)),
            ("LC_TIME", PyObject::int(2)),
            ("LC_COLLATE", PyObject::int(3)),
            ("LC_MONETARY", PyObject::int(4)),
            ("LC_MESSAGES", PyObject::int(5)),
            ("LC_ALL", PyObject::int(6)),
            ("CHAR_MAX", PyObject::int(127)),
            (
                "Error",
                PyObject::builtin_type(CompactString::from("locale.Error")),
            ),
            (
                "str",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::str_val(CompactString::from("")));
                    }
                    Ok(PyObject::str_val(CompactString::from(
                        args[0].py_to_string(),
                    )))
                }),
            ),
            (
                "atof",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("atof() requires 1 argument"));
                    }
                    let s = args[0].py_to_string().replace(',', "");
                    let f: f64 = s.parse().map_err(|_| {
                        PyException::value_error(format!("could not convert '{}' to float", s))
                    })?;
                    Ok(PyObject::float(f))
                }),
            ),
            (
                "atoi",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("atoi() requires 1 argument"));
                    }
                    let s = args[0].py_to_string().replace(',', "");
                    let n: i64 = s.parse().map_err(|_| {
                        PyException::value_error(format!("could not convert '{}' to int", s))
                    })?;
                    Ok(PyObject::int(n))
                }),
            ),
        ],
    )
}

// ── inspect module (stub) ──

// ── getpass module ───────────────────────────────────────────────────
