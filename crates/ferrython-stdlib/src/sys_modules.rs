//! System, OS, and platform stdlib modules

use compact_str::CompactString;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;

static RECURSION_LIMIT: AtomicI64 = AtomicI64::new(1000);

// Thread-local active exception info for sys.exc_info().
// Set by the VM when entering an except block, cleared when leaving.
thread_local! {
    static ACTIVE_EXC_INFO: std::cell::RefCell<Option<(ExceptionKind, String, Option<PyObjectRef>)>> =
        const { std::cell::RefCell::new(None) };
    static TRACE_FUNC: std::cell::RefCell<Option<PyObjectRef>> =
        const { std::cell::RefCell::new(None) };
    static PROFILE_FUNC: std::cell::RefCell<Option<PyObjectRef>> =
        const { std::cell::RefCell::new(None) };
    static EXCEPT_HOOK: std::cell::RefCell<Option<PyObjectRef>> =
        const { std::cell::RefCell::new(None) };
}

/// Called by VM when entering an except handler.
pub fn set_exc_info(kind: ExceptionKind, msg: String, obj: Option<PyObjectRef>) {
    ACTIVE_EXC_INFO.with(|c| *c.borrow_mut() = Some((kind, msg, obj)));
}

/// Called by VM when leaving an except handler.
pub fn clear_exc_info() {
    ACTIVE_EXC_INFO.with(|c| *c.borrow_mut() = None);
}

/// Read active exception info for traceback.format_exc() etc.
pub fn get_exc_info() -> Option<(ExceptionKind, String)> {
    ACTIVE_EXC_INFO.with(|c| {
        c.borrow().as_ref().map(|(k, m, _)| (k.clone(), m.clone()))
    })
}

/// Get the current trace function (for VM hook dispatch).
pub fn get_trace_func() -> Option<PyObjectRef> {
    TRACE_FUNC.with(|c| c.borrow().clone())
}

/// Set the trace function (called by sys.settrace).
pub fn set_trace_func(func: Option<PyObjectRef>) {
    TRACE_FUNC.with(|c| *c.borrow_mut() = func);
}

/// Get the current profile function (for VM hook dispatch).
pub fn get_profile_func() -> Option<PyObjectRef> {
    PROFILE_FUNC.with(|c| c.borrow().clone())
}

/// Set the profile function (called by sys.setprofile).
pub fn set_profile_func(func: Option<PyObjectRef>) {
    PROFILE_FUNC.with(|c| *c.borrow_mut() = func);
}

/// Get the custom excepthook (for unhandled exception display).
pub fn get_excepthook() -> Option<PyObjectRef> {
    EXCEPT_HOOK.with(|c| c.borrow().clone())
}

/// Set the custom excepthook.
pub fn set_excepthook(func: Option<PyObjectRef>) {
    EXCEPT_HOOK.with(|c| *c.borrow_mut() = func);
}

/// Get the current recursion limit (for VM stack depth checking).
pub fn get_recursion_limit() -> i64 {
    RECURSION_LIMIT.load(Ordering::Relaxed)
}

pub fn create_sys_module() -> PyObjectRef {
    make_module("sys", vec![
        ("version", PyObject::str_val(CompactString::from("3.8.0 (ferrython)"))),
        ("version_info", PyObject::tuple(vec![
            PyObject::int(3), PyObject::int(8), PyObject::int(0),
            PyObject::str_val(CompactString::from("final")), PyObject::int(0),
        ])),
        ("platform", PyObject::str_val(CompactString::from(std::env::consts::OS))),
        ("executable", PyObject::str_val(CompactString::from("ferrython"))),
        ("argv", PyObject::list(vec![PyObject::str_val(CompactString::from(""))])),
        ("path", {
            // Build sys.path from PYTHONPATH env + cwd
            let mut path_items: Vec<PyObjectRef> = Vec::new();
            path_items.push(PyObject::str_val(CompactString::from("")));
            if let Ok(pypath) = std::env::var("PYTHONPATH") {
                for p in std::env::split_paths(&pypath) {
                    path_items.push(PyObject::str_val(
                        CompactString::from(p.to_string_lossy().as_ref()),
                    ));
                }
            }
            path_items.push(PyObject::str_val(CompactString::from(".")));
            PyObject::list(path_items)
        }),
        ("modules", PyObject::dict_from_pairs(vec![
            (PyObject::str_val(CompactString::from("sys")), PyObject::str_val(CompactString::from("<module 'sys' (built-in)>"))),
            (PyObject::str_val(CompactString::from("os")), PyObject::str_val(CompactString::from("<module 'os' (built-in)>"))),
            (PyObject::str_val(CompactString::from("builtins")), PyObject::str_val(CompactString::from("<module 'builtins' (built-in)>"))),
        ])),
        ("maxsize", PyObject::int(i64::MAX)),
        ("maxunicode", PyObject::int(0x10FFFF)),
        ("byteorder", PyObject::str_val(CompactString::from(if cfg!(target_endian = "little") { "little" } else { "big" }))),
        ("prefix", PyObject::str_val(CompactString::from("/usr/local"))),
        ("exec_prefix", PyObject::str_val(CompactString::from("/usr/local"))),
        ("implementation", PyObject::str_val(CompactString::from("ferrython"))),
        ("stdin", make_stdio_object("<stdin>", "r", 0)),
        ("stdout", make_stdio_object("<stdout>", "w", 1)),
        ("stderr", make_stdio_object("<stderr>", "w", 2)),
        ("__stdin__", make_stdio_object("<stdin>", "r", 0)),
        ("__stdout__", make_stdio_object("<stdout>", "w", 1)),
        ("__stderr__", make_stdio_object("<stderr>", "w", 2)),
        ("getrecursionlimit", make_builtin(sys_getrecursionlimit)),
        ("setrecursionlimit", make_builtin(sys_setrecursionlimit)),
        ("exit", make_builtin(sys_exit)),
        ("getsizeof", make_builtin(sys_getsizeof)),
        ("getrefcount", make_builtin(|args| {
            check_args("sys.getrefcount", args, 1)?;
            // Return Arc strong_count + 1 (for the arg reference itself, matching CPython)
            let count = std::sync::Arc::strong_count(&args[0]) as i64;
            Ok(PyObject::int(count + 1))
        })),
        ("settrace", make_builtin(sys_settrace)),
        ("gettrace", make_builtin(sys_gettrace)),
        ("setprofile", make_builtin(sys_setprofile)),
        ("getprofile", make_builtin(sys_getprofile)),
        ("excepthook", make_builtin(sys_excepthook_default)),
        ("__excepthook__", make_builtin(sys_excepthook_default)),
        ("getdefaultencoding", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("utf-8"))))),
        ("getfilesystemencoding", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("utf-8"))))),
        ("intern", make_builtin(|args| { check_args("sys.intern", args, 1)?; Ok(args[0].clone()) })),
        ("flags", PyObject::tuple(vec![
            PyObject::int(0), // debug
            PyObject::int(0), // inspect
            PyObject::int(0), // interactive
            PyObject::int(0), // optimize
            PyObject::int(0), // dont_write_bytecode
            PyObject::int(0), // no_user_site
            PyObject::int(0), // no_site
            PyObject::int(0), // ignore_environment
            PyObject::int(0), // verbose
            PyObject::int(0), // bytes_warning
            PyObject::int(0), // quiet
            PyObject::int(0), // hash_randomization
            PyObject::int(0), // isolated
            PyObject::bool_val(false), // dev_mode
            PyObject::int(0), // utf8_mode
        ])),
        ("float_info", PyObject::tuple(vec![
            PyObject::float(f64::MAX),       // max
            PyObject::int(308),               // max_exp
            PyObject::float(f64::MIN_POSITIVE), // min
            PyObject::int(-307),              // min_exp
            PyObject::int(15),                // dig
            PyObject::int(53),                // mant_dig
            PyObject::float(f64::EPSILON),    // epsilon
            PyObject::int(2),                 // radix
            PyObject::int(1024),              // max_10_exp
            PyObject::int(-1021),             // min_10_exp
        ])),
        ("int_info", PyObject::tuple(vec![
            PyObject::int(30),  // bits_per_digit
            PyObject::int(4),   // sizeof_digit
        ])),
        ("hash_info", PyObject::tuple(vec![
            PyObject::int(64),  // width
            PyObject::int(0),   // modulus
            PyObject::int(0),   // inf
            PyObject::int(0),   // nan
            PyObject::int(0),   // imag
        ])),
        ("__debug__", PyObject::bool_val(true)),
        ("dont_write_bytecode", PyObject::bool_val(true)),
        ("meta_path", PyObject::list(vec![])),
        ("path_hooks", PyObject::list(vec![])),
        ("exc_info", make_builtin(|_| {
            ACTIVE_EXC_INFO.with(|c| {
                let borrow = c.borrow();
                if let Some((kind, msg, obj)) = borrow.as_ref() {
                    let type_obj = PyObject::exception_type(kind.clone());
                    let value_obj = if let Some(o) = obj {
                        o.clone()
                    } else {
                        PyObject::str_val(CompactString::from(msg.as_str()))
                    };
                    // Extract __traceback__ from the exception value
                    let tb_obj = value_obj.get_attr("__traceback__")
                        .unwrap_or_else(|| PyObject::none());
                    Ok(PyObject::tuple(vec![type_obj, value_obj, tb_obj]))
                } else {
                    Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none(), PyObject::none()]))
                }
            })
        })),
        ("_getframe", make_builtin(|_| {
            // Return a minimal frame-like object with common attributes
            let mut attrs = indexmap::IndexMap::new();
            attrs.insert(CompactString::from("f_locals"), PyObject::dict_from_pairs(vec![]));
            attrs.insert(CompactString::from("f_globals"), PyObject::dict_from_pairs(vec![]));
            attrs.insert(CompactString::from("f_lineno"), PyObject::int(0));
            attrs.insert(CompactString::from("f_code"), PyObject::none());
            attrs.insert(CompactString::from("f_back"), PyObject::none());
            Ok(PyObject::module_with_attrs(CompactString::from("frame"), attrs))
        })),
        ("base_prefix", PyObject::str_val(CompactString::from("/usr/local"))),
        ("base_exec_prefix", PyObject::str_val(CompactString::from("/usr/local"))),
        ("api_version", PyObject::int(1013)),
        ("copyright", PyObject::str_val(CompactString::from(
            "Copyright (c) Ferrython contributors.\nBased on Python, Copyright (c) Python Software Foundation."
        ))),
        ("thread_info", PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("pthread")), // name
            PyObject::none(), // lock
            PyObject::str_val(CompactString::from("")), // version
        ])),
        ("builtin_module_names", PyObject::tuple({
            let mut names: Vec<PyObjectRef> = [
                "_abc", "_io", "_thread", "_weakref", "builtins", "errno", "gc",
                "marshal", "os", "sys", "time",
            ].iter().map(|n| PyObject::str_val(CompactString::from(*n))).collect();
            names.sort_by(|a, b| a.py_to_string().cmp(&b.py_to_string()));
            names
        })),
        ("getfilesystemencodeerrors", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("surrogateescape"))))),
    ])
}

fn sys_getrecursionlimit(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(RECURSION_LIMIT.load(Ordering::Relaxed)))
}
fn sys_setrecursionlimit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.setrecursionlimit", args, 1)?;
    let limit = args[0].as_int().ok_or_else(|| PyException::type_error("an integer is required"))?;
    if limit <= 0 {
        return Err(PyException::value_error("recursion limit must be positive"));
    }
    RECURSION_LIMIT.store(limit, Ordering::Relaxed);
    Ok(PyObject::none())
}
fn sys_exit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = if args.is_empty() {
        PyObject::int(0)
    } else {
        args[0].clone()
    };
    Err(PyException::system_exit(code))
}
fn sys_getsizeof(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.getsizeof", args, 1)?;
    Ok(PyObject::int(std::mem::size_of::<PyObject>() as i64))
}

fn sys_settrace(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.settrace", args, 1)?;
    if matches!(&args[0].payload, PyObjectPayload::None) {
        set_trace_func(None);
    } else {
        set_trace_func(Some(args[0].clone()));
    }
    Ok(PyObject::none())
}

fn sys_gettrace(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(get_trace_func().unwrap_or_else(PyObject::none))
}

fn sys_setprofile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.setprofile", args, 1)?;
    if matches!(&args[0].payload, PyObjectPayload::None) {
        set_profile_func(None);
    } else {
        set_profile_func(Some(args[0].clone()));
    }
    Ok(PyObject::none())
}

fn sys_getprofile(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(get_profile_func().unwrap_or_else(PyObject::none))
}

/// Default sys.excepthook: prints exception to stderr.
fn sys_excepthook_default(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error("excepthook requires 3 arguments"));
    }
    let exc_type = &args[0];
    let exc_value = &args[1];
    let _exc_tb = &args[2];
    let type_name = exc_type.py_to_string();
    let value_str = exc_value.py_to_string();
    if value_str.is_empty() {
        eprintln!("{}", type_name);
    } else {
        eprintln!("{}: {}", type_name, value_str);
    }
    Ok(PyObject::none())
}

