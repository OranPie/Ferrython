use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef,
};
use indexmap::IndexMap;

// ── pwd module (Unix password database) ──

pub fn create_pwd_module() -> PyObjectRef {
    #[cfg(unix)]
    fn make_pwd_struct(
        name: &str,
        passwd: &str,
        uid: u32,
        gid: u32,
        gecos: &str,
        dir: &str,
        shell: &str,
    ) -> PyObjectRef {
        let cls = PyObject::class(
            CompactString::from("struct_passwd"),
            vec![],
            IndexMap::new(),
        );
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("pw_name"),
            PyObject::str_val(CompactString::from(name)),
        );
        attrs.insert(
            CompactString::from("pw_passwd"),
            PyObject::str_val(CompactString::from(passwd)),
        );
        attrs.insert(CompactString::from("pw_uid"), PyObject::int(uid as i64));
        attrs.insert(CompactString::from("pw_gid"), PyObject::int(gid as i64));
        attrs.insert(
            CompactString::from("pw_gecos"),
            PyObject::str_val(CompactString::from(gecos)),
        );
        attrs.insert(
            CompactString::from("pw_dir"),
            PyObject::str_val(CompactString::from(dir)),
        );
        attrs.insert(
            CompactString::from("pw_shell"),
            PyObject::str_val(CompactString::from(shell)),
        );
        PyObject::instance_with_attrs(cls, attrs)
    }

    let getpwnam_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("pwd.getpwnam", args, 1)?;
        let name = args[0].py_to_string();
        #[cfg(unix)]
        {
            let c_name = std::ffi::CString::new(name.as_str())
                .map_err(|_| PyException::value_error("invalid user name"))?;
            let pw = unsafe { libc::getpwnam(c_name.as_ptr()) };
            if pw.is_null() {
                return Err(PyException::key_error(format!(
                    "getpwnam(): name not found: '{}'",
                    name
                )));
            }
            unsafe {
                Ok(make_pwd_struct(
                    &std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy(),
                    &std::ffi::CStr::from_ptr((*pw).pw_passwd).to_string_lossy(),
                    (*pw).pw_uid,
                    (*pw).pw_gid,
                    &std::ffi::CStr::from_ptr((*pw).pw_gecos).to_string_lossy(),
                    &std::ffi::CStr::from_ptr((*pw).pw_dir).to_string_lossy(),
                    &std::ffi::CStr::from_ptr((*pw).pw_shell).to_string_lossy(),
                ))
            }
        }
        #[cfg(not(unix))]
        {
            Err(PyException::runtime_error(
                "pwd module not available on this platform",
            ))
        }
    });

    let getpwuid_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("pwd.getpwuid", args, 1)?;
        let uid = args[0].to_int()? as u32;
        #[cfg(unix)]
        {
            let pw = unsafe { libc::getpwuid(uid) };
            if pw.is_null() {
                return Err(PyException::key_error(format!(
                    "getpwuid(): uid not found: {}",
                    uid
                )));
            }
            unsafe {
                Ok(make_pwd_struct(
                    &std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy(),
                    &std::ffi::CStr::from_ptr((*pw).pw_passwd).to_string_lossy(),
                    (*pw).pw_uid,
                    (*pw).pw_gid,
                    &std::ffi::CStr::from_ptr((*pw).pw_gecos).to_string_lossy(),
                    &std::ffi::CStr::from_ptr((*pw).pw_dir).to_string_lossy(),
                    &std::ffi::CStr::from_ptr((*pw).pw_shell).to_string_lossy(),
                ))
            }
        }
        #[cfg(not(unix))]
        {
            let _ = uid;
            Err(PyException::runtime_error(
                "pwd module not available on this platform",
            ))
        }
    });

    let getpwall_fn = make_builtin(|_args: &[PyObjectRef]| {
        #[cfg(unix)]
        {
            let mut users = Vec::new();
            unsafe {
                libc::setpwent();
                loop {
                    let pw = libc::getpwent();
                    if pw.is_null() {
                        break;
                    }
                    users.push(make_pwd_struct(
                        &std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy(),
                        &std::ffi::CStr::from_ptr((*pw).pw_passwd).to_string_lossy(),
                        (*pw).pw_uid,
                        (*pw).pw_gid,
                        &std::ffi::CStr::from_ptr((*pw).pw_gecos).to_string_lossy(),
                        &std::ffi::CStr::from_ptr((*pw).pw_dir).to_string_lossy(),
                        &std::ffi::CStr::from_ptr((*pw).pw_shell).to_string_lossy(),
                    ));
                }
                libc::endpwent();
            }
            Ok(PyObject::list(users))
        }
        #[cfg(not(unix))]
        {
            Ok(PyObject::list(vec![]))
        }
    });

    make_module(
        "pwd",
        vec![
            ("getpwnam", getpwnam_fn),
            ("getpwuid", getpwuid_fn),
            ("getpwall", getpwall_fn),
        ],
    )
}
