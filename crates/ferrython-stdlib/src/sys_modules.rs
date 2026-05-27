//! System, OS, and platform stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min, make_builtin, make_module, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, SharedGlobals};
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

mod atexit;
mod errno;
mod fcntl;
mod getpass;
mod grp;
mod locale;
mod mmap;
mod os_path;
mod platform;
mod pwd;
mod resource;
mod sched;
mod site;
mod sysconfig;

pub use atexit::{create_atexit_module, register_atexit_callback, unregister_atexit_callback};
pub use errno::create_errno_module;
pub use fcntl::create_fcntl_module;
pub use getpass::create_getpass_module;
pub use grp::create_grp_module;
pub use locale::{create_locale_module, get_current_ctype_locale};
pub use mmap::create_mmap_module;
pub use os_path::create_os_path_module;
pub use platform::create_platform_module;
pub use pwd::create_pwd_module;
pub use resource::create_resource_module;
pub use sched::create_sched_module;
pub use site::create_site_module;
pub use sysconfig::create_sysconfig_module;

static RECURSION_LIMIT: AtomicI64 = AtomicI64::new(1000);
const INT_MAX_STR_DIGITS_THRESHOLD: i64 = 640;

/// Fast atomic flags indicating whether trace/profile functions are installed.
/// The VM reads these instead of doing thread-local RefCell access per frame.
static TRACE_ACTIVE: AtomicBool = AtomicBool::new(false);
static PROFILE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The argv to expose as sys.argv when the sys module is first loaded.
/// Set by the CLI before execution via `ferrython_stdlib::set_argv()`.
static SYS_ARGV: std::sync::LazyLock<parking_lot::RwLock<Vec<String>>> =
    std::sync::LazyLock::new(|| parking_lot::RwLock::new(vec![String::new()]));

/// Set the process argv that will be exposed as `sys.argv`.
/// Must be called before the first `import sys`.
pub fn set_argv(args: Vec<String>) {
    *SYS_ARGV.write() = args;
}

/// Get the current sys.argv values.
pub fn get_argv() -> Vec<String> {
    SYS_ARGV.read().clone()
}

/// Check if any trace function is active (atomic load — ~1ns vs ~15ns for thread-local).
#[inline(always)]
pub fn is_trace_active() -> bool {
    TRACE_ACTIVE.load(Ordering::Relaxed)
}

/// Check if any profile function is active (atomic load).
#[inline(always)]
pub fn is_profile_active() -> bool {
    PROFILE_ACTIVE.load(Ordering::Relaxed)
}

// Thread-local trace/profile hooks and current frame.
thread_local! {
    static TRACE_FUNC: std::cell::RefCell<Option<PyObjectRef>> =
        const { std::cell::RefCell::new(None) };
    static PROFILE_FUNC: std::cell::RefCell<Option<PyObjectRef>> =
        const { std::cell::RefCell::new(None) };
    static EXCEPT_HOOK: std::cell::RefCell<Option<PyObjectRef>> =
        const { std::cell::RefCell::new(None) };
    static CURRENT_FRAME: std::cell::RefCell<Option<PyObjectRef>> =
        const { std::cell::RefCell::new(None) };
    static CURRENT_GLOBALS: std::cell::RefCell<Option<SharedGlobals>> =
        const { std::cell::RefCell::new(None) };
    static CURRENT_SYS_MODULE: std::cell::RefCell<Option<PyObjectRef>> =
        const { std::cell::RefCell::new(None) };
}

/// Read active exception info for traceback.format_exc() etc.
/// Reads lazily through the VM's active_exception pointer.
pub fn get_exc_info() -> Option<(ExceptionKind, CompactString)> {
    ferrython_core::error::get_active_exc_info()
}

/// Get the current trace function (for VM hook dispatch).
pub fn get_trace_func() -> Option<PyObjectRef> {
    TRACE_FUNC.with(|c| c.borrow().clone())
}

/// Set the trace function (called by sys.settrace).
pub fn set_trace_func(func: Option<PyObjectRef>) {
    TRACE_ACTIVE.store(func.is_some(), Ordering::Relaxed);
    TRACE_FUNC.with(|c| *c.borrow_mut() = func);
}

/// Get the current profile function (for VM hook dispatch).
pub fn get_profile_func() -> Option<PyObjectRef> {
    PROFILE_FUNC.with(|c| c.borrow().clone())
}

