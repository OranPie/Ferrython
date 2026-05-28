use super::*;

pub(super) fn register_numeric_type_assertions(tc_ns: &mut IndexMap<CompactString, PyObjectRef>) {
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
}
