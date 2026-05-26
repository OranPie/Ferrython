//! Text processing stdlib modules (string, re, textwrap, fnmatch)

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::register_bytearray_export;
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, new_fx_hashkey_map, FxHashKeyMap, IteratorData,
    PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use super::fs_modules::glob_match;

mod difflib;
mod encodings;
mod pprint;
pub use difflib::create_difflib_module;
pub use pprint::create_pprint_module;
mod fnmatch;
mod html;
mod html_parser;
mod unicodedata;
pub use html_parser::create_html_parser_module;
pub use unicodedata::create_unicodedata_module;
use unicodedata::unicode_lookup_name;
mod shlex;
mod textwrap;
pub use encodings::{
    create_encodings_aliases_module, create_encodings_codec_module, create_encodings_idna_module,
    create_encodings_module, create_multibytecodec_module, create_string_internal_module,
};
pub use fnmatch::create_fnmatch_module;
pub use html::create_html_module;
pub use shlex::create_shlex_module;
pub use textwrap::create_textwrap_module;

thread_local! {
    static RE_REGEX_CACHE: RefCell<Vec<(String, i64, regex::Regex)>> = const { RefCell::new(Vec::new()) };
}

fn cached_build_regex(pattern: &str, flags: i64) -> Result<regex::Regex, PyException> {
    RE_REGEX_CACHE.with(|cache| {
        {
            let cache_ref = cache.borrow();
            if let Some((_, _, compiled)) =
                cache_ref.iter().find(|(cached_pattern, cached_flags, _)| {
                    cached_pattern == pattern && *cached_flags == flags
                })
            {
                return Ok(compiled.clone());
            }
        }

        let compiled = build_regex(pattern, flags)?;
        let mut cache_ref = cache.borrow_mut();
        if cache_ref.len() >= 64 {
            cache_ref.remove(0);
        }
        cache_ref.push((pattern.to_string(), flags, compiled.clone()));
        Ok(compiled)
    })
}

pub fn create_string_module() -> PyObjectRef {
    make_module("string", vec![
        ("ascii_lowercase", PyObject::str_val(CompactString::from("abcdefghijklmnopqrstuvwxyz"))),
        ("ascii_uppercase", PyObject::str_val(CompactString::from("ABCDEFGHIJKLMNOPQRSTUVWXYZ"))),
        ("ascii_letters", PyObject::str_val(CompactString::from("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"))),
        ("digits", PyObject::str_val(CompactString::from("0123456789"))),
        ("hexdigits", PyObject::str_val(CompactString::from("0123456789abcdefABCDEF"))),
        ("octdigits", PyObject::str_val(CompactString::from("01234567"))),
        ("punctuation", PyObject::str_val(CompactString::from("!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"))),
        ("whitespace", PyObject::str_val(CompactString::from(" \t\n\r\x0b\x0c"))),
        ("printable", PyObject::str_val(CompactString::from("0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~ \t\n\r\x0b\x0c"))),
        ("Template", PyObject::native_function("string.Template", template_new)),
        ("Formatter", create_formatter_class()),
        ("capwords", make_builtin(string_capwords)),
    ])
}

fn string_capwords(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("capwords() requires a string"));
    }
    let s = args[0].py_to_string();
    let sep = if args.len() > 1 {
        Some(args[1].py_to_string())
    } else {
        None
    };
    let result: String = match sep {
        Some(ref sep_str) => s
            .split(sep_str.as_str())
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                }
            })
            .collect::<Vec<_>>()
            .join(sep_str),
        None => s
            .split_whitespace()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn create_formatter_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("format"),
        make_builtin(formatter_format),
    );
    ns.insert(
        CompactString::from("vformat"),
        make_builtin(formatter_format),
    );
    PyObject::class(CompactString::from("Formatter"), vec![], ns)
}

fn formatter_format(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self (Formatter instance), args[1] = format_string, rest = positional/kwargs
    if args.len() < 2 {
        return Err(PyException::type_error("format() requires a format string"));
    }
    let fmt_str = args[1].py_to_string();
    let pos_args = if args.len() > 2 {
        &args[2..]
    } else {
        &[] as &[PyObjectRef]
    };
    // Check if last arg is a kwargs dict
    let (pos_args_final, kwargs) = if let Some(last) = pos_args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            (&pos_args[..pos_args.len() - 1], Some(map.read().clone()))
        } else {
            (pos_args, None)
        }
    } else {
        (pos_args, None)
    };
    let result = format_string_impl(&fmt_str, pos_args_final, &kwargs)?;
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn format_string_impl(
    fmt: &str,
    pos_args: &[PyObjectRef],
    kwargs: &Option<FxHashKeyMap>,
) -> PyResult<String> {
    let mut result = String::new();
    let mut auto_idx = 0usize;
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                result.push('{');
                i += 2;
                continue;
            }
            i += 1;
            let start = i;
            let mut depth = 1;
            while i < chars.len() && depth > 0 {
                if chars[i] == '{' {
                    depth += 1;
                }
                if chars[i] == '}' {
                    depth -= 1;
                }
                if depth > 0 {
                    i += 1;
                }
            }
            let field: String = chars[start..i].iter().collect();
            i += 1; // skip }
                    // Parse field_name:format_spec
            let (field_name, _format_spec) = if let Some(colon) = field.find(':') {
                (&field[..colon], &field[colon + 1..])
            } else {
                (field.as_str(), "")
            };
            let value = if field_name.is_empty() {
                if auto_idx < pos_args.len() {
                    let v = pos_args[auto_idx].clone();
                    auto_idx += 1;
                    v
                } else {
                    return Err(PyException::index_error("Replacement index out of range"));
                }
            } else if let Ok(idx) = field_name.parse::<usize>() {
                if idx < pos_args.len() {
                    pos_args[idx].clone()
                } else {
                    return Err(PyException::index_error("Replacement index out of range"));
                }
            } else if let Some(ref kw) = kwargs {
                kw.get(&HashableKey::str_key(CompactString::from(field_name)))
                    .cloned()
                    .ok_or_else(|| PyException::key_error(format!("'{}'", field_name)))?
            } else {
                return Err(PyException::key_error(format!("'{}'", field_name)));
            };
            result.push_str(&value.py_to_string());
        } else if chars[i] == '}' {
            if i + 1 < chars.len() && chars[i + 1] == '}' {
                result.push('}');
                i += 2;
                continue;
            }
            result.push('}');
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    Ok(result)
}

fn template_substitute(template: &str, kwargs: &FxHashKeyMap, safe: bool) -> PyResult<String> {
    let mut result = String::new();
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == '$' && i + 1 < len {
            if chars[i + 1] == '$' {
                // Escaped $$
                result.push('$');
                i += 2;
            } else if chars[i + 1] == '{' {
                // ${name} form
                let start = i + 2;
                if let Some(end_pos) = chars[start..].iter().position(|&c| c == '}') {
                    let name: String = chars[start..start + end_pos].iter().collect();
                    let key = HashableKey::str_key(CompactString::from(&name));
                    if let Some(val) = kwargs.get(&key) {
                        result.push_str(&val.py_to_string());
                    } else if safe {
                        result.push_str(&format!("${{{}}}", name));
                    } else {
                        return Err(PyException::key_error(format!("'{}'", name)));
                    }
                    i = start + end_pos + 1;
                } else {
                    result.push('$');
                    i += 1;
                }
            } else if chars[i + 1].is_alphanumeric() || chars[i + 1] == '_' {
                // $name form
                let start = i + 1;
                let mut end = start;
                while end < len && (chars[end].is_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                let name: String = chars[start..end].iter().collect();
                let key = HashableKey::str_key(CompactString::from(&name));
                if let Some(val) = kwargs.get(&key) {
                    result.push_str(&val.py_to_string());
                } else if safe {
                    result.push('$');
                    result.push_str(&name);
                } else {
                    return Err(PyException::key_error(format!("'{}'", name)));
                }
                i = end;
            } else {
                result.push('$');
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    Ok(result)
}

fn extract_kwargs_dict(args: &[PyObjectRef]) -> FxHashKeyMap {
    for arg in args.iter().rev() {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            return d.read().clone();
        }
    }
    new_fx_hashkey_map()
}

fn template_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "Template() requires a template string",
        ));
    }
    let tmpl_str = args[0].py_to_string();
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("template"),
        PyObject::str_val(CompactString::from(tmpl_str)),
    );
    attrs.insert(
        CompactString::from("substitute"),
        PyObject::native_function("Template.substitute", template_substitute_method),
    );
    attrs.insert(
        CompactString::from("safe_substitute"),
        PyObject::native_function("Template.safe_substitute", template_safe_substitute_method),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );
    Ok(PyObject::module_with_attrs(
        CompactString::from("Template"),
        attrs,
    ))
}

fn template_substitute_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("substitute() needs self"));
    }
    let self_obj = &args[0];
    let tmpl = self_obj
        .get_attr("template")
        .ok_or(PyException::attribute_error("template"))?
        .py_to_string();
    let kwargs = extract_kwargs_dict(&args[1..]);
    let result = template_substitute(&tmpl, &kwargs, false)?;
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn template_safe_substitute_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("safe_substitute() needs self"));
    }
    let self_obj = &args[0];
    let tmpl = self_obj
        .get_attr("template")
        .ok_or(PyException::attribute_error("template"))?
        .py_to_string();
    let kwargs = extract_kwargs_dict(&args[1..]);
    let result = template_substitute(&tmpl, &kwargs, true)?;
    Ok(PyObject::str_val(CompactString::from(result)))
}

const RE_FLAG_TEMPLATE: i64 = 1;
const RE_FLAG_IGNORECASE: i64 = 2;
const RE_FLAG_LOCALE: i64 = 4;
const RE_FLAG_MULTILINE: i64 = 8;
const RE_FLAG_DOTALL: i64 = 16;
const RE_FLAG_UNICODE: i64 = 32;
const RE_FLAG_VERBOSE: i64 = 64;
const RE_FLAG_ASCII: i64 = 256;

fn re_pattern_class() -> PyObjectRef {
    static RE_PATTERN_CLASS: OnceLock<PyObjectRef> = OnceLock::new();
    RE_PATTERN_CLASS
        .get_or_init(|| {
            let mut ns = IndexMap::new();
            ns.insert(
                CompactString::from("__module__"),
                PyObject::str_val(CompactString::from("re")),
            );
            ns.insert(
                CompactString::from("match"),
                PyObject::native_function("Pattern.match", compiled_match),
            );
            ns.insert(
                CompactString::from("search"),
                PyObject::native_function("Pattern.search", compiled_search),
            );
            ns.insert(
                CompactString::from("findall"),
                PyObject::native_function("Pattern.findall", compiled_findall),
            );
            ns.insert(
                CompactString::from("finditer"),
                PyObject::native_function("Pattern.finditer", compiled_finditer),
            );
            ns.insert(
                CompactString::from("sub"),
                PyObject::native_function("Pattern.sub", compiled_sub),
            );
            ns.insert(
                CompactString::from("split"),
                PyObject::native_function("Pattern.split", compiled_split),
            );
            ns.insert(
                CompactString::from("fullmatch"),
                PyObject::native_function("Pattern.fullmatch", compiled_fullmatch),
            );
            ns.insert(
                CompactString::from("subn"),
                PyObject::native_function("Pattern.subn", compiled_subn),
            );
            ns.insert(
                CompactString::from("scanner"),
                PyObject::native_function("Pattern.scanner", compiled_scanner),
            );
            ns.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("Pattern.__repr__", pattern_repr),
            );
            ns.insert(
                CompactString::from("__hash__"),
                PyObject::native_function("Pattern.__hash__", pattern_hash),
            );
            ns.insert(
                CompactString::from("__eq__"),
                PyObject::native_function("Pattern.__eq__", pattern_eq),
            );
            ns.insert(
                CompactString::from("__copy__"),
                PyObject::native_function("Pattern.__copy__", return_self),
            );
            ns.insert(
                CompactString::from("__deepcopy__"),
                PyObject::native_function("Pattern.__deepcopy__", return_self),
            );
            for name in ["__lt__", "__le__", "__gt__", "__ge__"] {
                ns.insert(
                    CompactString::from(name),
                    PyObject::native_function("Pattern.order", pattern_order_error),
                );
            }
            PyObject::class(CompactString::from("Pattern"), vec![], ns)
        })
        .clone()
}

fn return_self(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    args.first()
        .cloned()
        .ok_or_else(|| PyException::type_error("missing self"))
}

fn re_scanner_class() -> PyObjectRef {
    static RE_SCANNER_CLASS: OnceLock<PyObjectRef> = OnceLock::new();
    RE_SCANNER_CLASS
        .get_or_init(|| {
            let mut ns = IndexMap::new();
            ns.insert(
                CompactString::from("__module__"),
                PyObject::str_val(CompactString::from("_sre")),
            );
            ns.insert(
                CompactString::from("match"),
                PyObject::native_function("Scanner.match", scanner_match),
            );
            ns.insert(
                CompactString::from("search"),
                PyObject::native_function("Scanner.search", scanner_search),
            );
            PyObject::class(CompactString::from("SRE_Scanner"), vec![], ns)
        })
        .clone()
}

fn regex_flag_class() -> PyObjectRef {
    static REGEX_FLAG_CLASS: OnceLock<PyObjectRef> = OnceLock::new();
    REGEX_FLAG_CLASS
        .get_or_init(|| {
            let mut ns = IndexMap::new();
            ns.insert(
                CompactString::from("__module__"),
                PyObject::str_val(CompactString::from("re")),
            );
            ns.insert(
                CompactString::from("__repr__"),
                PyObject::native_function("RegexFlag.__repr__", regex_flag_repr_method),
            );
            ns.insert(
                CompactString::from("__str__"),
                PyObject::native_function("RegexFlag.__str__", regex_flag_repr_method),
            );
            ns.insert(
                CompactString::from("__int__"),
                PyObject::native_function("RegexFlag.__int__", regex_flag_int_method),
            );
            ns.insert(
                CompactString::from("__index__"),
                PyObject::native_function("RegexFlag.__index__", regex_flag_int_method),
            );
            ns.insert(
                CompactString::from("__or__"),
                PyObject::native_function("RegexFlag.__or__", regex_flag_or_method),
            );
            ns.insert(
                CompactString::from("__ror__"),
                PyObject::native_function("RegexFlag.__ror__", regex_flag_or_method),
            );
            ns.insert(
                CompactString::from("__and__"),
                PyObject::native_function("RegexFlag.__and__", regex_flag_and_method),
            );
            ns.insert(
                CompactString::from("__rand__"),
                PyObject::native_function("RegexFlag.__rand__", regex_flag_and_method),
            );
            ns.insert(
                CompactString::from("__xor__"),
                PyObject::native_function("RegexFlag.__xor__", regex_flag_xor_method),
            );
            ns.insert(
                CompactString::from("__rxor__"),
                PyObject::native_function("RegexFlag.__rxor__", regex_flag_xor_method),
            );
            ns.insert(
                CompactString::from("__invert__"),
                PyObject::native_function("RegexFlag.__invert__", regex_flag_invert_method),
            );
            PyObject::class(
                CompactString::from("RegexFlag"),
                vec![PyObject::builtin_type(CompactString::from("int"))],
                ns,
            )
        })
        .clone()
}

fn regex_flag_int(obj: &PyObjectRef) -> Option<i64> {
    match &obj.payload {
        PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => obj.to_int().ok(),
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__re_flag_value__")
            .and_then(|v| v.to_int().ok())
            .or_else(|| {
                inst.attrs
                    .read()
                    .get("__builtin_value__")
                    .and_then(|v| v.to_int().ok())
            }),
        _ => None,
    }
}

fn regex_flag_obj(value: i64) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__re_flag_value__"),
        PyObject::int(value),
    );
    attrs.insert(
        CompactString::from("__builtin_value__"),
        PyObject::int(value),
    );
    PyObject::instance_with_attrs(regex_flag_class(), attrs)
}

fn regex_flag_repr_text(value: i64) -> String {
    if value < 0 {
        let inverted = !value;
        if let Some(inner) = re_flag_repr(inverted, true) {
            if inner.contains('|') {
                format!("~({})", inner)
            } else {
                format!("~{}", inner)
            }
        } else {
            format!("{}", value)
        }
    } else {
        re_flag_repr(value, true).unwrap_or_else(|| format!("{}", value))
    }
}

fn regex_flag_repr_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("RegexFlag.__repr__ requires self"));
    }
    let value = regex_flag_int(&args[0]).unwrap_or(0);
    Ok(PyObject::str_val(CompactString::from(
        regex_flag_repr_text(value),
    )))
}

fn regex_flag_int_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("RegexFlag.__int__ requires self"));
    }
    Ok(PyObject::int(regex_flag_int(&args[0]).unwrap_or(0)))
}

fn regex_flag_or_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("RegexFlag.__or__ requires other"));
    }
    Ok(regex_flag_obj(
        regex_flag_int(&args[0]).unwrap_or(0) | regex_flag_int(&args[1]).unwrap_or(0),
    ))
}

fn regex_flag_and_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("RegexFlag.__and__ requires other"));
    }
    Ok(regex_flag_obj(
        regex_flag_int(&args[0]).unwrap_or(0) & regex_flag_int(&args[1]).unwrap_or(0),
    ))
}

fn regex_flag_xor_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("RegexFlag.__xor__ requires other"));
    }
    Ok(regex_flag_obj(
        regex_flag_int(&args[0]).unwrap_or(0) ^ regex_flag_int(&args[1]).unwrap_or(0),
    ))
}

fn regex_flag_invert_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "RegexFlag.__invert__ requires self",
        ));
    }
    Ok(regex_flag_obj(!regex_flag_int(&args[0]).unwrap_or(0)))
}

// ── json module (basic) ──

pub fn create_re_module() -> PyObjectRef {
    make_module(
        "re",
        vec![
            ("IGNORECASE", regex_flag_obj(RE_FLAG_IGNORECASE)),
            ("I", regex_flag_obj(RE_FLAG_IGNORECASE)),
            ("MULTILINE", regex_flag_obj(RE_FLAG_MULTILINE)),
            ("M", regex_flag_obj(RE_FLAG_MULTILINE)),
            ("DOTALL", regex_flag_obj(RE_FLAG_DOTALL)),
            ("S", regex_flag_obj(RE_FLAG_DOTALL)),
            ("VERBOSE", regex_flag_obj(RE_FLAG_VERBOSE)),
            ("X", regex_flag_obj(RE_FLAG_VERBOSE)),
            ("UNICODE", regex_flag_obj(RE_FLAG_UNICODE)),
            ("U", regex_flag_obj(RE_FLAG_UNICODE)),
            ("ASCII", regex_flag_obj(RE_FLAG_ASCII)),
            ("A", regex_flag_obj(RE_FLAG_ASCII)),
            ("LOCALE", regex_flag_obj(RE_FLAG_LOCALE)),
            ("L", regex_flag_obj(RE_FLAG_LOCALE)),
            ("TEMPLATE", regex_flag_obj(RE_FLAG_TEMPLATE)),
            ("T", regex_flag_obj(RE_FLAG_TEMPLATE)),
            ("DEBUG", regex_flag_obj(128)),
            ("match", PyObject::native_function("re.match", re_match)),
            ("search", PyObject::native_function("re.search", re_search)),
            (
                "findall",
                PyObject::native_function("re.findall", re_findall),
            ),
            (
                "finditer",
                PyObject::native_function("re.finditer", re_finditer),
            ),
            ("sub", PyObject::native_function("re.sub", re_sub)),
            ("subn", PyObject::native_function("re.subn", re_subn)),
            ("split", PyObject::native_function("re.split", re_split)),
            (
                "compile",
                PyObject::native_function("re.compile", re_compile),
            ),
            (
                "_compile",
                PyObject::native_function("re._compile", re_compile),
            ),
            ("escape", PyObject::native_function("re.escape", re_escape)),
            (
                "fullmatch",
                PyObject::native_function("re.fullmatch", re_fullmatch),
            ),
            ("purge", make_builtin(|_| Ok(PyObject::none()))),
            ("error", PyObject::exception_type(ExceptionKind::ReError)),
            ("Pattern", re_pattern_class()),
            (
                "Match",
                PyObject::class(CompactString::from("Match"), vec![], IndexMap::new()),
            ),
            (
                "Scanner",
                PyObject::native_function("re.Scanner", re_scanner_new),
            ),
        ],
    )
}