/// Set the profile function (called by sys.setprofile).
pub fn set_profile_func(func: Option<PyObjectRef>) {
    PROFILE_ACTIVE.store(func.is_some(), Ordering::Relaxed);
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

/// Set the current globals (called by VM at script entry).
pub fn set_current_globals(g: Option<SharedGlobals>) {
    CURRENT_GLOBALS.with(|c| *c.borrow_mut() = g);
}

/// Get the current globals (for _getframe fallback).
pub fn get_current_globals() -> Option<SharedGlobals> {
    CURRENT_GLOBALS.with(|c| c.borrow().clone())
}

/// Set the current frame (called by VM before native function dispatch).
pub fn set_current_frame(frame: Option<PyObjectRef>) {
    CURRENT_FRAME.with(|c| *c.borrow_mut() = frame);
}

/// Get the current frame (for sys._getframe).
pub fn get_current_frame() -> Option<PyObjectRef> {
    CURRENT_FRAME.with(|c| c.borrow().clone())
}

/// Get the active sys.stdout object, including Python-level replacements.
pub fn get_current_stdout() -> Option<PyObjectRef> {
    CURRENT_SYS_MODULE.with(|c| c.borrow().as_ref().and_then(|sys| sys.get_attr("stdout")))
}

/// Get the active sys module, including Python-level attribute replacements.
pub fn get_current_sys_module() -> Option<PyObjectRef> {
    CURRENT_SYS_MODULE.with(|c| c.borrow().clone())
}

/// Get the current recursion limit (for VM stack depth checking).
pub fn get_recursion_limit() -> i64 {
    RECURSION_LIMIT.load(Ordering::Relaxed)
}

pub fn create_sys_module() -> PyObjectRef {
    let module = make_module("sys", vec![
        ("version", PyObject::str_val(CompactString::from("3.8.0 (ferrython)"))),
        ("version_info", {
            let vi_attrs = IndexMap::from([
                (CompactString::from("major"), PyObject::int(3)),
                (CompactString::from("minor"), PyObject::int(8)),
                (CompactString::from("micro"), PyObject::int(0)),
                (CompactString::from("releaselevel"), PyObject::str_val(CompactString::from("final"))),
                (CompactString::from("serial"), PyObject::int(0)),
            ]);
            let cls = PyObject::class(CompactString::from("sys.version_info"), vec![PyObject::builtin_type(CompactString::from("tuple"))], IndexMap::new());
            let inst = PyObject::instance_with_attrs(cls, vi_attrs);
            // Also make it tuple-like for code that indexes sys.version_info[0]
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                    "version_info.__getitem__", |args: &[PyObjectRef]| {
                        // VM may pass (self, key) or just (key)
                        let idx_obj = if args.len() >= 2 { &args[1] } else if !args.is_empty() { &args[0] } else {
                            return Err(PyException::type_error("__getitem__ requires index"));
                        };
                        let items = vec![
                            PyObject::int(3), PyObject::int(8), PyObject::int(0),
                            PyObject::str_val(CompactString::from("final")), PyObject::int(0),
                        ];
                        // Handle slice
                        if let PyObjectPayload::Slice(sd) = &idx_obj.payload {
                            let len = 5i64;
                            let s = sd.start.as_ref().and_then(|v| v.as_int()).unwrap_or(0);
                            let e = sd.stop.as_ref().and_then(|v| v.as_int()).unwrap_or(len);
                            let st = sd.step.as_ref().and_then(|v| v.as_int()).unwrap_or(1);
                            let s = if s < 0 { (len + s).max(0) } else { s.min(len) } as usize;
                            let e = if e < 0 { (len + e).max(0) } else { e.min(len) } as usize;
                            if st == 1 && s <= e {
                                return Ok(PyObject::tuple(items[s..e].to_vec()));
                            }
                            // General step
                            let mut result = Vec::new();
                            let mut i = s as i64;
                            while (st > 0 && i < e as i64) || (st < 0 && i > e as i64) {
                                if i >= 0 && (i as usize) < items.len() {
                                    result.push(items[i as usize].clone());
                                }
                                i += st;
                            }
                            return Ok(PyObject::tuple(result));
                        }
                        let idx = idx_obj.as_int().unwrap_or(0);
                        let idx = if idx < 0 { 5 + idx } else { idx } as usize;
                        if idx < items.len() {
                            Ok(items[idx].clone())
                        } else {
                            Err(PyException::index_error("version_info index out of range"))
                        }
                    }
                ));
                attrs.insert(CompactString::from("__len__"), PyObject::native_closure(
                    "version_info.__len__", |_: &[PyObjectRef]| Ok(PyObject::int(5))
                ));
                attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
                    "version_info.__repr__", |_: &[PyObjectRef]| {
                        Ok(PyObject::str_val(CompactString::from("sys.version_info(major=3, minor=8, micro=0, releaselevel='final', serial=0)")))
                    }
                ));
                let ge_fn = PyObject::native_closure("version_info.__ge__", |args: &[PyObjectRef]| {
                    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
                    if let PyObjectPayload::Tuple(t) = &args[1].payload {
                        let self_parts = [3i64, 8, 0];
                        for (i, sp) in self_parts.iter().enumerate() {
                            let other_val = t.get(i).and_then(|v| v.as_int()).unwrap_or(0);
                            if *sp > other_val { return Ok(PyObject::bool_val(true)); }
                            if *sp < other_val { return Ok(PyObject::bool_val(false)); }
                        }
                        return Ok(PyObject::bool_val(true)); // equal
                    }
                    Ok(PyObject::bool_val(false))
                });
                attrs.insert(CompactString::from("__ge__"), ge_fn);
                let lt_fn = PyObject::native_closure("version_info.__lt__", |args: &[PyObjectRef]| {
                    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
                    if let PyObjectPayload::Tuple(t) = &args[1].payload {
                        let self_parts = [3i64, 8, 0];
                        for (i, sp) in self_parts.iter().enumerate() {
                            let other_val = t.get(i).and_then(|v| v.as_int()).unwrap_or(0);
                            if *sp < other_val { return Ok(PyObject::bool_val(true)); }
                            if *sp > other_val { return Ok(PyObject::bool_val(false)); }
                        }
                        // If we've exhausted self_parts but other tuple is longer, self is "less"
                        return Ok(PyObject::bool_val(self_parts.len() < t.len()));
                    }
                    Ok(PyObject::bool_val(false))
                });
                attrs.insert(CompactString::from("__lt__"), lt_fn);
                let gt_fn = PyObject::native_closure("version_info.__gt__", |args: &[PyObjectRef]| {
                    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
                    if let PyObjectPayload::Tuple(t) = &args[1].payload {
                        let self_parts = [3i64, 8, 0];
                        let cmp_len = self_parts.len().min(t.len());
                        for i in 0..cmp_len {
                            let other_val = t.get(i).and_then(|v| v.as_int()).unwrap_or(0);
                            if self_parts[i] > other_val { return Ok(PyObject::bool_val(true)); }
                            if self_parts[i] < other_val { return Ok(PyObject::bool_val(false)); }
                        }
                        // Equal up to min length — longer one is greater
                        return Ok(PyObject::bool_val(self_parts.len() > t.len()));
                    }
                    Ok(PyObject::bool_val(false))
                });
                attrs.insert(CompactString::from("__gt__"), gt_fn);
                let le_fn = PyObject::native_closure("version_info.__le__", |args: &[PyObjectRef]| {
                    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
                    if let PyObjectPayload::Tuple(t) = &args[1].payload {
                        let self_parts = [3i64, 8, 0];
                        let cmp_len = self_parts.len().min(t.len());
                        for i in 0..cmp_len {
                            let other_val = t.get(i).and_then(|v| v.as_int()).unwrap_or(0);
                            if self_parts[i] < other_val { return Ok(PyObject::bool_val(true)); }
                            if self_parts[i] > other_val { return Ok(PyObject::bool_val(false)); }
                        }
                        return Ok(PyObject::bool_val(self_parts.len() <= t.len()));
                    }
                    Ok(PyObject::bool_val(false))
                });
                attrs.insert(CompactString::from("__le__"), le_fn);
                let eq_fn = PyObject::native_closure("version_info.__eq__", |args: &[PyObjectRef]| {
                    if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
                    if let PyObjectPayload::Tuple(t) = &args[1].payload {
                        let self_parts = [3i64, 8, 0];
                        if t.len() != self_parts.len() { return Ok(PyObject::bool_val(false)); }
                        for (i, sp) in self_parts.iter().enumerate() {
                            let other_val = t.get(i).and_then(|v| v.as_int()).unwrap_or(-1);
                            if *sp != other_val { return Ok(PyObject::bool_val(false)); }
                        }
                        return Ok(PyObject::bool_val(true));
                    }
                    Ok(PyObject::bool_val(false))
                });
                attrs.insert(CompactString::from("__eq__"), eq_fn);
                attrs.insert(CompactString::from("__iter__"), PyObject::native_closure(
                    "version_info.__iter__", |_: &[PyObjectRef]| {
                        Ok(PyObject::tuple(vec![
                            PyObject::int(3), PyObject::int(8), PyObject::int(0),
                            PyObject::str_val(CompactString::from("final")), PyObject::int(0),
                        ]))
                    }
                ));
            }
            inst
        }),
        ("platform", PyObject::str_val(CompactString::from(std::env::consts::OS))),
        ("executable", PyObject::str_val(CompactString::from("ferrython"))),
        ("argv", {
            let argv_strs = SYS_ARGV.read();
            PyObject::list(
                argv_strs.iter()
                    .map(|s| PyObject::str_val(CompactString::from(s.as_str())))
                    .collect()
            )
        }),
        ("path", {
            // Build sys.path from PYTHONPATH env + import search paths + cwd
            let mut path_items: Vec<PyObjectRef> = Vec::new();
            path_items.push(PyObject::str_val(CompactString::from("")));
            if let Ok(pypath) = std::env::var("PYTHONPATH") {
                for p in std::env::split_paths(&pypath) {
                    path_items.push(PyObject::str_val(
                        CompactString::from(p.to_string_lossy().as_ref()),
                    ));
                }
            }
            // Include import system search paths (stdlib, site-packages)
            let mut seen = std::collections::HashSet::new();
            seen.insert(String::new());
            seen.insert(".".to_string());
            for s in ferrython_core::get_extra_sys_paths() {
                if seen.insert(s.clone()) {
                    path_items.push(PyObject::str_val(CompactString::from(&s)));
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
        ("implementation", {
            let mut impl_attrs = IndexMap::new();
            impl_attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("ferrython")));
            impl_attrs.insert(CompactString::from("version"), PyObject::tuple(vec![
                PyObject::int(3), PyObject::int(8), PyObject::int(0),
                PyObject::str_val(CompactString::from("final")), PyObject::int(0),
            ]));
            impl_attrs.insert(CompactString::from("cache_tag"), PyObject::str_val(CompactString::from("ferrython-38")));
            impl_attrs.insert(CompactString::from("hexversion"), PyObject::int(0x030800f0));
            let cls = PyObject::class(CompactString::from("sys.implementation"), vec![], IndexMap::new());
            PyObject::instance_with_attrs(cls, impl_attrs)
        }),
        ("stdin", make_stdio_object("<stdin>", "r", 0)),
        ("stdout", make_stdio_object("<stdout>", "w", 1)),
        ("stderr", make_stdio_object("<stderr>", "w", 2)),
        ("__stdin__", make_stdio_object("<stdin>", "r", 0)),
        ("__stdout__", make_stdio_object("<stdout>", "w", 1)),
        ("__stderr__", make_stdio_object("<stderr>", "w", 2)),
        ("getrecursionlimit", make_builtin(sys_getrecursionlimit)),
        ("setrecursionlimit", make_builtin(sys_setrecursionlimit)),
        ("get_int_max_str_digits", make_builtin(sys_get_int_max_str_digits)),
        ("set_int_max_str_digits", make_builtin(sys_set_int_max_str_digits)),
        ("exit", make_builtin(sys_exit)),
        ("getsizeof", make_builtin(sys_getsizeof)),
        ("getrefcount", make_builtin(|args| {
            check_args("sys.getrefcount", args, 1)?;
            // Return Arc strong_count + 1 (for the arg reference itself, matching CPython)
            let count = PyObjectRef::strong_count(&args[0]) as i64;
            Ok(PyObject::int(count + 1))
        })),
        ("settrace", make_builtin(sys_settrace)),
        ("gettrace", make_builtin(sys_gettrace)),
        ("setprofile", make_builtin(sys_setprofile)),
        ("getprofile", make_builtin(sys_getprofile)),
        ("excepthook", make_builtin(sys_excepthook_default)),
        ("__excepthook__", make_builtin(sys_excepthook_default)),
        ("unraisablehook", make_builtin(sys_unraisablehook_default)),
        ("__unraisablehook__", make_builtin(sys_unraisablehook_default)),
        ("getdefaultencoding", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("utf-8"))))),
        ("getfilesystemencoding", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("utf-8"))))),
        ("intern", make_builtin(|args| { check_args("sys.intern", args, 1)?; Ok(args[0].clone()) })),
        ("flags", {
            // CPython sys.flags is a named structseq with attribute access
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("__namedtuple__"), PyObject::bool_val(true));
            ns.insert(CompactString::from("_fields"), PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("debug")),
                PyObject::str_val(CompactString::from("inspect")),
                PyObject::str_val(CompactString::from("interactive")),
                PyObject::str_val(CompactString::from("optimize")),
                PyObject::str_val(CompactString::from("dont_write_bytecode")),
                PyObject::str_val(CompactString::from("no_user_site")),
                PyObject::str_val(CompactString::from("no_site")),
                PyObject::str_val(CompactString::from("ignore_environment")),
                PyObject::str_val(CompactString::from("verbose")),
                PyObject::str_val(CompactString::from("bytes_warning")),
                PyObject::str_val(CompactString::from("quiet")),
                PyObject::str_val(CompactString::from("hash_randomization")),
                PyObject::str_val(CompactString::from("isolated")),
                PyObject::str_val(CompactString::from("dev_mode")),
                PyObject::str_val(CompactString::from("utf8_mode")),
            ]));
            ns.insert(CompactString::from("_field_defaults"), PyObject::dict(IndexMap::new()));
            let cls = PyObject::class(CompactString::from("sys.flags"), vec![], ns);
            let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
            attrs.insert(CompactString::from("debug"), PyObject::int(0));
            attrs.insert(CompactString::from("inspect"), PyObject::int(0));
            attrs.insert(CompactString::from("interactive"), PyObject::int(0));
            attrs.insert(CompactString::from("optimize"), PyObject::int(0));
            attrs.insert(CompactString::from("dont_write_bytecode"), PyObject::int(0));
            attrs.insert(CompactString::from("no_user_site"), PyObject::int(0));
            attrs.insert(CompactString::from("no_site"), PyObject::int(0));
            attrs.insert(CompactString::from("ignore_environment"), PyObject::int(0));
            attrs.insert(CompactString::from("verbose"), PyObject::int(0));
            attrs.insert(CompactString::from("bytes_warning"), PyObject::int(0));
            attrs.insert(CompactString::from("quiet"), PyObject::int(0));
            attrs.insert(CompactString::from("hash_randomization"), PyObject::int(0));
            attrs.insert(CompactString::from("isolated"), PyObject::int(0));
            attrs.insert(CompactString::from("dev_mode"), PyObject::bool_val(false));
            attrs.insert(CompactString::from("utf8_mode"), PyObject::int(0));
            let inst = PyObject::instance_with_attrs(cls, attrs);
            inst
        }),
        ("float_info", {
            // CPython sys.float_info is a named structseq — we use a namedtuple-style class
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("__namedtuple__"), PyObject::bool_val(true));
            ns.insert(CompactString::from("_fields"), PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("max")),
                PyObject::str_val(CompactString::from("max_exp")),
                PyObject::str_val(CompactString::from("min")),
                PyObject::str_val(CompactString::from("min_exp")),
                PyObject::str_val(CompactString::from("dig")),
                PyObject::str_val(CompactString::from("mant_dig")),
                PyObject::str_val(CompactString::from("epsilon")),
                PyObject::str_val(CompactString::from("radix")),
                PyObject::str_val(CompactString::from("max_10_exp")),
                PyObject::str_val(CompactString::from("min_10_exp")),
            ]));
            ns.insert(CompactString::from("_field_defaults"), PyObject::dict(IndexMap::new()));
            let cls = PyObject::class(CompactString::from("sys.float_info"), vec![], ns);
            let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
            attrs.insert(CompactString::from("max"), PyObject::float(f64::MAX));
            attrs.insert(CompactString::from("max_exp"), PyObject::int(308));
            attrs.insert(CompactString::from("min"), PyObject::float(f64::MIN_POSITIVE));
            attrs.insert(CompactString::from("min_exp"), PyObject::int(-307));
            attrs.insert(CompactString::from("dig"), PyObject::int(15));
            attrs.insert(CompactString::from("mant_dig"), PyObject::int(53));
            attrs.insert(CompactString::from("epsilon"), PyObject::float(f64::EPSILON));
            attrs.insert(CompactString::from("radix"), PyObject::int(2));
            attrs.insert(CompactString::from("max_10_exp"), PyObject::int(1024));
            attrs.insert(CompactString::from("min_10_exp"), PyObject::int(-1021));
            // Store tuple data for index access
            attrs.insert(CompactString::from("__tuple_data__"), PyObject::tuple(vec![
                PyObject::float(f64::MAX),
                PyObject::int(308),
                PyObject::float(f64::MIN_POSITIVE),
                PyObject::int(-307),
                PyObject::int(15),
                PyObject::int(53),
                PyObject::float(f64::EPSILON),
                PyObject::int(2),
                PyObject::int(1024),
                PyObject::int(-1021),
            ]));
            PyObject::instance_with_attrs(cls, attrs)
        }),
        ("int_info", {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("__namedtuple__"), PyObject::bool_val(true));
            ns.insert(CompactString::from("_fields"), PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("bits_per_digit")),
                PyObject::str_val(CompactString::from("sizeof_digit")),
            ]));
            ns.insert(CompactString::from("_field_defaults"), PyObject::dict(IndexMap::new()));
            let cls = PyObject::class(CompactString::from("sys.int_info"), vec![], ns);
            let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
            attrs.insert(CompactString::from("bits_per_digit"), PyObject::int(30));
            attrs.insert(CompactString::from("sizeof_digit"), PyObject::int(4));
            attrs.insert(CompactString::from("__tuple_data__"), PyObject::tuple(vec![
                PyObject::int(30), PyObject::int(4),
            ]));
            PyObject::instance_with_attrs(cls, attrs)
        }),
        ("hash_info", {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("__namedtuple__"), PyObject::bool_val(true));
            ns.insert(CompactString::from("_fields"), PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("width")),
                PyObject::str_val(CompactString::from("modulus")),
                PyObject::str_val(CompactString::from("inf")),
                PyObject::str_val(CompactString::from("nan")),
                PyObject::str_val(CompactString::from("imag")),
            ]));
            ns.insert(CompactString::from("_field_defaults"), PyObject::dict(IndexMap::new()));
            let cls = PyObject::class(CompactString::from("sys.hash_info"), vec![], ns);
            let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
            attrs.insert(CompactString::from("width"), PyObject::int(64));
            attrs.insert(CompactString::from("modulus"), PyObject::int(0));
            attrs.insert(CompactString::from("inf"), PyObject::int(0));
            attrs.insert(CompactString::from("nan"), PyObject::int(0));
            attrs.insert(CompactString::from("imag"), PyObject::int(0));
            attrs.insert(CompactString::from("__tuple_data__"), PyObject::tuple(vec![
                PyObject::int(64), PyObject::int(0), PyObject::int(0), PyObject::int(0), PyObject::int(0),
            ]));
            PyObject::instance_with_attrs(cls, attrs)
        }),
        ("__debug__", PyObject::bool_val(true)),
        ("dont_write_bytecode", PyObject::bool_val(true)),
        ("meta_path", PyObject::list(vec![])),
        ("path_hooks", PyObject::list(vec![])),
        ("exc_info", make_builtin(|_| {
            // Read active exception lazily through VM pointer (zero TLS writes on raise/catch)
            if let Some((kind, msg, obj)) = ferrython_core::error::get_active_exc_object() {
                let type_obj = PyObject::exception_type(kind);
                let value_obj = if let Some(o) = obj {
                    o
                } else {
                    PyObject::str_val(CompactString::from(msg.as_str()))
                };
                let tb_obj = value_obj.get_attr("__traceback__")
                    .unwrap_or_else(|| PyObject::none());
                Ok(PyObject::tuple(vec![type_obj, value_obj, tb_obj]))
            } else {
                Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none(), PyObject::none()]))
            }
        })),
        ("_getframe", PyObject::native_function("sys._getframe", |args| {
            // sys._getframe([depth]) — return frame at given depth
            let depth = if !args.is_empty() {
                if let PyObjectPayload::Int(i) = &args[0].payload {
                    i.to_i64().unwrap_or(0) as usize
                } else { 0 }
            } else { 0 };
            // Try to get the current frame from the VM-provided thread-local
            if let Some(frame) = get_current_frame() {
                let mut current = frame;
                for _ in 0..depth {
                    if let Some(back) = current.get_attr("f_back") {
                        if matches!(&back.payload, PyObjectPayload::None) { break; }
                        current = back;
                    } else { break; }
                }
                return Ok(current);
            }
            // Fallback: return a minimal frame with current globals if available
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("f_locals"), PyObject::dict_from_pairs(vec![]));
            let globals = if let Some(sg) = get_current_globals() {
                // Build a dict from the live SharedGlobals
                let g_map = sg.read();
                let pairs: Vec<_> = g_map.iter()
                    .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
                    .collect();
                PyObject::dict_from_pairs(pairs)
            } else {
                PyObject::dict_from_pairs(vec![])
            };
            attrs.insert(CompactString::from("f_globals"), globals);
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
        ("hexversion", PyObject::int(0x030800f0)),
        ("warnoptions", PyObject::list(vec![])),
        ("path_importer_cache", PyObject::dict_from_pairs(vec![])),
        ("displayhook", make_builtin(|args| {
            check_args("sys.displayhook", args, 1)?;
            if !matches!(args[0].payload, PyObjectPayload::None) {
                println!("{}", args[0].py_to_string());
            }
            Ok(PyObject::none())
        })),
        ("breakpointhook", make_builtin(|_args| {
            // Default breakpointhook just prints a message (no real debugger)
            eprintln!("--Return--");
            Ok(PyObject::none())
        })),
    ]);
    CURRENT_SYS_MODULE.with(|c| *c.borrow_mut() = Some(module.clone()));
    module
}