/// Create a file-like object for stdin/stdout/stderr
fn make_stdio_object(name: &str, mode: &str, fileno: i64) -> PyObjectRef {
    use indexmap::IndexMap;
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name)));
    attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(mode)));
    attrs.insert(CompactString::from("encoding"), PyObject::str_val(CompactString::from("utf-8")));
    attrs.insert(CompactString::from("errors"), PyObject::str_val(CompactString::from("strict")));
    attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    attrs.insert(CompactString::from("line_buffering"), PyObject::bool_val(fileno != 0));
    attrs.insert(CompactString::from("_fileno"), PyObject::int(fileno));
    attrs.insert(CompactString::from("newlines"), PyObject::none());
    attrs.insert(CompactString::from("buffer"), PyObject::none());
    attrs.insert(CompactString::from("write"), PyObject::native_function("write", stdio_write));
    attrs.insert(CompactString::from("writelines"), PyObject::native_function("writelines", stdio_writelines));
    attrs.insert(CompactString::from("read"), PyObject::native_function("read", stdio_read));
    attrs.insert(CompactString::from("readline"), PyObject::native_function("readline", stdio_readline));
    attrs.insert(CompactString::from("readlines"), PyObject::native_function("readlines", stdio_readlines));
    attrs.insert(CompactString::from("flush"), PyObject::native_function("flush", stdio_flush));
    attrs.insert(CompactString::from("fileno"), PyObject::native_function("fileno", stdio_fileno));
    attrs.insert(CompactString::from("isatty"), PyObject::native_function("isatty", stdio_isatty));
    attrs.insert(CompactString::from("readable"), PyObject::native_function("readable", stdio_readable));
    attrs.insert(CompactString::from("writable"), PyObject::native_function("writable", stdio_writable));
    attrs.insert(CompactString::from("seekable"), PyObject::native_function("seekable", stdio_seekable));
    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    
    PyObject::module_with_attrs(CompactString::from("_io.TextIOWrapper"), attrs)
}

fn get_stdio_fd(args: &[PyObjectRef]) -> i64 {
    args.first()
        .and_then(|s| s.get_attr("_fileno"))
        .and_then(|v| v.to_int().ok())
        .unwrap_or(-1)
}

fn stdio_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    let text = if args.len() > 1 { args[1].py_to_string() } else { String::new() };
    let len = text.len();
    if fd == 2 {
        eprint!("{}", text);
    } else {
        print!("{}", text);
    }
    Ok(PyObject::int(len as i64))
}

fn stdio_writelines(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    let lines_obj = if args.len() > 1 { &args[1] } else {
        return Err(PyException::type_error("writelines() missing argument"));
    };
    let items = lines_obj.to_list()?;
    for item in items {
        let text = item.py_to_string();
        if fd == 2 { eprint!("{}", text); } else { print!("{}", text); }
    }
    Ok(PyObject::none())
}

fn stdio_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    if fd != 0 {
        return Err(PyException::runtime_error("not readable"));
    }
    use std::io::Read;
    let max = if args.len() > 1 { args[1].to_int().unwrap_or(-1) } else { -1 };
    let mut buf = String::new();
    if max < 0 {
        std::io::stdin().read_to_string(&mut buf).unwrap_or(0);
    } else {
        let mut handle = std::io::stdin().take(max as u64);
        handle.read_to_string(&mut buf).unwrap_or(0);
    }
    Ok(PyObject::str_val(CompactString::from(buf)))
}

fn stdio_readline(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    if fd != 0 {
        return Err(PyException::runtime_error("not readable"));
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap_or(0);
    Ok(PyObject::str_val(CompactString::from(line)))
}

fn stdio_readlines(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    if fd != 0 {
        return Err(PyException::runtime_error("not readable"));
    }
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let lines: Vec<PyObjectRef> = stdin.lock().lines()
        .filter_map(|l| l.ok())
        .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
        .collect();
    Ok(PyObject::list(lines))
}

fn stdio_flush(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    Ok(PyObject::none())
}

fn stdio_fileno(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(get_stdio_fd(args)))
}

fn stdio_isatty(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(false))
}

fn stdio_readable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(get_stdio_fd(args) == 0))
}

fn stdio_writable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(get_stdio_fd(args) != 0))
}

fn stdio_seekable(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(false))
}

// ── os module ──