fn sre_int_arg(args: &[PyObjectRef], index: usize, name: &str) -> PyResult<i64> {
    args.get(index)
        .ok_or_else(|| PyException::type_error(format!("{}() missing required argument", name)))?
        .to_int()
}

fn sre_ascii_tolower(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = sre_int_arg(args, 0, "ascii_tolower")?;
    let lowered = if (b'A' as i64..=b'Z' as i64).contains(&code) {
        code + 32
    } else {
        code
    };
    Ok(PyObject::int(lowered))
}

fn sre_unicode_tolower(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = sre_int_arg(args, 0, "unicode_tolower")?;
    let lowered = u32::try_from(code)
        .ok()
        .and_then(char::from_u32)
        .and_then(|ch| ch.to_lowercase().next())
        .map(|ch| ch as i64)
        .unwrap_or(code);
    Ok(PyObject::int(lowered))
}

fn sre_ascii_iscased(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = sre_int_arg(args, 0, "ascii_iscased")?;
    Ok(PyObject::bool_val(
        (b'A' as i64..=b'Z' as i64).contains(&code) || (b'a' as i64..=b'z' as i64).contains(&code),
    ))
}

fn sre_unicode_iscased(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = sre_int_arg(args, 0, "unicode_iscased")?;
    let iscased = u32::try_from(code)
        .ok()
        .and_then(char::from_u32)
        .map(|ch| {
            let original = ch.to_string();
            ch.to_lowercase().collect::<String>() != original
                || ch.to_uppercase().collect::<String>() != original
        })
        .unwrap_or(false);
    Ok(PyObject::bool_val(iscased))
}

fn sre_getcodesize(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(4))
}

fn sre_compile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 6 {
        return Err(PyException::type_error(
            "compile() missing required arguments",
        ));
    }
    if !matches!(args[4].payload, PyObjectPayload::Dict(_)) {
        return Err(PyException::type_error(format!(
            "compile() argument 'groupindex' must be dict, not {}",
            args[4].type_name()
        )));
    }
    let PyObjectPayload::List(code) = &args[2].payload else {
        return Err(PyException::type_error(format!(
            "compile() argument 'code' must be list, not {}",
            args[2].type_name()
        )));
    };
    for item in code.read().iter() {
        match item.to_int() {
            Ok(value) if (0..=u32::MAX as i64).contains(&value) => {}
            Ok(_) => {
                return Err(PyException::overflow_error(
                    "regular expression code size limit exceeded",
                ));
            }
            Err(exc) if matches!(exc.kind, ExceptionKind::OverflowError) => {
                return Err(PyException::overflow_error(
                    "regular expression code size limit exceeded",
                ));
            }
            Err(exc) => return Err(exc),
        }
    }
    Err(PyException::new(
        ExceptionKind::RuntimeError,
        CompactString::from("invalid SRE code"),
    ))
}

pub fn create_sre_module() -> PyObjectRef {
    make_module(
        "_sre",
        vec![
            ("MAGIC", PyObject::int(20171005)),
            ("CODESIZE", PyObject::int(4)),
            ("MAXREPEAT", PyObject::int(u32::MAX as i64)),
            ("MAXGROUPS", PyObject::int(2_147_483_647)),
            (
                "ascii_tolower",
                PyObject::native_function("_sre.ascii_tolower", sre_ascii_tolower),
            ),
            (
                "unicode_tolower",
                PyObject::native_function("_sre.unicode_tolower", sre_unicode_tolower),
            ),
            (
                "ascii_iscased",
                PyObject::native_function("_sre.ascii_iscased", sre_ascii_iscased),
            ),
            (
                "unicode_iscased",
                PyObject::native_function("_sre.unicode_iscased", sre_unicode_iscased),
            ),
            (
                "getcodesize",
                PyObject::native_function("_sre.getcodesize", sre_getcodesize),
            ),
            (
                "compile",
                PyObject::native_function("_sre.compile", sre_compile),
            ),
        ],
    )
}

fn is_re_pattern_object(obj: &PyObjectRef) -> bool {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        if attrs.contains_key("__re_pattern__") {
            return true;
        }
        if !attrs.contains_key("_pattern_text") {
            return false;
        }
        drop(attrs);
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            return cd.name.as_str() == "Pattern";
        }
    }
    false
}

fn re_pattern_text_attr(obj: &PyObjectRef) -> Option<String> {
    if !is_re_pattern_object(obj) {
        return None;
    }
    obj.get_attr("_pattern_text").map(|v| v.py_to_string())
}

fn re_pattern_is_bytes(obj: &PyObjectRef) -> bool {
    if is_re_pattern_object(obj) {
        return obj
            .get_attr("_pattern_is_bytes")
            .map(|v| v.is_truthy())
            .unwrap_or(false);
    }
    extract_bytes_like(obj).is_some()
}

fn readonly_mapping(map: FxHashKeyMap) -> PyObjectRef {
    PyObject::wrap(PyObjectPayload::MappingProxy(Rc::new(PyCell::new(map))))
}

fn bytes_to_regex_text(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| byte as char).collect()
}

fn regex_text_to_bytes(text: &str) -> Vec<u8> {
    text.chars().map(|ch| ch as u32 as u8).collect()
}

fn py_re_text(text: &str, is_bytes: bool) -> PyObjectRef {
    if is_bytes {
        PyObject::bytes(regex_text_to_bytes(text))
    } else {
        PyObject::str_val(CompactString::from(text))
    }
}

fn extract_bytes_like(obj: &PyObjectRef) -> Option<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
            Some((**bytes).clone())
        }
        PyObjectPayload::Instance(inst) => {
            let next = {
                let attrs = inst.attrs.read();
                if attrs
                    .get("__array__")
                    .map(|flag| flag.is_truthy())
                    .unwrap_or(false)
                {
                    if let Some(data) = attrs.get("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let bytes: Vec<u8> = items
                                .read()
                                .iter()
                                .filter_map(|item| item.to_int().ok().map(|value| value as u8))
                                .collect();
                            return Some(bytes);
                        }
                    }
                }
                attrs.get("__builtin_value__").cloned().or_else(|| {
                    if attrs
                        .get("__memoryview__")
                        .map(|flag| flag.is_truthy())
                        .unwrap_or(false)
                    {
                        attrs.get("obj").cloned()
                    } else {
                        None
                    }
                })
            };
            next.and_then(|value| extract_bytes_like(&value))
        }
        _ => None,
    }
}

fn extract_str_like(obj: &PyObjectRef) -> Option<String> {
    match &obj.payload {
        PyObjectPayload::Str(_) => Some(obj.py_to_string()),
        PyObjectPayload::Instance(inst) => {
            let next = inst.attrs.read().get("__builtin_value__").cloned();
            next.and_then(|value| extract_str_like(&value))
        }
        _ => None,
    }
}

fn extract_re_subject(obj: &PyObjectRef) -> PyResult<(String, bool)> {
    if let Some(bytes) = extract_bytes_like(obj) {
        return Ok((bytes_to_regex_text(&bytes), true));
    }
    if let Some(text) = extract_str_like(obj) {
        return Ok((text, false));
    }
    Err(PyException::type_error(
        "expected string or bytes-like object",
    ))
}

fn ensure_re_compatible(pattern_obj: &PyObjectRef, subject_is_bytes: bool) -> PyResult<()> {
    if re_pattern_is_bytes(pattern_obj) != subject_is_bytes {
        return Err(PyException::type_error(
            "cannot use a string pattern on a bytes-like object",
        ));
    }
    Ok(())
}

fn extract_re_replacement(obj: &PyObjectRef, subject_is_bytes: bool) -> PyResult<String> {
    if subject_is_bytes {
        if let Some(bytes) = extract_bytes_like(obj) {
            return Ok(bytes_to_regex_text(&bytes));
        }
        return Err(PyException::type_error(
            "sequence item must be bytes-like object",
        ));
    }
    if let Some(text) = extract_str_like(obj) {
        return Ok(text);
    }
    Err(PyException::type_error(
        "sequence item must be str instance",
    ))
}

/// Extract regex pattern string from either a str, bytes, or compiled Pattern.
/// For bytes, decodes as Latin-1 to preserve all byte values as chars.
fn extract_re_pattern(obj: &PyObjectRef) -> PyResult<String> {
    if let Some(pattern) = re_pattern_text_attr(obj) {
        return Ok(pattern);
    }
    if let Some(bytes) = extract_bytes_like(obj) {
        return Ok(bytes_to_regex_text(&bytes));
    }
    if let Some(text) = extract_str_like(obj) {
        return Ok(text);
    }
    match &obj.payload {
        _ => Err(PyException::type_error(
            "first argument must be string or compiled pattern",
        )),
    }
}

fn extract_re_pattern_and_flags(obj: &PyObjectRef, supplied_flags: i64) -> PyResult<(String, i64)> {
    let pattern = extract_re_pattern(obj)?;
    if is_re_pattern_object(obj) {
        if supplied_flags != 0 {
            return Err(PyException::value_error(
                "cannot process flags argument with a compiled pattern",
            ));
        }
        let flags = obj
            .get_attr("flags")
            .and_then(|f| f.to_int().ok())
            .unwrap_or(0);
        Ok((pattern, flags))
    } else {
        Ok((pattern, supplied_flags))
    }
}

fn leading_inline_flags(pattern: &str) -> i64 {
    split_leading_inline_flags(pattern).1
}

fn split_leading_inline_flags(pattern: &str) -> (&str, i64) {
    let bytes = pattern.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'(' || bytes[1] != b'?' {
        return (pattern, 0);
    }
    let mut flags = 0;
    let mut i = 2;
    while i < bytes.len() {
        match bytes[i] {
            b'i' => flags |= RE_FLAG_IGNORECASE,
            b'L' => flags |= RE_FLAG_LOCALE,
            b'm' => flags |= RE_FLAG_MULTILINE,
            b's' => flags |= RE_FLAG_DOTALL,
            b'u' => flags |= RE_FLAG_UNICODE,
            b'x' => flags |= RE_FLAG_VERBOSE,
            b'a' => flags |= RE_FLAG_ASCII,
            b')' => return (&pattern[i + 1..], flags),
            b':' | b'-' => return (pattern, 0),
            _ => return (pattern, 0),
        }
        i += 1;
    }
    (pattern, 0)
}

fn anchor_pattern(pattern: &str, suffix: &str) -> String {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    let mut anchored = format!("^(?:{}){}", body, suffix);
    if inline_flags != 0 {
        let mut prefix = String::from("(?");
        if inline_flags & RE_FLAG_IGNORECASE != 0 {
            prefix.push('i');
        }
        if inline_flags & RE_FLAG_LOCALE != 0 {
            prefix.push('L');
        }
        if inline_flags & RE_FLAG_MULTILINE != 0 {
            prefix.push('m');
        }
        if inline_flags & RE_FLAG_DOTALL != 0 {
            prefix.push('s');
        }
        if inline_flags & RE_FLAG_UNICODE != 0 {
            prefix.push('u');
        }
        if inline_flags & RE_FLAG_VERBOSE != 0 {
            prefix.push('x');
        }
        if inline_flags & RE_FLAG_ASCII != 0 {
            prefix.push('a');
        }
        prefix.push(')');
        anchored = format!("{}{}", prefix, anchored);
    }
    anchored
}

fn effective_re_flags(pattern: &str, flags: i64, is_bytes: bool) -> i64 {
    let mut effective = flags | leading_inline_flags(pattern);
    if !is_bytes && effective & RE_FLAG_ASCII == 0 {
        effective |= RE_FLAG_UNICODE;
    }
    effective
}

fn regex_engine_flags(flags: i64, is_bytes: bool) -> i64 {
    if is_bytes && flags & RE_FLAG_LOCALE == 0 {
        flags | RE_FLAG_ASCII
    } else {
        flags
    }
}

fn is_simple_nonboundary_pattern(pattern: &str) -> bool {
    split_leading_inline_flags(pattern).0 == r"\B"
}

fn re_flag_repr(flags: i64, is_bytes: bool) -> Option<String> {
    let mut remaining = if is_bytes {
        flags
    } else {
        flags & !RE_FLAG_UNICODE
    };
    let mut parts = Vec::new();
    for (bit, name) in [
        (RE_FLAG_IGNORECASE, "re.IGNORECASE"),
        (RE_FLAG_LOCALE, "re.LOCALE"),
        (RE_FLAG_MULTILINE, "re.MULTILINE"),
        (RE_FLAG_DOTALL, "re.DOTALL"),
        (RE_FLAG_VERBOSE, "re.VERBOSE"),
        (RE_FLAG_ASCII, "re.ASCII"),
        (RE_FLAG_TEMPLATE, "re.TEMPLATE"),
    ] {
        if remaining & bit != 0 {
            parts.push(name.to_string());
            remaining &= !bit;
        }
    }
    if remaining != 0 {
        parts.push(format!("0x{:x}", remaining));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("|"))
    }
}

fn write_re_debug_output(pattern: &str) -> PyResult<()> {
    let text = re_debug_dump(pattern);
    let target = crate::get_stdout_override().or_else(crate::sys_modules::get_current_stdout);
    if let Some(target) = target {
        return write_text_to_file_object(&target, &text);
    }
    print!("{}", text);
    Ok(())
}

fn re_debug_dump(pattern: &str) -> String {
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

fn write_text_to_file_object(target: &PyObjectRef, text: &str) -> PyResult<()> {
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

fn shorten_pattern_repr(repr: String) -> String {
    const BODY_LIMIT: usize = 220;
    if repr.chars().count() <= BODY_LIMIT + 4 {
        return repr;
    }
    let (prefix, suffix_len) = if repr.starts_with("b'") && repr.ends_with('\'') {
        ("b'", 1)
    } else if repr.starts_with("b\"") && repr.ends_with('"') {
        ("b\"", 1)
    } else if repr.starts_with('\'') && repr.ends_with('\'') {
        ("'", 1)
    } else if repr.starts_with('"') && repr.ends_with('"') {
        ("\"", 1)
    } else {
        let mut s: String = repr.chars().take(BODY_LIMIT).collect();
        s.push_str("...");
        return s;
    };
    let body_start = prefix.len();
    let body_end = repr.len().saturating_sub(suffix_len);
    let body = &repr[body_start..body_end];
    let short_body: String = body.chars().take(BODY_LIMIT).collect();
    format!("{}{}...{}", prefix, short_body, &repr[body_end..])
}

fn compiled_pattern_text(self_obj: &PyObjectRef) -> PyResult<String> {
    if let Some(text) = re_pattern_text_attr(self_obj) {
        return Ok(text);
    }
    let pattern_obj = self_obj
        .get_attr("pattern")
        .ok_or(PyException::attribute_error("pattern"))?;
    extract_re_pattern(&pattern_obj)
}

fn compiled_pattern_flags(self_obj: &PyObjectRef) -> i64 {
    self_obj
        .get_attr("flags")
        .and_then(|f| f.to_int().ok())
        .unwrap_or(0)
}

fn pattern_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Pattern.__repr__ requires self"));
    }
    let self_obj = &args[0];
    let pattern_obj = self_obj
        .get_attr("pattern")
        .unwrap_or_else(|| PyObject::str_val(CompactString::from("")));
    let pat_repr = shorten_pattern_repr(pattern_obj.repr());
    let flags = compiled_pattern_flags(self_obj);
    let is_bytes = re_pattern_is_bytes(self_obj);
    let result = if let Some(flag_repr) = re_flag_repr(flags, is_bytes) {
        format!("re.compile({}, {})", pat_repr, flag_repr)
    } else {
        format!("re.compile({})", pat_repr)
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn pattern_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Pattern.__hash__ requires self"));
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let self_obj = &args[0];
    let mut hasher = DefaultHasher::new();
    compiled_pattern_text(self_obj)?.hash(&mut hasher);
    compiled_pattern_flags(self_obj).hash(&mut hasher);
    re_pattern_is_bytes(self_obj).hash(&mut hasher);
    Ok(PyObject::int(hasher.finish() as i64))
}

fn pattern_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    if !is_re_pattern_object(&args[1]) {
        return Ok(PyObject::bool_val(false));
    }
    let left = &args[0];
    let right = &args[1];
    Ok(PyObject::bool_val(
        compiled_pattern_text(left)? == compiled_pattern_text(right)?
            && compiled_pattern_flags(left) == compiled_pattern_flags(right)
            && re_pattern_is_bytes(left) == re_pattern_is_bytes(right),
    ))
}

fn pattern_order_error(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Err(PyException::type_error(
        "'<' not supported between instances of 're.Pattern' and 're.Pattern'",
    ))
}

fn ascii_escape_class(ch: char) -> Option<&'static str> {
    match ch {
        's' => Some(r"[ \t\n\r\f\v]"),
        'S' => Some(r"[^ \t\n\r\f\v]"),
        'd' => Some(r"[0-9]"),
        'D' => Some(r"[^0-9]"),
        'w' => Some(r"[A-Za-z0-9_]"),
        'W' => Some(r"[^A-Za-z0-9_]"),
        _ => None,
    }
}

