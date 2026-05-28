use super::parser::escape_xml;
use super::*;

// ── Convert XmlElement ↔ PyObject ──────────────────────────────────────
//
// Element objects store ALL mutable state in instance attrs + a shared children
// list (Rc<PyCell<Vec<PyObjectRef>>>).  This eliminates the dual-state problem
// where `child.text = "hello"` updated instance attrs but not the inner struct.

type ChildrenList = Rc<PyCell<Vec<PyObjectRef>>>;

/// Convert a parsed XmlElement tree into a live PyObject Element.
pub(super) fn xml_element_to_pyobject(elem: &XmlElement) -> PyObjectRef {
    // Recursively convert children first
    let child_objs: Vec<PyObjectRef> = elem
        .children
        .iter()
        .map(|c| xml_element_to_pyobject(c))
        .collect();
    let children = Rc::new(PyCell::new(child_objs));
    build_element_object(&elem.tag, &elem.text, &elem.tail, &elem.attrib, children)
}

/// Core builder: all Element methods operate on instance attrs + shared children list.
fn build_element_object(
    tag: &str,
    text: &str,
    tail: &str,
    attrib: &[(String, String)],
    children: ChildrenList,
) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__etree_element__"),
        PyObject::bool_val(true),
    );

    // Scalar attrs
    attrs.insert(
        CompactString::from("tag"),
        PyObject::str_val(CompactString::from(tag)),
    );
    attrs.insert(
        CompactString::from("text"),
        if text.is_empty() {
            PyObject::none()
        } else {
            PyObject::str_val(CompactString::from(text))
        },
    );
    attrs.insert(
        CompactString::from("tail"),
        if tail.is_empty() {
            PyObject::none()
        } else {
            PyObject::str_val(CompactString::from(tail))
        },
    );

    // attrib dict
    let mut attrib_map = IndexMap::new();
    for (k, v) in attrib {
        attrib_map.insert(
            HashableKey::str_key(CompactString::from(k.as_str())),
            PyObject::str_val(CompactString::from(v.as_str())),
        );
    }
    attrs.insert(CompactString::from("attrib"), PyObject::dict(attrib_map));

    // ── attrib helpers ────────────────────────────────────────────────
    // get/set/keys/values/items read from the `attrib` dict in instance attrs,
    // but we also keep a local attrib Vec for legacy compat.
    let attrib_inner = Arc::new(Mutex::new(attrib.to_vec()));

    let ai = attrib_inner.clone();
    attrs.insert(
        CompactString::from("get"),
        PyObject::native_closure("get", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "get() requires at least 1 argument",
                ));
            }
            let key = args[0].py_to_string();
            let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
            let guard = ai.lock().unwrap();
            for (k, v) in guard.iter() {
                if k == &key {
                    return Ok(PyObject::str_val(CompactString::from(v.as_str())));
                }
            }
            Ok(default)
        }),
    );

    let ai = attrib_inner.clone();
    attrs.insert(
        CompactString::from("set"),
        PyObject::native_closure("set", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("set() requires 2 arguments"));
            }
            let key = args[0].py_to_string();
            let val = args[1].py_to_string();
            let mut guard = ai.lock().unwrap();
            for entry in guard.iter_mut() {
                if entry.0 == key {
                    entry.1 = val;
                    return Ok(PyObject::none());
                }
            }
            guard.push((key, val));
            Ok(PyObject::none())
        }),
    );

    let ai = attrib_inner.clone();
    attrs.insert(
        CompactString::from("keys"),
        PyObject::native_closure("keys", move |_args| {
            let guard = ai.lock().unwrap();
            let items: Vec<PyObjectRef> = guard
                .iter()
                .map(|(k, _)| PyObject::str_val(CompactString::from(k.as_str())))
                .collect();
            Ok(PyObject::list(items))
        }),
    );

    let ai = attrib_inner.clone();
    attrs.insert(
        CompactString::from("values"),
        PyObject::native_closure("values", move |_args| {
            let guard = ai.lock().unwrap();
            let items: Vec<PyObjectRef> = guard
                .iter()
                .map(|(_, v)| PyObject::str_val(CompactString::from(v.as_str())))
                .collect();
            Ok(PyObject::list(items))
        }),
    );

    let ai = attrib_inner.clone();
    attrs.insert(
        CompactString::from("items"),
        PyObject::native_closure("items", move |_args| {
            let guard = ai.lock().unwrap();
            let items: Vec<PyObjectRef> = guard
                .iter()
                .map(|(k, v)| {
                    PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(k.as_str())),
                        PyObject::str_val(CompactString::from(v.as_str())),
                    ])
                })
                .collect();
            Ok(PyObject::list(items))
        }),
    );

    // ── Child navigation ──────────────────────────────────────────────

    let ch = children.clone();
    attrs.insert(
        CompactString::from("find"),
        PyObject::native_closure("find", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "find() requires at least 1 argument",
                ));
            }
            let tag_match = args[0].py_to_string();
            if tag_match.starts_with(".//") {
                let real_tag = &tag_match[3..];
                fn find_desc(children: &[PyObjectRef], tag: &str) -> Option<PyObjectRef> {
                    for c in children {
                        let matched = c
                            .get_attr("tag")
                            .map(|t| {
                                let s = t.py_to_string();
                                s == tag || tag == "*"
                            })
                            .unwrap_or(false);
                        if matched {
                            return Some(c.clone());
                        }
                        if let PyObjectPayload::Instance(ref d) = c.payload {
                            if let Some(fa_fn) =
                                d.attrs.read().get(&CompactString::from("findall")).cloned()
                            {
                                if let PyObjectPayload::NativeClosure(nc) = &fa_fn.payload {
                                    if let Ok(list_obj) =
                                        (nc.func)(&[PyObject::str_val(CompactString::from("*"))])
                                    {
                                        if let PyObjectPayload::List(items) = &list_obj.payload {
                                            if let Some(found) = find_desc(&items.read(), tag) {
                                                return Some(found);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None
                }
                let guard = ch.read();
                if let Some(found) = find_desc(&guard, real_tag) {
                    return Ok(found);
                }
                return Ok(PyObject::none());
            }
            // Handle path expressions like "book/title"
            if tag_match.contains('/') {
                let parts: Vec<&str> = tag_match.splitn(2, '/').collect();
                let first = parts[0];
                let rest = parts[1];
                let guard = ch.read();
                for child in guard.iter() {
                    if let Some(t) = child.get_attr("tag") {
                        if t.py_to_string() == first {
                            if let Some(find_fn) = child.get_attr("find") {
                                if let PyObjectPayload::NativeClosure(nc) = &find_fn.payload {
                                    if let Ok(result) =
                                        (nc.func)(&[PyObject::str_val(CompactString::from(rest))])
                                    {
                                        if !matches!(result.payload, PyObjectPayload::None) {
                                            return Ok(result);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                return Ok(PyObject::none());
            }
            let guard = ch.read();
            for child in guard.iter() {
                if let Some(t) = child.get_attr("tag") {
                    if t.py_to_string() == tag_match {
                        return Ok(child.clone());
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );

    let ch = children.clone();
    attrs.insert(
        CompactString::from("findall"),
        PyObject::native_closure("findall", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "findall() requires at least 1 argument",
                ));
            }
            let tag_match = args[0].py_to_string();
            if tag_match.starts_with(".//") {
                let real_tag = &tag_match[3..];
                fn findall_desc(
                    children: &[PyObjectRef],
                    tag: &str,
                    results: &mut Vec<PyObjectRef>,
                ) {
                    for c in children {
                        let matched = c
                            .get_attr("tag")
                            .map(|t| {
                                let s = t.py_to_string();
                                s == tag || tag == "*"
                            })
                            .unwrap_or(false);
                        if matched {
                            results.push(c.clone());
                        }
                        if let PyObjectPayload::Instance(ref d) = c.payload {
                            if let Some(fa_fn) =
                                d.attrs.read().get(&CompactString::from("findall")).cloned()
                            {
                                if let PyObjectPayload::NativeClosure(nc) = &fa_fn.payload {
                                    if let Ok(list_obj) =
                                        (nc.func)(&[PyObject::str_val(CompactString::from("*"))])
                                    {
                                        if let PyObjectPayload::List(items) = &list_obj.payload {
                                            findall_desc(&items.read(), tag, results);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let mut results = Vec::new();
                let guard = ch.read();
                findall_desc(&guard, real_tag, &mut results);
                return Ok(PyObject::list(results));
            }
            let guard = ch.read();
            let results: Vec<PyObjectRef> = guard
                .iter()
                .filter(|c| {
                    c.get_attr("tag")
                        .map(|t| {
                            let s = t.py_to_string();
                            s == tag_match || tag_match == "*"
                        })
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            Ok(PyObject::list(results))
        }),
    );

    let ch = children.clone();
    attrs.insert(
        CompactString::from("findtext"),
        PyObject::native_closure("findtext", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "findtext() requires at least 1 argument",
                ));
            }
            let tag_match = args[0].py_to_string();
            let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
            let guard = ch.read();
            for child in guard.iter() {
                if let Some(t) = child.get_attr("tag") {
                    if t.py_to_string() == tag_match {
                        let text = child
                            .get_attr("text")
                            .map(|t| {
                                if matches!(t.payload, PyObjectPayload::None) {
                                    String::new()
                                } else {
                                    t.py_to_string()
                                }
                            })
                            .unwrap_or_default();
                        return Ok(PyObject::str_val(CompactString::from(text)));
                    }
                }
            }
            Ok(default)
        }),
    );

    // iter() is added post-creation to include self reference

    // ── Mutation ───────────────────────────────────────────────────────

    let ch = children.clone();
    attrs.insert(
        CompactString::from("append"),
        PyObject::native_closure("append", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("append() requires 1 argument"));
            }
            ch.write().push(args[0].clone());
            Ok(PyObject::none())
        }),
    );

    let ch = children.clone();
    attrs.insert(
        CompactString::from("remove"),
        PyObject::native_closure("remove", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("remove() requires 1 argument"));
            }
            let child_tag = extract_element_tag(&args[0]);
            let mut guard = ch.write();
            if let Some(idx) = guard
                .iter()
                .position(|c| extract_element_tag(c) == child_tag)
            {
                guard.remove(idx);
            }
            Ok(PyObject::none())
        }),
    );

    // ── Dunder methods ────────────────────────────────────────────────

    let ch = children.clone();
    attrs.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("__len__", move |_args| {
            Ok(PyObject::int(ch.read().len() as i64))
        }),
    );

    attrs.insert(CompactString::from("__repr__"), {
        let tag_str = CompactString::from(tag);
        PyObject::native_closure("__repr__", move |_args| {
            Ok(PyObject::str_val(CompactString::from(format!(
                "<Element '{}' at 0x0>",
                tag_str
            ))))
        })
    });

    let ch = children.clone();
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("__iter__", move |_args| {
            let guard = ch.read();
            Ok(PyObject::list(guard.clone()))
        }),
    );

    let ch = children.clone();
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("__getitem__", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("__getitem__ requires 1 argument"));
            }
            let idx = args[0]
                .to_int()
                .map_err(|_| PyException::type_error("index must be an integer"))?;
            let guard = ch.read();
            let len = guard.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::new(
                    ferrython_core::error::ExceptionKind::IndexError,
                    "child index out of range",
                ));
            }
            Ok(guard[actual as usize].clone())
        }),
    );

    let cls = PyObject::class(CompactString::from("Element"), vec![], IndexMap::new());
    let element = PyObject::instance_with_attrs(cls, attrs);

    // Post-creation: add iter() that includes self in the iteration
    if let PyObjectPayload::Instance(ref d) = element.payload {
        let elem_ref = element.clone();
        let ch = children.clone();
        d.attrs.write().insert(
            CompactString::from("iter"),
            PyObject::native_closure("iter", move |args| {
                let tag_filter = if !args.is_empty() {
                    let s = args[0].py_to_string();
                    if s == "None" {
                        None
                    } else {
                        Some(s)
                    }
                } else {
                    None
                };
                let mut results = Vec::new();
                // Include self first (CPython behavior)
                let self_tag = elem_ref
                    .get_attr("tag")
                    .map(|t| t.py_to_string())
                    .unwrap_or_default();
                let self_matches = match &tag_filter {
                    Some(t) => self_tag == *t,
                    None => true,
                };
                if self_matches {
                    results.push(elem_ref.clone());
                }
                // Then recurse into children
                let guard = ch.read();
                collect_pyobject_elements(&guard, &tag_filter, &mut results);
                Ok(PyObject::list(results))
            }),
        );
    }

    element
}

/// Recursively collect matching PyObject elements for iter().
fn collect_pyobject_elements(
    children: &[PyObjectRef],
    tag_filter: &Option<String>,
    results: &mut Vec<PyObjectRef>,
) {
    for child in children {
        let tag = child
            .get_attr("tag")
            .map(|t| t.py_to_string())
            .unwrap_or_default();
        let matches = match tag_filter {
            Some(t) => tag == *t,
            None => true,
        };
        if matches {
            results.push(child.clone());
        }
        // Recurse into child's children
        if let PyObjectPayload::Instance(ref d) = child.payload {
            let r = d.attrs.read();
            if let Some(iter_fn) = r.get(&CompactString::from("__iter__")) {
                if let PyObjectPayload::NativeClosure(nc) = &iter_fn.payload {
                    if let Ok(list_obj) = (nc.func)(&[]) {
                        if let PyObjectPayload::List(items) = &list_obj.payload {
                            let items = items.read();
                            collect_pyobject_elements(&items, tag_filter, results);
                        }
                    }
                }
            }
        }
    }
}

fn extract_element_tag(obj: &PyObjectRef) -> String {
    if let PyObjectPayload::Instance(ref d) = obj.payload {
        let r = d.attrs.read();
        if let Some(tag) = r.get(&CompactString::from("tag")) {
            return tag.py_to_string();
        }
    }
    String::new()
}

/// Reconstruct an XmlElement tree from a live PyObject Element (reads instance attrs).
pub(super) fn pyobject_to_xml_element(obj: &PyObjectRef) -> PyResult<XmlElement> {
    if let PyObjectPayload::Instance(ref d) = obj.payload {
        let r = d.attrs.read();
        let tag = r
            .get(&CompactString::from("tag"))
            .map(|t| t.py_to_string())
            .unwrap_or_default();
        let text = r
            .get(&CompactString::from("text"))
            .map(|t| {
                if matches!(t.payload, PyObjectPayload::None) {
                    String::new()
                } else {
                    t.py_to_string()
                }
            })
            .unwrap_or_default();
        let tail = r
            .get(&CompactString::from("tail"))
            .map(|t| {
                if matches!(t.payload, PyObjectPayload::None) {
                    String::new()
                } else {
                    t.py_to_string()
                }
            })
            .unwrap_or_default();

        let mut attrib = Vec::new();
        if let Some(attr_obj) = r.get(&CompactString::from("attrib")) {
            if let PyObjectPayload::Dict(map) = &attr_obj.payload {
                let mr = map.read();
                for (k, v) in mr.iter() {
                    if let HashableKey::Str(ks) = k {
                        attrib.push((ks.to_string(), v.py_to_string()));
                    }
                }
            }
        }

        // Get children via __iter__ (returns the shared children list)
        let mut children = Vec::new();
        if let Some(iter_fn) = r.get(&CompactString::from("__iter__")) {
            if let PyObjectPayload::NativeClosure(nc) = &iter_fn.payload {
                if let Ok(list_obj) = (nc.func)(&[]) {
                    if let PyObjectPayload::List(items) = &list_obj.payload {
                        for child_obj in items.read().iter() {
                            if let Ok(child_elem) = pyobject_to_xml_element(child_obj) {
                                children.push(child_elem);
                            }
                        }
                    }
                }
            }
        }

        Ok(XmlElement {
            tag,
            text,
            tail,
            attrib,
            children,
        })
    } else {
        Err(PyException::type_error("expected an Element object"))
    }
}

pub(super) fn element_to_string(elem: &XmlElement) -> String {
    let mut s = String::new();
    s.push('<');
    s.push_str(&elem.tag);
    for (k, v) in &elem.attrib {
        s.push(' ');
        s.push_str(k);
        s.push_str("=\"");
        s.push_str(&escape_xml(v));
        s.push('"');
    }
    if elem.children.is_empty() && elem.text.is_empty() {
        s.push_str(" />");
    } else {
        s.push('>');
        if !elem.text.is_empty() {
            s.push_str(&escape_xml(&elem.text));
        }
        for child in &elem.children {
            s.push_str(&element_to_string(child));
            if !child.tail.is_empty() {
                s.push_str(&escape_xml(&child.tail));
            }
        }
        s.push_str("</");
        s.push_str(&elem.tag);
        s.push('>');
    }
    s
}
