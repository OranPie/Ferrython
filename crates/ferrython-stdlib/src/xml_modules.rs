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

fn xml_element_to_pyobject(elem: &XmlElement) -> PyObjectRef {
    let inner = Arc::new(Mutex::new(elem.clone()));
    build_element_object(inner)
}

fn build_element_object(inner: Arc<Mutex<XmlElement>>) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("__etree_element__"), PyObject::bool_val(true));

    // tag property
    {
        let guard = inner.lock().unwrap();
        attrs.insert(CompactString::from("tag"), PyObject::str_val(CompactString::from(&guard.tag)));
        attrs.insert(CompactString::from("text"),
            if guard.text.is_empty() { PyObject::none() }
            else { PyObject::str_val(CompactString::from(&guard.text)) });
        attrs.insert(CompactString::from("tail"),
            if guard.tail.is_empty() { PyObject::none() }
            else { PyObject::str_val(CompactString::from(&guard.tail)) });

        // attrib as dict
        let mut attrib_map = IndexMap::new();
        for (k, v) in &guard.attrib {
            attrib_map.insert(
                HashableKey::Str(CompactString::from(k.as_str())),
                PyObject::str_val(CompactString::from(v.as_str())),
            );
        }
        attrs.insert(CompactString::from("attrib"), PyObject::dict(attrib_map));
    }

    // get(key, default=None)
    let st = inner.clone();
    attrs.insert(CompactString::from("get"), PyObject::native_closure("get", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("get() requires at least 1 argument"));
        }
        let key = args[0].py_to_string();
        let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
        let guard = st.lock().unwrap();
        for (k, v) in &guard.attrib {
            if k == &key {
                return Ok(PyObject::str_val(CompactString::from(v.as_str())));
            }
        }
        Ok(default)
    }));

    // set(key, value)
    let st = inner.clone();
    attrs.insert(CompactString::from("set"), PyObject::native_closure("set", move |args| {
        if args.len() < 2 {
            return Err(PyException::type_error("set() requires 2 arguments"));
        }
        let key = args[0].py_to_string();
        let val = args[1].py_to_string();
        let mut guard = st.lock().unwrap();
        for entry in &mut guard.attrib {
            if entry.0 == key {
                entry.1 = val;
                return Ok(PyObject::none());
            }
        }
        guard.attrib.push((key, val));
        Ok(PyObject::none())
    }));

    // keys()
    let st = inner.clone();
    attrs.insert(CompactString::from("keys"), PyObject::native_closure("keys", move |_args| {
        let guard = st.lock().unwrap();
        let items: Vec<PyObjectRef> = guard.attrib.iter()
            .map(|(k, _)| PyObject::str_val(CompactString::from(k.as_str())))
            .collect();
        Ok(PyObject::list(items))
    }));

    // values()
    let st = inner.clone();
    attrs.insert(CompactString::from("values"), PyObject::native_closure("values", move |_args| {
        let guard = st.lock().unwrap();
        let items: Vec<PyObjectRef> = guard.attrib.iter()
            .map(|(_, v)| PyObject::str_val(CompactString::from(v.as_str())))
            .collect();
        Ok(PyObject::list(items))
    }));

    // items()
    let st = inner.clone();
    attrs.insert(CompactString::from("items"), PyObject::native_closure("items", move |_args| {
        let guard = st.lock().unwrap();
        let items: Vec<PyObjectRef> = guard.attrib.iter()
            .map(|(k, v)| PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(k.as_str())),
                PyObject::str_val(CompactString::from(v.as_str())),
            ]))
            .collect();
        Ok(PyObject::list(items))
    }));

    // find(match)
    let st = inner.clone();
    attrs.insert(CompactString::from("find"), PyObject::native_closure("find", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("find() requires at least 1 argument"));
        }
        let tag_match = args[0].py_to_string();
        let guard = st.lock().unwrap();
        for child in &guard.children {
            if child.tag == tag_match {
                return Ok(xml_element_to_pyobject(child));
            }
        }
        Ok(PyObject::none())
    }));

    // findall(match)
    let st = inner.clone();
    attrs.insert(CompactString::from("findall"), PyObject::native_closure("findall", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("findall() requires at least 1 argument"));
        }
        let tag_match = args[0].py_to_string();
        let guard = st.lock().unwrap();
        let mut results = Vec::new();
        for child in &guard.children {
            if child.tag == tag_match || tag_match == "*" {
                results.push(xml_element_to_pyobject(child));
            }
        }
        Ok(PyObject::list(results))
    }));

    // findtext(match, default=None)
    let st = inner.clone();
    attrs.insert(CompactString::from("findtext"), PyObject::native_closure("findtext", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("findtext() requires at least 1 argument"));
        }
        let tag_match = args[0].py_to_string();
        let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
        let guard = st.lock().unwrap();
        for child in &guard.children {
            if child.tag == tag_match {
                return if child.text.is_empty() {
                    Ok(PyObject::str_val(CompactString::from("")))
                } else {
                    Ok(PyObject::str_val(CompactString::from(&child.text)))
                };
            }
        }
        Ok(default)
    }));

    // iter(tag=None)
    let st = inner.clone();
    attrs.insert(CompactString::from("iter"), PyObject::native_closure("iter", move |args| {
        let tag_filter = if !args.is_empty() {
            let s = args[0].py_to_string();
            if s == "None" { None } else { Some(s) }
        } else {
            None
        };
        let guard = st.lock().unwrap();
        let mut results = Vec::new();
        collect_elements_recursive(&guard, &tag_filter, &mut results);
        Ok(PyObject::list(results))
    }));

    // append(subelement)
    let st = inner.clone();
    attrs.insert(CompactString::from("append"), PyObject::native_closure("append", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("append() requires 1 argument"));
        }
        let child_elem = pyobject_to_xml_element(&args[0])?;
        let mut guard = st.lock().unwrap();
        guard.children.push(child_elem);
        Ok(PyObject::none())
    }));

    // remove(subelement)
    let st = inner.clone();
    attrs.insert(CompactString::from("remove"), PyObject::native_closure("remove", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("remove() requires 1 argument"));
        }
        let child_tag = extract_element_tag(&args[0]);
        let mut guard = st.lock().unwrap();
        if let Some(idx) = guard.children.iter().position(|c| c.tag == child_tag) {
            guard.children.remove(idx);
        }
        Ok(PyObject::none())
    }));

    // __len__
    let st = inner.clone();
    attrs.insert(CompactString::from("__len__"), PyObject::native_closure("__len__", move |_args| {
        let guard = st.lock().unwrap();
        Ok(PyObject::int(guard.children.len() as i64))
    }));

    // __repr__
    let st = inner.clone();
    attrs.insert(CompactString::from("__repr__"), PyObject::native_closure("__repr__", move |_args| {
        let guard = st.lock().unwrap();
        Ok(PyObject::str_val(CompactString::from(
            format!("<Element '{}' at 0x{:x}>", guard.tag, 0)
        )))
    }));

    // __iter__ — iterate over children
    let st = inner.clone();
    attrs.insert(CompactString::from("__iter__"), PyObject::native_closure("__iter__", move |_args| {
        let guard = st.lock().unwrap();
        let children: Vec<PyObjectRef> = guard.children.iter()
            .map(|c| xml_element_to_pyobject(c))
            .collect();
        Ok(PyObject::list(children))
    }));

    // __getitem__ — index children
    let st = inner.clone();
    attrs.insert(CompactString::from("__getitem__"), PyObject::native_closure("__getitem__", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("__getitem__ requires 1 argument"));
        }
        let idx = args[0].to_int().map_err(|_| PyException::type_error("index must be an integer"))?;
        let guard = st.lock().unwrap();
        let len = guard.children.len() as i64;
        let actual = if idx < 0 { len + idx } else { idx };
        if actual < 0 || actual >= len {
            return Err(PyException::new(ferrython_core::error::ExceptionKind::IndexError, "child index out of range"));
        }
        Ok(xml_element_to_pyobject(&guard.children[actual as usize]))
    }));

    let cls = PyObject::class(CompactString::from("Element"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

fn collect_elements_recursive(elem: &XmlElement, tag_filter: &Option<String>, results: &mut Vec<PyObjectRef>) {
    let matches = match tag_filter {
        Some(t) => elem.tag == *t,
        None => true,
    };
    if matches {
        results.push(xml_element_to_pyobject(elem));
    }
    for child in &elem.children {
        collect_elements_recursive(child, tag_filter, results);
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

fn pyobject_to_xml_element(obj: &PyObjectRef) -> PyResult<XmlElement> {
    if let PyObjectPayload::Instance(ref d) = obj.payload {
        let r = d.attrs.read();
        let tag = r.get(&CompactString::from("tag"))
            .map(|t| t.py_to_string())
            .unwrap_or_default();
        let text = r.get(&CompactString::from("text"))
            .map(|t| {
                if matches!(t.payload, PyObjectPayload::None) { String::new() }
                else { t.py_to_string() }
            })
            .unwrap_or_default();
        let tail = r.get(&CompactString::from("tail"))
            .map(|t| {
                if matches!(t.payload, PyObjectPayload::None) { String::new() }
                else { t.py_to_string() }
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

        Ok(XmlElement { tag, text, tail, attrib, children: Vec::new() })
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

    // attrib dict
    if args.len() > 1 {
        if let PyObjectPayload::Dict(map) = &args[1].payload {
            let r = map.read();
            for (k, v) in r.iter() {
                if let HashableKey::Str(ks) = k {
                    elem.attrib.push((ks.to_string(), v.py_to_string()));
                }
            }
        }
    }

    // kwargs
    if args.len() > 2 {
        if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
            let r = map.read();
            for (k, v) in r.iter() {
                if let HashableKey::Str(ks) = k {
                    let key = ks.to_string();
                    if key != "attrib" {
                        elem.attrib.push((key, v.py_to_string()));
                    }
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

    if args.len() > 2 {
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
    let elem = pyobject_to_xml_element(&args[0])?;
    let s = element_to_string(&elem);
    Ok(PyObject::str_val(CompactString::from(s)))
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
    let root_inner = Arc::new(Mutex::new(root));

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("__etree_tree__"), PyObject::bool_val(true));

    // getroot()
    let st = root_inner.clone();
    attrs.insert(CompactString::from("getroot"), PyObject::native_closure("getroot", move |_args| {
        let guard = st.lock().unwrap();
        Ok(xml_element_to_pyobject(&guard))
    }));

    // find(match)
    let st = root_inner.clone();
    attrs.insert(CompactString::from("find"), PyObject::native_closure("find", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("find() requires 1 argument"));
        }
        let tag_match = args[0].py_to_string();
        let guard = st.lock().unwrap();
        for child in &guard.children {
            if child.tag == tag_match {
                return Ok(xml_element_to_pyobject(child));
            }
        }
        Ok(PyObject::none())
    }));

    // findall(match)
    let st = root_inner.clone();
    attrs.insert(CompactString::from("findall"), PyObject::native_closure("findall", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("findall() requires 1 argument"));
        }
        let tag_match = args[0].py_to_string();
        let guard = st.lock().unwrap();
        let mut results = Vec::new();
        for child in &guard.children {
            if child.tag == tag_match || tag_match == "*" {
                results.push(xml_element_to_pyobject(child));
            }
        }
        Ok(PyObject::list(results))
    }));

    // parse(source) — re-parse from string
    attrs.insert(CompactString::from("parse"), PyObject::native_closure("parse", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("parse() requires 1 argument"));
        }
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
