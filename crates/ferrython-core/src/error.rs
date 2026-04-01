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
}

impl fmt::Display for ExceptionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl ExceptionKind {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "BaseException" => Some(Self::BaseException),
            "SystemExit" => Some(Self::SystemExit),
            "KeyboardInterrupt" => Some(Self::KeyboardInterrupt),
            "GeneratorExit" => Some(Self::GeneratorExit),
            "Exception" => Some(Self::Exception),
            "StopIteration" => Some(Self::StopIteration),
            "ArithmeticError" => Some(Self::ArithmeticError),
            "OverflowError" => Some(Self::OverflowError),
            "ZeroDivisionError" => Some(Self::ZeroDivisionError),
            "AssertionError" | "AssertionError" => Some(Self::AssertionError),
            "AttributeError" => Some(Self::AttributeError),
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
}

impl PyException {
    pub fn new(kind: ExceptionKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), original: None }
    }
    pub fn with_original(kind: ExceptionKind, message: impl Into<String>, obj: PyObjectRef) -> Self {
        Self { kind, message: message.into(), original: Some(obj) }
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
    pub fn assertion_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::AssertionError, msg) }
    pub fn not_implemented_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::NotImplementedError, msg) }
    pub fn recursion_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::RecursionError, msg) }
    pub fn unbound_local_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::UnboundLocalError, msg) }
    pub fn syntax_error(msg: impl Into<String>) -> Self { Self::new(ExceptionKind::SyntaxError, msg) }
}

/// Convenience result type used throughout the VM.
pub type PyResult<T> = Result<T, PyException>;