fn sys_getrecursionlimit(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(RECURSION_LIMIT.load(Ordering::Relaxed)))
}
fn sys_setrecursionlimit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.setrecursionlimit", args, 1)?;
    let limit = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("an integer is required"))?;
    if limit <= 0 {
        return Err(PyException::value_error("recursion limit must be positive"));
    }
    RECURSION_LIMIT.store(limit, Ordering::Relaxed);
    Ok(PyObject::none())
}
fn sys_get_int_max_str_digits(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(ferrython_bytecode::get_int_max_str_digits()))
}
fn sys_set_int_max_str_digits(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.set_int_max_str_digits", args, 1)?;
    let limit = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("an integer is required"))?;
    if limit != 0 && limit < INT_MAX_STR_DIGITS_THRESHOLD {
        return Err(PyException::value_error(format!(
            "maxdigits must be 0 or larger than {}",
            INT_MAX_STR_DIGITS_THRESHOLD
        )));
    }
    ferrython_bytecode::set_int_max_str_digits(limit);
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
    let exc_tb = &args[2];
    let mut text = format_traceback_chain(exc_tb);
    let type_name = exception_display_type(exc_type, exc_value);
    let value_str = exception_display_value(exc_value);
    if value_str.is_empty() {
        text.push_str(&type_name);
        text.push('\n');
    } else {
        text.push_str(&format!("{}: {}\n", type_name, value_str));
    }
    write_stderr(&text);
    Ok(PyObject::none())
}

