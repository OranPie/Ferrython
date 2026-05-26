use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

// ── getpass module ───────────────────────────────────────────────────
pub fn create_getpass_module() -> PyObjectRef {
    fn getpass_getuser(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .or_else(|_| std::env::var("USERNAME"));
        let user = match user {
            Ok(u) => u,
            Err(_) => {
                // Last resort: try whoami command (unix)
                std::process::Command::new("whoami")
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            }
        };
        Ok(PyObject::str_val(CompactString::from(user)))
    }

    fn getpass_getpass(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let prompt = if args.is_empty() {
            "Password: "
        } else {
            args[0].as_str().unwrap_or("Password: ")
        };
        eprint!("{}", prompt);
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| PyException::runtime_error(format!("getpass failed: {}", e)))?;
        Ok(PyObject::str_val(CompactString::from(input.trim_end())))
    }

    make_module(
        "getpass",
        vec![
            ("getuser", make_builtin(getpass_getuser)),
            ("getpass", make_builtin(getpass_getpass)),
        ],
    )
}

// ── errno module ──
