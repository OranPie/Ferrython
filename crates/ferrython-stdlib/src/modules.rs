//! Standard library module creation functions.

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

// ── math module ──

pub fn create_math_module() -> PyObjectRef {
    make_module("math", vec![
        ("pi", PyObject::float(std::f64::consts::PI)),
        ("e", PyObject::float(std::f64::consts::E)),
        ("tau", PyObject::float(std::f64::consts::TAU)),
        ("inf", PyObject::float(f64::INFINITY)),
        ("nan", PyObject::float(f64::NAN)),
        ("sqrt", make_builtin(math_sqrt)),
        ("ceil", make_builtin(math_ceil)),
        ("floor", make_builtin(math_floor)),
        ("abs", make_builtin(math_fabs)),
        ("fabs", make_builtin(math_fabs)),
        ("pow", make_builtin(math_pow)),
        ("log", make_builtin(math_log)),
        ("log2", make_builtin(math_log2)),
        ("log10", make_builtin(math_log10)),
        ("exp", make_builtin(math_exp)),
        ("sin", make_builtin(math_sin)),
        ("cos", make_builtin(math_cos)),
        ("tan", make_builtin(math_tan)),
        ("asin", make_builtin(math_asin)),
        ("acos", make_builtin(math_acos)),
        ("atan", make_builtin(math_atan)),
        ("atan2", make_builtin(math_atan2)),
        ("degrees", make_builtin(math_degrees)),
        ("radians", make_builtin(math_radians)),
        ("isnan", make_builtin(math_isnan)),
        ("isinf", make_builtin(math_isinf)),
        ("isfinite", make_builtin(math_isfinite)),
        ("gcd", make_builtin(math_gcd)),
        ("factorial", make_builtin(math_factorial)),
        ("trunc", make_builtin(math_trunc)),
        ("copysign", make_builtin(math_copysign)),
        ("hypot", make_builtin(math_hypot)),
        ("modf", make_builtin(math_modf)),
        ("fmod", make_builtin(math_fmod)),
        ("frexp", make_builtin(math_frexp)),
        ("ldexp", make_builtin(math_ldexp)),
    ])
}

fn math_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sqrt", args, 1)?;
    let x = args[0].to_float()?;
    if x < 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x.sqrt()))
}
fn math_ceil(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.ceil", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.ceil() as i64))
}
fn math_floor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.floor", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.floor() as i64))
}
fn math_fabs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fabs", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.abs()))
}
fn math_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.pow", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.powf(args[1].to_float()?)))
}
fn math_log(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("math.log requires at least 1 argument")); }
    let x = args[0].to_float()?;
    if x <= 0.0 { return Err(PyException::value_error("math domain error")); }
    if args.len() > 1 {
        let base = args[1].to_float()?;
        Ok(PyObject::float(x.ln() / base.ln()))
    } else {
        Ok(PyObject::float(x.ln()))
    }
}
fn math_log2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log2", args, 1)?;
    let x = args[0].to_float()?;
    if x <= 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x.log2()))
}
fn math_log10(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log10", args, 1)?;
    let x = args[0].to_float()?;
    if x <= 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x.log10()))
}
fn math_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.exp", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.exp()))
}
fn math_sin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sin", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.sin()))
}
fn math_cos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.cos", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.cos()))
}
fn math_tan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.tan", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.tan()))
}
fn math_asin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.asin", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.asin()))
}
fn math_acos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.acos", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.acos()))
}
fn math_atan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.atan()))
}
fn math_atan2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan2", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.atan2(args[1].to_float()?)))
}
fn math_degrees(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.degrees", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.to_degrees()))
}
fn math_radians(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.radians", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.to_radians()))
}
fn math_isnan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isnan", args, 1)?;
    Ok(PyObject::bool_val(args[0].to_float()?.is_nan()))
}
fn math_isinf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isinf", args, 1)?;
    Ok(PyObject::bool_val(args[0].to_float()?.is_infinite()))
}
fn math_isfinite(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isfinite", args, 1)?;
    Ok(PyObject::bool_val(args[0].to_float()?.is_finite()))
}
fn math_gcd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.gcd", args, 2)?;
    let mut a = args[0].to_int()?.abs();
    let mut b = args[1].to_int()?.abs();
    while b != 0 { let t = b; b = a % b; a = t; }
    Ok(PyObject::int(a))
}
fn math_factorial(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.factorial", args, 1)?;
    let n = args[0].to_int()?;
    if n < 0 { return Err(PyException::value_error("factorial() not defined for negative values")); }
    let mut result: i64 = 1;
    for i in 2..=n {
        result = result.checked_mul(i).ok_or_else(|| PyException::overflow_error("factorial result too large"))?;
    }
    Ok(PyObject::int(result))
}
fn math_trunc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.trunc", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.trunc() as i64))
}
fn math_copysign(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.copysign", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.copysign(args[1].to_float()?)))
}
fn math_hypot(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.hypot", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.hypot(args[1].to_float()?)))
}
fn math_modf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.modf", args, 1)?;
    let x = args[0].to_float()?;
    let fract = x.fract();
    let trunc = x.trunc();
    Ok(PyObject::tuple(vec![PyObject::float(fract), PyObject::float(trunc)]))
}
fn math_fmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fmod", args, 2)?;
    Ok(PyObject::float(args[0].to_float()? % args[1].to_float()?))
}
fn math_frexp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.frexp", args, 1)?;
    let (m, e) = frexp(args[0].to_float()?);
    Ok(PyObject::tuple(vec![PyObject::float(m), PyObject::int(e as i64)]))
}
fn math_ldexp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.ldexp", args, 2)?;
    let x = args[0].to_float()?;
    let i = args[1].to_int()? as i32;
    Ok(PyObject::float(x * (2.0f64).powi(i)))
}

fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 { return (0.0, 0); }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1022;
    let mantissa = f64::from_bits((bits & 0x800FFFFFFFFFFFFF) | 0x3FE0000000000000);
    (mantissa, exp)
}

// ── sys module ──

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
        ("path", PyObject::list(vec![
            PyObject::str_val(CompactString::from("")),
            PyObject::str_val(CompactString::from(".")),
        ])),
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
    ])
}

// ── json module (basic) ──

pub fn create_json_module() -> PyObjectRef {
    make_module("json", vec![
        ("dumps", make_builtin(json_dumps)),
        ("loads", make_builtin(json_loads)),
    ])
}

fn json_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("json.dumps", args, 1)?;
    let s = py_to_json(&args[0])?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn py_to_json(obj: &PyObjectRef) -> PyResult<String> {
    match &obj.payload {
        PyObjectPayload::None => Ok("null".into()),
        PyObjectPayload::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        PyObjectPayload::Int(n) => Ok(n.to_string()),
        PyObjectPayload::Float(f) => {
            if f.is_nan() { return Err(PyException::value_error("NaN is not JSON serializable")); }
            if f.is_infinite() { return Err(PyException::value_error("Infinity is not JSON serializable")); }
            Ok(format!("{}", f))
        }
        PyObjectPayload::Str(s) => Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"))),
        PyObjectPayload::List(items) => {
            let r = items.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|i| py_to_json(i)).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        PyObjectPayload::Tuple(items) => {
            let parts: Result<Vec<String>, _> = items.iter().map(|i| py_to_json(i)).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|(k, v)| {
                let key_str = match k {
                    HashableKey::Str(s) => format!("\"{}\"", s),
                    HashableKey::Int(n) => format!("\"{}\"", n),
                    _ => return Err(PyException::type_error("keys must be str")),
                };
                let val_str = py_to_json(v)?;
                Ok(format!("{}: {}", key_str, val_str))
            }).collect();
            Ok(format!("{{{}}}", parts?.join(", ")))
        }
        _ => Err(PyException::type_error(format!("Object of type {} is not JSON serializable", obj.type_name()))),
    }
}

fn json_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("json.loads", args, 1)?;
    let s = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("json.loads requires a string")),
    };
    parse_json_value(&s, &mut 0)
}

fn parse_json_value(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    skip_ws(s, pos);
    if *pos >= s.len() { return Err(PyException::value_error("Unexpected end of JSON")); }
    let ch = s.as_bytes()[*pos] as char;
    match ch {
        '"' => parse_json_string(s, pos),
        't' | 'f' => parse_json_bool(s, pos),
        'n' => parse_json_null(s, pos),
        '[' => parse_json_array(s, pos),
        '{' => parse_json_object(s, pos),
        _ => parse_json_number(s, pos),
    }
}

fn skip_ws(s: &str, pos: &mut usize) {
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_whitespace() { *pos += 1; }
}

