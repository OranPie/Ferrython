use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef,
};

// ── glob module ──

pub fn create_glob_module() -> PyObjectRef {
    make_module(
        "glob",
        vec![
            ("glob", make_builtin(glob_glob)),
            ("iglob", make_builtin(glob_glob)),
            ("escape", make_builtin(glob_escape)),
            (
                "has_magic",
                make_builtin(|args: &[PyObjectRef]| {
                    check_args("glob.has_magic", args, 1)?;
                    let s = args[0].py_to_string();
                    Ok(PyObject::bool_val(
                        s.contains('*') || s.contains('?') || s.contains('[') || s.contains(']'),
                    ))
                }),
            ),
        ],
    )
}

fn glob_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("escape requires a pathname"));
    }
    let s = args[0].py_to_string();
    let escaped: String = s
        .chars()
        .map(|c| match c {
            '*' | '?' | '[' => {
                let mut r = String::from('[');
                r.push(c);
                r.push(']');
                r
            }
            _ => c.to_string(),
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(escaped)))
}

fn glob_glob(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("glob requires a pattern"));
    }
    let pattern = args[0].py_to_string();
    // Check for recursive kwarg
    let recursive = if args.len() > 1 {
        args[1].is_truthy()
    } else {
        pattern.contains("**")
    };

    let mut results = Vec::new();
    if recursive && pattern.contains("**") {
        glob_recursive(&pattern, &mut results)?;
    } else {
        glob_simple(&pattern, &mut results)?;
    }
    results.sort_by(|a, b| a.py_to_string().cmp(&b.py_to_string()));
    Ok(PyObject::list(results))
}

fn glob_simple(pattern: &str, results: &mut Vec<PyObjectRef>) -> PyResult<()> {
    glob_expand(pattern, results);
    Ok(())
}

/// Recursively expand glob pattern by handling wildcards in any path component.
fn glob_expand(pattern: &str, results: &mut Vec<PyObjectRef>) {
    // Split pattern into components
    let parts: Vec<&str> = pattern.split('/').collect();

    // Find first component with a wildcard
    let wild_idx = parts
        .iter()
        .position(|p| p.contains('*') || p.contains('?') || p.contains('['));

    match wild_idx {
        None => {
            // No wildcards: check if the literal path exists
            let p = std::path::Path::new(pattern);
            if p.exists() {
                results.push(PyObject::str_val(CompactString::from(pattern)));
            }
        }
        Some(idx) => {
            let dir_prefix: String = if idx == 0 {
                ".".to_string()
            } else {
                parts[..idx].join("/")
            };
            let wild_part = parts[idx];
            let rest: Option<String> = if idx + 1 < parts.len() {
                Some(parts[idx + 1..].join("/"))
            } else {
                None
            };

            if let Ok(entries) = std::fs::read_dir(&dir_prefix) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !glob_match(wild_part, &name) {
                        continue;
                    }

                    let matched_path = if idx == 0 {
                        name
                    } else {
                        format!("{}/{}", parts[..idx].join("/"), name)
                    };

                    match &rest {
                        None => {
                            results.push(PyObject::str_val(CompactString::from(matched_path)));
                        }
                        Some(remainder) => {
                            let sub = format!("{}/{}", matched_path, remainder);
                            glob_expand(&sub, results);
                        }
                    }
                }
            }
        }
    }
}

fn glob_recursive(pattern: &str, results: &mut Vec<PyObjectRef>) -> PyResult<()> {
    // Split on ** to get prefix and suffix
    // e.g. "src/**/*.rs" → prefix="src/", suffix="*.rs"
    if let Some(star_pos) = pattern.find("**") {
        let prefix = &pattern[..star_pos];
        let suffix = &pattern[star_pos + 2..];
        let suffix = suffix
            .strip_prefix('/')
            .or_else(|| suffix.strip_prefix('\\'))
            .unwrap_or(suffix);
        let base_dir = if prefix.is_empty() {
            ".".to_string()
        } else {
            prefix
                .trim_end_matches('/')
                .trim_end_matches('\\')
                .to_string()
        };
        let base_path = std::path::Path::new(&base_dir);
        if base_path.is_dir() {
            walk_dir_recursive(base_path, suffix, results);
        }
    } else {
        glob_simple(pattern, results)?;
    }
    Ok(())
}

fn walk_dir_recursive(dir: &std::path::Path, file_pattern: &str, results: &mut Vec<PyObjectRef>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Match directory itself if pattern is empty
                if file_pattern.is_empty() {
                    results.push(PyObject::str_val(CompactString::from(
                        path.to_string_lossy().to_string(),
                    )));
                }
                walk_dir_recursive(&path, file_pattern, results);
            } else if !file_pattern.is_empty() {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if glob_match(file_pattern, &name) {
                    results.push(PyObject::str_val(CompactString::from(
                        path.to_string_lossy().to_string(),
                    )));
                }
            }
        }
    }
}

pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
        return pattern == text;
    }
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_at(&pat, 0, &txt, 0)
}

fn glob_match_at(pat: &[char], mut pi: usize, txt: &[char], mut ti: usize) -> bool {
    while pi < pat.len() {
        match pat[pi] {
            '*' => {
                pi += 1;
                // Match zero or more characters
                for k in ti..=txt.len() {
                    if glob_match_at(pat, pi, txt, k) {
                        return true;
                    }
                }
                return false;
            }
            '?' => {
                if ti >= txt.len() {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
            '[' => {
                if ti >= txt.len() {
                    return false;
                }
                let c = txt[ti];
                pi += 1;
                let negate = pi < pat.len() && (pat[pi] == '!' || pat[pi] == '^');
                if negate {
                    pi += 1;
                }
                let mut matched = false;
                while pi < pat.len() && pat[pi] != ']' {
                    if pi + 2 < pat.len() && pat[pi + 1] == '-' {
                        if c >= pat[pi] && c <= pat[pi + 2] {
                            matched = true;
                        }
                        pi += 3;
                    } else {
                        if c == pat[pi] {
                            matched = true;
                        }
                        pi += 1;
                    }
                }
                if pi < pat.len() {
                    pi += 1;
                } // skip ']'
                if matched == negate {
                    return false;
                }
                ti += 1;
            }
            c => {
                if ti >= txt.len() || txt[ti] != c {
                    return false;
                }
                pi += 1;
                ti += 1;
            }
        }
    }
    ti == txt.len()
}