pub fn create_os_module() -> PyObjectRef {
    make_module("os", vec![
        ("name", PyObject::str_val(CompactString::from(if cfg!(windows) { "nt" } else { "posix" }))),
        ("sep", PyObject::str_val(CompactString::from(std::path::MAIN_SEPARATOR.to_string()))),
        ("linesep", PyObject::str_val(CompactString::from(if cfg!(windows) { "\r\n" } else { "\n" }))),
        ("curdir", PyObject::str_val(CompactString::from("."))),
        ("pardir", PyObject::str_val(CompactString::from(".."))),
        ("extsep", PyObject::str_val(CompactString::from("."))),
        ("getcwd", make_builtin(os_getcwd)),
        ("listdir", make_builtin(os_listdir)),
        ("mkdir", make_builtin(os_mkdir)),
        ("makedirs", make_builtin(os_makedirs)),
        ("remove", make_builtin(os_remove)),
        ("unlink", make_builtin(os_remove)),
        ("rmdir", make_builtin(os_rmdir)),
        ("removedirs", make_builtin(os_removedirs)),
        ("rename", make_builtin(os_rename)),
        ("path", create_os_path_module()),
        ("getenv", make_builtin(os_getenv)),
        ("environ", {
            // Build _Environ instance: a dict-like object that syncs with OS env.
            // os.environ["X"] = "Y" calls putenv; del os.environ["X"] calls unsetenv.
            let initial_pairs: Vec<(PyObjectRef, PyObjectRef)> = std::env::vars().map(|(k, v)| (
                PyObject::str_val(CompactString::from(k)),
                PyObject::str_val(CompactString::from(v)),
            )).collect();
            let data = PyObject::dict_from_pairs(initial_pairs);
            let data_ref = data.clone();
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("_data"), data.clone());

            // Dunders are called via try_call_dunder which prepends self as args[0].
            // StoreSubscr/DeleteSubscr Module handler calls directly without self.
            // Use helper: last 1 arg = key, last 2 args = key+val.
            attrs.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                "__getitem__", move |args| {
                    // args may be [self, key] or [key]
                    let key_str = args.last().ok_or_else(|| PyException::key_error("key required"))?.py_to_string();
                    match std::env::var(&key_str) {
                        Ok(val) => Ok(PyObject::str_val(CompactString::from(val))),
                        Err(_) => Err(PyException::key_error(format!("'{}'", key_str))),
                    }
                }
            ));
            let d2 = data_ref.clone();
            attrs.insert(CompactString::from("__setitem__"), PyObject::native_closure(
                "__setitem__", move |args| {
                    // args may be [self, key, val] or [key, val]
                    if args.len() < 2 { return Err(PyException::type_error("__setitem__ requires key and value")); }
                    let val_str = args[args.len() - 1].py_to_string();
                    let key_str = args[args.len() - 2].py_to_string();
                    unsafe { std::env::set_var(&key_str, &val_str); }
                    if let PyObjectPayload::Dict(dd) = &d2.payload {
                        dd.write().insert(
                            HashableKey::Str(CompactString::from(&key_str)),
                            PyObject::str_val(CompactString::from(&val_str)),
                        );
                    }
                    Ok(PyObject::none())
                }
            ));
            let d3 = data_ref.clone();
            attrs.insert(CompactString::from("__delitem__"), PyObject::native_closure(
                "__delitem__", move |args| {
                    let key_str = args.last().ok_or_else(|| PyException::key_error("key required"))?.py_to_string();
                    unsafe { std::env::remove_var(&key_str); }
                    if let PyObjectPayload::Dict(dd) = &d3.payload {
                        dd.write().swap_remove(&HashableKey::Str(CompactString::from(&key_str)));
                    }
                    Ok(PyObject::none())
                }
            ));
            attrs.insert(CompactString::from("__contains__"), PyObject::native_closure(
                "__contains__", move |args| {
                    let key_str = args.last().map(|a| a.py_to_string()).unwrap_or_default();
                    Ok(PyObject::bool_val(std::env::var(&key_str).is_ok()))
                }
            ));
            attrs.insert(CompactString::from("get"), PyObject::native_closure(
                "get", move |args| {
                    // args: [self, key] or [self, key, default]
                    // Skip self (first arg if module)
                    let real_args = if args.len() > 1 && matches!(&args[0].payload, PyObjectPayload::Module(_)) {
                        &args[1..]
                    } else { args };
                    if real_args.is_empty() { return Ok(PyObject::none()); }
                    let key_str = real_args[0].py_to_string();
                    match std::env::var(&key_str) {
                        Ok(val) => Ok(PyObject::str_val(CompactString::from(val))),
                        Err(_) => Ok(real_args.get(1).cloned().unwrap_or_else(PyObject::none)),
                    }
                }
            ));
            attrs.insert(CompactString::from("keys"), PyObject::native_closure(
                "keys", move |_| {
                    let keys: Vec<PyObjectRef> = std::env::vars()
                        .map(|(k, _)| PyObject::str_val(CompactString::from(k)))
                        .collect();
                    Ok(PyObject::list(keys))
                }
            ));
            attrs.insert(CompactString::from("values"), PyObject::native_closure(
                "values", move |_| {
                    let vals: Vec<PyObjectRef> = std::env::vars()
                        .map(|(_, v)| PyObject::str_val(CompactString::from(v)))
                        .collect();
                    Ok(PyObject::list(vals))
                }
            ));
            attrs.insert(CompactString::from("items"), PyObject::native_closure(
                "items", move |_| {
                    let items: Vec<PyObjectRef> = std::env::vars()
                        .map(|(k, v)| PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(k)),
                            PyObject::str_val(CompactString::from(v)),
                        ]))
                        .collect();
                    Ok(PyObject::list(items))
                }
            ));
            attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
                "__repr__", move |_| {
                    Ok(PyObject::str_val(CompactString::from("environ({...})")))
                }
            ));
            PyObject::module_with_attrs(CompactString::from("_Environ"), attrs)
        }),
        ("cpu_count", make_builtin(os_cpu_count)),
        ("getpid", make_builtin(os_getpid)),
        ("fspath", PyObject::native_function("os.fspath", os_fspath)),
        ("walk", make_builtin(os_walk)),
        ("stat", make_builtin(os_stat)),
        ("chmod", make_builtin(os_chmod)),
        ("symlink", make_builtin(os_symlink)),
        ("readlink", make_builtin(os_readlink)),
        ("isatty", make_builtin(os_isatty)),
        ("chdir", make_builtin(os_chdir)),
        ("system", make_builtin(os_system)),
        ("popen", make_builtin(os_popen)),
        ("getppid", make_builtin(|_| {
            Ok(PyObject::int(std::process::id() as i64)) // Approximate with current PID
        })),
        ("urandom", make_builtin(|args| {
            let n = if args.is_empty() { 16 } else { args[0].as_int().unwrap_or(16) as usize };
            let mut buf = vec![0u8; n];
            #[cfg(unix)]
            {
                use std::io::Read;
                if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
                    let _ = f.read_exact(&mut buf);
                }
            }
            Ok(PyObject::bytes(buf))
        })),
        ("access", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let path = args[0].py_to_string();
            Ok(PyObject::bool_val(std::path::Path::new(&path).exists()))
        })),
        ("umask", make_builtin(|_| Ok(PyObject::int(0o022)))),
        ("getlogin", make_builtin(|_| {
            let user = std::env::var("USER")
                .or_else(|_| std::env::var("LOGNAME"))
                .or_else(|_| {
                    // Fallback: try whoami command
                    std::process::Command::new("whoami")
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .map_err(|_| std::env::VarError::NotPresent)
                })
                .unwrap_or_else(|_| String::from("unknown"));
            Ok(PyObject::str_val(CompactString::from(user)))
        })),
        ("devnull", PyObject::str_val(CompactString::from(if cfg!(windows) { "nul" } else { "/dev/null" }))),
        ("F_OK", PyObject::int(0)),
        ("R_OK", PyObject::int(4)),
        ("W_OK", PyObject::int(2)),
        ("X_OK", PyObject::int(1)),
        ("O_RDONLY", PyObject::int(0)),
        ("O_WRONLY", PyObject::int(1)),
        ("O_RDWR", PyObject::int(2)),
        ("O_CREAT", PyObject::int(0o100)),
        ("O_TRUNC", PyObject::int(0o1000)),
        ("O_APPEND", PyObject::int(0o2000)),
        ("scandir", make_builtin(os_scandir)),
        ("putenv", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("putenv requires 2 arguments")); }
            let key = args[0].py_to_string();
            let val = args[1].py_to_string();
            // Safety: we ensure no concurrent modification of env in this single-threaded interpreter
            unsafe { std::env::set_var(&key, &val); }
            Ok(PyObject::none())
        })),
        ("unsetenv", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("unsetenv requires 1 argument")); }
            let key = args[0].py_to_string();
            unsafe { std::env::remove_var(&key); }
            Ok(PyObject::none())
        })),
        ("lstat", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("os.lstat requires path")); }
            let path = args[0].py_to_string();
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            crate::fs_modules::build_stat_result(meta)
        })),
        ("expanduser", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("expanduser requires path")); }
            let path = args[0].py_to_string();
            if path.starts_with("~/") || path == "~" {
                if let Ok(home) = std::env::var("HOME") {
                    let expanded = if path == "~" { home } else { format!("{}{}", home, &path[1..]) };
                    return Ok(PyObject::str_val(CompactString::from(expanded)));
                }
            }
            Ok(PyObject::str_val(CompactString::from(path)))
        })),
        // Unix ID functions
        ("getuid", make_builtin(|_| {
            #[cfg(unix)] { Ok(PyObject::int(unsafe { libc::getuid() } as i64)) }
            #[cfg(not(unix))] { Ok(PyObject::int(0)) }
        })),
        ("getgid", make_builtin(|_| {
            #[cfg(unix)] { Ok(PyObject::int(unsafe { libc::getgid() } as i64)) }
            #[cfg(not(unix))] { Ok(PyObject::int(0)) }
        })),
        ("geteuid", make_builtin(|_| {
            #[cfg(unix)] { Ok(PyObject::int(unsafe { libc::geteuid() } as i64)) }
            #[cfg(not(unix))] { Ok(PyObject::int(0)) }
        })),
        ("getegid", make_builtin(|_| {
            #[cfg(unix)] { Ok(PyObject::int(unsafe { libc::getegid() } as i64)) }
            #[cfg(not(unix))] { Ok(PyObject::int(0)) }
        })),
        ("getppid", make_builtin(|_| {
            #[cfg(unix)] { Ok(PyObject::int(unsafe { libc::getppid() } as i64)) }
            #[cfg(not(unix))] { Ok(PyObject::int(0)) }
        })),
        // Process management
        ("kill", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("os.kill requires pid and signal")); }
            let pid = args[0].as_int().ok_or_else(|| PyException::type_error("pid must be int"))?;
            let sig = args[1].as_int().ok_or_else(|| PyException::type_error("signal must be int"))?;
            #[cfg(unix)] {
                let ret = unsafe { libc::kill(pid as i32, sig as i32) };
                if ret != 0 { return Err(PyException::os_error(format!("kill failed: errno {}", ret))); }
            }
            Ok(PyObject::none())
        })),
        // File operations
        ("link", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("os.link requires src and dst")); }
            std::fs::hard_link(args[0].py_to_string(), args[1].py_to_string())
                .map_err(|e| PyException::os_error(format!("{}", e)))?;
            Ok(PyObject::none())
        })),
        ("truncate", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("os.truncate requires path and length")); }
            let path = args[0].py_to_string();
            let length = args[1].as_int().unwrap_or(0) as u64;
            let f = std::fs::OpenOptions::new().write(true).open(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            f.set_len(length).map_err(|e| PyException::os_error(format!("{}", e)))?;
            Ok(PyObject::none())
        })),
        // Pipe and fd operations
        ("pipe", make_builtin(|_| {
            #[cfg(unix)] {
                let mut fds = [0i32; 2];
                let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
                if ret != 0 { return Err(PyException::os_error("pipe() failed".to_string())); }
                Ok(PyObject::tuple(vec![PyObject::int(fds[0] as i64), PyObject::int(fds[1] as i64)]))
            }
            #[cfg(not(unix))] { Err(PyException::not_implemented_error("os.pipe not available on this platform")) }
        })),
        ("dup", make_builtin(|args| {
            check_args("os.dup", args, 1)?;
            let fd = args[0].as_int().ok_or_else(|| PyException::type_error("fd must be int"))?;
            #[cfg(unix)] {
                let new_fd = unsafe { libc::dup(fd as i32) };
                if new_fd < 0 { return Err(PyException::os_error("dup() failed".to_string())); }
                Ok(PyObject::int(new_fd as i64))
            }
            #[cfg(not(unix))] { Err(PyException::not_implemented_error("os.dup not available")) }
        })),
        ("dup2", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("os.dup2 requires oldfd and newfd")); }
            let oldfd = args[0].as_int().ok_or_else(|| PyException::type_error("fd must be int"))?;
            let newfd = args[1].as_int().ok_or_else(|| PyException::type_error("fd must be int"))?;
            #[cfg(unix)] {
                let ret = unsafe { libc::dup2(oldfd as i32, newfd as i32) };
                if ret < 0 { return Err(PyException::os_error("dup2() failed".to_string())); }
                Ok(PyObject::int(ret as i64))
            }
            #[cfg(not(unix))] { Err(PyException::not_implemented_error("os.dup2 not available")) }
        })),
        // Terminal/system info
        ("get_terminal_size", make_builtin(|_| {
            // Default fallback; real implementation would use ioctl
            let cols = std::env::var("COLUMNS").ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(80);
            let lines = std::env::var("LINES").ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(24);
            let cls = PyObject::class(CompactString::from("terminal_size"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("columns"), PyObject::int(cols));
                attrs.insert(CompactString::from("lines"), PyObject::int(lines));
            }
            Ok(inst)
        })),
        ("uname", make_builtin(|_| {
            #[cfg(unix)] {
                let mut info: libc::utsname = unsafe { std::mem::zeroed() };
                unsafe { libc::uname(&mut info); }
                let to_str = |arr: &[i8]| -> String {
                    let bytes: Vec<u8> = arr.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
                    String::from_utf8_lossy(&bytes).to_string()
                };
                let cls = PyObject::class(CompactString::from("uname_result"), vec![], IndexMap::new());
                let inst = PyObject::instance(cls);
                if let PyObjectPayload::Instance(ref data) = inst.payload {
                    let mut attrs = data.attrs.write();
                    attrs.insert(CompactString::from("sysname"), PyObject::str_val(CompactString::from(to_str(&info.sysname))));
                    attrs.insert(CompactString::from("nodename"), PyObject::str_val(CompactString::from(to_str(&info.nodename))));
                    attrs.insert(CompactString::from("release"), PyObject::str_val(CompactString::from(to_str(&info.release))));
                    attrs.insert(CompactString::from("version"), PyObject::str_val(CompactString::from(to_str(&info.version))));
                    attrs.insert(CompactString::from("machine"), PyObject::str_val(CompactString::from(to_str(&info.machine))));
                }
                Ok(inst)
            }
            #[cfg(not(unix))] {
                Err(PyException::not_implemented_error("os.uname not available on this platform"))
            }
        })),
        ("times", make_builtin(|_| {
            #[cfg(unix)] {
                let mut tms: libc::tms = unsafe { std::mem::zeroed() };
                unsafe { libc::times(&mut tms); }
                let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
                Ok(PyObject::tuple(vec![
                    PyObject::float(tms.tms_utime as f64 / ticks),
                    PyObject::float(tms.tms_stime as f64 / ticks),
                    PyObject::float(tms.tms_cutime as f64 / ticks),
                    PyObject::float(tms.tms_cstime as f64 / ticks),
                    PyObject::float(0.0), // elapsed
                ]))
            }
            #[cfg(not(unix))] { Ok(PyObject::tuple(vec![PyObject::float(0.0); 5])) }
        })),
        // Path constants
        ("pathsep", PyObject::str_val(CompactString::from(if cfg!(windows) { ";" } else { ":" }))),
        ("altsep", PyObject::none()),
        // Low-level file descriptor operations
        ("close", make_builtin(|args| {
            check_args("os.close", args, 1)?;
            let fd = args[0].as_int().ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
            #[cfg(unix)] {
                let ret = unsafe { libc::close(fd) };
                if ret != 0 { return Err(PyException::os_error(format!("Bad file descriptor: {}", fd))); }
            }
            Ok(PyObject::none())
        })),
        ("open", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("os.open requires path, flags, and optional mode")); }
            let path = args[0].py_to_string();
            let flags = if args.len() > 1 { args[1].as_int().unwrap_or(0) as i32 } else { 0 };
            let mode = if args.len() > 2 { args[2].as_int().unwrap_or(0o666) as u32 } else { 0o666 };
            #[cfg(unix)] {
                let cpath = std::ffi::CString::new(path.as_str())
                    .map_err(|_| PyException::value_error("invalid path"))?;
                let fd = unsafe { libc::open(cpath.as_ptr(), flags, mode) };
                if fd < 0 { return Err(PyException::os_error(format!("No such file or directory: '{}'", path))); }
                Ok(PyObject::int(fd as i64))
            }
            #[cfg(not(unix))] { Err(PyException::not_implemented_error("os.open not available")) }
        })),
        ("read", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("os.read requires fd and count")); }
            let fd = args[0].as_int().ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
            let count = args[1].as_int().ok_or_else(|| PyException::type_error("count must be int"))? as usize;
            #[cfg(unix)] {
                let mut buf = vec![0u8; count];
                let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, count) };
                if n < 0 { return Err(PyException::os_error("read failed".to_string())); }
                buf.truncate(n as usize);
                Ok(PyObject::bytes(buf))
            }
            #[cfg(not(unix))] { Err(PyException::not_implemented_error("os.read not available")) }
        })),
        ("write", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("os.write requires fd and data")); }
            let fd = args[0].as_int().ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
            let data = match &args[1].payload {
                PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => b.clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => return Err(PyException::type_error("data must be bytes-like")),
            };
            #[cfg(unix)] {
                let n = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
                if n < 0 { return Err(PyException::os_error("write failed".to_string())); }
                Ok(PyObject::int(n as i64))
            }
            #[cfg(not(unix))] { Err(PyException::not_implemented_error("os.write not available")) }
        })),
        ("fdopen", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("os.fdopen requires fd")); }
            let _fd = args[0].as_int().ok_or_else(|| PyException::type_error("fd must be int"))?;
            // For now, return a simple wrapper — full implementation would create a file object
            Err(PyException::not_implemented_error("os.fdopen not fully implemented; use open() instead"))
        })),
        ("fstat", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("os.fstat requires fd")); }
            let fd = args[0].as_int().ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
            #[cfg(unix)]
            {
                use std::os::unix::io::FromRawFd;
                let file = unsafe { std::fs::File::from_raw_fd(fd) };
                let meta = file.metadata().map_err(|e| PyException::os_error(format!("{}", e)));
                std::mem::forget(file);
                let meta = meta?;
                build_stat_result_from_meta(&meta)
            }
            #[cfg(not(unix))]
            {
                let _ = fd;
                Err(PyException::not_implemented_error("os.fstat not supported on this platform"))
            }
        })),
        ("ftruncate", make_builtin(|args| {
            check_args_min("os.ftruncate", args, 2)?;
            let fd = args[0].as_int().ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
            let length = args[1].as_int().ok_or_else(|| PyException::type_error("length must be int"))? as u64;
            #[cfg(unix)]
            {
                use std::os::unix::io::FromRawFd;
                let file = unsafe { std::fs::File::from_raw_fd(fd) };
                let result = file.set_len(length).map_err(|e| PyException::os_error(format!("{}", e)));
                std::mem::forget(file);
                result?;
                Ok(PyObject::none())
            }
            #[cfg(not(unix))]
            {
                let _ = (fd, length);
                Err(PyException::not_implemented_error("os.ftruncate not supported on this platform"))
            }
        })),
        ("stat_result", make_builtin(|_| {
            Ok(PyObject::class(CompactString::from("stat_result"), vec![], IndexMap::new()))
        })),
    ])
}

fn os_getcwd(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cwd = std::env::current_dir()
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::str_val(CompactString::from(cwd.to_string_lossy().to_string())))
}
fn os_listdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = if args.is_empty() { ".".to_string() } else { args[0].py_to_string() };
    let entries = std::fs::read_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    let items: Vec<PyObjectRef> = entries
        .filter_map(|e| e.ok())
        .map(|e| PyObject::str_val(CompactString::from(e.file_name().to_string_lossy().to_string())))
        .collect();
    Ok(PyObject::list(items))
}
fn os_mkdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("os.mkdir() requires at least 1 argument")); }
    let path = args[0].py_to_string();
    let exist_ok = args.iter().skip(1).any(|a| {
        if let PyObjectPayload::Dict(kw) = &a.payload {
            kw.read().get(&HashableKey::Str(CompactString::from("exist_ok")))
                .map(|v| matches!(&v.payload, PyObjectPayload::Bool(true)))
                .unwrap_or(false)
        } else { false }
    });
    match std::fs::create_dir(&path) {
        Ok(_) => Ok(PyObject::none()),
        Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => Ok(PyObject::none()),
        Err(e) => Err(PyException::os_error(format!("{}", e))),
    }
}
fn os_makedirs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("os.makedirs() requires at least 1 argument")); }
    let path = args[0].py_to_string();
    // Check for exist_ok kwarg (may be in trailing dict)
    let exist_ok = args.iter().skip(1).any(|a| {
        if let PyObjectPayload::Dict(kw) = &a.payload {
            kw.read().get(&HashableKey::Str(CompactString::from("exist_ok")))
                .map(|v| matches!(&v.payload, PyObjectPayload::Bool(true)))
                .unwrap_or(false)
        } else {
            matches!(&a.payload, PyObjectPayload::Bool(true))
        }
    });
    match std::fs::create_dir_all(&path) {
        Ok(_) => Ok(PyObject::none()),
        Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => Ok(PyObject::none()),
        Err(e) => Err(PyException::os_error(format!("{}", e))),
    }
}
fn os_remove(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.remove", args, 1)?;
    std::fs::remove_file(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_rmdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.rmdir", args, 1)?;
    std::fs::remove_dir(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_removedirs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.removedirs", args, 1)?;
    let path_str = args[0].py_to_string();
    let mut path = std::path::PathBuf::from(&*path_str);
    // Remove the leaf directory first
    std::fs::remove_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    // Walk up, removing empty parent directories until one fails
    while let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() {
            break;
        }
        if std::fs::remove_dir(parent).is_err() {
            break;
        }
        path = parent.to_path_buf();
    }
    Ok(PyObject::none())
}
fn os_rename(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.rename", args, 2)?;
    std::fs::rename(args[0].py_to_string(), args[1].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_getenv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("os.getenv requires at least 1 argument")); }
    let key = args[0].py_to_string();
    let default = if args.len() > 1 { args[1].clone() } else { PyObject::none() };
    match std::env::var(&key) {
        Ok(v) => Ok(PyObject::str_val(CompactString::from(v))),
        Err(_) => Ok(default),
    }
}
fn os_cpu_count(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(num_cpus() as i64))
}
fn os_getpid(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(std::process::id() as i64))
}

