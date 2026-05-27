//! Truthiness, dunder dispatch, and exception matching helpers.

use crate::builtins;
use crate::VirtualMachine;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyObjectMethods, PyObjectPayload, PyObjectRef};

impl VirtualMachine {
    /// Truthiness test that dispatches __bool__/__len__ on instances.
    /// Walk a class hierarchy to find if it inherits from an ExceptionType
    pub(crate) fn find_exception_kind(cls: &PyObjectRef) -> ExceptionKind {
        match &cls.payload {
            PyObjectPayload::ExceptionType(kind) => kind.clone(),
            PyObjectPayload::BuiltinType(name) | PyObjectPayload::BuiltinFunction(name) => {
                ExceptionKind::from_name(name).unwrap_or(ExceptionKind::RuntimeError)
            }
            PyObjectPayload::Class(cd) => {
                // Check if the class name itself maps to a known exception kind
                if let Some(kind) = ExceptionKind::from_name(&cd.name) {
                    return kind;
                }
                for base in &cd.bases {
                    let kind = Self::find_exception_kind(base);
                    if !matches!(kind, ExceptionKind::RuntimeError) {
                        return kind;
                    }
                    // Also check if base IS the exception type
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        return k.clone();
                    }
                }
                // Check MRO
                for base in &cd.mro {
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        return k.clone();
                    }
                }
                ExceptionKind::RuntimeError
            }
            _ => ExceptionKind::RuntimeError,
        }
    }

    /// Check if any exception kind in the class's full MRO matches the expected handler.
    /// Unlike find_exception_kind (which returns the first non-RuntimeError kind),
    /// this checks ALL bases — essential for multiple inheritance like
    /// `BadRequestKeyError(BadRequest, KeyError)` where the second base matters.
    pub(crate) fn any_exception_kind_matches(cls: &PyObjectRef, expected: &ExceptionKind) -> bool {
        match &cls.payload {
            PyObjectPayload::ExceptionType(kind) => exception_kind_matches(kind, expected),
            PyObjectPayload::BuiltinType(name) | PyObjectPayload::BuiltinFunction(name) => {
                if let Some(kind) = ExceptionKind::from_name(name) {
                    exception_kind_matches(&kind, expected)
                } else {
                    false
                }
            }
            PyObjectPayload::Class(cd) => {
                // Direct name match
                if let Some(kind) = ExceptionKind::from_name(&cd.name) {
                    if exception_kind_matches(&kind, expected) {
                        return true;
                    }
                }
                // Check all bases recursively
                for base in &cd.bases {
                    if Self::any_exception_kind_matches(base, expected) {
                        return true;
                    }
                }
                // Check MRO entries
                for base in &cd.mro {
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        if exception_kind_matches(k, expected) {
                            return true;
                        }
                    }
                    if let PyObjectPayload::Class(bc) = &base.payload {
                        if let Some(kind) = ExceptionKind::from_name(&bc.name) {
                            if exception_kind_matches(&kind, expected) {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    pub(crate) fn vm_is_truthy(&mut self, obj: &PyObjectRef) -> PyResult<bool> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                    let referent = (nc.func)(&[])?;
                    return self.vm_is_truthy(&referent);
                }
            }
            if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__bool__") {
                let method = self.resolve_descriptor(&raw_method, obj)?;
                let result = self.call_object(method, vec![])?;
                return Ok(result.is_truthy());
            }
            if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__len__") {
                let method = self.resolve_descriptor(&raw_method, obj)?;
                let result = self.call_object(method, vec![])?;
                return Ok(result.is_truthy());
            }
            // Builtin base type subclass: delegate to __builtin_value__
            if let Some(bv) = Self::get_builtin_value(obj) {
                return Ok(bv.is_truthy());
            }
        }
        Ok(obj.is_truthy())
    }

    /// Try to call a dunder method on an instance. Returns None if the object
    /// is not an Instance or doesn't have the named dunder.
    pub(crate) fn try_call_dunder(
        &mut self,
        obj: &PyObjectRef,
        dunder: &str,
        args: Vec<PyObjectRef>,
    ) -> Result<Option<PyObjectRef>, PyException> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                // Namedtuple methods need to bypass raw class lookups like
                // tuple.__getitem__ so they can operate on the stored _tuple.
                if inst.class.get_attr("__namedtuple__").is_some() {
                    match builtins::call_method(obj, dunder, &args) {
                        Ok(result) => return Ok(Some(result)),
                        Err(e) => return Err(e),
                    }
                }
                // Use resolve_instance_dunder to skip BuiltinBoundMethod from builtin type bases
                if let Some(raw_method) = Self::resolve_instance_dunder(obj, dunder) {
                    let method = self.resolve_descriptor(&raw_method, obj)?;
                    return Ok(Some(self.call_object(method, args)?));
                }
                // Fall through: check __builtin_value__ for supported container operations
                if matches!(
                    dunder,
                    "__getitem__"
                        | "__setitem__"
                        | "__delitem__"
                        | "__contains__"
                        | "__iter__"
                        | "__len__"
                        | "__bool__"
                        | "__add__"
                        | "__mul__"
                        | "__or__"
                        | "__and__"
                        | "__sub__"
                        | "__xor__"
                        | "__ior__"
                        | "__iand__"
                        | "__isub__"
                        | "__ixor__"
                        | "__eq__"
                        | "__ne__"
                        | "__lt__"
                        | "__le__"
                        | "__gt__"
                        | "__ge__"
                ) {
                    if let Some(bv) = Self::get_builtin_value(obj) {
                        return self.try_call_dunder(&bv, dunder, args);
                    }
                }
            }
            PyObjectPayload::Module { .. } => {
                if let Some(method) = obj.get_attr(dunder) {
                    // Module methods expect self as first arg (like file objects with _bind_methods)
                    let mut method_args = vec![obj.clone()];
                    method_args.extend(args);
                    return Ok(Some(self.call_object(method, method_args)?));
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

/// Check if `actual` exception kind matches `expected` (including inheritance).
pub(crate) fn exception_kind_matches(actual: &ExceptionKind, expected: &ExceptionKind) -> bool {
    if std::mem::discriminant(actual) == std::mem::discriminant(expected) {
        return true;
    }
    // Walk the exception hierarchy
    match expected {
        ExceptionKind::BaseException => true, // catches everything
        ExceptionKind::Exception => !matches!(
            actual,
            ExceptionKind::SystemExit
                | ExceptionKind::KeyboardInterrupt
                | ExceptionKind::GeneratorExit
                | ExceptionKind::BaseExceptionGroup
        ),
        ExceptionKind::ArithmeticError => matches!(
            actual,
            ExceptionKind::ArithmeticError
                | ExceptionKind::FloatingPointError
                | ExceptionKind::OverflowError
                | ExceptionKind::ZeroDivisionError
        ),
        ExceptionKind::LookupError => matches!(
            actual,
            ExceptionKind::LookupError | ExceptionKind::IndexError | ExceptionKind::KeyError
        ),
        ExceptionKind::OSError => matches!(
            actual,
            ExceptionKind::OSError
                | ExceptionKind::BlockingIOError
                | ExceptionKind::BrokenPipeError
                | ExceptionKind::FileExistsError
                | ExceptionKind::FileNotFoundError
                | ExceptionKind::PermissionError
                | ExceptionKind::TimeoutError
                | ExceptionKind::IsADirectoryError
                | ExceptionKind::NotADirectoryError
                | ExceptionKind::ProcessLookupError
                | ExceptionKind::ConnectionError
                | ExceptionKind::ConnectionResetError
                | ExceptionKind::ConnectionAbortedError
                | ExceptionKind::ConnectionRefusedError
                | ExceptionKind::InterruptedError
                | ExceptionKind::ChildProcessError
        ),
        ExceptionKind::ConnectionError => matches!(
            actual,
            ExceptionKind::ConnectionError
                | ExceptionKind::ConnectionResetError
                | ExceptionKind::ConnectionAbortedError
                | ExceptionKind::ConnectionRefusedError
        ),
        ExceptionKind::UnicodeError => matches!(
            actual,
            ExceptionKind::UnicodeError
                | ExceptionKind::UnicodeDecodeError
                | ExceptionKind::UnicodeEncodeError
                | ExceptionKind::UnicodeTranslateError
        ),
        ExceptionKind::ValueError => matches!(
            actual,
            ExceptionKind::ValueError
                | ExceptionKind::UnicodeError
                | ExceptionKind::UnicodeDecodeError
                | ExceptionKind::UnicodeEncodeError
                | ExceptionKind::UnicodeTranslateError
                | ExceptionKind::JSONDecodeError
        ),
        ExceptionKind::Warning => matches!(
            actual,
            ExceptionKind::Warning
                | ExceptionKind::DeprecationWarning
                | ExceptionKind::RuntimeWarning
                | ExceptionKind::UserWarning
                | ExceptionKind::SyntaxWarning
                | ExceptionKind::FutureWarning
                | ExceptionKind::ImportWarning
                | ExceptionKind::UnicodeWarning
                | ExceptionKind::EncodingWarning
                | ExceptionKind::BytesWarning
                | ExceptionKind::ResourceWarning
                | ExceptionKind::PendingDeprecationWarning
        ),
        ExceptionKind::ImportError => matches!(
            actual,
            ExceptionKind::ImportError | ExceptionKind::ModuleNotFoundError
        ),
        ExceptionKind::RuntimeError => matches!(
            actual,
            ExceptionKind::RuntimeError
                | ExceptionKind::NotImplementedError
                | ExceptionKind::RecursionError
                | ExceptionKind::ReError
        ),
        ExceptionKind::NameError => matches!(
            actual,
            ExceptionKind::NameError | ExceptionKind::UnboundLocalError
        ),
        ExceptionKind::SyntaxError => matches!(
            actual,
            ExceptionKind::SyntaxError | ExceptionKind::IndentationError | ExceptionKind::TabError
        ),
        ExceptionKind::SubprocessError => matches!(
            actual,
            ExceptionKind::SubprocessError
                | ExceptionKind::CalledProcessError
                | ExceptionKind::TimeoutExpired
        ),
        ExceptionKind::BaseExceptionGroup => matches!(
            actual,
            ExceptionKind::BaseExceptionGroup | ExceptionKind::ExceptionGroup
        ),
        _ => false,
    }
}