fn normalize_future_set_ops(pattern: &str) -> String {
    if !(pattern.contains("--")
        || pattern.contains("&&")
        || pattern.contains("||")
        || pattern.contains("~~"))
    {
        return pattern.to_string();
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = String::with_capacity(pattern.len());
    let mut i = 0;
    let mut in_class = false;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == '[' && !in_class {
            in_class = true;
            result.push(chars[i]);
            i += 1;
            continue;
        }
        if chars[i] == ']' && in_class {
            in_class = false;
            result.push(chars[i]);
            i += 1;
            continue;
        }
        if in_class
            && i + 1 < chars.len()
            && chars[i] == chars[i + 1]
            && matches!(chars[i], '-' | '&' | '|' | '~')
        {
            let code = chars[i] as u32;
            result.push_str(&format!(r"\x{{{:x}}}\x{{{:x}}}", code, code));
            i += 2;
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn parse_decimal_saturating(chars: &[char]) -> Option<u64> {
    if chars.is_empty() || !chars.iter().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let mut value = 0_u64;
    for &ch in chars {
        value = value
            .saturating_mul(10)
            .saturating_add(ch.to_digit(10).unwrap_or(0) as u64);
    }
    Some(value)
}

fn parse_decimal_bytes_limited(bytes: &[u8], limit: u64) -> PyResult<Option<u64>> {
    if bytes.is_empty() || !bytes.iter().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let mut value = 0_u64;
    for &byte in bytes {
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add((byte - b'0') as u64))
            .ok_or_else(|| PyException::overflow_error("the repetition number is too large"))?;
        if value >= limit {
            return Err(PyException::overflow_error(
                "the repetition number is too large",
            ));
        }
    }
    Ok(Some(value))
}

fn normalize_repeat_for_rust(chars: &[char], start: usize) -> Option<(String, usize)> {
    const REPEAT_COMPILE_LIMIT: u64 = 100_001;
    if chars.get(start) != Some(&'{') {
        return None;
    }
    let mut close = start + 1;
    while close < chars.len() && chars[close] != '}' {
        close += 1;
    }
    if close >= chars.len() {
        return None;
    }
    let body = &chars[start + 1..close];
    let comma = body.iter().position(|&ch| ch == ',');
    let (min, max, valid) = match comma {
        Some(pos) => {
            let left = &body[..pos];
            let right = &body[pos + 1..];
            let min = if left.is_empty() {
                Some(0)
            } else {
                parse_decimal_saturating(left)
            };
            let max = if right.is_empty() {
                None
            } else {
                parse_decimal_saturating(right)
            };
            (
                min,
                max,
                min.is_some() && (right.is_empty() || max.is_some()),
            )
        }
        None => {
            let value = parse_decimal_saturating(body);
            (value, value, value.is_some())
        }
    };
    if !valid {
        return None;
    }
    let min = min.unwrap_or(0);
    let mut end = close + 1;
    let lazy = end < chars.len() && chars[end] == '?';
    if lazy {
        end += 1;
    }
    let suffix = if lazy { "?" } else { "" };
    let normalized = match (comma, max) {
        (Some(_), Some(max)) if min == 0 && max > REPEAT_COMPILE_LIMIT => {
            format!("*{}", suffix)
        }
        (Some(_), None) if min > REPEAT_COMPILE_LIMIT => {
            format!("{{{}}}{}", REPEAT_COMPILE_LIMIT, suffix)
        }
        (Some(_), Some(max)) if min > REPEAT_COMPILE_LIMIT => {
            let capped = REPEAT_COMPILE_LIMIT.min(max);
            format!("{{{}}}{}", capped, suffix)
        }
        (Some(_), Some(max)) if max > REPEAT_COMPILE_LIMIT => {
            format!("{{{},}}{}", min, suffix)
        }
        (Some(_), Some(max)) => format!("{{{},{}}}{}", min, max, suffix),
        (Some(_), None) => format!("{{{},}}{}", min, suffix),
        (None, _) if min > REPEAT_COMPILE_LIMIT => {
            format!("{{{}}}{}", REPEAT_COMPILE_LIMIT, suffix)
        }
        (None, _) => format!("{{{}}}{}", min, suffix),
    };
    Some((normalized, end))
}

fn parse_named_unicode_escape(chars: &[char], start: usize) -> Option<(char, usize)> {
    if start + 2 >= chars.len() || chars[start] != '\\' || chars[start + 1] != 'N' {
        return None;
    }
    if chars[start + 2] != '{' {
        return None;
    }
    let name_start = start + 3;
    let mut end = name_start;
    while end < chars.len() && chars[end] != '}' {
        end += 1;
    }
    if end >= chars.len() || end == name_start {
        return None;
    }
    let name: String = chars[name_start..end].iter().collect();
    unicode_lookup_name(&name).map(|ch| (ch, end + 1))
}

fn convert_python_regex(pattern: &str, flags: i64) -> String {
    // Convert Python regex syntax to Rust regex syntax
    let normalized_pattern = normalize_future_set_ops(pattern);
    let normalized_pattern =
        convert_scoped_ascii_flags(&normalized_pattern, flags & RE_FLAG_ASCII != 0);
    let chars: Vec<char> = normalized_pattern.chars().collect();
    let mut result = String::with_capacity(normalized_pattern.len());
    let mut i = 0;
    let mut in_char_class = false;
    let ascii_mode = flags & 256 != 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            // Octal escapes apply both inside and outside char classes
            match chars[i + 1] {
                'N' => {
                    if let Some((ch, end)) = parse_named_unicode_escape(&chars, i) {
                        result.push(ch);
                        i = end;
                        continue;
                    }
                }
                '0'..='7' => {
                    let start = i + 1;
                    let mut end = start + 1;
                    // Consume up to 3 octal digits total (Python allows \0 through \377)
                    while end < chars.len()
                        && end < start + 3
                        && chars[end] >= '0'
                        && chars[end] <= '7'
                    {
                        end += 1;
                    }
                    let oct_str: String = chars[start..end].iter().collect();
                    // Only treat as octal if the value fits in a byte, or if it starts with 0
                    // (to distinguish from backreferences like \1..\9 outside char classes)
                    let is_octal = in_char_class
                        || chars[i + 1] == '0'
                        || (end - start >= 2 && chars[i + 1] <= '3');
                    if is_octal {
                        if let Ok(val) = u32::from_str_radix(&oct_str, 8) {
                            if val <= 0x7f {
                                result.push_str(&format!("\\x{:02x}", val));
                            } else {
                                // Unicode escape for values > 127
                                result.push_str(&format!("\\u{{{:04x}}}", val));
                            }
                            i = end;
                            continue;
                        }
                    }
                    if !in_char_class {
                        // Not octal — pass through (might be backreference)
                        result.push(chars[i]);
                        result.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    // In char class, pass through
                    result.push(chars[i]);
                    result.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                _ => {}
            }
            if ascii_mode {
                if let Some(class) = ascii_escape_class(chars[i + 1]) {
                    result.push_str(class);
                    i += 2;
                    continue;
                }
            }
            if !in_char_class {
                match chars[i + 1] {
                    'Z' => {
                        result.push_str("\\z");
                        i += 2;
                        continue;
                    }
                    'a' => {
                        result.push_str("\\x07");
                        i += 2;
                        continue;
                    } // Python \a = bell (BEL)
                    _ => {}
                }
            }
            // Pass through escaped chars (including inside char class)
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
        } else if !in_char_class && chars[i] == '[' {
            in_char_class = true;
            result.push('[');
            i += 1;
            // Handle negation and ] as first char
            if i < chars.len() && chars[i] == '^' {
                result.push('^');
                i += 1;
            }
            // ] as first char in class is literal
            if i < chars.len() && chars[i] == ']' {
                result.push(']');
                i += 1;
            }
        } else if in_char_class && chars[i] == ']' {
            in_char_class = false;
            result.push(']');
            i += 1;
        } else if in_char_class && chars[i] == '[' {
            // Escape bare [ inside character class (Rust regex treats it as nested class)
            result.push_str("\\[");
            i += 1;
        } else if !in_char_class && chars[i] == '{' {
            if let Some((repeat, end)) = normalize_repeat_for_rust(&chars, i) {
                result.push_str(&repeat);
                i = end;
            } else if i + 1 < chars.len() && chars[i + 1] == '}' {
                // CPython treats an empty repeat marker as literal braces.
                result.push_str("\\{\\}");
                i += 2;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else if !in_char_class && chars[i] == '(' && i + 1 < chars.len() && chars[i + 1] == '?' {
            // Convert conditional groups (?(N)yes|no) → (?:yes|no)
            if i + 2 < chars.len() && chars[i + 2] == '(' {
                let mut j = i + 3;
                while j < chars.len() && chars[j] != ')' {
                    j += 1;
                }
                if j < chars.len() {
                    result.push_str("(?:");
                    i = j + 1;
                    continue;
                }
            }
            result.push(chars[i]);
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

fn convert_scoped_ascii_flags(pattern: &str, default_ascii: bool) -> String {
    fn parse_flags(chars: &[char], start: usize) -> Option<(usize, bool, String)> {
        let mut i = start;
        let mut ascii = None;
        let mut rust_flags = String::new();
        while i < chars.len() {
            match chars[i] {
                'a' => ascii = Some(true),
                'u' => ascii = Some(false),
                'L' => {}
                'i' | 'm' | 's' | 'x' | '-' => rust_flags.push(chars[i]),
                ':' => return Some((i, ascii.unwrap_or(false), rust_flags)),
                ')' => return None,
                _ => return None,
            }
            i += 1;
        }
        None
    }

    fn find_group_end(chars: &[char], start: usize) -> Option<usize> {
        let mut i = start;
        let mut depth = 1usize;
        let mut in_class = false;
        while i < chars.len() {
            match chars[i] {
                '\\' => i += 2,
                '[' if !in_class => {
                    in_class = true;
                    i += 1;
                }
                ']' if in_class => {
                    in_class = false;
                    i += 1;
                }
                '(' if !in_class => {
                    depth += 1;
                    i += 1;
                }
                ')' if !in_class => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }
        None
    }

    fn push_escape(out: &mut String, esc: char, ascii: bool, in_class: bool) {
        if ascii {
            if in_class {
                match esc {
                    'w' => out.push_str("A-Za-z0-9_"),
                    'd' => out.push_str("0-9"),
                    's' => out.push_str(" \\t\\n\\r\\f\\v"),
                    _ => {
                        out.push('\\');
                        out.push(esc);
                    }
                }
            } else if let Some(class) = ascii_escape_class(esc) {
                out.push_str(class);
            } else {
                out.push('\\');
                out.push(esc);
            }
        } else {
            out.push('\\');
            out.push(esc);
        }
    }

    fn convert_range(chars: &[char], start: usize, end: usize, ascii: bool, out: &mut String) {
        let mut i = start;
        let mut in_class = false;
        while i < end {
            if chars[i] == '\\' && i + 1 < end {
                push_escape(out, chars[i + 1], ascii, in_class);
                i += 2;
            } else if chars[i] == '[' && !in_class {
                in_class = true;
                out.push('[');
                i += 1;
            } else if chars[i] == ']' && in_class {
                in_class = false;
                out.push(']');
                i += 1;
            } else if !in_class && chars[i] == '(' && i + 2 < end && chars[i + 1] == '?' {
                if let Some((colon, scoped_ascii, rust_flags)) = parse_flags(chars, i + 2) {
                    if let Some(close) = find_group_end(chars, colon + 1) {
                        if rust_flags.is_empty() {
                            out.push_str("(?:");
                        } else {
                            out.push_str("(?");
                            out.push_str(&rust_flags);
                            out.push(':');
                        }
                        convert_range(chars, colon + 1, close, scoped_ascii, out);
                        out.push(')');
                        i = close + 1;
                        continue;
                    }
                }
                out.push(chars[i]);
                i += 1;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
    }

    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    convert_range(&chars, 0, chars.len(), default_ascii, &mut out);
    out
}

/// Convert Python replacement string syntax to Rust regex syntax.
/// Python uses `\1`, `\2`, `\g<name>`, `\g<1>` for backreferences.
/// Rust regex uses `$1`, `$2`, `$name`, `${1}`.
fn python_repl_to_rust(repl: &str) -> String {
    fn push_literal(result: &mut String, ch: char) {
        if ch == '$' {
            result.push_str("$$");
        } else {
            result.push(ch);
        }
    }

    let mut result = String::with_capacity(repl.len());
    let chars: Vec<char> = repl.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '0' {
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && digits.len() < 3 && matches!(chars[j], '0'..='7') {
                    digits.push(chars[j]);
                    j += 1;
                }
                if let Ok(value) = u32::from_str_radix(&digits, 8) {
                    if let Some(ch) = char::from_u32(value) {
                        push_literal(&mut result, ch);
                    }
                }
                i = j;
            } else if matches!(next, '1'..='9') {
                if i + 3 < chars.len()
                    && matches!(chars[i + 1], '0'..='3')
                    && matches!(chars[i + 2], '0'..='7')
                    && matches!(chars[i + 3], '0'..='7')
                {
                    let digits: String = chars[i + 1..=i + 3].iter().collect();
                    if let Ok(value) = u32::from_str_radix(&digits, 8) {
                        if let Some(ch) = char::from_u32(value) {
                            push_literal(&mut result, ch);
                        }
                    }
                    i += 4;
                } else {
                    let mut j = i + 1;
                    let mut digits = String::new();
                    while j < chars.len() && digits.len() < 2 && chars[j].is_ascii_digit() {
                        digits.push(chars[j]);
                        j += 1;
                    }
                    result.push_str(&format!("${{{}}}", digits));
                    i = j;
                }
            } else if next == 'g' && i + 2 < chars.len() && chars[i + 2] == '<' {
                i += 3;
                let start = i;
                while i < chars.len() && chars[i] != '>' {
                    i += 1;
                }
                let group: String = chars[start..i].iter().collect();
                if i < chars.len() {
                    i += 1;
                }
                result.push_str(&format!("${{{}}}", group));
            } else if next == '\\' {
                result.push('\\');
                i += 2;
            } else {
                let literal = match next {
                    'a' => Some('\x07'),
                    'b' => Some('\x08'),
                    'f' => Some('\x0c'),
                    'n' => Some('\n'),
                    'r' => Some('\r'),
                    't' => Some('\t'),
                    'v' => Some('\x0b'),
                    _ => None,
                };
                if let Some(ch) = literal {
                    push_literal(&mut result, ch);
                } else {
                    result.push('\\');
                    result.push(next);
                }
                i += 2;
            }
        } else {
            push_literal(&mut result, chars[i]);
            i += 1;
        }
    }
    result
}

fn group_count_from_pattern_obj(obj: &PyObjectRef) -> usize {
    obj.get_attr("groups")
        .and_then(|v| v.to_int().ok())
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0)
}

fn groupindex_contains(obj: &PyObjectRef, name: &str) -> bool {
    obj.get_attr("groupindex")
        .and_then(|groupindex| {
            if let PyObjectPayload::MappingProxy(map) | PyObjectPayload::Dict(map) =
                &groupindex.payload
            {
                let key = HashableKey::str_key(CompactString::from(name));
                Some(map.read().contains_key(&key))
            } else {
                None
            }
        })
        .unwrap_or(false)
}

fn re_error_with_pattern(
    message: impl Into<String>,
    pos: Option<usize>,
    pattern: Option<PyObjectRef>,
) -> PyException {
    let msg = message.into();
    let (lineno, colno) = if let (Some(pos), Some(pattern_obj)) = (pos, pattern.as_ref()) {
        let text = extract_re_pattern(pattern_obj).unwrap_or_else(|_| pattern_obj.py_to_string());
        let before = text.chars().take(pos).collect::<String>();
        let line = before.chars().filter(|&ch| ch == '\n').count() + 1;
        let col = before
            .rsplit_once('\n')
            .map(|(_, tail)| tail.chars().count() + 1)
            .unwrap_or_else(|| before.chars().count() + 1);
        (line, col)
    } else {
        (0, 0)
    };
    let display = match pos {
        Some(pos) if lineno > 1 => format!(
            "{} at position {} (line {}, column {})",
            msg, pos, lineno, colno
        ),
        Some(pos) => format!("{} at position {}", msg, pos),
        None => msg.clone(),
    };
    let mut attrs = ferrython_core::object::FxAttrMap::default();
    attrs.insert(
        CompactString::from("msg"),
        PyObject::str_val(CompactString::from(msg)),
    );
    attrs.insert(
        CompactString::from("pos"),
        pos.map(|p| PyObject::int(p as i64))
            .unwrap_or_else(PyObject::none),
    );
    attrs.insert(
        CompactString::from("pattern"),
        pattern.unwrap_or_else(PyObject::none),
    );
    attrs.insert(CompactString::from("lineno"), PyObject::int(lineno as i64));
    attrs.insert(CompactString::from("colno"), PyObject::int(colno as i64));
    let original = PyObject::wrap(PyObjectPayload::ExceptionInstance(
        std::mem::ManuallyDrop::new(Box::new(
            ferrython_core::object::ExceptionInstanceData::new_attrs(
                ExceptionKind::ReError,
                CompactString::from(display.clone()),
                vec![PyObject::str_val(CompactString::from(display.clone()))],
                Some(Rc::new(PyCell::new(attrs))),
            ),
        )),
    ));
    PyException::with_original(
        ExceptionKind::ReError,
        CompactString::from(display),
        original,
    )
}

fn re_error(message: impl Into<String>, pos: Option<usize>) -> PyException {
    re_error_with_pattern(message, pos, None)
}

fn re_pattern_error(
    message: impl Into<String>,
    pos: Option<usize>,
    pattern_obj: &PyObjectRef,
) -> PyException {
    re_error_with_pattern(message, pos, Some(pattern_obj.clone()))
}

fn parse_decimal_limited(chars: &[char], limit: u64) -> Result<u64, ()> {
    let mut value = 0_u64;
    for &ch in chars {
        let digit = ch.to_digit(10).ok_or(())? as u64;
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(digit))
            .ok_or(())?;
        if value >= limit {
            return Err(());
        }
    }
    Ok(value)
}

fn repeat_quantifier_end(
    chars: &[char],
    start: usize,
    pattern_obj: &PyObjectRef,
) -> PyResult<Option<usize>> {
    let len = chars.len();
    let mut end = match chars[start] {
        '*' | '+' | '?' => start + 1,
        '{' => {
            let mut close = start + 1;
            while close < len && chars[close] != '}' {
                close += 1;
            }
            if close >= len {
                return Ok(None);
            }
            let body = &chars[start + 1..close];
            let comma = body.iter().position(|&ch| ch == ',');
            let valid = match comma {
                Some(pos) => {
                    let left = &body[..pos];
                    let right = &body[pos + 1..];
                    (!left.is_empty() && left.iter().all(|ch| ch.is_ascii_digit()))
                        || (!right.is_empty() && right.iter().all(|ch| ch.is_ascii_digit()))
                }
                None => !body.is_empty() && body.iter().all(|ch| ch.is_ascii_digit()),
            };
            if !valid {
                return Ok(None);
            }
            let limit = u32::MAX as u64;
            let min = match comma {
                Some(0) => 0,
                Some(pos) => parse_decimal_limited(&body[..pos], limit).map_err(|_| {
                    PyException::overflow_error("the repetition number is too large")
                })?,
                None => parse_decimal_limited(body, limit).map_err(|_| {
                    PyException::overflow_error("the repetition number is too large")
                })?,
            };
            let max = match comma {
                Some(pos) if pos + 1 == body.len() => None,
                Some(pos) => {
                    Some(parse_decimal_limited(&body[pos + 1..], limit).map_err(|_| {
                        PyException::overflow_error("the repetition number is too large")
                    })?)
                }
                None => Some(min),
            };
            if let Some(max) = max {
                if min > max {
                    return Err(re_pattern_error(
                        "min repeat greater than max repeat",
                        Some(start + 1),
                        pattern_obj,
                    ));
                }
            }
            close + 1
        }
        _ => return Ok(None),
    };
    if end < len && chars[end] == '?' {
        end += 1;
    }
    Ok(Some(end))
}

fn validate_escape(
    chars: &[char],
    start: usize,
    in_class: bool,
    is_bytes: bool,
    group_count: usize,
    open_captures: &[(usize, usize)],
    pattern_obj: &PyObjectRef,
) -> PyResult<usize> {
    if start + 1 >= chars.len() {
        return Err(re_pattern_error(
            "bad escape (end of pattern)",
            Some(start),
            pattern_obj,
        ));
    }
    let next = chars[start + 1];
    if is_bytes && matches!(next, 'u' | 'U' | 'N') {
        return Err(re_pattern_error(
            format!("bad escape \\{}", next),
            Some(start),
            pattern_obj,
        ));
    }
    match next {
        '0'..='7' => {
            let mut end = start + 1;
            while end < chars.len() && end < start + 4 && matches!(chars[end], '0'..='7') {
                end += 1;
            }
            let digits: String = chars[start + 1..end].iter().collect();
            if digits.len() == 3 {
                if let Ok(value) = u32::from_str_radix(&digits, 8) {
                    if value > 0o377 {
                        return Err(re_pattern_error(
                            format!("octal escape value \\{} outside of range 0-0o377", digits),
                            Some(start),
                            pattern_obj,
                        ));
                    }
                }
            }
            Ok(end)
        }
        '8' | '9' if in_class => Err(re_pattern_error(
            format!("bad escape \\{}", next),
            Some(start),
            pattern_obj,
        )),
        '1'..='9' => {
            let mut end = start + 1;
            while end < chars.len() && end < start + 3 && chars[end].is_ascii_digit() {
                end += 1;
            }
            let digits: String = chars[start + 1..end].iter().collect();
            let group = digits.parse::<usize>().unwrap_or(usize::MAX);
            if open_captures.iter().any(|&(_, n)| n == group) {
                return Err(re_pattern_error(
                    "cannot refer to an open group",
                    Some(start),
                    pattern_obj,
                ));
            }
            if group > group_count {
                return Err(re_pattern_error(
                    format!("invalid group reference {}", group),
                    Some(start + 1),
                    pattern_obj,
                ));
            }
            Ok(end)
        }
        'x' => {
            let end = (start + 4).min(chars.len());
            let ok = start + 3 < chars.len()
                && chars[start + 2].is_ascii_hexdigit()
                && chars[start + 3].is_ascii_hexdigit();
            if !ok {
                let mut frag_end = start + 2;
                while frag_end < chars.len()
                    && frag_end < start + 4
                    && chars[frag_end].is_ascii_hexdigit()
                {
                    frag_end += 1;
                }
                let fragment: String = chars[start..frag_end].iter().collect();
                return Err(re_pattern_error(
                    format!("incomplete escape {}", fragment),
                    Some(start),
                    pattern_obj,
                ));
            }
            Ok(end)
        }
        'u' | 'U' => {
            let needed = if next == 'u' { 4 } else { 8 };
            let end = start + 2 + needed;
            let ok = end <= chars.len()
                && chars[start + 2..end]
                    .iter()
                    .all(|ch| ch.is_ascii_hexdigit());
            if !ok {
                let mut frag_end = start + 2;
                while frag_end < chars.len()
                    && frag_end < end
                    && chars[frag_end].is_ascii_hexdigit()
                {
                    frag_end += 1;
                }
                let fragment: String = chars[start..frag_end].iter().collect();
                return Err(re_pattern_error(
                    format!("incomplete escape {}", fragment),
                    Some(start),
                    pattern_obj,
                ));
            }
            let digits: String = chars[start + 2..end].iter().collect();
            if u32::from_str_radix(&digits, 16).map_or(true, |value| value > 0x10ffff) {
                let fragment: String = chars[start..end].iter().collect();
                return Err(re_pattern_error(
                    format!("bad escape {}", fragment),
                    Some(start),
                    pattern_obj,
                ));
            }
            Ok(end)
        }
        'N' => {
            if start + 2 >= chars.len() || chars[start + 2] != '{' {
                return Err(re_pattern_error("missing {", Some(start + 2), pattern_obj));
            }
            let name_start = start + 3;
            if name_start >= chars.len() || chars[name_start] == '}' {
                return Err(re_pattern_error(
                    "missing character name",
                    Some(name_start),
                    pattern_obj,
                ));
            }
            let mut end = name_start;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end >= chars.len() {
                return Err(re_pattern_error(
                    "missing }, unterminated name",
                    Some(name_start),
                    pattern_obj,
                ));
            }
            let name: String = chars[name_start..end].iter().collect();
            if unicode_lookup_name(&name).is_none() {
                return Err(re_pattern_error(
                    format!("undefined character name '{}'", name),
                    Some(start),
                    pattern_obj,
                ));
            }
            Ok(end + 1)
        }
        _ if next.is_ascii_alphabetic() => {
            let allowed = if in_class {
                "bBdDsSwWafnrtvxuUN"
            } else {
                "AbBdDsSwWZafnrtvxuUN"
            };
            if !allowed.contains(next) {
                return Err(re_pattern_error(
                    format!("bad escape \\{}", next),
                    Some(start),
                    pattern_obj,
                ));
            }
            Ok(start + 2)
        }
        _ => Ok(start + 2),
    }
}

fn validate_character_class(
    chars: &[char],
    start: usize,
    is_bytes: bool,
    pattern_obj: &PyObjectRef,
) -> PyResult<usize> {
    let len = chars.len();
    let mut end = start + 1;
    if end < len && chars[end] == '^' {
        end += 1;
    }
    if end < len && chars[end] == ']' {
        end += 1;
    }
    while end < len {
        if chars[end] == '\\' {
            if end + 2 < len && chars[end + 1] == 'N' && chars[end + 2] == '{' {
                end += 3;
                while end < len && chars[end] != '}' {
                    end += 1;
                }
                if end < len {
                    end += 1;
                }
            } else {
                end = (end + 2).min(len);
            }
            continue;
        }
        if chars[end] == ']' {
            break;
        }
        end += 1;
    }
    if end >= len {
        let mut i = start + 1;
        while i < len {
            if chars[i] == '\\' {
                i = validate_escape(chars, i, true, is_bytes, 0, &[], pattern_obj)?;
            } else {
                i += 1;
            }
        }
        return Err(re_pattern_error(
            "unterminated character set",
            Some(start),
            pattern_obj,
        ));
    }
    let mut i = start + 1;
    while i < end {
        if chars[i] == '\\' {
            i = validate_escape(chars, i, true, is_bytes, 0, &[], pattern_obj)?;
        } else {
            i += 1;
        }
    }
    let body: String = chars[start + 1..end].iter().collect();
    if let Some(pos) = body.find("\\w-") {
        let after = body[pos + 3..].chars().next().unwrap_or(']');
        return Err(re_pattern_error(
            format!("bad character range \\w-{}", after),
            Some(start + 1 + pos),
            pattern_obj,
        ));
    }
    if let Some(pos) = body.find("-\\w") {
        let before = body[..pos].chars().next_back().unwrap_or('[');
        return Err(re_pattern_error(
            format!("bad character range {}-\\w", before),
            Some(start + 1 + pos.saturating_sub(1)),
            pattern_obj,
        ));
    }
    for i in start + 1..end.saturating_sub(2) {
        if chars[i + 1] == '-'
            && chars[i] != '\\'
            && chars[i + 2] != '\\'
            && chars[i + 2] != '-'
            && chars[i] > chars[i + 2]
        {
            return Err(re_pattern_error(
                format!("bad character range {}-{}", chars[i], chars[i + 2]),
                Some(i),
                pattern_obj,
            ));
        }
    }
    Ok(end + 1)
}

fn validate_group_name(name: &str, pos: usize, pattern_obj: &PyObjectRef) -> PyResult<()> {
    if name.is_empty() {
        return Err(re_pattern_error(
            "missing group name",
            Some(pos),
            pattern_obj,
        ));
    }
    if !is_group_name(name) {
        return Err(re_pattern_error(
            format!("bad character in group name '{}'", name),
            Some(pos),
            pattern_obj,
        ));
    }
    Ok(())
}

fn validate_re_pattern_syntax(
    pattern: &str,
    is_bytes: bool,
    pattern_obj: &PyObjectRef,
) -> PyResult<()> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut group_count = 0usize;
    let mut atom_available = false;
    let mut last_was_repeat = false;
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i = validate_escape(
                    &chars,
                    i,
                    false,
                    is_bytes,
                    group_count,
                    &groups,
                    pattern_obj,
                )?;
                atom_available = true;
                last_was_repeat = false;
            }
            '[' => {
                i = validate_character_class(&chars, i, is_bytes, pattern_obj)?;
                atom_available = true;
                last_was_repeat = false;
            }
            '(' => {
                if i + 1 < chars.len() && chars[i + 1] == '?' {
                    if i + 2 >= chars.len() {
                        return Err(re_pattern_error(
                            "unexpected end of pattern",
                            Some(i + 2),
                            pattern_obj,
                        ));
                    }
                    match chars[i + 2] {
                        '#' => {
                            let mut end = i + 3;
                            while end < chars.len() && chars[end] != ')' {
                                end += 1;
                            }
                            if end >= chars.len() {
                                return Err(re_pattern_error(
                                    "missing ), unterminated comment",
                                    Some(i),
                                    pattern_obj,
                                ));
                            }
                            i = end + 1;
                            atom_available = false;
                            last_was_repeat = false;
                        }
                        ':' => {
                            groups.push((i, 0));
                            i += 3;
                            atom_available = false;
                            last_was_repeat = false;
                        }
                        'P' => {
                            if i + 3 >= chars.len() {
                                return Err(re_pattern_error(
                                    "unexpected end of pattern",
                                    Some(i + 3),
                                    pattern_obj,
                                ));
                            }
                            match chars[i + 3] {
                                '<' => {
                                    let name_start = i + 4;
                                    let mut end = name_start;
                                    while end < chars.len() && chars[end] != '>' {
                                        end += 1;
                                    }
                                    if end >= chars.len() {
                                        return Err(re_pattern_error(
                                            "missing >, unterminated name",
                                            Some(name_start),
                                            pattern_obj,
                                        ));
                                    }
                                    let name: String = chars[name_start..end].iter().collect();
                                    validate_group_name(&name, name_start, pattern_obj)?;
                                    group_count += 1;
                                    groups.push((i, group_count));
                                    i = end + 1;
                                    atom_available = false;
                                    last_was_repeat = false;
                                }
                                '=' => {
                                    let name_start = i + 4;
                                    let mut end = name_start;
                                    while end < chars.len() && chars[end] != ')' {
                                        end += 1;
                                    }
                                    let name: String = chars[name_start..end].iter().collect();
                                    validate_group_name(&name, name_start, pattern_obj)?;
                                    i = end;
                                    atom_available = true;
                                    last_was_repeat = false;
                                }
                                other => {
                                    return Err(re_pattern_error(
                                        format!("unknown extension ?P{}", other),
                                        Some(i + 1),
                                        pattern_obj,
                                    ));
                                }
                            }
                        }
                        '<' => {
                            if i + 3 >= chars.len() {
                                return Err(re_pattern_error(
                                    "unexpected end of pattern",
                                    Some(i + 3),
                                    pattern_obj,
                                ));
                            }
                            if chars[i + 3] == '=' || chars[i + 3] == '!' {
                                groups.push((i, 0));
                                i += 4;
                                atom_available = false;
                                last_was_repeat = false;
                            } else {
                                let mut end = i + 3;
                                while end < chars.len() && chars[end] != ')' {
                                    end += 1;
                                }
                                let ext: String =
                                    chars[i + 1..end.min(chars.len())].iter().collect();
                                return Err(re_pattern_error(
                                    format!("unknown extension {}", ext),
                                    Some(i + 1),
                                    pattern_obj,
                                ));
                            }
                        }
                        '(' => {
                            let name_start = i + 3;
                            let mut end = name_start;
                            while end < chars.len() && chars[end] != ')' {
                                end += 1;
                            }
                            let name: String =
                                chars[name_start..end.min(chars.len())].iter().collect();
                            if name.is_empty() {
                                return Err(re_pattern_error(
                                    "missing group name",
                                    Some(name_start),
                                    pattern_obj,
                                ));
                            }
                            if name.chars().all(|ch| ch.is_ascii_digit()) {
                                let group = parse_decimal_limited(
                                    &chars[name_start..end],
                                    usize::MAX as u64,
                                )
                                .unwrap_or(usize::MAX as u64)
                                    as usize;
                                if group > group_count {
                                    return Err(re_pattern_error(
                                        format!("invalid group reference {}", group),
                                        Some(name_start),
                                        pattern_obj,
                                    ));
                                }
                            } else {
                                validate_group_name(&name, name_start, pattern_obj)?;
                            }
                            groups.push((i, 0));
                            i = if end < chars.len() { end + 1 } else { end };
                            atom_available = false;
                            last_was_repeat = false;
                        }
                        flag if matches!(flag, 'a' | 'i' | 'L' | 'm' | 's' | 'u' | 'x' | '-') => {
                            let mut end = i + 2;
                            while end < chars.len()
                                && matches!(
                                    chars[end],
                                    'a' | 'i' | 'L' | 'm' | 's' | 'u' | 'x' | '-'
                                )
                            {
                                end += 1;
                            }
                            if end >= chars.len() {
                                return Err(re_pattern_error(
                                    "missing -, : or )",
                                    Some(end),
                                    pattern_obj,
                                ));
                            }
                            match chars[end] {
                                ')' => {
                                    validate_inline_flag_set(
                                        &chars[i + 2..end],
                                        &[],
                                        i + 2,
                                        pattern_obj,
                                    )?;
                                    i = end + 1;
                                    atom_available = false;
                                    last_was_repeat = false;
                                }
                                ':' => {
                                    let (enabled, disabled) =
                                        split_inline_flag_parts(&chars[i + 2..end]);
                                    validate_inline_flag_set(
                                        enabled,
                                        disabled,
                                        i + 2,
                                        pattern_obj,
                                    )?;
                                    groups.push((i, 0));
                                    i = end + 1;
                                    atom_available = false;
                                    last_was_repeat = false;
                                }
                                _ => {
                                    return Err(re_pattern_error(
                                        "unknown flag",
                                        Some(end),
                                        pattern_obj,
                                    ));
                                }
                            }
                        }
                        other => {
                            return Err(re_pattern_error(
                                format!("unknown extension ?{}", other),
                                Some(i + 1),
                                pattern_obj,
                            ));
                        }
                    }
                } else {
                    group_count += 1;
                    groups.push((i, group_count));
                    i += 1;
                    atom_available = false;
                    last_was_repeat = false;
                }
            }
            ')' => {
                if groups.pop().is_none() {
                    return Err(re_pattern_error(
                        "unbalanced parenthesis",
                        Some(i),
                        pattern_obj,
                    ));
                }
                i += 1;
                atom_available = true;
                last_was_repeat = false;
            }
            '*' | '+' | '?' | '{' => {
                if let Some(end) = repeat_quantifier_end(&chars, i, pattern_obj)? {
                    if last_was_repeat {
                        return Err(re_pattern_error("multiple repeat", Some(i), pattern_obj));
                    }
                    if !atom_available {
                        return Err(re_pattern_error("nothing to repeat", Some(i), pattern_obj));
                    }
                    i = end;
                    atom_available = false;
                    last_was_repeat = true;
                } else {
                    i += 1;
                    atom_available = true;
                    last_was_repeat = false;
                }
            }
            '|' | '^' | '$' => {
                i += 1;
                atom_available = false;
                last_was_repeat = false;
            }
            _ => {
                i += 1;
                atom_available = true;
                last_was_repeat = false;
            }
        }
    }
    if let Some(&(pos, _)) = groups.first() {
        return Err(re_pattern_error(
            "missing ), unterminated subpattern",
            Some(pos),
            pattern_obj,
        ));
    }
    Ok(())
}

fn split_inline_flag_parts<'a>(flags: &'a [char]) -> (&'a [char], &'a [char]) {
    if let Some(pos) = flags.iter().position(|&ch| ch == '-') {
        (&flags[..pos], &flags[pos + 1..])
    } else {
        (flags, &[])
    }
}

fn validate_inline_flag_set(
    enabled: &[char],
    disabled: &[char],
    base_pos: usize,
    pattern_obj: &PyObjectRef,
) -> PyResult<()> {
    if enabled.is_empty() && !disabled.is_empty() {
        return Err(re_pattern_error(
            "missing flag",
            Some(base_pos + enabled.len() + 1),
            pattern_obj,
        ));
    }
    for (idx, flag) in enabled.iter().enumerate() {
        if !matches!(flag, 'a' | 'i' | 'L' | 'm' | 's' | 'u' | 'x') {
            return Err(re_pattern_error(
                "unknown flag",
                Some(base_pos + idx),
                pattern_obj,
            ));
        }
    }
    for (idx, flag) in disabled.iter().enumerate() {
        if !matches!(flag, 'i' | 'm' | 's' | 'x' | 'a' | 'u' | 'L') {
            return Err(re_pattern_error(
                "unknown flag",
                Some(base_pos + enabled.len() + 1 + idx),
                pattern_obj,
            ));
        }
        if matches!(flag, 'a' | 'u' | 'L') {
            return Err(re_pattern_error(
                "bad inline flags: cannot turn off flags 'a', 'u' and 'L'",
                Some(base_pos + enabled.len() + 1 + idx),
                pattern_obj,
            ));
        }
    }
    if enabled
        .iter()
        .any(|flag| disabled.iter().any(|disabled| disabled == flag))
    {
        let off_pos = disabled
            .iter()
            .position(|flag| enabled.iter().any(|enabled| enabled == flag))
            .unwrap_or(0);
        return Err(re_pattern_error(
            "bad inline flags: flag turned on and off",
            Some(base_pos + enabled.len() + 1 + off_pos),
            pattern_obj,
        ));
    }
    let mode_flags = enabled
        .iter()
        .filter(|&&flag| matches!(flag, 'a' | 'u' | 'L'))
        .count();
    if mode_flags > 1 {
        let pos = enabled
            .iter()
            .enumerate()
            .filter(|(_, &flag)| matches!(flag, 'a' | 'u' | 'L'))
            .nth(1)
            .map(|(idx, _)| base_pos + idx)
            .unwrap_or(base_pos);
        return Err(re_pattern_error(
            "bad inline flags: flags 'a', 'u' and 'L' are incompatible",
            Some(pos),
            pattern_obj,
        ));
    }
    Ok(())
}

fn is_ascii_letter(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

fn is_group_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

fn validate_numeric_group(group: usize, group_count: usize, pos: usize) -> PyResult<()> {
    if group > group_count {
        Err(re_error(
            format!("invalid group reference {}", group),
            Some(pos),
        ))
    } else {
        Ok(())
    }
}

fn validate_replacement_template(repl: &str, pattern_obj: &PyObjectRef) -> PyResult<()> {
    let group_count = group_count_from_pattern_obj(pattern_obj);
    let chars: Vec<char> = repl.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' {
            i += 1;
            continue;
        }
        let slash_pos = i;
        if i + 1 >= chars.len() {
            break;
        }
        let next = chars[i + 1];
        match next {
            'a' | 'b' | 'f' | 'n' | 'r' | 't' | 'v' | '\\' => {
                i += 2;
            }
            '0' => {
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && digits.len() < 3 && matches!(chars[j], '0'..='7') {
                    digits.push(chars[j]);
                    j += 1;
                }
                if digits.len() == 3 {
                    if let Ok(value) = u32::from_str_radix(&digits, 8) {
                        if value > 0o377 {
                            return Err(re_error(
                                format!("octal escape value \\{} outside of range 0-0o377", digits),
                                Some(slash_pos),
                            ));
                        }
                    }
                }
                i = j;
            }
            '1'..='9' => {
                if i + 3 < chars.len()
                    && matches!(chars[i + 1], '0'..='7')
                    && matches!(chars[i + 2], '0'..='7')
                    && matches!(chars[i + 3], '0'..='7')
                {
                    let digits: String = chars[i + 1..=i + 3].iter().collect();
                    if let Ok(value) = u32::from_str_radix(&digits, 8) {
                        if value > 0o377 {
                            return Err(re_error(
                                format!("octal escape value \\{} outside of range 0-0o377", digits),
                                Some(slash_pos),
                            ));
                        }
                    }
                    i += 4;
                    continue;
                }
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && digits.len() < 2 && chars[j].is_ascii_digit() {
                    digits.push(chars[j]);
                    j += 1;
                }
                let group = digits.parse::<usize>().unwrap_or(0);
                validate_numeric_group(group, group_count, slash_pos + 1)?;
                i = j;
            }
            'g' => {
                if i + 2 >= chars.len() || chars[i + 2] != '<' {
                    return Err(re_error("missing <", Some(slash_pos + 2)));
                }
                let name_start = i + 3;
                let mut j = name_start;
                while j < chars.len() && chars[j] != '>' {
                    j += 1;
                }
                if j >= chars.len() {
                    if name_start >= chars.len() {
                        return Err(re_error("missing group name", Some(name_start)));
                    }
                    return Err(re_error("missing >, unterminated name", Some(name_start)));
                }
                let name: String = chars[name_start..j].iter().collect();
                if name.is_empty() {
                    return Err(re_error("missing group name", Some(name_start)));
                }
                if name.chars().all(|ch| ch.is_ascii_digit()) {
                    let group = name.parse::<usize>().unwrap_or(usize::MAX);
                    validate_numeric_group(group, group_count, name_start)?;
                } else if !is_group_name(&name) {
                    return Err(re_error(
                        format!("bad character in group name '{}'", name),
                        Some(name_start),
                    ));
                } else if !groupindex_contains(pattern_obj, &name) {
                    return Err(PyException::index_error(format!(
                        "unknown group name '{}'",
                        name
                    )));
                }
                i = j + 1;
            }
            _ if is_ascii_letter(next) => {
                return Err(re_error(format!("bad escape \\{}", next), Some(slash_pos)));
            }
            _ => {
                i += 2;
            }
        }
    }
    Ok(())
}

