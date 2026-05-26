use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, to_shared_fx, ExceptionInstanceData, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef, SharedFxAttrMap,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

pub fn create_subprocess_module() -> PyObjectRef {
    // CalledProcessError(returncode, cmd, output=None, stderr=None)
    let cpe_cls = PyObject::class(
        CompactString::from("CalledProcessError"),
        vec![],
        IndexMap::new(),
    );
    let cpe_cls_ref = cpe_cls.clone();
    let _called_process_error =
        PyObject::native_closure("CalledProcessError", move |args: &[PyObjectRef]| {
            let returncode = if !args.is_empty() {
                args[0].clone()
            } else {
                PyObject::int(1)
            };
            let cmd = if args.len() > 1 {
                args[1].clone()
            } else {
                PyObject::str_val(CompactString::from(""))
            };
            let output = if args.len() > 2 {
                args[2].clone()
            } else {
                PyObject::none()
            };
            let stderr = if args.len() > 3 {
                args[3].clone()
            } else {
                PyObject::none()
            };
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("returncode"), returncode.clone());
            attrs.insert(CompactString::from("cmd"), cmd.clone());
            attrs.insert(CompactString::from("output"), output);
            attrs.insert(CompactString::from("stderr"), stderr);
            let msg = format!(
                "Command '{}' returned non-zero exit status {}.",
                cmd.py_to_string(),
                returncode.py_to_string()
            );
            attrs.insert(
                CompactString::from("args"),
                PyObject::tuple(vec![PyObject::str_val(CompactString::from(&msg))]),
            );
            attrs.insert(
                CompactString::from("__str__"),
                PyObject::native_closure("__str__", move |_: &[PyObjectRef]| {
                    Ok(PyObject::str_val(CompactString::from(&msg)))
                }),
            );
            Ok(PyObject::instance_with_attrs(cpe_cls_ref.clone(), attrs))
        });

    let completed_process_cls = PyObject::class(
        CompactString::from("CompletedProcess"),
        vec![],
        IndexMap::new(),
    );
    make_module(
        "subprocess",
        vec![
            ("PIPE", PyObject::int(-1)),
            ("STDOUT", PyObject::int(-2)),
            ("DEVNULL", PyObject::int(-3)),
            (
                "CalledProcessError",
                PyObject::exception_type(ExceptionKind::CalledProcessError),
            ),
            (
                "SubprocessError",
                PyObject::exception_type(ExceptionKind::RuntimeError),
            ),
            ("CompletedProcess", completed_process_cls),
            ("run", make_builtin(subprocess_run)),
            ("call", make_builtin(subprocess_call)),
            ("check_output", make_builtin(subprocess_check_output)),
            ("check_call", make_builtin(subprocess_check_call)),
            ("Popen", make_builtin(subprocess_popen)),
            ("getoutput", make_builtin(subprocess_getoutput)),
            ("getstatusoutput", make_builtin(subprocess_getstatusoutput)),
            (
                "TimeoutExpired",
                PyObject::exception_type(ExceptionKind::TimeoutExpired),
            ),
        ],
    )
}