fn parse_json_string(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip "
    let mut result = String::new();
    while *pos < s.len() {
        let ch = s.as_bytes()[*pos] as char;
        if ch == '"' { *pos += 1; return Ok(PyObject::str_val(CompactString::from(result))); }
        if ch == '\\' {
            *pos += 1;
            if *pos >= s.len() { break; }
            let esc = s.as_bytes()[*pos] as char;
            match esc {
                'n' => result.push('\n'),
                't' => result.push('\t'),
                'r' => result.push('\r'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                _ => { result.push('\\'); result.push(esc); }
            }
        } else {
            result.push(ch);
        }
        *pos += 1;
    }
    Err(PyException::value_error("Unterminated string"))
}

fn parse_json_bool(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("true") { *pos += 4; return Ok(PyObject::bool_val(true)); }
    if s[*pos..].starts_with("false") { *pos += 5; return Ok(PyObject::bool_val(false)); }
    Err(PyException::value_error("Invalid JSON"))
}

fn parse_json_null(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("null") { *pos += 4; return Ok(PyObject::none()); }
    Err(PyException::value_error("Invalid JSON"))
}

fn parse_json_number(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    let start = *pos;
    let mut is_float = false;
    if *pos < s.len() && s.as_bytes()[*pos] == b'-' { *pos += 1; }
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    if *pos < s.len() && s.as_bytes()[*pos] == b'.' {
        is_float = true; *pos += 1;
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    }
    if *pos < s.len() && (s.as_bytes()[*pos] == b'e' || s.as_bytes()[*pos] == b'E') {
        is_float = true; *pos += 1;
        if *pos < s.len() && (s.as_bytes()[*pos] == b'+' || s.as_bytes()[*pos] == b'-') { *pos += 1; }
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    }
    let num_str = &s[start..*pos];
    if is_float {
        let f: f64 = num_str.parse().map_err(|_| PyException::value_error("Invalid number"))?;
        Ok(PyObject::float(f))
    } else {
        let i: i64 = num_str.parse().map_err(|_| PyException::value_error("Invalid number"))?;
        Ok(PyObject::int(i))
    }
}

fn parse_json_array(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip [
    let mut items = Vec::new();
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b']' { *pos += 1; return Ok(PyObject::list(items)); }
    loop {
        items.push(parse_json_value(s, pos)?);
        skip_ws(s, pos);
        if *pos >= s.len() { break; }
        if s.as_bytes()[*pos] == b']' { *pos += 1; return Ok(PyObject::list(items)); }
        if s.as_bytes()[*pos] == b',' { *pos += 1; } else { break; }
    }
    Err(PyException::value_error("Invalid JSON array"))
}

fn parse_json_object(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip {
    let pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
    let dict = PyObject::dict_from_pairs(pairs);
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b'}' { *pos += 1; return Ok(dict); }
    loop {
        skip_ws(s, pos);
        let key = parse_json_string(s, pos)?;
        skip_ws(s, pos);
        if *pos >= s.len() || s.as_bytes()[*pos] != b':' { return Err(PyException::value_error("Expected ':'")); }
        *pos += 1;
        let value = parse_json_value(s, pos)?;
        let hk = HashableKey::Str(CompactString::from(key.py_to_string()));
        match &dict.payload {
            PyObjectPayload::Dict(map) => { map.write().insert(hk, value); }
            _ => unreachable!(),
        }
        skip_ws(s, pos);
        if *pos >= s.len() { break; }
        if s.as_bytes()[*pos] == b'}' { *pos += 1; return Ok(dict); }
        if s.as_bytes()[*pos] == b',' { *pos += 1; } else { break; }
    }
    Err(PyException::value_error("Invalid JSON object"))
}

// ── time module ──

pub fn create_time_module() -> PyObjectRef {
    make_module("time", vec![
        ("time", make_builtin(time_time)),
        ("sleep", make_builtin(time_sleep)),
        ("monotonic", make_builtin(time_monotonic)),
        ("perf_counter", make_builtin(time_monotonic)),
        ("perf_counter_ns", make_builtin(|_args| {
            use std::time::Instant;
            static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            let start = START.get_or_init(Instant::now);
            Ok(PyObject::int(start.elapsed().as_nanos() as i64))
        })),
        ("time_ns", make_builtin(|_args| {
            use std::time::SystemTime;
            let dur = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
            Ok(PyObject::int(dur.as_nanos() as i64))
        })),
        ("process_time", make_builtin(time_monotonic)),
        ("strftime", make_builtin(time_strftime)),
        ("localtime", make_builtin(time_localtime)),
        ("gmtime", make_builtin(time_localtime)),
    ])
}

fn time_time(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::SystemTime;
    let dur = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    Ok(PyObject::float(dur.as_secs_f64()))
}

fn time_sleep(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("time.sleep", args, 1)?;
    let secs = args[0].to_float()?;
    if secs < 0.0 { return Err(PyException::value_error("sleep length must be non-negative")); }
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    Ok(PyObject::none())
}

fn time_monotonic(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::Instant;
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    Ok(PyObject::float(start.elapsed().as_secs_f64()))
}

fn time_strftime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("strftime requires a format string")); }
    let fmt = args[0].py_to_string();
    // Simplified strftime — handle common format codes
    use std::time::SystemTime;
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    // Basic time decomposition (UTC)
    let s = (secs % 60) as u32;
    let m = ((secs / 60) % 60) as u32;
    let h = ((secs / 3600) % 24) as u32;
    let days = (secs / 86400) as i64;
    // Days since epoch → year/month/day (simplified)
    let mut y: i64 = 1970;
    let mut remaining = days;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mon = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < md as i64 { mon = i; break; }
        remaining -= md as i64;
    }
    let day = remaining + 1;
    let result = fmt
        .replace("%Y", &format!("{:04}", y))
        .replace("%m", &format!("{:02}", mon + 1))
        .replace("%d", &format!("{:02}", day))
        .replace("%H", &format!("{:02}", h))
        .replace("%M", &format!("{:02}", m))
        .replace("%S", &format!("{:02}", s))
        .replace("%%", "%");
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn time_localtime(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Return a basic time tuple (year, month, day, hour, minute, second, weekday, yearday, dst)
    use std::time::SystemTime;
    let secs = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
    let s = (secs % 60) as i64;
    let m = ((secs / 60) % 60) as i64;
    let h = ((secs / 3600) % 24) as i64;
    let days = (secs / 86400) as i64;
    let mut y: i64 = 1970;
    let mut remaining = days;
    loop {
        let days_in_year = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mon = 1i64;
    for &md in &month_days {
        if remaining < md as i64 { break; }
        remaining -= md as i64;
        mon += 1;
    }
    let day = remaining + 1;
    let wday = ((days + 3) % 7) as i64; // 0=Monday for time.struct_time
    let yday = {
        let mut yd = day;
        for i in 0..(mon - 1) as usize { yd += month_days[i] as i64; }
        yd
    };
    Ok(PyObject::tuple(vec![
        PyObject::int(y), PyObject::int(mon), PyObject::int(day),
        PyObject::int(h), PyObject::int(m), PyObject::int(s),
        PyObject::int(wday), PyObject::int(yday), PyObject::int(-1),
    ]))
}

// ── random module (basic) ──

pub fn create_random_module() -> PyObjectRef {
    make_module("random", vec![
        ("random", make_builtin(random_random)),
        ("randint", make_builtin(random_randint)),
        ("choice", make_builtin(random_choice)),
        ("shuffle", make_builtin(random_shuffle)),
        ("seed", make_builtin(random_seed)),
        ("randrange", make_builtin(random_randrange)),
        ("uniform", make_builtin(|args| {
            check_args("random.uniform", args, 2)?;
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a + simple_random() * (b - a)))
        })),
        ("sample", make_builtin(|args| {
            check_args("random.sample", args, 2)?;
            let items = args[0].to_list()?;
            let k = args[1].to_int()? as usize;
            if k > items.len() { return Err(PyException::value_error("Sample larger than population")); }
            let mut result = Vec::with_capacity(k);
            let mut pool = items.clone();
            for _ in 0..k {
                let idx = (simple_random() * pool.len() as f64) as usize;
                let idx = idx.min(pool.len() - 1);
                result.push(pool.remove(idx));
            }
            Ok(PyObject::list(result))
        })),
        ("choices", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("random.choices requires at least 1 argument")); }
            let items = args[0].to_list()?;
            let k = if args.len() > 1 { args[1].to_int()? as usize } else { 1 };
            let mut result = Vec::with_capacity(k);
            for _ in 0..k {
                let idx = (simple_random() * items.len() as f64) as usize;
                result.push(items[idx.min(items.len()-1)].clone());
            }
            Ok(PyObject::list(result))
        })),
    ])
}

fn simple_random() -> f64 {
    use std::time::SystemTime;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cnt = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().subsec_nanos() as u64;
    let seed = nanos.wrapping_mul(6364136223846793005).wrapping_add(cnt.wrapping_mul(1442695040888963407));
    (seed >> 11) as f64 / (1u64 << 53) as f64
}

fn random_random(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::float(simple_random()))
}
fn random_randint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.randint", args, 2)?;
    let a = args[0].to_int()?;
    let b = args[1].to_int()?;
    let range = (b - a + 1) as f64;
    Ok(PyObject::int(a + (simple_random() * range) as i64))
}
fn random_choice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.choice", args, 1)?;
    let items = args[0].to_list()?;
    if items.is_empty() { return Err(PyException::index_error("Cannot choose from an empty sequence")); }
    let idx = (simple_random() * items.len() as f64) as usize;
    Ok(items[idx.min(items.len()-1)].clone())
}
fn random_shuffle(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.shuffle", args, 1)?;
    // Simplified in-place shuffle
    Ok(PyObject::none())
}
fn random_seed(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::none())
}
fn random_randrange(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("randrange requires at least 1 argument")); }
    let start = if args.len() == 1 { 0 } else { args[0].to_int()? };
    let stop = if args.len() == 1 { args[0].to_int()? } else { args[1].to_int()? };
    let step = if args.len() > 2 { args[2].to_int()? } else { 1 };
    let range = ((stop - start) as f64 / step as f64).ceil() as i64;
    if range <= 0 { return Err(PyException::value_error("empty range for randrange()")); }
    let idx = (simple_random() * range as f64) as i64;
    Ok(PyObject::int(start + idx * step))
}

// ── Stub modules ──

pub fn create_collections_module() -> PyObjectRef {
    make_module("collections", vec![
        ("OrderedDict", make_builtin(collections_ordered_dict)),
        ("defaultdict", make_builtin(collections_defaultdict)),
        ("Counter", make_builtin(collections_counter)),
        ("namedtuple", make_builtin(collections_namedtuple)),
        ("deque", make_builtin(collections_deque)),
    ])
}

fn collections_ordered_dict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // OrderedDict is just a regular dict in Python 3.7+
    if args.is_empty() {
        Ok(PyObject::dict_from_pairs(vec![]))
    } else {
        let items = args[0].to_list()?;
        let mut pairs = Vec::new();
        for item in items {
            if let PyObjectPayload::Tuple(t) = &item.payload {
                if t.len() == 2 {
                    pairs.push((t[0].clone(), t[1].clone()));
                }
            }
        }
        Ok(PyObject::dict_from_pairs(pairs))
    }
}

fn collections_defaultdict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let factory = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
        Some(args[0].clone())
    } else {
        None
    };
    
    let mut map = IndexMap::new();
    // Store factory as a special key
    if let Some(f) = factory {
        map.insert(
            HashableKey::Str(CompactString::from("__defaultdict_factory__")),
            f,
        );
    }
    
    // If initial data provided
    if args.len() >= 2 {
        if let PyObjectPayload::Dict(src) = &args[1].payload {
            for (k, v) in src.read().iter() {
                map.insert(k.clone(), v.clone());
            }
        }
    }
    
    Ok(PyObject::dict(map))
}

fn collections_counter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::dict_from_pairs(vec![]));
    }
    // Handle dict input: Counter({"red": 4, "blue": 2})
    if let PyObjectPayload::Dict(m) = &args[0].payload {
        let map = m.read();
        let pairs: Vec<(PyObjectRef, PyObjectRef)> = map.iter()
            .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__"))
            .map(|(k, v)| (k.to_object(), v.clone()))
            .collect();
        return Ok(PyObject::dict_from_pairs(pairs));
    }
    let items = args[0].to_list()?;
    let mut counts: IndexMap<HashableKey, i64> = IndexMap::new();
    for item in &items {
        let key = item.to_hashable_key()?;
        *counts.entry(key).or_insert(0) += 1;
    }
    let pairs: Vec<(PyObjectRef, PyObjectRef)> = counts.into_iter()
        .map(|(k, v)| {
            let key_obj = match k {
                HashableKey::Str(s) => PyObject::str_val(s),
                HashableKey::Int(i) => {
                    match i {
                        PyInt::Small(n) => PyObject::int(n),
                        PyInt::Big(b) => PyObject::big_int(*b),
                    }
                }
                HashableKey::Float(f) => PyObject::float(f.0),
                HashableKey::Bool(b) => PyObject::bool_val(b),
                HashableKey::None => PyObject::none(),
                HashableKey::Bytes(b) => PyObject::bytes(b),
                HashableKey::Tuple(items) => PyObject::tuple(
                    items.into_iter().map(|_| PyObject::none()).collect()
                ),
                HashableKey::FrozenSet(items) => {
                    let mut map = indexmap::IndexMap::new();
                    for k in items { map.insert(k.clone(), k.to_object()); }
                    PyObject::frozenset(map)
                },
            };
            (key_obj, PyObject::int(v))
        })
        .collect();
    Ok(PyObject::dict_from_pairs(pairs))
}

fn collections_namedtuple(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("namedtuple requires typename and field_names"));
    }
    let typename = args[0].py_to_string();
    
    // Parse field names
    let field_names: Vec<CompactString> = match &args[1].payload {
        PyObjectPayload::Str(s) => {
            // "x y" or "x, y" style
            s.replace(',', " ").split_whitespace()
                .map(|s| CompactString::from(s))
                .collect()
        }
        PyObjectPayload::List(_) => {
            args[1].to_list()?.iter().map(|i| CompactString::from(i.py_to_string())).collect()
        }
        PyObjectPayload::Tuple(items) => {
            items.iter().map(|i| CompactString::from(i.py_to_string())).collect()
        }
        _ => {
            args[1].to_list()?.iter().map(|i| CompactString::from(i.py_to_string())).collect()
        }
    };
    
    // Create a class with namespace containing field info
    let mut namespace = IndexMap::new();
    // Store field names for __init__ and indexing
    let fields_tuple = PyObject::tuple(
        field_names.iter().map(|n| PyObject::str_val(n.clone())).collect()
    );
    namespace.insert(CompactString::from("_fields"), fields_tuple);
    namespace.insert(CompactString::from("__namedtuple__"), PyObject::bool_val(true));
    
    // Store field indices  
    for (i, name) in field_names.iter().enumerate() {
        namespace.insert(
            CompactString::from(format!("_field_idx_{}", name)),
            PyObject::int(i as i64)
        );
    }
    
    let cls = PyObject::class(
        CompactString::from(typename.as_str()),
        vec![],
        namespace,
    );
    
    Ok(cls)
}

fn collections_deque(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let items = if args.is_empty() {
        vec![]
    } else {
        args[0].to_list()?
    };
    let deque_cls = PyObject::class(
        CompactString::from("deque"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(deque_cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_data"), PyObject::list(items));
    }
    Ok(inst)
}

pub fn create_functools_module() -> PyObjectRef {
    make_module("functools", vec![
        ("reduce", PyObject::native_function("functools.reduce", functools_reduce)),
        ("partial", PyObject::native_function("functools.partial", functools_partial)),
        ("lru_cache", make_builtin(|args| {
            // lru_cache(func) — bare decorator: return func unchanged
            // lru_cache(maxsize=N) — called with int arg: return identity decorator
            if args.is_empty() { 
                // @lru_cache() with no args — return identity decorator
                return Ok(make_builtin(|args| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    Ok(args[0].clone())
                }));
            }
            // If first arg is a callable (function), apply directly
            match &args[0].payload {
                PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction { .. } 
                | PyObjectPayload::BuiltinFunction(_) => {
                    Ok(args[0].clone())
                }
                _ => {
                    // Called with maxsize parameter — return identity decorator
                    Ok(make_builtin(|args| {
                        if args.is_empty() { return Ok(PyObject::none()); }
                        Ok(args[0].clone())
                    }))
                }
            }
        })),
        ("wraps", make_builtin(|args| {
            // Simple pass-through decorator — return identity
            if args.is_empty() { return Ok(PyObject::none()); }
            Ok(make_builtin(|args| {
                if args.is_empty() { return Ok(PyObject::none()); }
                Ok(args[0].clone())
            }))
        })),
        ("cached_property", make_builtin(|args| {
            // Stub — just wrap the function in a property-like
            if args.is_empty() { return Err(PyException::type_error("cached_property requires 1 argument")); }
            Ok(PyObject::wrap(PyObjectPayload::Property {
                fget: Some(args[0].clone()),
                fset: None,
                fdel: None,
            }))
        })),
        ("total_ordering", make_builtin(|args| {
            // Stub — just return the class unchanged
            if args.is_empty() { return Err(PyException::type_error("total_ordering requires 1 argument")); }
            Ok(args[0].clone())
        })),
    ])
}

fn functools_partial(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("partial() requires at least 1 argument")); }
    let func = args[0].clone();
    let partial_args = if args.len() > 1 { args[1..].to_vec() } else { vec![] };
    Ok(PyObject::wrap(PyObjectPayload::Partial {
        func,
        args: partial_args,
        kwargs: vec![],
    }))
}