fn os_fspath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.fspath", args, 1)?;
    match &args[0].payload {
        PyObjectPayload::Str(_) => Ok(args[0].clone()),
        PyObjectPayload::Bytes(_) => Ok(args[0].clone()),
        _ => {
            // Check for __fspath__ method
            if let Some(method) = args[0].get_attr("__fspath__") {
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => func(&[args[0].clone()]),
                    PyObjectPayload::NativeClosure { func, .. } => func(&[args[0].clone()]),
                    PyObjectPayload::Function(_) => {
                        Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
                    }
                    _ => Err(PyException::type_error(format!(
                        "expected str, bytes or os.PathLike object, not '{}'", args[0].type_name()
                    ))),
                }
            } else {
                Err(PyException::type_error(format!(
                    "expected str, bytes or os.PathLike object, not '{}'", args[0].type_name()
                )))
            }
        }
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

fn os_walk(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("os.walk() requires at least 1 argument"));
    }
    let path = args[0].py_to_string();
    let topdown = if args.len() > 1 { args[1].is_truthy() } else { true };
    let mut results = Vec::new();
    walk_dir_recursive(&path, topdown, &mut results);
    Ok(PyObject::list(results))
}

fn walk_dir_recursive(dir: &str, topdown: bool, results: &mut Vec<PyObjectRef>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut dirnames = Vec::new();
    let mut filenames = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            dirnames.push(name);
        } else {
            filenames.push(name);
        }
    }
    let tuple = PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(dir)),
        PyObject::list(dirnames.iter().map(|n| PyObject::str_val(CompactString::from(n.as_str()))).collect()),
        PyObject::list(filenames.iter().map(|n| PyObject::str_val(CompactString::from(n.as_str()))).collect()),
    ]);
    if topdown {
        results.push(tuple);
        for name in &dirnames {
            let child = format!("{}/{}", dir.trim_end_matches('/'), name);
            walk_dir_recursive(&child, topdown, results);
        }
    } else {
        for name in &dirnames {
            let child = format!("{}/{}", dir.trim_end_matches('/'), name);
            walk_dir_recursive(&child, topdown, results);
        }
        results.push(tuple);
    }
}

fn build_stat_result_from_meta(meta: &std::fs::Metadata) -> PyResult<PyObjectRef> {
    let mut attrs = indexmap::IndexMap::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        attrs.insert(CompactString::from("st_mode"), PyObject::int(meta.mode() as i64));
        attrs.insert(CompactString::from("st_ino"), PyObject::int(meta.ino() as i64));
        attrs.insert(CompactString::from("st_dev"), PyObject::int(meta.dev() as i64));
        attrs.insert(CompactString::from("st_nlink"), PyObject::int(meta.nlink() as i64));
        attrs.insert(CompactString::from("st_uid"), PyObject::int(meta.uid() as i64));
        attrs.insert(CompactString::from("st_gid"), PyObject::int(meta.gid() as i64));
    }
    #[cfg(not(unix))]
    {
        attrs.insert(CompactString::from("st_mode"), PyObject::int(0));
        attrs.insert(CompactString::from("st_ino"), PyObject::int(0));
        attrs.insert(CompactString::from("st_dev"), PyObject::int(0));
        attrs.insert(CompactString::from("st_nlink"), PyObject::int(0));
        attrs.insert(CompactString::from("st_uid"), PyObject::int(0));
        attrs.insert(CompactString::from("st_gid"), PyObject::int(0));
    }
    attrs.insert(CompactString::from("st_size"), PyObject::int(meta.len() as i64));
    let epoch = std::time::SystemTime::UNIX_EPOCH;
    let mtime = meta.modified().ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let atime = meta.accessed().ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let ctime = meta.created().ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    attrs.insert(CompactString::from("st_mtime"), PyObject::float(mtime));
    attrs.insert(CompactString::from("st_atime"), PyObject::float(atime));
    attrs.insert(CompactString::from("st_ctime"), PyObject::float(ctime));
    Ok(PyObject::module_with_attrs(CompactString::from("os.stat_result"), attrs))
}

fn os_stat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.stat", args, 1)?;
    let path = args[0].py_to_string();
    let meta = std::fs::metadata(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    build_stat_result_from_meta(&meta)
}

fn os_chmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.chmod", args, 2)?;
    #[cfg(unix)]
    {
        let path = args[0].py_to_string();
        let mode = args[1].as_int()
            .ok_or_else(|| PyException::type_error("an integer is required"))?;
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(mode as u32);
        std::fs::set_permissions(&path, perms)
            .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    }
    Ok(PyObject::none())
}

fn os_symlink(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.symlink", args, 2)?;
    let src = args[0].py_to_string();
    let dst = args[1].py_to_string();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&src, &dst)
            .map_err(|e| PyException::os_error(format!("{}: '{}' -> '{}'", e, dst, src)))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (&src, &dst);
        return Err(PyException::os_error("os.symlink() not available on this platform"));
    }
    Ok(PyObject::none())
}

fn os_readlink(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.readlink", args, 1)?;
    let path = args[0].py_to_string();
    let target = std::fs::read_link(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    Ok(PyObject::str_val(CompactString::from(target.to_string_lossy().to_string())))
}

fn os_isatty(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.isatty", args, 1)?;
    let fd = args[0].as_int()
        .ok_or_else(|| PyException::type_error("an integer is required"))?;
    Ok(PyObject::bool_val(is_fd_terminal(fd)))
}

#[cfg(unix)]
fn is_fd_terminal(fd: i64) -> bool {
    unsafe {
        extern "C" { fn isatty(fd: i32) -> i32; }
        isatty(fd as i32) != 0
    }
}

#[cfg(not(unix))]
fn is_fd_terminal(_fd: i64) -> bool {
    false
}

fn os_chdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.chdir", args, 1)?;
    let path = args[0].py_to_string();
    std::env::set_current_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    Ok(PyObject::none())
}

fn os_system(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.system", args, 1)?;
    let cmd = args[0].py_to_string();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .status()
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::int(status.code().unwrap_or(-1) as i64))
}

/// os.popen(cmd) → file-like object with read()/close()
fn os_popen(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.popen", args, 1)?;
    let cmd = args[0].py_to_string();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    let data = String::from_utf8_lossy(&output.stdout).to_string();
    let data_arc = std::sync::Arc::new(parking_lot::RwLock::new(data));

    let cls = PyObject::class(CompactString::from("_POpenFile"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        let d = data_arc.clone();
        attrs.insert(CompactString::from("read"), PyObject::native_closure("popen.read", move |_: &[PyObjectRef]| {
            Ok(PyObject::str_val(CompactString::from(d.read().as_str())))
        }));
        attrs.insert(CompactString::from("close"), make_builtin(|_| Ok(PyObject::none())));
        let d2 = data_arc;
        attrs.insert(CompactString::from("readline"), PyObject::native_closure("popen.readline", move |_: &[PyObjectRef]| {
            Ok(PyObject::str_val(CompactString::from(d2.read().as_str())))
        }));
    }
    Ok(inst)
}

fn os_scandir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = if args.is_empty() { ".".to_string() } else { args[0].py_to_string() };
    let entries = std::fs::read_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = entry.path().to_string_lossy().to_string();
        let file_type = entry.file_type().ok();
        let is_file = file_type.as_ref().map(|ft| ft.is_file()).unwrap_or(false);
        let is_dir = file_type.as_ref().map(|ft| ft.is_dir()).unwrap_or(false);
        let is_symlink = file_type.as_ref().map(|ft| ft.is_symlink()).unwrap_or(false);

        let cls = PyObject::class(CompactString::from("DirEntry"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(&name)));
        attrs.insert(CompactString::from("path"), PyObject::str_val(CompactString::from(&full_path)));

        let is_file_val = is_file;
        attrs.insert(CompactString::from("is_file"), PyObject::native_closure(
            "DirEntry.is_file", move |_| Ok(PyObject::bool_val(is_file_val))));
        let is_dir_val = is_dir;
        attrs.insert(CompactString::from("is_dir"), PyObject::native_closure(
            "DirEntry.is_dir", move |_| Ok(PyObject::bool_val(is_dir_val))));
        let is_sym_val = is_symlink;
        attrs.insert(CompactString::from("is_symlink"), PyObject::native_closure(
            "DirEntry.is_symlink", move |_| Ok(PyObject::bool_val(is_sym_val))));
        // stat() — cached metadata
        let stat_path = full_path.clone();
        attrs.insert(CompactString::from("stat"), PyObject::native_closure(
            "DirEntry.stat", move |_| {
                let meta = std::fs::metadata(&stat_path)
                    .map_err(|e| PyException::os_error(format!("{}: '{}'", e, stat_path)))?;
                crate::fs_modules::build_stat_result(meta)
            }));
        // __repr__
        let repr_name = name.clone();
        attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
            "DirEntry.__repr__", move |_| {
                Ok(PyObject::str_val(CompactString::from(format!("<DirEntry '{}'>", repr_name))))
            }));
        // __str__ returns name
        let str_name = name.clone();
        attrs.insert(CompactString::from("__str__"), PyObject::native_closure(
            "DirEntry.__str__", move |_| {
                Ok(PyObject::str_val(CompactString::from(str_name.as_str())))
            }));
        items.push(PyObject::instance_with_attrs(cls, attrs));
    }
    Ok(PyObject::list(items))
}

// ── os.path module ──


pub fn create_os_path_module() -> PyObjectRef {
    make_module("os.path", vec![
        ("join", make_builtin(os_path_join)),
        ("exists", make_builtin(os_path_exists)),
        ("isfile", make_builtin(os_path_isfile)),
        ("isdir", make_builtin(os_path_isdir)),
        ("islink", make_builtin(os_path_islink)),
        ("basename", make_builtin(os_path_basename)),
        ("dirname", make_builtin(os_path_dirname)),
        ("abspath", make_builtin(os_path_abspath)),
        ("splitext", make_builtin(os_path_splitext)),
        ("split", make_builtin(os_path_split)),
        ("isabs", make_builtin(os_path_isabs)),
        ("normpath", make_builtin(os_path_normpath)),
        ("expanduser", make_builtin(os_path_expanduser)),
        ("expandvars", make_builtin(os_path_expandvars)),
        ("getsize", make_builtin(os_path_getsize)),
        ("getmtime", make_builtin(os_path_getmtime)),
        ("getctime", make_builtin(os_path_getctime)),
        ("getatime", make_builtin(os_path_getatime)),
        ("sep", PyObject::str_val(CompactString::from(std::path::MAIN_SEPARATOR.to_string()))),
        ("realpath", make_builtin(os_path_realpath)),
        ("relpath", make_builtin(os_path_relpath)),
        ("commonpath", make_builtin(os_path_commonpath)),
        ("commonprefix", make_builtin(os_path_commonprefix)),
        ("samefile", make_builtin(os_path_samefile)),
    ])
}

fn os_path_join(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("os.path.join requires at least 1 argument")); }
    let mut path = std::path::PathBuf::from(args[0].py_to_string());
    for arg in &args[1..] {
        path.push(arg.py_to_string());
    }
    Ok(PyObject::str_val(CompactString::from(path.to_string_lossy().to_string())))
}
fn os_path_exists(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.exists", args, 1)?;
    Ok(PyObject::bool_val(std::path::Path::new(&args[0].py_to_string()).exists()))
}
fn os_path_isfile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.isfile", args, 1)?;
    Ok(PyObject::bool_val(std::path::Path::new(&args[0].py_to_string()).is_file()))
}
fn os_path_isdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.isdir", args, 1)?;
    Ok(PyObject::bool_val(std::path::Path::new(&args[0].py_to_string()).is_dir()))
}
fn os_path_basename(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.basename", args, 1)?;
    let s = args[0].py_to_string();
    // Python: basename("/a/b/") → "", basename("/a/b") → "b"
    if s.ends_with('/') && s.len() > 1 {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    let name = if let Some(pos) = s.rfind('/') {
        &s[pos + 1..]
    } else {
        &s
    };
    Ok(PyObject::str_val(CompactString::from(name)))
}
fn os_path_dirname(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.dirname", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let dir = p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
    Ok(PyObject::str_val(CompactString::from(dir)))
}
fn os_path_abspath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.abspath", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let abs = std::fs::canonicalize(p).unwrap_or_else(|_| {
        let mut cwd = std::env::current_dir().unwrap_or_default();
        cwd.push(&s);
        cwd
    });
    Ok(PyObject::str_val(CompactString::from(abs.to_string_lossy().to_string())))
}
fn os_path_splitext(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.splitext", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let stem = s[..s.len()-ext.len()].to_string();
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(stem)),
        PyObject::str_val(CompactString::from(ext)),
    ]))
}
fn os_path_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.split", args, 1)?;
    let s = args[0].py_to_string();
    // Python's os.path.split: trailing slash → (path, "")
    if s.ends_with('/') && s.len() > 1 {
        let trimmed = s.trim_end_matches('/');
        return Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(trimmed)),
            PyObject::str_val(CompactString::from("")),
        ]));
    }
    if let Some(pos) = s.rfind('/') {
        let head = if pos == 0 { "/" } else { &s[..pos] };
        let tail = &s[pos + 1..];
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(head)),
            PyObject::str_val(CompactString::from(tail)),
        ]))
    } else {
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("")),
            PyObject::str_val(CompactString::from(s)),
        ]))
    }
}
fn os_path_isabs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.isabs", args, 1)?;
    Ok(PyObject::bool_val(std::path::Path::new(&args[0].py_to_string()).is_absolute()))
}
fn os_path_normpath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.normpath", args, 1)?;
    let s = args[0].py_to_string();
    // Basic normpath: collapse separators and resolve . / ..
    let mut parts: Vec<&str> = Vec::new();
    for part in s.split('/') {
        match part {
            "" | "." => {}
            ".." => { parts.pop(); }
            other => parts.push(other),
        }
    }
    let result = if s.starts_with('/') {
        format!("/{}", parts.join("/"))
    } else if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}