fn subprocess_run(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("subprocess.run requires arguments"));
    }
    // Accept either a list of strings or a single string (for shell=True)
    let cmd_parts: Vec<String> = match &args[0].payload {
        PyObjectPayload::Str(s) => vec![s.to_string()],
        _ => args[0]
            .to_list()?
            .iter()
            .map(|a| a.py_to_string())
            .collect(),
    };
    if cmd_parts.is_empty() {
        return Err(PyException::value_error("empty command"));
    }

    let mut text_mode = false;
    let mut capture = false;
    let mut cwd: Option<String> = None;
    let mut shell = false;
    let mut check = false;
    let mut input_data: Option<Vec<u8>> = None;
    let mut env_vars: Option<Vec<(String, String)>> = None;
    let mut timeout_secs: Option<f64> = None;

    for arg in &args[1..] {
        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
            let r = kw_map.read();
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("text"))) {
                text_mode = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                "universal_newlines",
            ))) {
                text_mode = text_mode || v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("capture_output"))) {
                capture = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("cwd"))) {
                cwd = Some(v.py_to_string());
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("shell"))) {
                shell = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("check"))) {
                check = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("input"))) {
                match &v.payload {
                    PyObjectPayload::Bytes(b) => input_data = Some((**b).clone()),
                    PyObjectPayload::Str(s) => input_data = Some(s.as_bytes().to_vec()),
                    _ if !matches!(v.payload, PyObjectPayload::None) => {
                        input_data = Some(v.py_to_string().into_bytes())
                    }
                    _ => {}
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("timeout"))) {
                if let Ok(t) = v.to_float() {
                    timeout_secs = Some(t);
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("env"))) {
                if let PyObjectPayload::Dict(env_map) = &v.payload {
                    let er = env_map.read();
                    let mut pairs = Vec::new();
                    for (k, val) in er.iter() {
                        let key_str = match k {
                            HashableKey::Str(s) => s.to_string(),
                            HashableKey::Int(i) => i.to_string(),
                            _ => continue,
                        };
                        pairs.push((key_str, val.py_to_string()));
                    }
                    env_vars = Some(pairs);
                }
            }
        }
    }

    let mut cmd = if shell {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(cmd_parts.join(" "));
        c
    } else {
        let mut c = std::process::Command::new(&cmd_parts[0]);
        c.args(&cmd_parts[1..]);
        c
    };

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    if let Some(pairs) = env_vars {
        cmd.env_clear();
        for (k, v) in pairs {
            cmd.env(k, v);
        }
    }

    // If input is provided, pipe stdin
    if input_data.is_some() {
        cmd.stdin(std::process::Stdio::piped());
    }
    if capture {
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
    }

    if let Some(data) = input_data {
        let mut child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| PyException::runtime_error(format!("subprocess error: {}", e)))?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(&data);
        }
        if let Some(t) = timeout_secs {
            // Poll-based timeout: try_wait in a loop
            let dur = std::time::Duration::from_secs_f64(t);
            let start = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_status)) => {
                        let out = child.wait_with_output().map_err(|e| {
                            PyException::runtime_error(format!("subprocess error: {}", e))
                        })?;
                        return build_completed_process(
                            out.status.code().unwrap_or(-1),
                            out.stdout,
                            out.stderr,
                            text_mode,
                            check,
                        );
                    }
                    Ok(None) => {
                        if start.elapsed() >= dur {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Err(PyException::runtime_error("subprocess.TimeoutExpired"));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(e) => {
                        return Err(PyException::runtime_error(format!(
                            "subprocess error: {}",
                            e
                        )))
                    }
                }
            }
        }
        let out = child
            .wait_with_output()
            .map_err(|e| PyException::runtime_error(format!("subprocess error: {}", e)))?;
        return build_completed_process(
            out.status.code().unwrap_or(-1),
            out.stdout,
            out.stderr,
            text_mode,
            check,
        );
    }

    // Handle timeout for non-input case
    if let Some(t) = timeout_secs {
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| PyException::runtime_error(format!("subprocess error: {}", e)))?;
        let dur = std::time::Duration::from_secs_f64(t);
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    let out = child.wait_with_output().map_err(|e| {
                        PyException::runtime_error(format!("subprocess error: {}", e))
                    })?;
                    return build_completed_process(
                        out.status.code().unwrap_or(-1),
                        out.stdout,
                        out.stderr,
                        text_mode,
                        check,
                    );
                }
                Ok(None) => {
                    if start.elapsed() >= dur {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(PyException::runtime_error("subprocess.TimeoutExpired"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => {
                    return Err(PyException::runtime_error(format!(
                        "subprocess error: {}",
                        e
                    )))
                }
            }
        }
    }

    // No stdin input, no timeout — simple output capture
    let output = cmd.output();
    match output {
        Ok(out) => build_completed_process(
            out.status.code().unwrap_or(-1),
            out.stdout,
            out.stderr,
            text_mode,
            check,
        ),
        Err(e) => Err(PyException::runtime_error(format!(
            "subprocess error: {}",
            e
        ))),
    }
}

fn subprocess_getoutput(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "getoutput() requires a command string",
        ));
    }
    let cmd = args[0].py_to_string();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();
    match output {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).to_string();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&err);
            }
            // Strip trailing newline like CPython
            if combined.ends_with('\n') {
                combined.pop();
            }
            Ok(PyObject::str_val(CompactString::from(combined)))
        }
        Err(e) => Err(PyException::runtime_error(format!("getoutput: {}", e))),
    }
}

fn subprocess_getstatusoutput(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "getstatusoutput() requires a command string",
        ));
    }
    let cmd = args[0].py_to_string();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();
    match output {
        Ok(out) => {
            let code = out.status.code().unwrap_or(-1) as i64;
            let mut combined = String::from_utf8_lossy(&out.stdout).to_string();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&err);
            }
            if combined.ends_with('\n') {
                combined.pop();
            }
            Ok(PyObject::tuple(vec![
                PyObject::int(code),
                PyObject::str_val(CompactString::from(combined)),
            ]))
        }
        Err(e) => Err(PyException::runtime_error(format!(
            "getstatusoutput: {}",
            e
        ))),
    }
}

