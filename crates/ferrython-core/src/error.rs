//! Error types and exception hierarchy for Ferrython.

use std::fmt;
use std::sync::Arc;
use thiserror::Error;

use crate::object::PyObject;
/// Type alias matching the one in object.rs.
type PyObjectRef = Arc<PyObject>;

/// The kind of exception (maps to Python's exception classes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExceptionKind {
    BaseException,
    SystemExit,
    KeyboardInterrupt,
    GeneratorExit,
    Exception,
    StopIteration,
    StopAsyncIteration,
    ArithmeticError,
    FloatingPointError,
    OverflowError,
    ZeroDivisionError,
    AssertionError,
    AttributeError,
    BlockingIOError,
    BrokenPipeError,
    BufferError,
    EOFError,
    FileExistsError,
    FileNotFoundError,
    ImportError,
    ModuleNotFoundError,
    IndexError,
    KeyError,
    LookupError,
    MemoryError,
    NameError,
    NotImplementedError,
    OSError,
    PermissionError,
    RecursionError,
    ReferenceError,
    RuntimeError,
    SyntaxError,
    SystemError,
    TypeError,
    UnboundLocalError,
    UnicodeDecodeError,
    UnicodeEncodeError,
    UnicodeError,
    ValueError,
    Warning,
    DeprecationWarning,
    RuntimeWarning,
    UserWarning,
    // Additional OS exceptions
    TimeoutError,
    IsADirectoryError,
    NotADirectoryError,
    ProcessLookupError,
    ConnectionError,
    ConnectionResetError,
    ConnectionAbortedError,
    ConnectionRefusedError,
    InterruptedError,
    ChildProcessError,
    // Additional warning types
    SyntaxWarning,
    FutureWarning,
    ImportWarning,
    UnicodeWarning,
    BytesWarning,
    ResourceWarning,
    PendingDeprecationWarning,
    // Indentation
    IndentationError,
    TabError,
    // Module-specific subclasses
    JSONDecodeError,
    // subprocess module
    SubprocessError,
    CalledProcessError,
    TimeoutExpired,
}

