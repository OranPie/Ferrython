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
mod exception_hooks;
mod fcntl;
mod getpass;
mod grp;
mod locale;
mod mmap;
mod os;
mod os_path;
mod platform;
mod pwd;
mod resource;
mod sched;
mod site;
mod stdio;
mod sysconfig;

pub use atexit::{create_atexit_module, register_atexit_callback, unregister_atexit_callback};
pub use errno::create_errno_module;
use exception_hooks::{sys_excepthook_default, sys_unraisablehook_default};
pub use fcntl::create_fcntl_module;
pub use getpass::create_getpass_module;
pub use grp::create_grp_module;
pub use locale::{create_locale_module, get_current_ctype_locale};
pub use mmap::create_mmap_module;
pub use os::{create_os_module, make_terminal_size_instance};
pub use os_path::create_os_path_module;
pub use platform::create_platform_module;
pub use pwd::create_pwd_module;
pub use resource::create_resource_module;
pub use sched::create_sched_module;
pub use site::create_site_module;
use stdio::make_stdio_object;
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
