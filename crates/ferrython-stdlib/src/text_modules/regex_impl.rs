use super::*;

const RE_FLAG_TEMPLATE: i64 = 1;
const RE_FLAG_IGNORECASE: i64 = 2;
const RE_FLAG_LOCALE: i64 = 4;
const RE_FLAG_MULTILINE: i64 = 8;
const RE_FLAG_DOTALL: i64 = 16;
const RE_FLAG_UNICODE: i64 = 32;
const RE_FLAG_VERBOSE: i64 = 64;
const RE_FLAG_ASCII: i64 = 256;

mod classes;
mod compiled;
mod functions;
mod pattern;
mod sre;
use classes::*;
mod match_object;
use compiled::*;
use functions::*;
use match_object::*;
pub use match_object::{
    match_end_fn, match_group_fn, match_groupdict_fn, match_groups_fn, match_span_fn,
    match_start_fn,
};
use pattern::*;
pub use sre::create_sre_module;
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
