use super::*;

pub(super) fn register_basic_assertions(tc_ns: &mut IndexMap<CompactString, PyObjectRef>) {
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
}
