use compact_str::CompactString;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};
use indexmap::IndexMap;

pub fn create_platform_module() -> PyObjectRef {
    make_module(
        "platform",
        vec![
            (
                "system",
                make_builtin(|_| {
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
                }),
            ),
            (
                "machine",
                make_builtin(|_| {
                    Ok(PyObject::str_val(CompactString::from(
                        std::env::consts::ARCH,
                    )))
                }),
            ),
            (
                "python_version",
                make_builtin(|_| Ok(PyObject::str_val(CompactString::from("3.8.0")))),
            ),
            (
                "python_implementation",
                make_builtin(|_| Ok(PyObject::str_val(CompactString::from("Ferrython")))),
            ),
            (
                "node",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        let mut buf = [0u8; 256];
                        let cstr = unsafe {
                            libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len());
                            std::ffi::CStr::from_ptr(buf.as_ptr() as *const libc::c_char)
                        };
                        Ok(PyObject::str_val(CompactString::from(
                            cstr.to_str().unwrap_or("localhost"),
                        )))
                    }
                    #[cfg(not(unix))]
                    Ok(PyObject::str_val(CompactString::from("localhost")))
                }),
            ),
            (
                "release",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        let mut utsname = unsafe { std::mem::zeroed::<libc::utsname>() };
                        unsafe {
                            libc::uname(&mut utsname);
                        }
                        let release = unsafe { std::ffi::CStr::from_ptr(utsname.release.as_ptr()) };
                        Ok(PyObject::str_val(CompactString::from(
                            release.to_str().unwrap_or(""),
                        )))
                    }
                    #[cfg(not(unix))]
                    Ok(PyObject::str_val(CompactString::from("")))
                }),
            ),
            (
                "version",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        let mut utsname = unsafe { std::mem::zeroed::<libc::utsname>() };
                        unsafe {
                            libc::uname(&mut utsname);
                        }
                        let version = unsafe { std::ffi::CStr::from_ptr(utsname.version.as_ptr()) };
                        Ok(PyObject::str_val(CompactString::from(
                            version.to_str().unwrap_or(""),
                        )))
                    }
                    #[cfg(not(unix))]
                    Ok(PyObject::str_val(CompactString::from("")))
                }),
            ),
            (
                "processor",
                make_builtin(|_| {
                    Ok(PyObject::str_val(CompactString::from(
                        std::env::consts::ARCH,
                    )))
                }),
            ),
            (
                "architecture",
                make_builtin(|_| {
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(
                            if cfg!(target_pointer_width = "64") {
                                "64bit"
                            } else {
                                "32bit"
                            },
                        )),
                        PyObject::str_val(CompactString::from("ELF")),
                    ]))
                }),
            ),
            (
                "python_version_tuple",
                make_builtin(|_| {
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from("3")),
                        PyObject::str_val(CompactString::from("8")),
                        PyObject::str_val(CompactString::from("0")),
                    ]))
                }),
            ),
            (
                "python_compiler",
                make_builtin(|_| Ok(PyObject::str_val(CompactString::from("Ferrython (Rust)")))),
            ),
            (
                "python_build",
                make_builtin(|_| {
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from("default")),
                        PyObject::str_val(CompactString::from("")),
                    ]))
                }),
            ),
            (
                "platform",
                make_builtin(|_| {
                    let system = match std::env::consts::OS {
                        "linux" => "Linux",
                        "macos" => "Darwin",
                        "windows" => "Windows",
                        o => o,
                    };
                    let machine = std::env::consts::ARCH;
                    #[cfg(unix)]
                    {
                        let mut utsname = unsafe { std::mem::zeroed::<libc::utsname>() };
                        unsafe {
                            libc::uname(&mut utsname);
                        }
                        let release = unsafe { std::ffi::CStr::from_ptr(utsname.release.as_ptr()) };
                        Ok(PyObject::str_val(CompactString::from(format!(
                            "{}-{}-{}",
                            system,
                            release.to_str().unwrap_or(""),
                            machine
                        ))))
                    }
                    #[cfg(not(unix))]
                    Ok(PyObject::str_val(CompactString::from(format!(
                        "{}-{}",
                        system, machine
                    ))))
                }),
            ),
            (
                "python_branch",
                make_builtin(|_| Ok(PyObject::str_val(CompactString::from("")))),
            ),
            (
                "python_revision",
                make_builtin(|_| Ok(PyObject::str_val(CompactString::from("")))),
            ),
            (
                "mac_ver",
                make_builtin(|_| {
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from("")),
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from("")),
                            PyObject::str_val(CompactString::from("")),
                            PyObject::str_val(CompactString::from("")),
                        ]),
                        PyObject::str_val(CompactString::from("")),
                    ]))
                }),
            ),
            (
                "linux_distribution",
                make_builtin(|_| {
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from("")),
                        PyObject::str_val(CompactString::from("")),
                        PyObject::str_val(CompactString::from("")),
                    ]))
                }),
            ),
            (
                "uname",
                make_builtin(|_| {
                    let system = match std::env::consts::OS {
                        "linux" => "Linux",
                        "macos" => "Darwin",
                        "windows" => "Windows",
                        o => o,
                    };
                    let machine = std::env::consts::ARCH;
                    let mut attrs = IndexMap::new();
                    attrs.insert(
                        CompactString::from("system"),
                        PyObject::str_val(CompactString::from(system)),
                    );
                    #[cfg(unix)]
                    {
                        let mut buf = [0u8; 256];
                        unsafe {
                            libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len());
                        }
                        let hostname = unsafe {
                            std::ffi::CStr::from_ptr(buf.as_ptr() as *const libc::c_char)
                        };
                        attrs.insert(
                            CompactString::from("node"),
                            PyObject::str_val(CompactString::from(
                                hostname.to_str().unwrap_or("localhost"),
                            )),
                        );
                        let mut utsname = unsafe { std::mem::zeroed::<libc::utsname>() };
                        unsafe {
                            libc::uname(&mut utsname);
                        }
                        let release = unsafe { std::ffi::CStr::from_ptr(utsname.release.as_ptr()) };
                        let version = unsafe { std::ffi::CStr::from_ptr(utsname.version.as_ptr()) };
                        attrs.insert(
                            CompactString::from("release"),
                            PyObject::str_val(CompactString::from(release.to_str().unwrap_or(""))),
                        );
                        attrs.insert(
                            CompactString::from("version"),
                            PyObject::str_val(CompactString::from(version.to_str().unwrap_or(""))),
                        );
                    }
                    #[cfg(not(unix))]
                    {
                        attrs.insert(
                            CompactString::from("node"),
                            PyObject::str_val(CompactString::from("localhost")),
                        );
                        attrs.insert(
                            CompactString::from("release"),
                            PyObject::str_val(CompactString::from("")),
                        );
                        attrs.insert(
                            CompactString::from("version"),
                            PyObject::str_val(CompactString::from("")),
                        );
                    }
                    attrs.insert(
                        CompactString::from("machine"),
                        PyObject::str_val(CompactString::from(machine)),
                    );
                    attrs.insert(
                        CompactString::from("processor"),
                        PyObject::str_val(CompactString::from(machine)),
                    );
                    let cls = PyObject::class(
                        CompactString::from("uname_result"),
                        vec![],
                        IndexMap::new(),
                    );
                    Ok(PyObject::instance_with_attrs(cls, attrs))
                }),
            ),
        ],
    )
}

// ── locale module ──
