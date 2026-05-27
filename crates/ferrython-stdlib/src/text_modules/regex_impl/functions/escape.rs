use super::*;

pub(in crate::text_modules::regex_impl) fn re_escape_needs_backslash(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '?'
            | '*'
            | '+'
            | '-'
            | '|'
            | '^'
            | '$'
            | '\\'
            | '.'
            | '&'
            | '~'
            | '#'
            | ' '
            | '\t'
            | '\n'
            | '\r'
            | '\u{0b}'
            | '\u{0c}'
    )
}

pub(in crate::text_modules::regex_impl) fn re_escape(
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("re.escape() requires a string"));
    }
    match &args[0].payload {
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
            let mut escaped = Vec::with_capacity(bytes.len());
            for &byte in bytes.iter() {
                if re_escape_needs_backslash(byte as char) {
                    escaped.push(b'\\');
                }
                escaped.push(byte);
            }
            Ok(PyObject::bytes(escaped))
        }
        _ => {
            let s = args[0].py_to_string();
            let mut escaped = String::with_capacity(s.len());
            for ch in s.chars() {
                if re_escape_needs_backslash(ch) {
                    escaped.push('\\');
                }
                escaped.push(ch);
            }
            Ok(PyObject::str_val(CompactString::from(escaped)))
        }
    }
}
