use super::*;

pub fn create_html_module() -> PyObjectRef {
    fn html_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("html.escape requires 1 argument"));
        }
        let s = args[0].py_to_string();
        let quote = if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Bool(b) => *b,
                _ => true,
            }
        } else {
            true
        };
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' if quote => out.push_str("&quot;"),
                '\'' if quote => out.push_str("&#x27;"),
                _ => out.push(c),
            }
        }
        Ok(PyObject::str_val(CompactString::from(out)))
    }

    fn html_unescape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("html.unescape requires 1 argument"));
        }
        let s = args[0].py_to_string();
        let out = s
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#x27;", "'")
            .replace("&#39;", "'")
            .replace("&apos;", "'")
            .replace("&#x2F;", "/")
            .replace("&#x3D;", "=");
        // Handle numeric character references &#NNN; and &#xHHH;
        let mut result = String::new();
        let mut chars = out.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '&' && chars.peek() == Some(&'#') {
                chars.next(); // consume '#'
                let mut num_str = String::new();
                let is_hex = chars.peek() == Some(&'x') || chars.peek() == Some(&'X');
                if is_hex {
                    chars.next();
                }
                for nc in chars.by_ref() {
                    if nc == ';' {
                        break;
                    }
                    num_str.push(nc);
                }
                let code = if is_hex {
                    u32::from_str_radix(&num_str, 16).ok()
                } else {
                    num_str.parse::<u32>().ok()
                };
                if let Some(cp) = code.and_then(char::from_u32) {
                    result.push(cp);
                } else {
                    result.push('&');
                    result.push('#');
                    if is_hex {
                        result.push('x');
                    }
                    result.push_str(&num_str);
                    result.push(';');
                }
            } else {
                result.push(c);
            }
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }

    // _replace_charref is internal CPython — used by html.parser and some libs
    let replace_charref = make_builtin(|args: &[PyObjectRef]| {
        // _replace_charref(s) — replace HTML character references in string
        if args.is_empty() {
            return Ok(PyObject::str_val(CompactString::from("")));
        }
        let s = args[0].py_to_string();
        // Simple passthrough — mistune uses re.sub with this
        Ok(PyObject::str_val(CompactString::from(s)))
    });

    make_module(
        "html",
        vec![
            ("escape", make_builtin(html_escape)),
            ("unescape", make_builtin(html_unescape)),
            ("_replace_charref", replace_charref),
        ],
    )
}