fn os_path_expanduser(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.expanduser", args, 1)?;
    let s = args[0].py_to_string();
    if s.starts_with('~') {
        if let Ok(home) = std::env::var("HOME") {
            return Ok(PyObject::str_val(CompactString::from(format!("{}{}", home, &s[1..]))));
        }
    }
    Ok(PyObject::str_val(CompactString::from(s)))
}
fn os_path_getsize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getsize", args, 1)?;
    let s = args[0].py_to_string();
    match std::fs::metadata(&s) {
        Ok(m) => Ok(PyObject::int(m.len() as i64)),
        Err(e) => Err(PyException::from_io_error(&e, Some(&s))),
    }
}

fn os_path_realpath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.realpath", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let real = std::fs::canonicalize(p).unwrap_or_else(|_| {
        let mut cwd = std::env::current_dir().unwrap_or_default();
        cwd.push(&s);
        cwd
    });
    Ok(PyObject::str_val(CompactString::from(real.to_string_lossy().to_string())))
}

fn os_path_relpath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("os.path.relpath() requires at least 1 argument"));
    }
    let path_str = args[0].py_to_string();
    let start_str = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    };
    let make_abs = |s: &str| -> std::path::PathBuf {
        let p = std::path::Path::new(s);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            let mut cwd = std::env::current_dir().unwrap_or_default();
            cwd.push(s);
            cwd
        }
    };
    let path_abs = make_abs(&path_str);
    let start_abs = make_abs(&start_str);
    let path_components: Vec<_> = path_abs.components().collect();
    let start_components: Vec<_> = start_abs.components().collect();
    let common_len = path_components.iter().zip(start_components.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut result = std::path::PathBuf::new();
    for _ in common_len..start_components.len() {
        result.push("..");
    }
    for component in &path_components[common_len..] {
        result.push(component);
    }
    let result_str = if result.as_os_str().is_empty() {
        ".".to_string()
    } else {
        result.to_string_lossy().to_string()
    };
    Ok(PyObject::str_val(CompactString::from(result_str)))
}

fn os_path_commonpath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.commonpath", args, 1)?;
    let paths = args[0].to_list()?;
    if paths.is_empty() {
        return Err(PyException::value_error("commonpath() arg is an empty sequence"));
    }
    let path_strs: Vec<String> = paths.iter().map(|p| p.py_to_string()).collect();
    let first_abs = path_strs[0].starts_with('/');
    for p in &path_strs[1..] {
        if p.starts_with('/') != first_abs {
            return Err(PyException::value_error("Can't mix absolute and relative paths"));
        }
    }
    let split: Vec<Vec<&str>> = path_strs.iter()
        .map(|p| p.split('/').filter(|s| !s.is_empty()).collect())
        .collect();
    let min_len = split.iter().map(|p| p.len()).min().unwrap_or(0);
    let mut common_len = 0;
    for i in 0..min_len {
        if split.iter().all(|p| p[i] == split[0][i]) {
            common_len = i + 1;
        } else {
            break;
        }
    }
    let common_parts: Vec<&str> = split[0][..common_len].to_vec();
    let result = if first_abs {
        if common_parts.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", common_parts.join("/"))
        }
    } else if common_parts.is_empty() {
        ".".to_string()
    } else {
        common_parts.join("/")
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn os_path_getmtime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getmtime", args, 1)?;
    let s = args[0].py_to_string();
    let meta = std::fs::metadata(&s).map_err(|e| PyException::from_io_error(&e, Some(&s)))?;
    let mtime = meta.modified().map_err(|_| PyException::runtime_error("getmtime failed"))?;
    let epoch = mtime.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    Ok(PyObject::float(epoch.as_secs_f64()))
}

fn os_path_getctime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getctime", args, 1)?;
    let s = args[0].py_to_string();
    let meta = std::fs::metadata(&s).map_err(|e| PyException::from_io_error(&e, Some(&s)))?;
    // On Unix, ctime is metadata change time (use created or modified as fallback)
    let ctime = meta.created().or_else(|_| meta.modified())
        .map_err(|_| PyException::runtime_error("getctime failed"))?;
    let epoch = ctime.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    Ok(PyObject::float(epoch.as_secs_f64()))
}

fn os_path_getatime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getatime", args, 1)?;
    let s = args[0].py_to_string();
    let meta = std::fs::metadata(&s).map_err(|e| PyException::from_io_error(&e, Some(&s)))?;
    let atime = meta.accessed().map_err(|_| PyException::runtime_error("getatime failed"))?;
    let epoch = atime.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    Ok(PyObject::float(epoch.as_secs_f64()))
}

fn os_path_expandvars(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.expandvars", args, 1)?;
    let s = args[0].py_to_string();
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'{' {
                i += 1; // skip {
                let start = i;
                while i < bytes.len() && bytes[i] != b'}' { i += 1; }
                let var = &s[start..i];
                if i < bytes.len() { i += 1; } // skip }
                result.push_str(&std::env::var(var).unwrap_or(format!("${{{}}}", var)));
            } else {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
                let var = &s[start..i];
                if var.is_empty() {
                    result.push('$');
                } else {
                    result.push_str(&std::env::var(var).unwrap_or(format!("${}", var)));
                }
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn os_path_commonprefix(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.commonprefix", args, 1)?;
    let paths = args[0].to_list()?;
    if paths.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
    let strs: Vec<String> = paths.iter().map(|p| p.py_to_string()).collect();
    let first = strs[0].as_bytes();
    let mut prefix_len = first.len();
    for s in &strs[1..] {
        let b = s.as_bytes();
        prefix_len = prefix_len.min(b.len());
        for i in 0..prefix_len {
            if first[i] != b[i] { prefix_len = i; break; }
        }
    }
    Ok(PyObject::str_val(CompactString::from(&strs[0][..prefix_len])))
}

fn os_path_samefile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("samefile() requires 2 arguments")); }
    let a = std::fs::canonicalize(args[0].py_to_string());
    let b = std::fs::canonicalize(args[1].py_to_string());
    match (a, b) {
        (Ok(pa), Ok(pb)) => Ok(PyObject::bool_val(pa == pb)),
        _ => Ok(PyObject::bool_val(false)),
    }
}

fn os_path_islink(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.islink", args, 1)?;
    let s = args[0].py_to_string();
    Ok(PyObject::bool_val(std::fs::symlink_metadata(&s).map(|m| m.file_type().is_symlink()).unwrap_or(false)))
}


pub fn create_platform_module() -> PyObjectRef {
    make_module("platform", vec![
        ("system", make_builtin(|_| {
            let os = std::env::consts::OS;
            // CPython capitalizes: "Linux", "Darwin", "Windows"
            let capitalized = match os {
                "linux" => "Linux",
                "macos" => "Darwin",
                "windows" => "Windows",
                "freebsd" => "FreeBSD",
                "openbsd" => "OpenBSD",
                "netbsd" => "NetBSD",
                other => other,
            };
            Ok(PyObject::str_val(CompactString::from(capitalized)))
        })),
        ("machine", make_builtin(|_| Ok(PyObject::str_val(CompactString::from(std::env::consts::ARCH))))),
        ("python_version", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("3.8.0"))))),
        ("python_implementation", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("Ferrython"))))),
        ("node", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("localhost"))))),
        ("release", make_builtin(|_| Ok(PyObject::str_val(CompactString::from(""))))),
        ("version", make_builtin(|_| Ok(PyObject::str_val(CompactString::from(""))))),
        ("processor", make_builtin(|_| Ok(PyObject::str_val(CompactString::from(std::env::consts::ARCH))))),
        ("architecture", make_builtin(|_| Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(if cfg!(target_pointer_width = "64") { "64bit" } else { "32bit" })),
            PyObject::str_val(CompactString::from("ELF")),
        ])))),
    ])
}

// ── locale module ──

pub fn create_locale_module() -> PyObjectRef {
    // Detect system locale from environment
    fn get_system_locale() -> (String, String) {
        let lang = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_CTYPE"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_else(|_| "C".to_string());
        if lang == "C" || lang == "POSIX" || lang.is_empty() {
            return ("C".to_string(), "ANSI_X3.4-1968".to_string());
        }
        // Parse "en_US.UTF-8" into ("en_US", "UTF-8")
        if let Some(dot) = lang.find('.') {
            let locale_name = &lang[..dot];
            let encoding = lang[dot+1..].trim_end_matches(|c: char| !c.is_alphanumeric() && c != '-');
            (locale_name.to_string(), encoding.to_string())
        } else {
            (lang.clone(), "UTF-8".to_string())
        }
    }

    let current_locale: Arc<RwLock<(String, String)>> = Arc::new(RwLock::new(get_system_locale()));

    let cl1 = current_locale.clone();
    let getlocale_fn = PyObject::native_closure("getlocale", move |_: &[PyObjectRef]| {
        let (name, enc) = cl1.read().clone();
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(name)),
            PyObject::str_val(CompactString::from(enc)),
        ]))
    });

    let cl2 = current_locale.clone();
    let setlocale_fn = PyObject::native_closure("setlocale", move |args: &[PyObjectRef]| {
        let _category = if !args.is_empty() { args[0].to_int().unwrap_or(0) } else { 0 };
        if args.len() >= 2 {
            let locale_str = args[1].py_to_string();
            if locale_str.is_empty() || locale_str == "C" || locale_str == "POSIX" {
                *cl2.write() = ("C".to_string(), "ANSI_X3.4-1968".to_string());
            } else if let Some(dot) = locale_str.find('.') {
                *cl2.write() = (locale_str[..dot].to_string(), locale_str[dot+1..].to_string());
            } else {
                *cl2.write() = (locale_str.clone(), "UTF-8".to_string());
            }
        }
        let (name, enc) = cl2.read().clone();
        Ok(PyObject::str_val(CompactString::from(format!("{}.{}", name, enc))))
    });

    let cl3 = current_locale.clone();
    let localeconv_fn = PyObject::native_closure("localeconv", move |_: &[PyObjectRef]| {
        let (name, _) = cl3.read().clone();
        let is_c = name == "C" || name == "POSIX";
        let mut conv = IndexMap::new();
        conv.insert(CompactString::from("decimal_point"), PyObject::str_val(CompactString::from(".")));
        conv.insert(CompactString::from("thousands_sep"), PyObject::str_val(CompactString::from(if is_c { "" } else { "," })));
        conv.insert(CompactString::from("grouping"), PyObject::list(if is_c { vec![] } else { vec![PyObject::int(3), PyObject::int(0)] }));
        conv.insert(CompactString::from("int_curr_symbol"), PyObject::str_val(CompactString::from("")));
        conv.insert(CompactString::from("currency_symbol"), PyObject::str_val(CompactString::from("")));
        conv.insert(CompactString::from("mon_decimal_point"), PyObject::str_val(CompactString::from(".")));
        conv.insert(CompactString::from("mon_thousands_sep"), PyObject::str_val(CompactString::from(if is_c { "" } else { "," })));
        conv.insert(CompactString::from("p_sign_posn"), PyObject::int(1));
        conv.insert(CompactString::from("n_sign_posn"), PyObject::int(1));
        let dict = PyObject::dict_from_pairs(conv.into_iter().map(|(k, v)| (PyObject::str_val(k), v)).collect());
        Ok(dict)
    });

    let normalize_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("normalize() requires 1 argument")); }
        let s = args[0].py_to_string();
        // Simple normalization: "en_US" → "en_US.UTF-8"
        if !s.contains('.') {
            Ok(PyObject::str_val(CompactString::from(format!("{}.UTF-8", s))))
        } else {
            Ok(PyObject::str_val(CompactString::from(s)))
        }
    });

    make_module("locale", vec![
        ("getlocale", getlocale_fn),
        ("setlocale", setlocale_fn),
        ("localeconv", localeconv_fn),
        ("normalize", normalize_fn),
        ("getpreferredencoding", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("UTF-8"))))),
        ("getdefaultlocale", make_builtin(|_| {
            let lang = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string());
            let (name, enc) = if let Some(dot) = lang.find('.') {
                (lang[..dot].to_string(), lang[dot+1..].to_string())
            } else {
                (lang, "UTF-8".to_string())
            };
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(name)),
                PyObject::str_val(CompactString::from(enc)),
            ]))
        })),
        ("LC_ALL", PyObject::int(0)),
        ("LC_COLLATE", PyObject::int(1)),
        ("LC_CTYPE", PyObject::int(2)),
        ("LC_MESSAGES", PyObject::int(3)),
        ("LC_MONETARY", PyObject::int(4)),
        ("LC_NUMERIC", PyObject::int(5)),
        ("LC_TIME", PyObject::int(6)),
        ("CHAR_MAX", PyObject::int(127)),
        ("Error", PyObject::builtin_type(CompactString::from("locale.Error"))),
        ("str", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
        })),
        ("atof", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("atof() requires 1 argument")); }
            let s = args[0].py_to_string().replace(',', "");
            let f: f64 = s.parse().map_err(|_| PyException::value_error(format!("could not convert '{}' to float", s)))?;
            Ok(PyObject::float(f))
        })),
        ("atoi", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("atoi() requires 1 argument")); }
            let s = args[0].py_to_string().replace(',', "");
            let n: i64 = s.parse().map_err(|_| PyException::value_error(format!("could not convert '{}' to int", s)))?;
            Ok(PyObject::int(n))
        })),
    ])
}

// ── inspect module (stub) ──

