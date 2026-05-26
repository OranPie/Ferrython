//! xml.parsers and xml.parsers.expat modules.

use compact_str::CompactString;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

/// xml.parsers.expat — minimal expat parser interface
pub fn create_xml_parsers_expat_module() -> PyObjectRef {
    let expat_error = PyObject::class(CompactString::from("ExpatError"), vec![], IndexMap::new());

    let parser_create = make_builtin(|args: &[PyObjectRef]| {
        let encoding = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            "UTF-8".to_string()
        };
        let cls = PyObject::class(CompactString::from("xmlparser"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("encoding"),
                PyObject::str_val(CompactString::from(&encoding)),
            );
            w.insert(
                CompactString::from("Parse"),
                make_builtin(|_| Ok(PyObject::int(1))),
            );
            w.insert(
                CompactString::from("ParseFile"),
                make_builtin(|_| Ok(PyObject::int(1))),
            );
            w.insert(
                CompactString::from("SetBase"),
                make_builtin(|_| Ok(PyObject::none())),
            );
            w.insert(
                CompactString::from("GetBase"),
                make_builtin(|_| Ok(PyObject::none())),
            );
            w.insert(
                CompactString::from("GetInputContext"),
                make_builtin(|_| Ok(PyObject::none())),
            );
            w.insert(
                CompactString::from("ExternalEntityParserCreate"),
                make_builtin(|_| Ok(PyObject::none())),
            );
            w.insert(
                CompactString::from("SetParamEntityParsing"),
                make_builtin(|_| Ok(PyObject::none())),
            );
            w.insert(
                CompactString::from("UseForeignDTD"),
                make_builtin(|_| Ok(PyObject::none())),
            );
            for cb in &[
                "StartElementHandler",
                "EndElementHandler",
                "CharacterDataHandler",
                "ProcessingInstructionHandler",
                "CommentHandler",
                "StartNamespaceDeclHandler",
                "EndNamespaceDeclHandler",
                "DefaultHandler",
                "DefaultHandlerExpand",
                "NotStandaloneHandler",
                "ExternalEntityRefHandler",
            ] {
                w.insert(CompactString::from(*cb), PyObject::none());
            }
            w.insert(
                CompactString::from("buffer_text"),
                PyObject::bool_val(false),
            );
            w.insert(
                CompactString::from("ordered_attributes"),
                PyObject::bool_val(false),
            );
            w.insert(
                CompactString::from("returns_unicode"),
                PyObject::bool_val(true),
            );
        }
        Ok(inst)
    });

    let error_cls = PyObject::class(CompactString::from("errors"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = error_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("XML_ERROR_SYNTAX"),
            PyObject::str_val(CompactString::from("syntax error")),
        );
        ns.insert(
            CompactString::from("XML_ERROR_NO_MEMORY"),
            PyObject::str_val(CompactString::from("out of memory")),
        );
    }

    make_module(
        "xml.parsers.expat",
        vec![
            ("ParserCreate", parser_create),
            ("ExpatError", expat_error.clone()),
            ("error", expat_error),
            ("errors", error_cls),
            ("XML_PARAM_ENTITY_PARSING_NEVER", PyObject::int(0)),
            (
                "XML_PARAM_ENTITY_PARSING_UNLESS_STANDALONE",
                PyObject::int(1),
            ),
            ("XML_PARAM_ENTITY_PARSING_ALWAYS", PyObject::int(2)),
        ],
    )
}

/// xml.parsers — package namespace
pub fn create_xml_parsers_module() -> PyObjectRef {
    make_module("xml.parsers", vec![])
}