fn functools_reduce(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("reduce() requires at least 2 arguments")); }
    let func = args[0].clone();
    let items = args[1].to_list()?;
    let mut acc = if args.len() > 2 {
        args[2].clone()
    } else if !items.is_empty() {
        items[0].clone()
    } else {
        return Err(PyException::type_error("reduce() of empty sequence with no initial value"));
    };
    let start_idx = if args.len() > 2 { 0 } else { 1 };
    for item in &items[start_idx..] {
        // Call func(acc, item) — but we're a builtin, so we can't easily call Python funcs here.
        // This would need VM access; for now we'll return a stub error.
        let _ = func;
        let _ = item;
        return Err(PyException::type_error("functools.reduce not fully implemented yet"));
    }
    Ok(acc)
}

pub fn create_itertools_module() -> PyObjectRef {
    make_module("itertools", vec![
        ("count", make_builtin(itertools_count)),
        ("chain", make_builtin(itertools_chain)),
        ("repeat", make_builtin(itertools_repeat)),
        ("cycle", make_builtin(itertools_cycle)),
        ("islice", make_builtin(itertools_islice)),
        ("zip_longest", make_builtin(itertools_zip_longest)),
        ("product", make_builtin(itertools_product)),
        ("accumulate", make_builtin(itertools_accumulate)),
        ("dropwhile", make_builtin(itertools_dropwhile)),
        ("takewhile", make_builtin(itertools_takewhile)),
        ("combinations", make_builtin(itertools_combinations)),
        ("permutations", make_builtin(itertools_permutations)),
        ("groupby", make_builtin(itertools_groupby)),
        ("chain.from_iterable", make_builtin(itertools_chain_from_iterable)),
        ("compress", make_builtin(itertools_compress)),
        ("tee", make_builtin(itertools_tee)),
        ("starmap", make_builtin(|_args| Ok(PyObject::none()))),
    ])
}

fn itertools_count(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let start = if args.is_empty() { 0i64 } else { args[0].to_int()? };
    let step = if args.len() >= 2 { args[1].to_int()? } else { 1 };
    // Return a list-based iterator with a large range (lazy would be better, but this works)
    let items: Vec<PyObjectRef> = (0..1000).map(|i| PyObject::int(start + i * step)).collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items, index: 0 }
    )))))
}

fn itertools_chain(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mut all_items = Vec::new();
    for arg in args {
        let items = arg.to_list()?;
        all_items.extend(items);
    }
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items: all_items, index: 0 }
    )))))
}

fn itertools_repeat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("repeat() missing required argument"));
    }
    let item = args[0].clone();
    let count = if args.len() >= 2 { args[1].to_int()? as usize } else { 100 };
    let items: Vec<PyObjectRef> = std::iter::repeat(item).take(count).collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items, index: 0 }
    )))))
}

fn itertools_cycle(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("cycle() missing required argument"));
    }
    let base = args[0].to_list()?;
    if base.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
            IteratorData::List { items: vec![], index: 0 }
        )))));
    }
    // Materialize a reasonable number of cycles
    let mut items = Vec::new();
    for _ in 0..1000 {
        items.extend(base.iter().cloned());
    }
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items, index: 0 }
    )))))
}

fn itertools_islice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("islice() requires at least 2 arguments"));
    }
    let items = args[0].to_list()?;
    let (start, stop, step) = if args.len() == 2 {
        (0usize, args[1].to_int()? as usize, 1usize)
    } else if args.len() == 3 {
        let s = if matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { args[1].to_int()? as usize };
        (s, args[2].to_int()? as usize, 1usize)
    } else {
        let s = if matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { args[1].to_int()? as usize };
        let step = if matches!(&args[3].payload, PyObjectPayload::None) { 1 } else { args[3].to_int()? as usize };
        (s, args[2].to_int()? as usize, step)
    };
    let result: Vec<PyObjectRef> = items.into_iter()
        .skip(start)
        .take(stop - start)
        .step_by(step.max(1))
        .collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items: result, index: 0 }
    )))))
}

fn itertools_zip_longest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("zip_longest requires at least 2 arguments"));
    }
    let lists: Vec<Vec<PyObjectRef>> = args.iter()
        .map(|a| a.to_list())
        .collect::<Result<Vec<_>, _>>()?;
    let max_len = lists.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut result = Vec::new();
    for i in 0..max_len {
        let tuple: Vec<PyObjectRef> = lists.iter()
            .map(|l| l.get(i).cloned().unwrap_or_else(PyObject::none))
            .collect();
        result.push(PyObject::tuple(tuple));
    }
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items: result, index: 0 }
    )))))
}

fn itertools_product(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
            IteratorData::List { items: vec![PyObject::tuple(vec![])], index: 0 }
        )))));
    }
    let lists: Vec<Vec<PyObjectRef>> = args.iter()
        .map(|a| a.to_list())
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = vec![vec![]];
    for lst in &lists {
        let mut new_result = Vec::new();
        for prefix in &result {
            for item in lst {
                let mut combo = prefix.clone();
                combo.push(item.clone());
                new_result.push(combo);
            }
        }
        result = new_result;
    }
    let items: Vec<PyObjectRef> = result.into_iter()
        .map(|combo| PyObject::tuple(combo))
        .collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items, index: 0 }
    )))))
}

fn itertools_accumulate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("accumulate requires an iterable")); }
    let items = args[0].to_list()?;
    if items.is_empty() { return Ok(PyObject::list(vec![])); }
    let mut result = Vec::new();
    let mut acc = items[0].clone();
    result.push(acc.clone());
    for item in &items[1..] {
        let a = acc.to_float().unwrap_or(acc.as_int().unwrap_or(0) as f64);
        let b = item.to_float().unwrap_or(item.as_int().unwrap_or(0) as f64);
        let sum = a + b;
        acc = if acc.as_int().is_some() && item.as_int().is_some() {
            PyObject::int(sum as i64)
        } else {
            PyObject::float(sum)
        };
        result.push(acc.clone());
    }
    Ok(PyObject::list(result))
}

fn itertools_dropwhile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("dropwhile requires predicate and iterable")); }
    let items = args[1].to_list()?;
    Ok(PyObject::list(items))
}

fn itertools_takewhile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("takewhile requires predicate and iterable")); }
    let items = args[1].to_list()?;
    Ok(PyObject::list(items))
}

fn itertools_combinations(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("combinations requires iterable and r")); }
    let items = args[0].to_list()?;
    let r = args[1].as_int().unwrap_or(2) as usize;
    let n = items.len();
    if r > n { return Ok(PyObject::list(vec![])); }
    let mut result = Vec::new();
    let mut indices: Vec<usize> = (0..r).collect();
    result.push(PyObject::tuple(indices.iter().map(|&i| items[i].clone()).collect()));
    loop {
        let mut i_opt = None;
        for i in (0..r).rev() {
            if indices[i] != i + n - r {
                i_opt = Some(i);
                break;
            }
        }
        let i = match i_opt { Some(i) => i, None => break };
        indices[i] += 1;
        for j in (i + 1)..r {
            indices[j] = indices[j - 1] + 1;
        }
        result.push(PyObject::tuple(indices.iter().map(|&idx| items[idx].clone()).collect()));
    }
    Ok(PyObject::list(result))
}

fn itertools_permutations(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("permutations requires iterable")); }
    let items = args[0].to_list()?;
    let r = if args.len() > 1 { args[1].as_int().unwrap_or(items.len() as i64) as usize } else { items.len() };
    let n = items.len();
    if r > n { return Ok(PyObject::list(vec![])); }
    let mut result = Vec::new();
    let mut indices: Vec<usize> = (0..n).collect();
    let mut cycles: Vec<usize> = (0..r).map(|i| n - i).collect();
    result.push(PyObject::tuple(indices[..r].iter().map(|&i| items[i].clone()).collect()));
    'outer: loop {
        for i in (0..r).rev() {
            cycles[i] -= 1;
            if cycles[i] == 0 {
                let tmp = indices[i];
                for j in i..n-1 { indices[j] = indices[j+1]; }
                indices[n-1] = tmp;
                cycles[i] = n - i;
                if i == 0 { break 'outer; }
            } else {
                let j = n - cycles[i];
                indices.swap(i, j);
                result.push(PyObject::tuple(indices[..r].iter().map(|&idx| items[idx].clone()).collect()));
                continue 'outer;
            }
        }
        break;
    }
    Ok(PyObject::list(result))
}

fn itertools_groupby(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("groupby requires iterable")); }
    let items = args[0].to_list()?;
    if items.is_empty() { return Ok(PyObject::list(vec![])); }
    let mut result = Vec::new();
    let mut current_key = items[0].py_to_string();
    let mut current_group = vec![items[0].clone()];
    for item in &items[1..] {
        let key = item.py_to_string();
        if key == current_key {
            current_group.push(item.clone());
        } else {
            result.push(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(current_key.as_str())),
                PyObject::list(current_group),
            ]));
            current_key = key;
            current_group = vec![item.clone()];
        }
    }
    result.push(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(current_key.as_str())),
        PyObject::list(current_group),
    ]));
    Ok(PyObject::list(result))
}

fn itertools_chain_from_iterable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("chain.from_iterable requires iterable")); }
    let outer = args[0].to_list()?;
    let mut result = Vec::new();
    for inner in &outer {
        let items = inner.to_list()?;
        result.extend(items);
    }
    Ok(PyObject::list(result))
}

fn itertools_compress(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("compress requires data and selectors")); }
    let data = args[0].to_list()?;
    let selectors = args[1].to_list()?;
    let mut result = Vec::new();
    for (d, s) in data.iter().zip(selectors.iter()) {
        if s.is_truthy() {
            result.push(d.clone());
        }
    }
    Ok(PyObject::list(result))
}

fn itertools_tee(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("tee requires iterable")); }
    let items = args[0].to_list()?;
    let n = if args.len() > 1 { args[1].as_int().unwrap_or(2) } else { 2 };
    let copies: Vec<PyObjectRef> = (0..n).map(|_| PyObject::list(items.clone())).collect();
    Ok(PyObject::tuple(copies))
}

pub fn create_io_module() -> PyObjectRef {
    make_module("io", vec![
        ("StringIO", make_builtin(|_args| Ok(PyObject::none()))),
        ("BytesIO", make_builtin(|_args| Ok(PyObject::none()))),
    ])
}

pub fn create_re_module() -> PyObjectRef {
    make_module("re", vec![
        ("IGNORECASE", PyObject::int(2)),
        ("I", PyObject::int(2)),
        ("MULTILINE", PyObject::int(8)),
        ("M", PyObject::int(8)),
        ("DOTALL", PyObject::int(16)),
        ("S", PyObject::int(16)),
        ("VERBOSE", PyObject::int(64)),
        ("X", PyObject::int(64)),
        ("match", PyObject::native_function("re.match", re_match)),
        ("search", PyObject::native_function("re.search", re_search)),
        ("findall", PyObject::native_function("re.findall", re_findall)),
        ("finditer", PyObject::native_function("re.finditer", re_finditer)),
        ("sub", PyObject::native_function("re.sub", re_sub)),
        ("subn", PyObject::native_function("re.subn", re_subn)),
        ("split", PyObject::native_function("re.split", re_split)),
        ("compile", PyObject::native_function("re.compile", re_compile)),
        ("escape", PyObject::native_function("re.escape", re_escape)),
        ("fullmatch", PyObject::native_function("re.fullmatch", re_fullmatch)),
    ])
}