impl fmt::Display for ExceptionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl ExceptionKind {
    /// Check if this exception kind is a subclass of `parent`, following CPython's hierarchy.
    pub fn is_subclass_of(&self, parent: &ExceptionKind) -> bool {
        if std::mem::discriminant(self) == std::mem::discriminant(parent) { return true; }
        match parent {
            ExceptionKind::BaseException => true,
            ExceptionKind::Exception => !matches!(self,
                ExceptionKind::SystemExit | ExceptionKind::KeyboardInterrupt | ExceptionKind::GeneratorExit
            ),
            ExceptionKind::ArithmeticError => matches!(self,
                ExceptionKind::ArithmeticError | ExceptionKind::FloatingPointError |
                ExceptionKind::OverflowError | ExceptionKind::ZeroDivisionError
            ),
            ExceptionKind::LookupError => matches!(self,
                ExceptionKind::LookupError | ExceptionKind::IndexError | ExceptionKind::KeyError
            ),
            ExceptionKind::OSError => matches!(self,
                ExceptionKind::OSError | ExceptionKind::FileExistsError |
                ExceptionKind::FileNotFoundError | ExceptionKind::PermissionError |
                ExceptionKind::TimeoutError | ExceptionKind::IsADirectoryError |
                ExceptionKind::NotADirectoryError | ExceptionKind::ProcessLookupError |
                ExceptionKind::ConnectionError | ExceptionKind::ConnectionResetError |
                ExceptionKind::ConnectionAbortedError | ExceptionKind::ConnectionRefusedError |
                ExceptionKind::InterruptedError | ExceptionKind::ChildProcessError |
                ExceptionKind::BlockingIOError | ExceptionKind::BrokenPipeError
            ),
            ExceptionKind::ConnectionError => matches!(self,
                ExceptionKind::ConnectionError | ExceptionKind::ConnectionResetError |
                ExceptionKind::ConnectionAbortedError | ExceptionKind::ConnectionRefusedError |
                ExceptionKind::BrokenPipeError
            ),
            ExceptionKind::ValueError => matches!(self,
                ExceptionKind::ValueError | ExceptionKind::UnicodeError |
                ExceptionKind::UnicodeDecodeError | ExceptionKind::UnicodeEncodeError |
                ExceptionKind::JSONDecodeError
            ),
            ExceptionKind::UnicodeError => matches!(self,
                ExceptionKind::UnicodeError | ExceptionKind::UnicodeDecodeError |
                ExceptionKind::UnicodeEncodeError
            ),
            ExceptionKind::Warning => matches!(self,
                ExceptionKind::Warning | ExceptionKind::DeprecationWarning |
                ExceptionKind::RuntimeWarning | ExceptionKind::UserWarning |
                ExceptionKind::SyntaxWarning | ExceptionKind::FutureWarning |
                ExceptionKind::ImportWarning | ExceptionKind::UnicodeWarning |
                ExceptionKind::BytesWarning | ExceptionKind::ResourceWarning |
                ExceptionKind::PendingDeprecationWarning
            ),
            ExceptionKind::ImportError => matches!(self,
                ExceptionKind::ImportError | ExceptionKind::ModuleNotFoundError
            ),
            ExceptionKind::RuntimeError => matches!(self,
                ExceptionKind::RuntimeError | ExceptionKind::RecursionError |
                ExceptionKind::NotImplementedError
            ),
            ExceptionKind::SyntaxError => matches!(self,
                ExceptionKind::SyntaxError | ExceptionKind::IndentationError |
                ExceptionKind::TabError
            ),
            ExceptionKind::IndentationError => matches!(self,
                ExceptionKind::IndentationError | ExceptionKind::TabError
            ),
            ExceptionKind::NameError => matches!(self,
                ExceptionKind::NameError | ExceptionKind::UnboundLocalError
            ),
            ExceptionKind::SubprocessError => matches!(self,
                ExceptionKind::SubprocessError | ExceptionKind::CalledProcessError |
                ExceptionKind::TimeoutExpired
            ),
            _ => false,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "BaseException" => Some(Self::BaseException),
            "SystemExit" => Some(Self::SystemExit),
            "KeyboardInterrupt" => Some(Self::KeyboardInterrupt),
            "GeneratorExit" => Some(Self::GeneratorExit),
            "Exception" => Some(Self::Exception),
            "StopIteration" => Some(Self::StopIteration),
            "StopAsyncIteration" => Some(Self::StopAsyncIteration),
            "ArithmeticError" => Some(Self::ArithmeticError),
            "FloatingPointError" => Some(Self::FloatingPointError),
            "OverflowError" => Some(Self::OverflowError),
            "ZeroDivisionError" => Some(Self::ZeroDivisionError),
            "AssertionError" => Some(Self::AssertionError),
            "AttributeError" => Some(Self::AttributeError),
            "BlockingIOError" => Some(Self::BlockingIOError),
            "BrokenPipeError" => Some(Self::BrokenPipeError),
            "BufferError" => Some(Self::BufferError),
            "EOFError" => Some(Self::EOFError),
            "FileExistsError" => Some(Self::FileExistsError),
            "FileNotFoundError" => Some(Self::FileNotFoundError),
            "ImportError" => Some(Self::ImportError),
            "ModuleNotFoundError" => Some(Self::ModuleNotFoundError),
            "IndexError" => Some(Self::IndexError),
            "KeyError" => Some(Self::KeyError),
            "LookupError" => Some(Self::LookupError),
            "MemoryError" => Some(Self::MemoryError),
            "NameError" => Some(Self::NameError),
            "NotImplementedError" => Some(Self::NotImplementedError),
            "OSError" | "IOError" => Some(Self::OSError),
            "PermissionError" => Some(Self::PermissionError),
            "RecursionError" => Some(Self::RecursionError),
            "ReferenceError" => Some(Self::ReferenceError),
            "RuntimeError" => Some(Self::RuntimeError),
            "SyntaxError" => Some(Self::SyntaxError),
            "SystemError" => Some(Self::SystemError),
            "TypeError" => Some(Self::TypeError),
            "UnboundLocalError" => Some(Self::UnboundLocalError),
            "UnicodeDecodeError" => Some(Self::UnicodeDecodeError),
            "UnicodeEncodeError" => Some(Self::UnicodeEncodeError),
            "UnicodeError" => Some(Self::UnicodeError),
            "ValueError" => Some(Self::ValueError),
            "Warning" => Some(Self::Warning),
            "DeprecationWarning" => Some(Self::DeprecationWarning),
            "RuntimeWarning" => Some(Self::RuntimeWarning),
            "UserWarning" => Some(Self::UserWarning),
            "TimeoutError" => Some(Self::TimeoutError),
            "IsADirectoryError" => Some(Self::IsADirectoryError),
            "NotADirectoryError" => Some(Self::NotADirectoryError),
            "ProcessLookupError" => Some(Self::ProcessLookupError),
            "ConnectionError" => Some(Self::ConnectionError),
            "ConnectionResetError" => Some(Self::ConnectionResetError),
            "ConnectionAbortedError" => Some(Self::ConnectionAbortedError),
            "ConnectionRefusedError" => Some(Self::ConnectionRefusedError),
            "InterruptedError" => Some(Self::InterruptedError),
            "ChildProcessError" => Some(Self::ChildProcessError),
            "SyntaxWarning" => Some(Self::SyntaxWarning),
            "FutureWarning" => Some(Self::FutureWarning),
            "ImportWarning" => Some(Self::ImportWarning),
            "UnicodeWarning" => Some(Self::UnicodeWarning),
            "BytesWarning" => Some(Self::BytesWarning),
            "ResourceWarning" => Some(Self::ResourceWarning),
            "PendingDeprecationWarning" => Some(Self::PendingDeprecationWarning),
            "IndentationError" => Some(Self::IndentationError),
            "TabError" => Some(Self::TabError),
            "JSONDecodeError" | "json.JSONDecodeError" => Some(Self::JSONDecodeError),
            "SubprocessError" => Some(Self::SubprocessError),
            "CalledProcessError" => Some(Self::CalledProcessError),
            "TimeoutExpired" => Some(Self::TimeoutExpired),
            _ => None,
        }
    }
}

