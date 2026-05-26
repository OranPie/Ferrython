use super::*;

pub fn create_shlex_module() -> PyObjectRef {
    fn shlex_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("shlex.split requires 1 argument"));
        }
        let s = args[0].py_to_string();
        let mut result = Vec::new();
        let mut current = String::new();
        let mut in_single = false;
        let mut in_double = false;
        let mut escape_next = false;
        for c in s.chars() {
            if escape_next {
                current.push(c);
                escape_next = false;
                continue;
            }
            if c == '\\' && !in_single {
                escape_next = true;
                continue;
            }
            if c == '\'' && !in_double {
                in_single = !in_single;
                continue;
            }
            if c == '"' && !in_single {
                in_double = !in_double;
                continue;
            }
            if c.is_whitespace() && !in_single && !in_double {
                if !current.is_empty() {
                    result.push(PyObject::str_val(CompactString::from(&current)));
                    current.clear();
                }
                continue;
            }
            current.push(c);
        }
        if !current.is_empty() {
            result.push(PyObject::str_val(CompactString::from(&current)));
        }
        Ok(PyObject::list(result))
    }

    fn shlex_quote(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("shlex.quote requires 1 argument"));
        }
        let s = args[0].py_to_string();
        if s.is_empty() {
            return Ok(PyObject::str_val(CompactString::from("''")));
        }
        // If safe chars only, return as-is
        if s.chars().all(|c| {
            c.is_alphanumeric()
                || matches!(c, '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-' | '_')
        }) {
            return Ok(PyObject::str_val(CompactString::from(&s)));
        }
        // Wrap in single quotes, escaping any single quotes
        let escaped = s.replace('\'', "'\"'\"'");
        Ok(PyObject::str_val(CompactString::from(format!(
            "'{}'",
            escaped
        ))))
    }

    fn shlex_join(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("shlex.join requires 1 argument"));
        }
        let items = match &args[0].payload {
            PyObjectPayload::List(items) => items.read().clone(),
            PyObjectPayload::Tuple(items) => (**items).clone(),
            _ => return Err(PyException::type_error("shlex.join expects an iterable")),
        };
        let parts: Vec<String> = items
            .iter()
            .map(|item| {
                let s = item.py_to_string();
                if s.is_empty()
                    || s.chars().any(|c| {
                        c.is_whitespace()
                            || matches!(
                                c,
                                '\'' | '"'
                                    | '\\'
                                    | '|'
                                    | '&'
                                    | ';'
                                    | '('
                                    | ')'
                                    | '<'
                                    | '>'
                                    | '!'
                                    | '`'
                                    | '$'
                                    | '{'
                                    | '}'
                                    | '['
                                    | ']'
                            )
                    })
                {
                    let escaped = s.replace('\'', "'\"'\"'");
                    format!("'{}'", escaped)
                } else {
                    s
                }
            })
            .collect();
        Ok(PyObject::str_val(CompactString::from(parts.join(" "))))
    }

    make_module(
        "shlex",
        vec![
            ("split", make_builtin(shlex_split)),
            ("quote", make_builtin(shlex_quote)),
            ("join", make_builtin(shlex_join)),
        ],
    )
}
