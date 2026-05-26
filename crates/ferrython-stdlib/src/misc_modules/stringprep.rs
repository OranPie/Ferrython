use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

// ── stringprep module ──

pub fn create_stringprep_module() -> PyObjectRef {
    // RFC 3454 string preparation — used by SASL, LDAP, etc.
    make_module(
        "stringprep",
        vec![
            (
                "in_table_a1",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let c = args[0].py_to_string();
                    let ch = c.chars().next().unwrap_or('\0');
                    // Unassigned code points (simplified check)
                    Ok(PyObject::bool_val(
                        !ch.is_alphanumeric() && !ch.is_ascii() && (ch as u32) > 0xFFFD,
                    ))
                }),
            ),
            (
                "in_table_b1",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let c = args[0].py_to_string();
                    let ch = c.chars().next().unwrap_or('\0');
                    // Commonly mapped to nothing: soft hyphen, zero-width joiner, etc.
                    Ok(PyObject::bool_val(
                        ch == '\u{00AD}'
                            || ch == '\u{200B}'
                            || ch == '\u{200C}'
                            || ch == '\u{200D}',
                    ))
                }),
            ),
            (
                "in_table_c12",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let c = args[0].py_to_string();
                    let ch = c.chars().next().unwrap_or('\0');
                    // Non-ASCII space
                    Ok(PyObject::bool_val(ch.is_whitespace() && !ch.is_ascii()))
                }),
            ),
            (
                "in_table_c21",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let c = args[0].py_to_string();
                    let ch = c.chars().next().unwrap_or('\0');
                    Ok(PyObject::bool_val(ch.is_control() && ch.is_ascii()))
                }),
            ),
            (
                "in_table_c22",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let c = args[0].py_to_string();
                    let ch = c.chars().next().unwrap_or('\0');
                    Ok(PyObject::bool_val(ch.is_control() && !ch.is_ascii()))
                }),
            ),
            (
                "in_table_d1",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let c = args[0].py_to_string();
                    let ch = c.chars().next().unwrap_or('\0');
                    // RTL characters (simplified)
                    Ok(PyObject::bool_val(
                        (ch as u32) >= 0x0590 && (ch as u32) <= 0x08FF,
                    ))
                }),
            ),
            (
                "in_table_d2",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let c = args[0].py_to_string();
                    let ch = c.chars().next().unwrap_or('\0');
                    // LTR characters (simplified: Latin, CJK, etc.)
                    Ok(PyObject::bool_val(
                        ch.is_alphanumeric() && (ch as u32) < 0x0590,
                    ))
                }),
            ),
        ],
    )
}
