use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef,
};
use indexmap::IndexMap;

// ── grp module (Unix group database) ──

pub fn create_grp_module() -> PyObjectRef {
    #[cfg(unix)]
    fn make_grp_struct(name: &str, passwd: &str, gid: u32, members: Vec<String>) -> PyObjectRef {
        let cls = PyObject::class(CompactString::from("struct_group"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("gr_name"),
            PyObject::str_val(CompactString::from(name)),
        );
        attrs.insert(
            CompactString::from("gr_passwd"),
            PyObject::str_val(CompactString::from(passwd)),
        );
        attrs.insert(CompactString::from("gr_gid"), PyObject::int(gid as i64));
        attrs.insert(
            CompactString::from("gr_mem"),
            PyObject::list(
                members
                    .iter()
                    .map(|m| PyObject::str_val(CompactString::from(m.as_str())))
                    .collect(),
            ),
        );
        PyObject::instance_with_attrs(cls, attrs)
    }

    let getgrnam_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("grp.getgrnam", args, 1)?;
        let name = args[0].py_to_string();
        #[cfg(unix)]
        {
            let c_name = std::ffi::CString::new(name.as_str())
                .map_err(|_| PyException::value_error("invalid group name"))?;
            let grp = unsafe { libc::getgrnam(c_name.as_ptr()) };
            if grp.is_null() {
                return Err(PyException::key_error(format!(
                    "getgrnam(): name not found: '{}'",
                    name
                )));
            }
            unsafe {
                let gr_name = std::ffi::CStr::from_ptr((*grp).gr_name)
                    .to_string_lossy()
                    .into_owned();
                let gr_passwd = std::ffi::CStr::from_ptr((*grp).gr_passwd)
                    .to_string_lossy()
                    .into_owned();
                let gid = (*grp).gr_gid;
                let mut members = Vec::new();
                let mut p = (*grp).gr_mem;
                if !p.is_null() && (p as usize) % std::mem::align_of::<*mut std::ffi::c_char>() == 0
                {
                    while !(*p).is_null() {
                        members.push(std::ffi::CStr::from_ptr(*p).to_string_lossy().into_owned());
                        p = p.add(1);
                    }
                }
                Ok(make_grp_struct(&gr_name, &gr_passwd, gid, members))
            }
        }
        #[cfg(not(unix))]
        {
            Err(PyException::runtime_error(
                "grp module not available on this platform",
            ))
        }
    });

    let getgrgid_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("grp.getgrgid", args, 1)?;
        let gid = args[0].to_int()? as u32;
        #[cfg(unix)]
        {
            let grp = unsafe { libc::getgrgid(gid) };
            if grp.is_null() {
                return Err(PyException::key_error(format!(
                    "getgrgid(): gid not found: {}",
                    gid
                )));
            }
            unsafe {
                let gr_name = std::ffi::CStr::from_ptr((*grp).gr_name)
                    .to_string_lossy()
                    .into_owned();
                let gr_passwd = std::ffi::CStr::from_ptr((*grp).gr_passwd)
                    .to_string_lossy()
                    .into_owned();
                let gid = (*grp).gr_gid;
                let mut members = Vec::new();
                let mut p = (*grp).gr_mem;
                if !p.is_null() && (p as usize) % std::mem::align_of::<*mut std::ffi::c_char>() == 0
                {
                    while !(*p).is_null() {
                        members.push(std::ffi::CStr::from_ptr(*p).to_string_lossy().into_owned());
                        p = p.add(1);
                    }
                }
                Ok(make_grp_struct(&gr_name, &gr_passwd, gid, members))
            }
        }
        #[cfg(not(unix))]
        {
            let _ = gid;
            Err(PyException::runtime_error(
                "grp module not available on this platform",
            ))
        }
    });

    let getgrall_fn = make_builtin(|_args: &[PyObjectRef]| {
        #[cfg(unix)]
        {
            let mut groups = Vec::new();
            unsafe {
                libc::setgrent();
                loop {
                    let grp = libc::getgrent();
                    if grp.is_null() {
                        break;
                    }
                    let gr_name = std::ffi::CStr::from_ptr((*grp).gr_name)
                        .to_string_lossy()
                        .into_owned();
                    let gr_passwd = std::ffi::CStr::from_ptr((*grp).gr_passwd)
                        .to_string_lossy()
                        .into_owned();
                    let gid = (*grp).gr_gid;
                    let mut members = Vec::new();
                    let mut p = (*grp).gr_mem;
                    if !p.is_null()
                        && (p as usize) % std::mem::align_of::<*mut std::ffi::c_char>() == 0
                    {
                        while !(*p).is_null() {
                            members
                                .push(std::ffi::CStr::from_ptr(*p).to_string_lossy().into_owned());
                            p = p.add(1);
                        }
                    }
                    groups.push(make_grp_struct(&gr_name, &gr_passwd, gid, members));
                }
                libc::endgrent();
            }
            Ok(PyObject::list(groups))
        }
        #[cfg(not(unix))]
        {
            Ok(PyObject::list(vec![]))
        }
    });

    make_module(
        "grp",
        vec![
            ("getgrnam", getgrnam_fn),
            ("getgrgid", getgrgid_fn),
            ("getgrall", getgrall_fn),
        ],
    )
}

// ── pwd module (Unix password database) ──