fn validate_replacement_for_pattern(
    pattern_obj: &PyObjectRef,
    flags: i64,
    repl: &str,
) -> PyResult<()> {
    if !repl.contains('\\') {
        return Ok(());
    }
    if is_re_pattern_object(pattern_obj) {
        validate_replacement_template(repl, pattern_obj)
    } else {
        let compiled = re_compile(&[pattern_obj.clone(), PyObject::int(flags)])?;
        validate_replacement_template(repl, &compiled)
    }
}

fn needs_fancy_regex(pattern: &str) -> bool {
    // Detect lookahead/lookbehind which require fancy-regex
    let bytes = pattern.as_bytes();
    let len = bytes.len();
    for i in 0..len.saturating_sub(1) {
        if bytes[i] == b'(' && bytes[i + 1] == b'?' && i + 2 < len {
            match bytes[i + 2] {
                b'=' | b'!' => return true, // (?= (?!
                b'<' if i + 3 < len && (bytes[i + 3] == b'=' || bytes[i + 3] == b'!') => {
                    return true
                } // (?<= (?<!
                _ => {}
            }
        }
        if bytes[i] == b'\\' && i + 1 < len && bytes[i + 1].is_ascii_digit() && bytes[i + 1] != b'0'
        {
            return true;
        }
    }
    false
}

