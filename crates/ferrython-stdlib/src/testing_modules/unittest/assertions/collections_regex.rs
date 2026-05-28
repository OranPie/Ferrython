use super::*;

pub(super) fn register_collection_regex_assertions(
    tc_ns: &mut IndexMap<CompactString, PyObjectRef>,
) {
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
}
