//! xml.sax helper modules.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── xml.sax module ──

pub fn create_xml_sax_module() -> PyObjectRef {
    let content_handler_cls = PyObject::class(
        CompactString::from("ContentHandler"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = content_handler_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("startDocument"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("endDocument"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("startElement"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("endElement"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("characters"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
    }

    let error_handler_cls =
        PyObject::class(CompactString::from("ErrorHandler"), vec![], IndexMap::new());

    let sax_exception =
        PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError);

    let make_parser_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("XMLReader"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("setContentHandler"),
                make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
            );
            w.insert(
                CompactString::from("setErrorHandler"),
                make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
            );
            w.insert(
                CompactString::from("parse"),
                make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
            );
        }
        Ok(inst)
    });

    make_module(
        "xml.sax",
        vec![
            ("ContentHandler", content_handler_cls),
            ("ErrorHandler", error_handler_cls),
            ("SAXException", sax_exception.clone()),
            ("SAXParseException", sax_exception),
            ("make_parser", make_parser_fn),
            (
                "parseString",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            ),
            (
                "parse",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            ),
        ],
    )
}

/// xml.sax.handler — SAX handler base classes
pub fn create_xml_sax_handler_module() -> PyObjectRef {
    let content_handler = PyObject::class(
        CompactString::from("ContentHandler"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = content_handler.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("startDocument"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("endDocument"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("startElement"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("endElement"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("characters"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("setDocumentLocator"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
        );
    }
    let error_handler =
        PyObject::class(CompactString::from("ErrorHandler"), vec![], IndexMap::new());
    let dtd_handler = PyObject::class(CompactString::from("DTDHandler"), vec![], IndexMap::new());
    let entity_resolver = PyObject::class(
        CompactString::from("EntityResolver"),
        vec![],
        IndexMap::new(),
    );

    make_module(
        "xml.sax.handler",
        vec![
            ("ContentHandler", content_handler),
            ("ErrorHandler", error_handler),
            ("DTDHandler", dtd_handler),
            ("EntityResolver", entity_resolver),
            (
                "feature_namespaces",
                PyObject::str_val(CompactString::from(
                    "http://xml.org/sax/features/namespaces",
                )),
            ),
            (
                "feature_namespace_prefixes",
                PyObject::str_val(CompactString::from(
                    "http://xml.org/sax/features/namespace-prefixes",
                )),
            ),
            (
                "feature_validation",
                PyObject::str_val(CompactString::from(
                    "http://xml.org/sax/features/validation",
                )),
            ),
            (
                "feature_external_ges",
                PyObject::str_val(CompactString::from(
                    "http://xml.org/sax/features/external-general-entities",
                )),
            ),
            (
                "feature_external_pes",
                PyObject::str_val(CompactString::from(
                    "http://xml.org/sax/features/external-parameter-entities",
                )),
            ),
            ("all_features", PyObject::list(vec![])),
            (
                "property_lexical_handler",
                PyObject::str_val(CompactString::from(
                    "http://xml.org/sax/properties/lexical-handler",
                )),
            ),
            (
                "property_declaration_handler",
                PyObject::str_val(CompactString::from(
                    "http://xml.org/sax/properties/declaration-handler",
                )),
            ),
            ("all_properties", PyObject::list(vec![])),
        ],
    )
}

/// xml.sax.saxutils — escape/unescape and XMLGenerator
pub fn create_xml_sax_saxutils_module() -> PyObjectRef {
    fn sax_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "escape() requires at least 1 argument",
            ));
        }
        let mut data = args[0].py_to_string();
        // Default entities
        data = data.replace('&', "&amp;");
        data = data.replace('<', "&lt;");
        data = data.replace('>', "&gt;");
        // Additional entities from dict argument
        if args.len() > 1 {
            if let PyObjectPayload::Dict(ref d) = args[1].payload {
                let map = d.read();
                for (k, v) in map.iter() {
                    let key = k.to_object().py_to_string();
                    let val = v.py_to_string();
                    data = data.replace(&key, &val);
                }
            }
        }
        Ok(PyObject::str_val(CompactString::from(data)))
    }

    fn sax_unescape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "unescape() requires at least 1 argument",
            ));
        }
        let mut data = args[0].py_to_string();
        // Additional entities from dict argument (applied first)
        if args.len() > 1 {
            if let PyObjectPayload::Dict(ref d) = args[1].payload {
                let map = d.read();
                for (k, v) in map.iter() {
                    let key = k.to_object().py_to_string();
                    let val = v.py_to_string();
                    data = data.replace(&key, &val);
                }
            }
        }
        // Default entities (reverse of escape)
        data = data.replace("&lt;", "<");
        data = data.replace("&gt;", ">");
        data = data.replace("&amp;", "&");
        Ok(PyObject::str_val(CompactString::from(data)))
    }

    fn sax_quoteattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "quoteattr() requires at least 1 argument",
            ));
        }
        let data = args[0].py_to_string();
        let mut escaped = data.replace('&', "&amp;");
        escaped = escaped.replace('<', "&lt;");
        escaped = escaped.replace('>', "&gt;");
        if escaped.contains('"') {
            if escaped.contains('\'') {
                escaped = escaped.replace('"', "&quot;");
                Ok(PyObject::str_val(CompactString::from(format!(
                    "\"{}\"",
                    escaped
                ))))
            } else {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "'{}'",
                    escaped
                ))))
            }
        } else {
            Ok(PyObject::str_val(CompactString::from(format!(
                "\"{}\"",
                escaped
            ))))
        }
    }

    // XMLGenerator class (stub)
    let xml_gen = PyObject::class(CompactString::from("XMLGenerator"), vec![], IndexMap::new());

    make_module(
        "xml.sax.saxutils",
        vec![
            (
                "escape",
                PyObject::native_function("saxutils.escape", sax_escape),
            ),
            (
                "unescape",
                PyObject::native_function("saxutils.unescape", sax_unescape),
            ),
            (
                "quoteattr",
                PyObject::native_function("saxutils.quoteattr", sax_quoteattr),
            ),
            ("XMLGenerator", xml_gen),
        ],
    )
}