/// Strip VERBOSE (re.X) comments and unescaped whitespace from a regex pattern.
fn strip_verbose(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut in_char_class = false;
    let mut verbose = true;
    let mut verbose_stack: Vec<bool> = Vec::new();
    'outer: while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' && i + 1 < chars.len() {
            // Escaped character — always keep
            result.push(ch);
            result.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if ch == '[' && !in_char_class {
            in_char_class = true;
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == ']' && in_char_class {
            in_char_class = false;
            result.push(ch);
            i += 1;
            continue;
        }
        if in_char_class {
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == '(' && i + 2 < chars.len() && chars[i + 1] == '?' && chars[i + 2] == '#' {
            while i < chars.len() {
                result.push(chars[i]);
                if chars[i] == ')' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if ch == '(' && i + 1 < chars.len() && chars[i + 1] == '?' {
            let mut j = i + 2;
            let mut scoped_verbose = verbose;
            let mut negated = false;
            while j < chars.len() {
                match chars[j] {
                    'x' if negated => scoped_verbose = false,
                    'x' => scoped_verbose = true,
                    '-' => negated = true,
                    'a' | 'i' | 'L' | 'm' | 's' | 'u' => {}
                    ':' => {
                        for ch in &chars[i..=j] {
                            result.push(*ch);
                        }
                        verbose_stack.push(verbose);
                        verbose = scoped_verbose;
                        i = j + 1;
                        continue 'outer;
                    }
                    ')' => {
                        for ch in &chars[i..=j] {
                            result.push(*ch);
                        }
                        verbose = scoped_verbose;
                        i = j + 1;
                        continue 'outer;
                    }
                    _ => break,
                }
                j += 1;
            }
        }
        if ch == '(' {
            verbose_stack.push(verbose);
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == ')' {
            if let Some(previous) = verbose_stack.pop() {
                verbose = previous;
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if verbose && ch == '#' {
            // Skip to end of line
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            i += 1; // skip the newline too
            continue;
        }
        if verbose && ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        result.push(ch);
        i += 1;
    }
    result
}

fn build_regex(pattern: &str, flags: i64) -> Result<regex::Regex, PyException> {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    let effective_flags = flags | inline_flags;
    let mut pat = if effective_flags & RE_FLAG_VERBOSE != 0 {
        strip_verbose(body)
    } else {
        body.to_string()
    };
    pat = convert_python_regex(&pat, effective_flags);
    let mut prefix = String::new();
    if effective_flags & RE_FLAG_IGNORECASE != 0 {
        prefix.push_str("(?i)");
    }
    if effective_flags & RE_FLAG_MULTILINE != 0 {
        prefix.push_str("(?m)");
    }
    if effective_flags & RE_FLAG_DOTALL != 0 {
        prefix.push_str("(?s)");
    }
    pat = format!("{}{}", prefix, pat);
    regex::Regex::new(&pat).map_err(|e| PyException::runtime_error(format!("re: {}", e)))
}

fn build_fancy_regex(pattern: &str, flags: i64) -> Result<fancy_regex::Regex, PyException> {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    let effective_flags = flags | inline_flags;
    let mut pat = if effective_flags & RE_FLAG_VERBOSE != 0 {
        strip_verbose(body)
    } else {
        body.to_string()
    };
    pat = convert_python_regex(&pat, effective_flags);
    let mut prefix = String::new();
    if effective_flags & RE_FLAG_IGNORECASE != 0 {
        prefix.push_str("(?i)");
    }
    if effective_flags & RE_FLAG_MULTILINE != 0 {
        prefix.push_str("(?m)");
    }
    if effective_flags & RE_FLAG_DOTALL != 0 {
        prefix.push_str("(?s)");
    }
    pat = format!("{}{}", prefix, pat);
    fancy_regex::Regex::new(&pat).map_err(|e| PyException::runtime_error(format!("re: {}", e)))
}

fn fancy_find_all(re: &fancy_regex::Regex, text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut pos = 0;
    while pos <= text.len() {
        match re.find(&text[pos..]) {
            Ok(Some(m)) => {
                if m.start() == m.end() {
                    pos += 1;
                    continue;
                }
                results.push(m.as_str().to_string());
                pos += m.end();
            }
            _ => break,
        }
    }
    results
}

fn fancy_captures(re: &fancy_regex::Regex, text: &str) -> Vec<Vec<Option<String>>> {
    let mut results = Vec::new();
    let mut pos = 0;
    while pos <= text.len() {
        match re.captures(&text[pos..]) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                if whole.start() == whole.end() {
                    pos += 1;
                    continue;
                }
                let mut groups = Vec::new();
                for i in 0..caps.len() {
                    groups.push(caps.get(i).map(|m| m.as_str().to_string()));
                }
                results.push(groups);
                pos += whole.end();
            }
            _ => break,
        }
    }
    results
}

/// Extract named capture group index from a fancy_regex::Regex
fn extract_fancy_group_names(re: &fancy_regex::Regex) -> FxHashKeyMap {
    let mut map = new_fx_hashkey_map();
    // fancy_regex exposes capture_names()
    for (idx, name_opt) in re.capture_names().enumerate() {
        if let Some(name) = name_opt {
            map.insert(
                HashableKey::str_key(CompactString::from(name)),
                PyObject::int(idx as i64),
            );
        }
    }
    map
}

fn regex_offset_to_py_index(text: &str, offset: usize, is_bytes: bool) -> i64 {
    let _ = is_bytes;
    text[..offset.min(text.len())].chars().count() as i64
}

fn py_index_to_regex_offset(text: &str, index: usize) -> usize {
    if index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(index)
        .map(|(offset, _)| offset)
        .unwrap_or(text.len())
}

fn py_span(start: i64, end: i64) -> PyObjectRef {
    PyObject::tuple(vec![PyObject::int(start), PyObject::int(end)])
}

fn group_spans_to_py(spans: &[Option<(i64, i64)>]) -> PyObjectRef {
    PyObject::tuple(
        spans
            .iter()
            .map(|span| match span {
                Some((start, end)) => py_span(*start, *end),
                None => py_span(-1, -1),
            })
            .collect(),
    )
}

fn match_regs(start: i64, end: i64, spans: &[Option<(i64, i64)>]) -> PyObjectRef {
    let mut regs = Vec::with_capacity(spans.len() + 1);
    regs.push(py_span(start, end));
    regs.extend(spans.iter().map(|span| match span {
        Some((start, end)) => py_span(*start, *end),
        None => py_span(-1, -1),
    }));
    PyObject::tuple(regs)
}

fn match_lastindex(spans: &[Option<(i64, i64)>]) -> Option<i64> {
    let mut best: Option<(usize, i64)> = None;
    for (idx, span) in spans.iter().enumerate() {
        if let Some((_, end)) = span {
            match best {
                Some((_, best_end)) if *end < best_end => {}
                Some((_, best_end)) if *end == best_end => {}
                _ => best = Some((idx + 1, *end)),
            }
        }
    }
    best.map(|(idx, _)| idx as i64)
}

fn match_lastgroup(lastindex: Option<i64>, groupindex_map: &FxHashKeyMap) -> PyObjectRef {
    let Some(lastindex) = lastindex else {
        return PyObject::none();
    };
    for (key, value) in groupindex_map.iter() {
        if value.to_int().ok() == Some(lastindex) {
            if let HashableKey::Str(name) = key {
                return PyObject::str_val(name.to_compact_string());
            }
        }
    }
    PyObject::none()
}

fn match_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Match.__repr__ requires self"));
    }
    let self_obj = &args[0];
    let start = self_obj
        .get_attr("_start")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(0);
    let end = self_obj
        .get_attr("_end")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(0);
    let matched = self_obj
        .get_attr("_match")
        .map(|v| v.repr())
        .unwrap_or_else(|| "''".to_string());
    Ok(PyObject::str_val(CompactString::from(format!(
        "<re.Match object; span=({}, {}), match={}>",
        start, end, matched
    ))))
}

fn insert_match_methods(attrs: &mut IndexMap<CompactString, PyObjectRef>) {
    attrs.insert(
        CompactString::from("group"),
        PyObject::native_function("Match.group", match_group),
    );
    attrs.insert(
        CompactString::from("groups"),
        PyObject::native_function("Match.groups", match_groups),
    );
    attrs.insert(
        CompactString::from("groupdict"),
        PyObject::native_function("Match.groupdict", match_groupdict),
    );
    attrs.insert(
        CompactString::from("start"),
        PyObject::native_function("Match.start", match_start),
    );
    attrs.insert(
        CompactString::from("end"),
        PyObject::native_function("Match.end", match_end),
    );
    attrs.insert(
        CompactString::from("span"),
        PyObject::native_function("Match.span", match_span),
    );
    attrs.insert(
        CompactString::from("expand"),
        PyObject::native_function("Match.expand", match_expand),
    );
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_function("Match.__getitem__", match_getitem),
    );
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("Match.__repr__", match_repr),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );
}

fn make_fancy_match_object(
    text: &str,
    start: usize,
    end: usize,
    full: &str,
    groups: Vec<Option<String>>,
    group_names: FxHashKeyMap,
    is_bytes: bool,
) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_match"), py_re_text(full, is_bytes));
    let start_index = regex_offset_to_py_index(text, start, is_bytes);
    let end_index = regex_offset_to_py_index(text, end, is_bytes);
    attrs.insert(CompactString::from("_start"), PyObject::int(start_index));
    attrs.insert(CompactString::from("_end"), PyObject::int(end_index));
    attrs.insert(CompactString::from("_text"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("string"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("pos"), PyObject::int(0));
    attrs.insert(
        CompactString::from("endpos"),
        PyObject::int(regex_offset_to_py_index(text, text.len(), is_bytes)),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    let group_objs: Vec<PyObjectRef> = groups
        .iter()
        .map(|g| {
            g.as_ref()
                .map(|s| py_re_text(s, is_bytes))
                .unwrap_or(PyObject::none())
        })
        .collect();
    let group_spans: Vec<Option<(i64, i64)>> = groups
        .iter()
        .map(|group| {
            group.as_ref().and_then(|value| {
                text[start..end].find(value).map(|rel| {
                    let abs_start = start + rel;
                    let abs_end = abs_start + value.len();
                    (
                        regex_offset_to_py_index(text, abs_start, is_bytes),
                        regex_offset_to_py_index(text, abs_end, is_bytes),
                    )
                })
            })
        })
        .collect();
    let lastindex = match_lastindex(&group_spans);
    attrs.insert(CompactString::from("_groups"), PyObject::tuple(group_objs));
    attrs.insert(
        CompactString::from("_group_spans"),
        group_spans_to_py(&group_spans),
    );
    attrs.insert(
        CompactString::from("_groupindex"),
        PyObject::dict_fx(group_names.clone()),
    );
    attrs.insert(
        CompactString::from("regs"),
        match_regs(start_index, end_index, &group_spans),
    );
    attrs.insert(
        CompactString::from("lastindex"),
        lastindex.map(PyObject::int).unwrap_or_else(PyObject::none),
    );
    attrs.insert(
        CompactString::from("lastgroup"),
        match_lastgroup(lastindex, &group_names),
    );
    attrs.insert(CompactString::from("re"), PyObject::bool_val(true));
    insert_match_methods(&mut attrs);
    PyObject::module_with_attrs(CompactString::from("Match"), attrs)
}

fn make_match_object_from_captures(
    caps: &regex::Captures,
    text: &str,
    re_obj: &regex::Regex,
    is_bytes: bool,
) -> PyObjectRef {
    let whole = caps.get(0).unwrap();
    let full_match = whole.as_str().to_string();
    let start = regex_offset_to_py_index(text, whole.start(), is_bytes);
    let end = regex_offset_to_py_index(text, whole.end(), is_bytes);
    let mut groups = Vec::new();
    let mut group_spans = Vec::new();
    for i in 1..caps.len() {
        if let Some(g) = caps.get(i) {
            groups.push(py_re_text(g.as_str(), is_bytes));
            group_spans.push(Some((
                regex_offset_to_py_index(text, g.start(), is_bytes),
                regex_offset_to_py_index(text, g.end(), is_bytes),
            )));
        } else {
            groups.push(PyObject::none());
            group_spans.push(None);
        }
    }
    let groups_tuple = PyObject::tuple(groups);
    let mut groupindex_map = new_fx_hashkey_map();
    for (i, name_opt) in re_obj.capture_names().enumerate() {
        if let Some(name) = name_opt {
            groupindex_map.insert(
                HashableKey::str_key(CompactString::from(name)),
                PyObject::int(i as i64),
            );
        }
    }
    let lastindex = match_lastindex(&group_spans);
    let groupindex = PyObject::dict_fx(groupindex_map.clone());
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("_match"),
        py_re_text(&full_match, is_bytes),
    );
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(CompactString::from("_text"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("string"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("pos"), PyObject::int(0));
    attrs.insert(
        CompactString::from("endpos"),
        PyObject::int(regex_offset_to_py_index(text, text.len(), is_bytes)),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    attrs.insert(CompactString::from("_groups"), groups_tuple);
    attrs.insert(
        CompactString::from("_group_spans"),
        group_spans_to_py(&group_spans),
    );
    attrs.insert(CompactString::from("_groupindex"), groupindex);
    attrs.insert(
        CompactString::from("regs"),
        match_regs(start, end, &group_spans),
    );
    attrs.insert(
        CompactString::from("lastindex"),
        lastindex.map(PyObject::int).unwrap_or_else(PyObject::none),
    );
    attrs.insert(
        CompactString::from("lastgroup"),
        match_lastgroup(lastindex, &groupindex_map),
    );
    attrs.insert(CompactString::from("re"), PyObject::bool_val(true));
    insert_match_methods(&mut attrs);
    PyObject::module_with_attrs(CompactString::from("Match"), attrs)
}

fn make_match_object(
    m: regex::Match,
    text: &str,
    re_obj: &regex::Regex,
    is_bytes: bool,
) -> PyObjectRef {
    let full_match = m.as_str().to_string();
    let start = regex_offset_to_py_index(text, m.start(), is_bytes);
    let end = regex_offset_to_py_index(text, m.end(), is_bytes);
    // groups - store captured groups
    // Use captures_at to find the capture at this match's start position
    let captures = re_obj.captures_at(text, m.start());
    let mut groups = Vec::new();
    let mut group_spans = Vec::new();
    if let Some(caps) = &captures {
        for i in 1..caps.len() {
            if let Some(g) = caps.get(i) {
                groups.push(py_re_text(g.as_str(), is_bytes));
                group_spans.push(Some((
                    regex_offset_to_py_index(text, g.start(), is_bytes),
                    regex_offset_to_py_index(text, g.end(), is_bytes),
                )));
            } else {
                groups.push(PyObject::none());
                group_spans.push(None);
            }
        }
    }
    let groups_tuple = PyObject::tuple(groups);
    // Build name→index mapping for named capture groups
    let mut groupindex_map = new_fx_hashkey_map();
    for (i, name_opt) in re_obj.capture_names().enumerate() {
        if let Some(name) = name_opt {
            groupindex_map.insert(
                HashableKey::str_key(CompactString::from(name)),
                PyObject::int(i as i64),
            );
        }
    }
    let lastindex = match_lastindex(&group_spans);
    let groupindex = PyObject::dict_fx(groupindex_map.clone());
    // Build the match object with pre-bound data attributes
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("_match"),
        py_re_text(&full_match, is_bytes),
    );
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(CompactString::from("_text"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("string"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("pos"), PyObject::int(0));
    attrs.insert(
        CompactString::from("endpos"),
        PyObject::int(regex_offset_to_py_index(text, text.len(), is_bytes)),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    attrs.insert(CompactString::from("_groups"), groups_tuple);
    attrs.insert(
        CompactString::from("_group_spans"),
        group_spans_to_py(&group_spans),
    );
    attrs.insert(CompactString::from("_groupindex"), groupindex);
    attrs.insert(
        CompactString::from("regs"),
        match_regs(start, end, &group_spans),
    );
    attrs.insert(
        CompactString::from("lastindex"),
        lastindex.map(PyObject::int).unwrap_or_else(PyObject::none),
    );
    attrs.insert(
        CompactString::from("lastgroup"),
        match_lastgroup(lastindex, &groupindex_map),
    );
    attrs.insert(CompactString::from("re"), PyObject::bool_val(true));
    insert_match_methods(&mut attrs);
    let match_obj = PyObject::module_with_attrs(CompactString::from("Match"), attrs);
    match_obj
}

fn make_simple_match_object(
    text: &str,
    start_offset: usize,
    end_offset: usize,
    is_bytes: bool,
) -> PyObjectRef {
    let start = regex_offset_to_py_index(text, start_offset, is_bytes);
    let end = regex_offset_to_py_index(text, end_offset, is_bytes);
    let group_spans: Vec<Option<(i64, i64)>> = Vec::new();
    let groupindex_map = new_fx_hashkey_map();
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("_match"),
        py_re_text(&text[start_offset..end_offset], is_bytes),
    );
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(CompactString::from("_text"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("string"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("pos"), PyObject::int(0));
    attrs.insert(
        CompactString::from("endpos"),
        PyObject::int(regex_offset_to_py_index(text, text.len(), is_bytes)),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    attrs.insert(CompactString::from("_groups"), PyObject::tuple(Vec::new()));
    attrs.insert(
        CompactString::from("_group_spans"),
        group_spans_to_py(&group_spans),
    );
    attrs.insert(
        CompactString::from("_groupindex"),
        PyObject::dict_fx(groupindex_map.clone()),
    );
    attrs.insert(
        CompactString::from("regs"),
        match_regs(start, end, &group_spans),
    );
    attrs.insert(CompactString::from("lastindex"), PyObject::none());
    attrs.insert(CompactString::from("lastgroup"), PyObject::none());
    attrs.insert(CompactString::from("re"), PyObject::bool_val(true));
    insert_match_methods(&mut attrs);
    PyObject::module_with_attrs(CompactString::from("Match"), attrs)
}

fn attach_bytearray_source(match_obj: &PyObjectRef, source: &PyObjectRef) {
    if !matches!(source.payload, PyObjectPayload::ByteArray(_)) {
        return;
    }
    if let PyObjectPayload::Module(md) = &match_obj.payload {
        md.attrs
            .write()
            .insert(CompactString::from("_bytearray_source"), source.clone());
    }
}

fn warn_nonleading_flags(pattern: &str, pattern_obj: &PyObjectRef) -> PyResult<()> {
    let Some(pos) = pattern.find("(?i)") else {
        return Ok(());
    };
    if pos == 0 {
        return Ok(());
    }
    let display = if pattern.chars().count() > 40 {
        let prefix: String = pattern.chars().take(20).collect();
        format!(
            "Flags not at the start of the expression {} (truncated)",
            pattern_obj.repr_for_message(&prefix)
        )
    } else {
        format!(
            "Flags not at the start of the expression {}",
            pattern_obj.repr()
        )
    };
    if let Some(warnings) = crate::load_module("warnings") {
        if let (Some(warn_fn), Some(dep_cls)) = (
            warnings.get_attr("warn"),
            warnings.get_attr("DeprecationWarning"),
        ) {
            ferrython_core::object::call_callable(
                &warn_fn,
                &[PyObject::str_val(CompactString::from(display)), dep_cls],
            )?;
        }
    }
    Ok(())
}

trait ReprForWarning {
    fn repr_for_message(&self, text: &str) -> String;
}

impl ReprForWarning for PyObjectRef {
    fn repr_for_message(&self, text: &str) -> String {
        if extract_bytes_like(self).is_some() {
            PyObject::bytes(regex_text_to_bytes(text)).repr()
        } else {
            PyObject::str_val(CompactString::from(text)).repr()
        }
    }
}

fn strip_nonleading_global_flags(pattern: &str) -> (String, i64, bool) {
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = String::with_capacity(pattern.len());
    let mut flags = 0;
    let mut changed = false;
    let mut i = 0;
    while i < chars.len() {
        if i > 0 && i + 3 < chars.len() && chars[i] == '(' && chars[i + 1] == '?' {
            let mut j = i + 2;
            let mut seen = 0;
            while j < chars.len() {
                match chars[j] {
                    'i' => seen |= RE_FLAG_IGNORECASE,
                    'm' => seen |= RE_FLAG_MULTILINE,
                    's' => seen |= RE_FLAG_DOTALL,
                    'x' => seen |= RE_FLAG_VERBOSE,
                    'a' => seen |= RE_FLAG_ASCII,
                    'u' => seen |= RE_FLAG_UNICODE,
                    'L' => seen |= RE_FLAG_LOCALE,
                    ')' if seen != 0 => {
                        flags |= seen;
                        changed = true;
                        i = j + 1;
                        continue;
                    }
                    _ => break,
                }
                j += 1;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    (result, flags, changed)
}

fn match_group_count(self_obj: &PyObjectRef) -> usize {
    self_obj
        .get_attr("_groups")
        .and_then(|groups| {
            if let PyObjectPayload::Tuple(items) = &groups.payload {
                Some(items.len())
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn match_int_arg(arg: &PyObjectRef) -> PyResult<i64> {
    if let Ok(value) = arg.to_int() {
        return Ok(value);
    }
    if let Some(index_method) = arg.get_attr("__index__") {
        let value = ferrython_core::object::call_callable(&index_method, &[])?;
        return value
            .to_int()
            .map_err(|_| PyException::index_error("no such group"));
    }
    Err(PyException::index_error("no such group"))
}

fn match_group_index(self_obj: &PyObjectRef, arg: &PyObjectRef) -> PyResult<i64> {
    if let PyObjectPayload::Str(name) = &arg.payload {
        if let Some(groupindex) = self_obj.get_attr("_groupindex") {
            if let PyObjectPayload::Dict(d) = &groupindex.payload {
                let key = HashableKey::str_key(name.to_compact_string());
                if let Some(idx_obj) = d.read().get(&key).cloned() {
                    return idx_obj.to_int();
                }
            }
        }
        return Err(PyException::index_error(format!(
            "no such group: '{}'",
            name
        )));
    }
    let idx = match_int_arg(arg)?;
    let idx_usize = usize::try_from(idx).map_err(|_| PyException::index_error("no such group"))?;
    if idx_usize > match_group_count(self_obj) {
        return Err(PyException::index_error("no such group"));
    }
    Ok(idx)
}

fn match_bytearray_group(self_obj: &PyObjectRef, idx: i64) -> Option<PyObjectRef> {
    if !match_is_bytes(self_obj) {
        return None;
    }
    let source = self_obj.get_attr("_bytearray_source")?;
    let PyObjectPayload::ByteArray(bytes) = &source.payload else {
        return None;
    };
    let (start, end) = if idx == 0 {
        (
            self_obj.get_attr("_start")?.to_int().ok()?,
            self_obj.get_attr("_end")?.to_int().ok()?,
        )
    } else {
        let group_spans = self_obj.get_attr("_group_spans")?;
        let PyObjectPayload::Tuple(items) = &group_spans.payload else {
            return None;
        };
        let item = items.get((idx - 1) as usize)?;
        match &item.payload {
            PyObjectPayload::None => return Some(PyObject::none()),
            PyObjectPayload::Tuple(span) if span.len() == 2 => {
                (span[0].to_int().ok()?, span[1].to_int().ok()?)
            }
            _ => return None,
        }
    };
    let len = bytes.len();
    let start = start.max(0) as usize;
    let end = end.max(0) as usize;
    if start >= len || start >= end {
        return Some(PyObject::bytes(Vec::new()));
    }
    Some(PyObject::bytes(bytes[start..end.min(len)].to_vec()))
}

fn match_group_one(self_obj: &PyObjectRef, arg: Option<&PyObjectRef>) -> PyResult<PyObjectRef> {
    let idx = match arg {
        Some(arg) => match_group_index(self_obj, arg)?,
        None => 0,
    };
    if let Some(value) = match_bytearray_group(self_obj, idx) {
        return Ok(value);
    }
    if idx == 0 {
        return self_obj
            .get_attr("_match")
            .ok_or_else(|| PyException::index_error("no such group"));
    }
    if let Some(groups) = self_obj.get_attr("_groups") {
        if let PyObjectPayload::Tuple(items) = &groups.payload {
            let item_idx = (idx - 1) as usize;
            if item_idx < items.len() {
                return Ok(items[item_idx].clone());
            }
        }
    }
    Err(PyException::index_error("no such group"))
}

fn match_group(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("group() needs self"));
    }
    let self_obj = &args[0];
    if args.len() <= 2 {
        return match_group_one(self_obj, args.get(1));
    }
    let mut items = Vec::with_capacity(args.len() - 1);
    for arg in &args[1..] {
        items.push(match_group_one(self_obj, Some(arg))?);
    }
    Ok(PyObject::tuple(items))
}

fn match_groupdict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("groupdict() needs self"));
    }
    let self_obj = &args[0];
    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
    let mut result = IndexMap::new();
    if let Some(groupindex) = self_obj.get_attr("_groupindex") {
        if let PyObjectPayload::Dict(d) = &groupindex.payload {
            if let Some(groups) = self_obj.get_attr("_groups") {
                if let PyObjectPayload::Tuple(items) = &groups.payload {
                    for (key, idx_obj) in d.read().iter() {
                        let idx = idx_obj.to_int().unwrap_or(0);
                        let i = (idx - 1) as usize;
                        let val = if i < items.len() {
                            if matches!(items[i].payload, PyObjectPayload::None) {
                                default.clone()
                            } else {
                                items[i].clone()
                            }
                        } else {
                            default.clone()
                        };
                        result.insert(key.clone(), val);
                    }
                }
            }
        }
    }
    Ok(PyObject::dict(result))
}

fn match_groups(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("groups() needs self"));
    }
    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
    if let Some(groups) = args[0].get_attr("_groups") {
        if let PyObjectPayload::Tuple(items) = &groups.payload {
            let values: Vec<PyObjectRef> = items
                .iter()
                .map(|item| {
                    if matches!(item.payload, PyObjectPayload::None) {
                        default.clone()
                    } else {
                        item.clone()
                    }
                })
                .collect();
            return Ok(PyObject::tuple(values));
        }
    }
    Ok(PyObject::tuple(vec![]))
}

fn match_span_bounds(self_obj: &PyObjectRef, arg: Option<&PyObjectRef>) -> PyResult<(i64, i64)> {
    let idx = match arg {
        Some(arg) => match_group_index(self_obj, arg)?,
        None => 0,
    };
    if idx == 0 {
        let start = self_obj
            .get_attr("_start")
            .and_then(|v| v.to_int().ok())
            .unwrap_or(0);
        let end = self_obj
            .get_attr("_end")
            .and_then(|v| v.to_int().ok())
            .unwrap_or(0);
        return Ok((start, end));
    }
    if let Some(group_spans) = self_obj.get_attr("_group_spans") {
        if let PyObjectPayload::Tuple(items) = &group_spans.payload {
            let item_idx = (idx - 1) as usize;
            if item_idx < items.len() {
                if let PyObjectPayload::Tuple(span) = &items[item_idx].payload {
                    if span.len() == 2 {
                        return Ok((
                            span[0].to_int().unwrap_or(-1),
                            span[1].to_int().unwrap_or(-1),
                        ));
                    }
                }
            }
        }
    }
    Err(PyException::index_error("no such group"))
}

fn match_start(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("start() needs self"));
    }
    let (start, _) = match_span_bounds(&args[0], args.get(1))?;
    Ok(PyObject::int(start))
}

fn match_end(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("end() needs self"));
    }
    let (_, end) = match_span_bounds(&args[0], args.get(1))?;
    Ok(PyObject::int(end))
}

fn match_span(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("span() needs self"));
    }
    let (start, end) = match_span_bounds(&args[0], args.get(1))?;
    Ok(py_span(start, end))
}

fn match_is_bytes(self_obj: &PyObjectRef) -> bool {
    self_obj
        .get_attr("_is_bytes")
        .map(|flag| flag.is_truthy())
        .unwrap_or(false)
}

fn match_group_template_text(self_obj: &PyObjectRef, group: PyObjectRef) -> PyResult<String> {
    let value = match_group_one(self_obj, Some(&group))?;
    if matches!(value.payload, PyObjectPayload::None) {
        return Ok(String::new());
    }
    if let Some(bytes) = extract_bytes_like(&value) {
        return Ok(bytes_to_regex_text(&bytes));
    }
    if let Some(text) = extract_str_like(&value) {
        return Ok(text);
    }
    Ok(value.py_to_string())
}

fn push_octal_escape(result: &mut String, digits: &str) {
    if let Ok(value) = u32::from_str_radix(digits, 8) {
        if let Some(ch) = char::from_u32(value) {
            result.push(ch);
        }
    }
}

fn expand_match_template(self_obj: &PyObjectRef, template: &str) -> PyResult<String> {
    let chars: Vec<char> = template.chars().collect();
    let mut result = String::with_capacity(template.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' {
            result.push(chars[i]);
            i += 1;
            continue;
        }
        if i + 1 >= chars.len() {
            result.push('\\');
            break;
        }
        let next = chars[i + 1];
        match next {
            'g' if i + 2 < chars.len() && chars[i + 2] == '<' => {
                let mut j = i + 3;
                while j < chars.len() && chars[j] != '>' {
                    j += 1;
                }
                let name: String = chars[i + 3..j].iter().collect();
                let group_arg = if name.chars().all(|ch| ch.is_ascii_digit()) {
                    PyObject::int(name.parse::<i64>().unwrap_or(0))
                } else {
                    PyObject::str_val(CompactString::from(name))
                };
                result.push_str(&match_group_template_text(self_obj, group_arg)?);
                i = if j < chars.len() { j + 1 } else { j };
            }
            '0' => {
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && digits.len() < 3 && matches!(chars[j], '0'..='7') {
                    digits.push(chars[j]);
                    j += 1;
                }
                push_octal_escape(&mut result, &digits);
                i = j;
            }
            '1'..='9' => {
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && chars[j].is_ascii_digit() {
                    digits.push(chars[j]);
                    j += 1;
                }
                result.push_str(&match_group_template_text(
                    self_obj,
                    PyObject::int(digits.parse::<i64>().unwrap_or(0)),
                )?);
                i = j;
            }
            'a' => {
                result.push('\x07');
                i += 2;
            }
            'b' => {
                result.push('\x08');
                i += 2;
            }
            'f' => {
                result.push('\x0c');
                i += 2;
            }
            'n' => {
                result.push('\n');
                i += 2;
            }
            'r' => {
                result.push('\r');
                i += 2;
            }
            't' => {
                result.push('\t');
                i += 2;
            }
            'v' => {
                result.push('\x0b');
                i += 2;
            }
            '\\' => {
                result.push('\\');
                i += 2;
            }
            _ => {
                result.push(next);
                i += 2;
            }
        }
    }
    Ok(result)
}

fn match_expand(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("expand() needs self and template"));
    }
    let is_bytes = match_is_bytes(&args[0]);
    let template = extract_re_replacement(&args[1], is_bytes)?;
    let expanded = expand_match_template(&args[0], &template)?;
    Ok(py_re_text(&expanded, is_bytes))
}

