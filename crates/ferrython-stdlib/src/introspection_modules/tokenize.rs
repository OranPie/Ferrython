use super::*;

// ── tokenize module ──

pub fn create_tokenize_module() -> PyObjectRef {
    fn tokenize_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args("generate_tokens", args, 1)?;
        // args[0] should be a readline callable; collect all lines first
        let source = if let PyObjectPayload::NativeClosure(nc) = &args[0].payload {
            let mut lines = String::new();
            loop {
                let line = (nc.func)(&[])?;
                let s = line.py_to_string();
                if s.is_empty() {
                    break;
                }
                lines.push_str(&s);
            }
            lines
        } else {
            // Fallback: treat as string source
            args[0].py_to_string()
        };
        let mut tokens = Vec::new();
        let mut indent_stack: Vec<usize> = vec![0];

        for (lineno, line) in source.lines().enumerate() {
            let lineno = lineno + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                tokens.push(make_token_info(
                    61,
                    "",
                    (lineno, 0),
                    (lineno, line.len()),
                    line,
                ));
                continue;
            }
            if trimmed.starts_with('#') {
                tokens.push(make_token_info(
                    60,
                    trimmed,
                    (lineno, 0),
                    (lineno, line.len()),
                    line,
                ));
                continue;
            }
            // Count leading indent
            let indent = line.len() - line.trim_start().len();
            let prev = *indent_stack.last().unwrap_or(&0);
            if indent > prev {
                indent_stack.push(indent);
                tokens.push(make_token_info(5, "", (lineno, 0), (lineno, indent), line));
            // INDENT
            } else {
                while indent < *indent_stack.last().unwrap_or(&0) {
                    indent_stack.pop();
                    tokens.push(make_token_info(6, "", (lineno, 0), (lineno, 0), line));
                    // DEDENT
                }
            }
            let mut col = 0;
            let chars: Vec<char> = line.chars().collect();
            while col < chars.len() {
                if chars[col].is_whitespace() {
                    col += 1;
                    continue;
                }
                if chars[col] == '#' {
                    // Inline comment — consume rest of line
                    let comment: String = chars[col..].iter().collect();
                    tokens.push(make_token_info(
                        60,
                        &comment,
                        (lineno, col),
                        (lineno, chars.len()),
                        line,
                    ));
                    let _ = chars.len(); // col unused
                    break;
                }
                let start_col = col;
                if chars[col].is_alphabetic() || chars[col] == '_' {
                    while col < chars.len() && (chars[col].is_alphanumeric() || chars[col] == '_') {
                        col += 1;
                    }
                    let word: String = chars[start_col..col].iter().collect();
                    tokens.push(make_token_info(
                        1,
                        &word,
                        (lineno, start_col),
                        (lineno, col),
                        line,
                    ));
                } else if chars[col].is_ascii_digit() {
                    while col < chars.len()
                        && (chars[col].is_ascii_digit()
                            || chars[col] == '.'
                            || chars[col] == 'e'
                            || chars[col] == 'E'
                            || chars[col] == '_')
                    {
                        col += 1;
                    }
                    let num: String = chars[start_col..col].iter().collect();
                    tokens.push(make_token_info(
                        2,
                        &num,
                        (lineno, start_col),
                        (lineno, col),
                        line,
                    ));
                } else if chars[col] == '"' || chars[col] == '\'' {
                    let quote = chars[col];
                    col += 1;
                    while col < chars.len() && chars[col] != quote {
                        col += 1;
                    }
                    if col < chars.len() {
                        col += 1;
                    }
                    let s: String = chars[start_col..col].iter().collect();
                    tokens.push(make_token_info(
                        3,
                        &s,
                        (lineno, start_col),
                        (lineno, col),
                        line,
                    ));
                } else {
                    // Multi-character operators
                    let mut end = col + 1;
                    if end < chars.len() {
                        let two: String = chars[col..end + 1].iter().collect();
                        if matches!(
                            two.as_str(),
                            "==" | "!="
                                | "<="
                                | ">="
                                | "**"
                                | "//"
                                | "<<"
                                | ">>"
                                | "+="
                                | "-="
                                | "*="
                                | "/="
                                | "%="
                                | "&="
                                | "|="
                                | "^="
                                | "->"
                                | ":="
                        ) {
                            end += 1;
                            // Check for 3-char ops
                            if end < chars.len() {
                                let three: String = chars[col..end + 1].iter().collect();
                                if matches!(three.as_str(), "**=" | "//=" | "<<=" | ">>=") {
                                    end += 1;
                                }
                            }
                        }
                    }
                    let op: String = chars[col..end].iter().collect();
                    col = end;
                    tokens.push(make_token_info(
                        54,
                        &op,
                        (lineno, start_col),
                        (lineno, col),
                        line,
                    ));
                }
            }
            tokens.push(make_token_info(
                4,
                "\n",
                (lineno, line.len()),
                (lineno, line.len() + 1),
                line,
            ));
        }
        // Emit remaining DEDENT tokens
        while indent_stack.len() > 1 {
            indent_stack.pop();
            tokens.push(make_token_info(6, "", (0, 0), (0, 0), ""));
        }
        tokens.push(make_token_info(0, "", (0, 0), (0, 0), ""));
        Ok(PyObject::list(tokens))
    }

    fn make_token_info(
        type_id: i64,
        string: &str,
        start: (usize, usize),
        end: (usize, usize),
        line: &str,
    ) -> PyObjectRef {
        PyObject::tuple(vec![
            PyObject::int(type_id),
            PyObject::str_val(CompactString::from(string)),
            PyObject::tuple(vec![
                PyObject::int(start.0 as i64),
                PyObject::int(start.1 as i64),
            ]),
            PyObject::tuple(vec![
                PyObject::int(end.0 as i64),
                PyObject::int(end.1 as i64),
            ]),
            PyObject::str_val(CompactString::from(line)),
        ])
    }

    fn tokenize_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args("open", args, 1)?;
        let filename = args[0].py_to_string();
        let content = std::fs::read_to_string(filename.as_str())
            .map_err(|e| PyException::os_error(format!("{}", e)))?;
        Ok(PyObject::str_val(CompactString::from(content)))
    }

    fn detect_encoding(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("utf-8")),
            PyObject::list(vec![]),
        ]))
    }

    // Build tok_name mapping (same as token.tok_name)
    let tok_name_entries: Vec<(i64, &str)> = vec![
        (0, "ENDMARKER"),
        (1, "NAME"),
        (2, "NUMBER"),
        (3, "STRING"),
        (4, "NEWLINE"),
        (5, "INDENT"),
        (6, "DEDENT"),
        (54, "OP"),
        (59, "ERRORTOKEN"),
        (60, "COMMENT"),
        (61, "NL"),
        (62, "ENCODING"),
    ];
    let mut tok_name_map = IndexMap::new();
    for (id, name) in &tok_name_entries {
        tok_name_map.insert(
            HashableKey::Int(ferrython_core::types::PyInt::Small(*id)),
            PyObject::str_val(CompactString::from(*name)),
        );
    }
    let tok_name = PyObject::dict(tok_name_map);

    // TokenInfo namedtuple-like class
    let mut ti_ns = IndexMap::new();
    ti_ns.insert(
        CompactString::from("_fields"),
        PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("type")),
            PyObject::str_val(CompactString::from("string")),
            PyObject::str_val(CompactString::from("start")),
            PyObject::str_val(CompactString::from("end")),
            PyObject::str_val(CompactString::from("line")),
        ]),
    );
    let token_info_cls = PyObject::class(CompactString::from("TokenInfo"), vec![], ti_ns);

    make_module(
        "tokenize",
        vec![
            ("generate_tokens", make_builtin(tokenize_string)),
            ("tokenize", make_builtin(tokenize_string)),
            ("open", make_builtin(tokenize_open)),
            ("detect_encoding", make_builtin(detect_encoding)),
            ("tok_name", tok_name),
            ("TokenInfo", token_info_cls),
            ("ENDMARKER", PyObject::int(0)),
            ("NAME", PyObject::int(1)),
            ("NUMBER", PyObject::int(2)),
            ("STRING", PyObject::int(3)),
            ("NEWLINE", PyObject::int(4)),
            ("INDENT", PyObject::int(5)),
            ("DEDENT", PyObject::int(6)),
            ("OP", PyObject::int(54)),
            ("COMMENT", PyObject::int(60)),
            ("NL", PyObject::int(61)),
            ("ENCODING", PyObject::int(62)),
            ("ERRORTOKEN", PyObject::int(59)),
            ("EXACT_TOKEN_TYPES", PyObject::dict(IndexMap::new())),
        ],
    )
}
