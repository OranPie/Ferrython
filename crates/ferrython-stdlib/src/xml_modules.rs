//! XML stdlib modules: xml.etree.ElementTree

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

mod dom;
mod element;
mod expat;
mod packages;
mod parser;
mod sax;

pub use dom::{create_xml_dom_minidom_module, create_xml_dom_module};
use element::{element_to_string, pyobject_to_xml_element, xml_element_to_pyobject};
pub use expat::{create_xml_parsers_expat_module, create_xml_parsers_module};
pub use packages::{create_xml_etree_module, create_xml_module};
use parser::{XmlElement, XmlParser};
pub use sax::{
    create_xml_sax_handler_module, create_xml_sax_module, create_xml_sax_saxutils_module,
    create_xml_sax_xmlreader_module,
};

// ── Module functions ───────────────────────────────────────────────────

fn etree_element(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "Element() requires at least 1 argument: tag",
        ));
    }
    let tag = args[0].py_to_string();
    let mut elem = XmlElement::new(&tag);

    // Extract attrib from either positional arg or kwargs
    let last = args.len().saturating_sub(1);
    let has_kwargs = last > 0 && matches!(&args[last].payload, PyObjectPayload::Dict(_));

    // Check kwargs for attrib={}
    if has_kwargs {
        if let PyObjectPayload::Dict(kw) = &args[last].payload {
            let r = kw.read();
            // attrib={...} kwarg
            if let Some(att) = r.get(&HashableKey::str_key(CompactString::from("attrib"))) {
                if let PyObjectPayload::Dict(am) = &att.payload {
                    let ar = am.read();
                    for (k, v) in ar.iter() {
                        if let HashableKey::Str(ks) = k {
                            elem.attrib.push((ks.to_string(), v.py_to_string()));
                        }
                    }
                }
            }
            // Additional kwargs become attributes (except 'attrib' itself)
            for (k, v) in r.iter() {
                if let HashableKey::Str(ks) = k {
                    let key = ks.to_string();
                    if key != "attrib" {
                        elem.attrib.push((key, v.py_to_string()));
                    }
                }
            }
        }
    } else if args.len() > 1 {
        // Positional attrib dict
        if let PyObjectPayload::Dict(map) = &args[1].payload {
            let r = map.read();
            for (k, v) in r.iter() {
                if let HashableKey::Str(ks) = k {
                    elem.attrib.push((ks.to_string(), v.py_to_string()));
                }
            }
        }
    }

    Ok(xml_element_to_pyobject(&elem))
}

fn etree_subelement(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "SubElement() requires at least 2 arguments: parent, tag",
        ));
    }
    let tag = args[1].py_to_string();
    let mut child_elem = XmlElement::new(&tag);

    // Extract attrib from positional arg or kwargs
    let last = args.len().saturating_sub(1);
    let has_kwargs = last > 1 && matches!(&args[last].payload, PyObjectPayload::Dict(_));

    if has_kwargs {
        if let PyObjectPayload::Dict(kw) = &args[last].payload {
            let r = kw.read();
            if let Some(att) = r.get(&HashableKey::str_key(CompactString::from("attrib"))) {
                if let PyObjectPayload::Dict(am) = &att.payload {
                    let ar = am.read();
                    for (k, v) in ar.iter() {
                        if let HashableKey::Str(ks) = k {
                            child_elem.attrib.push((ks.to_string(), v.py_to_string()));
                        }
                    }
                }
            }
            for (k, v) in r.iter() {
                if let HashableKey::Str(ks) = k {
                    let key = ks.to_string();
                    if key != "attrib" {
                        child_elem.attrib.push((key, v.py_to_string()));
                    }
                }
            }
        }
    } else if args.len() > 2 {
        if let PyObjectPayload::Dict(map) = &args[2].payload {
            let r = map.read();
            for (k, v) in r.iter() {
                if let HashableKey::Str(ks) = k {
                    child_elem.attrib.push((ks.to_string(), v.py_to_string()));
                }
            }
        }
    }

    let child_obj = xml_element_to_pyobject(&child_elem);

    // Append to parent
    if let PyObjectPayload::Instance(ref d) = args[0].payload {
        let r = d.attrs.read();
        if let Some(append_fn) = r.get(&CompactString::from("append")) {
            if let PyObjectPayload::NativeClosure(nc) = &append_fn.payload {
                let _ = (nc.func)(&[child_obj.clone()]);
            }
        }
    }

    Ok(child_obj)
}