/// Match.__getitem__: m[0], m[1], m['name'] — delegates to match_group
fn match_getitem(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "Match.__getitem__() requires self and index",
        ));
    }
    // Repack as [self, index] for match_group
    match_group(args)
}

// Public wrappers for match object methods (used by VM re_sub_with_callable)
pub fn match_group_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_group(args)
}
pub fn match_groups_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_groups(args)
}
pub fn match_groupdict_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_groupdict(args)
}
pub fn match_start_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_start(args)
}
pub fn match_end_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_end(args)
}
pub fn match_span_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_span(args)
}

fn needs_fancy_regex_with_flags(pattern: &str, flags: i64) -> bool {
    // Check both original and verbose-stripped pattern
    if needs_fancy_regex(pattern) {
        return true;
    }
    if flags & 64 != 0 {
        let stripped = strip_verbose(pattern);
        if needs_fancy_regex(&stripped) {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
struct SimpleDotRepeat {
    min: u64,
    max: Option<u64>,
    lazy: bool,
    exact: bool,
}

fn parse_simple_dot_repeat(pattern: &str) -> PyResult<Option<SimpleDotRepeat>> {
    let bytes = pattern.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'.' || bytes[1] != b'{' {
        return Ok(None);
    }
    let Some(close_rel) = bytes[2..].iter().position(|&byte| byte == b'}') else {
        return Ok(None);
    };
    let close = close_rel + 2;
    let lazy = matches!(bytes.get(close + 1), Some(b'?'));
    let expected_len = close + 1 + usize::from(lazy);
    if expected_len != bytes.len() {
        return Ok(None);
    }

    let body = &bytes[2..close];
    let comma = body.iter().position(|&byte| byte == b',');
    if let Some(pos) = comma {
        if body[pos + 1..].contains(&b',') {
            return Ok(None);
        }
    }

    let limit = u32::MAX as u64;
    let (min, max, exact) = match comma {
        Some(pos) => {
            let left = &body[..pos];
            let right = &body[pos + 1..];
            if left.is_empty() && right.is_empty() {
                return Ok(None);
            }
            let min = if left.is_empty() {
                0
            } else {
                let Some(value) = parse_decimal_bytes_limited(left, limit)? else {
                    return Ok(None);
                };
                value
            };
            let max = if right.is_empty() {
                None
            } else {
                let Some(value) = parse_decimal_bytes_limited(right, limit)? else {
                    return Ok(None);
                };
                Some(value)
            };
            (min, max, false)
        }
        None => {
            let Some(value) = parse_decimal_bytes_limited(body, limit)? else {
                return Ok(None);
            };
            (value, Some(value), true)
        }
    };

    if let Some(max) = max {
        if min > max {
            return Ok(None);
        }
    }

    Ok(Some(SimpleDotRepeat {
        min,
        max,
        lazy,
        exact,
    }))
}

fn dot_repeat_prefix_end(
    text: &str,
    _is_bytes: bool,
    dotall: bool,
    cap: Option<u64>,
) -> (u64, usize) {
    let mut count = 0_u64;
    let mut end_offset = 0_usize;
    for (idx, ch) in text.char_indices() {
        if matches!(cap, Some(limit) if count >= limit) {
            break;
        }
        if !dotall && ch == '\n' {
            break;
        }
        count += 1;
        end_offset = idx + ch.len_utf8();
    }
    (count, end_offset)
}

fn simple_dot_repeat_match(
    pattern: &str,
    text: &str,
    is_bytes: bool,
    flags: i64,
) -> PyResult<Option<PyObjectRef>> {
    let Some(plan) = parse_simple_dot_repeat(pattern)? else {
        return Ok(None);
    };
    let cap = if plan.exact || plan.lazy {
        Some(plan.min)
    } else {
        plan.max
    };
    let (available, end_offset) =
        dot_repeat_prefix_end(text, is_bytes, flags & RE_FLAG_DOTALL != 0, cap);
    if available < plan.min {
        return Ok(Some(PyObject::none()));
    }
    if plan.exact {
        if available < plan.min {
            return Ok(Some(PyObject::none()));
        }
    } else if let Some(max) = plan.max {
        if available > max {
            return Ok(None);
        }
    }
    Ok(Some(make_simple_match_object(
        text, 0, end_offset, is_bytes,
    )))
}

fn simple_ascii_ignorecase_literal_match(
    pattern: &str,
    text: &str,
    is_bytes: bool,
    flags: i64,
) -> Option<PyObjectRef> {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    let effective_flags = flags | inline_flags;
    if effective_flags & RE_FLAG_IGNORECASE == 0 {
        return None;
    }
    if is_bytes {
        if effective_flags & RE_FLAG_LOCALE != 0 {
            return None;
        }
    } else if effective_flags & RE_FLAG_ASCII == 0 {
        return None;
    }
    let mut chars = body.chars();
    let literal = chars.next()?;
    if chars.next().is_some() || literal.is_ascii() {
        return None;
    }
    if text.chars().next() == Some(literal) {
        Some(make_simple_match_object(
            text,
            0,
            literal.len_utf8(),
            is_bytes,
        ))
    } else {
        Some(PyObject::none())
    }
}

fn re_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.match() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if text.is_empty() && is_simple_nonboundary_pattern(&pattern) {
        return Ok(PyObject::none());
    }
    if let Some(result) =
        simple_ascii_ignorecase_literal_match(&pattern, &text, subject_is_bytes, flags)
    {
        return Ok(result);
    }
    if let Some(result) = simple_dot_repeat_match(&pattern, &text, subject_is_bytes, flags)? {
        return Ok(result);
    }
    let anchored = anchor_pattern(&pattern, "");
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&anchored, engine_flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                    .collect();
                let result = make_fancy_match_object(
                    &text,
                    whole.start(),
                    whole.end(),
                    whole.as_str(),
                    groups,
                    extract_fancy_group_names(&re),
                    subject_is_bytes,
                );
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            _ => Ok(PyObject::none()),
        }
    } else {
        let re = cached_build_regex(&anchored, engine_flags)?;
        match re.find(&text) {
            Some(m) => {
                let result = make_match_object(m, &text, &re, subject_is_bytes);
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            None => Ok(PyObject::none()),
        }
    }
}

fn re_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.search() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if text.is_empty() && is_simple_nonboundary_pattern(&pattern) {
        return Ok(PyObject::none());
    }
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                    .collect();
                let result = make_fancy_match_object(
                    &text,
                    whole.start(),
                    whole.end(),
                    whole.as_str(),
                    groups,
                    extract_fancy_group_names(&re),
                    subject_is_bytes,
                );
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            _ => Ok(PyObject::none()),
        }
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        match re.find(&text) {
            Some(m) => {
                let result = make_match_object(m, &text, &re, subject_is_bytes);
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            None => Ok(PyObject::none()),
        }
    }
}