fn convert_python_regex(pattern: &str) -> String {
    // Convert Python regex syntax to Rust regex syntax
    // Most are compatible, but a few need translation
    let mut result = pattern.to_string();
    // Python uses (?P<name>) for named groups, Rust regex uses (?P<name>) too — compatible!
    // Python uses \d, \w, \s etc — compatible
    // Python uses (?:...) for non-capturing groups — compatible
    result
}

fn build_regex(pattern: &str, flags: i64) -> Result<regex::Regex, PyException> {
    let mut pat = convert_python_regex(pattern);
    // Apply flags as inline flags
    let mut prefix = String::new();
    if flags & 2 != 0 { prefix.push_str("(?i)"); }
    if flags & 8 != 0 { prefix.push_str("(?m)"); }
    if flags & 16 != 0 { prefix.push_str("(?s)"); }
    pat = format!("{}{}", prefix, pat);
    regex::Regex::new(&pat).map_err(|e| PyException::runtime_error(format!("re: {}", e)))
}

fn make_match_object(m: regex::Match, text: &str, re_obj: &regex::Regex) -> PyObjectRef {
    let full_match = m.as_str().to_string();
    let start = m.start() as i64;
    let end = m.end() as i64;
    // groups - store captured groups
    let captures = re_obj.captures(text);
    let mut groups = Vec::new();
    if let Some(caps) = &captures {
        for i in 1..caps.len() {
            if let Some(g) = caps.get(i) {
                groups.push(PyObject::str_val(CompactString::from(g.as_str().to_string())));
            } else {
                groups.push(PyObject::none());
            }
        }
    }
    let groups_tuple = PyObject::tuple(groups);
    // Build the match object with pre-bound data attributes
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_match"), PyObject::str_val(CompactString::from(full_match)));
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(CompactString::from("_text"), PyObject::str_val(CompactString::from(text.to_string())));
    attrs.insert(CompactString::from("_groups"), groups_tuple);
    attrs.insert(CompactString::from("group"), PyObject::native_function("Match.group", match_group));
    attrs.insert(CompactString::from("groups"), PyObject::native_function("Match.groups", match_groups));
    attrs.insert(CompactString::from("start"), PyObject::native_function("Match.start", match_start));
    attrs.insert(CompactString::from("end"), PyObject::native_function("Match.end", match_end));
    attrs.insert(CompactString::from("span"), PyObject::native_function("Match.span", match_span));
    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    let match_obj = PyObject::module_with_attrs(CompactString::from("Match"), attrs);
    match_obj
}

fn match_group(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("group() needs self")); }
    let self_obj = &args[0];
    let group_num = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
    if group_num == 0 {
        if let Some(m) = self_obj.get_attr("_match") {
            return Ok(m);
        }
    }
    if let Some(groups) = self_obj.get_attr("_groups") {
        if let PyObjectPayload::Tuple(items) = &groups.payload {
            let idx = (group_num - 1) as usize;
            if idx < items.len() {
                return Ok(items[idx].clone());
            }
        }
    }
    Err(PyException::index_error("no such group"))
}

fn match_groups(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("groups() needs self")); }
    if let Some(groups) = args[0].get_attr("_groups") {
        return Ok(groups);
    }
    Ok(PyObject::tuple(vec![]))
}

fn match_start(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("start() needs self")); }
    if let Some(s) = args[0].get_attr("_start") { return Ok(s); }
    Ok(PyObject::int(0))
}

fn match_end(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("end() needs self")); }
    if let Some(e) = args[0].get_attr("_end") { return Ok(e); }
    Ok(PyObject::int(0))
}

fn match_span(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("span() needs self")); }
    let start = args[0].get_attr("_start").unwrap_or(PyObject::int(0));
    let end = args[0].get_attr("_end").unwrap_or(PyObject::int(0));
    Ok(PyObject::tuple(vec![start, end]))
}

fn re_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.match() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    // re.match anchors at start
    let anchored = format!("^(?:{})", pattern);
    let re = build_regex(&anchored, flags)?;
    match re.find(&text) {
        Some(m) => {
            let orig_re = build_regex(&pattern, flags)?;
            Ok(make_match_object(m, &text, &orig_re))
        }
        None => Ok(PyObject::none()),
    }
}

fn re_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.search() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    match re.find(&text) {
        Some(m) => Ok(make_match_object(m, &text, &re)),
        None => Ok(PyObject::none()),
    }
}

fn re_fullmatch(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.fullmatch() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let anchored = format!("^(?:{})$", pattern);
    let re = build_regex(&anchored, flags)?;
    let orig_re = build_regex(&pattern, flags)?;
    match re.find(&text) {
        Some(m) => Ok(make_match_object(m, &text, &orig_re)),
        None => Ok(PyObject::none()),
    }
}

fn re_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.findall() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    // If pattern has groups, return group(1) for single group, tuple for multiple
    let cap_count = re.captures_len() - 1;
    if cap_count == 0 {
        let results: Vec<PyObjectRef> = re.find_iter(&text)
            .map(|m| PyObject::str_val(CompactString::from(m.as_str())))
            .collect();
        Ok(PyObject::list(results))
    } else if cap_count == 1 {
        let results: Vec<PyObjectRef> = re.captures_iter(&text)
            .filter_map(|caps| caps.get(1).map(|m| PyObject::str_val(CompactString::from(m.as_str()))))
            .collect();
        Ok(PyObject::list(results))
    } else {
        let results: Vec<PyObjectRef> = re.captures_iter(&text)
            .map(|caps| {
                let groups: Vec<PyObjectRef> = (1..=cap_count)
                    .map(|i| caps.get(i)
                        .map(|m| PyObject::str_val(CompactString::from(m.as_str())))
                        .unwrap_or(PyObject::none()))
                    .collect();
                PyObject::tuple(groups)
            })
            .collect();
        Ok(PyObject::list(results))
    }
}

fn re_finditer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.finditer() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    let matches: Vec<PyObjectRef> = re.find_iter(&text)
        .map(|m| make_match_object(m, &text, &re))
        .collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(std::sync::Mutex::new(
        IteratorData::List { items: matches, index: 0 }
    )))))
}

fn re_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("re.sub() requires pattern, repl, and string")); }
    let pattern = args[0].py_to_string();
    let repl = args[1].py_to_string();
    let text = args[2].py_to_string();
    let count = if args.len() > 3 { args[3].to_int().unwrap_or(0) as usize } else { 0 };
    let flags = if args.len() > 4 { args[4].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    let result = if count == 0 {
        re.replace_all(&text, repl.as_str()).to_string()
    } else {
        re.replacen(&text, count, repl.as_str()).to_string()
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn re_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("re.subn() requires pattern, repl, and string")); }
    let pattern = args[0].py_to_string();
    let repl = args[1].py_to_string();
    let text = args[2].py_to_string();
    let flags = if args.len() > 3 { args[3].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    let count = re.find_iter(&text).count();
    let result = re.replace_all(&text, repl.as_str()).to_string();
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(result)),
        PyObject::int(count as i64),
    ]))
}

fn re_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.split() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let maxsplit = if args.len() > 2 { args[2].to_int().unwrap_or(0) as usize } else { 0 };
    let flags = if args.len() > 3 { args[3].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    let parts: Vec<PyObjectRef> = if maxsplit == 0 {
        re.split(&text).map(|s| PyObject::str_val(CompactString::from(s))).collect()
    } else {
        re.splitn(&text, maxsplit + 1).map(|s| PyObject::str_val(CompactString::from(s))).collect()
    };
    Ok(PyObject::list(parts))
}

fn re_compile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("re.compile() requires a pattern")); }
    let pattern = args[0].py_to_string();
    let flags = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
    // Validate the pattern compiles
    let _ = build_regex(&pattern, flags)?;
    // Return a compiled pattern object with match/search/findall etc.
    let pat_str = PyObject::str_val(CompactString::from(pattern.clone()));
    let flags_obj = PyObject::int(flags);
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("pattern"), pat_str);
    attrs.insert(CompactString::from("flags"), flags_obj);
    attrs.insert(CompactString::from("match"), PyObject::native_function("Pattern.match", compiled_match));
    attrs.insert(CompactString::from("search"), PyObject::native_function("Pattern.search", compiled_search));
    attrs.insert(CompactString::from("findall"), PyObject::native_function("Pattern.findall", compiled_findall));
    attrs.insert(CompactString::from("sub"), PyObject::native_function("Pattern.sub", compiled_sub));
    attrs.insert(CompactString::from("split"), PyObject::native_function("Pattern.split", compiled_split));
    attrs.insert(CompactString::from("fullmatch"), PyObject::native_function("Pattern.fullmatch", compiled_fullmatch));
    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    Ok(PyObject::module_with_attrs(CompactString::from("Pattern"), attrs))
}

fn compiled_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.match() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_match(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(flags)])
}

fn compiled_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.search() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_search(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(flags)])
}

fn compiled_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.findall() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_findall(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(flags)])
}

fn compiled_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("Pattern.sub() requires self, repl, and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_sub(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), args[2].clone(), PyObject::int(0), PyObject::int(flags)])
}

fn compiled_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.split() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_split(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(0), PyObject::int(flags)])
}

fn compiled_fullmatch(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.fullmatch() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_fullmatch(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(flags)])
}

fn re_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("re.escape() requires a string")); }
    let s = args[0].py_to_string();
    let escaped = regex::escape(&s);
    Ok(PyObject::str_val(CompactString::from(escaped)))
}

pub fn create_hashlib_module() -> PyObjectRef {
    make_module("hashlib", vec![
        ("md5", make_builtin(hashlib_md5)),
        ("sha1", make_builtin(hashlib_sha1)),
        ("sha256", make_builtin(hashlib_sha256)),
        ("sha512", make_builtin(hashlib_sha512)),
        ("sha224", make_builtin(hashlib_sha224)),
        ("sha384", make_builtin(hashlib_sha384)),
        ("new", make_builtin(hashlib_new)),
    ])
}

fn make_hash_object(name: &str, digest_hex: String, digest_bytes: Vec<u8>, block_size: i64, digest_size: i64) -> PyObjectRef {
    let class = PyObject::class(CompactString::from(name), vec![], IndexMap::new());
    let attrs = IndexMap::new();
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class: class.clone(),
        attrs: Arc::new(RwLock::new(attrs)),
    }));
    {
        let a = if let PyObjectPayload::Instance(ref d) = inst.payload { d.attrs.clone() } else { unreachable!() };
        let mut w = a.write();
        w.insert(CompactString::from("_hexdigest"), PyObject::str_val(CompactString::from(&digest_hex)));
        w.insert(CompactString::from("_digest"), PyObject::bytes(digest_bytes));
        w.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name)));
        w.insert(CompactString::from("block_size"), PyObject::int(block_size));
        w.insert(CompactString::from("digest_size"), PyObject::int(digest_size));
    }
    inst
}

fn hashlib_md5(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use md5::Md5;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Md5::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("md5", hex, result.to_vec(), 64, 16))
}

fn hashlib_sha1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha1::Sha1;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha1::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha1", hex, result.to_vec(), 64, 20))
}

fn hashlib_sha256(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha256;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha256", hex, result.to_vec(), 64, 32))
}

fn hashlib_sha224(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha224;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha224::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha224", hex, result.to_vec(), 64, 28))
}

fn hashlib_sha384(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha384;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha384::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha384", hex, result.to_vec(), 128, 48))
}

fn hashlib_sha512(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha512;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha512::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha512", hex, result.to_vec(), 128, 64))
}

fn hashlib_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("hashlib.new() requires algorithm name")); }
    let name = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("algorithm name must be a string")),
    };
    let data_args = if args.len() > 1 { &args[1..] } else { &[] as &[PyObjectRef] };
    match name.as_str() {
        "md5" => hashlib_md5(data_args),
        "sha1" => hashlib_sha1(data_args),
        "sha256" => hashlib_sha256(data_args),
        "sha224" => hashlib_sha224(data_args),
        "sha384" => hashlib_sha384(data_args),
        "sha512" => hashlib_sha512(data_args),
        _ => Err(PyException::value_error(format!("unsupported hash type {}", name))),
    }
}

// ── copy module ──

pub fn create_copy_module() -> PyObjectRef {
    make_module("copy", vec![
        ("copy", make_builtin(copy_copy)),
        ("deepcopy", make_builtin(copy_deepcopy)),
    ])
}

