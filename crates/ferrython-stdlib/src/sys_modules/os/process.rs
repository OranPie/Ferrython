use super::*;

pub(super) fn os_getenv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn os_cpu_count(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(num_cpus() as i64))
}

pub(super) fn os_getpid(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(std::process::id() as i64))
}

pub(super) fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

pub(super) fn os_system(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
pub(super) fn os_popen(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
