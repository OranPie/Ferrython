use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};
use indexmap::IndexMap;

// ── resource module (unix) ──

pub fn create_resource_module() -> PyObjectRef {
    let getrlimit_fn = make_builtin(|args: &[PyObjectRef]| {
        let resource = if !args.is_empty() {
            args[0].to_int().unwrap_or(0) as i32
        } else {
            0
        };
        #[cfg(unix)]
        {
            let mut rlim: libc::rlimit = unsafe { std::mem::zeroed() };
            #[cfg(target_os = "linux")]
            let resource_arg = resource as libc::__rlimit_resource_t;
            #[cfg(not(target_os = "linux"))]
            let resource_arg = resource as libc::c_int;
            let ret = unsafe { libc::getrlimit(resource_arg, &mut rlim) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("getrlimit: {}", err)));
            }
            let soft = if rlim.rlim_cur == libc::RLIM_INFINITY {
                -1i64
            } else {
                rlim.rlim_cur as i64
            };
            let hard = if rlim.rlim_max == libc::RLIM_INFINITY {
                -1i64
            } else {
                rlim.rlim_max as i64
            };
            Ok(PyObject::tuple(vec![
                PyObject::int(soft),
                PyObject::int(hard),
            ]))
        }
        #[cfg(not(unix))]
        {
            let _ = resource;
            Err(PyException::os_error(
                "getrlimit() is not supported on this platform",
            ))
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
                rlim_cur: if soft < 0 {
                    libc::RLIM_INFINITY
                } else {
                    soft as libc::rlim_t
                },
                rlim_max: if hard < 0 {
                    libc::RLIM_INFINITY
                } else {
                    hard as libc::rlim_t
                },
            };
            #[cfg(target_os = "linux")]
            let resource_arg = resource as libc::__rlimit_resource_t;
            #[cfg(not(target_os = "linux"))]
            let resource_arg = resource as libc::c_int;
            let ret = unsafe { libc::setrlimit(resource_arg, &rlim) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("setrlimit: {}", err)));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (resource, limits);
            return Err(PyException::os_error(
                "setrlimit() is not supported on this platform",
            ));
        }
        Ok(PyObject::none())
    });

    let getrusage_fn = make_builtin(|args: &[PyObjectRef]| {
        let who = if !args.is_empty() {
            args[0].to_int().unwrap_or(0) as i32
        } else {
            0
        };
        #[cfg(unix)]
        {
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            let ret = unsafe { libc::getrusage(who, &mut usage) };
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("getrusage: {}", err)));
            }
            let cls = PyObject::class(
                CompactString::from("struct_rusage"),
                vec![],
                IndexMap::new(),
            );
            let mut attrs = IndexMap::new();
            attrs.insert(
                CompactString::from("ru_utime"),
                PyObject::float(
                    usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1_000_000.0,
                ),
            );
            attrs.insert(
                CompactString::from("ru_stime"),
                PyObject::float(
                    usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1_000_000.0,
                ),
            );
            attrs.insert(
                CompactString::from("ru_maxrss"),
                PyObject::int(usage.ru_maxrss),
            );
            attrs.insert(
                CompactString::from("ru_ixrss"),
                PyObject::int(usage.ru_ixrss),
            );
            attrs.insert(
                CompactString::from("ru_idrss"),
                PyObject::int(usage.ru_idrss),
            );
            attrs.insert(
                CompactString::from("ru_isrss"),
                PyObject::int(usage.ru_isrss),
            );
            attrs.insert(
                CompactString::from("ru_minflt"),
                PyObject::int(usage.ru_minflt),
            );
            attrs.insert(
                CompactString::from("ru_majflt"),
                PyObject::int(usage.ru_majflt),
            );
            attrs.insert(
                CompactString::from("ru_nswap"),
                PyObject::int(usage.ru_nswap),
            );
            attrs.insert(
                CompactString::from("ru_inblock"),
                PyObject::int(usage.ru_inblock),
            );
            attrs.insert(
                CompactString::from("ru_oublock"),
                PyObject::int(usage.ru_oublock),
            );
            attrs.insert(
                CompactString::from("ru_msgsnd"),
                PyObject::int(usage.ru_msgsnd),
            );
            attrs.insert(
                CompactString::from("ru_msgrcv"),
                PyObject::int(usage.ru_msgrcv),
            );
            attrs.insert(
                CompactString::from("ru_nsignals"),
                PyObject::int(usage.ru_nsignals),
            );
            attrs.insert(
                CompactString::from("ru_nvcsw"),
                PyObject::int(usage.ru_nvcsw),
            );
            attrs.insert(
                CompactString::from("ru_nivcsw"),
                PyObject::int(usage.ru_nivcsw),
            );
            Ok(PyObject::instance_with_attrs(cls, attrs))
        }
        #[cfg(not(unix))]
        {
            let _ = who;
            let cls = PyObject::class(
                CompactString::from("struct_rusage"),
                vec![],
                IndexMap::new(),
            );
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

    make_module(
        "resource",
        vec![
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
        ],
    )
}
