use compact_str::CompactString;
use ferrython_core::object::{make_module, PyObject, PyObjectRef};

// ── builtins module ──

pub fn create_builtins_module() -> PyObjectRef {
    fn exception_types() -> Vec<(&'static str, PyObjectRef)> {
        use ferrython_core::error::ExceptionKind;
        [
            ("BaseException", ExceptionKind::BaseException),
            ("BaseExceptionGroup", ExceptionKind::BaseExceptionGroup),
            ("GeneratorExit", ExceptionKind::GeneratorExit),
            ("KeyboardInterrupt", ExceptionKind::KeyboardInterrupt),
            ("SystemExit", ExceptionKind::SystemExit),
            ("Exception", ExceptionKind::Exception),
            ("ArithmeticError", ExceptionKind::ArithmeticError),
            ("FloatingPointError", ExceptionKind::FloatingPointError),
            ("OverflowError", ExceptionKind::OverflowError),
            ("ZeroDivisionError", ExceptionKind::ZeroDivisionError),
            ("AssertionError", ExceptionKind::AssertionError),
            ("AttributeError", ExceptionKind::AttributeError),
            ("BufferError", ExceptionKind::BufferError),
            ("EOFError", ExceptionKind::EOFError),
            ("ExceptionGroup", ExceptionKind::ExceptionGroup),
            ("ImportError", ExceptionKind::ImportError),
            ("ModuleNotFoundError", ExceptionKind::ModuleNotFoundError),
            ("LookupError", ExceptionKind::LookupError),
            ("IndexError", ExceptionKind::IndexError),
            ("KeyError", ExceptionKind::KeyError),
            ("MemoryError", ExceptionKind::MemoryError),
            ("NameError", ExceptionKind::NameError),
            ("UnboundLocalError", ExceptionKind::UnboundLocalError),
            ("OSError", ExceptionKind::OSError),
            ("IOError", ExceptionKind::OSError),
            ("EnvironmentError", ExceptionKind::OSError),
            ("BlockingIOError", ExceptionKind::BlockingIOError),
            ("ChildProcessError", ExceptionKind::ChildProcessError),
            ("ConnectionError", ExceptionKind::ConnectionError),
            ("BrokenPipeError", ExceptionKind::BrokenPipeError),
            (
                "ConnectionAbortedError",
                ExceptionKind::ConnectionAbortedError,
            ),
            (
                "ConnectionRefusedError",
                ExceptionKind::ConnectionRefusedError,
            ),
            ("ConnectionResetError", ExceptionKind::ConnectionResetError),
            ("FileExistsError", ExceptionKind::FileExistsError),
            ("FileNotFoundError", ExceptionKind::FileNotFoundError),
            ("InterruptedError", ExceptionKind::InterruptedError),
            ("IsADirectoryError", ExceptionKind::IsADirectoryError),
            ("NotADirectoryError", ExceptionKind::NotADirectoryError),
            ("PermissionError", ExceptionKind::PermissionError),
            ("ProcessLookupError", ExceptionKind::ProcessLookupError),
            ("TimeoutError", ExceptionKind::TimeoutError),
            ("ReferenceError", ExceptionKind::ReferenceError),
            ("RuntimeError", ExceptionKind::RuntimeError),
            ("NotImplementedError", ExceptionKind::NotImplementedError),
            ("RecursionError", ExceptionKind::RecursionError),
            ("StopAsyncIteration", ExceptionKind::StopAsyncIteration),
            ("StopIteration", ExceptionKind::StopIteration),
            ("SyntaxError", ExceptionKind::SyntaxError),
            ("IndentationError", ExceptionKind::IndentationError),
            ("TabError", ExceptionKind::TabError),
            ("SystemError", ExceptionKind::SystemError),
            ("TypeError", ExceptionKind::TypeError),
            ("ValueError", ExceptionKind::ValueError),
            ("UnicodeError", ExceptionKind::UnicodeError),
            ("UnicodeDecodeError", ExceptionKind::UnicodeDecodeError),
            ("UnicodeEncodeError", ExceptionKind::UnicodeEncodeError),
            (
                "UnicodeTranslateError",
                ExceptionKind::UnicodeTranslateError,
            ),
            ("Warning", ExceptionKind::Warning),
            ("BytesWarning", ExceptionKind::BytesWarning),
            ("DeprecationWarning", ExceptionKind::DeprecationWarning),
            ("EncodingWarning", ExceptionKind::EncodingWarning),
            ("FutureWarning", ExceptionKind::FutureWarning),
            ("ImportWarning", ExceptionKind::ImportWarning),
            (
                "PendingDeprecationWarning",
                ExceptionKind::PendingDeprecationWarning,
            ),
            ("ResourceWarning", ExceptionKind::ResourceWarning),
            ("RuntimeWarning", ExceptionKind::RuntimeWarning),
            ("SyntaxWarning", ExceptionKind::SyntaxWarning),
            ("UnicodeWarning", ExceptionKind::UnicodeWarning),
            ("UserWarning", ExceptionKind::UserWarning),
        ]
        .into_iter()
        .map(|(name, kind)| (name, PyObject::exception_type(kind)))
        .collect()
    }

    let mut attrs = vec![
        (
            "__name__",
            PyObject::str_val(CompactString::from("builtins")),
        ),
        (
            "__doc__",
            PyObject::str_val(CompactString::from(
                "Built-in functions, exceptions, and other objects.",
            )),
        ),
        (
            "print",
            PyObject::builtin_function(CompactString::from("print")),
        ),
        (
            "len",
            PyObject::builtin_function(CompactString::from("len")),
        ),
        (
            "range",
            PyObject::builtin_function(CompactString::from("range")),
        ),
    ];
    attrs.extend(exception_types());

    make_module("builtins", {
        attrs.extend(vec![
            ("int", PyObject::builtin_type(CompactString::from("int"))),
            (
                "float",
                PyObject::builtin_type(CompactString::from("float")),
            ),
            ("str", PyObject::builtin_type(CompactString::from("str"))),
            ("bool", PyObject::builtin_type(CompactString::from("bool"))),
            ("list", PyObject::builtin_type(CompactString::from("list"))),
            (
                "tuple",
                PyObject::builtin_type(CompactString::from("tuple")),
            ),
            ("dict", PyObject::builtin_type(CompactString::from("dict"))),
            ("set", PyObject::builtin_type(CompactString::from("set"))),
            (
                "frozenset",
                PyObject::builtin_type(CompactString::from("frozenset")),
            ),
            (
                "bytes",
                PyObject::builtin_type(CompactString::from("bytes")),
            ),
            (
                "bytearray",
                PyObject::builtin_type(CompactString::from("bytearray")),
            ),
            ("type", PyObject::builtin_type(CompactString::from("type"))),
            (
                "object",
                PyObject::builtin_type(CompactString::from("object")),
            ),
            (
                "complex",
                PyObject::builtin_type(CompactString::from("complex")),
            ),
            (
                "super",
                PyObject::builtin_type(CompactString::from("super")),
            ),
            (
                "property",
                PyObject::builtin_type(CompactString::from("property")),
            ),
            (
                "classmethod",
                PyObject::builtin_type(CompactString::from("classmethod")),
            ),
            (
                "staticmethod",
                PyObject::builtin_type(CompactString::from("staticmethod")),
            ),
            (
                "abs",
                PyObject::builtin_function(CompactString::from("abs")),
            ),
            (
                "all",
                PyObject::builtin_function(CompactString::from("all")),
            ),
            (
                "any",
                PyObject::builtin_function(CompactString::from("any")),
            ),
            (
                "ascii",
                PyObject::builtin_function(CompactString::from("ascii")),
            ),
            (
                "bin",
                PyObject::builtin_function(CompactString::from("bin")),
            ),
            (
                "callable",
                PyObject::builtin_function(CompactString::from("callable")),
            ),
            (
                "chr",
                PyObject::builtin_function(CompactString::from("chr")),
            ),
            (
                "dir",
                PyObject::builtin_function(CompactString::from("dir")),
            ),
            (
                "divmod",
                PyObject::builtin_function(CompactString::from("divmod")),
            ),
            (
                "enumerate",
                PyObject::builtin_type(CompactString::from("enumerate")),
            ),
            (
                "eval",
                PyObject::builtin_function(CompactString::from("eval")),
            ),
            (
                "exec",
                PyObject::builtin_function(CompactString::from("exec")),
            ),
            (
                "filter",
                PyObject::builtin_function(CompactString::from("filter")),
            ),
            (
                "format",
                PyObject::builtin_function(CompactString::from("format")),
            ),
            (
                "getattr",
                PyObject::builtin_function(CompactString::from("getattr")),
            ),
            (
                "globals",
                PyObject::builtin_function(CompactString::from("globals")),
            ),
            (
                "hasattr",
                PyObject::builtin_function(CompactString::from("hasattr")),
            ),
            (
                "hash",
                PyObject::builtin_function(CompactString::from("hash")),
            ),
            (
                "hex",
                PyObject::builtin_function(CompactString::from("hex")),
            ),
            ("id", PyObject::builtin_function(CompactString::from("id"))),
            (
                "input",
                PyObject::builtin_function(CompactString::from("input")),
            ),
            (
                "isinstance",
                PyObject::builtin_function(CompactString::from("isinstance")),
            ),
            (
                "issubclass",
                PyObject::builtin_function(CompactString::from("issubclass")),
            ),
            (
                "iter",
                PyObject::builtin_function(CompactString::from("iter")),
            ),
            (
                "locals",
                PyObject::builtin_function(CompactString::from("locals")),
            ),
            (
                "map",
                PyObject::builtin_function(CompactString::from("map")),
            ),
            (
                "max",
                PyObject::builtin_function(CompactString::from("max")),
            ),
            (
                "min",
                PyObject::builtin_function(CompactString::from("min")),
            ),
            (
                "next",
                PyObject::builtin_function(CompactString::from("next")),
            ),
            (
                "oct",
                PyObject::builtin_function(CompactString::from("oct")),
            ),
            (
                "open",
                PyObject::builtin_function(CompactString::from("open")),
            ),
            (
                "ord",
                PyObject::builtin_function(CompactString::from("ord")),
            ),
            (
                "pow",
                PyObject::builtin_function(CompactString::from("pow")),
            ),
            (
                "repr",
                PyObject::builtin_function(CompactString::from("repr")),
            ),
            (
                "reversed",
                PyObject::builtin_function(CompactString::from("reversed")),
            ),
            (
                "round",
                PyObject::builtin_function(CompactString::from("round")),
            ),
            (
                "setattr",
                PyObject::builtin_function(CompactString::from("setattr")),
            ),
            (
                "sorted",
                PyObject::builtin_function(CompactString::from("sorted")),
            ),
            (
                "sum",
                PyObject::builtin_function(CompactString::from("sum")),
            ),
            (
                "vars",
                PyObject::builtin_function(CompactString::from("vars")),
            ),
            (
                "zip",
                PyObject::builtin_function(CompactString::from("zip")),
            ),
            (
                "__import__",
                PyObject::builtin_function(CompactString::from("__import__")),
            ),
            (
                "__build_class__",
                PyObject::builtin_function(CompactString::from("__build_class__")),
            ),
            // Exception types
            (
                "Exception",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::Exception),
            ),
            (
                "ValueError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::ValueError),
            ),
            (
                "TypeError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::TypeError),
            ),
            (
                "KeyError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::KeyError),
            ),
            (
                "IndexError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::IndexError),
            ),
            (
                "AttributeError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::AttributeError),
            ),
            (
                "NameError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::NameError),
            ),
            (
                "RuntimeError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
            (
                "StopIteration",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::StopIteration),
            ),
            (
                "OSError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::OSError),
            ),
            (
                "IOError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::OSError),
            ),
            (
                "FileNotFoundError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::FileNotFoundError),
            ),
            (
                "ImportError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::ImportError),
            ),
            (
                "NotImplementedError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::NotImplementedError),
            ),
            (
                "ZeroDivisionError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::ZeroDivisionError),
            ),
            (
                "OverflowError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::OverflowError),
            ),
            (
                "AssertionError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::AssertionError),
            ),
            (
                "SyntaxError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::SyntaxError),
            ),
            // Additional builtins
            (
                "breakpoint",
                PyObject::builtin_function(CompactString::from("breakpoint")),
            ),
            (
                "compile",
                PyObject::builtin_function(CompactString::from("compile")),
            ),
            (
                "delattr",
                PyObject::builtin_function(CompactString::from("delattr")),
            ),
            (
                "memoryview",
                PyObject::builtin_type(CompactString::from("memoryview")),
            ),
            (
                "slice",
                PyObject::builtin_type(CompactString::from("slice")),
            ),
            ("NotImplemented", PyObject::not_implemented()),
            ("Ellipsis", PyObject::ellipsis()),
            ("__debug__", PyObject::bool_val(true)),
        ]);
        attrs
    })
}