/// Default sys.unraisablehook: prints an unraisable exception summary.
fn sys_unraisablehook_default(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unraisablehook requires an argument",
        ));
    }
    let unraisable = &args[0];
    let exc_type = unraisable
        .get_attr("exc_type")
        .unwrap_or_else(|| PyObject::none());
    let exc_value = unraisable
        .get_attr("exc_value")
        .unwrap_or_else(|| PyObject::none());
    let object = unraisable
        .get_attr("object")
        .unwrap_or_else(|| PyObject::none());
    write_stderr(&format!(
        "Exception ignored in: {}\n{}: {}\n",
        object.py_to_string(),
        exception_display_type(&exc_type, &exc_value),
        exception_display_value(&exc_value)
    ));
    Ok(PyObject::none())
}

fn exception_display_type(exc_type: &PyObjectRef, exc_value: &PyObjectRef) -> String {
    if let PyObjectPayload::Instance(inst) = &exc_value.payload {
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            return cd.name.to_string();
        }
    }
    match &exc_type.payload {
        PyObjectPayload::ExceptionType(kind) => format!("{}", kind),
        PyObjectPayload::Class(cd) => cd.name.to_string(),
        _ => exc_type.py_to_string(),
    }
}

fn exception_display_value(exc_value: &PyObjectRef) -> String {
    if let PyObjectPayload::Instance(inst) = &exc_value.payload {
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            if cd.namespace.read().contains_key("__str__") {
                return "<exception str() failed>".to_string();
            }
        }
        if let Some(args) = inst.attrs.read().get("args") {
            if let PyObjectPayload::Tuple(items) = &args.payload {
                return match items.len() {
                    0 => String::new(),
                    1 => items[0].py_to_string(),
                    _ => args.py_to_string(),
                };
            }
        }
    }
    exc_value.py_to_string()
}

