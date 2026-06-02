use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef,
};

const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

const SOFT_KEYWORDS: &[&str] = &["_", "case", "match", "type"];

pub fn create_keyword_module() -> PyObjectRef {
    make_module(
        "keyword",
        vec![
            ("kwlist", string_list(KEYWORDS)),
            ("softkwlist", string_list(SOFT_KEYWORDS)),
            ("iskeyword", make_builtin(is_keyword)),
            ("issoftkeyword", make_builtin(is_soft_keyword)),
        ],
    )
}

fn string_list(items: &[&str]) -> PyObjectRef {
    PyObject::list(
        items
            .iter()
            .map(|item| PyObject::str_val(CompactString::from(*item)))
            .collect(),
    )
}

fn is_keyword(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("keyword.iskeyword", args, 1)?;
    Ok(PyObject::bool_val(
        args[0]
            .as_str()
            .is_some_and(|name| KEYWORDS.binary_search(&name).is_ok()),
    ))
}

fn is_soft_keyword(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("keyword.issoftkeyword", args, 1)?;
    Ok(PyObject::bool_val(args[0].as_str().is_some_and(|name| {
        SOFT_KEYWORDS.binary_search(&name).is_ok()
    })))
}
