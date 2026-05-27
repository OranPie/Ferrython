use super::*;

mod environ;
mod fs_ops;
mod pathlike;
mod permissions;
mod process;
mod stat;
mod terminal;
mod walk;

use environ::create_environ_object;
use fs_ops::{
    os_chdir, os_getcwd, os_listdir, os_makedirs, os_mkdir, os_remove, os_removedirs, os_rename,
    os_replace, os_rmdir,
};
use pathlike::{create_pathlike_class, os_fspath};
use permissions::{os_chmod, os_chown, os_isatty, os_readlink, os_symlink};
use process::{os_cpu_count, os_getenv, os_getpid, os_popen, os_system};
use stat::{build_stat_result_from_meta, os_scandir, os_stat};
use terminal::make_terminal_size_class;
pub use terminal::make_terminal_size_instance;
use walk::os_walk;

// ── os module ──

pub fn create_os_module() -> PyObjectRef {
    make_module(
        "os",
        vec![
            (
                "name",
                PyObject::str_val(CompactString::from(if cfg!(windows) {
                    "nt"
                } else {
                    "posix"
                })),
            ),
            (
                "sep",
                PyObject::str_val(CompactString::from(std::path::MAIN_SEPARATOR.to_string())),
            ),
            (
                "linesep",
                PyObject::str_val(CompactString::from(if cfg!(windows) {
                    "\r\n"
                } else {
                    "\n"
                })),
            ),
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
            ("replace", make_builtin(os_replace)),
            ("path", create_os_path_module()),
            ("getenv", make_builtin(os_getenv)),
            ("environ", create_environ_object()),
            (
                "_Environ",
                PyObject::class(CompactString::from("_Environ"), vec![], IndexMap::new()),
            ),
            ("cpu_count", make_builtin(os_cpu_count)),
            ("getpid", make_builtin(os_getpid)),
            ("fspath", PyObject::native_function("os.fspath", os_fspath)),
            ("PathLike", create_pathlike_class()),
            ("walk", make_builtin(os_walk)),
            ("stat", make_builtin(os_stat)),
            ("chmod", make_builtin(os_chmod)),
            ("chown", make_builtin(os_chown)),
            ("symlink", make_builtin(os_symlink)),
            ("readlink", make_builtin(os_readlink)),
            ("isatty", make_builtin(os_isatty)),
            ("chdir", make_builtin(os_chdir)),
            ("system", make_builtin(os_system)),
            ("popen", make_builtin(os_popen)),
            (
                "getppid",
                make_builtin(|_| {
                    Ok(PyObject::int(std::process::id() as i64)) // Approximate with current PID
                }),
            ),
            (
                "urandom",
                make_builtin(|args| {
                    let n = if args.is_empty() {
                        16
                    } else {
                        args[0].as_int().unwrap_or(16) as usize
                    };
                    let mut buf = vec![0u8; n];
                    #[cfg(unix)]
                    {
                        use std::io::Read;
                        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
                            let _ = f.read_exact(&mut buf);
                        }
                    }
                    Ok(PyObject::bytes(buf))
                }),
            ),
            (
                "access",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let path = args[0].py_to_string();
                    Ok(PyObject::bool_val(std::path::Path::new(&path).exists()))
                }),
            ),
            ("umask", make_builtin(|_| Ok(PyObject::int(0o022)))),
            (
                "getlogin",
                make_builtin(|_| {
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
                }),
            ),
            (
                "devnull",
                PyObject::str_val(CompactString::from(if cfg!(windows) {
                    "nul"
                } else {
                    "/dev/null"
                })),
            ),
            ("F_OK", PyObject::int(0)),
            ("R_OK", PyObject::int(4)),
            ("W_OK", PyObject::int(2)),
            ("X_OK", PyObject::int(1)),
            ("O_RDONLY", PyObject::int(0)),
            ("O_WRONLY", PyObject::int(1)),
            ("O_RDWR", PyObject::int(2)),
            ("O_CREAT", PyObject::int(0o100)),
            ("O_EXCL", PyObject::int(0o200)),
            ("O_NOCTTY", PyObject::int(0o400)),
            ("O_TRUNC", PyObject::int(0o1000)),
            ("O_APPEND", PyObject::int(0o2000)),
            ("O_NONBLOCK", PyObject::int(0o4000)),
            ("O_CLOEXEC", PyObject::int(0o2000000)),
            ("SEEK_SET", PyObject::int(0)),
            ("SEEK_CUR", PyObject::int(1)),
            ("SEEK_END", PyObject::int(2)),
            (
                "strerror",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("strerror requires an error code"));
                    }
                    let code = args[0].as_int().unwrap_or(0) as i32;
                    #[cfg(unix)]
                    {
                        let msg = unsafe {
                            let p = libc::strerror(code);
                            if p.is_null() {
                                "Unknown error".to_string()
                            } else {
                                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
                            }
                        };
                        Ok(PyObject::str_val(CompactString::from(msg)))
                    }
                    #[cfg(not(unix))]
                    {
                        Ok(PyObject::str_val(CompactString::from(format!(
                            "Error {}",
                            code
                        ))))
                    }
                }),
            ),
            ("scandir", make_builtin(os_scandir)),
            (
                "putenv",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("putenv requires 2 arguments"));
                    }
                    let key = args[0].py_to_string();
                    let val = args[1].py_to_string();
                    // Safety: we ensure no concurrent modification of env in this single-threaded interpreter
                    unsafe {
                        std::env::set_var(&key, &val);
                    }
                    Ok(PyObject::none())
                }),
            ),
            (
                "unsetenv",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("unsetenv requires 1 argument"));
                    }
                    let key = args[0].py_to_string();
                    unsafe {
                        std::env::remove_var(&key);
                    }
                    Ok(PyObject::none())
                }),
            ),
            (
                "lstat",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("os.lstat requires path"));
                    }
                    let path = args[0].py_to_string();
                    let meta = std::fs::symlink_metadata(&path)
                        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
                    crate::fs_modules::build_stat_result(meta)
                }),
            ),
            (
                "expanduser",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("expanduser requires path"));
                    }
                    let path = args[0].py_to_string();
                    if path.starts_with("~/") || path == "~" {
                        if let Ok(home) = std::env::var("HOME") {
                            let expanded = if path == "~" {
                                home
                            } else {
                                format!("{}{}", home, &path[1..])
                            };
                            return Ok(PyObject::str_val(CompactString::from(expanded)));
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(path)))
                }),
            ),
            // Unix ID functions
            (
                "getuid",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        Ok(PyObject::int(unsafe { libc::getuid() } as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::os_error(
                            "getuid() is not supported on this platform",
                        ))
                    }
                }),
            ),
            (
                "getgid",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        Ok(PyObject::int(unsafe { libc::getgid() } as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::os_error(
                            "getgid() is not supported on this platform",
                        ))
                    }
                }),
            ),
            (
                "geteuid",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        Ok(PyObject::int(unsafe { libc::geteuid() } as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::os_error(
                            "geteuid() is not supported on this platform",
                        ))
                    }
                }),
            ),
            (
                "getegid",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        Ok(PyObject::int(unsafe { libc::getegid() } as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::os_error(
                            "getegid() is not supported on this platform",
                        ))
                    }
                }),
            ),
            (
                "getppid",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        Ok(PyObject::int(unsafe { libc::getppid() } as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::os_error(
                            "getppid() is not supported on this platform",
                        ))
                    }
                }),
            ),
            // Process management
            (
                "kill",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("os.kill requires pid and signal"));
                    }
                    let pid = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("pid must be int"))?;
                    let sig = args[1]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("signal must be int"))?;
                    #[cfg(unix)]
                    {
                        let ret = unsafe { libc::kill(pid as i32, sig as i32) };
                        if ret != 0 {
                            return Err(PyException::os_error(format!(
                                "kill failed: errno {}",
                                ret
                            )));
                        }
                    }
                    Ok(PyObject::none())
                }),
            ),
            // File operations
            (
                "link",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("os.link requires src and dst"));
                    }
                    std::fs::hard_link(args[0].py_to_string(), args[1].py_to_string())
                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                    Ok(PyObject::none())
                }),
            ),
            (
                "truncate",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "os.truncate requires path and length",
                        ));
                    }
                    let path = args[0].py_to_string();
                    let length = args[1].as_int().unwrap_or(0) as u64;
                    let f = std::fs::OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
                    f.set_len(length)
                        .map_err(|e| PyException::os_error(format!("{}", e)))?;
                    Ok(PyObject::none())
                }),
            ),
            // Pipe and fd operations
            (
                "pipe",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        let mut fds = [0i32; 2];
                        let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
                        if ret != 0 {
                            return Err(PyException::os_error("pipe() failed".to_string()));
                        }
                        Ok(PyObject::tuple(vec![
                            PyObject::int(fds[0] as i64),
                            PyObject::int(fds[1] as i64),
                        ]))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::not_implemented_error(
                            "os.pipe not available on this platform",
                        ))
                    }
                }),
            ),
            (
                "dup",
                make_builtin(|args| {
                    check_args("os.dup", args, 1)?;
                    let fd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?;
                    #[cfg(unix)]
                    {
                        let new_fd = unsafe { libc::dup(fd as i32) };
                        if new_fd < 0 {
                            return Err(PyException::os_error("dup() failed".to_string()));
                        }
                        Ok(PyObject::int(new_fd as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::not_implemented_error("os.dup not available"))
                    }
                }),
            ),
            (
                "dup2",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("os.dup2 requires oldfd and newfd"));
                    }
                    let oldfd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?;
                    let newfd = args[1]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?;
                    #[cfg(unix)]
                    {
                        let ret = unsafe { libc::dup2(oldfd as i32, newfd as i32) };
                        if ret < 0 {
                            return Err(PyException::os_error("dup2() failed".to_string()));
                        }
                        Ok(PyObject::int(ret as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::not_implemented_error("os.dup2 not available"))
                    }
                }),
            ),
            // terminal_size class exposed on the os module
            ("terminal_size", make_terminal_size_class()),
            // Terminal/system info
            (
                "get_terminal_size",
                make_builtin(|_| {
                    // Default fallback; real implementation would use ioctl
                    let cols = std::env::var("COLUMNS")
                        .ok()
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(80);
                    let lines = std::env::var("LINES")
                        .ok()
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(24);
                    Ok(make_terminal_size_instance(cols, lines))
                }),
            ),
            (
                "uname",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        let mut info: libc::utsname = unsafe { std::mem::zeroed() };
                        unsafe {
                            libc::uname(&mut info);
                        }
                        let to_str = |arr: &[i8]| -> String {
                            let bytes: Vec<u8> = arr
                                .iter()
                                .take_while(|&&c| c != 0)
                                .map(|&c| c as u8)
                                .collect();
                            String::from_utf8_lossy(&bytes).to_string()
                        };
                        let cls = PyObject::class(
                            CompactString::from("uname_result"),
                            vec![],
                            IndexMap::new(),
                        );
                        let inst = PyObject::instance(cls);
                        if let PyObjectPayload::Instance(ref data) = inst.payload {
                            let mut attrs = data.attrs.write();
                            attrs.insert(
                                CompactString::from("sysname"),
                                PyObject::str_val(CompactString::from(to_str(&info.sysname))),
                            );
                            attrs.insert(
                                CompactString::from("nodename"),
                                PyObject::str_val(CompactString::from(to_str(&info.nodename))),
                            );
                            attrs.insert(
                                CompactString::from("release"),
                                PyObject::str_val(CompactString::from(to_str(&info.release))),
                            );
                            attrs.insert(
                                CompactString::from("version"),
                                PyObject::str_val(CompactString::from(to_str(&info.version))),
                            );
                            attrs.insert(
                                CompactString::from("machine"),
                                PyObject::str_val(CompactString::from(to_str(&info.machine))),
                            );
                        }
                        Ok(inst)
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::not_implemented_error(
                            "os.uname not available on this platform",
                        ))
                    }
                }),
            ),
            (
                "times",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        let mut tms: libc::tms = unsafe { std::mem::zeroed() };
                        unsafe {
                            libc::times(&mut tms);
                        }
                        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
                        Ok(PyObject::tuple(vec![
                            PyObject::float(tms.tms_utime as f64 / ticks),
                            PyObject::float(tms.tms_stime as f64 / ticks),
                            PyObject::float(tms.tms_cutime as f64 / ticks),
                            PyObject::float(tms.tms_cstime as f64 / ticks),
                            PyObject::float(0.0), // elapsed
                        ]))
                    }
                    #[cfg(not(unix))]
                    {
                        Ok(PyObject::tuple(vec![PyObject::float(0.0); 5]))
                    }
                }),
            ),
            // Path constants
            (
                "pathsep",
                PyObject::str_val(CompactString::from(if cfg!(windows) { ";" } else { ":" })),
            ),
            ("altsep", PyObject::none()),
            // Low-level file descriptor operations
            (
                "close",
                make_builtin(|args| {
                    check_args("os.close", args, 1)?;
                    let fd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?
                        as i32;
                    #[cfg(unix)]
                    {
                        let ret = unsafe { libc::close(fd) };
                        if ret != 0 {
                            return Err(PyException::os_error(format!(
                                "Bad file descriptor: {}",
                                fd
                            )));
                        }
                    }
                    Ok(PyObject::none())
                }),
            ),
            (
                "open",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "os.open requires path, flags, and optional mode",
                        ));
                    }
                    let path = args[0].py_to_string();
                    let flags = if args.len() > 1 {
                        args[1].as_int().unwrap_or(0) as i32
                    } else {
                        0
                    };
                    let mode = if args.len() > 2 {
                        args[2].as_int().unwrap_or(0o666) as u32
                    } else {
                        0o666
                    };
                    #[cfg(unix)]
                    {
                        let cpath = std::ffi::CString::new(path.as_str())
                            .map_err(|_| PyException::value_error("invalid path"))?;
                        let fd = unsafe { libc::open(cpath.as_ptr(), flags, mode) };
                        if fd < 0 {
                            return Err(PyException::os_error(format!(
                                "No such file or directory: '{}'",
                                path
                            )));
                        }
                        Ok(PyObject::int(fd as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::not_implemented_error("os.open not available"))
                    }
                }),
            ),
            (
                "read",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("os.read requires fd and count"));
                    }
                    let fd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?
                        as i32;
                    let count = args[1]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("count must be int"))?
                        as usize;
                    #[cfg(unix)]
                    {
                        let mut buf = vec![0u8; count];
                        let n =
                            unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, count) };
                        if n < 0 {
                            return Err(PyException::os_error("read failed".to_string()));
                        }
                        buf.truncate(n as usize);
                        Ok(PyObject::bytes(buf))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::not_implemented_error("os.read not available"))
                    }
                }),
            ),
            (
                "write",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("os.write requires fd and data"));
                    }
                    let fd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?
                        as i32;
                    let data = match &args[1].payload {
                        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
                        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                        _ => return Err(PyException::type_error("data must be bytes-like")),
                    };
                    #[cfg(unix)]
                    {
                        let n = unsafe {
                            libc::write(fd, data.as_ptr() as *const libc::c_void, data.len())
                        };
                        if n < 0 {
                            return Err(PyException::os_error("write failed".to_string()));
                        }
                        Ok(PyObject::int(n as i64))
                    }
                    #[cfg(not(unix))]
                    {
                        Err(PyException::not_implemented_error("os.write not available"))
                    }
                }),
            ),
            (
                "fdopen",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("os.fdopen requires fd"));
                    }
                    let fd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?
                        as i32;
                    let mode = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        "r".to_string()
                    };
                    #[cfg(unix)]
                    {
                        let is_binary = mode.contains('b');
                        // State: (fd, closed, name)
                        let state = Rc::new(PyCell::new((fd, false)));
                        let mode_str = mode.clone();
                        let name_str = format!("<fdopen fd={}>", fd);
                        let mut attrs = IndexMap::new();
                        attrs.insert(
                            CompactString::from("mode"),
                            PyObject::str_val(CompactString::from(&mode_str)),
                        );
                        attrs.insert(
                            CompactString::from("name"),
                            PyObject::str_val(CompactString::from(&name_str)),
                        );
                        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
                        // read([size])
                        let s1 = state.clone();
                        let is_bin_r = is_binary;
                        attrs.insert(
                            CompactString::from("read"),
                            PyObject::native_closure("fdopen.read", move |a| {
                                let g = s1.read();
                                if g.1 {
                                    return Err(PyException::value_error(
                                        "I/O operation on closed file",
                                    ));
                                }
                                let fd = g.0;
                                drop(g);
                                let size = if !a.is_empty() && a.len() > 1 {
                                    a[1].as_int().unwrap_or(-1) as isize
                                } else if !a.is_empty() {
                                    a[0].as_int().unwrap_or(-1) as isize
                                } else {
                                    -1isize
                                };
                                let buf = if size < 0 {
                                    // Read all
                                    let mut buf = Vec::new();
                                    let mut tmp = [0u8; 8192];
                                    loop {
                                        let n = unsafe {
                                            libc::read(
                                                fd,
                                                tmp.as_mut_ptr() as *mut libc::c_void,
                                                tmp.len(),
                                            )
                                        };
                                        if n <= 0 {
                                            break;
                                        }
                                        buf.extend_from_slice(&tmp[..n as usize]);
                                    }
                                    buf
                                } else {
                                    let mut buf = vec![0u8; size as usize];
                                    let n = unsafe {
                                        libc::read(
                                            fd,
                                            buf.as_mut_ptr() as *mut libc::c_void,
                                            buf.len(),
                                        )
                                    };
                                    if n < 0 {
                                        return Err(PyException::os_error(
                                            "read failed".to_string(),
                                        ));
                                    }
                                    buf.truncate(n as usize);
                                    buf
                                };
                                if is_bin_r {
                                    Ok(PyObject::bytes(buf))
                                } else {
                                    Ok(PyObject::str_val(CompactString::from(
                                        String::from_utf8_lossy(&buf).as_ref(),
                                    )))
                                }
                            }),
                        );
                        // write(data)
                        let s2 = state.clone();
                        attrs.insert(
                            CompactString::from("write"),
                            PyObject::native_closure("fdopen.write", move |a| {
                                let g = s2.read();
                                if g.1 {
                                    return Err(PyException::value_error(
                                        "I/O operation on closed file",
                                    ));
                                }
                                let fd = g.0;
                                drop(g);
                                if a.is_empty()
                                    || (a.len() == 1
                                        && matches!(a[0].payload, PyObjectPayload::Instance(_)))
                                {
                                    return Err(PyException::type_error("write requires data"));
                                }
                                let data_arg = if a.len() > 1 { &a[1] } else { &a[0] };
                                let data_bytes = match &data_arg.payload {
                                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                                        (**b).clone()
                                    }
                                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                                    _ => {
                                        return Err(PyException::type_error(
                                            "write requires str or bytes",
                                        ))
                                    }
                                };
                                let n = unsafe {
                                    libc::write(
                                        fd,
                                        data_bytes.as_ptr() as *const libc::c_void,
                                        data_bytes.len(),
                                    )
                                };
                                if n < 0 {
                                    return Err(PyException::os_error("write failed".to_string()));
                                }
                                Ok(PyObject::int(n as i64))
                            }),
                        );
                        // seek(offset, whence=0)
                        let s3 = state.clone();
                        attrs.insert(
                            CompactString::from("seek"),
                            PyObject::native_closure("fdopen.seek", move |a| {
                                let g = s3.read();
                                if g.1 {
                                    return Err(PyException::value_error(
                                        "I/O operation on closed file",
                                    ));
                                }
                                let fd = g.0;
                                drop(g);
                                let offset = if a.len() > 1 {
                                    a[1].as_int().unwrap_or(0) as i64
                                } else if !a.is_empty() {
                                    a[0].as_int().unwrap_or(0) as i64
                                } else {
                                    0i64
                                };
                                let whence = if a.len() > 2 {
                                    a[2].as_int().unwrap_or(0) as i32
                                } else {
                                    0i32
                                };
                                let pos = unsafe { libc::lseek(fd, offset as libc::off_t, whence) };
                                if pos < 0 {
                                    return Err(PyException::os_error("seek failed".to_string()));
                                }
                                Ok(PyObject::int(pos as i64))
                            }),
                        );
                        // tell()
                        let s4 = state.clone();
                        attrs.insert(
                            CompactString::from("tell"),
                            PyObject::native_closure("fdopen.tell", move |_a| {
                                let g = s4.read();
                                if g.1 {
                                    return Err(PyException::value_error(
                                        "I/O operation on closed file",
                                    ));
                                }
                                let fd = g.0;
                                drop(g);
                                let pos = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) };
                                Ok(PyObject::int(pos as i64))
                            }),
                        );
                        // flush()
                        let s5 = state.clone();
                        attrs.insert(
                            CompactString::from("flush"),
                            PyObject::native_closure("fdopen.flush", move |_a| {
                                let g = s5.read();
                                if g.1 {
                                    return Err(PyException::value_error(
                                        "I/O operation on closed file",
                                    ));
                                }
                                let fd = g.0;
                                drop(g);
                                unsafe {
                                    libc::fsync(fd);
                                }
                                Ok(PyObject::none())
                            }),
                        );
                        // close()
                        let s6 = state.clone();
                        attrs.insert(
                            CompactString::from("close"),
                            PyObject::native_closure("fdopen.close", move |_| {
                                let mut g = s6.write();
                                if !g.1 {
                                    g.1 = true;
                                    unsafe {
                                        libc::close(g.0);
                                    }
                                }
                                Ok(PyObject::none())
                            }),
                        );
                        // __enter__(self) -> self
                        attrs.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_closure("fdopen.__enter__", move |a| {
                                if a.is_empty() {
                                    return Ok(PyObject::none());
                                }
                                Ok(a[0].clone())
                            }),
                        );
                        // __exit__ -> close
                        let s7 = state.clone();
                        attrs.insert(
                            CompactString::from("__exit__"),
                            PyObject::native_closure("fdopen.__exit__", move |_| {
                                let mut g = s7.write();
                                if !g.1 {
                                    g.1 = true;
                                    unsafe {
                                        libc::close(g.0);
                                    }
                                }
                                Ok(PyObject::bool_val(false))
                            }),
                        );
                        // Return as an Instance so it's treated as a file-like object
                        let class = PyObject::class(
                            CompactString::from("_io.FileIO"),
                            vec![],
                            IndexMap::new(),
                        );
                        Ok(PyObject::instance_with_attrs(class, attrs))
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = (fd, mode);
                        Err(PyException::not_implemented_error(
                            "os.fdopen not available on this platform",
                        ))
                    }
                }),
            ),
            (
                "fstat",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("os.fstat requires fd"));
                    }
                    let fd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?
                        as i32;
                    #[cfg(unix)]
                    {
                        use std::os::unix::io::FromRawFd;
                        let file = unsafe { std::fs::File::from_raw_fd(fd) };
                        let meta = file
                            .metadata()
                            .map_err(|e| PyException::os_error(format!("{}", e)));
                        std::mem::forget(file);
                        let meta = meta?;
                        build_stat_result_from_meta(&meta)
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = fd;
                        Err(PyException::not_implemented_error(
                            "os.fstat not supported on this platform",
                        ))
                    }
                }),
            ),
            (
                "ftruncate",
                make_builtin(|args| {
                    check_args_min("os.ftruncate", args, 2)?;
                    let fd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?
                        as i32;
                    let length = args[1]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("length must be int"))?
                        as u64;
                    #[cfg(unix)]
                    {
                        use std::os::unix::io::FromRawFd;
                        let file = unsafe { std::fs::File::from_raw_fd(fd) };
                        let result = file
                            .set_len(length)
                            .map_err(|e| PyException::os_error(format!("{}", e)));
                        std::mem::forget(file);
                        result?;
                        Ok(PyObject::none())
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = (fd, length);
                        Err(PyException::not_implemented_error(
                            "os.ftruncate not supported on this platform",
                        ))
                    }
                }),
            ),
            (
                "lseek",
                make_builtin(|args| {
                    check_args_min("os.lseek", args, 3)?;
                    let fd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?
                        as i32;
                    let offset = args[1]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("offset must be int"))?
                        as i64;
                    let whence = args[2]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("whence must be int"))?
                        as i32;
                    #[cfg(unix)]
                    {
                        use std::io::Seek;
                        use std::os::unix::io::FromRawFd;
                        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
                        let seek_from = match whence {
                            0 => std::io::SeekFrom::Start(offset as u64),
                            1 => std::io::SeekFrom::Current(offset),
                            2 => std::io::SeekFrom::End(offset),
                            _ => {
                                std::mem::forget(file);
                                return Err(PyException::value_error("invalid whence"));
                            }
                        };
                        let result = file.seek(seek_from);
                        std::mem::forget(file);
                        match result {
                            Ok(pos) => Ok(PyObject::int(pos as i64)),
                            Err(e) => Err(PyException::os_error(format!("{}", e))),
                        }
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = (fd, offset, whence);
                        Err(PyException::not_implemented_error(
                            "os.lseek not supported on this platform",
                        ))
                    }
                }),
            ),
            (
                "fsync",
                make_builtin(|args| {
                    check_args_min("os.fsync", args, 1)?;
                    let fd = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("fd must be int"))?
                        as i32;
                    #[cfg(unix)]
                    {
                        use std::os::unix::io::FromRawFd;
                        let file = unsafe { std::fs::File::from_raw_fd(fd) };
                        let result = file
                            .sync_all()
                            .map_err(|e| PyException::os_error(format!("{}", e)));
                        std::mem::forget(file);
                        result?;
                        Ok(PyObject::none())
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = fd;
                        Err(PyException::not_implemented_error(
                            "os.fsync not supported on this platform",
                        ))
                    }
                }),
            ),
            (
                "stat_result",
                make_builtin(|_| {
                    Ok(PyObject::class(
                        CompactString::from("stat_result"),
                        vec![],
                        IndexMap::new(),
                    ))
                }),
            ),
            // waitpid and W* macros
            (
                "waitpid",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "os.waitpid requires pid and options",
                        ));
                    }
                    let pid = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("pid must be int"))?
                        as i32;
                    let options = args[1]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("options must be int"))?
                        as i32;
                    #[cfg(unix)]
                    {
                        let mut status: i32 = 0;
                        let ret = unsafe { libc::waitpid(pid, &mut status, options) };
                        if ret < 0 {
                            return Err(PyException::os_error("waitpid failed".to_string()));
                        }
                        Ok(PyObject::tuple(vec![
                            PyObject::int(ret as i64),
                            PyObject::int(status as i64),
                        ]))
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = (pid, options);
                        Err(PyException::not_implemented_error(
                            "os.waitpid not available",
                        ))
                    }
                }),
            ),
            ("WNOHANG", PyObject::int(1)),
            ("WUNTRACED", PyObject::int(2)),
            (
                "WIFEXITED",
                make_builtin(|args| {
                    check_args("os.WIFEXITED", args, 1)?;
                    let status = args[0].as_int().unwrap_or(0) as i32;
                    Ok(PyObject::bool_val(libc::WIFEXITED(status)))
                }),
            ),
            (
                "WEXITSTATUS",
                make_builtin(|args| {
                    check_args("os.WEXITSTATUS", args, 1)?;
                    let status = args[0].as_int().unwrap_or(0) as i32;
                    Ok(PyObject::int(libc::WEXITSTATUS(status) as i64))
                }),
            ),
            (
                "WIFSIGNALED",
                make_builtin(|args| {
                    check_args("os.WIFSIGNALED", args, 1)?;
                    let status = args[0].as_int().unwrap_or(0) as i32;
                    Ok(PyObject::bool_val(libc::WIFSIGNALED(status)))
                }),
            ),
            (
                "WTERMSIG",
                make_builtin(|args| {
                    check_args("os.WTERMSIG", args, 1)?;
                    let status = args[0].as_int().unwrap_or(0) as i32;
                    Ok(PyObject::int(libc::WTERMSIG(status) as i64))
                }),
            ),
            (
                "WIFSTOPPED",
                make_builtin(|args| {
                    check_args("os.WIFSTOPPED", args, 1)?;
                    let status = args[0].as_int().unwrap_or(0) as i32;
                    Ok(PyObject::bool_val(libc::WIFSTOPPED(status)))
                }),
            ),
            (
                "WSTOPSIG",
                make_builtin(|args| {
                    check_args("os.WSTOPSIG", args, 1)?;
                    let status = args[0].as_int().unwrap_or(0) as i32;
                    Ok(PyObject::int(libc::WSTOPSIG(status) as i64))
                }),
            ),
            (
                "fsencode",
                make_builtin(|args| {
                    check_args("os.fsencode", args, 1)?;
                    let s = args[0].py_to_string();
                    Ok(PyObject::bytes(s.into_bytes()))
                }),
            ),
            (
                "fsdecode",
                make_builtin(|args| {
                    check_args("os.fsdecode", args, 1)?;
                    match &args[0].payload {
                        PyObjectPayload::Bytes(b) => {
                            let s = String::from_utf8_lossy(b).to_string();
                            Ok(PyObject::str_val(CompactString::from(s)))
                        }
                        _ => Ok(PyObject::str_val(CompactString::from(
                            args[0].py_to_string(),
                        ))),
                    }
                }),
            ),
        ],
    )
}
