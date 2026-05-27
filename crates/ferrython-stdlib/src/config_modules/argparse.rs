//! argparse module implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, CompareOp, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

mod namespace;
mod parse;
mod parser;

use namespace::create_argparse_namespace_class;
use parse::argparse_parse_args;
use parser::{
    argparse_add_argument, argparse_add_argument_group, argparse_add_mutually_exclusive_group,
    argparse_add_subparsers, argparse_argument_parser_repr, argparse_error, argparse_exit,
    argparse_get_default, argparse_parse_args_method, argparse_parse_known_args_method,
    argparse_print_help, argparse_set_defaults, init_argument_parser,
};

pub fn create_argparse_module() -> PyObjectRef {
    let namespace_cls = create_argparse_namespace_class();
    let ns_for_init = namespace_cls.clone();

    let mut ap_ns = IndexMap::new();
    ap_ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("ArgumentParser.__init__", move |args: &[PyObjectRef]| {
            check_args_min("ArgumentParser.__init__", args, 1)?;
            let ap_cls = if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error(
                    "ArgumentParser.__init__ requires an instance",
                ));
            };
            init_argument_parser(&args[0], &ap_cls, &ns_for_init, &args[1..])?;
            Ok(PyObject::none())
        }),
    );
    ap_ns.insert(
        CompactString::from("add_argument"),
        PyObject::native_function("ArgumentParser.add_argument", argparse_add_argument),
    );
    ap_ns.insert(
        CompactString::from("add_argument_group"),
        PyObject::native_function(
            "ArgumentParser.add_argument_group",
            argparse_add_argument_group,
        ),
    );
    ap_ns.insert(
        CompactString::from("parse_args"),
        PyObject::native_function("ArgumentParser.parse_args", argparse_parse_args_method),
    );
    ap_ns.insert(
        CompactString::from("parse_known_args"),
        PyObject::native_function(
            "ArgumentParser.parse_known_args",
            argparse_parse_known_args_method,
        ),
    );
    ap_ns.insert(
        CompactString::from("print_help"),
        PyObject::native_function("ArgumentParser.print_help", argparse_print_help),
    );
    ap_ns.insert(
        CompactString::from("add_subparsers"),
        PyObject::native_function("ArgumentParser.add_subparsers", argparse_add_subparsers),
    );
    ap_ns.insert(
        CompactString::from("set_defaults"),
        PyObject::native_function("ArgumentParser.set_defaults", argparse_set_defaults),
    );
    ap_ns.insert(
        CompactString::from("get_default"),
        PyObject::native_function("ArgumentParser.get_default", argparse_get_default),
    );
    ap_ns.insert(
        CompactString::from("add_mutually_exclusive_group"),
        PyObject::native_function(
            "ArgumentParser.add_mutually_exclusive_group",
            argparse_add_mutually_exclusive_group,
        ),
    );
    ap_ns.insert(
        CompactString::from("exit"),
        PyObject::native_function("ArgumentParser.exit", argparse_exit),
    );
    ap_ns.insert(
        CompactString::from("error"),
        PyObject::native_function("ArgumentParser.error", argparse_error),
    );
    ap_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_function("ArgumentParser.__repr__", argparse_argument_parser_repr),
    );
    let ap_cls = PyObject::class(CompactString::from("ArgumentParser"), vec![], ap_ns);

    make_module(
        "argparse",
        vec![
            ("ArgumentParser", ap_cls),
            ("Namespace", namespace_cls),
            ("Action", make_builtin(|_| Ok(PyObject::none()))),
            ("HelpFormatter", make_builtin(|_| Ok(PyObject::none()))),
            (
                "RawDescriptionHelpFormatter",
                make_builtin(|_| Ok(PyObject::none())),
            ),
            (
                "RawTextHelpFormatter",
                make_builtin(|_| Ok(PyObject::none())),
            ),
            (
                "ArgumentDefaultsHelpFormatter",
                make_builtin(|_| Ok(PyObject::none())),
            ),
            (
                "SUPPRESS",
                PyObject::str_val(CompactString::from("==SUPPRESS==")),
            ),
            ("OPTIONAL", PyObject::str_val(CompactString::from("?"))),
            ("ZERO_OR_MORE", PyObject::str_val(CompactString::from("*"))),
            ("ONE_OR_MORE", PyObject::str_val(CompactString::from("+"))),
            ("REMAINDER", PyObject::str_val(CompactString::from("..."))),
            (
                "FileType",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    Ok(args[0].clone())
                }),
            ),
        ],
    )
}
