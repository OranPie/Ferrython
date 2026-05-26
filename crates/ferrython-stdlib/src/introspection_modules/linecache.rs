use super::*;

// ── linecache module ──

pub fn create_linecache_module() -> PyObjectRef {
    let getline_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "linecache.getline requires filename and lineno",
            ));
        }
        let filename = args[0].py_to_string();
        let lineno = match &args[1].payload {
            PyObjectPayload::Int(n) => n.to_i64().unwrap_or(0) as usize,
            _ => 0,
        };
        // Try to read the file and get the line
        match std::fs::read_to_string(&filename) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                if lineno > 0 && lineno <= lines.len() {
                    Ok(PyObject::str_val(CompactString::from(format!(
                        "{}\n",
                        lines[lineno - 1]
                    ))))
                } else {
                    Ok(PyObject::str_val(CompactString::from("")))
                }
            }
            Err(_) => Ok(PyObject::str_val(CompactString::from(""))),
        }
    });

    let getlines_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "linecache.getlines requires filename",
            ));
        }
        let filename = args[0].py_to_string();
        match std::fs::read_to_string(&filename) {
            Ok(content) => {
                let lines: Vec<PyObjectRef> = content
                    .lines()
                    .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
                    .collect();
                Ok(PyObject::list(lines))
            }
            Err(_) => Ok(PyObject::list(vec![])),
        }
    });

    let clearcache_fn = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));

    let checkcache_fn = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));

    make_module(
        "linecache",
        vec![
            ("getline", getline_fn),
            ("getlines", getlines_fn),
            ("clearcache", clearcache_fn),
            ("checkcache", checkcache_fn),
            ("cache", PyObject::dict(IndexMap::new())),
        ],
    )
}
