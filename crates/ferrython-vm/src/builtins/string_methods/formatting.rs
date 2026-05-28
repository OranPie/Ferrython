use compact_str::CompactString;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectRef};

/// Resolve a format field like "0", "0.attr", "0[key]", "0.attr[0]", or "name".
/// Supports chained attribute access (`.`) and getitem (`[...]`) in any order.
pub(super) fn resolve_format_field(field_name: &str, args: &[PyObjectRef]) -> Option<PyObjectRef> {
    let base_end = field_name
        .find(|c: char| c == '.' || c == '[')
        .unwrap_or(field_name.len());
    let base_name = &field_name[..base_end];
    let rest = &field_name[base_end..];

    let mut current = if let Ok(idx) = base_name.parse::<usize>() {
        args.get(idx)?.clone()
    } else {
        return None;
    };

    let mut chars = rest.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '.' {
            chars.next();
            let mut attr = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == '.' || nc == '[' {
                    break;
                }
                attr.push(nc);
                chars.next();
            }
            if let Some(v) = current.get_attr(&attr) {
                current = v;
            } else {
                return Some(PyObject::str_val(CompactString::from("")));
            }
        } else if c == '[' {
            chars.next();
            let mut key = String::new();
            for nc in chars.by_ref() {
                if nc == ']' {
                    break;
                }
                key.push(nc);
            }
            if let Ok(idx) = key.parse::<i64>() {
                let key_obj = PyObject::int(idx);
                if let Ok(v) = current.get_item(&key_obj) {
                    current = v;
                } else {
                    return Some(PyObject::str_val(CompactString::from("")));
                }
            } else {
                let key_obj = PyObject::str_val(CompactString::from(key));
                if let Ok(v) = current.get_item(&key_obj) {
                    current = v;
                } else {
                    return Some(PyObject::str_val(CompactString::from("")));
                }
            }
        } else {
            break;
        }
    }

    Some(current)
}

/// Resolve nested `{N}` references in a format spec.
/// E.g., `{1}>{2}` with args=['hi', '*', 10] becomes `*>10`.
pub(super) fn resolve_nested_spec(spec: &str, args: &[PyObjectRef]) -> String {
    let mut result = String::new();
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut ref_name = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                ref_name.push(c);
            }
            if let Ok(idx) = ref_name.parse::<usize>() {
                if let Some(val) = args.get(idx) {
                    result.push_str(&val.py_to_string());
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}
