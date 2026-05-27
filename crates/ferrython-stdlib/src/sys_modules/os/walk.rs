use super::*;

pub(super) fn os_walk(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.walk() requires at least 1 argument",
        ));
    }
    let path = args[0].py_to_string();
    let topdown = if args.len() > 1 {
        args[1].is_truthy()
    } else {
        true
    };
    let mut results = Vec::new();
    walk_dir_recursive(&path, topdown, &mut results);
    Ok(PyObject::list(results))
}

fn walk_dir_recursive(dir: &str, topdown: bool, results: &mut Vec<PyObjectRef>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut dirnames = Vec::new();
    let mut filenames = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            dirnames.push(name);
        } else {
            filenames.push(name);
        }
    }
    let tuple = PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(dir)),
        PyObject::list(
            dirnames
                .iter()
                .map(|n| PyObject::str_val(CompactString::from(n.as_str())))
                .collect(),
        ),
        PyObject::list(
            filenames
                .iter()
                .map(|n| PyObject::str_val(CompactString::from(n.as_str())))
                .collect(),
        ),
    ]);
    if topdown {
        results.push(tuple);
        for name in &dirnames {
            let child = format!("{}/{}", dir.trim_end_matches('/'), name);
            walk_dir_recursive(&child, topdown, results);
        }
    } else {
        for name in &dirnames {
            let child = format!("{}/{}", dir.trim_end_matches('/'), name);
            walk_dir_recursive(&child, topdown, results);
        }
        results.push(tuple);
    }
}
