//! System, OS, and platform stdlib modules

use compact_str::CompactString;
use std::sync::atomic::{AtomicI64, Ordering};
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args,
};

static RECURSION_LIMIT: AtomicI64 = AtomicI64::new(1000);

/// Thread-local active exception info for sys.exc_info().
/// Set by the VM when entering an except block, cleared when leaving.
thread_local! {
    static ACTIVE_EXC_INFO: std::cell::RefCell<Option<(ExceptionKind, String, Option<PyObjectRef>)>> =
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
                    Ok(PyObject::tuple(vec![type_obj, value_obj, PyObject::none()]))
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
    let code = if args.is_empty() { 0 } else { args[0].to_int().unwrap_or(1) };
    std::process::exit(code as i32);
}
fn sys_getsizeof(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.getsizeof", args, 1)?;
    Ok(PyObject::int(std::mem::size_of::<PyObject>() as i64))
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
        ("rename", make_builtin(os_rename)),
        ("path", create_os_path_module()),
        ("getenv", make_builtin(os_getenv)),
        ("environ", PyObject::dict_from_pairs(
            std::env::vars().map(|(k, v)| (
                PyObject::str_val(CompactString::from(k)),
                PyObject::str_val(CompactString::from(v)),
            )).collect()
        )),
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
    check_args("os.mkdir", args, 1)?;
    std::fs::create_dir(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_makedirs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.makedirs", args, 1)?;
    std::fs::create_dir_all(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
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

fn os_stat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.stat", args, 1)?;
    let path = args[0].py_to_string();
    let meta = std::fs::metadata(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
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

// ── os.path module ──


pub fn create_os_path_module() -> PyObjectRef {
    make_module("os.path", vec![
        ("join", make_builtin(os_path_join)),
        ("exists", make_builtin(os_path_exists)),
        ("isfile", make_builtin(os_path_isfile)),
        ("isdir", make_builtin(os_path_isdir)),
        ("basename", make_builtin(os_path_basename)),
        ("dirname", make_builtin(os_path_dirname)),
        ("abspath", make_builtin(os_path_abspath)),
        ("splitext", make_builtin(os_path_splitext)),
        ("split", make_builtin(os_path_split)),
        ("isabs", make_builtin(os_path_isabs)),
        ("normpath", make_builtin(os_path_normpath)),
        ("expanduser", make_builtin(os_path_expanduser)),
        ("getsize", make_builtin(os_path_getsize)),
        ("sep", PyObject::str_val(CompactString::from(std::path::MAIN_SEPARATOR.to_string()))),
        ("realpath", make_builtin(os_path_realpath)),
        ("relpath", make_builtin(os_path_relpath)),
        ("commonpath", make_builtin(os_path_commonpath)),
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
        Err(_e) => Err(PyException::os_error(format!("No such file: '{}'", s))),
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

// ── string module ──


pub fn create_platform_module() -> PyObjectRef {
    make_module("platform", vec![
        ("system", make_builtin(|_| Ok(PyObject::str_val(CompactString::from(std::env::consts::OS))))),
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

// ── locale module (stub) ──


pub fn create_locale_module() -> PyObjectRef {
    make_module("locale", vec![
        ("getlocale", make_builtin(|_| Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("en_US")),
            PyObject::str_val(CompactString::from("UTF-8")),
        ])))),
        ("setlocale", make_builtin(|args| {
            if args.len() >= 2 { Ok(args[1].clone()) }
            else { Ok(PyObject::str_val(CompactString::from(""))) }
        })),
        ("LC_ALL", PyObject::int(0)),
        ("LC_COLLATE", PyObject::int(1)),
        ("LC_CTYPE", PyObject::int(2)),
        ("LC_NUMERIC", PyObject::int(5)),
        ("LC_TIME", PyObject::int(6)),
        ("getpreferredencoding", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("UTF-8"))))),
    ])
}

// ── inspect module (stub) ──

// ── getpass module ───────────────────────────────────────────────────
pub fn create_getpass_module() -> PyObjectRef {
    fn getpass_getuser(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());
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