fn copy_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("copy() requires 1 argument")); }
    shallow_copy(&args[0])
}

fn copy_deepcopy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("deepcopy() requires 1 argument")); }
    deep_copy(&args[0])
}

fn shallow_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => Ok(PyObject::tuple(items.clone())),
        PyObjectPayload::List(items) => Ok(PyObject::list(items.read().clone())),
        PyObjectPayload::Dict(map) => Ok(PyObject::dict(map.read().clone())),
        PyObjectPayload::Set(set) => Ok(PyObject::set(set.read().clone())),
        PyObjectPayload::Instance(inst) => {
            // Create new instance with same class, shallow copy of attrs
            Ok(PyObject::wrap(PyObjectPayload::Instance(InstanceData {
                class: inst.class.clone(),
                attrs: Arc::new(RwLock::new(inst.attrs.read().clone())),
            })))
        }
        _ => Ok(obj.clone()),
    }
}

fn deep_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => {
            let new_items: Result<Vec<_>, _> = items.iter().map(|x| deep_copy(x)).collect();
            Ok(PyObject::tuple(new_items?))
        }
        PyObjectPayload::List(items) => {
            let new_items: Result<Vec<_>, _> = items.read().iter().map(|x| deep_copy(x)).collect();
            Ok(PyObject::list(new_items?))
        }
        PyObjectPayload::Dict(map) => {
            let mut new_map = IndexMap::new();
            for (k, v) in map.read().iter() {
                new_map.insert(k.clone(), deep_copy(v)?);
            }
            Ok(PyObject::dict(new_map))
        }
        PyObjectPayload::Set(set) => {
            Ok(PyObject::set(set.read().clone()))
        }
        _ => Ok(obj.clone()),
    }
}

// ── operator module ──

pub fn create_operator_module() -> PyObjectRef {
    make_module("operator", vec![
        ("add", make_builtin(|args| {
            check_args_min("add", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a + b));
                }
            }
            if let (Ok(a), Ok(b)) = (args[0].to_float(), args[1].to_float()) {
                Ok(PyObject::float(a + b))
            } else {
                let a = args[0].py_to_string();
                let b = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(format!("{}{}", a, b))))
            }
        })),
        ("sub", make_builtin(|args| {
            check_args_min("sub", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a - b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a - b))
        })),
        ("mul", make_builtin(|args| {
            check_args_min("mul", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a * b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a * b))
        })),
        ("truediv", make_builtin(|args| {
            check_args_min("truediv", args, 2)?;
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
            Ok(PyObject::float(a / b))
        })),
        ("floordiv", make_builtin(|args| {
            check_args_min("floordiv", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.div_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("float floor division by zero")); }
            Ok(PyObject::float((a / b).floor()))
        })),
        ("mod_", make_builtin(|args| {
            check_args_min("mod_", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        ("neg", make_builtin(|args| {
            check_args_min("neg", args, 1)?;
            if matches!(&args[0].payload, PyObjectPayload::Float(_)) {
                Ok(PyObject::float(-args[0].to_float()?))
            } else if let Ok(n) = args[0].to_int() {
                Ok(PyObject::int(-n))
            } else {
                Ok(PyObject::float(-args[0].to_float()?))
            }
        })),
        ("pos", make_builtin(|args| {
            check_args_min("pos", args, 1)?;
            Ok(args[0].clone())
        })),
        ("not_", make_builtin(|args| {
            check_args_min("not_", args, 1)?;
            Ok(PyObject::bool_val(!args[0].is_truthy()))
        })),
        ("eq", make_builtin(|args| {
            check_args_min("eq", args, 2)?;
            args[0].compare(&args[1], CompareOp::Eq)
        })),
        ("ne", make_builtin(|args| {
            check_args_min("ne", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ne)
        })),
        ("lt", make_builtin(|args| {
            check_args_min("lt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Lt)
        })),
        ("le", make_builtin(|args| {
            check_args_min("le", args, 2)?;
            args[0].compare(&args[1], CompareOp::Le)
        })),
        ("gt", make_builtin(|args| {
            check_args_min("gt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Gt)
        })),
        ("ge", make_builtin(|args| {
            check_args_min("ge", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ge)
        })),
        ("abs", make_builtin(|args| {
            check_args_min("abs", args, 1)?;
            check_args("abs", args, 1)?;
            args[0].py_abs()
        })),
        ("contains", make_builtin(|args| {
            check_args_min("contains", args, 2)?;
            Ok(PyObject::bool_val(args[0].contains(&args[1])?))
        })),
        ("getitem", make_builtin(|args| {
            check_args_min("getitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.read().get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("list index out of range"))
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.read().get(&key).cloned()
                        .ok_or_else(|| PyException::key_error(args[1].repr()))
                }
                PyObjectPayload::Tuple(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("tuple index out of range"))
                }
                _ => Err(PyException::type_error("object is not subscriptable")),
            }
        })),
        ("itemgetter", make_builtin(|args| {
            // Returns a Module-like callable that extracts an item
            check_args_min("itemgetter", args, 1)?;
            let key = args[0].clone();
            let mut attrs = vec![
                ("_key", key),
            ];
            attrs.push(("_bind_methods", PyObject::bool_val(true)));
            Ok(make_module("itemgetter", attrs))
        })),
        ("attrgetter", make_builtin(|args| {
            check_args_min("attrgetter", args, 1)?;
            let attr_name = args[0].clone();
            let mut attrs = vec![
                ("_attr", attr_name),
            ];
            attrs.push(("_bind_methods", PyObject::bool_val(true)));
            Ok(make_module("attrgetter", attrs))
        })),
    ])
}

// ── typing module (stub) ──

pub fn create_typing_module() -> PyObjectRef {
    let mut attrs: Vec<(&str, PyObjectRef)> = vec![
        ("Any", PyObject::none()),
        ("Union", PyObject::none()),
        ("Optional", PyObject::none()),
        ("List", PyObject::builtin_type(CompactString::from("list"))),
        ("Dict", PyObject::builtin_type(CompactString::from("dict"))),
        ("Set", PyObject::builtin_type(CompactString::from("set"))),
        ("Tuple", PyObject::builtin_type(CompactString::from("tuple"))),
        ("FrozenSet", PyObject::builtin_type(CompactString::from("frozenset"))),
        ("Type", PyObject::builtin_type(CompactString::from("type"))),
        ("Callable", PyObject::none()),
        ("Iterator", PyObject::none()),
        ("Generator", PyObject::none()),
        ("Sequence", PyObject::none()),
        ("Mapping", PyObject::none()),
        ("MutableMapping", PyObject::none()),
        ("Iterable", PyObject::none()),
    ];
    attrs.push(("TYPE_CHECKING", PyObject::bool_val(false)));
    make_module("typing", attrs)
}

// ── abc module (stub) ──

pub fn create_abc_module() -> PyObjectRef {
    make_module("abc", vec![
        ("ABC", PyObject::none()),
        ("ABCMeta", PyObject::none()),
        ("abstractmethod", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("abstractmethod requires 1 argument")); }
            Ok(args[0].clone())
        })),
    ])
}

// ── enum module (stub) ──

pub fn create_enum_module() -> PyObjectRef {
    // Create Enum as a base class marker
    let enum_class = PyObject::class(
        CompactString::from("Enum"),
        vec![],
        IndexMap::new(),
    );
    // Mark it as enum base
    if let PyObjectPayload::Class(ref cd) = enum_class.payload {
        cd.namespace.write().insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    }
    let int_enum = PyObject::class(
        CompactString::from("IntEnum"),
        vec![enum_class.clone()],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = int_enum.payload {
        cd.namespace.write().insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    }
    
    // auto() counter
    static AUTO_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
    
    make_module("enum", vec![
        ("Enum", enum_class),
        ("IntEnum", int_enum),
        ("Flag", PyObject::none()),
        ("IntFlag", PyObject::none()),
        ("auto", make_builtin(|_| {
            let val = AUTO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(PyObject::int(val))
        })),
        ("unique", make_builtin(|args| {
            if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
        })),
    ])
}

// ── contextlib module ──

pub fn create_contextlib_module() -> PyObjectRef {
    make_module("contextlib", vec![
        ("contextmanager", make_builtin(contextlib_contextmanager)),
        ("suppress", make_builtin(|_args| {
            // Stub: returns a no-op context manager
            Ok(make_module("suppress_cm", vec![
                ("__enter__", make_builtin(|_| Ok(PyObject::none()))),
                ("__exit__", make_builtin(|_| Ok(PyObject::bool_val(true)))),
            ]))
        })),
        ("closing", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("closing requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("ExitStack", make_builtin(|_| Ok(PyObject::none()))),
        ("redirect_stdout", make_builtin(|_| Ok(PyObject::none()))),
        ("redirect_stderr", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

fn contextlib_contextmanager(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // contextmanager decorator — returns the function unchanged.
    // The function is a generator function. When called, it returns a Generator.
    // The VM's SetupWith handles Generator objects as context managers directly.
    if args.is_empty() { return Err(PyException::type_error("contextmanager requires 1 argument")); }
    Ok(args[0].clone())
}

// ── dataclasses module ──

pub fn create_dataclasses_module() -> PyObjectRef {
    make_module("dataclasses", vec![
        ("dataclass", make_builtin(dataclass_decorator)),
        ("field", make_builtin(|args| {
            // Return a sentinel field object
            let default = if args.is_empty() { PyObject::none() } else { args[0].clone() };
            Ok(default)
        })),
        ("asdict", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("asdict requires 1 argument")); }
            // Convert instance attrs to dict
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                let mut map = IndexMap::new();
                for (k, v) in attrs.iter() {
                    if !k.starts_with('_') {
                        map.insert(HashableKey::Str(k.clone()), v.clone());
                    }
                }
                Ok(PyObject::dict(map))
            } else {
                Err(PyException::type_error("asdict() should be called on dataclass instances"))
            }
        })),
        ("astuple", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("astuple requires 1 argument")); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                let items: Vec<_> = attrs.values().cloned().collect();
                Ok(PyObject::tuple(items))
            } else {
                Err(PyException::type_error("astuple() should be called on dataclass instances"))
            }
        })),
        ("fields", make_builtin(|_| Ok(PyObject::tuple(vec![])))),
    ])
}

fn dataclass_decorator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("dataclass requires 1 argument")); }
    let cls = &args[0];
    
    // Get annotations to discover fields
    let mut field_names: Vec<CompactString> = Vec::new();
    let mut field_defaults: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let ns = cd.namespace.read();
        if let Some(annotations) = ns.get("__annotations__") {
            if let PyObjectPayload::Dict(ann_map) = &annotations.payload {
                for (k, _v) in ann_map.read().iter() {
                    if let HashableKey::Str(name) = k {
                        field_names.push(name.clone());
                        // Check for default value in class namespace
                        if let Some(default) = ns.get(name.as_str()) {
                            field_defaults.insert(name.clone(), default.clone());
                        }
                    }
                }
            }
        }
    }
    
    // Store __dataclass_fields__ as a tuple of (name, has_default, default_val) tuples
    let fields_info: Vec<PyObjectRef> = field_names.iter().map(|name| {
        let has_default = field_defaults.contains_key(name.as_str());
        let default_val = field_defaults.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
        PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(name.as_str())),
            PyObject::bool_val(has_default),
            default_val,
        ])
    }).collect();
    
    // Store on the class
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("__dataclass_fields__"), PyObject::tuple(fields_info));
        // Mark it as a dataclass
        ns.insert(CompactString::from("__dataclass__"), PyObject::bool_val(true));
    }
    
    Ok(cls.clone())
}

// ── struct module ──

pub fn create_struct_module() -> PyObjectRef {
    make_module("struct", vec![
        ("pack", make_builtin(struct_pack)),
        ("unpack", make_builtin(struct_unpack)),
        ("calcsize", make_builtin(struct_calcsize)),
    ])
}

fn struct_calcsize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("calcsize requires format string")); }
    let fmt = args[0].py_to_string();
    let mut size = 0usize;
    let mut chars = fmt.chars().peekable();
    // Skip byte order
    if let Some(&c) = chars.peek() {
        if "<>!=@".contains(c) { chars.next(); }
    }
    while let Some(c) = chars.next() {
        let count = if c.is_ascii_digit() {
            let mut n = (c as u8 - b'0') as usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() { n = n * 10 + (d as u8 - b'0') as usize; chars.next(); } else { break; }
            }
            let fc = chars.next().unwrap_or('x');
            size += n * format_char_size(fc);
            continue;
        } else { 1 };
        size += count * format_char_size(c);
    }
    Ok(PyObject::int(size as i64))
}