// ── getpass module ───────────────────────────────────────────────────
pub fn create_getpass_module() -> PyObjectRef {
    fn getpass_getuser(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .or_else(|_| std::env::var("USERNAME"));
        let user = match user {
            Ok(u) => u,
            Err(_) => {
                // Last resort: try whoami command (unix)
                std::process::Command::new("whoami")
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            }
        };
        Ok(PyObject::str_val(CompactString::from(user)))
    }

    fn getpass_getpass(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let prompt = if args.is_empty() { "Password: " } else { args[0].as_str().unwrap_or("Password: ") };
        eprint!("{}", prompt);
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).map_err(|e|
            PyException::runtime_error(format!("getpass failed: {}", e)))?;
        Ok(PyObject::str_val(CompactString::from(input.trim_end())))
    }

    make_module("getpass", vec![
        ("getuser", make_builtin(getpass_getuser)),
        ("getpass", make_builtin(getpass_getpass)),
    ])
}

// ── errno module ──

pub fn create_errno_module() -> PyObjectRef {
    make_module("errno", vec![
        ("EPERM", PyObject::int(1)),
        ("ENOENT", PyObject::int(2)),
        ("ESRCH", PyObject::int(3)),
        ("EINTR", PyObject::int(4)),
        ("EIO", PyObject::int(5)),
        ("ENXIO", PyObject::int(6)),
        ("E2BIG", PyObject::int(7)),
        ("ENOEXEC", PyObject::int(8)),
        ("EBADF", PyObject::int(9)),
        ("ECHILD", PyObject::int(10)),
        ("EAGAIN", PyObject::int(11)),
        ("ENOMEM", PyObject::int(12)),
        ("EACCES", PyObject::int(13)),
        ("EFAULT", PyObject::int(14)),
        ("EBUSY", PyObject::int(16)),
        ("EEXIST", PyObject::int(17)),
        ("EXDEV", PyObject::int(18)),
        ("ENODEV", PyObject::int(19)),
        ("ENOTDIR", PyObject::int(20)),
        ("EISDIR", PyObject::int(21)),
        ("EINVAL", PyObject::int(22)),
        ("ENFILE", PyObject::int(23)),
        ("EMFILE", PyObject::int(24)),
        ("ENOTTY", PyObject::int(25)),
        ("EFBIG", PyObject::int(27)),
        ("ENOSPC", PyObject::int(28)),
        ("ESPIPE", PyObject::int(29)),
        ("EROFS", PyObject::int(30)),
        ("EMLINK", PyObject::int(31)),
        ("EPIPE", PyObject::int(32)),
        ("EDOM", PyObject::int(33)),
        ("ERANGE", PyObject::int(34)),
        ("EDEADLK", PyObject::int(35)),
        ("ENAMETOOLONG", PyObject::int(36)),
        ("ENOLCK", PyObject::int(37)),
        ("ENOSYS", PyObject::int(38)),
        ("ENOTEMPTY", PyObject::int(39)),
        ("ECONNREFUSED", PyObject::int(111)),
        ("ETIMEDOUT", PyObject::int(110)),
        ("errorcode", make_builtin(|_| {
            let mut map = IndexMap::new();
            let codes: Vec<(i64, &str)> = vec![
                (1, "EPERM"), (2, "ENOENT"), (13, "EACCES"), (17, "EEXIST"),
                (22, "EINVAL"), (32, "EPIPE"), (110, "ETIMEDOUT"), (111, "ECONNREFUSED"),
            ];
            for (num, name) in codes {
                map.insert(HashableKey::Int(PyInt::Small(num)), PyObject::str_val(CompactString::from(name)));
            }
            Ok(PyObject::dict(map))
        })),
    ])
}

// ── atexit module ──

pub fn create_atexit_module() -> PyObjectRef {
    use std::sync::{Arc, Mutex};
    let callbacks: Arc<Mutex<Vec<PyObjectRef>>> = Arc::new(Mutex::new(Vec::new()));
    let cb_reg = callbacks.clone();
    let register_fn = PyObject::native_closure("atexit.register", move |args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("atexit.register requires a callable")); }
        cb_reg.lock().unwrap().push(args[0].clone());
        Ok(args[0].clone())
    });
    let cb_unreg = callbacks.clone();
    let unregister_fn = PyObject::native_closure("atexit.unregister", move |_args: &[PyObjectRef]| {
        let _cbs = cb_unreg.lock().unwrap();
        Ok(PyObject::none())
    });
    let _ncallbacks = PyObject::native_closure("atexit._ncallbacks", move |_args: &[PyObjectRef]| {
        let cbs = callbacks.lock().unwrap();
        Ok(PyObject::int(cbs.len() as i64))
    });
    make_module("atexit", vec![
        ("register", register_fn),
        ("unregister", unregister_fn),
        ("_run_exitfuncs", make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()))),
        ("_ncallbacks", _ncallbacks),
    ])
}

// ── site module ──

pub fn create_site_module() -> PyObjectRef {
    let layout = ferrython_toolchain::paths::InstallLayout::discover();
    let site_packages_str = layout.site_packages.to_string_lossy().to_string();
    let prefix_str = layout.prefix.to_string_lossy().to_string();

    let sp_clone = site_packages_str.clone();
    let getsitepackages = PyObject::native_closure("getsitepackages", move |_args: &[PyObjectRef]| {
        Ok(PyObject::list(vec![
            PyObject::str_val(CompactString::from(sp_clone.as_str())),
        ]))
    });

    let getusersitepackages = make_builtin(|_args: &[PyObjectRef]| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let user_site = format!("{}/.local/lib/ferrython/site-packages", home);
        Ok(PyObject::str_val(CompactString::from(user_site)))
    });

    let addsitedir = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(ferrython_core::error::PyException::type_error("addsitedir requires 1 argument"));
        }
        let _dir = args[0].py_to_string();
        Ok(PyObject::none())
    });

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let user_base = format!("{}/.local", home);
    let user_site = format!("{}/lib/ferrython/site-packages", user_base);

    make_module("site", vec![
        ("ENABLE_USER_SITE", PyObject::bool_val(true)),
        ("USER_SITE", PyObject::str_val(CompactString::from(user_site.as_str()))),
        ("USER_BASE", PyObject::str_val(CompactString::from(user_base.as_str()))),
        ("PREFIXES", PyObject::list(vec![
            PyObject::str_val(CompactString::from(prefix_str.as_str())),
        ])),
        ("getusersitepackages", getusersitepackages),
        ("getsitepackages", getsitepackages),
        ("addsitedir", addsitedir),
    ])
}

// ── sched module ──

pub fn create_sched_module() -> PyObjectRef {
    // Event namedtuple-like: (time, priority, sequence, action, argument, kwargs)
    let event_cls = PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());

    let event_cls2 = event_cls.clone();
    let scheduler_fn = PyObject::native_closure("scheduler", move |args: &[PyObjectRef]| {
        // scheduler(timefunc=time.monotonic, delayfunc=time.sleep)
        // We use std::time for the default implementation
        let _timefunc = args.first().cloned();
        let _delayfunc = args.get(1).cloned();

        // Internal priority queue: Vec of (time_f64, priority, sequence, action, args, kwargs)
        let queue: Arc<RwLock<Vec<(f64, i64, i64, PyObjectRef, PyObjectRef, PyObjectRef)>>> =
            Arc::new(RwLock::new(Vec::new()));
        let seq_counter: Arc<std::sync::atomic::AtomicI64> =
            Arc::new(std::sync::atomic::AtomicI64::new(0));

        let cls = PyObject::class(CompactString::from("scheduler"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);

        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();

            // enterabs(time, priority, action, argument=(), kwargs={})
            let q = queue.clone();
            let seq = seq_counter.clone();
            let ev_cls = event_cls2.clone();
            w.insert(CompactString::from("enterabs"), PyObject::native_closure(
                "enterabs", move |args: &[PyObjectRef]| {
                    if args.len() < 3 {
                        return Err(PyException::type_error("enterabs() requires at least 3 arguments"));
                    }
                    let time_val = args[0].to_float()?;
                    let priority = args[1].to_int().unwrap_or(1);
                    let action = args[2].clone();
                    let argument = args.get(3).cloned().unwrap_or_else(|| PyObject::tuple(vec![]));
                    let kwargs = args.get(4).cloned().unwrap_or_else(PyObject::none);
                    let s = seq.fetch_add(1, Ordering::SeqCst);

                    let event = PyObject::instance(ev_cls.clone());
                    if let PyObjectPayload::Instance(ref ed) = event.payload {
                        let mut ew = ed.attrs.write();
                        ew.insert(CompactString::from("time"), PyObject::float(time_val));
                        ew.insert(CompactString::from("priority"), PyObject::int(priority));
                        ew.insert(CompactString::from("sequence"), PyObject::int(s));
                        ew.insert(CompactString::from("action"), action.clone());
                        ew.insert(CompactString::from("argument"), argument.clone());
                        ew.insert(CompactString::from("kwargs"), kwargs.clone());
                    }

                    let mut qw = q.write();
                    qw.push((time_val, priority, s, action, argument, kwargs));
                    // Sort by (time, priority, sequence) ascending
                    qw.sort_by(|a, b| {
                        a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
                            .then(a.1.cmp(&b.1))
                            .then(a.2.cmp(&b.2))
                    });

                    Ok(event)
                }
            ));

            // enter(delay, priority, action, argument=(), kwargs={})
            let q2 = queue.clone();
            let seq2 = seq_counter.clone();
            let ev_cls2 = event_cls2.clone();
            w.insert(CompactString::from("enter"), PyObject::native_closure(
                "enter", move |args: &[PyObjectRef]| {
                    if args.len() < 3 {
                        return Err(PyException::type_error("enter() requires at least 3 arguments"));
                    }
                    let delay = args[0].to_float()?;
                    let priority = args[1].to_int().unwrap_or(1);
                    let action = args[2].clone();
                    let argument = args.get(3).cloned().unwrap_or_else(|| PyObject::tuple(vec![]));
                    let kwargs = args.get(4).cloned().unwrap_or_else(PyObject::none);
                    let s = seq2.fetch_add(1, Ordering::SeqCst);

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs_f64();
                    let time_val = now + delay;

                    let event = PyObject::instance(ev_cls2.clone());
                    if let PyObjectPayload::Instance(ref ed) = event.payload {
                        let mut ew = ed.attrs.write();
                        ew.insert(CompactString::from("time"), PyObject::float(time_val));
                        ew.insert(CompactString::from("priority"), PyObject::int(priority));
                        ew.insert(CompactString::from("sequence"), PyObject::int(s));
                        ew.insert(CompactString::from("action"), action.clone());
                        ew.insert(CompactString::from("argument"), argument.clone());
                        ew.insert(CompactString::from("kwargs"), kwargs.clone());
                    }

                    let mut qw = q2.write();
                    qw.push((time_val, priority, s, action, argument, kwargs));
                    qw.sort_by(|a, b| {
                        a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
                            .then(a.1.cmp(&b.1))
                            .then(a.2.cmp(&b.2))
                    });

                    Ok(event)
                }
            ));

            // cancel(event) — remove matching event from queue
            let q3 = queue.clone();
            w.insert(CompactString::from("cancel"), PyObject::native_closure(
                "cancel", move |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("cancel() requires 1 argument"));
                    }
                    let event = &args[0];
                    let ev_seq = event.get_attr("sequence")
                        .and_then(|s| s.as_int());
                    if let Some(seq_val) = ev_seq {
                        let mut qw = q3.write();
                        qw.retain(|e| e.2 != seq_val);
                        Ok(PyObject::none())
                    } else {
                        Err(PyException::runtime_error("event not in queue"))
                    }
                }
            ));

            // empty() -> bool
            let q4 = queue.clone();
            w.insert(CompactString::from("empty"), PyObject::native_closure(
                "empty", move |_args: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(q4.read().is_empty()))
                }
            ));

            // run(blocking=True) — execute events in order
            let q5 = queue.clone();
            w.insert(CompactString::from("run"), PyObject::native_closure(
                "run", move |args: &[PyObjectRef]| {
                    let blocking = args.first()
                        .map(|a| !matches!(&a.payload, PyObjectPayload::Bool(false)))
                        .unwrap_or(true);

                    loop {
                        let next = {
                            let qr = q5.read();
                            if qr.is_empty() { break; }
                            qr[0].clone()
                        };

                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs_f64();

                        if next.0 > now {
                            if !blocking { break; }
                            let delay = next.0 - now;
                            if delay > 0.0 {
                                std::thread::sleep(std::time::Duration::from_secs_f64(delay));
                            }
                        }

                        // Pop the event
                        {
                            let mut qw = q5.write();
                            if !qw.is_empty() && qw[0].2 == next.2 {
                                qw.remove(0);
                            } else {
                                continue;
                            }
                        }

                        // Call action(*argument)
                        let action = &next.3;
                        let argument = &next.4;
                        let call_args: Vec<PyObjectRef> = if let PyObjectPayload::Tuple(items) = &argument.payload {
                            items.clone()
                        } else {
                            vec![]
                        };
                        match &action.payload {
                            PyObjectPayload::NativeFunction { func, .. } => { func(&call_args)?; }
                            PyObjectPayload::NativeClosure { func, .. } => { func(&call_args)?; }
                            _ => {
                                // Python function — defer via request_vm_call
                                ferrython_core::error::request_vm_call(action.clone(), call_args);
                            }
                        }
                    }
                    Ok(PyObject::none())
                }
            ));

            // queue property — list of pending events (read-only snapshot)
            let q6 = queue.clone();
            let ev_cls3 = event_cls2.clone();
            w.insert(CompactString::from("queue"), PyObject::native_closure(
                "queue", move |_args: &[PyObjectRef]| {
                    let qr = q6.read();
                    let events: Vec<PyObjectRef> = qr.iter().map(|(t, p, s, act, arg, kw)| {
                        let event = PyObject::instance(ev_cls3.clone());
                        if let PyObjectPayload::Instance(ref ed) = event.payload {
                            let mut ew = ed.attrs.write();
                            ew.insert(CompactString::from("time"), PyObject::float(*t));
                            ew.insert(CompactString::from("priority"), PyObject::int(*p));
                            ew.insert(CompactString::from("sequence"), PyObject::int(*s));
                            ew.insert(CompactString::from("action"), act.clone());
                            ew.insert(CompactString::from("argument"), arg.clone());
                            ew.insert(CompactString::from("kwargs"), kw.clone());
                        }
                        event
                    }).collect();
                    Ok(PyObject::list(events))
                }
            ));
        }

        Ok(inst)
    });

    make_module("sched", vec![
        ("scheduler", scheduler_fn),
        ("Event", event_cls),
    ])
}