fn etree_fromstring(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("fromstring() requires 1 argument"));
    }
    let text = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => {
            return Err(PyException::type_error(
                "fromstring() requires a string argument",
            ))
        }
    };
    let mut parser = XmlParser::new(&text);
    match parser.parse_document() {
        Ok(elem) => Ok(xml_element_to_pyobject(&elem)),
        Err(e) => Err(PyException::value_error(format!("XML parse error: {}", e))),
    }
}

fn etree_tostring(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("tostring() requires 1 argument"));
    }
    // Check for encoding kwarg to determine return type
    let mut encoding_str = String::new();
    if args.len() > 1 {
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(d) = &last.payload {
                let r = d.read();
                if let Some(enc) = r.get(&HashableKey::str_key(CompactString::from("encoding"))) {
                    encoding_str = enc.py_to_string();
                }
            }
        }
    }
    let return_str = encoding_str == "unicode";
    // Reconstruct from PyObject attrs (handles text/tail set via Python assignment)
    let elem = pyobject_to_xml_element(&args[0])?;
    let xml_str = element_to_string(&elem);
    if return_str {
        Ok(PyObject::str_val(CompactString::from(xml_str)))
    } else {
        Ok(PyObject::bytes(xml_str.into_bytes()))
    }
}

fn etree_parse(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "parse() requires 1 argument: source",
        ));
    }
    let path = args[0].py_to_string();
    let content = std::fs::read_to_string(&path).map_err(|e| {
        PyException::new(
            ferrython_core::error::ExceptionKind::FileNotFoundError,
            format!("No such file or directory: '{}'", e),
        )
    })?;
    let mut parser = XmlParser::new(&content);
    match parser.parse_document() {
        Ok(root) => Ok(build_element_tree(root)),
        Err(e) => Err(PyException::value_error(format!("XML parse error: {}", e))),
    }
}