fn format_char_size(c: char) -> usize {
    match c {
        'x' | 'c' | 'b' | 'B' | '?' => 1,
        'h' | 'H' => 2,
        'i' | 'I' | 'l' | 'L' | 'f' => 4,
        'q' | 'Q' | 'd' => 8,
        'n' | 'N' | 'P' => std::mem::size_of::<usize>(),
        's' | 'p' => 1,
        _ => 0,
    }
}

fn struct_pack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("pack requires format string")); }
    let fmt = args[0].py_to_string();
    let mut result = Vec::new();
    let mut arg_idx = 1;
    let mut chars = fmt.chars().peekable();
    let little_endian = match chars.peek() {
        Some('<') => { chars.next(); true }
        Some('>') | Some('!') => { chars.next(); false }
        Some('=') | Some('@') => { chars.next(); cfg!(target_endian = "little") }
        _ => cfg!(target_endian = "little"),
    };
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() { continue; } // count handling simplified
        match c {
            'b' | 'B' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u8;
                result.push(val);
                arg_idx += 1;
            }
            'h' | 'H' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u16;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'i' | 'I' | 'l' | 'L' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u32;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'q' | 'Q' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u64;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'f' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_float()? as f32;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'd' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_float()?;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            '?' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                result.push(if args[arg_idx].is_truthy() { 1 } else { 0 });
                arg_idx += 1;
            }
            'x' => result.push(0),
            _ => {}
        }
    }
    Ok(PyObject::bytes(result))
}

fn struct_unpack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("unpack requires format string and bytes")); }
    let fmt = args[0].py_to_string();
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => b.clone(),
        _ => return Err(PyException::type_error("unpack requires bytes argument")),
    };
    let mut result = Vec::new();
    let mut offset = 0;
    let mut chars = fmt.chars().peekable();
    let little_endian = match chars.peek() {
        Some('<') => { chars.next(); true }
        Some('>') | Some('!') => { chars.next(); false }
        Some('=') | Some('@') => { chars.next(); cfg!(target_endian = "little") }
        _ => cfg!(target_endian = "little"),
    };
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() { continue; }
        match c {
            'b' => {
                if offset >= data.len() { break; }
                result.push(PyObject::int(data[offset] as i8 as i64));
                offset += 1;
            }
            'B' => {
                if offset >= data.len() { break; }
                result.push(PyObject::int(data[offset] as i64));
                offset += 1;
            }
            'h' => {
                if offset + 2 > data.len() { break; }
                let bytes: [u8; 2] = [data[offset], data[offset + 1]];
                let val = if little_endian { i16::from_le_bytes(bytes) } else { i16::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 2;
            }
            'H' => {
                if offset + 2 > data.len() { break; }
                let bytes: [u8; 2] = [data[offset], data[offset + 1]];
                let val = if little_endian { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 2;
            }
            'i' | 'l' => {
                if offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
                let val = if little_endian { i32::from_le_bytes(bytes) } else { i32::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 4;
            }
            'I' | 'L' => {
                if offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
                let val = if little_endian { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 4;
            }
            'q' => {
                if offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[offset..offset+8]);
                let val = if little_endian { i64::from_le_bytes(bytes) } else { i64::from_be_bytes(bytes) };
                result.push(PyObject::int(val));
                offset += 8;
            }
            'Q' => {
                if offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[offset..offset+8]);
                let val = if little_endian { u64::from_le_bytes(bytes) } else { u64::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 8;
            }
            'f' => {
                if offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
                let val = if little_endian { f32::from_le_bytes(bytes) } else { f32::from_be_bytes(bytes) };
                result.push(PyObject::float(val as f64));
                offset += 4;
            }
            'd' => {
                if offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[offset..offset+8]);
                let val = if little_endian { f64::from_le_bytes(bytes) } else { f64::from_be_bytes(bytes) };
                result.push(PyObject::float(val));
                offset += 8;
            }
            '?' => {
                if offset >= data.len() { break; }
                result.push(PyObject::bool_val(data[offset] != 0));
                offset += 1;
            }
            'x' => { offset += 1; }
            _ => {}
        }
    }
    Ok(PyObject::tuple(result))
}

// ── textwrap module ──

pub fn create_textwrap_module() -> PyObjectRef {
    make_module("textwrap", vec![
        ("dedent", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("dedent requires 1 argument")); }
            let text = args[0].py_to_string();
            // Find minimum indentation of non-empty lines
            let mut min_indent = usize::MAX;
            for line in text.lines() {
                if line.trim().is_empty() { continue; }
                let indent = line.len() - line.trim_start().len();
                if indent < min_indent { min_indent = indent; }
            }
            if min_indent == usize::MAX { return Ok(args[0].clone()); }
            let result: Vec<&str> = text.lines().map(|line| {
                if line.trim().is_empty() { line.trim() }
                else if line.len() >= min_indent { &line[min_indent..] }
                else { line }
            }).collect();
            Ok(PyObject::str_val(CompactString::from(result.join("\n"))))
        })),
        ("indent", make_builtin(|args| {
            check_args_min("indent", args, 2)?;
            let text = args[0].py_to_string();
            let prefix = args[1].py_to_string();
            let result: Vec<String> = text.lines().map(|line| {
                if line.trim().is_empty() { line.to_string() }
                else { format!("{}{}", prefix, line) }
            }).collect();
            Ok(PyObject::str_val(CompactString::from(result.join("\n"))))
        })),
        ("wrap", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("wrap requires 1 argument")); }
            let text = args[0].py_to_string();
            let width = if args.len() >= 2 { args[1].to_int().unwrap_or(70) as usize } else { 70 };
            let words: Vec<&str> = text.split_whitespace().collect();
            let mut lines = Vec::new();
            let mut current = String::new();
            for word in words {
                if current.is_empty() {
                    current = word.to_string();
                } else if current.len() + 1 + word.len() <= width {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    lines.push(PyObject::str_val(CompactString::from(current)));
                    current = word.to_string();
                }
            }
            if !current.is_empty() {
                lines.push(PyObject::str_val(CompactString::from(current)));
            }
            Ok(PyObject::list(lines))
        })),
        ("fill", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("fill requires 1 argument")); }
            let text = args[0].py_to_string();
            let width = if args.len() >= 2 { args[1].to_int().unwrap_or(70) as usize } else { 70 };
            let words: Vec<&str> = text.split_whitespace().collect();
            let mut lines = Vec::new();
            let mut current = String::new();
            for word in words {
                if current.is_empty() {
                    current = word.to_string();
                } else if current.len() + 1 + word.len() <= width {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    lines.push(current);
                    current = word.to_string();
                }
            }
            if !current.is_empty() { lines.push(current); }
            Ok(PyObject::str_val(CompactString::from(lines.join("\n"))))
        })),
    ])
}

// ── traceback module (stub) ──

pub fn create_traceback_module() -> PyObjectRef {
    make_module("traceback", vec![
        ("format_exc", make_builtin(|_| Ok(PyObject::str_val(CompactString::from(""))))),
        ("print_exc", make_builtin(|_| Ok(PyObject::none()))),
        ("format_exception", make_builtin(|_| Ok(PyObject::list(vec![])))),
        ("print_stack", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── warnings module (stub) ──

pub fn create_warnings_module() -> PyObjectRef {
    make_module("warnings", vec![
        ("warn", make_builtin(|_| Ok(PyObject::none()))),
        ("filterwarnings", make_builtin(|_| Ok(PyObject::none()))),
        ("simplefilter", make_builtin(|_| Ok(PyObject::none()))),
        ("resetwarnings", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── decimal module (stub) ──

pub fn create_decimal_module() -> PyObjectRef {
    make_module("decimal", vec![
        ("Decimal", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::float(0.0)); }
            let s = args[0].py_to_string();
            match s.parse::<f64>() {
                Ok(f) => Ok(PyObject::float(f)),
                Err(_) => Err(PyException::value_error(format!("Invalid literal for Decimal: '{}'", s))),
            }
        })),
        ("ROUND_HALF_UP", PyObject::str_val(CompactString::from("ROUND_HALF_UP"))),
        ("ROUND_HALF_DOWN", PyObject::str_val(CompactString::from("ROUND_HALF_DOWN"))),
        ("ROUND_CEILING", PyObject::str_val(CompactString::from("ROUND_CEILING"))),
        ("ROUND_FLOOR", PyObject::str_val(CompactString::from("ROUND_FLOOR"))),
        ("getcontext", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── statistics module ──

pub fn create_statistics_module() -> PyObjectRef {
    make_module("statistics", vec![
        ("mean", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("mean requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.is_empty() { return Err(PyException::value_error("mean requires a non-empty dataset")); }
            let sum: f64 = items.iter().map(|x| x.to_float().unwrap_or(0.0)).sum();
            Ok(PyObject::float(sum / items.len() as f64))
        })),
        ("median", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("median requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.is_empty() { return Err(PyException::value_error("median requires a non-empty dataset")); }
            let mut vals: Vec<f64> = items.iter().map(|x| x.to_float().unwrap_or(0.0)).collect();
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = vals.len();
            if n % 2 == 1 { Ok(PyObject::float(vals[n / 2])) }
            else { Ok(PyObject::float((vals[n / 2 - 1] + vals[n / 2]) / 2.0)) }
        })),
        ("stdev", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("stdev requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.len() < 2 { return Err(PyException::value_error("stdev requires at least 2 data points")); }
            let vals: Vec<f64> = items.iter().map(|x| x.to_float().unwrap_or(0.0)).collect();
            let mean: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
            let variance: f64 = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (vals.len() - 1) as f64;
            Ok(PyObject::float(variance.sqrt()))
        })),
        ("variance", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("variance requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.len() < 2 { return Err(PyException::value_error("variance requires at least 2 data points")); }
            let vals: Vec<f64> = items.iter().map(|x| x.to_float().unwrap_or(0.0)).collect();
            let mean: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
            let variance: f64 = vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (vals.len() - 1) as f64;
            Ok(PyObject::float(variance))
        })),
        ("mode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("mode requires 1 argument")); }
            let items = args[0].to_list()?;
            if items.is_empty() { return Err(PyException::value_error("mode requires a non-empty dataset")); }
            let mut counts: IndexMap<String, (PyObjectRef, usize)> = IndexMap::new();
            for item in &items {
                let key = item.py_to_string();
                counts.entry(key).or_insert_with(|| (item.clone(), 0)).1 += 1;
            }
            let max = counts.values().max_by_key(|v| v.1).unwrap();
            Ok(max.0.clone())
        })),
    ])
}

// ── numbers module (stub) ──

pub fn create_numbers_module() -> PyObjectRef {
    make_module("numbers", vec![
        ("Number", PyObject::none()),
        ("Complex", PyObject::none()),
        ("Real", PyObject::none()),
        ("Rational", PyObject::none()),
        ("Integral", PyObject::none()),
    ])
}

// ── platform module ──

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

pub fn create_inspect_module() -> PyObjectRef {
    make_module("inspect", vec![
        ("isfunction", make_builtin(|args| {
            check_args("inspect.isfunction", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Function(_))))
        })),
        ("isclass", make_builtin(|args| {
            check_args("inspect.isclass", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Class(_))))
        })),
        ("ismethod", make_builtin(|args| {
            check_args("inspect.ismethod", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::BoundMethod { .. })))
        })),
        ("ismodule", make_builtin(|args| {
            check_args("inspect.ismodule", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Module(_))))
        })),
        ("isbuiltin", make_builtin(|args| {
            check_args("inspect.isbuiltin", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::NativeFunction { .. } | PyObjectPayload::BuiltinFunction(_) | PyObjectPayload::BuiltinType(_))))
        })),
        ("getmembers", make_builtin(|args| {
            check_args("inspect.getmembers", args, 1)?;
            let dir_names = args[0].dir();
            let dir_list: Vec<PyObjectRef> = dir_names.into_iter().map(|n| PyObject::str_val(n)).collect();
            let names = PyObject::list(dir_list);
            let mut result = Vec::new();
            if let PyObjectPayload::List(items) = &names.payload {
                for item in items.read().iter() {
                    let name_str = item.py_to_string();
                    if let Some(val) = args[0].get_attr(&name_str) {
                        result.push(PyObject::tuple(vec![item.clone(), val]));
                    }
                }
            }
            Ok(PyObject::list(result))
        })),
    ])
}

// ── dis module (stub) ──

pub fn create_dis_module() -> PyObjectRef {
    make_module("dis", vec![
        ("dis", make_builtin(|_| { Ok(PyObject::none()) })),
    ])
}

