use super::*;

pub fn create_fnmatch_module() -> PyObjectRef {
    make_module(
        "fnmatch",
        vec![
            (
                "fnmatch",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("fnmatch requires name and pattern"));
                    }
                    let name = args[0].py_to_string();
                    let pattern = args[1].py_to_string();
                    Ok(PyObject::bool_val(glob_match(&pattern, &name)))
                }),
            ),
            (
                "fnmatchcase",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "fnmatchcase requires name and pattern",
                        ));
                    }
                    let name = args[0].py_to_string();
                    let pattern = args[1].py_to_string();
                    Ok(PyObject::bool_val(glob_match(&pattern, &name)))
                }),
            ),
            (
                "filter",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("filter requires names and pattern"));
                    }
                    let names = args[0].to_list()?;
                    let pattern = args[1].py_to_string();
                    let filtered: Vec<PyObjectRef> = names
                        .iter()
                        .filter(|n| glob_match(&pattern, &n.py_to_string()))
                        .cloned()
                        .collect();
                    Ok(PyObject::list(filtered))
                }),
            ),
            (
                "translate",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("translate requires a pattern"));
                    }
                    let pat = args[0].py_to_string();
                    let mut res = String::from("(?s:");
                    let chars: Vec<char> = pat.chars().collect();
                    let mut i = 0;
                    while i < chars.len() {
                        let c = chars[i];
                        match c {
                            '*' => res.push_str(".*"),
                            '?' => res.push('.'),
                            '[' => {
                                let mut j = i + 1;
                                if j < chars.len() && chars[j] == '!' {
                                    j += 1;
                                }
                                if j < chars.len() && chars[j] == ']' {
                                    j += 1;
                                }
                                while j < chars.len() && chars[j] != ']' {
                                    j += 1;
                                }
                                if j >= chars.len() {
                                    res.push_str("\\[");
                                } else {
                                    let mut stuff = String::new();
                                    for &ch in &chars[i + 1..j] {
                                        stuff.push(ch);
                                    }
                                    stuff = stuff.replace("\\", "\\\\");
                                    let mut bracket = String::from("[");
                                    if stuff.starts_with('!') {
                                        bracket.push('^');
                                        bracket.push_str(&stuff[1..]);
                                    } else {
                                        bracket.push_str(&stuff);
                                    }
                                    bracket.push(']');
                                    res.push_str(&bracket);
                                    i = j;
                                }
                            }
                            _ => {
                                // Escape regex special characters
                                if "(){}+.^$|\\".contains(c) {
                                    res.push('\\');
                                }
                                res.push(c);
                            }
                        }
                        i += 1;
                    }
                    res.push_str(r")\Z");
                    Ok(PyObject::str_val(CompactString::from(res)))
                }),
            ),
        ],
    )
}
