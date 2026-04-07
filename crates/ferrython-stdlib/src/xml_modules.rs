//! XML stdlib modules: xml.etree.ElementTree

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

// ── XML Parser ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct XmlElement {
    tag: String,
    text: String,
    tail: String,
    attrib: Vec<(String, String)>,
    children: Vec<XmlElement>,
}

impl XmlElement {
    fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
            text: String::new(),
            tail: String::new(),
            attrib: Vec::new(),
            children: Vec::new(),
        }
    }
}

struct XmlParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> XmlParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input: input.as_bytes(), pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn remaining(&self) -> &str {
        std::str::from_utf8(&self.input[self.pos..]).unwrap_or("")
    }

    fn starts_with(&self, s: &str) -> bool {
        self.remaining().starts_with(s)
    }

    fn skip_str(&mut self, s: &str) {
        self.pos += s.len();
    }

    fn read_name(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.' || c == b':' {
                self.advance();
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.input[start..self.pos]).to_string()
    }

    fn read_attr_value(&mut self) -> Result<String, String> {
        let quote = match self.peek() {
            Some(b'"') => b'"',
            Some(b'\'') => b'\'',
            _ => return Err("expected quote for attribute value".to_string()),
        };
        self.advance(); // skip opening quote
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == quote { break; }
            self.advance();
        }
        let val = String::from_utf8_lossy(&self.input[start..self.pos]).to_string();
        self.advance(); // skip closing quote
        Ok(unescape_xml(&val))
    }

    fn read_text_until_lt(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'<' { break; }
            self.advance();
        }
        let raw = String::from_utf8_lossy(&self.input[start..self.pos]).to_string();
        unescape_xml(&raw)
    }

    fn skip_xml_declaration(&mut self) {
        self.skip_ws();
        if self.starts_with("<?xml") {
            while let Some(c) = self.peek() {
                if c == b'>' {
                    self.advance();
                    break;
                }
                self.advance();
            }
        }
    }

    fn skip_comment(&mut self) -> bool {
        if self.starts_with("<!--") {
            while !self.starts_with("-->") && self.pos < self.input.len() {
                self.advance();
            }
            if self.starts_with("-->") {
                self.skip_str("-->");
            }
            true
        } else {
            false
        }
    }

    fn parse_element(&mut self) -> Result<XmlElement, String> {
        self.skip_ws();
        // skip comments
        while self.skip_comment() {
            self.skip_ws();
        }

        if self.peek() != Some(b'<') {
            return Err(format!("expected '<', found {:?} at pos {}", self.peek().map(|c| c as char), self.pos));
        }
        self.advance(); // skip '<'

        let tag = self.read_name();
        if tag.is_empty() {
            return Err("empty tag name".to_string());
        }

        let mut elem = XmlElement::new(&tag);

        // parse attributes
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'>') => { self.advance(); break; }
                Some(b'/') => {
                    self.advance();
                    if self.peek() == Some(b'>') {
                        self.advance();
                        return Ok(elem); // self-closing
                    }
                    return Err("expected '>' after '/'".to_string());
                }
                Some(_) => {
                    let attr_name = self.read_name();
                    if attr_name.is_empty() {
                        return Err("empty attribute name".to_string());
                    }
                    self.skip_ws();
                    if self.peek() == Some(b'=') {
                        self.advance();
                        self.skip_ws();
                        let attr_val = self.read_attr_value()?;
                        elem.attrib.push((attr_name, attr_val));
                    } else {
                        elem.attrib.push((attr_name, String::new()));
                    }
                }
                None => return Err("unexpected end of input in tag".to_string()),
            }
        }

        // parse content (text, children, closing tag)
        let text = self.read_text_until_lt();
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            elem.text = trimmed.to_string();
        }

        loop {
            self.skip_ws();
            while self.skip_comment() {
                self.skip_ws();
            }

            if self.pos >= self.input.len() { break; }

            // check for closing tag
            let closing = format!("</{}", tag);
            if self.starts_with(&closing) {
                self.skip_str(&closing);
                self.skip_ws();
                if self.peek() == Some(b'>') {
                    self.advance();
                }
                break;
            }

            if self.peek() == Some(b'<') && self.input.get(self.pos + 1) == Some(&b'/') {
                // mismatched close tag - skip it
                while let Some(c) = self.peek() {
                    self.advance();
                    if c == b'>' { break; }
                }
                break;
            }

            if self.peek() == Some(b'<') {
                let child = self.parse_element()?;
                // text after child is the child's tail
                let tail_text = self.read_text_until_lt();
                let tail_trimmed = tail_text.trim();
                let mut child = child;
                if !tail_trimmed.is_empty() {
                    child.tail = tail_trimmed.to_string();
                }
                elem.children.push(child);
            } else {
                break;
            }
        }

        Ok(elem)
    }

    fn parse_document(&mut self) -> Result<XmlElement, String> {
        self.skip_xml_declaration();
        self.skip_ws();
        while self.skip_comment() {
            self.skip_ws();
        }
        self.parse_element()
    }
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
     .replace("&lt;", "<")
     .replace("&gt;", ">")
     .replace("&quot;", "\"")
     .replace("&apos;", "'")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&apos;")
}

