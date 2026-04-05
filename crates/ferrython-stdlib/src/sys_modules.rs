//! System, OS, and platform stdlib modules

use compact_str::CompactString;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;

static RECURSION_LIMIT: AtomicI64 = AtomicI64::new(1000);

// Thread-local active exception info for sys.exc_info().
// Set by the VM when entering an except block, cleared when leaving.
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

/// Read active exception info for traceback.format_exc() etc.
pub fn get_exc_info() -> Option<(ExceptionKind, String)> {
    ACTIVE_EXC_INFO.with(|c| {
        c.borrow().as_ref().map(|(k, m, _)| (k.clone(), m.clone()))
    })
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
        ("environ", {
            // Build environ as a real Dict (so isinstance(os.environ, dict) == True)
            // Note: modifications via os.environ["X"] = "Y" update the dict but not the OS env.
            // Use os.putenv() to also update the OS env.
            // os.getenv() reads from OS env (matches CPython).
            PyObject::dict_from_pairs(
                std::env::vars().map(|(k, v)| (
                    PyObject::str_val(CompactString::from(k)),
                    PyObject::str_val(CompactString::from(v)),
                )).collect()
            )
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
        Err(_e) => Err(PyException::file_not_found_error(format!("No such file: '{}'", s))),
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
    let meta = std::fs::metadata(&s).map_err(|_| PyException::file_not_found_error(format!("No such file: '{}'", s)))?;
    let mtime = meta.modified().map_err(|_| PyException::runtime_error("getmtime failed"))?;
    let epoch = mtime.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    Ok(PyObject::float(epoch.as_secs_f64()))
}

fn os_path_getctime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getctime", args, 1)?;
    let s = args[0].py_to_string();
    let meta = std::fs::metadata(&s).map_err(|_| PyException::file_not_found_error(format!("No such file: '{}'", s)))?;
    // On Unix, ctime is metadata change time (use created or modified as fallback)
    let ctime = meta.created().or_else(|_| meta.modified())
        .map_err(|_| PyException::runtime_error("getctime failed"))?;
    let epoch = ctime.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    Ok(PyObject::float(epoch.as_secs_f64()))
}

fn os_path_getatime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getatime", args, 1)?;
    let s = args[0].py_to_string();
    let meta = std::fs::metadata(&s).map_err(|_| PyException::file_not_found_error(format!("No such file: '{}'", s)))?;
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
    make_module("site", vec![
        ("ENABLE_USER_SITE", PyObject::bool_val(false)),
        ("USER_SITE", PyObject::none()),
        ("USER_BASE", PyObject::none()),
        ("PREFIXES", PyObject::list(vec![])),
        ("getusersitepackages", make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::str_val(CompactString::from(""))))),
        ("getsitepackages", make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::list(vec![])))),
    ])
}

// ── sched module ──

pub fn create_sched_module() -> PyObjectRef {
    let scheduler_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("scheduler"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("enter"), make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("run"), make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("empty"), make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(true))));
            w.insert(CompactString::from("queue"), PyObject::list(vec![]));
        }
        Ok(inst)
    });
    make_module("sched", vec![("scheduler", scheduler_fn)])
}


// ── mmap module ──