fn build_completed_process(
    returncode: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    text_mode: bool,
    check: bool,
) -> PyResult<PyObjectRef> {
    if check && returncode != 0 {
        return Err(PyException::runtime_error(format!(
            "Command returned non-zero exit status {}",
            returncode
        )));
    }
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("returncode"),
        PyObject::int(returncode as i64),
    );
    if text_mode {
        ns.insert(
            CompactString::from("stdout"),
            PyObject::str_val(CompactString::from(
                String::from_utf8_lossy(&stdout).as_ref(),
            )),
        );
        ns.insert(
            CompactString::from("stderr"),
            PyObject::str_val(CompactString::from(
                String::from_utf8_lossy(&stderr).as_ref(),
            )),
        );
    } else {
        ns.insert(CompactString::from("stdout"), PyObject::bytes(stdout));
        ns.insert(CompactString::from("stderr"), PyObject::bytes(stderr));
    }
    let cls = PyObject::class(
        CompactString::from("CompletedProcess"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        for (k, v) in ns {
            attrs.insert(k, v);
        }
    }
    Ok(inst)
}

fn subprocess_call(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let result = subprocess_run(args)?;
    if let Some(rc) = result.get_attr("returncode") {
        Ok(rc)
    } else {
        Ok(PyObject::int(0))
    }
}

fn subprocess_check_call(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let result = subprocess_run(args)?;
    let rc = result
        .get_attr("returncode")
        .and_then(|v| v.as_int())
        .unwrap_or(0);
    if rc != 0 {
        let cmd = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            String::new()
        };
        let msg = format!("Command '{}' returned non-zero exit status {}", cmd, rc);
        let mut ex = PyException::new(ExceptionKind::CalledProcessError, &msg);
        // Build an ExceptionInstance with returncode attr for catching
        let exc_attrs = indexmap::IndexMap::from([
            (CompactString::from("returncode"), PyObject::int(rc)),
            (
                CompactString::from("cmd"),
                if !args.is_empty() {
                    args[0].clone()
                } else {
                    PyObject::none()
                },
            ),
            (CompactString::from("output"), PyObject::none()),
            (CompactString::from("stderr"), PyObject::none()),
        ]);
        ex.original = Some(PyObject::wrap(PyObjectPayload::ExceptionInstance(
            std::mem::ManuallyDrop::new(Box::new(ExceptionInstanceData::new_attrs(
                ExceptionKind::CalledProcessError,
                msg.into(),
                vec![PyObject::int(rc)],
                Some(to_shared_fx(exc_attrs)),
            ))),
        )));
        return Err(ex);
    }
    Ok(PyObject::int(rc))
}

fn subprocess_check_output(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let result = subprocess_run(args)?;
    if let Some(stdout) = result.get_attr("stdout") {
        Ok(stdout)
    } else {
        Ok(PyObject::bytes(vec![]))
    }
}