fn re_fullmatch(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.fullmatch() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    let anchored = anchor_pattern(&pattern, r"\z");
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&anchored, engine_flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                    .collect();
                let result = make_fancy_match_object(
                    &text,
                    whole.start(),
                    whole.end(),
                    whole.as_str(),
                    groups,
                    extract_fancy_group_names(&re),
                    subject_is_bytes,
                );
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            _ => Ok(PyObject::none()),
        }
    } else {
        let re = build_regex(&anchored, engine_flags)?;
        let orig_re = build_regex(&pattern, engine_flags)?;
        match re.find(&text) {
            Some(m) => {
                let result = make_match_object(m, &text, &orig_re, subject_is_bytes);
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            None => Ok(PyObject::none()),
        }
    }
}

fn re_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.findall() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if text.is_empty() && is_simple_nonboundary_pattern(&pattern) {
        return Ok(PyObject::list(vec![]));
    }
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        // Determine capture group count from first match
        let all_caps = fancy_captures(&re, &text);
        if all_caps.is_empty() {
            return Ok(PyObject::list(vec![]));
        }
        let cap_count = all_caps[0].len() - 1;
        if cap_count == 0 {
            let results: Vec<PyObjectRef> = fancy_find_all(&re, &text)
                .into_iter()
                .map(|s| py_re_text(&s, subject_is_bytes))
                .collect();
            Ok(PyObject::list(results))
        } else if cap_count == 1 {
            let results: Vec<PyObjectRef> = all_caps
                .into_iter()
                .map(|g| {
                    g.get(1)
                        .cloned()
                        .flatten()
                        .map(|s| py_re_text(&s, subject_is_bytes))
                        .unwrap_or_else(|| py_re_text("", subject_is_bytes))
                })
                .collect();
            Ok(PyObject::list(results))
        } else {
            let results: Vec<PyObjectRef> = all_caps
                .into_iter()
                .map(|g| {
                    let items: Vec<PyObjectRef> = g[1..]
                        .iter()
                        .map(|o| {
                            o.as_ref()
                                .map(|s| py_re_text(s.as_str(), subject_is_bytes))
                                .unwrap_or_else(|| py_re_text("", subject_is_bytes))
                        })
                        .collect();
                    PyObject::tuple(items)
                })
                .collect();
            Ok(PyObject::list(results))
        }
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let cap_count = re.captures_len() - 1;
        if cap_count == 0 {
            let results: Vec<PyObjectRef> = re
                .find_iter(&text)
                .map(|m| py_re_text(m.as_str(), subject_is_bytes))
                .collect();
            Ok(PyObject::list(results))
        } else if cap_count == 1 {
            let results: Vec<PyObjectRef> = re
                .captures_iter(&text)
                .map(|caps| {
                    caps.get(1)
                        .map(|m| py_re_text(m.as_str(), subject_is_bytes))
                        .unwrap_or_else(|| py_re_text("", subject_is_bytes))
                })
                .collect();
            Ok(PyObject::list(results))
        } else {
            let results: Vec<PyObjectRef> = re
                .captures_iter(&text)
                .map(|caps| {
                    let groups: Vec<PyObjectRef> = (1..=cap_count)
                        .map(|i| {
                            caps.get(i)
                                .map(|m| py_re_text(m.as_str(), subject_is_bytes))
                                .unwrap_or_else(|| py_re_text("", subject_is_bytes))
                        })
                        .collect();
                    PyObject::tuple(groups)
                })
                .collect();
            Ok(PyObject::list(results))
        }
    }
}

fn re_finditer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.finditer() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    if matches!(args[1].payload, PyObjectPayload::ByteArray(_)) {
        register_bytearray_export(&args[1]);
    }
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if text.is_empty() && is_simple_nonboundary_pattern(&pattern) {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: Vec::new(),
                index: 0,
            }),
        ))));
    }
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        let group_names = extract_fancy_group_names(&re);
        let mut matches: Vec<PyObjectRef> = Vec::new();
        let mut pos = 0;
        while pos <= text.len() {
            match re.captures(&text[pos..]) {
                Ok(Some(caps)) => {
                    let whole = caps.get(0).unwrap();
                    if whole.start() == whole.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + whole.start();
                    let abs_end = pos + whole.end();
                    let mut groups = Vec::new();
                    for i in 1..caps.len() {
                        groups.push(caps.get(i).map(|g| g.as_str().to_string()));
                    }
                    matches.push(make_fancy_match_object(
                        &text,
                        abs_start,
                        abs_end,
                        &text[abs_start..abs_end],
                        groups,
                        group_names.clone(),
                        subject_is_bytes,
                    ));
                    pos = abs_end;
                }
                _ => break,
            }
        }
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: matches,
                index: 0,
            }),
        ))))
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let mut matches: Vec<PyObjectRef> = Vec::new();
        let mut last_span: Option<(usize, usize)> = None;
        for caps in re.captures_iter(&text) {
            let whole = caps.get(0).unwrap();
            last_span = Some((whole.start(), whole.end()));
            matches.push(make_match_object_from_captures(
                &caps,
                &text,
                &re,
                subject_is_bytes,
            ));
        }
        if matches!(
            last_span,
            Some((start, end)) if start != end && end == text.len()
        ) {
            if let Some(caps) = re.captures_at(&text, text.len()) {
                let whole = caps.get(0).unwrap();
                if whole.start() == text.len() && whole.end() == text.len() {
                    matches.push(make_match_object_from_captures(
                        &caps,
                        &text,
                        &re,
                        subject_is_bytes,
                    ));
                }
            }
        }
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: matches,
                index: 0,
            }),
        ))))
    }
}

fn dollar_match_offsets(pattern: &str, text: &str, flags: i64) -> Option<Vec<usize>> {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    if body != "$" {
        return None;
    }
    let effective_flags = flags | inline_flags;
    let mut offsets = Vec::new();
    if effective_flags & RE_FLAG_MULTILINE != 0 {
        for (idx, ch) in text.char_indices() {
            if ch == '\n' {
                offsets.push(idx);
            }
        }
        offsets.push(text.len());
    } else {
        if text.ends_with('\n') {
            offsets.push(text.len() - 1);
        }
        offsets.push(text.len());
    }
    Some(offsets)
}

fn re_sub_plain_offsets(
    text: &str,
    repl: &str,
    count: usize,
    offsets: &[usize],
    subject_is_bytes: bool,
) -> (PyObjectRef, usize) {
    let limit = if count == 0 {
        offsets.len()
    } else {
        count.min(offsets.len())
    };
    let mut result = String::with_capacity(text.len() + repl.len().saturating_mul(limit));
    let mut last = 0;
    for &offset in offsets.iter().take(limit) {
        result.push_str(&text[last..offset]);
        result.push_str(repl);
        last = offset;
    }
    result.push_str(&text[last..]);
    (py_re_text(&result, subject_is_bytes), limit)
}

fn re_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "re.sub() requires pattern, repl, and string",
        ));
    }
    let repl_obj = &args[1];
    let (text, subject_is_bytes) = extract_re_subject(&args[2])?;
    // count and flags can be positional or in trailing kwargs dict
    let mut count = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::Dict(_)) {
        args[3].to_int().unwrap_or(0) as usize
    } else {
        0
    };
    let mut flags = if args.len() > 4 && !matches!(&args[4].payload, PyObjectPayload::Dict(_)) {
        args[4].to_int().unwrap_or(0)
    } else {
        0
    };
    // Check for trailing kwargs dict
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let map_r = map.read();
            for (k, v) in map_r.iter() {
                if let HashableKey::Str(s) = k {
                    match s.as_str() {
                        "count" => count = v.to_int().unwrap_or(0) as usize,
                        "flags" => flags = v.to_int().unwrap_or(0),
                        _ => {}
                    }
                }
            }
        }
    }
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    // Check if repl is callable
    let repl_is_callable = matches!(
        &repl_obj.payload,
        PyObjectPayload::Function(_)
            | PyObjectPayload::NativeFunction(_)
            | PyObjectPayload::NativeClosure(_)
            | PyObjectPayload::BoundMethod { .. }
    );
    if repl_is_callable {
        return re_sub_callable(
            &pattern,
            repl_obj,
            &text,
            count,
            engine_flags,
            subject_is_bytes,
        );
    }
    let repl = extract_re_replacement(repl_obj, subject_is_bytes)?;
    validate_replacement_for_pattern(&args[0], flags, &repl)?;
    if !repl.contains('\\') {
        if let Some(offsets) = dollar_match_offsets(&pattern, &text, flags) {
            return Ok(re_sub_plain_offsets(&text, &repl, count, &offsets, subject_is_bytes).0);
        }
    }
    let rust_repl = python_repl_to_rust(&repl);
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if count > 0 && n >= count {
                break;
            }
            match re.find(&text[pos..]) {
                Ok(Some(m)) => {
                    if m.start() == m.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + m.start();
                    let abs_end = pos + m.end();
                    result.push_str(&text[last..abs_start]);
                    result.push_str(&rust_repl);
                    last = abs_end;
                    pos = abs_end;
                    n += 1;
                }
                _ => break,
            }
        }
        result.push_str(&text[last..]);
        Ok(py_re_text(&result, subject_is_bytes))
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let result = if count == 0 {
            re.replace_all(&text, rust_repl.as_str()).to_string()
        } else {
            re.replacen(&text, count, rust_repl.as_str()).to_string()
        };
        Ok(py_re_text(&result, subject_is_bytes))
    }
}

/// re.sub with a callable replacement function
fn re_sub_callable(
    pattern: &str,
    repl_fn: &PyObjectRef,
    text: &str,
    count: usize,
    flags: i64,
    is_bytes: bool,
) -> PyResult<PyObjectRef> {
    if needs_fancy_regex_with_flags(pattern, flags) {
        let re = build_fancy_regex(pattern, flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if count > 0 && n >= count {
                break;
            }
            match re.captures(&text[pos..]) {
                Ok(Some(caps)) => {
                    let whole = caps.get(0).unwrap();
                    if whole.start() == whole.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + whole.start();
                    let abs_end = pos + whole.end();
                    result.push_str(&text[last..abs_start]);
                    let groups: Vec<Option<String>> = (1..caps.len())
                        .map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                        .collect();
                    let match_obj = make_fancy_match_object(
                        text,
                        abs_start,
                        abs_end,
                        whole.as_str(),
                        groups,
                        extract_fancy_group_names(&re),
                        is_bytes,
                    );
                    let replacement = ferrython_core::object::call_callable(repl_fn, &[match_obj])?;
                    result.push_str(&extract_re_replacement(&replacement, is_bytes)?);
                    last = abs_end;
                    pos = abs_end;
                    n += 1;
                }
                _ => break,
            }
        }
        result.push_str(&text[last..]);
        Ok(py_re_text(&result, is_bytes))
    } else {
        let re = build_regex(pattern, flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        for caps in re.captures_iter(text) {
            if count > 0 && n >= count {
                break;
            }
            let whole = caps.get(0).unwrap();
            result.push_str(&text[last..whole.start()]);
            let match_obj = make_match_object_from_captures(&caps, text, &re, is_bytes);
            let replacement = ferrython_core::object::call_callable(repl_fn, &[match_obj])?;
            result.push_str(&extract_re_replacement(&replacement, is_bytes)?);
            last = whole.end();
            n += 1;
        }
        result.push_str(&text[last..]);
        Ok(py_re_text(&result, is_bytes))
    }
}

fn re_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "re.subn() requires pattern, repl, and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[2])?;
    let mut count = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::Dict(_)) {
        args[3].to_int().unwrap_or(0) as usize
    } else {
        0
    };
    let mut flags = if args.len() > 4 && !matches!(&args[4].payload, PyObjectPayload::Dict(_)) {
        args[4].to_int().unwrap_or(0)
    } else {
        0
    };
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let map_r = map.read();
            for (k, v) in map_r.iter() {
                if let HashableKey::Str(s) = k {
                    match s.as_str() {
                        "count" => count = v.to_int().unwrap_or(0) as usize,
                        "flags" => flags = v.to_int().unwrap_or(0),
                        _ => {}
                    }
                }
            }
        }
    }
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    let repl = extract_re_replacement(&args[1], subject_is_bytes)?;
    validate_replacement_for_pattern(&args[0], flags, &repl)?;
    if !repl.contains('\\') {
        if let Some(offsets) = dollar_match_offsets(&pattern, &text, flags) {
            let (result, replacements) =
                re_sub_plain_offsets(&text, &repl, count, &offsets, subject_is_bytes);
            return Ok(PyObject::tuple(vec![
                result,
                PyObject::int(replacements as i64),
            ]));
        }
    }
    let rust_repl = python_repl_to_rust(&repl);
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if count > 0 && n >= count {
                break;
            }
            match re.find(&text[pos..]) {
                Ok(Some(m)) => {
                    if m.start() == m.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + m.start();
                    let abs_end = pos + m.end();
                    result.push_str(&text[last..abs_start]);
                    result.push_str(&rust_repl);
                    last = abs_end;
                    pos = abs_end;
                    n += 1;
                }
                _ => break,
            }
        }
        result.push_str(&text[last..]);
        Ok(PyObject::tuple(vec![
            py_re_text(&result, subject_is_bytes),
            PyObject::int(n as i64),
        ]))
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let found = re.find_iter(&text).count();
        let replacements = if count == 0 { found } else { found.min(count) };
        let result = if count == 0 {
            re.replace_all(&text, rust_repl.as_str()).to_string()
        } else {
            re.replacen(&text, count, rust_repl.as_str()).to_string()
        };
        Ok(PyObject::tuple(vec![
            py_re_text(&result, subject_is_bytes),
            PyObject::int(replacements as i64),
        ]))
    }
}

fn re_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.split() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let maxsplit = if args.len() > 2 {
        args[2].to_int().unwrap_or(0) as usize
    } else {
        0
    };
    let supplied_flags = if args.len() > 3 {
        args[3].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        let mut result = Vec::new();
        let mut last = 0;
        let mut splits = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if maxsplit > 0 && splits >= maxsplit {
                break;
            }
            match re.find(&text[pos..]) {
                Ok(Some(m)) => {
                    if m.start() == m.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + m.start();
                    let abs_end = pos + m.end();
                    result.push(py_re_text(&text[last..abs_start], subject_is_bytes));
                    last = abs_end;
                    pos = abs_end;
                    splits += 1;
                }
                _ => break,
            }
        }
        result.push(py_re_text(&text[last..], subject_is_bytes));
        Ok(PyObject::list(result))
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let num_groups = re.captures_len() - 1;

        let parts: Vec<PyObjectRef> = if num_groups == 0 {
            // No capturing groups: use simple split
            if maxsplit == 0 {
                re.split(&text)
                    .map(|s| py_re_text(s, subject_is_bytes))
                    .collect()
            } else {
                re.splitn(&text, maxsplit + 1)
                    .map(|s| py_re_text(s, subject_is_bytes))
                    .collect()
            }
        } else {
            // Capturing groups: include captured text in result (CPython behavior)
            let mut result = Vec::new();
            let mut last = 0;
            let mut splits = 0;
            for caps in re.captures_iter(&text) {
                if maxsplit > 0 && splits >= maxsplit {
                    break;
                }
                let whole = caps.get(0).unwrap();
                // Text before the match
                result.push(py_re_text(&text[last..whole.start()], subject_is_bytes));
                // Each capturing group
                for i in 1..=num_groups {
                    match caps.get(i) {
                        Some(m) => result.push(py_re_text(m.as_str(), subject_is_bytes)),
                        None => result.push(PyObject::none()),
                    }
                }
                last = whole.end();
                splits += 1;
            }
            // Remaining text after last match
            result.push(py_re_text(&text[last..], subject_is_bytes));
            result
        };
        Ok(PyObject::list(parts))
    }
}