/// A Python exception carrying a kind and message.
#[derive(Debug, Clone, Error)]
#[error("{kind}: {message}")]
pub struct PyException {
    pub kind: ExceptionKind,
    pub message: String,
    /// Original Python object (Instance) if raised from a custom exception class.
    pub original: Option<PyObjectRef>,
    /// Call stack frames at point of raise: `(filename, function, lineno)`.
    pub traceback: Vec<TracebackEntry>,
    /// Explicit exception cause (`raise X from Y`). Maps to __cause__.
    pub cause: Option<Box<PyException>>,
    /// Implicit exception context (exception active when this was raised). Maps to __context__.
    pub context: Option<Box<PyException>>,
    /// Value carried by StopIteration (generator return value).
    pub value: Option<PyObjectRef>,
    /// OSError details: (errno, strerror, filename) — populated by `from_io_error`.
    pub os_error_info: Option<OsErrorInfo>,
}

/// Details for OSError and its subclasses (FileNotFoundError, PermissionError, etc.).
#[derive(Debug, Clone)]
pub struct OsErrorInfo {
    pub errno: i32,
    pub strerror: String,
    pub filename: Option<String>,
}

/// A single entry in a Python traceback.
#[derive(Debug, Clone)]
pub struct TracebackEntry {
    pub filename: String,
    pub function: String,
    pub lineno: u32,
}

impl PyException {
    pub fn new(kind: ExceptionKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), original: None, traceback: Vec::new(), cause: None, context: None, value: None, os_error_info: None }
    }
    pub fn with_original(kind: ExceptionKind, message: impl Into<String>, obj: PyObjectRef) -> Self {
        Self { kind, message: message.into(), original: Some(obj), traceback: Vec::new(), cause: None, context: None, value: None, os_error_info: None }
    }
    pub fn type_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::TypeError, msg) }
    pub fn value_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::ValueError, msg) }
    pub fn name_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::NameError, msg) }
    pub fn attribute_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::AttributeError, msg) }
    pub fn runtime_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::RuntimeError, msg) }
    pub fn import_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::ImportError, msg) }
    pub fn index_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::IndexError, msg) }
    pub fn key_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::KeyError, msg) }
    pub fn zero_division_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::ZeroDivisionError, msg) }
    pub fn overflow_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::OverflowError, msg) }
    pub fn stop_iteration() -> Self { Self::new(ExceptionKind::StopIteration, "") }
    pub fn os_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::OSError, msg) }
    pub fn file_not_found_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::FileNotFoundError, msg) }
    pub fn permission_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::PermissionError, msg) }

    /// Create an OSError (or appropriate subclass) from a `std::io::Error`, with
    /// `.errno`, `.strerror`, and `.filename` attributes like CPython.
    pub fn from_io_error(err: &std::io::Error, filename: Option<&str>) -> Self {
        use std::io::ErrorKind;
        let errno = err.raw_os_error().unwrap_or(0);
        let strerror = err.to_string();
        let kind = match err.kind() {
            ErrorKind::NotFound => ExceptionKind::FileNotFoundError,
            ErrorKind::PermissionDenied => ExceptionKind::PermissionError,
            ErrorKind::AlreadyExists => ExceptionKind::FileExistsError,
            ErrorKind::TimedOut => ExceptionKind::TimeoutError,
            _ => ExceptionKind::OSError,
        };
        let msg = if let Some(fname) = filename {
            format!("[Errno {}] {}: '{}'", errno, strerror, fname)
        } else {
            format!("[Errno {}] {}", errno, strerror)
        };
        let mut exc = Self::new(kind, msg);
        exc.os_error_info = Some(OsErrorInfo {
            errno,
            strerror,
            filename: filename.map(|s| s.to_string()),
        });
        exc
    }
    pub fn assertion_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::AssertionError, msg) }
    pub fn not_implemented_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::NotImplementedError, msg) }
    pub fn recursion_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::RecursionError, msg) }
    pub fn unbound_local_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::UnboundLocalError, msg) }
    pub fn syntax_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::SyntaxError, msg) }
    pub fn json_decode_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::JSONDecodeError, msg) }
    pub fn system_exit(code: PyObjectRef) -> Self {
        let mut exc = Self::new(ExceptionKind::SystemExit, "");
        exc.value = Some(code);
        exc
    }
}