fn format_traceback_chain(tb: &PyObjectRef) -> String {
    let mut entries = Vec::new();
    let mut current = tb.clone();
    loop {
        let attrs = match &current.payload {
            PyObjectPayload::None => break,
            PyObjectPayload::Instance(inst) => inst.attrs.read().clone(),
            _ => break,
        };
        let filename = attrs
            .get("tb_filename")
            .map(|v| v.py_to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let lineno = attrs
            .get("tb_lineno")
            .and_then(|v| match &v.payload {
                PyObjectPayload::Int(n) => n.to_i64(),
                _ => None,
            })
            .unwrap_or(0);
        let function = attrs
            .get("tb_name")
            .map(|v| v.py_to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        entries.push((filename, lineno, function));
        match attrs.get("tb_next") {
            Some(next) if !matches!(next.payload, PyObjectPayload::None) => {
                current = next.clone();
            }
            _ => break,
        }
    }
    if entries.is_empty() {
        return String::new();
    }
    let mut text = String::from("Traceback (most recent call last):\n");
    for (filename, lineno, function) in entries {
        text.push_str(&format!(
            "  File \"{}\", line {}, in {}\n",
            filename, lineno, function
        ));
        if let Some(line) = source_line(&filename, lineno) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                text.push_str(&format!("    {}\n", trimmed));
            }
        }
    }
    text
}

fn source_line(filename: &str, lineno: i64) -> Option<String> {
    if lineno <= 0 {
        return None;
    }
    let content = std::fs::read_to_string(filename).ok()?;
    content
        .lines()
        .nth((lineno as usize).saturating_sub(1))
        .map(|line| line.to_string())
}

fn write_stderr(text: &str) {
    let Some(stderr) =
        CURRENT_SYS_MODULE.with(|c| c.borrow().as_ref().and_then(|sys| sys.get_attr("stderr")))
    else {
        eprint!("{}", text);
        return;
    };
    let Some(write) = stderr.get_attr("write") else {
        eprint!("{}", text);
        return;
    };
    if ferrython_core::object::call_callable(
        &write,
        &[PyObject::str_val(CompactString::from(text))],
    )
    .is_err()
    {
        eprint!("{}", text);
    }
}

/// Create a file-like object for stdin/stdout/stderr
fn make_stdio_object(name: &str, mode: &str, fileno: i64) -> PyObjectRef {
    use indexmap::IndexMap;
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("name"),
        PyObject::str_val(CompactString::from(name)),
    );
    attrs.insert(
        CompactString::from("mode"),
        PyObject::str_val(CompactString::from(mode)),
    );
    attrs.insert(
        CompactString::from("encoding"),
        PyObject::str_val(CompactString::from("utf-8")),
    );
    attrs.insert(
        CompactString::from("errors"),
        PyObject::str_val(CompactString::from("strict")),
    );
    attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    attrs.insert(
        CompactString::from("line_buffering"),
        PyObject::bool_val(fileno != 0),
    );
    attrs.insert(CompactString::from("_fileno"), PyObject::int(fileno));
    attrs.insert(CompactString::from("newlines"), PyObject::none());
    attrs.insert(
        CompactString::from("buffer"),
        make_stdio_buffer_object(name, mode, fileno),
    );
    attrs.insert(
        CompactString::from("write"),
        PyObject::native_function("write", stdio_write),
    );
    attrs.insert(
        CompactString::from("writelines"),
        PyObject::native_function("writelines", stdio_writelines),
    );
    attrs.insert(
        CompactString::from("read"),
        PyObject::native_function("read", stdio_read),
    );
    attrs.insert(
        CompactString::from("readline"),
        PyObject::native_function("readline", stdio_readline),
    );
    attrs.insert(
        CompactString::from("readlines"),
        PyObject::native_function("readlines", stdio_readlines),
    );
    attrs.insert(
        CompactString::from("flush"),
        PyObject::native_function("flush", stdio_flush),
    );
    attrs.insert(
        CompactString::from("fileno"),
        PyObject::native_function("fileno", stdio_fileno),
    );
    attrs.insert(
        CompactString::from("isatty"),
        PyObject::native_function("isatty", stdio_isatty),
    );
    attrs.insert(
        CompactString::from("readable"),
        PyObject::native_function("readable", stdio_readable),
    );
    attrs.insert(
        CompactString::from("writable"),
        PyObject::native_function("writable", stdio_writable),
    );
    attrs.insert(
        CompactString::from("seekable"),
        PyObject::native_function("seekable", stdio_seekable),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );

    PyObject::module_with_attrs(CompactString::from("_io.TextIOWrapper"), attrs)
}

fn make_stdio_buffer_object(name: &str, mode: &str, fileno: i64) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("name"),
        PyObject::str_val(CompactString::from(name)),
    );
    attrs.insert(
        CompactString::from("mode"),
        PyObject::str_val(CompactString::from(format!("{}b", mode))),
    );
    attrs.insert(CompactString::from("_fileno"), PyObject::int(fileno));
    attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    attrs.insert(
        CompactString::from("write"),
        PyObject::native_function("write", stdio_buffer_write),
    );
    attrs.insert(
        CompactString::from("flush"),
        PyObject::native_function("flush", stdio_flush),
    );
    attrs.insert(
        CompactString::from("fileno"),
        PyObject::native_function("fileno", stdio_fileno),
    );
    attrs.insert(
        CompactString::from("isatty"),
        PyObject::native_function("isatty", stdio_isatty),
    );
    attrs.insert(
        CompactString::from("readable"),
        PyObject::native_function("readable", stdio_readable),
    );
    attrs.insert(
        CompactString::from("writable"),
        PyObject::native_function("writable", stdio_writable),
    );
    attrs.insert(
        CompactString::from("seekable"),
        PyObject::native_function("seekable", stdio_seekable),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );
    PyObject::module_with_attrs(CompactString::from("_io.BufferedWriter"), attrs)
}