fn subprocess_popen(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::sync::{Arc, Mutex};

    if args.is_empty() {
        return Err(PyException::type_error("Popen requires args"));
    }
    // Accept either a list or a string
    let cmd_parts: Vec<String> = match &args[0].payload {
        PyObjectPayload::Str(s) => vec![s.to_string()],
        _ => args[0]
            .to_list()?
            .iter()
            .map(|a| a.py_to_string())
            .collect(),
    };
    if cmd_parts.is_empty() {
        return Err(PyException::value_error("empty command"));
    }
    let mut capture_stdout = false;
    let mut capture_stderr = false;
    let mut pipe_stdin = false;
    let mut cwd: Option<String> = None;
    let mut shell = false;
    let mut text_mode = false;
    let mut env_vars: Option<Vec<(String, String)>> = None;

    for arg in &args[1..] {
        if let PyObjectPayload::Dict(kw_map) = &arg.payload {
            let r = kw_map.read();
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("stdout"))) {
                capture_stdout = v.as_int().unwrap_or(0) == -1; // PIPE
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("stderr"))) {
                capture_stderr = v.as_int().unwrap_or(0) == -1;
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("stdin"))) {
                pipe_stdin = v.as_int().unwrap_or(0) == -1;
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("cwd"))) {
                cwd = Some(v.py_to_string());
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("shell"))) {
                shell = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("text"))) {
                text_mode = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                "universal_newlines",
            ))) {
                text_mode = text_mode || v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("env"))) {
                if let PyObjectPayload::Dict(env_map) = &v.payload {
                    let er = env_map.read();
                    let mut pairs = Vec::new();
                    for (k, val) in er.iter() {
                        let key_str = match k {
                            HashableKey::Str(s) => s.to_string(),
                            _ => continue,
                        };
                        pairs.push((key_str, val.py_to_string()));
                    }
                    env_vars = Some(pairs);
                }
            }
        }
    }

    let mut cmd = if shell {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(cmd_parts.join(" "));
        c
    } else {
        let mut c = std::process::Command::new(&cmd_parts[0]);
        c.args(&cmd_parts[1..]);
        c
    };

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    if let Some(pairs) = env_vars {
        cmd.env_clear();
        for (k, v) in pairs {
            cmd.env(k, v);
        }
    }
    if capture_stdout {
        cmd.stdout(std::process::Stdio::piped());
    }
    if capture_stderr {
        cmd.stderr(std::process::Stdio::piped());
    }
    if pipe_stdin {
        cmd.stdin(std::process::Stdio::piped());
    }

    let child = cmd
        .spawn()
        .map_err(|e| PyException::runtime_error(&format!("Popen: {e}")))?;
    let child_pid = child.id() as i64;
    let child_arc = Arc::new(Mutex::new(Some(child)));

    let cls = PyObject::class(CompactString::from("Popen"), vec![], IndexMap::new());
    let inst_ref = PyObject::instance(cls);
    let inst_attrs = if let PyObjectPayload::Instance(data) = &inst_ref.payload {
        data.attrs.clone()
    } else {
        unreachable!()
    };

    // Set initial attributes
    {
        let mut a = inst_attrs.write();
        a.insert(CompactString::from("returncode"), PyObject::none());
        a.insert(CompactString::from("args"), args[0].clone());
        a.insert(CompactString::from("pid"), PyObject::int(child_pid));
        // Expose stdout/stderr/stdin as None or PIPE marker
        a.insert(
            CompactString::from("stdout"),
            if capture_stdout {
                PyObject::str_val(CompactString::from("<pipe>"))
            } else {
                PyObject::none()
            },
        );
        a.insert(
            CompactString::from("stderr"),
            if capture_stderr {
                PyObject::str_val(CompactString::from("<pipe>"))
            } else {
                PyObject::none()
            },
        );
        a.insert(
            CompactString::from("stdin"),
            if pipe_stdin {
                PyObject::str_val(CompactString::from("<pipe>"))
            } else {
                PyObject::none()
            },
        );
    }
    let is_text = text_mode;

    // Helper: update returncode on instance
    fn set_returncode(attrs: &SharedFxAttrMap, code: i32) {
        attrs.write().insert(
            CompactString::from("returncode"),
            PyObject::int(code as i64),
        );
    }

    // communicate(input=None)
    {
        let ch = child_arc.clone();
        let ia = inst_attrs.clone();
        inst_attrs.write().insert(
            CompactString::from("communicate"),
            PyObject::native_closure("Popen.communicate", move |args| {
                // input can be positional arg[0] or in a kwargs dict
                let mut input_data: Option<Vec<u8>> = None;
                for arg in args.iter() {
                    if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                        let r = kw_map.read();
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("input")))
                        {
                            if !matches!(v.payload, PyObjectPayload::None) {
                                input_data = Some(match &v.payload {
                                    PyObjectPayload::Bytes(b) => (**b).clone(),
                                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                                    _ => v.py_to_string().into_bytes(),
                                });
                            }
                        }
                    } else if !matches!(arg.payload, PyObjectPayload::None) && input_data.is_none()
                    {
                        input_data = Some(match &arg.payload {
                            PyObjectPayload::Bytes(b) => (**b).clone(),
                            PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                            _ => arg.py_to_string().into_bytes(),
                        });
                    }
                }
                let mut guard = ch.lock().unwrap();
                if let Some(child) = guard.take() {
                    let mut child = child;
                    if let Some(data) = input_data {
                        if let Some(ref mut stdin) = child.stdin {
                            use std::io::Write;
                            let _ = stdin.write_all(&data);
                        }
                        child.stdin.take(); // close stdin
                    }
                    let out = child
                        .wait_with_output()
                        .map_err(|e| PyException::runtime_error(&format!("communicate: {e}")))?;
                    set_returncode(&ia, out.status.code().unwrap_or(-1));
                    let stdout = if is_text {
                        PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(&out.stdout).as_ref(),
                        ))
                    } else {
                        PyObject::bytes(out.stdout)
                    };
                    let stderr = if is_text {
                        PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(&out.stderr).as_ref(),
                        ))
                    } else {
                        PyObject::bytes(out.stderr)
                    };
                    Ok(PyObject::tuple(vec![stdout, stderr]))
                } else {
                    let empty = if is_text {
                        PyObject::str_val(CompactString::new(""))
                    } else {
                        PyObject::bytes(vec![])
                    };
                    Ok(PyObject::tuple(vec![empty.clone(), empty]))
                }
            }),
        );
    }

    // wait(timeout=None)
    {
        let ch = child_arc.clone();
        let ia = inst_attrs.clone();
        inst_attrs.write().insert(
            CompactString::from("wait"),
            PyObject::native_closure("Popen.wait", move |_args| {
                let mut guard = ch.lock().unwrap();
                if let Some(ref mut child) = *guard {
                    let status = child
                        .wait()
                        .map_err(|e| PyException::runtime_error(&format!("wait: {e}")))?;
                    let code = status.code().unwrap_or(-1);
                    set_returncode(&ia, code);
                    Ok(PyObject::int(code as i64))
                } else {
                    let rc = ia.read().get("returncode").cloned();
                    Ok(rc.unwrap_or_else(|| PyObject::int(-1)))
                }
            }),
        );
    }

    // poll()
    {
        let ch = child_arc.clone();
        let ia = inst_attrs.clone();
        inst_attrs.write().insert(
            CompactString::from("poll"),
            PyObject::native_closure("Popen.poll", move |_args| {
                let mut guard = ch.lock().unwrap();
                if let Some(ref mut child) = *guard {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            let code = status.code().unwrap_or(-1);
                            set_returncode(&ia, code);
                            Ok(PyObject::int(code as i64))
                        }
                        Ok(None) => Ok(PyObject::none()),
                        Err(e) => Err(PyException::runtime_error(&format!("poll: {e}"))),
                    }
                } else {
                    let rc = ia.read().get("returncode").cloned();
                    Ok(rc.unwrap_or_else(PyObject::none))
                }
            }),
        );
    }

    // kill()
    {
        let ch = child_arc.clone();
        inst_attrs.write().insert(
            CompactString::from("kill"),
            PyObject::native_closure("Popen.kill", move |_args| {
                let mut guard = ch.lock().unwrap();
                if let Some(ref mut child) = *guard {
                    child
                        .kill()
                        .map_err(|e| PyException::runtime_error(&format!("kill: {e}")))?;
                }
                Ok(PyObject::none())
            }),
        );
    }

    // terminate() — sends SIGTERM on Unix
    {
        let ch = child_arc.clone();
        inst_attrs.write().insert(
            CompactString::from("terminate"),
            PyObject::native_closure("Popen.terminate", move |_args| {
                let mut guard = ch.lock().unwrap();
                if let Some(ref mut child) = *guard {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
                    }
                    #[cfg(not(unix))]
                    {
                        child
                            .kill()
                            .map_err(|e| PyException::runtime_error(&format!("terminate: {e}")))?;
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    // send_signal(sig)
    {
        let ch = child_arc.clone();
        inst_attrs.write().insert(
            CompactString::from("send_signal"),
            PyObject::native_closure("Popen.send_signal", move |args| {
                let sig = if !args.is_empty() {
                    args[0].as_int().unwrap_or(15) as i32
                } else {
                    15
                };
                let guard = ch.lock().unwrap();
                if let Some(ref child) = *guard {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(child.id() as libc::pid_t, sig);
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = sig;
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    // __enter__ / __exit__ for context manager
    {
        let ir = inst_ref.clone();
        inst_attrs.write().insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("Popen.__enter__", move |_args| Ok(ir.clone())),
        );
        let ch = child_arc.clone();
        let ia = inst_attrs.clone();
        inst_attrs.write().insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("Popen.__exit__", move |_args| {
                let mut guard = ch.lock().unwrap();
                if let Some(ref mut child) = *guard {
                    let _ = child.kill();
                    if let Ok(s) = child.wait() {
                        set_returncode(&ia, s.code().unwrap_or(-1));
                    }
                }
                Ok(PyObject::bool_val(false))
            }),
        );
    }

    Ok(inst_ref)
}