// ── logging module ──

pub fn create_logging_module() -> PyObjectRef {
    // Logging levels
    let debug_level = PyObject::int(10);
    let info_level = PyObject::int(20);
    let warning_level = PyObject::int(30);
    let error_level = PyObject::int(40);
    let critical_level = PyObject::int(50);

    make_module("logging", vec![
        ("DEBUG", debug_level),
        ("INFO", info_level),
        ("WARNING", warning_level.clone()),
        ("ERROR", error_level),
        ("CRITICAL", critical_level),
        ("NOTSET", PyObject::int(0)),
        ("basicConfig", make_builtin(|_args| { Ok(PyObject::none()) })),
        ("getLogger", make_builtin(logging_get_logger)),
        ("debug", make_builtin(|args| { logging_log(10, args) })),
        ("info", make_builtin(|args| { logging_log(20, args) })),
        ("warning", make_builtin(|args| { logging_log(30, args) })),
        ("error", make_builtin(|args| { logging_log(40, args) })),
        ("critical", make_builtin(|args| { logging_log(50, args) })),
        ("log", make_builtin(|args| {
            if args.len() >= 2 {
                let level = args[0].as_int().unwrap_or(20);
                logging_log(level, &args[1..])
            } else {
                Ok(PyObject::none())
            }
        })),
        ("StreamHandler", make_builtin(|_| Ok(PyObject::none()))),
        ("FileHandler", make_builtin(|_| Ok(PyObject::none()))),
        ("Formatter", make_builtin(|_| Ok(PyObject::none()))),
        ("Handler", make_builtin(|_| Ok(PyObject::none()))),
        ("Logger", make_builtin(logging_get_logger)),
    ])
}

fn logging_log(level: i64, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::none()); }
    let level_name = match level {
        10 => "DEBUG",
        20 => "INFO",
        30 => "WARNING",
        40 => "ERROR",
        50 => "CRITICAL",
        _ => "UNKNOWN",
    };
    let msg = args[0].py_to_string();
    eprintln!("{}:root:{}", level_name, msg);
    Ok(PyObject::none())
}

fn logging_get_logger(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let logger_name = if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
        CompactString::from("root")
    } else {
        CompactString::from(args[0].py_to_string())
    };
    // Return a logger object (Instance of a Logger class)
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("name"), PyObject::str_val(logger_name.clone()));
    ns.insert(CompactString::from("level"), PyObject::int(30)); // WARNING default
    // Logger methods — stored as NativeFunction attrs
    ns.insert(CompactString::from("debug"), make_builtin(move |args| logging_log(10, args)));
    ns.insert(CompactString::from("info"), make_builtin(move |args| logging_log(20, args)));
    ns.insert(CompactString::from("warning"), make_builtin(move |args| logging_log(30, args)));
    ns.insert(CompactString::from("error"), make_builtin(move |args| logging_log(40, args)));
    ns.insert(CompactString::from("critical"), make_builtin(move |args| logging_log(50, args)));
    ns.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
    ns.insert(CompactString::from("addHandler"), make_builtin(|_| Ok(PyObject::none())));
    ns.insert(CompactString::from("removeHandler"), make_builtin(|_| Ok(PyObject::none())));
    ns.insert(CompactString::from("hasHandlers"), make_builtin(|_| Ok(PyObject::bool_val(false))));
    ns.insert(CompactString::from("isEnabledFor"), make_builtin(|_| Ok(PyObject::bool_val(true))));
    ns.insert(CompactString::from("getEffectiveLevel"), make_builtin(|_| Ok(PyObject::int(30))));
    
    let cls = PyObject::class(CompactString::from("Logger"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        for (k, v) in ns {
            attrs.insert(k, v);
        }
    }
    Ok(inst)
}

// ── subprocess module (basic) ──

pub fn create_subprocess_module() -> PyObjectRef {
    make_module("subprocess", vec![
        ("PIPE", PyObject::int(-1)),
        ("STDOUT", PyObject::int(-2)),
        ("DEVNULL", PyObject::int(-3)),
        ("CalledProcessError", make_builtin(|_| Ok(PyObject::none()))),
        ("run", make_builtin(subprocess_run)),
        ("call", make_builtin(subprocess_call)),
        ("check_output", make_builtin(subprocess_check_output)),
        ("check_call", make_builtin(subprocess_call)),
        ("Popen", make_builtin(|_| {
            Err(PyException::runtime_error("subprocess.Popen not implemented"))
        })),
    ])
}

fn subprocess_run(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("subprocess.run requires arguments"));
    }
    let cmd_parts: Vec<String> = args[0].to_list()?.iter().map(|a| a.py_to_string()).collect();
    if cmd_parts.is_empty() {
        return Err(PyException::value_error("empty command"));
    }
    let output = std::process::Command::new(&cmd_parts[0])
        .args(&cmd_parts[1..])
        .output();
    match output {
        Ok(out) => {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("returncode"), PyObject::int(out.status.code().unwrap_or(-1) as i64));
            ns.insert(CompactString::from("stdout"), PyObject::bytes(out.stdout));
            ns.insert(CompactString::from("stderr"), PyObject::bytes(out.stderr));
            let cls = PyObject::class(CompactString::from("CompletedProcess"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(inst_data) = &inst.payload {
                let mut attrs = inst_data.attrs.write();
                for (k, v) in ns { attrs.insert(k, v); }
            }
            Ok(inst)
        }
        Err(e) => Err(PyException::runtime_error(format!("subprocess error: {}", e))),
    }
}

fn subprocess_call(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let result = subprocess_run(args)?;
    if let Some(rc) = result.get_attr("returncode") {
        Ok(rc)
    } else {
        Ok(PyObject::int(0))
    }
}

fn subprocess_check_output(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let result = subprocess_run(args)?;
    if let Some(stdout) = result.get_attr("stdout") {
        Ok(stdout)
    } else {
        Ok(PyObject::bytes(vec![]))
    }
}

// ── pathlib module (basic) ──

pub fn create_pathlib_module() -> PyObjectRef {
    make_module("pathlib", vec![
        ("Path", make_builtin(pathlib_path)),
        ("PurePath", make_builtin(pathlib_path)),
        ("PurePosixPath", make_builtin(pathlib_path)),
        ("PureWindowsPath", make_builtin(pathlib_path)),
    ])
}

fn pathlib_path(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path_str = if args.is_empty() { ".".to_string() } else { args[0].py_to_string() };
    let path = std::path::Path::new(&path_str);
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("_path"), PyObject::str_val(CompactString::from(path_str.as_str())));
    ns.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(
        path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    )));
    ns.insert(CompactString::from("stem"), PyObject::str_val(CompactString::from(
        path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    )));
    ns.insert(CompactString::from("suffix"), PyObject::str_val(CompactString::from(
        path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()
    )));
    ns.insert(CompactString::from("parent"), PyObject::str_val(CompactString::from(
        path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
    )));
    // Methods that need the path are implemented via BuiltinBoundMethod in the VM
    ns.insert(CompactString::from("__pathlib_path__"), PyObject::bool_val(true));

    let cls = PyObject::class(CompactString::from("Path"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        for (k, v) in ns { attrs.insert(k, v); }
    }
    Ok(inst)
}

// ── unittest module (basic) ──

pub fn create_unittest_module() -> PyObjectRef {
    // Create TestCase class
    let mut tc_ns = IndexMap::new();
    tc_ns.insert(CompactString::from("__unittest_testcase__"), PyObject::bool_val(true));
    let test_case = PyObject::class(CompactString::from("TestCase"), vec![], tc_ns);

    make_module("unittest", vec![
        ("TestCase", test_case),
        ("main", make_builtin(|_| Ok(PyObject::none()))),
        ("TestSuite", make_builtin(|_| Ok(PyObject::none()))),
        ("TestLoader", make_builtin(|_| Ok(PyObject::none()))),
        ("TextTestRunner", make_builtin(|_| Ok(PyObject::none()))),
        ("skip", make_builtin(|args| {
            // Return identity decorator
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("skipIf", make_builtin(|_| {
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("expectedFailure", make_builtin(|args| {
            if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
        })),
    ])
}

// ── threading module (basic) ──

pub fn create_threading_module() -> PyObjectRef {
    make_module("threading", vec![
        ("Thread", make_builtin(|_| Ok(PyObject::none()))),
        ("Lock", make_builtin(|_| Ok(PyObject::none()))),
        ("RLock", make_builtin(|_| Ok(PyObject::none()))),
        ("Event", make_builtin(|_| Ok(PyObject::none()))),
        ("Semaphore", make_builtin(|_| Ok(PyObject::none()))),
        ("BoundedSemaphore", make_builtin(|_| Ok(PyObject::none()))),
        ("Condition", make_builtin(|_| Ok(PyObject::none()))),
        ("Barrier", make_builtin(|_| Ok(PyObject::none()))),
        ("Timer", make_builtin(|_| Ok(PyObject::none()))),
        ("current_thread", make_builtin(|_| {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("MainThread")));
            ns.insert(CompactString::from("ident"), PyObject::int(1));
            ns.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            ns.insert(CompactString::from("is_alive"), make_builtin(|_| Ok(PyObject::bool_val(true))));
            ns.insert(CompactString::from("getName"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("MainThread")))));
            let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(i) = &inst.payload {
                let mut attrs = i.attrs.write();
                for (k, v) in ns { attrs.insert(k, v); }
            }
            Ok(inst)
        })),
        ("active_count", make_builtin(|_| Ok(PyObject::int(1)))),
        ("enumerate", make_builtin(|_| Ok(PyObject::list(vec![])))),
        ("main_thread", make_builtin(|_| Ok(PyObject::none()))),
        ("local", make_builtin(|_| {
            // Thread-local storage — return a simple object
            let cls = PyObject::class(CompactString::from("local"), vec![], IndexMap::new());
            Ok(PyObject::instance(cls))
        })),
    ])
}

// ── csv module (basic) ──

pub fn create_csv_module() -> PyObjectRef {
    make_module("csv", vec![
        ("reader", make_builtin(csv_reader)),
        ("writer", make_builtin(|_| Ok(PyObject::none()))),
        ("DictReader", make_builtin(|_| Ok(PyObject::none()))),
        ("DictWriter", make_builtin(|_| Ok(PyObject::none()))),
        ("QUOTE_ALL", PyObject::int(1)),
        ("QUOTE_MINIMAL", PyObject::int(0)),
        ("QUOTE_NONNUMERIC", PyObject::int(2)),
        ("QUOTE_NONE", PyObject::int(3)),
    ])
}

fn csv_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.reader requires an iterable"));
    }
    // Convert iterable of strings into list of lists
    let lines = args[0].to_list()?;
    let mut rows = Vec::new();
    for line in &lines {
        let s = line.py_to_string();
        let fields: Vec<PyObjectRef> = s.split(',')
            .map(|f| {
                let f = f.trim();
                let f = if f.starts_with('"') && f.ends_with('"') {
                    &f[1..f.len()-1]
                } else {
                    f
                };
                PyObject::str_val(CompactString::from(f))
            })
            .collect();
        rows.push(PyObject::list(fields));
    }
    Ok(PyObject::list(rows))
}

// ── shutil module (basic) ──

pub fn create_shutil_module() -> PyObjectRef {
    make_module("shutil", vec![
        ("copy", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("copy requires src and dst")); }
            let src = args[0].py_to_string();
            let dst = args[1].py_to_string();
            std::fs::copy(&src, &dst).map_err(|e| PyException::runtime_error(format!("{}", e)))?;
            Ok(PyObject::str_val(CompactString::from(dst)))
        })),
        ("copy2", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("copy2 requires src and dst")); }
            let src = args[0].py_to_string();
            let dst = args[1].py_to_string();
            std::fs::copy(&src, &dst).map_err(|e| PyException::runtime_error(format!("{}", e)))?;
            Ok(PyObject::str_val(CompactString::from(dst)))
        })),
        ("rmtree", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("rmtree requires path")); }
            let path = args[0].py_to_string();
            std::fs::remove_dir_all(&path).map_err(|e| PyException::runtime_error(format!("{}", e)))?;
            Ok(PyObject::none())
        })),
        ("move", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("move requires src and dst")); }
            let src = args[0].py_to_string();
            let dst = args[1].py_to_string();
            std::fs::rename(&src, &dst).map_err(|e| PyException::runtime_error(format!("{}", e)))?;
            Ok(PyObject::str_val(CompactString::from(dst)))
        })),
        ("which", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::none()); }
            let name = args[0].py_to_string();
            if let Ok(path) = std::env::var("PATH") {
                for dir in path.split(':') {
                    let candidate = std::path::Path::new(dir).join(&name);
                    if candidate.exists() {
                        return Ok(PyObject::str_val(CompactString::from(candidate.to_string_lossy().to_string())));
                    }
                }
            }
            Ok(PyObject::none())
        })),
        ("disk_usage", make_builtin(|_| Ok(PyObject::none()))),
        ("get_terminal_size", make_builtin(|_| {
            Ok(PyObject::tuple(vec![PyObject::int(80), PyObject::int(24)]))
        })),
    ])
}

