use super::*;

// ── difflib module ───────────────────────────────────────────────────

/// Compute matching blocks between two string sequences using LCS dynamic programming.
/// Returns (a_start, b_start, size) triples with a sentinel (a.len(), b.len(), 0).
fn find_matching_blocks(a: &[String], b: &[String]) -> Vec<(usize, usize, usize)> {
    let m = a.len();
    let n = b.len();
    if m == 0 || n == 0 {
        return vec![(m, n, 0)];
    }
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 && j > 0 {
        if a[i - 1] == b[j - 1] {
            pairs.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] >= dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    pairs.reverse();
    let mut blocks = Vec::new();
    let mut idx = 0;
    while idx < pairs.len() {
        let (sa, sb) = pairs[idx];
        let mut cnt = 1;
        while idx + cnt < pairs.len()
            && pairs[idx + cnt].0 == sa + cnt
            && pairs[idx + cnt].1 == sb + cnt
        {
            cnt += 1;
        }
        blocks.push((sa, sb, cnt));
        idx += cnt;
    }
    blocks.push((m, n, 0));
    blocks
}

/// Convert matching blocks to opcodes (tag, a_start, a_end, b_start, b_end).
fn opcodes_from_matching_blocks(
    blocks: &[(usize, usize, usize)],
) -> Vec<(String, usize, usize, usize, usize)> {
    let mut ops = Vec::new();
    let (mut ai, mut bj) = (0usize, 0usize);
    for &(a_start, b_start, size) in blocks {
        let tag = if ai < a_start && bj < b_start {
            Some("replace")
        } else if ai < a_start {
            Some("delete")
        } else if bj < b_start {
            Some("insert")
        } else {
            None
        };
        if let Some(t) = tag {
            ops.push((t.to_string(), ai, a_start, bj, b_start));
        }
        if size > 0 {
            ops.push((
                "equal".to_string(),
                a_start,
                a_start + size,
                b_start,
                b_start + size,
            ));
        }
        ai = a_start + size;
        bj = b_start + size;
    }
    ops
}

/// Group opcodes into hunks for diff output, respecting context size n.
fn group_opcodes(
    opcodes: &[(String, usize, usize, usize, usize)],
    n: usize,
) -> Vec<Vec<(String, usize, usize, usize, usize)>> {
    let mut codes: Vec<(String, usize, usize, usize, usize)> = if opcodes.is_empty() {
        vec![("equal".to_string(), 0, 1, 0, 1)]
    } else {
        opcodes.to_vec()
    };
    if codes[0].0 == "equal" {
        let (ref t, i1, i2, j1, j2) = codes[0];
        codes[0] = (
            t.clone(),
            i2.saturating_sub(n).max(i1),
            i2,
            j2.saturating_sub(n).max(j1),
            j2,
        );
    }
    let last = codes.len() - 1;
    if codes[last].0 == "equal" {
        let (ref t, i1, i2, j1, j2) = codes[last];
        codes[last] = (t.clone(), i1, (i1 + n).min(i2), j1, (j1 + n).min(j2));
    }
    let nn = n + n;
    let mut groups: Vec<Vec<(String, usize, usize, usize, usize)>> = Vec::new();
    let mut group: Vec<(String, usize, usize, usize, usize)> = Vec::new();
    for (tag, i1, i2, j1, j2) in codes {
        if tag == "equal" && i2 - i1 > nn {
            group.push((tag.clone(), i1, (i1 + n).min(i2), j1, (j1 + n).min(j2)));
            groups.push(group);
            group = Vec::new();
            let ni1 = i2.saturating_sub(n).max(i1);
            let nj1 = j2.saturating_sub(n).max(j1);
            group.push((tag, ni1, i2, nj1, j2));
        } else {
            group.push((tag, i1, i2, j1, j2));
        }
    }
    if !group.is_empty() && group.iter().any(|(t, ..)| t != "equal") {
        groups.push(group);
    }
    groups
}

fn format_range_unified(start: usize, stop: usize) -> String {
    let beginning = start + 1;
    let length = stop - start;
    if length == 1 {
        format!("{}", beginning)
    } else if length == 0 {
        format!("{},0", start)
    } else {
        format!("{},{}", beginning, length)
    }
}

fn format_range_context(start: usize, stop: usize) -> String {
    let beginning = start + 1;
    let length = stop - start;
    if length == 0 {
        format!("{}", start)
    } else if length == 1 {
        format!("{}", beginning)
    } else {
        format!("{},{}", beginning, beginning + length - 1)
    }
}

pub fn create_difflib_module() -> PyObjectRef {
    fn extract_lines(obj: &PyObjectRef) -> PyResult<Vec<String>> {
        match &obj.payload {
            PyObjectPayload::List(items) => {
                Ok(items.read().iter().map(|i| i.py_to_string()).collect())
            }
            _ => Err(PyException::type_error("expected list")),
        }
    }

    fn parse_diff_kwargs(args: &[PyObjectRef]) -> (String, String, String, String, usize, String) {
        let mut fromfile = String::new();
        let mut tofile = String::new();
        let mut fromfiledate = String::new();
        let mut tofiledate = String::new();
        let mut n = 3usize;
        let mut lineterm = String::from("\n");
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw) = &last.payload {
                let kw = kw.read();
                if let Some(v) = kw.get(&HashableKey::str_key(CompactString::from("fromfile"))) {
                    fromfile = v.py_to_string();
                }
                if let Some(v) = kw.get(&HashableKey::str_key(CompactString::from("tofile"))) {
                    tofile = v.py_to_string();
                }
                if let Some(v) = kw.get(&HashableKey::str_key(CompactString::from("fromfiledate")))
                {
                    fromfiledate = v.py_to_string();
                }
                if let Some(v) = kw.get(&HashableKey::str_key(CompactString::from("tofiledate"))) {
                    tofiledate = v.py_to_string();
                }
                if let Some(v) = kw.get(&HashableKey::str_key(CompactString::from("n"))) {
                    n = v.to_int().unwrap_or(3) as usize;
                }
                if let Some(v) = kw.get(&HashableKey::str_key(CompactString::from("lineterm"))) {
                    lineterm = v.py_to_string();
                }
            }
        }
        for (i, arg) in args.iter().enumerate().skip(2) {
            if matches!(&arg.payload, PyObjectPayload::Dict(_)) {
                break;
            }
            match i {
                2 => fromfile = arg.py_to_string(),
                3 => tofile = arg.py_to_string(),
                4 => fromfiledate = arg.py_to_string(),
                5 => tofiledate = arg.py_to_string(),
                6 => n = arg.to_int().unwrap_or(3) as usize,
                7 => lineterm = arg.py_to_string(),
                _ => break,
            }
        }
        (fromfile, tofile, fromfiledate, tofiledate, n, lineterm)
    }

    fn unified_diff(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "unified_diff requires at least 2 arguments",
            ));
        }
        let a_lines = extract_lines(&args[0])?;
        let b_lines = extract_lines(&args[1])?;
        let (fromfile, tofile, fromfiledate, tofiledate, n, _lineterm) = parse_diff_kwargs(args);

        let blocks = find_matching_blocks(&a_lines, &b_lines);
        let opcodes = opcodes_from_matching_blocks(&blocks);
        let groups = group_opcodes(&opcodes, n);

        let mut result: Vec<PyObjectRef> = Vec::new();
        let mut started = false;
        for group in &groups {
            if !started {
                let from_h = if fromfiledate.is_empty() {
                    format!("--- {}\n", fromfile)
                } else {
                    format!("--- {}\t{}\n", fromfile, fromfiledate)
                };
                let to_h = if tofiledate.is_empty() {
                    format!("+++ {}\n", tofile)
                } else {
                    format!("+++ {}\t{}\n", tofile, tofiledate)
                };
                result.push(PyObject::str_val(CompactString::from(from_h)));
                result.push(PyObject::str_val(CompactString::from(to_h)));
                started = true;
            }
            let first = &group[0];
            let last_op = &group[group.len() - 1];
            let hunk = format!(
                "@@ -{} +{} @@\n",
                format_range_unified(first.1, last_op.2),
                format_range_unified(first.3, last_op.4),
            );
            result.push(PyObject::str_val(CompactString::from(hunk)));
            for (tag, i1, i2, j1, j2) in group {
                match tag.as_str() {
                    "equal" => {
                        for k in *i1..*i2 {
                            result.push(PyObject::str_val(CompactString::from(format!(
                                " {}",
                                a_lines[k]
                            ))));
                        }
                    }
                    "replace" => {
                        for k in *i1..*i2 {
                            result.push(PyObject::str_val(CompactString::from(format!(
                                "-{}",
                                a_lines[k]
                            ))));
                        }
                        for k in *j1..*j2 {
                            result.push(PyObject::str_val(CompactString::from(format!(
                                "+{}",
                                b_lines[k]
                            ))));
                        }
                    }
                    "delete" => {
                        for k in *i1..*i2 {
                            result.push(PyObject::str_val(CompactString::from(format!(
                                "-{}",
                                a_lines[k]
                            ))));
                        }
                    }
                    "insert" => {
                        for k in *j1..*j2 {
                            result.push(PyObject::str_val(CompactString::from(format!(
                                "+{}",
                                b_lines[k]
                            ))));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(PyObject::list(result))
    }

    fn ndiff(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "ndiff requires at least 2 arguments",
            ));
        }
        let a_lines = extract_lines(&args[0])?;
        let b_lines = extract_lines(&args[1])?;

        let blocks = find_matching_blocks(&a_lines, &b_lines);
        let opcodes = opcodes_from_matching_blocks(&blocks);

        let mut result: Vec<PyObjectRef> = Vec::new();
        for (tag, i1, i2, j1, j2) in &opcodes {
            match tag.as_str() {
                "equal" => {
                    for k in *i1..*i2 {
                        result.push(PyObject::str_val(CompactString::from(format!(
                            "  {}",
                            a_lines[k]
                        ))));
                    }
                }
                "replace" => {
                    for k in *i1..*i2 {
                        result.push(PyObject::str_val(CompactString::from(format!(
                            "- {}",
                            a_lines[k]
                        ))));
                    }
                    for k in *j1..*j2 {
                        result.push(PyObject::str_val(CompactString::from(format!(
                            "+ {}",
                            b_lines[k]
                        ))));
                    }
                }
                "delete" => {
                    for k in *i1..*i2 {
                        result.push(PyObject::str_val(CompactString::from(format!(
                            "- {}",
                            a_lines[k]
                        ))));
                    }
                }
                "insert" => {
                    for k in *j1..*j2 {
                        result.push(PyObject::str_val(CompactString::from(format!(
                            "+ {}",
                            b_lines[k]
                        ))));
                    }
                }
                _ => {}
            }
        }
        Ok(PyObject::list(result))
    }

    fn context_diff(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "context_diff requires at least 2 arguments",
            ));
        }
        let a_lines = extract_lines(&args[0])?;
        let b_lines = extract_lines(&args[1])?;
        let (fromfile, tofile, fromfiledate, tofiledate, n, _lineterm) = parse_diff_kwargs(args);

        let blocks = find_matching_blocks(&a_lines, &b_lines);
        let opcodes = opcodes_from_matching_blocks(&blocks);
        let groups = group_opcodes(&opcodes, n);

        let mut result: Vec<PyObjectRef> = Vec::new();
        let mut started = false;
        for group in &groups {
            if !started {
                let from_h = if fromfiledate.is_empty() {
                    format!("*** {}\n", fromfile)
                } else {
                    format!("*** {}\t{}\n", fromfile, fromfiledate)
                };
                let to_h = if tofiledate.is_empty() {
                    format!("--- {}\n", tofile)
                } else {
                    format!("--- {}\t{}\n", tofile, tofiledate)
                };
                result.push(PyObject::str_val(CompactString::from(from_h)));
                result.push(PyObject::str_val(CompactString::from(to_h)));
                started = true;
            }
            result.push(PyObject::str_val(CompactString::from("***************\n")));

            let first = &group[0];
            let last_op = &group[group.len() - 1];

            // "From" section
            result.push(PyObject::str_val(CompactString::from(format!(
                "*** {} ****\n",
                format_range_context(first.1, last_op.2)
            ))));
            let has_from_changes = group.iter().any(|(t, ..)| t == "replace" || t == "delete");
            if has_from_changes {
                for (tag, i1, i2, _, _) in group {
                    match tag.as_str() {
                        "equal" => {
                            for k in *i1..*i2 {
                                result.push(PyObject::str_val(CompactString::from(format!(
                                    "  {}",
                                    a_lines[k]
                                ))));
                            }
                        }
                        "replace" => {
                            for k in *i1..*i2 {
                                result.push(PyObject::str_val(CompactString::from(format!(
                                    "! {}",
                                    a_lines[k]
                                ))));
                            }
                        }
                        "delete" => {
                            for k in *i1..*i2 {
                                result.push(PyObject::str_val(CompactString::from(format!(
                                    "- {}",
                                    a_lines[k]
                                ))));
                            }
                        }
                        _ => {}
                    }
                }
            }

            // "To" section
            result.push(PyObject::str_val(CompactString::from(format!(
                "--- {} ----\n",
                format_range_context(first.3, last_op.4)
            ))));
            let has_to_changes = group.iter().any(|(t, ..)| t == "replace" || t == "insert");
            if has_to_changes {
                for (tag, _, _, j1, j2) in group {
                    match tag.as_str() {
                        "equal" => {
                            for k in *j1..*j2 {
                                result.push(PyObject::str_val(CompactString::from(format!(
                                    "  {}",
                                    b_lines[k]
                                ))));
                            }
                        }
                        "replace" => {
                            for k in *j1..*j2 {
                                result.push(PyObject::str_val(CompactString::from(format!(
                                    "! {}",
                                    b_lines[k]
                                ))));
                            }
                        }
                        "insert" => {
                            for k in *j1..*j2 {
                                result.push(PyObject::str_val(CompactString::from(format!(
                                    "+ {}",
                                    b_lines[k]
                                ))));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(PyObject::list(result))
    }

    fn get_close_matches(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "get_close_matches requires at least 2 arguments",
            ));
        }
        let word = args[0].py_to_string();
        let possibilities: Vec<String> = match &args[1].payload {
            PyObjectPayload::List(items) => items.read().iter().map(|i| i.py_to_string()).collect(),
            _ => return Err(PyException::type_error("expected list")),
        };
        let n = if args.len() > 2 {
            args[2].to_int().unwrap_or(3) as usize
        } else {
            3
        };
        let cutoff = if args.len() > 3 {
            match &args[3].payload {
                PyObjectPayload::Float(f) => *f,
                _ => 0.6,
            }
        } else {
            0.6
        };

        let word_chars: Vec<char> = word.chars().collect();
        let mut scored: Vec<(f64, &String)> = possibilities
            .iter()
            .filter_map(|p| {
                let p_chars: Vec<char> = p.chars().collect();
                let matches = lcs_length(&word_chars, &p_chars);
                let total = word_chars.len() + p_chars.len();
                let ratio = if total > 0 {
                    2.0 * matches as f64 / total as f64
                } else {
                    1.0
                };
                if ratio >= cutoff {
                    Some((ratio, p))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(n);
        Ok(PyObject::list(
            scored
                .iter()
                .map(|(_, s)| PyObject::str_val(CompactString::from(s.as_str())))
                .collect(),
        ))
    }

    fn sequence_matcher_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        fn seq_from_obj(obj: &PyObjectRef) -> Vec<String> {
            match &obj.payload {
                PyObjectPayload::List(items) => {
                    items.read().iter().map(|i| i.py_to_string()).collect()
                }
                PyObjectPayload::Str(s) => s.chars().map(|c| c.to_string()).collect(),
                _ => vec![obj.py_to_string()],
            }
        }

        let mut a_seq: Vec<String> = Vec::new();
        let mut b_seq: Vec<String> = Vec::new();
        if args.len() > 1 {
            a_seq = seq_from_obj(&args[1]);
        }
        if args.len() > 2 {
            b_seq = seq_from_obj(&args[2]);
        }
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw) = &last.payload {
                let kw = kw.read();
                if let Some(v) = kw.get(&HashableKey::str_key(CompactString::from("a"))) {
                    a_seq = seq_from_obj(v);
                }
                if let Some(v) = kw.get(&HashableKey::str_key(CompactString::from("b"))) {
                    b_seq = seq_from_obj(v);
                }
            }
        }

        let blocks = find_matching_blocks(&a_seq, &b_seq);
        let opcodes = opcodes_from_matching_blocks(&blocks);

        let matching: usize = blocks.iter().map(|&(_, _, s)| s).sum();
        let total = a_seq.len() + b_seq.len();
        let ratio_val = if total > 0 {
            2.0 * matching as f64 / total as f64
        } else {
            1.0
        };

        let cls = PyObject::class(
            CompactString::from("SequenceMatcher"),
            vec![],
            IndexMap::new(),
        );
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            let a_obj = if args.len() > 1 {
                args[1].clone()
            } else {
                PyObject::str_val(CompactString::from(""))
            };
            let b_obj = if args.len() > 2 {
                args[2].clone()
            } else {
                PyObject::str_val(CompactString::from(""))
            };
            attrs.insert(CompactString::from("a"), a_obj);
            attrs.insert(CompactString::from("b"), b_obj);

            let rf = ratio_val;
            attrs.insert(
                CompactString::from("ratio"),
                PyObject::native_closure("SequenceMatcher.ratio", move |_: &[PyObjectRef]| {
                    Ok(PyObject::float(rf))
                }),
            );
            attrs.insert(
                CompactString::from("quick_ratio"),
                PyObject::native_closure(
                    "SequenceMatcher.quick_ratio",
                    move |_: &[PyObjectRef]| Ok(PyObject::float(rf)),
                ),
            );

            let bc = blocks.clone();
            attrs.insert(
                CompactString::from("get_matching_blocks"),
                PyObject::native_closure(
                    "SequenceMatcher.get_matching_blocks",
                    move |_: &[PyObjectRef]| {
                        let r: Vec<PyObjectRef> = bc
                            .iter()
                            .map(|&(a, b, s)| {
                                PyObject::tuple(vec![
                                    PyObject::int(a as i64),
                                    PyObject::int(b as i64),
                                    PyObject::int(s as i64),
                                ])
                            })
                            .collect();
                        Ok(PyObject::list(r))
                    },
                ),
            );

            let oc = opcodes;
            attrs.insert(
                CompactString::from("get_opcodes"),
                PyObject::native_closure(
                    "SequenceMatcher.get_opcodes",
                    move |_: &[PyObjectRef]| {
                        let r: Vec<PyObjectRef> = oc
                            .iter()
                            .map(|(tag, i1, i2, j1, j2)| {
                                PyObject::tuple(vec![
                                    PyObject::str_val(CompactString::from(tag.as_str())),
                                    PyObject::int(*i1 as i64),
                                    PyObject::int(*i2 as i64),
                                    PyObject::int(*j1 as i64),
                                    PyObject::int(*j2 as i64),
                                ])
                            })
                            .collect();
                        Ok(PyObject::list(r))
                    },
                ),
            );
        }
        Ok(inst)
    }

    fn html_diff_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        // Parse optional kwargs: tabsize=8, wrapcolumn=None, linejunk=None, charjunk=None
        let mut tabsize = 8usize;
        let mut wrapcolumn: Option<usize> = None;
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw) = &last.payload {
                let r = kw.read();
                if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("tabsize"))) {
                    tabsize = v.as_int().unwrap_or(8) as usize;
                }
                if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("wrapcolumn"))) {
                    if let Some(w) = v.as_int() {
                        wrapcolumn = Some(w as usize);
                    }
                }
            }
        }

        let cls = PyObject::class(CompactString::from("HtmlDiff"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(
                CompactString::from("_tabsize"),
                PyObject::int(tabsize as i64),
            );
            if let Some(w) = wrapcolumn {
                attrs.insert(CompactString::from("_wrapcolumn"), PyObject::int(w as i64));
            } else {
                attrs.insert(CompactString::from("_wrapcolumn"), PyObject::none());
            }

            // make_file(fromlines, tolines, ...)
            attrs.insert(
                CompactString::from("make_file"),
                make_builtin(|args: &[PyObjectRef]| {
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "make_file requires fromlines and tolines",
                        ));
                    }
                    let from_lines = extract_lines(&args[0])?;
                    let to_lines = extract_lines(&args[1])?;
                    let fromdesc = if args.len() > 2 {
                        args[2].py_to_string()
                    } else {
                        String::new()
                    };
                    let todesc = if args.len() > 3 {
                        args[3].py_to_string()
                    } else {
                        String::new()
                    };
                    let table =
                        html_diff_make_table_impl(&from_lines, &to_lines, &fromdesc, &todesc);
                    let html = format!(
                        "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\"\n\
                     \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">\n\
                     <html>\n<head>\n\
                     <meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\" />\n\
                     <title></title>\n\
                     <style type=\"text/css\">\n\
                     table.diff {{font-family:Courier; border:medium;}}\n\
                     .diff_header {{background-color:#e0e0e0}}\n\
                     td.diff_header {{text-align:right}}\n\
                     .diff_next {{background-color:#c0c0c0}}\n\
                     .diff_add {{background-color:#aaffaa}}\n\
                     .diff_chg {{background-color:#ffff77}}\n\
                     .diff_sub {{background-color:#ffaaaa}}\n\
                     </style>\n</head>\n<body>\n{}\n</body>\n</html>",
                        table
                    );
                    Ok(PyObject::str_val(CompactString::from(html)))
                }),
            );

            // make_table(fromlines, tolines, ...)
            attrs.insert(
                CompactString::from("make_table"),
                make_builtin(|args: &[PyObjectRef]| {
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "make_table requires fromlines and tolines",
                        ));
                    }
                    let from_lines = extract_lines(&args[0])?;
                    let to_lines = extract_lines(&args[1])?;
                    let fromdesc = if args.len() > 2 {
                        args[2].py_to_string()
                    } else {
                        String::new()
                    };
                    let todesc = if args.len() > 3 {
                        args[3].py_to_string()
                    } else {
                        String::new()
                    };
                    let table =
                        html_diff_make_table_impl(&from_lines, &to_lines, &fromdesc, &todesc);
                    Ok(PyObject::str_val(CompactString::from(table)))
                }),
            );
        }
        Ok(inst)
    }

    fn html_diff_make_table_impl(
        from_lines: &[String],
        to_lines: &[String],
        fromdesc: &str,
        todesc: &str,
    ) -> String {
        fn html_escape_str(s: &str) -> String {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        }

        let blocks = find_matching_blocks(from_lines, to_lines);
        let opcodes = opcodes_from_matching_blocks(&blocks);

        let mut rows = String::new();
        // Header row
        rows.push_str("<table class=\"diff\" summary=\"Legends\">\n");
        rows.push_str("<colgroup></colgroup> <colgroup></colgroup> <colgroup></colgroup>\n");
        rows.push_str("<colgroup></colgroup> <colgroup></colgroup> <colgroup></colgroup>\n");
        if !fromdesc.is_empty() || !todesc.is_empty() {
            rows.push_str(&format!(
                "<thead><tr><th class=\"diff_next\"><br /></th><th colspan=\"2\" class=\"diff_header\">{}</th>\
                 <th class=\"diff_next\"><br /></th><th colspan=\"2\" class=\"diff_header\">{}</th></tr></thead>\n",
                html_escape_str(fromdesc), html_escape_str(todesc)
            ));
        }
        rows.push_str("<tbody>\n");

        for (tag, i1, i2, j1, j2) in &opcodes {
            match tag.as_str() {
                "equal" => {
                    for k in 0..(*i2 - *i1) {
                        let line_a = from_lines.get(i1 + k).map(|s| s.as_str()).unwrap_or("");
                        rows.push_str(&format!(
                            "<tr><td class=\"diff_next\"></td><td class=\"diff_header\">{}</td><td nowrap=\"nowrap\">{}</td>\
                             <td class=\"diff_next\"></td><td class=\"diff_header\">{}</td><td nowrap=\"nowrap\">{}</td></tr>\n",
                            i1 + k + 1, html_escape_str(line_a), j1 + k + 1, html_escape_str(line_a)
                        ));
                    }
                }
                "replace" => {
                    let max_k = (*i2 - *i1).max(*j2 - *j1);
                    for k in 0..max_k {
                        let from_num = if k < (*i2 - *i1) {
                            format!("{}", i1 + k + 1)
                        } else {
                            String::new()
                        };
                        let from_text = if k < (*i2 - *i1) {
                            format!(
                                "<td class=\"diff_chg\" nowrap=\"nowrap\">{}</td>",
                                html_escape_str(
                                    from_lines.get(i1 + k).map(|s| s.as_str()).unwrap_or("")
                                )
                            )
                        } else {
                            "<td></td>".to_string()
                        };
                        let to_num = if k < (*j2 - *j1) {
                            format!("{}", j1 + k + 1)
                        } else {
                            String::new()
                        };
                        let to_text = if k < (*j2 - *j1) {
                            format!(
                                "<td class=\"diff_chg\" nowrap=\"nowrap\">{}</td>",
                                html_escape_str(
                                    to_lines.get(j1 + k).map(|s| s.as_str()).unwrap_or("")
                                )
                            )
                        } else {
                            "<td></td>".to_string()
                        };
                        rows.push_str(&format!(
                            "<tr><td class=\"diff_next\"></td><td class=\"diff_header\">{}</td>{}\
                             <td class=\"diff_next\"></td><td class=\"diff_header\">{}</td>{}</tr>\n",
                            from_num, from_text, to_num, to_text
                        ));
                    }
                }
                "delete" => {
                    for k in *i1..*i2 {
                        let line = from_lines.get(k).map(|s| s.as_str()).unwrap_or("");
                        rows.push_str(&format!(
                            "<tr><td class=\"diff_next\"></td><td class=\"diff_header\">{}</td>\
                             <td class=\"diff_sub\" nowrap=\"nowrap\">{}</td>\
                             <td class=\"diff_next\"></td><td class=\"diff_header\"></td><td></td></tr>\n",
                            k + 1, html_escape_str(line)
                        ));
                    }
                }
                "insert" => {
                    for k in *j1..*j2 {
                        let line = to_lines.get(k).map(|s| s.as_str()).unwrap_or("");
                        rows.push_str(&format!(
                            "<tr><td class=\"diff_next\"></td><td class=\"diff_header\"></td><td></td>\
                             <td class=\"diff_next\"></td><td class=\"diff_header\">{}</td>\
                             <td class=\"diff_add\" nowrap=\"nowrap\">{}</td></tr>\n",
                            k + 1, html_escape_str(line)
                        ));
                    }
                }
                _ => {}
            }
        }
        rows.push_str("</tbody>\n</table>");
        rows
    }

    make_module(
        "difflib",
        vec![
            ("unified_diff", make_builtin(unified_diff)),
            ("ndiff", make_builtin(ndiff)),
            ("context_diff", make_builtin(context_diff)),
            ("get_close_matches", make_builtin(get_close_matches)),
            ("SequenceMatcher", make_builtin(sequence_matcher_ctor)),
            ("HtmlDiff", make_builtin(html_diff_ctor)),
        ],
    )
}

/// Compute Longest Common Subsequence length (character-level, used by get_close_matches)
fn lcs_length(a: &[char], b: &[char]) -> usize {
    let m = a.len();
    let n = b.len();
    if m == 0 || n == 0 {
        return 0;
    }
    let mut prev = vec![0usize; n + 1];
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        for j in 1..=n {
            if a[i - 1] == b[j - 1] {
                curr[j] = prev[j - 1] + 1;
            } else {
                curr[j] = prev[j].max(curr[j - 1]);
            }
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.iter_mut().for_each(|x| *x = 0);
    }
    prev[n]
}