// ── Cross-crate error conversions ──

impl From<ferrython_parser::ParseError> for PyException {
    fn from(e: ferrython_parser::ParseError) -> Self {
        PyException::syntax_error(format!("{}", e))
    }
}

impl From<ferrython_compiler::CompileError> for PyException {
    fn from(e: ferrython_compiler::CompileError) -> Self {
        PyException::syntax_error(format!("{}", e))
    }
}

/// Convenience result type used throughout the VM.
pub type PyResult<T> = Result<T, PyException>;

// ── Thread-local exception info (for sys.exc_info() / traceback.format_exc()) ──

use std::cell::{RefCell, Cell};

thread_local! {
    static THREAD_EXC_INFO: RefCell<Option<(ExceptionKind, String, Vec<TracebackEntry>)>>
        = RefCell::new(None);
}

/// Store the current exception info in thread-local storage.
/// Called by the VM when entering an except block.
pub fn set_thread_exc_info(kind: ExceptionKind, msg: String, tb: Vec<TracebackEntry>) {
    THREAD_EXC_INFO.with(|c| *c.borrow_mut() = Some((kind, msg, tb)));
}

/// Clear thread-local exception info. Called when leaving except blocks.
pub fn clear_thread_exc_info() {
    THREAD_EXC_INFO.with(|c| *c.borrow_mut() = None);
}

/// Get the current thread-local exception info, if any.
pub fn get_thread_exc_info() -> Option<(ExceptionKind, String, Vec<TracebackEntry>)> {
    THREAD_EXC_INFO.with(|c| c.borrow().clone())
}

// ── Pending VM method call (stdlib → VM callback) ──
// Stdlib modules can't call Python functions directly (no VM access).
// When a NativeClosure needs the VM to call a method, it stores the
// request here and the VM executes it after the NativeClosure returns.

thread_local! {
    static PENDING_VM_CALLS: RefCell<Vec<(PyObjectRef, Vec<PyObjectRef>)>>
        = RefCell::new(Vec::new());
    static COLLECT_VM_RESULTS: Cell<bool> = Cell::new(false);
}

/// Request the VM to call `func` with `args` after the current NativeClosure returns.
/// Multiple calls can be queued — they execute in order.
pub fn request_vm_call(func: PyObjectRef, args: Vec<PyObjectRef>) {
    PENDING_VM_CALLS.with(|c| c.borrow_mut().push((func, args)));
}

/// Take the pending VM call request (called by the VM after NativeClosure dispatch).
/// Returns calls in FIFO order; returns None when queue is empty.
pub fn take_pending_vm_call() -> Option<(PyObjectRef, Vec<PyObjectRef>)> {
    PENDING_VM_CALLS.with(|c| {
        let mut calls = c.borrow_mut();
        if calls.is_empty() { None } else { Some(calls.remove(0)) }
    })
}

/// When set, the VM collects all pending call results into a list
/// (used by Pool.map to gather results from multiple Python function calls).
pub fn set_collect_vm_call_results(val: bool) {
    COLLECT_VM_RESULTS.with(|c| c.set(val));
}

pub fn take_collect_vm_call_results() -> bool {
    COLLECT_VM_RESULTS.with(|c| {
        let val = c.get();
        c.set(false);
        val
    })
}