// ── glob module ──

pub fn create_glob_module() -> PyObjectRef {
    make_module("glob", vec![
        ("glob", make_builtin(glob_glob)),
        ("iglob", make_builtin(glob_glob)),
    ])
}

fn glob_glob(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("glob requires a pattern"));
    }
    let pattern = args[0].py_to_string();
    // Basic glob: handle *, ?, but not **
    // Use std::fs for simple patterns
    let path = std::path::Path::new(&pattern);
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let file_pattern = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    
    let mut results = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if glob_match(&file_pattern, &name) {
                let full = entry.path().to_string_lossy().to_string();
                results.push(PyObject::str_val(CompactString::from(full)));
            }
        }
    }
    Ok(PyObject::list(results))
}

fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" { return true; }
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == text;
    }
    // Simple wildcard matching
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        // No *, just ? wildcards
        if pattern.len() != text.len() { return false; }
        return pattern.chars().zip(text.chars()).all(|(p, t)| p == '?' || p == t);
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        if let Some(idx) = text[pos..].find(part) {
            if i == 0 && idx != 0 { return false; }
            pos += idx + part.len();
        } else {
            return false;
        }
    }
    if !parts.last().unwrap_or(&"").is_empty() {
        return pos == text.len();
    }
    true
}

// ── tempfile module (basic) ──

pub fn create_tempfile_module() -> PyObjectRef {
    make_module("tempfile", vec![
        ("gettempdir", make_builtin(|_| {
            Ok(PyObject::str_val(CompactString::from(
                std::env::temp_dir().to_string_lossy().to_string()
            )))
        })),
        ("mkdtemp", make_builtin(|_| {
            let dir = std::env::temp_dir().join(format!("ferrython_tmp_{}", std::process::id()));
            std::fs::create_dir_all(&dir).ok();
            Ok(PyObject::str_val(CompactString::from(dir.to_string_lossy().to_string())))
        })),
        ("NamedTemporaryFile", make_builtin(|_| Ok(PyObject::none()))),
        ("TemporaryDirectory", make_builtin(|_| Ok(PyObject::none()))),
        ("mkstemp", make_builtin(|_| {
            let path = std::env::temp_dir().join(format!("ferrython_{}", std::process::id()));
            Ok(PyObject::tuple(vec![PyObject::int(0), PyObject::str_val(CompactString::from(path.to_string_lossy().to_string()))]))
        })),
    ])
}

// ── fnmatch module ──

pub fn create_fnmatch_module() -> PyObjectRef {
    make_module("fnmatch", vec![
        ("fnmatch", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("fnmatch requires name and pattern")); }
            let name = args[0].py_to_string();
            let pattern = args[1].py_to_string();
            Ok(PyObject::bool_val(glob_match(&pattern, &name)))
        })),
        ("fnmatchcase", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("fnmatchcase requires name and pattern")); }
            let name = args[0].py_to_string();
            let pattern = args[1].py_to_string();
            Ok(PyObject::bool_val(glob_match(&pattern, &name)))
        })),
        ("filter", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("filter requires names and pattern")); }
            let names = args[0].to_list()?;
            let pattern = args[1].py_to_string();
            let filtered: Vec<PyObjectRef> = names.iter()
                .filter(|n| glob_match(&pattern, &n.py_to_string()))
                .cloned().collect();
            Ok(PyObject::list(filtered))
        })),
    ])
}

// ── base64 module ──

pub fn create_base64_module() -> PyObjectRef {
    make_module("base64", vec![
        ("b64encode", make_builtin(base64_encode)),
        ("b64decode", make_builtin(base64_decode)),
        ("b16encode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b16encode requires data")); }
            let data = extract_bytes(&args[0])?;
            let hex: String = data.iter().map(|b| format!("{:02X}", b)).collect();
            Ok(PyObject::bytes(hex.into_bytes()))
        })),
        ("b16decode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b16decode requires data")); }
            let s = args[0].py_to_string();
            let bytes: Vec<u8> = (0..s.len())
                .step_by(2)
                .filter_map(|i| u8::from_str_radix(&s[i..i+2], 16).ok())
                .collect();
            Ok(PyObject::bytes(bytes))
        })),
        ("encodebytes", make_builtin(base64_encode)),
        ("decodebytes", make_builtin(base64_decode)),
    ])
}

fn extract_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.clone()),
        PyObjectPayload::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(PyException::type_error("expected bytes-like object")),
    }
}

fn base64_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("b64encode requires data")); }
    let data = extract_bytes(&args[0])?;
    // Simple base64 encoding
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize]);
        result.push(CHARS[((n >> 12) & 63) as usize]);
        if chunk.len() > 1 { result.push(CHARS[((n >> 6) & 63) as usize]); } else { result.push(b'='); }
        if chunk.len() > 2 { result.push(CHARS[(n & 63) as usize]); } else { result.push(b'='); }
    }
    Ok(PyObject::bytes(result))
}

fn base64_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("b64decode requires data")); }
    let input_bytes = extract_bytes(&args[0])?;
    let input: Vec<u8> = input_bytes.iter().copied().filter(|&b| b != b'\n' && b != b'\r').collect();
    fn decode_char(c: u8) -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    }
    let mut result = Vec::new();
    for chunk in input.chunks(4) {
        if chunk.len() < 4 { break; }
        let n = (decode_char(chunk[0]) << 18) | (decode_char(chunk[1]) << 12) | (decode_char(chunk[2]) << 6) | decode_char(chunk[3]);
        result.push((n >> 16) as u8);
        if chunk[2] != b'=' { result.push((n >> 8) as u8); }
        if chunk[3] != b'=' { result.push(n as u8); }
    }
    Ok(PyObject::bytes(result))
}

// ── pprint module ──

pub fn create_pprint_module() -> PyObjectRef {
    make_module("pprint", vec![
        ("pprint", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::none()); }
            println!("{}", args[0].py_to_string());
            Ok(PyObject::none())
        })),
        ("pformat", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
        })),
        ("PrettyPrinter", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── argparse module (basic) ──

pub fn create_argparse_module() -> PyObjectRef {
    let mut ap_ns = IndexMap::new();
    ap_ns.insert(CompactString::from("__argparse__"), PyObject::bool_val(true));
    let argument_parser = PyObject::class(CompactString::from("ArgumentParser"), vec![], ap_ns);

    make_module("argparse", vec![
        ("ArgumentParser", argument_parser),
        ("Namespace", make_builtin(|_| Ok(PyObject::none()))),
        ("Action", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── datetime module ──

pub fn create_datetime_module() -> PyObjectRef {
    let datetime_cls = make_module("datetime", vec![
        ("now", make_builtin(datetime_now)),
        ("today", make_builtin(datetime_now)),
        ("utcnow", make_builtin(datetime_now)),
        ("fromisoformat", make_builtin(datetime_fromisoformat)),
    ]);
    let date_cls = make_module("date", vec![
        ("today", make_builtin(date_today)),
        ("fromisoformat", make_builtin(datetime_fromisoformat)),
    ]);
    make_module("datetime", vec![
        ("datetime", datetime_cls),
        ("date", date_cls),
        ("time", make_builtin(datetime_time_obj)),
        ("timedelta", make_builtin(datetime_timedelta)),
        ("MINYEAR", PyObject::int(1)),
        ("MAXYEAR", PyObject::int(9999)),
    ])
}

fn datetime_now(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let micros = now.subsec_micros();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hour = (time_of_day / 3600) as i64;
    let minute = ((time_of_day % 3600) / 60) as i64;
    let second = (time_of_day % 60) as i64;
    let (year, month, day) = days_to_ymd(days as i64 + 719468);
    Ok(make_datetime_instance(year, month, day, hour, minute, second, micros as i64))
}

fn date_today(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let days = now.as_secs() / 86400;
    let (year, month, day) = days_to_ymd(days as i64 + 719468);
    let class = PyObject::class(CompactString::from("date"), vec![], IndexMap::new());
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("year"), PyObject::int(year));
        w.insert(CompactString::from("month"), PyObject::int(month));
        w.insert(CompactString::from("day"), PyObject::int(day));
    }
    Ok(inst)
}

fn datetime_fromisoformat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("fromisoformat", args, 1)?;
    let s = args[0].py_to_string();
    // Parse ISO format: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS
    let parts: Vec<&str> = s.split('T').collect();
    let date_parts: Vec<&str> = parts[0].split('-').collect();
    if date_parts.len() < 3 { return Err(PyException::value_error("Invalid isoformat")); }
    let year: i64 = date_parts[0].parse().map_err(|_| PyException::value_error("Invalid year"))?;
    let month: i64 = date_parts[1].parse().map_err(|_| PyException::value_error("Invalid month"))?;
    let day: i64 = date_parts[2].parse().map_err(|_| PyException::value_error("Invalid day"))?;
    let (hour, minute, second) = if parts.len() > 1 {
        let time_parts: Vec<&str> = parts[1].split(':').collect();
        let h: i64 = time_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let m: i64 = time_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let sec: i64 = time_parts.get(2).and_then(|s| s.split('.').next().unwrap_or("0").parse().ok()).unwrap_or(0);
        (h, m, sec)
    } else { (0, 0, 0) };
    Ok(make_datetime_instance(year, month, day, hour, minute, second, 0))
}

fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    // Civil days from epoch to Y-M-D (algorithm from Howard Hinnant)
    let z = days;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2) / 153;
    let d = doy - (153*mp + 2)/5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn make_datetime_instance(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64, microsecond: i64) -> PyObjectRef {
    let class = PyObject::class(CompactString::from("datetime"), vec![], IndexMap::new());
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("year"), PyObject::int(year));
        w.insert(CompactString::from("month"), PyObject::int(month));
        w.insert(CompactString::from("day"), PyObject::int(day));
        w.insert(CompactString::from("hour"), PyObject::int(hour));
        w.insert(CompactString::from("minute"), PyObject::int(minute));
        w.insert(CompactString::from("second"), PyObject::int(second));
        w.insert(CompactString::from("microsecond"), PyObject::int(microsecond));
    }
    inst
}

fn datetime_time_obj(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let hour = if !args.is_empty() { args[0].to_int()? } else { 0 };
    let minute = if args.len() > 1 { args[1].to_int()? } else { 0 };
    let second = if args.len() > 2 { args[2].to_int()? } else { 0 };
    let microsecond = if args.len() > 3 { args[3].to_int()? } else { 0 };
    let class = PyObject::class(CompactString::from("time"), vec![], IndexMap::new());
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("hour"), PyObject::int(hour));
        w.insert(CompactString::from("minute"), PyObject::int(minute));
        w.insert(CompactString::from("second"), PyObject::int(second));
        w.insert(CompactString::from("microsecond"), PyObject::int(microsecond));
    }
    Ok(inst)
}

fn datetime_timedelta(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let days = if !args.is_empty() { args[0].to_int()? } else { 0 };
    let seconds = if args.len() > 1 { args[1].to_int()? } else { 0 };
    let microseconds = if args.len() > 2 { args[2].to_int()? } else { 0 };
    let total_seconds = days * 86400 + seconds;
    let class = PyObject::class(CompactString::from("timedelta"), vec![], IndexMap::new());
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("days"), PyObject::int(days));
        w.insert(CompactString::from("seconds"), PyObject::int(seconds));
        w.insert(CompactString::from("microseconds"), PyObject::int(microseconds));
        w.insert(CompactString::from("total_seconds"), PyObject::float(total_seconds as f64 + microseconds as f64 / 1_000_000.0));
    }
    Ok(inst)
}

// ── weakref module ──

pub fn create_weakref_module() -> PyObjectRef {
    make_module("weakref", vec![
        ("ref", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("ref requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("proxy", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("proxy requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("WeakValueDictionary", make_builtin(|_| Ok(PyObject::dict(IndexMap::new())))),
        ("WeakKeyDictionary", make_builtin(|_| Ok(PyObject::dict(IndexMap::new())))),
        ("WeakSet", make_builtin(|_| Ok(PyObject::set(IndexMap::new())))),
    ])
}