fn build_element_tree(root: XmlElement) -> PyObjectRef {
    let root_obj = xml_element_to_pyobject(&root);

    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__etree_tree__"),
        PyObject::bool_val(true),
    );

    // getroot()
    let ro = root_obj.clone();
    attrs.insert(
        CompactString::from("getroot"),
        PyObject::native_closure("getroot", move |_args| Ok(ro.clone())),
    );

    // find(match) — delegate to root element
    let ro = root_obj.clone();
    attrs.insert(
        CompactString::from("find"),
        PyObject::native_closure("find", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("find() requires 1 argument"));
            }
            if let PyObjectPayload::Instance(ref d) = ro.payload {
                let r = d.attrs.read();
                if let Some(find_fn) = r.get(&CompactString::from("find")) {
                    if let PyObjectPayload::NativeClosure(nc) = &find_fn.payload {
                        return (nc.func)(args);
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );

    // findall(match) — delegate to root element
    let ro = root_obj.clone();
    attrs.insert(
        CompactString::from("findall"),
        PyObject::native_closure("findall", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("findall() requires 1 argument"));
            }
            if let PyObjectPayload::Instance(ref d) = ro.payload {
                let r = d.attrs.read();
                if let Some(fa_fn) = r.get(&CompactString::from("findall")) {
                    if let PyObjectPayload::NativeClosure(nc) = &fa_fn.payload {
                        return (nc.func)(args);
                    }
                }
            }
            Ok(PyObject::list(vec![]))
        }),
    );

    // parse(source) — re-parse from string
    attrs.insert(
        CompactString::from("parse"),
        PyObject::native_closure("parse", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("parse() requires 1 argument"));
            }
            let text = args[0].py_to_string();
            let mut parser = XmlParser::new(&text);
            match parser.parse_document() {
                Ok(root) => Ok(build_element_tree(root)),
                Err(e) => Err(PyException::value_error(format!("XML parse error: {}", e))),
            }
        }),
    );

    let cls = PyObject::class(CompactString::from("ElementTree"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

// ── ElementTree constructor ────────────────────────────────────────────

fn etree_element_tree(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // ElementTree(element=None)
    if args.is_empty() {
        return Ok(build_element_tree(XmlElement::new("root")));
    }
    let elem = pyobject_to_xml_element(&args[0])?;
    Ok(build_element_tree(elem))
}

// ── Public module constructors ─────────────────────────────────────────

pub fn create_xml_etree_elementtree_module() -> PyObjectRef {
    make_module(
        "xml.etree.ElementTree",
        vec![
            ("Element", make_builtin(etree_element)),
            ("SubElement", make_builtin(etree_subelement)),
            ("ElementTree", make_builtin(etree_element_tree)),
            ("fromstring", make_builtin(etree_fromstring)),
            ("tostring", make_builtin(etree_tostring)),
            ("parse", make_builtin(etree_parse)),
            ("XML", make_builtin(etree_fromstring)),
            (
                "Comment",
                make_builtin(|args: &[PyObjectRef]| {
                    let text = if !args.is_empty() {
                        args[0].py_to_string()
                    } else {
                        String::new()
                    };
                    let mut elem = XmlElement::new("!--");
                    elem.text = text;
                    Ok(xml_element_to_pyobject(&elem))
                }),
            ),
            (
                "ProcessingInstruction",
                make_builtin(|args: &[PyObjectRef]| {
                    let target = if !args.is_empty() {
                        args[0].py_to_string()
                    } else {
                        String::new()
                    };
                    let text = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        String::new()
                    };
                    let pi_tag = format!("?{}", target);
                    let mut elem = XmlElement::new(&pi_tag);
                    elem.text = text;
                    Ok(xml_element_to_pyobject(&elem))
                }),
            ),
            // QName(text_or_uri, tag=None) — qualified XML name
            (
                "QName",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "QName() requires at least 1 argument",
                        ));
                    }
                    let text = if args.len() >= 2 {
                        let uri = args[0].py_to_string();
                        let tag = args[1].py_to_string();
                        format!("{{{}}}{}", uri, tag)
                    } else {
                        args[0].py_to_string()
                    };
                    let cls =
                        PyObject::class(CompactString::from("QName"), vec![], IndexMap::new());
                    let mut attrs = IndexMap::new();
                    attrs.insert(
                        CompactString::from("text"),
                        PyObject::str_val(CompactString::from(text.as_str())),
                    );
                    Ok(PyObject::instance_with_attrs(cls, attrs))
                }),
            ),
            // indent(tree, space="  ", level=0) — add whitespace indentation
            (
                "indent",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            ),
            // TreeBuilder class (stub)
            (
                "TreeBuilder",
                make_builtin(|_args: &[PyObjectRef]| {
                    let cls = PyObject::class(
                        CompactString::from("TreeBuilder"),
                        vec![],
                        IndexMap::new(),
                    );
                    Ok(PyObject::instance(cls))
                }),
            ),
            // iterparse(source, events=None) — stub
            (
                "iterparse",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::list(vec![]))),
            ),
            // register_namespace(prefix, uri) — stub
            (
                "register_namespace",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            ),
            // HTML_EMPTY — set of HTML void elements (CPython compat)
            ("HTML_EMPTY", {
                let elems = vec![
                    "area", "base", "basefont", "br", "col", "frame", "hr", "img", "input",
                    "isindex", "link", "meta", "param",
                ];
                let mut set = IndexMap::new();
                for s in elems {
                    let val = PyObject::str_val(CompactString::from(s));
                    set.insert(HashableKey::str_key(CompactString::from(s)), val);
                }
                PyObject::set(set)
            }),
        ],
    )
}
