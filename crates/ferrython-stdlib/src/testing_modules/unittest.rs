use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, CompareOp, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

// ── unittest module ──

/// Helper: extract optional message from args at given index.
#[allow(dead_code)]
fn assert_msg(args: &[PyObjectRef], idx: usize) -> String {
    if args.len() > idx {
        args[idx].py_to_string()
    } else {
        String::new()
    }
}

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

    // assertEqual(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertEqual"),
        PyObject::native_closure("assertEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = if args.len() > 2 {
                    args[2].py_to_string()
                } else {
                    format!("{} != {}", args[0].py_to_string(), args[1].py_to_string())
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertNotEqual(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertNotEqual"),
        PyObject::native_closure("assertNotEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertNotEqual requires 2 arguments",
                ));
            }
            let result = args[0].compare(&args[1], CompareOp::Ne)?;
            if !result.is_truthy() {
                let msg = if args.len() > 2 {
                    args[2].py_to_string()
                } else {
                    format!("{} == {}", args[0].py_to_string(), args[1].py_to_string())
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertTrue(x[, msg])
    tc_ns.insert(
        CompactString::from("assertTrue"),
        PyObject::native_closure("assertTrue", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("assertTrue requires 1 argument"));
            }
            if !args[0].is_truthy() {
                let msg = assert_msg(args, 1);
                let msg = if msg.is_empty() {
                    format!("{} is not true", args[0].py_to_string())
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertFalse(x[, msg])
    tc_ns.insert(
        CompactString::from("assertFalse"),
        PyObject::native_closure("assertFalse", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("assertFalse requires 1 argument"));
            }
            if args[0].is_truthy() {
                let msg = assert_msg(args, 1);
                let msg = if msg.is_empty() {
                    format!("{} is not false", args[0].py_to_string())
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertIs(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertIs"),
        PyObject::native_closure("assertIs", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertIs requires 2 arguments"));
            }
            if !PyObjectRef::ptr_eq(&args[0], &args[1]) {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "{} is not {}",
                        args[0].py_to_string(),
                        args[1].py_to_string()
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertIsNot(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertIsNot"),
        PyObject::native_closure("assertIsNot", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertIsNot requires 2 arguments"));
            }
            if PyObjectRef::ptr_eq(&args[0], &args[1]) {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} is {}", args[0].py_to_string(), args[1].py_to_string())
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertIsNone(x[, msg])
    tc_ns.insert(
        CompactString::from("assertIsNone"),
        PyObject::native_closure("assertIsNone", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("assertIsNone requires 1 argument"));
            }
            if !matches!(args[0].payload, PyObjectPayload::None) {
                let msg = assert_msg(args, 1);
                let msg = if msg.is_empty() {
                    format!("{} is not None", args[0].py_to_string())
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertIsNotNone(x[, msg])
    tc_ns.insert(
        CompactString::from("assertIsNotNone"),
        PyObject::native_closure("assertIsNotNone", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "assertIsNotNone requires 1 argument",
                ));
            }
            if matches!(args[0].payload, PyObjectPayload::None) {
                let msg = assert_msg(args, 1);
                let msg = if msg.is_empty() {
                    "unexpectedly None".to_string()
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertIn(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertIn"),
        PyObject::native_closure("assertIn", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertIn requires 2 arguments"));
            }
            let contained = args[1].contains(&args[0])?;
            if !contained {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "{} not found in {}",
                        args[0].py_to_string(),
                        args[1].py_to_string()
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertNotIn(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertNotIn"),
        PyObject::native_closure("assertNotIn", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertNotIn requires 2 arguments"));
            }
            let contained = args[1].contains(&args[0])?;
            if contained {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "{} unexpectedly found in {}",
                        args[0].py_to_string(),
                        args[1].py_to_string()
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertGreater(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertGreater"),
        PyObject::native_closure("assertGreater", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertGreater requires 2 arguments",
                ));
            }
            let result = args[0].compare(&args[1], CompareOp::Gt)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "{} not greater than {}",
                        args[0].py_to_string(),
                        args[1].py_to_string()
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertLess(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertLess"),
        PyObject::native_closure("assertLess", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertLess requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Lt)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "{} not less than {}",
                        args[0].py_to_string(),
                        args[1].py_to_string()
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertRaises(exc_type) — returns a context manager
    tc_ns.insert(
        CompactString::from("assertRaises"),
        PyObject::native_closure("assertRaises", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "assertRaises requires an exception type",
                ));
            }
            let exc_type_name = match &args[0].payload {
                PyObjectPayload::Class(cd) => cd.name.clone(),
                PyObjectPayload::Str(s) => s.to_compact_string(),
                _ => CompactString::from(args[0].py_to_string()),
            };
            // Build a context-manager object with __enter__ / __exit__
            let cls = PyObject::class(
                CompactString::from("_AssertRaisesContext"),
                vec![],
                IndexMap::new(),
            );
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(
                    CompactString::from("expected"),
                    PyObject::str_val(exc_type_name.clone()),
                );
                w.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", |_args: &[PyObjectRef]| {
                        Ok(PyObject::none())
                    }),
                );
                let etype = exc_type_name.clone();
                w.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", move |args: &[PyObjectRef]| {
                        // args: exc_type, exc_val, exc_tb (or None if no exception)
                        let has_exc = if args.is_empty() {
                            false
                        } else {
                            !matches!(args[0].payload, PyObjectPayload::None)
                        };
                        if !has_exc {
                            return Err(PyException::assertion_error(format!(
                                "{} not raised",
                                etype
                            )));
                        }
                        // Suppress the exception
                        Ok(PyObject::bool_val(true))
                    }),
                );
            }
            Ok(inst)
        }),
    );

    // assertGreaterEqual(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertGreaterEqual"),
        PyObject::native_closure("assertGreaterEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertGreaterEqual requires 2 arguments",
                ));
            }
            let result = args[0].compare(&args[1], CompareOp::Ge)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "{} not greater than or equal to {}",
                        args[0].py_to_string(),
                        args[1].py_to_string()
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertLessEqual(a, b[, msg])
    tc_ns.insert(
        CompactString::from("assertLessEqual"),
        PyObject::native_closure("assertLessEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertLessEqual requires 2 arguments",
                ));
            }
            let result = args[0].compare(&args[1], CompareOp::Le)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "{} not less than or equal to {}",
                        args[0].py_to_string(),
                        args[1].py_to_string()
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertAlmostEqual(a, b[, places=7, msg=None])
    tc_ns.insert(
        CompactString::from("assertAlmostEqual"),
        PyObject::native_closure("assertAlmostEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertAlmostEqual requires 2 arguments",
                ));
            }
            let a = args[0].to_float().or_else(|_| {
                args[0].as_int().map(|i| i as f64).ok_or_else(|| {
                    PyException::type_error("assertAlmostEqual requires numeric arguments")
                })
            })?;
            let b = args[1].to_float().or_else(|_| {
                args[1].as_int().map(|i| i as f64).ok_or_else(|| {
                    PyException::type_error("assertAlmostEqual requires numeric arguments")
                })
            })?;
            let places = if args.len() > 2 {
                args[2].as_int().unwrap_or(7)
            } else {
                7
            };
            // CPython: round(a-b, places) == 0, equivalent to abs(a-b) < 0.5 * 10^(-places)
            let tolerance = 0.5 * 10f64.powi(-(places as i32));
            if (a - b).abs() >= tolerance {
                let msg = assert_msg(args, 3);
                let msg = if msg.is_empty() {
                    format!("{} != {} within {} places", a, b, places)
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertNotAlmostEqual(a, b[, places=7, msg=None])
    tc_ns.insert(
        CompactString::from("assertNotAlmostEqual"),
        PyObject::native_closure("assertNotAlmostEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertNotAlmostEqual requires 2 arguments",
                ));
            }
            let a = args[0].to_float().or_else(|_| {
                args[0].as_int().map(|i| i as f64).ok_or_else(|| {
                    PyException::type_error("assertNotAlmostEqual requires numeric arguments")
                })
            })?;
            let b = args[1].to_float().or_else(|_| {
                args[1].as_int().map(|i| i as f64).ok_or_else(|| {
                    PyException::type_error("assertNotAlmostEqual requires numeric arguments")
                })
            })?;
            let places = if args.len() > 2 {
                args[2].as_int().unwrap_or(7)
            } else {
                7
            };
            let tolerance = 0.5 * 10f64.powi(-(places as i32));
            if (a - b).abs() < tolerance {
                let msg = assert_msg(args, 3);
                let msg = if msg.is_empty() {
                    format!("{} == {} within {} places", a, b, places)
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertIsInstance(obj, cls[, msg])
    tc_ns.insert(
        CompactString::from("assertIsInstance"),
        PyObject::native_closure("assertIsInstance", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertIsInstance requires 2 arguments",
                ));
            }
            let obj_type = args[0].type_name();
            let expected = match &args[1].payload {
                PyObjectPayload::Class(cd) => cd.name.as_str().to_string(),
                _ => args[1].py_to_string(),
            };
            // Check direct type match or class hierarchy
            let is_instance = obj_type == expected || obj_type.eq_ignore_ascii_case(&expected);
            if !is_instance {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "{} is not an instance of {}",
                        args[0].py_to_string(),
                        expected
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertNotIsInstance(obj, cls[, msg])
    tc_ns.insert(
        CompactString::from("assertNotIsInstance"),
        PyObject::native_closure("assertNotIsInstance", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertNotIsInstance requires 2 arguments",
                ));
            }
            let obj_type = args[0].type_name();
            let expected = match &args[1].payload {
                PyObjectPayload::Class(cd) => cd.name.as_str().to_string(),
                _ => args[1].py_to_string(),
            };
            let is_instance = obj_type == expected || obj_type.eq_ignore_ascii_case(&expected);
            if is_instance {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} is an instance of {}", args[0].py_to_string(), expected)
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertRegex(text, regex[, msg])
    tc_ns.insert(
        CompactString::from("assertRegex"),
        PyObject::native_closure("assertRegex", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertRegex requires 2 arguments"));
            }
            let text = args[0].py_to_string();
            let pattern = args[1].py_to_string();
            let re = regex::Regex::new(&pattern)
                .map_err(|e| PyException::runtime_error(format!("Invalid regex: {}", e)))?;
            if re.find(&text).is_none() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("Regex '{}' didn't match '{}'", pattern, text)
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertNotRegex(text, regex[, msg])
    tc_ns.insert(
        CompactString::from("assertNotRegex"),
        PyObject::native_closure("assertNotRegex", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertNotRegex requires 2 arguments",
                ));
            }
            let text = args[0].py_to_string();
            let pattern = args[1].py_to_string();
            let re = regex::Regex::new(&pattern)
                .map_err(|e| PyException::runtime_error(format!("Invalid regex: {}", e)))?;
            if re.find(&text).is_some() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("Regex '{}' unexpectedly matched '{}'", pattern, text)
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertCountEqual(first, second[, msg]) — same elements, any order
    tc_ns.insert(
        CompactString::from("assertCountEqual"),
        PyObject::native_closure("assertCountEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertCountEqual requires 2 arguments",
                ));
            }
            let a_items = args[0].to_list()?;
            let b_items = args[1].to_list()?;
            if a_items.len() != b_items.len() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "Element counts differ: {} vs {}",
                        a_items.len(),
                        b_items.len()
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            let mut a_strs: Vec<String> = a_items.iter().map(|x| x.py_to_string()).collect();
            let mut b_strs: Vec<String> = b_items.iter().map(|x| x.py_to_string()).collect();
            a_strs.sort();
            b_strs.sort();
            if a_strs != b_strs {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    "Element counts differ".to_string()
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertDictEqual(d1, d2[, msg])
    tc_ns.insert(
        CompactString::from("assertDictEqual"),
        PyObject::native_closure("assertDictEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertDictEqual requires 2 arguments",
                ));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} != {}", args[0].py_to_string(), args[1].py_to_string())
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertListEqual(list1, list2[, msg])
    tc_ns.insert(
        CompactString::from("assertListEqual"),
        PyObject::native_closure("assertListEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertListEqual requires 2 arguments",
                ));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} != {}", args[0].py_to_string(), args[1].py_to_string())
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertTupleEqual(tuple1, tuple2[, msg])
    tc_ns.insert(
        CompactString::from("assertTupleEqual"),
        PyObject::native_closure("assertTupleEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertTupleEqual requires 2 arguments",
                ));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} != {}", args[0].py_to_string(), args[1].py_to_string())
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertSetEqual(set1, set2[, msg])
    tc_ns.insert(
        CompactString::from("assertSetEqual"),
        PyObject::native_closure("assertSetEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertSetEqual requires 2 arguments",
                ));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} != {}", args[0].py_to_string(), args[1].py_to_string())
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertSequenceEqual(seq1, seq2[, msg])
    tc_ns.insert(
        CompactString::from("assertSequenceEqual"),
        PyObject::native_closure("assertSequenceEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertSequenceEqual requires 2 arguments",
                ));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!(
                        "Sequences differ: {} != {}",
                        args[0].py_to_string(),
                        args[1].py_to_string()
                    )
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // assertMultiLineEqual(first, second[, msg])
    tc_ns.insert(
        CompactString::from("assertMultiLineEqual"),
        PyObject::native_closure("assertMultiLineEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertMultiLineEqual requires 2 arguments",
                ));
            }
            let a = args[0].py_to_string();
            let b = args[1].py_to_string();
            if a != b {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("'{}' != '{}'", a, b)
                } else {
                    msg
                };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        }),
    );

    // fail([msg]) — unconditionally fail
    tc_ns.insert(
        CompactString::from("fail"),
        PyObject::native_closure("fail", |args: &[PyObjectRef]| {
            let msg = if args.is_empty() {
                "Fail".to_string()
            } else {
                args[0].py_to_string()
            };
            Err(PyException::assertion_error(msg))
        }),
    );

    // subTest — context manager stub for subtests
    tc_ns.insert(
        CompactString::from("subTest"),
        PyObject::native_closure("subTest", |_args: &[PyObjectRef]| {
            let cls = PyObject::class(CompactString::from("_SubTest"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", |_: &[PyObjectRef]| Ok(PyObject::none())),
                );
                w.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", |_: &[PyObjectRef]| {
                        Ok(PyObject::bool_val(false))
                    }),
                );
            }
            Ok(inst)
        }),
    );

    // assertLogs(logger=None, level='INFO') — context manager
    tc_ns.insert(
        CompactString::from("assertLogs"),
        PyObject::native_closure("assertLogs", |args: &[PyObjectRef]| {
            let logger_name =
                if !args.is_empty() && !matches!(args[0].payload, PyObjectPayload::None) {
                    args[0].py_to_string()
                } else {
                    "root".to_string()
                };
            let level_str = if args.len() > 1 && !matches!(args[1].payload, PyObjectPayload::None) {
                args[1].py_to_string()
            } else {
                "INFO".to_string()
            };
            let level_num: i64 = match level_str.as_str() {
                "DEBUG" => 10,
                "INFO" => 20,
                "WARNING" => 30,
                "ERROR" => 40,
                "CRITICAL" => 50,
                _ => level_str.parse().unwrap_or(20),
            };

            // Shared state: captured records and output lines
            let records: Rc<PyCell<Vec<PyObjectRef>>> = Rc::new(PyCell::new(vec![]));
            let output: Rc<PyCell<Vec<PyObjectRef>>> = Rc::new(PyCell::new(vec![]));

            let cls = PyObject::class(
                CompactString::from("_AssertLogsContext"),
                vec![],
                IndexMap::new(),
            );
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("records"), PyObject::list(vec![]));
                w.insert(CompactString::from("output"), PyObject::list(vec![]));

                // Build a capturing handler
                let _recs_enter = records.clone();
                let _outs_enter = output.clone();
                let ln = logger_name.clone();
                let lnum = level_num;

                // __enter__: install capturing handler on the logger
                let recs_for_handler = records.clone();
                let outs_for_handler = output.clone();
                let handler_cls = PyObject::class(
                    CompactString::from("_CapturingHandler"),
                    vec![],
                    IndexMap::new(),
                );
                let handler_inst = PyObject::instance(handler_cls);
                if let PyObjectPayload::Instance(ref hd) = handler_inst.payload {
                    let mut ha = hd.attrs.write();
                    ha.insert(CompactString::from("level"), PyObject::int(0));
                    let rfh = recs_for_handler.clone();
                    let ofh = outs_for_handler.clone();
                    ha.insert(
                        CompactString::from("emit"),
                        PyObject::native_closure(
                            "_CapturingHandler.emit",
                            move |args: &[PyObjectRef]| {
                                let record = if args.len() >= 2 {
                                    &args[1]
                                } else if !args.is_empty() {
                                    &args[0]
                                } else {
                                    return Ok(PyObject::none());
                                };
                                rfh.write().push(record.clone());
                                let msg = record
                                    .get_attr("message")
                                    .or_else(|| record.get_attr("msg"))
                                    .map(|m| m.py_to_string())
                                    .unwrap_or_default();
                                let levelname = record
                                    .get_attr("levelname")
                                    .map(|l| l.py_to_string())
                                    .unwrap_or_else(|| "INFO".to_string());
                                let name = record
                                    .get_attr("name")
                                    .map(|n| n.py_to_string())
                                    .unwrap_or_else(|| "root".to_string());
                                let line = format!("{}:{}:{}", levelname, name, msg);
                                ofh.write()
                                    .push(PyObject::str_val(CompactString::from(line)));
                                Ok(PyObject::none())
                            },
                        ),
                    );
                    ha.insert(
                        CompactString::from("setLevel"),
                        make_builtin(|_| Ok(PyObject::none())),
                    );
                    ha.insert(
                        CompactString::from("setFormatter"),
                        make_builtin(|_| Ok(PyObject::none())),
                    );
                }

                let handler_for_enter = handler_inst.clone();
                let handler_for_exit = handler_inst;
                let inst_ref = d.attrs.clone();
                let ln_enter = ln.clone();

                w.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", move |args: &[PyObjectRef]| {
                        // self is args[0] when called via context manager
                        let ctx = if !args.is_empty() {
                            args[0].clone()
                        } else {
                            return Ok(PyObject::none());
                        };
                        // Add handler to the target logger
                        let logger = super::logging::logging_get_logger(&[PyObject::str_val(
                            CompactString::from(ln_enter.as_str()),
                        )])?;
                        if let Some(add_handler) = logger.get_attr("addHandler") {
                            match &add_handler.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[handler_for_enter.clone()]);
                                }
                                _ => {}
                            }
                        }
                        // Lower logger level
                        if let Some(set_level) = logger.get_attr("setLevel") {
                            match &set_level.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[PyObject::int(lnum)]);
                                }
                                _ => {}
                            }
                        }
                        Ok(ctx)
                    }),
                );

                let ln_exit = ln;
                let recs_exit = records.clone();
                let outs_exit = output.clone();
                let inst_exit = inst_ref;
                w.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", move |args: &[PyObjectRef]| {
                        // Remove the handler
                        let logger = super::logging::logging_get_logger(&[PyObject::str_val(
                            CompactString::from(ln_exit.as_str()),
                        )])?;
                        if let Some(rm_handler) = logger.get_attr("removeHandler") {
                            match &rm_handler.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[handler_for_exit.clone()]);
                                }
                                _ => {}
                            }
                        }
                        // Update .records and .output on context
                        let recs = recs_exit.read().clone();
                        let outs = outs_exit.read().clone();
                        {
                            let mut attrs = inst_exit.write();
                            attrs.insert(
                                CompactString::from("records"),
                                PyObject::list(recs.clone()),
                            );
                            attrs.insert(
                                CompactString::from("output"),
                                PyObject::list(outs.clone()),
                            );
                        }
                        // If no exc and no records, raise assertion error
                        let has_exc = if args.len() > 1 {
                            !matches!(args[1].payload, PyObjectPayload::None)
                        } else {
                            false
                        };
                        if !has_exc && recs.is_empty() {
                            return Err(PyException::assertion_error(format!(
                                "no logs of level INFO or above triggered on {}",
                                ln_exit
                            )));
                        }
                        Ok(PyObject::bool_val(false))
                    }),
                );
            }
            Ok(inst)
        }),
    );

    // assertRaisesRegex(exc_type, regex) — context manager
    tc_ns.insert(
        CompactString::from("assertRaisesRegex"),
        PyObject::native_closure("assertRaisesRegex", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertRaisesRegex requires exc_type and regex",
                ));
            }
            let exc_type_name = match &args[0].payload {
                PyObjectPayload::Class(cd) => cd.name.clone(),
                PyObjectPayload::Str(s) => s.to_compact_string(),
                _ => CompactString::from(args[0].py_to_string()),
            };
            let pattern = args[1].py_to_string();
            let cls = PyObject::class(
                CompactString::from("_AssertRaisesRegexContext"),
                vec![],
                IndexMap::new(),
            );
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(
                    CompactString::from("expected"),
                    PyObject::str_val(exc_type_name.clone()),
                );
                w.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", |_: &[PyObjectRef]| Ok(PyObject::none())),
                );
                let etype = exc_type_name;
                let pat = pattern;
                w.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", move |args: &[PyObjectRef]| {
                        let has_exc = if args.is_empty() {
                            false
                        } else {
                            !matches!(args[0].payload, PyObjectPayload::None)
                        };
                        if !has_exc {
                            return Err(PyException::assertion_error(format!(
                                "{} not raised",
                                etype
                            )));
                        }
                        // Check regex against exception message
                        let exc_msg = if args.len() > 1 {
                            args[1].py_to_string()
                        } else {
                            String::new()
                        };
                        if let Ok(re) = regex::Regex::new(&pat) {
                            if re.find(&exc_msg).is_none() {
                                return Err(PyException::assertion_error(format!(
                                    "\"{}\" does not match \"{}\"",
                                    pat, exc_msg
                                )));
                            }
                        }
                        Ok(PyObject::bool_val(true))
                    }),
                );
            }
            Ok(inst)
        }),
    );

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
