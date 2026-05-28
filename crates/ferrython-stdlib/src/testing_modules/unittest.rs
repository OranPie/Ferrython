use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

mod assertions;

// ── unittest module ──

#[allow(dead_code)]
pub fn create_unittest_module() -> PyObjectRef {
    // Build TestCase class with assert methods in the namespace so that
    // subclass instances inherit them via MRO lookup.
    let mut tc_ns = IndexMap::new();
    tc_ns.insert(
        CompactString::from("__unittest_testcase__"),
        PyObject::bool_val(true),
    );

    // setUp / tearDown / setUpClass / tearDownClass — default no-ops, subclasses override
    tc_ns.insert(
        CompactString::from("setUp"),
        make_builtin(|_| Ok(PyObject::none())),
    );
    tc_ns.insert(
        CompactString::from("tearDown"),
        make_builtin(|_| Ok(PyObject::none())),
    );
    tc_ns.insert(
        CompactString::from("setUpClass"),
        make_builtin(|_| Ok(PyObject::none())),
    );
    tc_ns.insert(
        CompactString::from("tearDownClass"),
        make_builtin(|_| Ok(PyObject::none())),
    );

    assertions::add_assertion_methods(&mut tc_ns);

    let test_case = PyObject::class(CompactString::from("TestCase"), vec![], tc_ns);

    make_module(
        "unittest",
        vec![
            ("TestCase", test_case),
            ("main", make_builtin(|_| Ok(PyObject::none()))),
            (
                "TestSuite",
                make_builtin(|args| {
                    let tests: Vec<PyObjectRef> = if !args.is_empty() {
                        args[0].to_list().unwrap_or_default()
                    } else {
                        vec![]
                    };
                    let test_list = Rc::new(PyCell::new(tests));
                    let mut attrs = IndexMap::new();
                    let tl = test_list.clone();
                    attrs.insert(
                        CompactString::from("_tests"),
                        PyObject::list(tl.read().clone()),
                    );
                    let tl = test_list.clone();
                    attrs.insert(
                        CompactString::from("addTest"),
                        PyObject::native_closure("addTest", move |args| {
                            if !args.is_empty() {
                                tl.write().push(args[0].clone());
                            }
                            Ok(PyObject::none())
                        }),
                    );
                    let tl = test_list.clone();
                    attrs.insert(
                        CompactString::from("__iter__"),
                        PyObject::native_closure("__iter__", move |_| {
                            Ok(PyObject::list(tl.read().clone()).get_iter()?)
                        }),
                    );
                    let tl = test_list.clone();
                    attrs.insert(
                        CompactString::from("__len__"),
                        PyObject::native_closure("__len__", move |_| {
                            Ok(PyObject::int(tl.read().len() as i64))
                        }),
                    );
                    let tl = test_list.clone();
                    attrs.insert(
                        CompactString::from("countTestCases"),
                        PyObject::native_closure("countTestCases", move |_| {
                            Ok(PyObject::int(tl.read().len() as i64))
                        }),
                    );
                    Ok(PyObject::module_with_attrs(
                        CompactString::from("TestSuite"),
                        attrs,
                    ))
                }),
            ),
            (
                "TestLoader",
                make_builtin(|_| {
                    let mut attrs = IndexMap::new();
                    attrs.insert(
                        CompactString::from("loadTestsFromTestCase"),
                        make_builtin(|args| {
                            if args.is_empty() {
                                return Err(PyException::type_error(
                                    "loadTestsFromTestCase() requires a TestCase class",
                                ));
                            }
                            let cls = &args[0];
                            let mut tests = vec![];
                            // Get test methods from the class namespace
                            if let PyObjectPayload::Class(cls_data) = &cls.payload {
                                let ns = cls_data.namespace.read();
                                for (name, _) in ns.iter() {
                                    if name.starts_with("test") {
                                        tests.push(PyObject::str_val(CompactString::from(
                                            name.as_str(),
                                        )));
                                    }
                                }
                            }
                            Ok(PyObject::list(tests))
                        }),
                    );
                    Ok(PyObject::module_with_attrs(
                        CompactString::from("TestLoader"),
                        attrs,
                    ))
                }),
            ),
            (
                "TextTestRunner",
                make_builtin(|_| {
                    // TextTestRunner() returns an object with a run(suite) method.
                    // run() returns a TestResult with wasSuccessful(), failures, errors.
                    let mut runner_attrs = IndexMap::new();
                    runner_attrs.insert(
                        CompactString::from("run"),
                        PyObject::native_closure("run", |_args| {
                            // Build a TestResult object
                            let mut res_attrs = IndexMap::new();
                            let failures = Rc::new(PyCell::new(Vec::<PyObjectRef>::new()));
                            let errors = Rc::new(PyCell::new(Vec::<PyObjectRef>::new()));
                            let _tests_run = Arc::new(std::sync::atomic::AtomicI64::new(0));

                            let f = failures.clone();
                            res_attrs
                                .insert(CompactString::from("failures"), PyObject::list(vec![]));
                            let e = errors.clone();
                            res_attrs.insert(CompactString::from("errors"), PyObject::list(vec![]));
                            res_attrs
                                .insert(CompactString::from("skipped"), PyObject::list(vec![]));
                            res_attrs.insert(
                                CompactString::from("expectedFailures"),
                                PyObject::list(vec![]),
                            );
                            res_attrs.insert(
                                CompactString::from("unexpectedSuccesses"),
                                PyObject::list(vec![]),
                            );
                            res_attrs.insert(CompactString::from("testsRun"), PyObject::int(0));

                            let f2 = failures.clone();
                            let e2 = errors.clone();
                            res_attrs.insert(
                                CompactString::from("wasSuccessful"),
                                PyObject::native_closure("wasSuccessful", move |_| {
                                    Ok(PyObject::bool_val(
                                        f2.read().is_empty() && e2.read().is_empty(),
                                    ))
                                }),
                            );
                            res_attrs.insert(
                                CompactString::from("addFailure"),
                                PyObject::native_closure("addFailure", move |args| {
                                    if !args.is_empty() {
                                        f.write().push(args[0].clone());
                                    }
                                    Ok(PyObject::none())
                                }),
                            );
                            res_attrs.insert(
                                CompactString::from("addError"),
                                PyObject::native_closure("addError", move |args| {
                                    if !args.is_empty() {
                                        e.write().push(args[0].clone());
                                    }
                                    Ok(PyObject::none())
                                }),
                            );
                            Ok(PyObject::module_with_attrs(
                                CompactString::from("TestResult"),
                                res_attrs,
                            ))
                        }),
                    );
                    Ok(PyObject::module_with_attrs(
                        CompactString::from("TextTestRunner"),
                        runner_attrs,
                    ))
                }),
            ),
            (
                "skip",
                make_builtin(|_args| {
                    Ok(make_builtin(|args| {
                        if args.is_empty() {
                            Ok(PyObject::none())
                        } else {
                            Ok(args[0].clone())
                        }
                    }))
                }),
            ),
            (
                "skipIf",
                make_builtin(|_| {
                    Ok(make_builtin(|args| {
                        if args.is_empty() {
                            Ok(PyObject::none())
                        } else {
                            Ok(args[0].clone())
                        }
                    }))
                }),
            ),
            (
                "skipUnless",
                make_builtin(|_| {
                    Ok(make_builtin(|args| {
                        if args.is_empty() {
                            Ok(PyObject::none())
                        } else {
                            Ok(args[0].clone())
                        }
                    }))
                }),
            ),
            (
                "expectedFailure",
                make_builtin(|args| {
                    if args.is_empty() {
                        Ok(PyObject::none())
                    } else {
                        Ok(args[0].clone())
                    }
                }),
            ),
            ("SkipTest", {
                let mut skip_ns = IndexMap::new();
                skip_ns.insert(
                    CompactString::from("__init__"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                PyObject::class(CompactString::from("SkipTest"), vec![], skip_ns)
            }),
        ],
    )
}