fn get_stdio_fd(args: &[PyObjectRef]) -> i64 {
    args.first()
        .and_then(|s| s.get_attr("_fileno"))
        .and_then(|v| v.to_int().ok())
        .unwrap_or(-1)
}

fn stdio_buffer_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    let arg = if args.len() > 1 {
        &args[1]
    } else {
        return Err(PyException::type_error("write() requires 1 argument"));
    };
    let bytes = match &arg.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
        _ => {
            return Err(PyException::type_error(format!(
                "a bytes-like object is required, not '{}'",
                arg.type_name()
            )))
        }
    };
    use std::io::Write;
    if fd == 2 {
        let _ = std::io::stderr().write_all(&bytes);
    } else {
        let _ = std::io::stdout().write_all(&bytes);
    }
    Ok(PyObject::int(bytes.len() as i64))
}

fn stdio_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    let arg = if args.len() > 1 {
        &args[1]
    } else {
        return Err(PyException::type_error("write() requires 1 argument"));
    };
    // TextIOWrapper rejects bytes (like CPython)
    if matches!(&arg.payload, PyObjectPayload::Bytes(_)) {
        return Err(PyException::type_error(
            "write() argument must be str, not bytes",
        ));
    }
    let text = arg.py_to_string();
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
    let lines_obj = if args.len() > 1 {
        &args[1]
    } else {
        return Err(PyException::type_error("writelines() missing argument"));
    };
    let items = lines_obj.to_list()?;
    for item in items {
        let text = item.py_to_string();
        if fd == 2 {
            eprint!("{}", text);
        } else {
            print!("{}", text);
        }
    }
    Ok(PyObject::none())
}

fn stdio_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    if fd != 0 {
        return Err(PyException::runtime_error("not readable"));
    }
    use std::io::Read;
    let max = if args.len() > 1 {
        args[1].to_int().unwrap_or(-1)
    } else {
        -1
    };
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
    let lines: Vec<PyObjectRef> = stdin
        .lock()
        .lines()
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
            ("environ", {
                // Build _Environ instance: a dict-like object that syncs with OS env.
                // os.environ["X"] = "Y" calls putenv; del os.environ["X"] calls unsetenv.
                let initial_pairs: Vec<(PyObjectRef, PyObjectRef)> = std::env::vars()
                    .map(|(k, v)| {
                        (
                            PyObject::str_val(CompactString::from(k)),
                            PyObject::str_val(CompactString::from(v)),
                        )
                    })
                    .collect();
                let data = PyObject::dict_from_pairs(initial_pairs);
                let data_ref = data.clone();
                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("_data"), data.clone());

                // Dunders are called via try_call_dunder which prepends self as args[0].
                // StoreSubscr/DeleteSubscr Module handler calls directly without self.
                // Use helper: last 1 arg = key, last 2 args = key+val.
                attrs.insert(
                    CompactString::from("__getitem__"),
                    PyObject::native_closure("__getitem__", move |args| {
                        // args may be [self, key] or [key]
                        let key_str = args
                            .last()
                            .ok_or_else(|| PyException::key_error("key required"))?
                            .py_to_string();
                        match std::env::var(&key_str) {
                            Ok(val) => Ok(PyObject::str_val(CompactString::from(val))),
                            Err(_) => Err(PyException::key_error(format!("'{}'", key_str))),
                        }
                    }),
                );
                let d2 = data_ref.clone();
                attrs.insert(
                    CompactString::from("__setitem__"),
                    PyObject::native_closure("__setitem__", move |args| {
                        // args may be [self, key, val] or [key, val]
                        if args.len() < 2 {
                            return Err(PyException::type_error(
                                "__setitem__ requires key and value",
                            ));
                        }
                        let val_str = args[args.len() - 1].py_to_string();
                        let key_str = args[args.len() - 2].py_to_string();
                        unsafe {
                            std::env::set_var(&key_str, &val_str);
                        }
                        if let PyObjectPayload::Dict(dd) = &d2.payload {
                            dd.write().insert(
                                HashableKey::str_key(CompactString::from(&key_str)),
                                PyObject::str_val(CompactString::from(&val_str)),
                            );
                        }
                        Ok(PyObject::none())
                    }),
                );
                let d3 = data_ref.clone();
                attrs.insert(
                    CompactString::from("__delitem__"),
                    PyObject::native_closure("__delitem__", move |args| {
                        let key_str = args
                            .last()
                            .ok_or_else(|| PyException::key_error("key required"))?
                            .py_to_string();
                        unsafe {
                            std::env::remove_var(&key_str);
                        }
                        if let PyObjectPayload::Dict(dd) = &d3.payload {
                            dd.write()
                                .swap_remove(&HashableKey::str_key(CompactString::from(&key_str)));
                        }
                        Ok(PyObject::none())
                    }),
                );
                attrs.insert(
                    CompactString::from("__contains__"),
                    PyObject::native_closure("__contains__", move |args| {
                        let key_str = args.last().map(|a| a.py_to_string()).unwrap_or_default();
                        Ok(PyObject::bool_val(std::env::var(&key_str).is_ok()))
                    }),
                );
                attrs.insert(
                    CompactString::from("get"),
                    PyObject::native_closure("get", move |args| {
                        // args: [self, key] or [self, key, default]
                        // Skip self (first arg if module)
                        let real_args = if args.len() > 1
                            && matches!(&args[0].payload, PyObjectPayload::Module(_))
                        {
                            &args[1..]
                        } else {
                            args
                        };
                        if real_args.is_empty() {
                            return Ok(PyObject::none());
                        }
                        let key_str = real_args[0].py_to_string();
                        match std::env::var(&key_str) {
                            Ok(val) => Ok(PyObject::str_val(CompactString::from(val))),
                            Err(_) => Ok(real_args.get(1).cloned().unwrap_or_else(PyObject::none)),
                        }
                    }),
                );
                attrs.insert(
                    CompactString::from("keys"),
                    PyObject::native_closure("keys", move |_| {
                        let keys: Vec<PyObjectRef> = std::env::vars()
                            .map(|(k, _)| PyObject::str_val(CompactString::from(k)))
                            .collect();
                        Ok(PyObject::list(keys))
                    }),
                );
                attrs.insert(
                    CompactString::from("values"),
                    PyObject::native_closure("values", move |_| {
                        let vals: Vec<PyObjectRef> = std::env::vars()
                            .map(|(_, v)| PyObject::str_val(CompactString::from(v)))
                            .collect();
                        Ok(PyObject::list(vals))
                    }),
                );
                attrs.insert(
                    CompactString::from("items"),
                    PyObject::native_closure("items", move |_| {
                        let items: Vec<PyObjectRef> = std::env::vars()
                            .map(|(k, v)| {
                                PyObject::tuple(vec![
                                    PyObject::str_val(CompactString::from(k)),
                                    PyObject::str_val(CompactString::from(v)),
                                ])
                            })
                            .collect();
                        Ok(PyObject::list(items))
                    }),
                );
                attrs.insert(
                    CompactString::from("copy"),
                    PyObject::native_closure("copy", move |_| {
                        let pairs: Vec<(PyObjectRef, PyObjectRef)> = std::env::vars()
                            .map(|(k, v)| {
                                (
                                    PyObject::str_val(CompactString::from(k)),
                                    PyObject::str_val(CompactString::from(v)),
                                )
                            })
                            .collect();
                        Ok(PyObject::dict_from_pairs(pairs))
                    }),
                );
                attrs.insert(
                    CompactString::from("__repr__"),
                    PyObject::native_closure("__repr__", move |_| {
                        Ok(PyObject::str_val(CompactString::from("environ({...})")))
                    }),
                );
                PyObject::module_with_attrs(CompactString::from("_Environ"), attrs)
            }),
            (
                "_Environ",
                PyObject::class(CompactString::from("_Environ"), vec![], IndexMap::new()),
            ),
            ("cpu_count", make_builtin(os_cpu_count)),
            ("getpid", make_builtin(os_getpid)),
            ("fspath", PyObject::native_function("os.fspath", os_fspath)),
            (
                "PathLike",
                PyObject::class(CompactString::from("PathLike"), vec![], {
                    let mut ns = IndexMap::new();
                    ns.insert(
                        CompactString::from("__fspath__"),
                        make_builtin(|_args: &[PyObjectRef]| {
                            Err(PyException::not_implemented_error(
                                "PathLike.__fspath__() is abstract",
                            ))
                        }),
                    );
                    // ABC register method — allows PathLike.register(SomeClass)
                    ns.insert(
                        CompactString::from("register"),
                        make_builtin(|args: &[PyObjectRef]| {
                            // register(cls, subclass) — just returns the subclass (no-op registration)
                            if args.len() >= 2 {
                                Ok(args[1].clone())
                            } else if args.len() == 1 {
                                Ok(args[0].clone())
                            } else {
                                Ok(PyObject::none())
                            }
                        }),
                    );
                    ns
                }),
            ),
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