fn re_compile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("re.compile() requires a pattern"));
    }
    let supplied_flags = if args.len() > 1 {
        args[1].to_int().unwrap_or(0)
    } else {
        0
    };
    if is_re_pattern_object(&args[0]) {
        if supplied_flags != 0 {
            return Err(PyException::value_error(
                "cannot process flags argument with a compiled pattern",
            ));
        }
        return Ok(args[0].clone());
    }
    let original_pattern = extract_re_pattern(&args[0])?;
    let is_bytes = re_pattern_is_bytes(&args[0]);
    let original_pattern_obj = if is_bytes {
        PyObject::bytes(
            extract_bytes_like(&args[0]).unwrap_or_else(|| regex_text_to_bytes(&original_pattern)),
        )
    } else {
        PyObject::str_val(CompactString::from(original_pattern.clone()))
    };
    warn_nonleading_flags(&original_pattern, &original_pattern_obj)?;
    let (pattern, extra_flags, _) = strip_nonleading_global_flags(&original_pattern);
    let supplied_flags = supplied_flags | extra_flags;
    let pattern_obj = if is_bytes {
        PyObject::bytes(
            extract_bytes_like(&args[0]).unwrap_or_else(|| regex_text_to_bytes(&pattern)),
        )
    } else {
        PyObject::str_val(CompactString::from(pattern.clone()))
    };
    let inline_flags = leading_inline_flags(&pattern);
    let inline_ascii = inline_flags & RE_FLAG_ASCII != 0;
    let inline_unicode = inline_flags & RE_FLAG_UNICODE != 0;
    let inline_locale = inline_flags & RE_FLAG_LOCALE != 0;
    if (inline_ascii && inline_unicode)
        || (inline_ascii && inline_locale)
        || (inline_unicode && inline_locale)
    {
        return Err(re_pattern_error("bad inline flags", Some(2), &pattern_obj));
    }
    if is_bytes && supplied_flags & RE_FLAG_UNICODE != 0 {
        return Err(PyException::value_error(
            "cannot use UNICODE flag with a bytes pattern",
        ));
    }
    if is_bytes && inline_unicode {
        return Err(re_pattern_error("bad inline flags", Some(2), &pattern_obj));
    }
    if is_bytes
        && (supplied_flags | inline_flags) & RE_FLAG_LOCALE != 0
        && (supplied_flags | inline_flags) & RE_FLAG_ASCII != 0
    {
        return Err(PyException::value_error(
            "ASCII and LOCALE flags are incompatible",
        ));
    }
    if !is_bytes && supplied_flags & RE_FLAG_LOCALE != 0 {
        return Err(PyException::value_error(
            "cannot use LOCALE flag with a str pattern",
        ));
    }
    if !is_bytes && inline_locale {
        return Err(re_pattern_error("bad inline flags", Some(2), &pattern_obj));
    }
    if !is_bytes && supplied_flags & RE_FLAG_ASCII != 0 && supplied_flags & RE_FLAG_UNICODE != 0 {
        return Err(PyException::value_error(
            "ASCII and UNICODE flags are incompatible",
        ));
    }
    if !is_bytes
        && ((supplied_flags & RE_FLAG_ASCII != 0 && inline_unicode)
            || (supplied_flags & RE_FLAG_UNICODE != 0 && inline_ascii))
    {
        return Err(PyException::value_error(
            "ASCII and UNICODE flags are incompatible",
        ));
    }
    let flags = effective_re_flags(&pattern, supplied_flags, is_bytes);
    let engine_flags = regex_engine_flags(flags, is_bytes);
    validate_re_pattern_syntax(&pattern, is_bytes, &pattern_obj)?;
    if flags & 128 != 0 {
        write_re_debug_output(&pattern)?;
    }
    let simple_dot_repeat =
        parse_simple_dot_repeat(split_leading_inline_flags(&pattern).0)?.is_some();
    // Validate the pattern compiles (try fancy if needed)
    let compile_result = if simple_dot_repeat {
        Ok(())
    } else if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        build_fancy_regex(&pattern, engine_flags).map(|_| ())
    } else {
        build_regex(&pattern, engine_flags).map(|_| ())
    };
    if let Err(exc) = compile_result {
        if matches!(exc.kind, ExceptionKind::RuntimeError) {
            let msg = exc
                .message
                .strip_prefix("re: ")
                .unwrap_or(exc.message.as_str())
                .to_string();
            return Err(re_pattern_error(msg, None, &pattern_obj));
        }
        return Err(exc);
    }
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__re_pattern__"),
        PyObject::bool_val(true),
    );
    attrs.insert(CompactString::from("pattern"), pattern_obj);
    attrs.insert(
        CompactString::from("_pattern_text"),
        PyObject::str_val(CompactString::from(pattern.clone())),
    );
    attrs.insert(
        CompactString::from("_pattern_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    attrs.insert(CompactString::from("flags"), PyObject::int(flags));
    // groups/groupindex: best-effort for standard regex
    if simple_dot_repeat {
        attrs.insert(
            CompactString::from("groupindex"),
            readonly_mapping(new_fx_hashkey_map()),
        );
        attrs.insert(CompactString::from("groups"), PyObject::int(0));
    } else if !needs_fancy_regex_with_flags(&pattern, engine_flags) {
        if let Ok(re_obj) = build_regex(&pattern, engine_flags) {
            let group_count = re_obj.captures_len() - 1;
            let mut groupindex_map = new_fx_hashkey_map();
            for name in re_obj.capture_names().flatten() {
                if let Some(idx) = re_obj
                    .capture_names()
                    .enumerate()
                    .find(|(_, n)| n.as_deref() == Some(name))
                    .map(|(i, _)| i)
                {
                    groupindex_map.insert(
                        HashableKey::str_key(CompactString::from(name)),
                        PyObject::int(idx as i64),
                    );
                }
            }
            attrs.insert(
                CompactString::from("groupindex"),
                readonly_mapping(groupindex_map),
            );
            attrs.insert(
                CompactString::from("groups"),
                PyObject::int(group_count as i64),
            );
        }
    } else {
        attrs.insert(
            CompactString::from("groupindex"),
            readonly_mapping(new_fx_hashkey_map()),
        );
        attrs.insert(CompactString::from("groups"), PyObject::int(0));
    }
    Ok(PyObject::instance_with_attrs(re_pattern_class(), attrs))
}

fn trailing_kwarg(args: &[PyObjectRef], name: &str) -> Option<PyObjectRef> {
    let last = args.last()?;
    if let PyObjectPayload::Dict(map) = &last.payload {
        let key = HashableKey::str_key(CompactString::from(name));
        return map.read().get(&key).cloned();
    }
    None
}

fn positional_arg(args: &[PyObjectRef], index: usize) -> Option<PyObjectRef> {
    let positional_len = if args
        .last()
        .map(|last| matches!(&last.payload, PyObjectPayload::Dict(_)))
        .unwrap_or(false)
    {
        args.len().saturating_sub(1)
    } else {
        args.len()
    };
    if index < positional_len {
        Some(args[index].clone())
    } else {
        None
    }
}

fn method_arg(args: &[PyObjectRef], index: usize, name: &str) -> Option<PyObjectRef> {
    positional_arg(args, index).or_else(|| trailing_kwarg(args, name))
}

fn method_int_arg(args: &[PyObjectRef], index: usize, name: &str, default: i64) -> i64 {
    method_arg(args, index, name)
        .and_then(|obj| obj.to_int().ok())
        .unwrap_or(default)
}

fn normalize_re_bound(value: i64, len: usize) -> usize {
    if value <= 0 {
        0
    } else {
        (value as usize).min(len)
    }
}

struct PatternWindow {
    pattern: PyObjectRef,
    string_obj: PyObjectRef,
    text: String,
    subject_is_bytes: bool,
    pos: usize,
    endpos: usize,
    pos_offset: usize,
    endpos_offset: usize,
}

fn pattern_window_args(args: &[PyObjectRef], method: &str) -> PyResult<PatternWindow> {
    if args.is_empty() {
        return Err(PyException::type_error(format!(
            "Pattern.{}() requires self",
            method
        )));
    }
    let pattern = args[0].clone();
    let string_obj = method_arg(args, 1, "string").ok_or_else(|| {
        PyException::type_error(format!("Pattern.{}() requires self and string", method))
    })?;
    let (text, subject_is_bytes) = extract_re_subject(&string_obj)?;
    ensure_re_compatible(&pattern, subject_is_bytes)?;
    let subject_len = regex_offset_to_py_index(&text, text.len(), subject_is_bytes) as usize;
    let pos = normalize_re_bound(method_int_arg(args, 2, "pos", 0), subject_len);
    let endpos = normalize_re_bound(
        method_int_arg(args, 3, "endpos", subject_len as i64),
        subject_len,
    );
    let pos_offset = py_index_to_regex_offset(&text, pos);
    let endpos_offset = py_index_to_regex_offset(&text, endpos);
    Ok(PatternWindow {
        pattern,
        string_obj,
        text,
        subject_is_bytes,
        pos,
        endpos,
        pos_offset,
        endpos_offset,
    })
}

fn window_slice(text: &str, pos: usize, endpos: usize) -> &str {
    if pos > endpos {
        ""
    } else {
        &text[pos.min(text.len())..endpos.min(text.len())]
    }
}

fn offset_match_result(
    result: &PyObjectRef,
    text: &str,
    subject_is_bytes: bool,
    pos: usize,
    endpos: usize,
) {
    if matches!(result.payload, PyObjectPayload::None) {
        return;
    }
    let PyObjectPayload::Module(md) = &result.payload else {
        return;
    };
    let offset = pos as i64;
    let mut attrs = md.attrs.write();
    let start = attrs
        .get("_start")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(0)
        + offset;
    let end = attrs.get("_end").and_then(|v| v.to_int().ok()).unwrap_or(0) + offset;
    let adjusted_spans = attrs.get("_group_spans").and_then(|spans_obj| {
        if let PyObjectPayload::Tuple(items) = &spans_obj.payload {
            Some(
                items
                    .iter()
                    .map(|item| {
                        if let PyObjectPayload::Tuple(pair) = &item.payload {
                            if pair.len() == 2 {
                                let start = pair[0].to_int().unwrap_or(-1);
                                let end = pair[1].to_int().unwrap_or(-1);
                                if start >= 0 && end >= 0 {
                                    return Some((start + offset, end + offset));
                                }
                            }
                        }
                        None
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        }
    });
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(
        CompactString::from("_text"),
        py_re_text(text, subject_is_bytes),
    );
    attrs.insert(
        CompactString::from("string"),
        py_re_text(text, subject_is_bytes),
    );
    attrs.insert(CompactString::from("pos"), PyObject::int(pos as i64));
    attrs.insert(CompactString::from("endpos"), PyObject::int(endpos as i64));
    if let Some(spans) = adjusted_spans {
        attrs.insert(
            CompactString::from("_group_spans"),
            group_spans_to_py(&spans),
        );
        attrs.insert(CompactString::from("regs"), match_regs(start, end, &spans));
    }
}

fn make_re_scanner(window: PatternWindow) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("pattern"), window.pattern.clone());
    attrs.insert(CompactString::from("_pattern"), window.pattern);
    attrs.insert(CompactString::from("string"), window.string_obj);
    attrs.insert(
        CompactString::from("_text"),
        py_re_text(&window.text, window.subject_is_bytes),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(window.subject_is_bytes),
    );
    attrs.insert(
        CompactString::from("_pos"),
        PyObject::int(window.pos as i64),
    );
    attrs.insert(
        CompactString::from("_endpos"),
        PyObject::int(window.endpos as i64),
    );
    PyObject::instance_with_attrs(re_scanner_class(), attrs)
}

fn scanner_set_pos(scanner: &PyObjectRef, pos: i64) {
    if let PyObjectPayload::Instance(inst) = &scanner.payload {
        inst.attrs
            .write()
            .insert(CompactString::from("_pos"), PyObject::int(pos));
    }
}

fn scanner_next(args: &[PyObjectRef], search: bool) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("scanner method requires self"));
    }
    let scanner = &args[0];
    let pattern = scanner
        .get_attr("_pattern")
        .ok_or_else(|| PyException::attribute_error("pattern"))?;
    let string_obj = scanner
        .get_attr("string")
        .ok_or_else(|| PyException::attribute_error("string"))?;
    let pos = scanner
        .get_attr("_pos")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(0);
    let endpos = scanner
        .get_attr("_endpos")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(pos);
    let call_args = vec![
        pattern,
        string_obj,
        PyObject::int(pos),
        PyObject::int(endpos),
    ];
    let result = if search {
        compiled_search(&call_args)?
    } else {
        compiled_match(&call_args)?
    };
    if matches!(result.payload, PyObjectPayload::None) {
        scanner_set_pos(scanner, endpos);
    } else {
        let end = result
            .get_attr("_end")
            .and_then(|v| v.to_int().ok())
            .unwrap_or(pos);
        scanner_set_pos(scanner, if end <= pos { pos + 1 } else { end });
    }
    Ok(result)
}

fn scanner_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    scanner_next(args, false)
}

fn scanner_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    scanner_next(args, true)
}

fn re_scanner_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Scanner() requires a lexicon"));
    }
    let lexicon = args[0].clone();
    let mut parts = Vec::new();
    if let PyObjectPayload::List(items) = &lexicon.payload {
        for item in items.read().iter() {
            if let PyObjectPayload::Tuple(pair) = &item.payload {
                if let Some(pattern_obj) = pair.first() {
                    if let Ok(pattern) = extract_re_pattern(pattern_obj) {
                        parts.push(format!("(?:{})", pattern));
                    }
                }
            }
        }
    }
    let combined = if parts.is_empty() {
        String::from("(?!)")
    } else {
        parts.join("|")
    };
    let pattern_obj = re_compile(&[PyObject::str_val(CompactString::from(combined))])?;
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_lexicon"), lexicon);
    attrs.insert(CompactString::from("scanner"), pattern_obj);
    attrs.insert(
        CompactString::from("scan"),
        PyObject::native_function("Scanner.scan", re_scanner_scan),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );
    Ok(PyObject::module_with_attrs(
        CompactString::from("Scanner"),
        attrs,
    ))
}

fn re_scanner_scan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("scan() requires self and string"));
    }
    let scanner = &args[0];
    let lexicon = scanner
        .get_attr("_lexicon")
        .ok_or_else(|| PyException::attribute_error("_lexicon"))?;
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let mut results = Vec::new();
    let mut pos = 0usize;
    while pos < text.len() {
        let mut matched = false;
        if let PyObjectPayload::List(items) = &lexicon.payload {
            for item in items.read().iter() {
                let PyObjectPayload::Tuple(pair) = &item.payload else {
                    continue;
                };
                if pair.len() < 2 {
                    continue;
                }
                let tail = py_re_text(&text[pos..], subject_is_bytes);
                let match_obj = re_match(&[pair[0].clone(), tail])?;
                if matches!(match_obj.payload, PyObjectPayload::None) {
                    continue;
                }
                let token = match_obj
                    .get_attr("_match")
                    .unwrap_or_else(|| py_re_text("", subject_is_bytes));
                let end = match_obj
                    .get_attr("_end")
                    .and_then(|v| v.to_int().ok())
                    .unwrap_or(0);
                if end <= 0 {
                    return Err(PyException::runtime_error(
                        "scanner pattern matched empty text",
                    ));
                }
                if !matches!(pair[1].payload, PyObjectPayload::None) {
                    let value =
                        ferrython_core::object::call_callable(&pair[1], &[scanner.clone(), token])?;
                    if !matches!(value.payload, PyObjectPayload::None) {
                        results.push(value);
                    }
                }
                pos += end as usize;
                matched = true;
                break;
            }
        }
        if !matched {
            break;
        }
    }
    Ok(PyObject::tuple(vec![
        PyObject::list(results),
        py_re_text(&text[pos..], subject_is_bytes),
    ]))
}

fn compiled_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "match")?;
    if window.pos > window.endpos {
        return Ok(PyObject::none());
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    let result = re_match(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])?;
    offset_match_result(
        &result,
        &window.text,
        window.subject_is_bytes,
        window.pos,
        window.endpos,
    );
    Ok(result)
}

fn compiled_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "search")?;
    if window.pos > window.endpos {
        return Ok(PyObject::none());
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    let result = re_search(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])?;
    offset_match_result(
        &result,
        &window.text,
        window.subject_is_bytes,
        window.pos,
        window.endpos,
    );
    Ok(result)
}

fn compiled_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "findall")?;
    if window.pos > window.endpos {
        return Ok(PyObject::list(vec![]));
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    re_findall(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])
}

fn compiled_finditer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "finditer")?;
    if window.pos > window.endpos {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: Vec::new(),
                index: 0,
            }),
        ))));
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    re_finditer(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])
}

fn compiled_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "Pattern.sub() requires self, repl, and string",
        ));
    }
    let self_obj = &args[0];
    let count = if args.len() > 3 {
        args[3].clone()
    } else {
        PyObject::int(0)
    };
    re_sub(&[self_obj.clone(), args[1].clone(), args[2].clone(), count])
}

fn compiled_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Pattern.split() requires self"));
    }
    let self_obj = &args[0];
    let string_obj = method_arg(args, 1, "string")
        .ok_or_else(|| PyException::type_error("Pattern.split() requires self and string"))?;
    let maxsplit = method_arg(args, 2, "maxsplit").unwrap_or_else(|| PyObject::int(0));
    re_split(&[self_obj.clone(), string_obj, maxsplit])
}

fn compiled_fullmatch(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "fullmatch")?;
    if window.pos > window.endpos {
        return Ok(PyObject::none());
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    let result = re_fullmatch(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])?;
    offset_match_result(
        &result,
        &window.text,
        window.subject_is_bytes,
        window.pos,
        window.endpos,
    );
    Ok(result)
}

fn compiled_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "Pattern.subn() requires self, repl, and string",
        ));
    }
    let self_obj = &args[0];
    let count = if args.len() > 3 {
        args[3].clone()
    } else {
        PyObject::int(0)
    };
    re_subn(&[self_obj.clone(), args[1].clone(), args[2].clone(), count])
}

fn compiled_scanner(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(make_re_scanner(pattern_window_args(args, "scanner")?))
}

fn re_escape_needs_backslash(ch: char) -> bool {
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

fn re_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