// ── mmap module ──

pub fn create_mmap_module() -> PyObjectRef {
    // mmap.mmap(fileno, length, ...) → mmap object
    // Simplified: backed by Vec<u8> (not real file-backed mapping)
    let mmap_fn = make_builtin(|args: &[PyObjectRef]| {
        let fileno = if !args.is_empty() { args[0].to_int().unwrap_or(-1) } else { -1 };
        let length = if args.len() > 1 { args[1].to_int().unwrap_or(0) as usize } else { 0 };

        // If fileno >= 0, try to read the file contents
        let initial_data: Vec<u8> = if fileno >= 0 {
            #[cfg(unix)]
            {
                use std::os::unix::io::FromRawFd;
                // Dup the fd so we don't close the caller's fd
                let dup_fd = unsafe { libc::dup(fileno as i32) };
                if dup_fd >= 0 {
                    let mut file = unsafe { std::fs::File::from_raw_fd(dup_fd) };
                    use std::io::Read;
                    let mut buf = Vec::new();
                    let _ = file.read_to_end(&mut buf);
                    if length > 0 { buf.resize(length, 0); }
                    buf
                } else {
                    vec![0u8; length]
                }
            }
            #[cfg(not(unix))]
            {
                vec![0u8; length]
            }
        } else {
            // Anonymous mapping
            vec![0u8; length]
        };

        let data: Arc<RwLock<Vec<u8>>> = Arc::new(RwLock::new(initial_data));
        let pos: Arc<RwLock<usize>> = Arc::new(RwLock::new(0));
        let closed: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
        let cls = PyObject::class(CompactString::from("mmap"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("closed"), PyObject::bool_val(false));

            // read(n=-1)
            let d2 = data.clone();
            let p2 = pos.clone();
            w.insert(CompactString::from("read"), PyObject::native_closure("read", move |args| {
                let n = if !args.is_empty() { args[0].to_int().unwrap_or(-1) } else { -1 };
                let mut p = p2.write();
                let d = d2.read();
                let start = *p;
                let end = if n < 0 { d.len() } else { std::cmp::min(start + n as usize, d.len()) };
                let slice = d[start..end].to_vec();
                *p = end;
                Ok(PyObject::bytes(slice))
            }));

            // read_byte()
            let d_rb = data.clone();
            let p_rb = pos.clone();
            w.insert(CompactString::from("read_byte"), PyObject::native_closure("read_byte", move |_args| {
                let mut p = p_rb.write();
                let d = d_rb.read();
                if *p >= d.len() {
                    return Err(PyException::value_error("read byte out of range"));
                }
                let byte = d[*p];
                *p += 1;
                Ok(PyObject::int(byte as i64))
            }));

            // write(data)
            let d3 = data.clone();
            let p3 = pos.clone();
            w.insert(CompactString::from("write"), PyObject::native_closure("write", move |args| {
                if args.is_empty() { return Err(PyException::type_error("write requires bytes")); }
                let bytes = match &args[0].payload {
                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => b.clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    _ => return Err(PyException::type_error("write requires bytes argument")),
                };
                let mut d = d3.write();
                let mut p = p3.write();
                let start = *p;
                for (i, &byte) in bytes.iter().enumerate() {
                    let idx = start + i;
                    if idx < d.len() {
                        d[idx] = byte;
                    } else {
                        d.push(byte);
                    }
                }
                *p = start + bytes.len();
                Ok(PyObject::int(bytes.len() as i64))
            }));

            // write_byte(byte)
            let d_wb = data.clone();
            let p_wb = pos.clone();
            w.insert(CompactString::from("write_byte"), PyObject::native_closure("write_byte", move |args| {
                if args.is_empty() { return Err(PyException::type_error("write_byte requires an integer")); }
                let byte = args[0].to_int()? as u8;
                let mut d = d_wb.write();
                let mut p = p_wb.write();
                if *p < d.len() {
                    d[*p] = byte;
                } else {
                    d.push(byte);
                }
                *p += 1;
                Ok(PyObject::none())
            }));

            // seek(pos, whence=0)
            let d_seek = data.clone();
            let p4 = pos.clone();
            w.insert(CompactString::from("seek"), PyObject::native_closure("seek", move |args| {
                if args.is_empty() { return Ok(PyObject::none()); }
                let offset = args[0].to_int().unwrap_or(0);
                let whence = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
                let mut p = p4.write();
                let len = d_seek.read().len() as i64;
                let new_pos = match whence {
                    0 => offset,           // SEEK_SET
                    1 => *p as i64 + offset, // SEEK_CUR
                    2 => len + offset,     // SEEK_END
                    _ => return Err(PyException::value_error("invalid whence")),
                };
                *p = new_pos.max(0) as usize;
                Ok(PyObject::none())
            }));

            // tell()
            let p5 = pos.clone();
            w.insert(CompactString::from("tell"), PyObject::native_closure("tell", move |_args| {
                Ok(PyObject::int(*p5.read() as i64))
            }));

            // size()
            let d6 = data.clone();
            w.insert(CompactString::from("size"), PyObject::native_closure("size", move |_args| {
                Ok(PyObject::int(d6.read().len() as i64))
            }));

            // find(sub, start=0, end=len)
            let d_find = data.clone();
            w.insert(CompactString::from("find"), PyObject::native_closure("find", move |args| {
                if args.is_empty() { return Err(PyException::type_error("find requires bytes argument")); }
                let sub = match &args[0].payload {
                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => b.clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    PyObjectPayload::Int(n) => vec![n.to_i64().unwrap_or(0) as u8],
                    _ => return Err(PyException::type_error("expected bytes")),
                };
                let d = d_find.read();
                let start = if args.len() > 1 { args[1].to_int().unwrap_or(0) as usize } else { 0 };
                let end = if args.len() > 2 { args[2].to_int().unwrap_or(d.len() as i64) as usize } else { d.len() };
                let end = end.min(d.len());
                if start >= end || sub.is_empty() {
                    return Ok(PyObject::int(if sub.is_empty() && start <= end { start as i64 } else { -1 }));
                }
                let haystack = &d[start..end];
                for i in 0..=(haystack.len().saturating_sub(sub.len())) {
                    if haystack[i..].starts_with(&sub) {
                        return Ok(PyObject::int((start + i) as i64));
                    }
                }
                Ok(PyObject::int(-1))
            }));

            // rfind(sub, start=0, end=len)
            let d_rfind = data.clone();
            w.insert(CompactString::from("rfind"), PyObject::native_closure("rfind", move |args| {
                if args.is_empty() { return Err(PyException::type_error("rfind requires bytes argument")); }
                let sub = match &args[0].payload {
                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => b.clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    PyObjectPayload::Int(n) => vec![n.to_i64().unwrap_or(0) as u8],
                    _ => return Err(PyException::type_error("expected bytes")),
                };
                let d = d_rfind.read();
                let start = if args.len() > 1 { args[1].to_int().unwrap_or(0) as usize } else { 0 };
                let end = if args.len() > 2 { args[2].to_int().unwrap_or(d.len() as i64) as usize } else { d.len() };
                let end = end.min(d.len());
                if start >= end || sub.is_empty() {
                    return Ok(PyObject::int(if sub.is_empty() && start <= end { end as i64 } else { -1 }));
                }
                let haystack = &d[start..end];
                for i in (0..=(haystack.len().saturating_sub(sub.len()))).rev() {
                    if haystack[i..].starts_with(&sub) {
                        return Ok(PyObject::int((start + i) as i64));
                    }
                }
                Ok(PyObject::int(-1))
            }));

            // readline()
            let d_rl = data.clone();
            let p_rl = pos.clone();
            w.insert(CompactString::from("readline"), PyObject::native_closure("readline", move |_args| {
                let mut p = p_rl.write();
                let d = d_rl.read();
                if *p >= d.len() {
                    return Ok(PyObject::bytes(vec![]));
                }
                let start = *p;
                let mut end = start;
                while end < d.len() && d[end] != b'\n' {
                    end += 1;
                }
                if end < d.len() { end += 1; } // include the newline
                let line = d[start..end].to_vec();
                *p = end;
                Ok(PyObject::bytes(line))
            }));

            // resize(newsize)
            let d_resize = data.clone();
            w.insert(CompactString::from("resize"), PyObject::native_closure("resize", move |args| {
                if args.is_empty() { return Err(PyException::type_error("resize requires length")); }
                let new_size = args[0].to_int()? as usize;
                d_resize.write().resize(new_size, 0);
                Ok(PyObject::none())
            }));

            // flush(offset=0, size=len)
            w.insert(CompactString::from("flush"), make_builtin(|_args: &[PyObjectRef]| {
                // For Vec-backed mmap, flush is a no-op
                Ok(PyObject::none())
            }));

            // move(dest, src, count)
            let d_move = data.clone();
            w.insert(CompactString::from("move"), PyObject::native_closure("move", move |args| {
                if args.len() < 3 {
                    return Err(PyException::type_error("move requires dest, src, count"));
                }
                let dest = args[0].to_int()? as usize;
                let src = args[1].to_int()? as usize;
                let count = args[2].to_int()? as usize;
                let mut d = d_move.write();
                let len = d.len();
                if src + count > len || dest + count > len {
                    return Err(PyException::value_error("source or destination out of range"));
                }
                // Use copy_within for safe overlapping moves
                d.copy_within(src..src + count, dest);
                Ok(PyObject::none())
            }));

            // close()
            let c_close = closed.clone();
            w.insert(CompactString::from("close"), PyObject::native_closure("close", move |_args| {
                *c_close.write() = true;
                Ok(PyObject::none())
            }));

            // __len__
            let d7 = data.clone();
            w.insert(CompactString::from("__len__"), PyObject::native_closure("__len__", move |_args| {
                Ok(PyObject::int(d7.read().len() as i64))
            }));

            // __getitem__ (indexing and slicing)
            let d8 = data.clone();
            w.insert(CompactString::from("__getitem__"), PyObject::native_closure("__getitem__", move |args| {
                if args.is_empty() { return Err(PyException::index_error("mmap index out of range")); }
                let idx = args[0].to_int().unwrap_or(0);
                let d = d8.read();
                let len = d.len() as i64;
                let resolved = if idx < 0 { len + idx } else { idx };
                if resolved < 0 || resolved >= len {
                    return Err(PyException::index_error("mmap index out of range"));
                }
                Ok(PyObject::int(d[resolved as usize] as i64))
            }));

            // __setitem__ (indexing)
            let d_si = data.clone();
            w.insert(CompactString::from("__setitem__"), PyObject::native_closure("__setitem__", move |args| {
                if args.len() < 2 { return Err(PyException::type_error("__setitem__ requires index and value")); }
                let idx = args[0].to_int().unwrap_or(0);
                let val = args[1].to_int()? as u8;
                let mut d = d_si.write();
                let len = d.len() as i64;
                let resolved = if idx < 0 { len + idx } else { idx };
                if resolved < 0 || resolved >= len {
                    return Err(PyException::index_error("mmap assignment index out of range"));
                }
                d[resolved as usize] = val;
                Ok(PyObject::none())
            }));

            // __enter__ / __exit__ for context manager
            let inst_ref = inst.clone();
            w.insert(CompactString::from("__enter__"), PyObject::native_closure("__enter__", move |_| Ok(inst_ref.clone())));
            w.insert(CompactString::from("__exit__"), make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(false))));

            // __repr__
            let d_repr = data.clone();
            w.insert(CompactString::from("__repr__"), PyObject::native_closure("__repr__", move |_args| {
                let len = d_repr.read().len();
                Ok(PyObject::str_val(CompactString::from(format!("<mmap.mmap object, length={}>", len))))
            }));
        }
        Ok(inst)
    });

    make_module("mmap", vec![
        ("mmap", mmap_fn),
        ("ACCESS_READ", PyObject::int(1)),
        ("ACCESS_WRITE", PyObject::int(2)),
        ("ACCESS_COPY", PyObject::int(3)),
        ("ACCESS_DEFAULT", PyObject::int(0)),
        ("PAGESIZE", PyObject::int({
            #[cfg(unix)]
            { unsafe { libc::sysconf(libc::_SC_PAGESIZE) as i64 } }
            #[cfg(not(unix))]
            { 4096i64 }
        })),
        ("ALLOCATIONGRANULARITY", PyObject::int({
            #[cfg(unix)]
            { unsafe { libc::sysconf(libc::_SC_PAGESIZE) as i64 } }
            #[cfg(not(unix))]
            { 65536i64 }
        })),
        ("MAP_SHARED", PyObject::int(1)),
        ("MAP_PRIVATE", PyObject::int(2)),
        ("PROT_READ", PyObject::int(1)),
        ("PROT_WRITE", PyObject::int(2)),
        ("PROT_EXEC", PyObject::int(4)),
    ])
}

