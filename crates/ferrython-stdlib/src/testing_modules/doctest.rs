use compact_str::CompactString;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};
use indexmap::IndexMap;

// ── doctest module (replaced by pure Python stdlib/Lib/doctest.py) ──

#[allow(dead_code)]
pub fn create_doctest_module() -> PyObjectRef {
    let testmod_fn = make_builtin(|_args: &[PyObjectRef]| {
        // Return a TestResults(failed=0, attempted=0) named tuple-like
        let cls = PyObject::class(CompactString::from("TestResults"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("failed"), PyObject::int(0));
        attrs.insert(CompactString::from("attempted"), PyObject::int(0));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    let run_docstring_fn = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));

    make_module(
        "doctest",
        vec![
            ("testmod", testmod_fn),
            ("run_docstring_examples", run_docstring_fn),
            (
                "DocTestRunner",
                make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
            ),
            (
                "DocTestFinder",
                make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
            ),
            ("ELLIPSIS", PyObject::int(8)),
            ("NORMALIZE_WHITESPACE", PyObject::int(2)),
            ("IGNORE_EXCEPTION_DETAIL", PyObject::int(4)),
            ("OPTIONFLAGS", PyObject::int(0)),
        ],
    )
}
