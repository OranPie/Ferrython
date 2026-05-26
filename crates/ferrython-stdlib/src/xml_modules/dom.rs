//! xml.dom and xml.dom.minidom modules.

use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

// ── xml.dom module ──

pub fn create_xml_dom_module() -> PyObjectRef {
    make_module(
        "xml.dom",
        vec![
            ("minidom", create_xml_dom_minidom_module()),
            ("EMPTY_NAMESPACE", PyObject::none()),
            (
                "XML_NAMESPACE",
                PyObject::str_val(CompactString::from("http://www.w3.org/XML/1998/namespace")),
            ),
            (
                "XMLNS_NAMESPACE",
                PyObject::str_val(CompactString::from("http://www.w3.org/2000/xmlns/")),
            ),
            (
                "XHTML_NAMESPACE",
                PyObject::str_val(CompactString::from("http://www.w3.org/1999/xhtml")),
            ),
        ],
    )
}

// ── xml.dom.minidom module ──

fn minidom_make_document(root_tag: &str, text_content: &str) -> PyObjectRef {
    let doc_cls = PyObject::class(CompactString::from("Document"), vec![], IndexMap::new());
    let doc = PyObject::instance(doc_cls);
    if let PyObjectPayload::Instance(ref d) = doc.payload {
        let mut w = d.attrs.write();
        let xml_str = if text_content.starts_with("<?xml") || text_content.starts_with('<') {
            text_content.to_string()
        } else {
            format!("<{0}>{1}</{0}>", root_tag, text_content)
        };
        let xml_ref = Arc::new(Mutex::new(xml_str.clone()));

        w.insert(
            CompactString::from("nodeName"),
            PyObject::str_val(CompactString::from("#document")),
        );
        w.insert(CompactString::from("nodeType"), PyObject::int(9));

        // documentElement
        let elem_cls = PyObject::class(CompactString::from("Element"), vec![], IndexMap::new());
        let elem = PyObject::instance(elem_cls);
        if let PyObjectPayload::Instance(ref ed) = elem.payload {
            let mut ew = ed.attrs.write();
            ew.insert(
                CompactString::from("tagName"),
                PyObject::str_val(CompactString::from(root_tag)),
            );
            ew.insert(
                CompactString::from("nodeName"),
                PyObject::str_val(CompactString::from(root_tag)),
            );
            ew.insert(CompactString::from("nodeType"), PyObject::int(1));
            ew.insert(CompactString::from("childNodes"), PyObject::list(vec![]));
            ew.insert(
                CompactString::from("attributes"),
                PyObject::dict(IndexMap::new()),
            );
            ew.insert(
                CompactString::from("getAttribute"),
                make_builtin(|args: &[PyObjectRef]| {
                    if args.len() > 1 {
                        Ok(PyObject::str_val(CompactString::from("")))
                    } else {
                        Ok(PyObject::str_val(CompactString::from("")))
                    }
                }),
            );
            ew.insert(
                CompactString::from("getElementsByTagName"),
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::list(vec![]))),
            );
        }
        w.insert(CompactString::from("documentElement"), elem);

        let xr = xml_ref.clone();
        w.insert(
            CompactString::from("toxml"),
            PyObject::native_closure("Document.toxml", move |_args: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(
                    xr.lock().unwrap().as_str(),
                )))
            }),
        );

        let xr2 = xml_ref.clone();
        w.insert(
            CompactString::from("toprettyxml"),
            PyObject::native_closure("Document.toprettyxml", move |args: &[PyObjectRef]| {
                let indent = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    "\t".to_string()
                };
                let raw = xr2.lock().unwrap().clone();
                // Simple pretty-print: just add indent prefix
                let pretty = format!("<?xml version=\"1.0\" ?>\n{}{}\n", indent, raw);
                Ok(PyObject::str_val(CompactString::from(pretty.as_str())))
            }),
        );

        w.insert(
            CompactString::from("getElementsByTagName"),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::list(vec![]))),
        );

        w.insert(
            CompactString::from("createElement"),
            make_builtin(|args: &[PyObjectRef]| {
                let tag = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    "div".to_string()
                };
                let e_cls =
                    PyObject::class(CompactString::from("Element"), vec![], IndexMap::new());
                let e = PyObject::instance(e_cls);
                if let PyObjectPayload::Instance(ref ed) = e.payload {
                    let mut ew = ed.attrs.write();
                    ew.insert(
                        CompactString::from("tagName"),
                        PyObject::str_val(CompactString::from(tag.as_str())),
                    );
                    ew.insert(
                        CompactString::from("nodeName"),
                        PyObject::str_val(CompactString::from(tag.as_str())),
                    );
                    ew.insert(CompactString::from("nodeType"), PyObject::int(1));
                    ew.insert(CompactString::from("childNodes"), PyObject::list(vec![]));
                }
                Ok(e)
            }),
        );

        w.insert(
            CompactString::from("createTextNode"),
            make_builtin(|args: &[PyObjectRef]| {
                let text = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    String::new()
                };
                let t_cls = PyObject::class(CompactString::from("Text"), vec![], IndexMap::new());
                let t = PyObject::instance(t_cls);
                if let PyObjectPayload::Instance(ref td) = t.payload {
                    let mut tw = td.attrs.write();
                    tw.insert(
                        CompactString::from("data"),
                        PyObject::str_val(CompactString::from(text.as_str())),
                    );
                    tw.insert(CompactString::from("nodeType"), PyObject::int(3));
                }
                Ok(t)
            }),
        );

        w.insert(
            CompactString::from("unlink"),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
        );
    }
    doc
}

pub fn create_xml_dom_minidom_module() -> PyObjectRef {
    let parse_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "parse() requires a filename argument",
            ));
        }
        let filename = args[0].py_to_string();
        match std::fs::read_to_string(&filename) {
            Ok(content) => Ok(minidom_make_document("root", &content)),
            Err(_e) => Err(PyException::new(
                ferrython_core::error::ExceptionKind::FileNotFoundError,
                format!("No such file: '{}'", filename),
            )),
        }
    });

    let parse_string_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "parseString() requires a string argument",
            ));
        }
        let xml_str = args[0].py_to_string();
        // Try to extract root tag name from the XML
        let root_tag = if let Some(start) = xml_str.find('<') {
            let after = &xml_str[start + 1..];
            if let Some(end) = after.find(|c: char| c == ' ' || c == '>' || c == '/') {
                let tag = &after[..end];
                if tag.starts_with('?') || tag.starts_with('!') {
                    "root"
                } else {
                    tag
                }
            } else {
                "root"
            }
        } else {
            "root"
        };
        Ok(minidom_make_document(root_tag, &xml_str))
    });

    make_module(
        "xml.dom.minidom",
        vec![
            ("parse", parse_fn),
            ("parseString", parse_string_fn),
            (
                "Document",
                PyObject::class(CompactString::from("Document"), vec![], IndexMap::new()),
            ),
            (
                "Element",
                PyObject::class(CompactString::from("Element"), vec![], IndexMap::new()),
            ),
            (
                "Text",
                PyObject::class(CompactString::from("Text"), vec![], IndexMap::new()),
            ),
            (
                "Node",
                PyObject::class(CompactString::from("Node"), vec![], IndexMap::new()),
            ),
        ],
    )
}
