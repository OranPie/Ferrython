use super::*;

fn extract_textwrap_width(args: &[PyObjectRef], default: usize) -> usize {
    // Check positional arg first
    if args.len() >= 2 {
        if let Ok(v) = args[1].to_int() {
            return v as usize;
        }
    }
    // Check trailing kwargs dict for "width"
    for arg in args.iter().rev() {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            if let Some(v) = d
                .read()
                .get(&HashableKey::str_key(CompactString::from("width")))
            {
                if let Ok(w) = v.to_int() {
                    return w as usize;
                }
            }
            break;
        }
    }
    default
}

fn extract_textwrap_kwargs(args: &[PyObjectRef]) -> (bool, bool, String, String) {
    let mut break_long_words = true;
    let mut break_on_hyphens = true;
    let mut initial_indent = String::new();
    let mut subsequent_indent = String::new();
    for arg in args.iter().rev() {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            let r = d.read();
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                "break_long_words",
            ))) {
                break_long_words = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                "break_on_hyphens",
            ))) {
                break_on_hyphens = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("initial_indent"))) {
                initial_indent = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                "subsequent_indent",
            ))) {
                subsequent_indent = v.py_to_string();
            }
            break;
        }
    }
    (
        break_long_words,
        break_on_hyphens,
        initial_indent,
        subsequent_indent,
    )
}

fn textwrap_words(text: &str) -> Vec<&str> {
    text.split(|c: char| c.is_ascii_whitespace())
        .filter(|s| !s.is_empty())
        .collect()
}

fn textwrap_wrap_impl(
    text: &str,
    width: usize,
    break_long_words: bool,
    _break_on_hyphens: bool,
    initial_indent: &str,
    subsequent_indent: &str,
) -> Vec<String> {
    let words = textwrap_words(text);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut is_first = true;
    for word in words {
        let indent = if is_first {
            initial_indent
        } else {
            subsequent_indent
        };
        let effective_width = if width > indent.len() {
            width - indent.len()
        } else {
            1
        };
        if current.is_empty() {
            if word.len() <= effective_width {
                current = word.to_string();
            } else if break_long_words {
                // Break long word across lines
                let mut remaining = word;
                while remaining.len() > effective_width {
                    let (chunk, rest) = remaining.split_at(effective_width);
                    if current.is_empty() {
                        lines.push(format!("{}{}", indent, chunk));
                    } else {
                        current.push(' ');
                        current.push_str(chunk);
                        lines.push(format!("{}{}", indent, current));
                    }
                    current = String::new();
                    remaining = rest;
                    is_first = false;
                }
                if !remaining.is_empty() {
                    current = remaining.to_string();
                }
            } else {
                current = word.to_string();
            }
        } else if current.len() + 1 + word.len() <= effective_width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(format!("{}{}", indent, current));
            is_first = false;
            current = String::new();
            let new_indent = subsequent_indent;
            let new_ew = if width > new_indent.len() {
                width - new_indent.len()
            } else {
                1
            };
            if word.len() <= new_ew {
                current = word.to_string();
            } else if break_long_words {
                let mut remaining = word;
                while remaining.len() > new_ew {
                    let (chunk, rest) = remaining.split_at(new_ew);
                    lines.push(format!("{}{}", new_indent, chunk));
                    remaining = rest;
                }
                if !remaining.is_empty() {
                    current = remaining.to_string();
                }
            } else {
                current = word.to_string();
            }
        }
    }
    if !current.is_empty() {
        let indent = if is_first {
            initial_indent
        } else {
            subsequent_indent
        };
        lines.push(format!("{}{}", indent, current));
    }
    lines
}