/// Create the os.terminal_size class (namedtuple-like).
pub fn make_terminal_size_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("terminal_size.__init__", |args| {
            // terminal_size((columns, lines))
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "terminal_size requires a (columns, lines) argument",
                ));
            }
            let seq = &args[1];
            let (cols, lines) = match &seq.payload {
                PyObjectPayload::Tuple(items) if items.len() >= 2 => {
                    let c = items[0].as_int().unwrap_or(80);
                    let l = items[1].as_int().unwrap_or(24);
                    (c, l)
                }
                _ => {
                    return Err(PyException::type_error(
                        "terminal_size requires a 2-item sequence",
                    ))
                }
            };
            if let PyObjectPayload::Instance(ref data) = args[0].payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("columns"), PyObject::int(cols));
                attrs.insert(CompactString::from("lines"), PyObject::int(lines));
            }
            Ok(PyObject::none())
        }),
    );
    PyObject::class(CompactString::from("terminal_size"), vec![], ns)
}

/// Create a terminal_size instance with columns and lines.
pub fn make_terminal_size_instance(cols: i64, lines: i64) -> PyObjectRef {
    let cls = make_terminal_size_class();
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("columns"), PyObject::int(cols));
    attrs.insert(CompactString::from("lines"), PyObject::int(lines));
    // Support tuple-like indexing, iteration, length, and repr
    let c = cols;
    let l = lines;
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("terminal_size.__getitem__", move |args| {
            let idx = args.last().and_then(|a| a.as_int()).unwrap_or(0);
            match idx {
                0 => Ok(PyObject::int(c)),
                1 => Ok(PyObject::int(l)),
                _ => Err(PyException::index_error("tuple index out of range")),
            }
        }),
    );
    attrs.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("terminal_size.__len__", |_| Ok(PyObject::int(2))),
    );
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("terminal_size.__iter__", move |_| {
            Ok(PyObject::tuple(vec![PyObject::int(c), PyObject::int(l)]))
        }),
    );
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("terminal_size.__repr__", move |_| {
            Ok(PyObject::str_val(CompactString::from(format!(
                "os.terminal_size(columns={}, lines={})",
                c, l
            ))))
        }),
    );
    PyObject::instance_with_attrs(cls, attrs)
}

fn os_getcwd(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cwd = std::env::current_dir().map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::str_val(CompactString::from(
        cwd.to_string_lossy().to_string(),
    )))
}
fn os_listdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = if args.is_empty() {
        ".".to_string()
    } else {
        args[0].py_to_string()
    };
    let entries = std::fs::read_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    let items: Vec<PyObjectRef> = entries
        .filter_map(|e| e.ok())
        .map(|e| {
            PyObject::str_val(CompactString::from(
                e.file_name().to_string_lossy().to_string(),
            ))
        })
        .collect();
    Ok(PyObject::list(items))
}
fn os_mkdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.mkdir() requires at least 1 argument",
        ));
    }
    let path = args[0].py_to_string();
    let exist_ok = args.iter().skip(1).any(|a| {
        if let PyObjectPayload::Dict(kw) = &a.payload {
            kw.read()
                .get(&HashableKey::str_key(CompactString::from("exist_ok")))
                .map(|v| matches!(&v.payload, PyObjectPayload::Bool(true)))
                .unwrap_or(false)
        } else {
            false
        }
    });
    match std::fs::create_dir(&path) {
        Ok(_) => Ok(PyObject::none()),
        Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => Ok(PyObject::none()),
        Err(e) => Err(PyException::from_io_error(&e, Some(&path))),
    }
}
fn os_makedirs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.makedirs() requires at least 1 argument",
        ));
    }
    let path = args[0].py_to_string();
    // Check for exist_ok kwarg (may be in trailing dict)
    let exist_ok = args.iter().skip(1).any(|a| {
        if let PyObjectPayload::Dict(kw) = &a.payload {
            kw.read()
                .get(&HashableKey::str_key(CompactString::from("exist_ok")))
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
    std::fs::remove_dir(&path).map_err(|e| PyException::os_error(format!("{}", e)))?;
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
fn os_replace(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.replace", args, 2)?;
    // On Unix, rename is atomic and replaces the destination
    std::fs::rename(args[0].py_to_string(), args[1].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_getenv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.getenv requires at least 1 argument",
        ));
    }
    let key = args[0].py_to_string();
    let default = if args.len() > 1 {
        args[1].clone()
    } else {
        PyObject::none()
    };
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
                    PyObjectPayload::NativeFunction(nf) => (nf.func)(&[args[0].clone()]),
                    PyObjectPayload::NativeClosure(nc) => (nc.func)(&[args[0].clone()]),
                    PyObjectPayload::Function(_) => Ok(PyObject::str_val(CompactString::from(
                        args[0].py_to_string(),
                    ))),
                    _ => Err(PyException::type_error(format!(
                        "expected str, bytes or os.PathLike object, not '{}'",
                        args[0].type_name()
                    ))),
                }
            } else {
                Err(PyException::type_error(format!(
                    "expected str, bytes or os.PathLike object, not '{}'",
                    args[0].type_name()
                )))
            }
        }
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn os_walk(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.walk() requires at least 1 argument",
        ));
    }
    let path = args[0].py_to_string();
    let topdown = if args.len() > 1 {
        args[1].is_truthy()
    } else {
        true
    };
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
        PyObject::list(
            dirnames
                .iter()
                .map(|n| PyObject::str_val(CompactString::from(n.as_str())))
                .collect(),
        ),
        PyObject::list(
            filenames
                .iter()
                .map(|n| PyObject::str_val(CompactString::from(n.as_str())))
                .collect(),
        ),
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
    let mut attrs = IndexMap::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        attrs.insert(
            CompactString::from("st_mode"),
            PyObject::int(meta.mode() as i64),
        );
        attrs.insert(
            CompactString::from("st_ino"),
            PyObject::int(meta.ino() as i64),
        );
        attrs.insert(
            CompactString::from("st_dev"),
            PyObject::int(meta.dev() as i64),
        );
        attrs.insert(
            CompactString::from("st_nlink"),
            PyObject::int(meta.nlink() as i64),
        );
        attrs.insert(
            CompactString::from("st_uid"),
            PyObject::int(meta.uid() as i64),
        );
        attrs.insert(
            CompactString::from("st_gid"),
            PyObject::int(meta.gid() as i64),
        );
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
    attrs.insert(
        CompactString::from("st_size"),
        PyObject::int(meta.len() as i64),
    );
    let epoch = std::time::SystemTime::UNIX_EPOCH;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let atime = meta
        .accessed()
        .ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let ctime = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(epoch).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    attrs.insert(CompactString::from("st_mtime"), PyObject::float(mtime));
    attrs.insert(CompactString::from("st_atime"), PyObject::float(atime));
    attrs.insert(CompactString::from("st_ctime"), PyObject::float(ctime));
    Ok(PyObject::module_with_attrs(
        CompactString::from("os.stat_result"),
        attrs,
    ))
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
        let mode = args[1]
            .as_int()
            .ok_or_else(|| PyException::type_error("an integer is required"))?;
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(mode as u32);
        std::fs::set_permissions(&path, perms)
            .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    }
    Ok(PyObject::none())
}