// ── Convert XmlElement ↔ PyObject ──────────────────────────────────────
//
// Element objects store ALL mutable state in instance attrs + a shared children
// list (Arc<RwLock<Vec<PyObjectRef>>>).  This eliminates the dual-state problem
// where `child.text = "hello"` updated instance attrs but not the inner struct.

use std::sync::RwLock;

type ChildrenList = Arc<RwLock<Vec<PyObjectRef>>>;

/// Convert a parsed XmlElement tree into a live PyObject Element.
fn xml_element_to_pyobject(elem: &XmlElement) -> PyObjectRef {
    // Recursively convert children first
    let child_objs: Vec<PyObjectRef> = elem.children.iter()
        .map(|c| xml_element_to_pyobject(c))
        .collect();
    let children = Arc::new(RwLock::new(child_objs));
    build_element_object(&elem.tag, &elem.text, &elem.tail, &elem.attrib, children)
}

/// Core builder: all Element methods operate on instance attrs + shared children list.
fn build_element_object(
    tag: &str, text: &str, tail: &str,
    attrib: &[(String, String)],
    children: ChildrenList,
) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("__etree_element__"), PyObject::bool_val(true));

    // Scalar attrs
    attrs.insert(CompactString::from("tag"), PyObject::str_val(CompactString::from(tag)));
    attrs.insert(CompactString::from("text"),
        if text.is_empty() { PyObject::none() }
        else { PyObject::str_val(CompactString::from(text)) });
    attrs.insert(CompactString::from("tail"),
        if tail.is_empty() { PyObject::none() }
        else { PyObject::str_val(CompactString::from(tail)) });

    // attrib dict
    let mut attrib_map = IndexMap::new();
    for (k, v) in attrib {
        attrib_map.insert(
            HashableKey::Str(CompactString::from(k.as_str())),
            PyObject::str_val(CompactString::from(v.as_str())),
        );
    }
    attrs.insert(CompactString::from("attrib"), PyObject::dict(attrib_map));

    // ── attrib helpers ────────────────────────────────────────────────
    // get/set/keys/values/items read from the `attrib` dict in instance attrs,
    // but we also keep a local attrib Vec for legacy compat.
    let attrib_inner = Arc::new(Mutex::new(attrib.to_vec()));

    let ai = attrib_inner.clone();
    attrs.insert(CompactString::from("get"), PyObject::native_closure("get", move |args| {
        if args.is_empty() { return Err(PyException::type_error("get() requires at least 1 argument")); }
        let key = args[0].py_to_string();
        let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
        let guard = ai.lock().unwrap();
        for (k, v) in guard.iter() {
            if k == &key { return Ok(PyObject::str_val(CompactString::from(v.as_str()))); }
        }
        Ok(default)
    }));

    let ai = attrib_inner.clone();
    attrs.insert(CompactString::from("set"), PyObject::native_closure("set", move |args| {
        if args.len() < 2 { return Err(PyException::type_error("set() requires 2 arguments")); }
        let key = args[0].py_to_string();
        let val = args[1].py_to_string();
        let mut guard = ai.lock().unwrap();
        for entry in guard.iter_mut() {
            if entry.0 == key { entry.1 = val; return Ok(PyObject::none()); }
        }
        guard.push((key, val));
        Ok(PyObject::none())
    }));

    let ai = attrib_inner.clone();
    attrs.insert(CompactString::from("keys"), PyObject::native_closure("keys", move |_args| {
        let guard = ai.lock().unwrap();
        let items: Vec<PyObjectRef> = guard.iter().map(|(k, _)| PyObject::str_val(CompactString::from(k.as_str()))).collect();
        Ok(PyObject::list(items))
    }));

    let ai = attrib_inner.clone();
    attrs.insert(CompactString::from("values"), PyObject::native_closure("values", move |_args| {
        let guard = ai.lock().unwrap();
        let items: Vec<PyObjectRef> = guard.iter().map(|(_, v)| PyObject::str_val(CompactString::from(v.as_str()))).collect();
        Ok(PyObject::list(items))
    }));

    let ai = attrib_inner.clone();
    attrs.insert(CompactString::from("items"), PyObject::native_closure("items", move |_args| {
        let guard = ai.lock().unwrap();
        let items: Vec<PyObjectRef> = guard.iter()
            .map(|(k, v)| PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(k.as_str())),
                PyObject::str_val(CompactString::from(v.as_str())),
            ])).collect();
        Ok(PyObject::list(items))
    }));

    // ── Child navigation ──────────────────────────────────────────────

    let ch = children.clone();
    attrs.insert(CompactString::from("find"), PyObject::native_closure("find", move |args| {
        if args.is_empty() { return Err(PyException::type_error("find() requires at least 1 argument")); }
        let tag_match = args[0].py_to_string();
        if tag_match.starts_with(".//") {
            let real_tag = &tag_match[3..];
            fn find_desc(children: &[PyObjectRef], tag: &str) -> Option<PyObjectRef> {
                for c in children {
                    let matched = c.get_attr("tag").map(|t| { let s = t.py_to_string(); s == tag || tag == "*" }).unwrap_or(false);
                    if matched { return Some(c.clone()); }
                    if let PyObjectPayload::Instance(ref d) = c.payload {
                        if let Some(fa_fn) = d.attrs.read().get(&CompactString::from("findall")).cloned() {
                            if let PyObjectPayload::NativeClosure { func, .. } = &fa_fn.payload {
                                if let Ok(list_obj) = func(&[PyObject::str_val(CompactString::from("*"))]) {
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
            let guard = ch.read().unwrap();
            if let Some(found) = find_desc(&guard, real_tag) { return Ok(found); }
            return Ok(PyObject::none());
        }
        let guard = ch.read().unwrap();
        for child in guard.iter() {
            if let Some(t) = child.get_attr("tag") {
                if t.py_to_string() == tag_match { return Ok(child.clone()); }
            }
        }
        Ok(PyObject::none())
    }));

    let ch = children.clone();
    attrs.insert(CompactString::from("findall"), PyObject::native_closure("findall", move |args| {
        if args.is_empty() { return Err(PyException::type_error("findall() requires at least 1 argument")); }
        let tag_match = args[0].py_to_string();
        if tag_match.starts_with(".//") {
            let real_tag = &tag_match[3..];
            fn findall_desc(children: &[PyObjectRef], tag: &str, results: &mut Vec<PyObjectRef>) {
                for c in children {
                    let matched = c.get_attr("tag").map(|t| { let s = t.py_to_string(); s == tag || tag == "*" }).unwrap_or(false);
                    if matched { results.push(c.clone()); }
                    if let PyObjectPayload::Instance(ref d) = c.payload {
                        if let Some(fa_fn) = d.attrs.read().get(&CompactString::from("findall")).cloned() {
                            if let PyObjectPayload::NativeClosure { func, .. } = &fa_fn.payload {
                                if let Ok(list_obj) = func(&[PyObject::str_val(CompactString::from("*"))]) {
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
            let guard = ch.read().unwrap();
            findall_desc(&guard, real_tag, &mut results);
            return Ok(PyObject::list(results));
        }
        let guard = ch.read().unwrap();
        let results: Vec<PyObjectRef> = guard.iter()
            .filter(|c| {
                c.get_attr("tag").map(|t| { let s = t.py_to_string(); s == tag_match || tag_match == "*" }).unwrap_or(false)
            })
            .cloned()
            .collect();
        Ok(PyObject::list(results))
    }));

    let ch = children.clone();
    attrs.insert(CompactString::from("findtext"), PyObject::native_closure("findtext", move |args| {
        if args.is_empty() { return Err(PyException::type_error("findtext() requires at least 1 argument")); }
        let tag_match = args[0].py_to_string();
        let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
        let guard = ch.read().unwrap();
        for child in guard.iter() {
            if let Some(t) = child.get_attr("tag") {
                if t.py_to_string() == tag_match {
                    let text = child.get_attr("text")
                        .map(|t| if matches!(t.payload, PyObjectPayload::None) { String::new() } else { t.py_to_string() })
                        .unwrap_or_default();
                    return Ok(PyObject::str_val(CompactString::from(text)));
                }
            }
        }
        Ok(default)
    }));

    // iter() is added post-creation to include self reference

    // ── Mutation ───────────────────────────────────────────────────────

    let ch = children.clone();
    attrs.insert(CompactString::from("append"), PyObject::native_closure("append", move |args| {
        if args.is_empty() { return Err(PyException::type_error("append() requires 1 argument")); }
        ch.write().unwrap().push(args[0].clone());
        Ok(PyObject::none())
    }));

    let ch = children.clone();
    attrs.insert(CompactString::from("remove"), PyObject::native_closure("remove", move |args| {
        if args.is_empty() { return Err(PyException::type_error("remove() requires 1 argument")); }
        let child_tag = extract_element_tag(&args[0]);
        let mut guard = ch.write().unwrap();
        if let Some(idx) = guard.iter().position(|c| extract_element_tag(c) == child_tag) {
            guard.remove(idx);
        }
        Ok(PyObject::none())
    }));

    // ── Dunder methods ────────────────────────────────────────────────

    let ch = children.clone();
    attrs.insert(CompactString::from("__len__"), PyObject::native_closure("__len__", move |_args| {
        Ok(PyObject::int(ch.read().unwrap().len() as i64))
    }));

    attrs.insert(CompactString::from("__repr__"), {
        let tag_str = CompactString::from(tag);
        PyObject::native_closure("__repr__", move |_args| {
            Ok(PyObject::str_val(CompactString::from(format!("<Element '{}' at 0x0>", tag_str))))
        })
    });

    let ch = children.clone();
    attrs.insert(CompactString::from("__iter__"), PyObject::native_closure("__iter__", move |_args| {
        let guard = ch.read().unwrap();
        Ok(PyObject::list(guard.clone()))
    }));

    let ch = children.clone();
    attrs.insert(CompactString::from("__getitem__"), PyObject::native_closure("__getitem__", move |args| {
        if args.is_empty() { return Err(PyException::type_error("__getitem__ requires 1 argument")); }
        let idx = args[0].to_int().map_err(|_| PyException::type_error("index must be an integer"))?;
        let guard = ch.read().unwrap();
        let len = guard.len() as i64;
        let actual = if idx < 0 { len + idx } else { idx };
        if actual < 0 || actual >= len {
            return Err(PyException::new(ferrython_core::error::ExceptionKind::IndexError, "child index out of range"));
        }
        Ok(guard[actual as usize].clone())
    }));

    let cls = PyObject::class(CompactString::from("Element"), vec![], IndexMap::new());
    let element = PyObject::instance_with_attrs(cls, attrs);

    // Post-creation: add iter() that includes self in the iteration
    if let PyObjectPayload::Instance(ref d) = element.payload {
        let elem_ref = element.clone();
        let ch = children.clone();
        d.attrs.write().insert(CompactString::from("iter"), PyObject::native_closure("iter", move |args| {
            let tag_filter = if !args.is_empty() {
                let s = args[0].py_to_string();
                if s == "None" { None } else { Some(s) }
            } else { None };
            let mut results = Vec::new();
            // Include self first (CPython behavior)
            let self_tag = elem_ref.get_attr("tag").map(|t| t.py_to_string()).unwrap_or_default();
            let self_matches = match &tag_filter {
                Some(t) => self_tag == *t,
                None => true,
            };
            if self_matches { results.push(elem_ref.clone()); }
            // Then recurse into children
            let guard = ch.read().unwrap();
            collect_pyobject_elements(&guard, &tag_filter, &mut results);
            Ok(PyObject::list(results))
        }));
    }

    element
}

/// Recursively collect matching PyObject elements for iter().
fn collect_pyobject_elements(children: &[PyObjectRef], tag_filter: &Option<String>, results: &mut Vec<PyObjectRef>) {
    for child in children {
        let tag = child.get_attr("tag").map(|t| t.py_to_string()).unwrap_or_default();
        let matches = match tag_filter {
            Some(t) => tag == *t,
            None => true,
        };
        if matches { results.push(child.clone()); }
        // Recurse into child's children
        if let PyObjectPayload::Instance(ref d) = child.payload {
            let r = d.attrs.read();
            if let Some(iter_fn) = r.get(&CompactString::from("__iter__")) {
                if let PyObjectPayload::NativeClosure { func, .. } = &iter_fn.payload {
                    if let Ok(list_obj) = func(&[]) {
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
fn pyobject_to_xml_element(obj: &PyObjectRef) -> PyResult<XmlElement> {
    if let PyObjectPayload::Instance(ref d) = obj.payload {
        let r = d.attrs.read();
        let tag = r.get(&CompactString::from("tag")).map(|t| t.py_to_string()).unwrap_or_default();
        let text = r.get(&CompactString::from("text")).map(|t| {
            if matches!(t.payload, PyObjectPayload::None) { String::new() } else { t.py_to_string() }
        }).unwrap_or_default();
        let tail = r.get(&CompactString::from("tail")).map(|t| {
            if matches!(t.payload, PyObjectPayload::None) { String::new() } else { t.py_to_string() }
        }).unwrap_or_default();

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
            if let PyObjectPayload::NativeClosure { func, .. } = &iter_fn.payload {
                if let Ok(list_obj) = func(&[]) {
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

        Ok(XmlElement { tag, text, tail, attrib, children })
    } else {
        Err(PyException::type_error("expected an Element object"))
    }
}

fn element_to_string(elem: &XmlElement) -> String {
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

// ── Module functions ───────────────────────────────────────────────────

fn etree_element(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Element() requires at least 1 argument: tag"));
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
            if let Some(att) = r.get(&HashableKey::Str(CompactString::from("attrib"))) {
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
        return Err(PyException::type_error("SubElement() requires at least 2 arguments: parent, tag"));
    }
    let tag = args[1].py_to_string();
    let mut child_elem = XmlElement::new(&tag);

    // Extract attrib from positional arg or kwargs
    let last = args.len().saturating_sub(1);
    let has_kwargs = last > 1 && matches!(&args[last].payload, PyObjectPayload::Dict(_));

    if has_kwargs {
        if let PyObjectPayload::Dict(kw) = &args[last].payload {
            let r = kw.read();
            if let Some(att) = r.get(&HashableKey::Str(CompactString::from("attrib"))) {
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
            if let PyObjectPayload::NativeClosure { func, .. } = &append_fn.payload {
                let _ = func(&[child_obj.clone()]);
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
        _ => return Err(PyException::type_error("fromstring() requires a string argument")),
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
                if let Some(enc) = r.get(&HashableKey::Str(CompactString::from("encoding"))) {
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
        return Err(PyException::type_error("parse() requires 1 argument: source"));
    }
    let path = args[0].py_to_string();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| PyException::new(ferrython_core::error::ExceptionKind::FileNotFoundError,
            format!("No such file or directory: '{}'", e)))?;
    let mut parser = XmlParser::new(&content);
    match parser.parse_document() {
        Ok(root) => Ok(build_element_tree(root)),
        Err(e) => Err(PyException::value_error(format!("XML parse error: {}", e))),
    }
}

fn build_element_tree(root: XmlElement) -> PyObjectRef {
    let root_obj = xml_element_to_pyobject(&root);

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("__etree_tree__"), PyObject::bool_val(true));

    // getroot()
    let ro = root_obj.clone();
    attrs.insert(CompactString::from("getroot"), PyObject::native_closure("getroot", move |_args| {
        Ok(ro.clone())
    }));

    // find(match) — delegate to root element
    let ro = root_obj.clone();
    attrs.insert(CompactString::from("find"), PyObject::native_closure("find", move |args| {
        if args.is_empty() { return Err(PyException::type_error("find() requires 1 argument")); }
        if let PyObjectPayload::Instance(ref d) = ro.payload {
            let r = d.attrs.read();
            if let Some(find_fn) = r.get(&CompactString::from("find")) {
                if let PyObjectPayload::NativeClosure { func, .. } = &find_fn.payload {
                    return func(args);
                }
            }
        }
        Ok(PyObject::none())
    }));

    // findall(match) — delegate to root element
    let ro = root_obj.clone();
    attrs.insert(CompactString::from("findall"), PyObject::native_closure("findall", move |args| {
        if args.is_empty() { return Err(PyException::type_error("findall() requires 1 argument")); }
        if let PyObjectPayload::Instance(ref d) = ro.payload {
            let r = d.attrs.read();
            if let Some(fa_fn) = r.get(&CompactString::from("findall")) {
                if let PyObjectPayload::NativeClosure { func, .. } = &fa_fn.payload {
                    return func(args);
                }
            }
        }
        Ok(PyObject::list(vec![]))
    }));

    // parse(source) — re-parse from string
    attrs.insert(CompactString::from("parse"), PyObject::native_closure("parse", move |args| {
        if args.is_empty() { return Err(PyException::type_error("parse() requires 1 argument")); }
        let text = args[0].py_to_string();
        let mut parser = XmlParser::new(&text);
        match parser.parse_document() {
            Ok(root) => Ok(build_element_tree(root)),
            Err(e) => Err(PyException::value_error(format!("XML parse error: {}", e))),
        }
    }));

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
    make_module("xml.etree.ElementTree", vec![
        ("Element", make_builtin(etree_element)),
        ("SubElement", make_builtin(etree_subelement)),
        ("ElementTree", make_builtin(etree_element_tree)),
        ("fromstring", make_builtin(etree_fromstring)),
        ("tostring", make_builtin(etree_tostring)),
        ("parse", make_builtin(etree_parse)),
        ("XML", make_builtin(etree_fromstring)),
        ("Comment", make_builtin(|args: &[PyObjectRef]| {
            let text = if !args.is_empty() { args[0].py_to_string() } else { String::new() };
            let mut elem = XmlElement::new("!--");
            elem.text = text;
            Ok(xml_element_to_pyobject(&elem))
        })),
        ("ProcessingInstruction", make_builtin(|args: &[PyObjectRef]| {
            let target = if !args.is_empty() { args[0].py_to_string() } else { String::new() };
            let text = if args.len() > 1 { args[1].py_to_string() } else { String::new() };
            let pi_tag = format!("?{}", target);
            let mut elem = XmlElement::new(&pi_tag);
            elem.text = text;
            Ok(xml_element_to_pyobject(&elem))
        })),
    ])
}

pub fn create_xml_module() -> PyObjectRef {
    // xml package — just expose the etree sub-module path
    make_module("xml", vec![
        ("etree", create_xml_etree_module()),
    ])
}

pub fn create_xml_etree_module() -> PyObjectRef {
    make_module("xml.etree", vec![
        ("ElementTree", create_xml_etree_elementtree_module()),
    ])
}

// ── xml.dom module ──

pub fn create_xml_dom_module() -> PyObjectRef {
    make_module("xml.dom", vec![
        ("minidom", create_xml_dom_minidom_module()),
        ("EMPTY_NAMESPACE", PyObject::none()),
        ("XML_NAMESPACE", PyObject::str_val(CompactString::from("http://www.w3.org/XML/1998/namespace"))),
        ("XMLNS_NAMESPACE", PyObject::str_val(CompactString::from("http://www.w3.org/2000/xmlns/"))),
        ("XHTML_NAMESPACE", PyObject::str_val(CompactString::from("http://www.w3.org/1999/xhtml"))),
    ])
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

        w.insert(CompactString::from("nodeName"), PyObject::str_val(CompactString::from("#document")));
        w.insert(CompactString::from("nodeType"), PyObject::int(9));

        // documentElement
        let elem_cls = PyObject::class(CompactString::from("Element"), vec![], IndexMap::new());
        let elem = PyObject::instance(elem_cls);
        if let PyObjectPayload::Instance(ref ed) = elem.payload {
            let mut ew = ed.attrs.write();
            ew.insert(CompactString::from("tagName"), PyObject::str_val(CompactString::from(root_tag)));
            ew.insert(CompactString::from("nodeName"), PyObject::str_val(CompactString::from(root_tag)));
            ew.insert(CompactString::from("nodeType"), PyObject::int(1));
            ew.insert(CompactString::from("childNodes"), PyObject::list(vec![]));
            ew.insert(CompactString::from("attributes"), PyObject::dict(IndexMap::new()));
            ew.insert(CompactString::from("getAttribute"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() > 1 {
                    Ok(PyObject::str_val(CompactString::from("")))
                } else {
                    Ok(PyObject::str_val(CompactString::from("")))
                }
            }));
            ew.insert(CompactString::from("getElementsByTagName"), make_builtin(|_args: &[PyObjectRef]| {
                Ok(PyObject::list(vec![]))
            }));
        }
        w.insert(CompactString::from("documentElement"), elem);

        let xr = xml_ref.clone();
        w.insert(CompactString::from("toxml"), PyObject::native_closure(
            "Document.toxml", move |_args: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(xr.lock().unwrap().as_str())))
            }
        ));

        let xr2 = xml_ref.clone();
        w.insert(CompactString::from("toprettyxml"), PyObject::native_closure(
            "Document.toprettyxml", move |args: &[PyObjectRef]| {
                let indent = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    "\t".to_string()
                };
                let raw = xr2.lock().unwrap().clone();
                // Simple pretty-print: just add indent prefix
                let pretty = format!("<?xml version=\"1.0\" ?>\n{}{}\n", indent, raw);
                Ok(PyObject::str_val(CompactString::from(pretty.as_str())))
            }
        ));

        w.insert(CompactString::from("getElementsByTagName"), make_builtin(|_args: &[PyObjectRef]| {
            Ok(PyObject::list(vec![]))
        }));

        w.insert(CompactString::from("createElement"), make_builtin(|args: &[PyObjectRef]| {
            let tag = if !args.is_empty() { args[0].py_to_string() } else { "div".to_string() };
            let e_cls = PyObject::class(CompactString::from("Element"), vec![], IndexMap::new());
            let e = PyObject::instance(e_cls);
            if let PyObjectPayload::Instance(ref ed) = e.payload {
                let mut ew = ed.attrs.write();
                ew.insert(CompactString::from("tagName"), PyObject::str_val(CompactString::from(tag.as_str())));
                ew.insert(CompactString::from("nodeName"), PyObject::str_val(CompactString::from(tag.as_str())));
                ew.insert(CompactString::from("nodeType"), PyObject::int(1));
                ew.insert(CompactString::from("childNodes"), PyObject::list(vec![]));
            }
            Ok(e)
        }));

        w.insert(CompactString::from("createTextNode"), make_builtin(|args: &[PyObjectRef]| {
            let text = if !args.is_empty() { args[0].py_to_string() } else { String::new() };
            let t_cls = PyObject::class(CompactString::from("Text"), vec![], IndexMap::new());
            let t = PyObject::instance(t_cls);
            if let PyObjectPayload::Instance(ref td) = t.payload {
                let mut tw = td.attrs.write();
                tw.insert(CompactString::from("data"), PyObject::str_val(CompactString::from(text.as_str())));
                tw.insert(CompactString::from("nodeType"), PyObject::int(3));
            }
            Ok(t)
        }));

        w.insert(CompactString::from("unlink"), make_builtin(|_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }));
    }
    doc
}

pub fn create_xml_dom_minidom_module() -> PyObjectRef {
    let parse_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("parse() requires a filename argument"));
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
            return Err(PyException::type_error("parseString() requires a string argument"));
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

    make_module("xml.dom.minidom", vec![
        ("parse", parse_fn),
        ("parseString", parse_string_fn),
        ("Document", PyObject::class(CompactString::from("Document"), vec![], IndexMap::new())),
        ("Element", PyObject::class(CompactString::from("Element"), vec![], IndexMap::new())),
        ("Text", PyObject::class(CompactString::from("Text"), vec![], IndexMap::new())),
        ("Node", PyObject::class(CompactString::from("Node"), vec![], IndexMap::new())),
    ])
}

// ── xml.sax module ──

pub fn create_xml_sax_module() -> PyObjectRef {
    let content_handler_cls = PyObject::class(
        CompactString::from("ContentHandler"), vec![], IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = content_handler_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("startDocument"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        ns.insert(CompactString::from("endDocument"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        ns.insert(CompactString::from("startElement"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        ns.insert(CompactString::from("endElement"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        ns.insert(CompactString::from("characters"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
    }

    let error_handler_cls = PyObject::class(
        CompactString::from("ErrorHandler"), vec![], IndexMap::new(),
    );

    let sax_exception = PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError);

    let make_parser_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("XMLReader"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("setContentHandler"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("setErrorHandler"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("parse"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    make_module("xml.sax", vec![
        ("ContentHandler", content_handler_cls),
        ("ErrorHandler", error_handler_cls),
        ("SAXException", sax_exception.clone()),
        ("SAXParseException", sax_exception),
        ("make_parser", make_parser_fn),
        ("parseString", make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()))),
        ("parse", make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()))),
    ])
}