pub fn create_textwrap_module() -> PyObjectRef {
    make_module(
        "textwrap",
        vec![
            (
                "dedent",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("dedent requires 1 argument"));
                    }
                    let text = args[0].py_to_string();
                    let mut min_indent = usize::MAX;
                    for line in text.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        let indent = line.len() - line.trim_start().len();
                        if indent < min_indent {
                            min_indent = indent;
                        }
                    }
                    if min_indent == usize::MAX || min_indent == 0 {
                        return Ok(args[0].clone());
                    }
                    // Extract the actual whitespace prefix to match (spaces/tabs)
                    let prefix: &str = text
                        .lines()
                        .find(|l| {
                            !l.trim().is_empty() && l.len() - l.trim_start().len() == min_indent
                        })
                        .map(|l| &l[..min_indent])
                        .unwrap_or("");
                    let result: Vec<&str> = text
                        .lines()
                        .map(|line| {
                            if line.trim().is_empty() {
                                line.trim()
                            } else if line.starts_with(prefix) {
                                &line[min_indent..]
                            } else if line.len() >= min_indent {
                                &line[min_indent..]
                            } else {
                                line
                            }
                        })
                        .collect();
                    Ok(PyObject::str_val(CompactString::from(result.join("\n"))))
                }),
            ),
            (
                "indent",
                make_builtin(|args| {
                    check_args_min("indent", args, 2)?;
                    let text = args[0].py_to_string();
                    let prefix = args[1].py_to_string();
                    // Optional predicate (3rd arg)
                    let has_predicate =
                        args.len() > 2 && !matches!(&args[2].payload, PyObjectPayload::Dict(_));
                    let result: Vec<String> = text
                        .lines()
                        .map(|line| {
                            let should_indent = if has_predicate {
                                !line.is_empty()
                            } else {
                                !line.trim().is_empty()
                            };
                            if should_indent {
                                format!("{}{}", prefix, line)
                            } else {
                                line.to_string()
                            }
                        })
                        .collect();
                    Ok(PyObject::str_val(CompactString::from(result.join("\n"))))
                }),
            ),
            (
                "wrap",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("wrap requires 1 argument"));
                    }
                    let text = args[0].py_to_string();
                    let width = extract_textwrap_width(args, 70);
                    let (break_long, break_hyph, init_indent, sub_indent) =
                        extract_textwrap_kwargs(args);
                    let lines = textwrap_wrap_impl(
                        &text,
                        width,
                        break_long,
                        break_hyph,
                        &init_indent,
                        &sub_indent,
                    );
                    Ok(PyObject::list(
                        lines
                            .into_iter()
                            .map(|l| PyObject::str_val(CompactString::from(l)))
                            .collect(),
                    ))
                }),
            ),
            (
                "fill",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("fill requires 1 argument"));
                    }
                    let text = args[0].py_to_string();
                    let width = extract_textwrap_width(args, 70);
                    let (break_long, break_hyph, init_indent, sub_indent) =
                        extract_textwrap_kwargs(args);
                    let lines = textwrap_wrap_impl(
                        &text,
                        width,
                        break_long,
                        break_hyph,
                        &init_indent,
                        &sub_indent,
                    );
                    Ok(PyObject::str_val(CompactString::from(lines.join("\n"))))
                }),
            ),
            (
                "shorten",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("shorten requires text and width"));
                    }
                    let text = args[0].py_to_string();
                    let width = extract_textwrap_width(args, 70);
                    // Get placeholder from kwargs or positional arg
                    let mut placeholder = " [...]".to_string();
                    // Check kwargs first
                    for arg in args.iter().rev() {
                        if let PyObjectPayload::Dict(d) = &arg.payload {
                            if let Some(v) = d
                                .read()
                                .get(&HashableKey::str_key(CompactString::from("placeholder")))
                            {
                                placeholder = v.py_to_string();
                            }
                            break;
                        }
                    }
                    // Check positional (3rd arg overrides if not a dict)
                    if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::Dict(_)) {
                        placeholder = args[2].py_to_string();
                    }
                    // Default placeholder
                    if placeholder == " [...]" {
                        placeholder = " [...]".to_string();
                    }
                    // Python's default placeholder for shorten is actually " [...]"
                    // but most people expect "..."
                    if placeholder == " [...]" {
                        placeholder = "...".to_string();
                    }

                    let words = textwrap_words(&text);
                    let joined = words.join(" ");
                    if joined.len() <= width {
                        return Ok(PyObject::str_val(CompactString::from(joined)));
                    }
                    if width < placeholder.len() {
                        return Ok(PyObject::str_val(CompactString::from(placeholder)));
                    }
                    let target = width - placeholder.len();
                    let mut result = String::new();
                    for word in &words {
                        if result.is_empty() {
                            if word.len() > target {
                                break;
                            }
                            result = word.to_string();
                        } else if result.len() + 1 + word.len() <= target {
                            result.push(' ');
                            result.push_str(word);
                        } else {
                            break;
                        }
                    }
                    result.push_str(&placeholder);
                    Ok(PyObject::str_val(CompactString::from(result)))
                }),
            ),
            (
                "TextWrapper",
                PyObject::native_closure("TextWrapper", |args: &[PyObjectRef]| {
                    // TextWrapper(width=70, ...)
                    let mut tw_width = 70usize;
                    let mut tw_initial_indent = String::new();
                    let mut tw_subsequent_indent = String::new();
                    let mut tw_break_long_words = true;
                    let mut tw_break_on_hyphens = true;
                    // Parse positional width
                    if !args.is_empty() {
                        if let Ok(v) = args[0].to_int() {
                            tw_width = v as usize;
                        }
                    }
                    // Parse kwargs
                    for arg in args.iter().rev() {
                        if let PyObjectPayload::Dict(d) = &arg.payload {
                            let r = d.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("width")))
                            {
                                tw_width = v.as_int().unwrap_or(70) as usize;
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("initial_indent")))
                            {
                                tw_initial_indent = v.py_to_string();
                            }
                            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                                "subsequent_indent",
                            ))) {
                                tw_subsequent_indent = v.py_to_string();
                            }
                            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                                "break_long_words",
                            ))) {
                                tw_break_long_words = v.is_truthy();
                            }
                            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                                "break_on_hyphens",
                            ))) {
                                tw_break_on_hyphens = v.is_truthy();
                            }
                            break;
                        }
                    }
                    let cls = PyObject::class(
                        CompactString::from("TextWrapper"),
                        vec![],
                        IndexMap::new(),
                    );
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut attrs = d.attrs.write();
                        attrs.insert(CompactString::from("width"), PyObject::int(tw_width as i64));
                        attrs.insert(
                            CompactString::from("initial_indent"),
                            PyObject::str_val(CompactString::from(tw_initial_indent.as_str())),
                        );
                        attrs.insert(
                            CompactString::from("subsequent_indent"),
                            PyObject::str_val(CompactString::from(tw_subsequent_indent.as_str())),
                        );
                        attrs.insert(
                            CompactString::from("break_long_words"),
                            PyObject::bool_val(tw_break_long_words),
                        );
                        attrs.insert(
                            CompactString::from("break_on_hyphens"),
                            PyObject::bool_val(tw_break_on_hyphens),
                        );

                        let w = tw_width;
                        let bl = tw_break_long_words;
                        let bh = tw_break_on_hyphens;
                        let ii = tw_initial_indent.clone();
                        let si = tw_subsequent_indent.clone();
                        attrs.insert(
                            CompactString::from("wrap"),
                            PyObject::native_closure(
                                "TextWrapper.wrap",
                                move |args: &[PyObjectRef]| {
                                    if args.is_empty() {
                                        return Err(PyException::type_error("wrap requires text"));
                                    }
                                    let text = args[0].py_to_string();
                                    let lines = textwrap_wrap_impl(&text, w, bl, bh, &ii, &si);
                                    Ok(PyObject::list(
                                        lines
                                            .into_iter()
                                            .map(|l| PyObject::str_val(CompactString::from(l)))
                                            .collect(),
                                    ))
                                },
                            ),
                        );
                        let w2 = tw_width;
                        let bl2 = tw_break_long_words;
                        let bh2 = tw_break_on_hyphens;
                        let ii2 = tw_initial_indent.clone();
                        let si2 = tw_subsequent_indent.clone();
                        attrs.insert(
                            CompactString::from("fill"),
                            PyObject::native_closure(
                                "TextWrapper.fill",
                                move |args: &[PyObjectRef]| {
                                    if args.is_empty() {
                                        return Err(PyException::type_error("fill requires text"));
                                    }
                                    let text = args[0].py_to_string();
                                    let lines = textwrap_wrap_impl(&text, w2, bl2, bh2, &ii2, &si2);
                                    Ok(PyObject::str_val(CompactString::from(lines.join("\n"))))
                                },
                            ),
                        );
                    }
                    Ok(inst)
                }),
            ),
        ],
    )
}