fn os_chown(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "chown requires at least 3 arguments",
        ));
    }
    #[cfg(unix)]
    {
        let path = args[0].py_to_string();
        let uid = args[1].as_int().unwrap_or(-1);
        let gid = args[2].as_int().unwrap_or(-1);
        let cpath = std::ffi::CString::new(path.as_str())
            .map_err(|_| PyException::value_error("embedded null in path"))?;
        let ret = unsafe { libc::chown(cpath.as_ptr(), uid as libc::uid_t, gid as libc::gid_t) };
        if ret != 0 {
            return Err(PyException::os_error(format!(
                "{}: '{}'",
                std::io::Error::last_os_error(),
                path
            )));
        }
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
        return Err(PyException::os_error(
            "os.symlink() not available on this platform",
        ));
    }
    Ok(PyObject::none())
}

fn os_readlink(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.readlink", args, 1)?;
    let path = args[0].py_to_string();
    let target = std::fs::read_link(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    Ok(PyObject::str_val(CompactString::from(
        target.to_string_lossy().to_string(),
    )))
}

fn os_isatty(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.isatty", args, 1)?;
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("an integer is required"))?;
    Ok(PyObject::bool_val(is_fd_terminal(fd)))
}

#[cfg(unix)]
fn is_fd_terminal(fd: i64) -> bool {
    unsafe {
        extern "C" {
            fn isatty(fd: i32) -> i32;
        }
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
    let data_arc = Rc::new(PyCell::new(data));

    let cls = PyObject::class(CompactString::from("_POpenFile"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        let d = data_arc.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("popen.read", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(d.read().as_str())))
            }),
        );
        attrs.insert(
            CompactString::from("close"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        let d2 = data_arc;
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("popen.readline", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(d2.read().as_str())))
            }),
        );
    }
    Ok(inst)
}

fn os_scandir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = if args.is_empty() {
        ".".to_string()
    } else {
        args[0].py_to_string()
    };
    let entries = std::fs::read_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let full_path = entry.path().to_string_lossy().to_string();
        let file_type = entry.file_type().ok();
        let is_file = file_type.as_ref().map(|ft| ft.is_file()).unwrap_or(false);
        let is_dir = file_type.as_ref().map(|ft| ft.is_dir()).unwrap_or(false);
        let is_symlink = file_type
            .as_ref()
            .map(|ft| ft.is_symlink())
            .unwrap_or(false);

        let cls = PyObject::class(CompactString::from("DirEntry"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(&name)),
        );
        attrs.insert(
            CompactString::from("path"),
            PyObject::str_val(CompactString::from(&full_path)),
        );

        let is_file_val = is_file;
        attrs.insert(
            CompactString::from("is_file"),
            PyObject::native_closure("DirEntry.is_file", move |_| {
                Ok(PyObject::bool_val(is_file_val))
            }),
        );
        let is_dir_val = is_dir;
        attrs.insert(
            CompactString::from("is_dir"),
            PyObject::native_closure("DirEntry.is_dir", move |_| {
                Ok(PyObject::bool_val(is_dir_val))
            }),
        );
        let is_sym_val = is_symlink;
        attrs.insert(
            CompactString::from("is_symlink"),
            PyObject::native_closure("DirEntry.is_symlink", move |_| {
                Ok(PyObject::bool_val(is_sym_val))
            }),
        );
        let stat_path = full_path.clone();
        attrs.insert(
            CompactString::from("stat"),
            PyObject::native_closure("DirEntry.stat", move |_| {
                let meta = std::fs::metadata(&stat_path)
                    .map_err(|e| PyException::os_error(format!("{}: '{}'", e, stat_path)))?;
                crate::fs_modules::build_stat_result(meta)
            }),
        );
        let repr_name = name.clone();
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("DirEntry.__repr__", move |_| {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "<DirEntry '{}'>",
                    repr_name
                ))))
            }),
        );
        let str_name = name.clone();
        attrs.insert(
            CompactString::from("__str__"),
            PyObject::native_closure("DirEntry.__str__", move |_| {
                Ok(PyObject::str_val(CompactString::from(str_name.as_str())))
            }),
        );
        items.push(PyObject::instance_with_attrs(cls, attrs));
    }
    // Wrap in a ScandirIterator with context manager support
    let items_list = PyObject::list(items);
    let cls = PyObject::class(
        CompactString::from("ScandirIterator"),
        vec![],
        IndexMap::new(),
    );
    let mut attrs = IndexMap::new();
    let items_ref = items_list.clone();
    attrs.insert(CompactString::from("_entries"), items_list);
    attrs.insert(
        CompactString::from("__enter__"),
        PyObject::native_closure("ScandirIterator.__enter__", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("expected self"));
            }
            Ok(args[0].clone())
        }),
    );
    attrs.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("ScandirIterator.__exit__", move |_| Ok(PyObject::none())),
    );
    let iter_items = items_ref;
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("ScandirIterator.__iter__", move |_| {
            ferrython_core::object::PyObjectMethods::get_iter(&iter_items)
        }),
    );
    Ok(PyObject::instance_with_attrs(cls, attrs))
}
