use super::*;

pub(in crate::text_modules::regex_impl) fn write_re_debug_output(pattern: &str) -> PyResult<()> {
    let text = re_debug_dump(pattern);
    let target = crate::get_stdout_override().or_else(crate::sys_modules::get_current_stdout);
    if let Some(target) = target {
        return write_text_to_file_object(&target, &text);
    }
    print!("{}", text);
    Ok(())
}

pub(in crate::text_modules::regex_impl) fn re_debug_dump(pattern: &str) -> String {
    fn push_line(out: &mut String, indent: usize, text: impl AsRef<str>) {
        out.push_str(&"  ".repeat(indent));
        out.push_str(text.as_ref());
        out.push('\n');
    }

    fn dump_class(chars: &[char], i: &mut usize, indent: usize, out: &mut String) {
        push_line(out, indent, "IN");
        *i += 1;
        if *i < chars.len() && chars[*i] == '^' {
            push_line(out, indent + 1, "NEGATE");
            *i += 1;
        }
        while *i < chars.len() {
            match chars[*i] {
                ']' => {
                    *i += 1;
                    break;
                }
                '\\' if *i + 1 < chars.len() => {
                    push_line(out, indent + 1, format!("CATEGORY \\{}", chars[*i + 1]));
                    *i += 2;
                }
                ch => {
                    push_line(out, indent + 1, format!("LITERAL {}", ch as u32));
                    *i += 1;
                }
            }
        }
    }

    fn dump_until(
        chars: &[char],
        i: &mut usize,
        indent: usize,
        group_no: &mut usize,
        out: &mut String,
    ) {
        while *i < chars.len() {
            match chars[*i] {
                ')' => break,
                '|' => {
                    push_line(out, indent, "OR");
                    *i += 1;
                }
                '[' => dump_class(chars, i, indent, out),
                '\\' if *i + 1 < chars.len() => {
                    let esc = chars[*i + 1];
                    match esc {
                        'A' | 'Z' | 'b' | 'B' => push_line(out, indent, format!("AT \\{}", esc)),
                        'd' | 'D' | 's' | 'S' | 'w' | 'W' => {
                            push_line(out, indent, format!("CATEGORY \\{}", esc))
                        }
                        _ => push_line(out, indent, format!("LITERAL {}", esc as u32)),
                    }
                    *i += 2;
                }
                '(' if *i + 1 < chars.len() && chars[*i + 1] == '?' => {
                    if *i + 2 < chars.len() && chars[*i + 2] == '(' {
                        push_line(out, indent, "GROUPREF_EXISTS");
                        *i += 3;
                        while *i < chars.len() && chars[*i] != ')' {
                            *i += 1;
                        }
                        if *i < chars.len() {
                            *i += 1;
                        }
                    } else if *i + 2 < chars.len() && chars[*i + 2] == ':' {
                        *i += 3;
                        dump_until(chars, i, indent, group_no, out);
                        if *i < chars.len() && chars[*i] == ')' {
                            *i += 1;
                        }
                    } else {
                        let start = *i;
                        *i += 2;
                        while *i < chars.len() && chars[*i] != ':' && chars[*i] != ')' {
                            *i += 1;
                        }
                        if *i < chars.len() && chars[*i] == ':' {
                            let flags: String = chars[start + 2..*i].iter().collect();
                            push_line(out, indent, format!("FLAGS {}", flags));
                            *i += 1;
                            dump_until(chars, i, indent + 1, group_no, out);
                            if *i < chars.len() && chars[*i] == ')' {
                                *i += 1;
                            }
                        }
                    }
                }
                '(' => {
                    *group_no += 1;
                    push_line(out, indent, format!("SUBPATTERN {} 0 0", *group_no));
                    *i += 1;
                    dump_until(chars, i, indent + 1, group_no, out);
                    if *i < chars.len() && chars[*i] == ')' {
                        *i += 1;
                    }
                }
                '*' | '+' | '?' => {
                    push_line(out, indent, format!("REPEAT {}", chars[*i]));
                    *i += 1;
                }
                '{' => {
                    let start = *i;
                    while *i < chars.len() && chars[*i] != '}' {
                        *i += 1;
                    }
                    if *i < chars.len() {
                        *i += 1;
                    }
                    let repeat: String = chars[start..*i].iter().collect();
                    push_line(out, indent, format!("REPEAT {}", repeat));
                }
                '^' => {
                    push_line(out, indent, "AT AT_BEGINNING");
                    *i += 1;
                }
                '$' => {
                    push_line(out, indent, "AT AT_END");
                    *i += 1;
                }
                ch => {
                    push_line(out, indent, format!("LITERAL {}", ch as u32));
                    *i += 1;
                }
            }
        }
    }

    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut group_no = 0;
    let mut out = String::new();
    dump_until(&chars, &mut i, 0, &mut group_no, &mut out);
    out.push('\n');
    out.push_str("SUCCESS\n");
    out
}

pub(in crate::text_modules::regex_impl) fn write_text_to_file_object(
    target: &PyObjectRef,
    text: &str,
) -> PyResult<()> {
    if let Some(write_fn) = target.get_attr("write") {
        let text_obj = PyObject::str_val(CompactString::from(text));
        let bind_self = matches!(write_fn.payload, PyObjectPayload::NativeFunction(_))
            && matches!(target.payload, PyObjectPayload::Module(_))
            && target.get_attr("_bind_methods").is_some();
        if bind_self {
            ferrython_core::object::call_callable(&write_fn, &[target.clone(), text_obj])?;
        } else {
            ferrython_core::object::call_callable(&write_fn, &[text_obj])?;
        }
    } else {
        print!("{}", text);
    }
    Ok(())
}
