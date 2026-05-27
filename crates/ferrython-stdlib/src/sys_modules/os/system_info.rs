use super::*;

pub(super) fn os_get_terminal_size(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(80);
    let lines = std::env::var("LINES")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(24);
    Ok(make_terminal_size_instance(cols, lines))
}

pub(super) fn os_uname(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
        let cls = PyObject::class(CompactString::from("uname_result"), vec![], IndexMap::new());
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
}

pub(super) fn os_times(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
            PyObject::float(0.0),
        ]))
    }
    #[cfg(not(unix))]
    {
        Ok(PyObject::tuple(vec![PyObject::float(0.0); 5]))
    }
}
