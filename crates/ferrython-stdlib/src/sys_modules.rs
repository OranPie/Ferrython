//! System, OS, and platform stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, ClassData,
    IteratorData, CompareOp, InstanceData,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::{Arc, Mutex};

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
        ("modules", PyObject::dict_from_pairs(vec![])),
        ("maxsize", PyObject::int(i64::MAX)),
        ("maxunicode", PyObject::int(0x10FFFF)),
        ("byteorder", PyObject::str_val(CompactString::from(if cfg!(target_endian = "little") { "little" } else { "big" }))),
        ("prefix", PyObject::str_val(CompactString::from("/usr/local"))),
        ("exec_prefix", PyObject::str_val(CompactString::from("/usr/local"))),
        ("implementation", PyObject::str_val(CompactString::from("ferrython"))),
        ("stdin", PyObject::str_val(CompactString::from("<stdin>"))),
        ("stdout", PyObject::str_val(CompactString::from("<stdout>"))),
        ("stderr", PyObject::str_val(CompactString::from("<stderr>"))),
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
    ])
}

fn sys_getrecursionlimit(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(1000))
}
fn sys_setrecursionlimit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.setrecursionlimit", args, 1)?;
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
fn os_path_stub(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(create_os_path_module())
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

fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
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
        ("sep", PyObject::str_val(CompactString::from(std::path::MAIN_SEPARATOR.to_string()))),
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
    let p = std::path::Path::new(&s);
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
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
    let p = std::path::Path::new(&s);
    let dir = p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(dir)),
        PyObject::str_val(CompactString::from(name)),
    ]))
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