pub fn create_mmap_module() -> PyObjectRef {
    // mmap.mmap(fileno, length, ...) → mmap object (simplified: backed by Vec<u8>)
    let mmap_fn = make_builtin(|args: &[PyObjectRef]| {
        let _fileno = if !args.is_empty() { args[0].to_int().unwrap_or(-1) } else { -1 };
        let length = if args.len() > 1 { args[1].to_int().unwrap_or(0) as usize } else { 0 };

        let data: Arc<RwLock<Vec<u8>>> = Arc::new(RwLock::new(vec![0u8; length]));
        let pos: Arc<RwLock<usize>> = Arc::new(RwLock::new(0));
        let cls = PyObject::class(CompactString::from("mmap"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            
            // read(n)
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

            // write(data)
            let d3 = data.clone();
            let p3 = pos.clone();
            w.insert(CompactString::from("write"), PyObject::native_closure("write", move |args| {
                if args.is_empty() { return Err(PyException::type_error("write requires bytes")); }
                if let PyObjectPayload::Bytes(b) = &args[0].payload {
                    let mut d = d3.write();
                    let mut p = p3.write();
                    let start = *p;
                    for (i, &byte) in b.iter().enumerate() {
                        let idx = start + i;
                        if idx < d.len() {
                            d[idx] = byte;
                        } else {
                            d.push(byte);
                        }
                    }
                    *p = start + b.len();
                    Ok(PyObject::int(b.len() as i64))
                } else {
                    Err(PyException::type_error("write requires bytes argument"))
                }
            }));

            // seek(pos)
            let p4 = pos.clone();
            w.insert(CompactString::from("seek"), PyObject::native_closure("seek", move |args| {
                if !args.is_empty() {
                    let new_pos = args[0].to_int().unwrap_or(0) as usize;
                    *p4.write() = new_pos;
                }
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

            // close()
            w.insert(CompactString::from("close"), make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())));

            // __len__
            let d7 = data.clone();
            w.insert(CompactString::from("__len__"), PyObject::native_closure("__len__", move |_args| {
                Ok(PyObject::int(d7.read().len() as i64))
            }));

            // __getitem__ (indexing)
            let d8 = data.clone();
            w.insert(CompactString::from("__getitem__"), PyObject::native_closure("__getitem__", move |args| {
                if args.is_empty() { return Err(PyException::index_error("mmap index out of range")); }
                let idx = args[0].to_int().unwrap_or(0) as usize;
                let d = d8.read();
                if idx < d.len() {
                    Ok(PyObject::int(d[idx] as i64))
                } else {
                    Err(PyException::index_error("mmap index out of range"))
                }
            }));

            // __enter__ / __exit__ for context manager
            let inst_ref = inst.clone();
            w.insert(CompactString::from("__enter__"), PyObject::native_closure("__enter__", move |_| Ok(inst_ref.clone())));
            w.insert(CompactString::from("__exit__"), make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(false))));
        }
        Ok(inst)
    });

    make_module("mmap", vec![
        ("mmap", mmap_fn),
        ("ACCESS_READ", PyObject::int(1)),
        ("ACCESS_WRITE", PyObject::int(2)),
        ("ACCESS_COPY", PyObject::int(3)),
        ("PAGESIZE", PyObject::int(4096)),
    ])
}

// ── resource module (unix) ──

pub fn create_resource_module() -> PyObjectRef {
    let getrlimit_fn = make_builtin(|args: &[PyObjectRef]| {
        let _resource = if !args.is_empty() { args[0].to_int().unwrap_or(0) } else { 0 };
        // Return (soft_limit, hard_limit) — use -1 for unlimited
        Ok(PyObject::tuple(vec![PyObject::int(-1), PyObject::int(-1)]))
    });

    let setrlimit_fn = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));

    let getrusage_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("struct_rusage"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("ru_utime"), PyObject::float(0.0));
        attrs.insert(CompactString::from("ru_stime"), PyObject::float(0.0));
        attrs.insert(CompactString::from("ru_maxrss"), PyObject::int(0));
        attrs.insert(CompactString::from("ru_minflt"), PyObject::int(0));
        attrs.insert(CompactString::from("ru_majflt"), PyObject::int(0));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    make_module("resource", vec![
        ("getrlimit", getrlimit_fn),
        ("setrlimit", setrlimit_fn),
        ("getrusage", getrusage_fn),
        ("RLIMIT_CPU", PyObject::int(0)),
        ("RLIMIT_FSIZE", PyObject::int(1)),
        ("RLIMIT_DATA", PyObject::int(2)),
        ("RLIMIT_STACK", PyObject::int(3)),
        ("RLIMIT_CORE", PyObject::int(4)),
        ("RLIMIT_RSS", PyObject::int(5)),
        ("RLIMIT_NOFILE", PyObject::int(7)),
        ("RLIMIT_AS", PyObject::int(9)),
        ("RUSAGE_SELF", PyObject::int(0)),
        ("RUSAGE_CHILDREN", PyObject::int(-1)),
    ])
}

pub fn create_fcntl_module() -> PyObjectRef {
    let fcntl_fn = make_builtin(|args: &[PyObjectRef]| {
        let _ = check_args("fcntl", args, 2);
        // Return 0 (success) — stub for file control operations
        Ok(PyObject::int(0))
    });

    let flock_fn = make_builtin(|args: &[PyObjectRef]| {
        let _ = check_args("flock", args, 2);
        Ok(PyObject::none())
    });

    let lockf_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Err(PyException::type_error(CompactString::from("lockf() requires at least 2 args"))); }
        Ok(PyObject::none())
    });

    let ioctl_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 { return Err(PyException::type_error(CompactString::from("ioctl() requires at least 2 args"))); }
        Ok(PyObject::int(0))
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
