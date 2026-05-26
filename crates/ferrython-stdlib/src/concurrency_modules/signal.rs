use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use super::push_deferred_call;

// ── signal module ────────────────────────────────────────────────────

use std::cell::RefCell as SignalRefCell;
use std::collections::HashMap as SignalMap;

thread_local! {
    static SIGNAL_HANDLERS: SignalRefCell<SignalMap<i64, PyObjectRef>> = SignalRefCell::new(SignalMap::new());
}

pub fn create_signal_module() -> PyObjectRef {
    // Signal constants (POSIX values)
    make_module(
        "signal",
        vec![
            ("SIGABRT", PyObject::int(6)),
            ("SIGALRM", PyObject::int(14)),
            ("SIGBUS", PyObject::int(7)),
            ("SIGCHLD", PyObject::int(17)),
            ("SIGCONT", PyObject::int(18)),
            ("SIGFPE", PyObject::int(8)),
            ("SIGHUP", PyObject::int(1)),
            ("SIGILL", PyObject::int(4)),
            ("SIGINT", PyObject::int(2)),
            ("SIGKILL", PyObject::int(9)),
            ("SIGPIPE", PyObject::int(13)),
            ("SIGQUIT", PyObject::int(3)),
            ("SIGSEGV", PyObject::int(11)),
            ("SIGSTOP", PyObject::int(19)),
            ("SIGTERM", PyObject::int(15)),
            ("SIGUSR1", PyObject::int(10)),
            ("SIGUSR2", PyObject::int(12)),
            ("SIGWINCH", PyObject::int(28)),
            ("NSIG", PyObject::int(65)),
            ("SIG_DFL", PyObject::int(0)),
            ("SIG_IGN", PyObject::int(1)),
            (
                "signal",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("signal() requires 2 arguments"));
                    }
                    let signum = args[0].to_int()?;
                    let handler = args[1].clone();
                    let handler_is_callable = matches!(
                        handler.payload,
                        PyObjectPayload::Function(_)
                            | PyObjectPayload::NativeFunction(_)
                            | PyObjectPayload::NativeClosure(_)
                            | PyObjectPayload::BoundMethod { .. }
                    );
                    // Return previous handler, store new one
                    let prev = SIGNAL_HANDLERS.with(|h| {
                        let mut map = h.borrow_mut();
                        let old = map
                            .get(&signum)
                            .cloned()
                            .unwrap_or_else(|| PyObject::int(0));
                        map.insert(signum, handler.clone());
                        old
                    });
                    // Install real OS signal handler so the process doesn't die
                    #[cfg(unix)]
                    {
                        use std::sync::atomic::{AtomicU64, Ordering};
                        // Global bitmask of signals with pending Python handlers
                        static PENDING_SIGNALS: AtomicU64 = AtomicU64::new(0);

                        let handler_int = handler.to_int().unwrap_or(-1);
                        if handler_int == 0 {
                            // SIG_DFL — restore default
                            unsafe {
                                libc::signal(signum as libc::c_int, libc::SIG_DFL);
                            }
                        } else if handler_int == 1 {
                            // SIG_IGN — ignore
                            unsafe {
                                libc::signal(signum as libc::c_int, libc::SIG_IGN);
                            }
                        } else if handler_is_callable && signum < 64 {
                            // Install a C handler that sets the pending bit
                            unsafe {
                                libc::signal(
                                    signum as libc::c_int,
                                    flag_signal_handler as *const () as libc::sighandler_t,
                                );
                            }
                        }

                        extern "C" fn flag_signal_handler(sig: libc::c_int) {
                            if sig >= 0 && sig < 64 {
                                PENDING_SIGNALS.fetch_or(1u64 << sig, Ordering::SeqCst);
                            }
                            // Re-arm (System V signal semantics reset to SIG_DFL after delivery)
                            unsafe {
                                libc::signal(
                                    sig,
                                    flag_signal_handler as *const () as libc::sighandler_t,
                                );
                            }
                        }
                    }
                    Ok(prev)
                }),
            ),
            (
                "getsignal",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("getsignal() requires 1 argument"));
                    }
                    let signum = args[0].to_int()?;
                    let handler = SIGNAL_HANDLERS.with(|h| {
                        h.borrow()
                            .get(&signum)
                            .cloned()
                            .unwrap_or_else(|| PyObject::int(0))
                    });
                    Ok(handler)
                }),
            ),
            (
                "raise_signal",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "raise_signal() requires 1 argument",
                        ));
                    }
                    let signum = args[0].to_int()?;
                    // Dispatch Python handler directly if registered
                    let handler = SIGNAL_HANDLERS.with(|h| h.borrow().get(&signum).cloned());
                    if let Some(ref h) = handler {
                        let h_int = h.to_int().unwrap_or(-1);
                        if h_int != 0 && h_int != 1 {
                            // It's a Python callable — invoke via deferred calls (VM will execute)
                            let call_args = vec![PyObject::int(signum), PyObject::none()];
                            match &h.payload {
                                PyObjectPayload::NativeFunction(nf) => {
                                    return (nf.func)(&call_args);
                                }
                                PyObjectPayload::NativeClosure(nc) => {
                                    return (nc.func)(&call_args);
                                }
                                _ => {
                                    // Python function — use deferred call mechanism
                                    push_deferred_call(h.clone(), call_args);
                                    return Ok(PyObject::none());
                                }
                            }
                        }
                    }
                    // No Python handler or SIG_DFL/SIG_IGN — raise through OS
                    #[cfg(unix)]
                    unsafe {
                        libc::raise(signum as libc::c_int);
                    }
                    Ok(PyObject::none())
                }),
            ),
            (
                "alarm",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("alarm() requires 1 argument"));
                    }
                    let secs = args[0].to_int()? as u32;
                    #[cfg(unix)]
                    let remaining = unsafe { libc::alarm(secs) };
                    #[cfg(not(unix))]
                    {
                        let _ = secs;
                        return Err(PyException::os_error(
                            "alarm() is not supported on this platform",
                        ));
                    }
                    #[cfg(unix)]
                    Ok(PyObject::int(remaining as i64))
                }),
            ),
            (
                "pause",
                make_builtin(|_| {
                    #[cfg(unix)]
                    {
                        unsafe {
                            libc::pause();
                        }
                        Ok(PyObject::none())
                    }
                    #[cfg(not(unix))]
                    Err(PyException::os_error(
                        "pause() is not supported on this platform",
                    ))
                }),
            ),
            (
                "set_wakeup_fd",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Ok(PyObject::int(-1));
                    }
                    let _fd = args[0].to_int()?;
                    Ok(PyObject::int(-1)) // return previous fd (-1 = none)
                }),
            ),
            (
                "valid_signals",
                make_builtin(|_| {
                    // Return set of valid signal numbers
                    let mut sigs = IndexMap::new();
                    for i in 1..32i64 {
                        if i != 9 && i != 19 {
                            // SIGKILL and SIGSTOP can't be caught
                            let obj = PyObject::int(i);
                            let key = HashableKey::Int(ferrython_core::types::PyInt::Small(i));
                            sigs.insert(key, obj);
                        }
                    }
                    Ok(PyObject::set(sigs))
                }),
            ),
            (
                "strsignal",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("strsignal() requires 1 argument"));
                    }
                    let signum = args[0].to_int()?;
                    let name = match signum {
                        1 => "Hangup",
                        2 => "Interrupt",
                        3 => "Quit",
                        4 => "Illegal instruction",
                        6 => "Aborted",
                        7 => "Bus error",
                        8 => "Floating point exception",
                        9 => "Killed",
                        10 => "User defined signal 1",
                        11 => "Segmentation fault",
                        12 => "User defined signal 2",
                        13 => "Broken pipe",
                        14 => "Alarm clock",
                        15 => "Terminated",
                        17 => "Child exited",
                        18 => "Continued",
                        19 => "Stopped",
                        28 => "Window changed",
                        _ => "Unknown signal",
                    };
                    Ok(PyObject::str_val(CompactString::from(name)))
                }),
            ),
            ("Signals", PyObject::none()),
            ("Handlers", PyObject::none()),
        ],
    )
}