// ── resource module (unix) ──

pub fn create_resource_module() -> PyObjectRef {
    let getrlimit_fn = make_builtin(|args: &[PyObjectRef]| {
        let resource = if !args.is_empty() { args[0].to_int().unwrap_or(0) as i32 } else { 0 };
        #[cfg(unix)]
        {
            let mut rlim: libc::rlimit = unsafe { std::mem::zeroed() };
            let ret = unsafe { libc::getrlimit(resource as libc::__rlimit_resource_t, &mut rlim) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("getrlimit: {}", err)));
            }
            let soft = if rlim.rlim_cur == libc::RLIM_INFINITY { -1i64 } else { rlim.rlim_cur as i64 };
            let hard = if rlim.rlim_max == libc::RLIM_INFINITY { -1i64 } else { rlim.rlim_max as i64 };
            Ok(PyObject::tuple(vec![PyObject::int(soft), PyObject::int(hard)]))
        }
        #[cfg(not(unix))]
        {
            let _ = resource;
            Ok(PyObject::tuple(vec![PyObject::int(-1), PyObject::int(-1)]))
        }
    });

    let setrlimit_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error("setrlimit() requires 2 arguments"));
        }
        let resource = args[0].to_int()? as i32;
        let limits = args[1].to_list()?;
        if limits.len() < 2 {
            return Err(PyException::value_error("expected (soft, hard) tuple"));
        }
        #[cfg(unix)]
        {
            let soft = limits[0].to_int()?;
            let hard = limits[1].to_int()?;
            let rlim = libc::rlimit {
                rlim_cur: if soft < 0 { libc::RLIM_INFINITY } else { soft as libc::rlim_t },
                rlim_max: if hard < 0 { libc::RLIM_INFINITY } else { hard as libc::rlim_t },
            };
            let ret = unsafe { libc::setrlimit(resource as libc::__rlimit_resource_t, &rlim) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("setrlimit: {}", err)));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (resource, limits);
        }
        Ok(PyObject::none())
    });

    let getrusage_fn = make_builtin(|args: &[PyObjectRef]| {
        let who = if !args.is_empty() { args[0].to_int().unwrap_or(0) as i32 } else { 0 };
        #[cfg(unix)]
        {
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            let ret = unsafe { libc::getrusage(who, &mut usage) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("getrusage: {}", err)));
            }
            let cls = PyObject::class(CompactString::from("struct_rusage"), vec![], IndexMap::new());
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("ru_utime"),
                PyObject::float(usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1_000_000.0));
            attrs.insert(CompactString::from("ru_stime"),
                PyObject::float(usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1_000_000.0));
            attrs.insert(CompactString::from("ru_maxrss"), PyObject::int(usage.ru_maxrss));
            attrs.insert(CompactString::from("ru_ixrss"), PyObject::int(usage.ru_ixrss));
            attrs.insert(CompactString::from("ru_idrss"), PyObject::int(usage.ru_idrss));
            attrs.insert(CompactString::from("ru_isrss"), PyObject::int(usage.ru_isrss));
            attrs.insert(CompactString::from("ru_minflt"), PyObject::int(usage.ru_minflt));
            attrs.insert(CompactString::from("ru_majflt"), PyObject::int(usage.ru_majflt));
            attrs.insert(CompactString::from("ru_nswap"), PyObject::int(usage.ru_nswap));
            attrs.insert(CompactString::from("ru_inblock"), PyObject::int(usage.ru_inblock));
            attrs.insert(CompactString::from("ru_oublock"), PyObject::int(usage.ru_oublock));
            attrs.insert(CompactString::from("ru_msgsnd"), PyObject::int(usage.ru_msgsnd));
            attrs.insert(CompactString::from("ru_msgrcv"), PyObject::int(usage.ru_msgrcv));
            attrs.insert(CompactString::from("ru_nsignals"), PyObject::int(usage.ru_nsignals));
            attrs.insert(CompactString::from("ru_nvcsw"), PyObject::int(usage.ru_nvcsw));
            attrs.insert(CompactString::from("ru_nivcsw"), PyObject::int(usage.ru_nivcsw));
            Ok(PyObject::instance_with_attrs(cls, attrs))
        }
        #[cfg(not(unix))]
        {
            let _ = who;
            let cls = PyObject::class(CompactString::from("struct_rusage"), vec![], IndexMap::new());
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("ru_utime"), PyObject::float(0.0));
            attrs.insert(CompactString::from("ru_stime"), PyObject::float(0.0));
            attrs.insert(CompactString::from("ru_maxrss"), PyObject::int(0));
            attrs.insert(CompactString::from("ru_minflt"), PyObject::int(0));
            attrs.insert(CompactString::from("ru_majflt"), PyObject::int(0));
            Ok(PyObject::instance_with_attrs(cls, attrs))
        }
    });

    let getpagesize_fn = make_builtin(|_args: &[PyObjectRef]| {
        #[cfg(unix)]
        {
            let ps = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
            Ok(PyObject::int(if ps > 0 { ps } else { 4096 }))
        }
        #[cfg(not(unix))]
        {
            Ok(PyObject::int(4096))
        }
    });

    make_module("resource", vec![
        ("getrlimit", getrlimit_fn),
        ("setrlimit", setrlimit_fn),
        ("getrusage", getrusage_fn),
        ("getpagesize", getpagesize_fn),
        ("RLIMIT_CPU", PyObject::int(0)),
        ("RLIMIT_FSIZE", PyObject::int(1)),
        ("RLIMIT_DATA", PyObject::int(2)),
        ("RLIMIT_STACK", PyObject::int(3)),
        ("RLIMIT_CORE", PyObject::int(4)),
        ("RLIMIT_RSS", PyObject::int(5)),
        ("RLIMIT_NPROC", PyObject::int(6)),
        ("RLIMIT_NOFILE", PyObject::int(7)),
        ("RLIMIT_MEMLOCK", PyObject::int(8)),
        ("RLIMIT_AS", PyObject::int(9)),
        ("RLIMIT_LOCKS", PyObject::int(10)),
        ("RLIMIT_SIGPENDING", PyObject::int(11)),
        ("RLIMIT_MSGQUEUE", PyObject::int(12)),
        ("RLIM_INFINITY", PyObject::int(-1)),
        ("RUSAGE_SELF", PyObject::int(0)),
        ("RUSAGE_CHILDREN", PyObject::int(-1)),
        ("RUSAGE_THREAD", PyObject::int(1)),
    ])
}

pub fn create_fcntl_module() -> PyObjectRef {
    let fcntl_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Err(PyException::type_error("fcntl() requires at least 2 args")); }
        let fd = args[0].to_int()? as i32;
        let cmd = args[1].to_int()? as i32;
        #[cfg(unix)]
        {
            let result = if args.len() > 2 {
                let arg = args[2].to_int()? as libc::c_long;
                unsafe { libc::fcntl(fd, cmd, arg) }
            } else {
                unsafe { libc::fcntl(fd, cmd) }
            };
            if result == -1 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("fcntl failed: {}", err)));
            }
            Ok(PyObject::int(result as i64))
        }
        #[cfg(not(unix))]
        {
            let _ = (fd, cmd);
            Ok(PyObject::int(0))
        }
    });

    let flock_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Err(PyException::type_error("flock() requires 2 args")); }
        let fd = args[0].to_int()? as i32;
        let operation = args[1].to_int()? as i32;
        #[cfg(unix)]
        {
            let result = unsafe { libc::flock(fd, operation) };
            if result == -1 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("flock failed: {}", err)));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (fd, operation);
        }
        Ok(PyObject::none())
    });

    let lockf_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Err(PyException::type_error("lockf() requires at least 2 args")); }
        let fd = args[0].to_int()? as i32;
        let cmd = args[1].to_int()? as i32;
        let len: i64 = if args.len() > 2 { args[2].to_int()? } else { 0 };
        let start: i64 = if args.len() > 3 { args[3].to_int()? } else { 0 };
        let whence: i32 = if args.len() > 4 { args[4].to_int()? as i32 } else { 0 };
        #[cfg(unix)]
        {
            let mut lock: libc::flock = unsafe { std::mem::zeroed() };
            lock.l_type = cmd as i16;
            lock.l_whence = whence as i16;
            lock.l_start = start as libc::off_t;
            lock.l_len = len as libc::off_t;
            let result = unsafe { libc::fcntl(fd, libc::F_SETLK, &lock) };
            if result == -1 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("lockf failed: {}", err)));
            }
        }
        #[cfg(not(unix))]
        { let _ = (fd, cmd, len, start, whence); }
        Ok(PyObject::none())
    });

    let ioctl_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Err(PyException::type_error("ioctl() requires at least 2 args")); }
        let fd = args[0].to_int()? as i32;
        let request = args[1].to_int()? as u64;
        #[cfg(unix)]
        {
            let result = if args.len() > 2 {
                let arg = args[2].to_int()? as libc::c_ulong;
                unsafe { libc::ioctl(fd, request as libc::c_ulong, arg) }
            } else {
                unsafe { libc::ioctl(fd, request as libc::c_ulong) }
            };
            if result == -1 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("ioctl failed: {}", err)));
            }
            Ok(PyObject::int(result as i64))
        }
        #[cfg(not(unix))]
        {
            let _ = (fd, request);
            Ok(PyObject::int(0))
        }
    });

    make_module("fcntl", vec![
        ("fcntl", fcntl_fn),
        ("flock", flock_fn),
        ("lockf", lockf_fn),
        ("ioctl", ioctl_fn),
        // Lock constants
        ("LOCK_SH", PyObject::int(1)),
        ("LOCK_EX", PyObject::int(2)),
        ("LOCK_NB", PyObject::int(4)),
        ("LOCK_UN", PyObject::int(8)),
        // fcntl constants
        ("F_DUPFD", PyObject::int(0)),
        ("F_GETFD", PyObject::int(1)),
        ("F_SETFD", PyObject::int(2)),
        ("F_GETFL", PyObject::int(3)),
        ("F_SETFL", PyObject::int(4)),
        ("F_GETLK", PyObject::int(5)),
        ("F_SETLK", PyObject::int(6)),
        ("F_SETLKW", PyObject::int(7)),
        ("FD_CLOEXEC", PyObject::int(1)),
    ])
}

// ── sysconfig module ──

pub fn create_sysconfig_module() -> PyObjectRef {
    let layout = ferrython_toolchain::paths::InstallLayout::discover();

    let get_python_version = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::str_val(CompactString::from("3.11")))
    });

    let get_platform = make_builtin(|_args: &[PyObjectRef]| {
        let platform = if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "windows") {
            "win32"
        } else {
            "unknown"
        };
        Ok(PyObject::str_val(CompactString::from(platform)))
    });

    let layout_path = layout.clone();
    let get_path = PyObject::native_closure("get_path", move |args: &[PyObjectRef]| {
        let name = if args.is_empty() { String::from("stdlib") } else { args[0].py_to_string() };
        let path_val = layout_path.get_path(&name)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        Ok(PyObject::str_val(CompactString::from(path_val)))
    });

    let layout_paths = layout.clone();
    let get_paths = PyObject::native_closure("get_paths", move |_args: &[PyObjectRef]| {
        let names = ["stdlib", "purelib", "platlib", "include", "scripts", "data"];
        let pairs: Vec<(PyObjectRef, PyObjectRef)> = names.iter()
            .map(|name| {
                let path_val = layout_paths.get_path(name)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                (
                    PyObject::str_val(CompactString::from(*name)),
                    PyObject::str_val(CompactString::from(path_val)),
                )
            })
            .collect();
        Ok(PyObject::dict_from_pairs(pairs))
    });

    let layout_var = layout.clone();
    let get_config_var = PyObject::native_closure("get_config_var", move |args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        let name = args[0].py_to_string();
        match layout_var.get_config_var(&name) {
            Some(val) => Ok(PyObject::str_val(CompactString::from(val))),
            None => Ok(PyObject::none()),
        }
    });

    let layout_vars = layout.clone();
    let get_config_vars = PyObject::native_closure("get_config_vars", move |_args: &[PyObjectRef]| {
        let keys = ["prefix", "exec_prefix", "base_prefix", "BINDIR", "py_version_short",
                     "SOABI", "EXT_SUFFIX", "SIZEOF_VOID_P", "installed_base"];
        let pairs: Vec<(PyObjectRef, PyObjectRef)> = keys.iter()
            .filter_map(|k| {
                layout_vars.get_config_var(k).map(|v| (
                    PyObject::str_val(CompactString::from(*k)),
                    PyObject::str_val(CompactString::from(v)),
                ))
            })
            .collect();
        Ok(PyObject::dict_from_pairs(pairs))
    });

    let get_default_scheme = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::str_val(CompactString::from("posix_prefix")))
    });

    let get_scheme_names = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::list(vec![
            PyObject::str_val(CompactString::from("posix_prefix")),
            PyObject::str_val(CompactString::from("posix_user")),
            PyObject::str_val(CompactString::from("nt")),
            PyObject::str_val(CompactString::from("nt_user")),
        ]))
    });

    make_module("sysconfig", vec![
        ("get_python_version", get_python_version),
        ("get_platform", get_platform),
        ("get_path", get_path),
        ("get_paths", get_paths),
        ("get_config_var", get_config_var),
        ("get_config_vars", get_config_vars),
        ("get_default_scheme", get_default_scheme),
        ("get_scheme_names", get_scheme_names),
    ])
}