/// xml.sax.xmlreader — XMLReader and AttributesImpl
pub fn create_xml_sax_xmlreader_module() -> PyObjectRef {
    let xml_reader = PyObject::class(CompactString::from("XMLReader"), vec![], IndexMap::new());
    let incremental_parser = PyObject::class(
        CompactString::from("IncrementalParser"),
        vec![],
        IndexMap::new(),
    );
    let locator = PyObject::class(CompactString::from("Locator"), vec![], IndexMap::new());
    let attrs_impl = PyObject::class(
        CompactString::from("AttributesImpl"),
        vec![],
        IndexMap::new(),
    );

    let attrs_ns_impl = PyObject::class(
        CompactString::from("AttributesNSImpl"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = attrs_ns_impl.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("__init__"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 3 {
                    let self_obj = &args[0];
                    if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
                        inst.attrs
                            .write()
                            .insert(CompactString::from("_attrs"), args[1].clone());
                        inst.attrs
                            .write()
                            .insert(CompactString::from("_qnames"), args[2].clone());
                    }
                }
                Ok(PyObject::none())
            }),
        );
        ns.insert(
            CompactString::from("getValueByQName"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Ok(PyObject::none());
                }
                let self_obj = &args[0];
                let qname = args[1].py_to_string();
                if let Some(qnames) = self_obj.get_attr("_qnames") {
                    if let Some(attrs) = self_obj.get_attr("_attrs") {
                        if let PyObjectPayload::Dict(ref qd) = qnames.payload {
                            let qm = qd.read();
                            for (k, v) in qm.iter() {
                                if v.py_to_string() == qname {
                                    let ns_key = k.to_object();
                                    if let PyObjectPayload::Dict(ref ad) = attrs.payload {
                                        let am = ad.read();
                                        if let Some(val) = am.get(
                                            &HashableKey::from_object(&ns_key)
                                                .unwrap_or(HashableKey::None),
                                        ) {
                                            return Ok(val.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Err(PyException::key_error(qname))
            }),
        );
        ns.insert(
            CompactString::from("getNames"),
            make_builtin(|args: &[PyObjectRef]| {
                if let Some(attrs) = args[0].get_attr("_attrs") {
                    if let PyObjectPayload::Dict(ref d) = attrs.payload {
                        let m = d.read();
                        let keys: Vec<PyObjectRef> = m.keys().map(|k| k.to_object()).collect();
                        return Ok(PyObject::list(keys));
                    }
                }
                Ok(PyObject::list(vec![]))
            }),
        );
        ns.insert(
            CompactString::from("getQNames"),
            make_builtin(|args: &[PyObjectRef]| {
                if let Some(qnames) = args[0].get_attr("_qnames") {
                    if let PyObjectPayload::Dict(ref d) = qnames.payload {
                        let m = d.read();
                        let vals: Vec<PyObjectRef> = m.values().cloned().collect();
                        return Ok(PyObject::list(vals));
                    }
                }
                Ok(PyObject::list(vec![]))
            }),
        );
        ns.insert(
            CompactString::from("__len__"),
            make_builtin(|args: &[PyObjectRef]| {
                if let Some(attrs) = args[0].get_attr("_attrs") {
                    if let PyObjectPayload::Dict(ref d) = attrs.payload {
                        return Ok(PyObject::int(d.read().len() as i64));
                    }
                }
                Ok(PyObject::int(0))
            }),
        );
    }

    make_module(
        "xml.sax.xmlreader",
        vec![
            ("XMLReader", xml_reader),
            ("IncrementalParser", incremental_parser),
            ("Locator", locator),
            ("AttributesImpl", attrs_impl),
            ("AttributesNSImpl", attrs_ns_impl),
        ],
    )
}
