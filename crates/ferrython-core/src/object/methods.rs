//! PyObjectMethods trait and implementation — arithmetic, comparison, attributes, iteration.

use crate::error::{PyException, PyResult};
use crate::types::{HashableKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

use super::payload::*;
use super::helpers::*;

// ── Extension trait for methods on PyObjectRef ──

pub trait PyObjectMethods {
    fn type_name(&self) -> &'static str;
    fn is_truthy(&self) -> bool;
    fn is_callable(&self) -> bool;
    fn is_same(&self, other: &Self) -> bool;
    fn py_to_string(&self) -> String;
    fn repr(&self) -> String;
    fn to_list(&self) -> PyResult<Vec<PyObjectRef>>;
    fn to_int(&self) -> PyResult<i64>;
    fn to_float(&self) -> PyResult<f64>;
    fn as_int(&self) -> Option<i64>;
    fn as_str(&self) -> Option<&str>;
    fn to_hashable_key(&self) -> PyResult<HashableKey>;
    fn add(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn sub(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn mul(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn floor_div(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn true_div(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn modulo(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn power(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn lshift(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn rshift(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn bit_and(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn bit_or(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn bit_xor(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn negate(&self) -> PyResult<PyObjectRef>;
    fn positive(&self) -> PyResult<PyObjectRef>;
    fn invert(&self) -> PyResult<PyObjectRef>;
    fn py_abs(&self) -> PyResult<PyObjectRef>;
    fn compare(&self, other: &Self, op: CompareOp) -> PyResult<PyObjectRef>;
    fn get_attr(&self, name: &str) -> Option<PyObjectRef>;
    fn py_len(&self) -> PyResult<usize>;
    fn get_item(&self, key: &PyObjectRef) -> PyResult<PyObjectRef>;
    fn contains(&self, item: &PyObjectRef) -> PyResult<bool>;
    fn get_iter(&self) -> PyResult<PyObjectRef>;
    fn format_value(&self, spec: &str) -> PyResult<String>;
    fn dir(&self) -> Vec<CompactString>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp { Lt, Le, Eq, Ne, Gt, Ge }

/// Walk a class and its base classes (MRO) to find an attribute.
pub fn lookup_in_class_mro(class: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    if let PyObjectPayload::Class(cd) = &class.payload {
        // Check own namespace first
        if let Some(v) = cd.namespace.read().get(name).cloned() {
            return Some(v);
        }
        // Use computed MRO if available, otherwise walk bases recursively
        if !cd.mro.is_empty() {
            for base in &cd.mro {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    if let Some(v) = bcd.namespace.read().get(name).cloned() {
                        return Some(v);
                    }
                }
            }
        } else {
            for base in &cd.bases {
                if let Some(v) = lookup_in_class_mro(base, name) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// Check if an object is a data descriptor (has __set__ or __delete__).
/// Data descriptors take priority over instance __dict__ in attribute lookup.
pub fn is_data_descriptor(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Property { .. } => true,
        PyObjectPayload::Instance(inst) => {
            has_method_in_class(&inst.class, "__set__")
                || has_method_in_class(&inst.class, "__delete__")
        }
        _ => false,
    }
}

/// Check if an object has __get__ (i.e. is any kind of descriptor).
pub fn has_descriptor_get(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Property { .. } => true,
        PyObjectPayload::Instance(inst) => {
            has_method_in_class(&inst.class, "__get__")
        }
        _ => false,
    }
}

/// Check if a class (or its MRO) has a method by name.
fn has_method_in_class(class: &PyObjectRef, name: &str) -> bool {
    lookup_in_class_mro(class, name).is_some()
}


/// Returns a BuiltinBoundMethod if the method name matches, None otherwise.
fn instance_builtin_method(obj: &PyObjectRef, inst: &InstanceData, name: &str) -> Option<PyObjectRef> {
    let make_bound = |name: &str| -> PyObjectRef {
        Arc::new(PyObject {
            payload: PyObjectPayload::BuiltinBoundMethod {
                receiver: obj.clone(),
                method_name: CompactString::from(name),
            }
        })
    };

    // Namedtuple
    if inst.class.get_attr("__namedtuple__").is_some() {
        if name == "_fields" {
            return inst.class.get_attr("_fields");
        }
        if matches!(name, "_asdict" | "_replace" | "_make" | "__len__" | "__iter__") {
            return Some(make_bound(name));
        }
    }

    // Deque
    if inst.attrs.read().contains_key("__deque__") {
        if matches!(name, "append" | "appendleft" | "pop" | "popleft" | "extend"
            | "extendleft" | "rotate" | "clear" | "copy" | "count" | "index"
            | "insert" | "remove" | "reverse" | "maxlen"
            | "__iter__" | "__len__" | "__contains__" | "__getitem__")
        {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    // Hashlib hash objects
    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.as_str() } else { "" };
    if matches!(class_name, "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512") {
        if matches!(name, "hexdigest" | "digest" | "update" | "copy") {
            return Some(make_bound(name));
        }
        return inst.attrs.read().get(name).cloned();
    }

    None
}

impl PyObjectMethods for PyObjectRef {
    fn type_name(&self) -> &'static str {
        match &self.payload {
            PyObjectPayload::None => "NoneType",
            PyObjectPayload::Ellipsis => "ellipsis",
            PyObjectPayload::NotImplemented => "NotImplementedType",
            PyObjectPayload::Bool(_) => "bool",
            PyObjectPayload::Int(_) => "int",
            PyObjectPayload::Float(_) => "float",
            PyObjectPayload::Complex { .. } => "complex",
            PyObjectPayload::Str(_) => "str",
            PyObjectPayload::Bytes(_) => "bytes",
            PyObjectPayload::ByteArray(_) => "bytearray",
            PyObjectPayload::List(_) => "list",
            PyObjectPayload::Tuple(_) => "tuple",
            PyObjectPayload::Set(_) => "set",
            PyObjectPayload::FrozenSet(_) => "frozenset",
            PyObjectPayload::Dict(_) => "dict",
            PyObjectPayload::Function(_) => "function",
            PyObjectPayload::BuiltinFunction(_) => "builtin_function_or_method",
            PyObjectPayload::BuiltinType(_) => "type",
            PyObjectPayload::BoundMethod { .. } => "method",
            PyObjectPayload::BuiltinBoundMethod { .. } => "builtin_method",
            PyObjectPayload::Code(_) => "code",
            PyObjectPayload::Class(_) => "type",
            PyObjectPayload::Instance(inst) => {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    Box::leak(cd.name.to_string().into_boxed_str())
                } else { "object" }
            }
            PyObjectPayload::Module(_) => "module",
            PyObjectPayload::Iterator(_) => "iterator",
            PyObjectPayload::Slice { .. } => "slice",
            PyObjectPayload::Cell(_) => "cell",
            PyObjectPayload::ExceptionType(_) => "type",
            PyObjectPayload::ExceptionInstance { .. } => "exception",
            PyObjectPayload::Generator(_) => "generator",
            PyObjectPayload::NativeFunction { .. } => "builtin_function_or_method",
            PyObjectPayload::NativeClosure { .. } => "builtin_function_or_method",
            PyObjectPayload::Property { .. } => "property",
            PyObjectPayload::StaticMethod(_) => "staticmethod",
            PyObjectPayload::ClassMethod(_) => "classmethod",
            PyObjectPayload::Super { .. } => "super",
            PyObjectPayload::Partial { .. } => "functools.partial",
            PyObjectPayload::InstanceDict(_) => "dict",
        }
    }

    fn is_truthy(&self) -> bool {
        match &self.payload {
            PyObjectPayload::None => false,
            PyObjectPayload::Bool(b) => *b,
            PyObjectPayload::Int(n) => !n.is_zero(),
            PyObjectPayload::Float(f) => *f != 0.0,
            PyObjectPayload::Complex { real, imag } => *real != 0.0 || *imag != 0.0,
            PyObjectPayload::Str(s) => !s.is_empty(),
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => !b.is_empty(),
            PyObjectPayload::List(v) => !v.read().is_empty(),
            PyObjectPayload::Tuple(v) => !v.is_empty(),
            PyObjectPayload::Set(m) => !m.read().is_empty(),
            PyObjectPayload::FrozenSet(m) => !m.is_empty(),
            PyObjectPayload::Dict(m) => !m.read().is_empty(),
            _ => true,
        }
    }

    fn is_callable(&self) -> bool {
        matches!(&self.payload, PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
            | PyObjectPayload::BuiltinType(_) | PyObjectPayload::BoundMethod { .. }
            | PyObjectPayload::BuiltinBoundMethod { .. }
            | PyObjectPayload::Class(_) | PyObjectPayload::ExceptionType(_)
            | PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. } | PyObjectPayload::Partial { .. })
            || (matches!(&self.payload, PyObjectPayload::Instance(_)) && self.get_attr("__call__").is_some())
    }

    fn is_same(&self, other: &Self) -> bool { Arc::ptr_eq(self, other) }

    fn py_to_string(&self) -> String {
        match &self.payload {
            PyObjectPayload::None => "None".into(),
            PyObjectPayload::Bool(true) => "True".into(),
            PyObjectPayload::Bool(false) => "False".into(),
            PyObjectPayload::Int(n) => n.to_string(),
            PyObjectPayload::Float(f) => float_to_str(*f),
            PyObjectPayload::Complex { real, imag } => {
                if *real == 0.0 { format!("{}j", imag) }
                else { format!("({}+{}j)", real, imag) }
            }
            PyObjectPayload::Str(s) => s.to_string(),
            PyObjectPayload::Bytes(b) => format!("b{:?}", String::from_utf8_lossy(b)),
            PyObjectPayload::List(items) => format_collection("[", "]", &items.read()),
            PyObjectPayload::Tuple(items) => {
                if items.len() == 1 { format!("({},)", items[0].repr()) }
                else { format_collection("(", ")", items) }
            }
            PyObjectPayload::Set(m) => {
                let m = m.read();
                if m.is_empty() { "set()".into() }
                else { format_set("{", "}", &m) }
            }
            PyObjectPayload::Dict(m) => format_dict(&m.read()),
            PyObjectPayload::InstanceDict(attrs) => {
                let attrs = attrs.read();
                let mut parts = Vec::new();
                for (k, v) in attrs.iter() {
                    parts.push(format!("'{}': {}", k, v.repr()));
                }
                format!("{{{}}}", parts.join(", "))
            }
            PyObjectPayload::Ellipsis => "Ellipsis".into(),
            PyObjectPayload::NotImplemented => "NotImplemented".into(),
            PyObjectPayload::Function(f) => format!("<function {}>", f.name),
            PyObjectPayload::BuiltinFunction(n) => format!("<built-in function {}>", n),
            PyObjectPayload::BuiltinType(n) => format!("<class '{}'>", n),
            PyObjectPayload::Code(c) => format!("<code object {}>", c.name),
            PyObjectPayload::Class(cd) => format!("<class '{}'>", cd.name),
            PyObjectPayload::Instance(inst) => {
                // Check for __str__ method first
                if let Some(str_fn) = self.get_attr("__str__") {
                    if !matches!(&str_fn.payload, PyObjectPayload::BuiltinBoundMethod { .. }) {
                        // __str__ exists but we can't call it from here (no VM)
                        // Fall through to default
                    }
                }
                // Check for message attribute (common for exception subclasses)
                if let Some(msg) = inst.attrs.read().get("message") {
                    let s = msg.py_to_string();
                    if !s.is_empty() {
                        return s;
                    }
                }
                // Check for args attribute
                if let Some(args) = inst.attrs.read().get("args") {
                    if let PyObjectPayload::Tuple(items) = &args.payload {
                        if items.len() == 1 {
                            return items[0].py_to_string();
                        } else if !items.is_empty() {
                            return args.py_to_string();
                        }
                    }
                }
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    format!("<{} object>", cd.name)
                } else { "<object>".into() }
            }
            PyObjectPayload::Module(m) => format!("<module '{}'>", m.name),
            PyObjectPayload::Iterator(_) => "<iterator>".into(),
            PyObjectPayload::ExceptionType(kind) => format!("<class '{}'>", kind),
            PyObjectPayload::ExceptionInstance { kind, message, args, .. } => {
                // KeyError wraps its argument in repr() for str()
                if *kind == crate::error::ExceptionKind::KeyError && args.len() == 1 {
                    return args[0].repr();
                }
                if message.is_empty() {
                    String::new()
                } else {
                    message.to_string()
                }
            }
            _ => format!("<{}>", self.type_name()),
        }
    }

    fn repr(&self) -> String {
        match &self.payload {
            PyObjectPayload::Str(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
            PyObjectPayload::ExceptionInstance { kind, message, .. } => {
                if message.is_empty() {
                    format!("{}()", kind)
                } else {
                    format!("{}('{}')", kind, message)
                }
            }
            PyObjectPayload::Instance(inst) => {
                // Check for __repr__ first
                if let Some(_) = self.get_attr("__repr__") {
                    // Can't call from here (no VM), but py_to_string should handle
                }
                // For exception-like instances, show ClassName(message)
                if let Some(args) = inst.attrs.read().get("args") {
                    if let PyObjectPayload::Tuple(items) = &args.payload {
                        let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            cd.name.as_str()
                        } else { "object" };
                        if items.is_empty() {
                            return format!("{}()", class_name);
                        } else {
                            let args_str: Vec<String> = items.iter().map(|a| a.repr()).collect();
                            return format!("{}({})", class_name, args_str.join(", "));
                        }
                    }
                }
                self.py_to_string()
            }
            _ => self.py_to_string(),
        }
    }

    fn to_list(&self) -> PyResult<Vec<PyObjectRef>> {
        match &self.payload {
            PyObjectPayload::List(v) => Ok(v.read().clone()),
            PyObjectPayload::Tuple(v) => Ok(v.clone()),
            PyObjectPayload::Set(m) => Ok(m.read().values().cloned().collect()),
            PyObjectPayload::FrozenSet(m) => Ok(m.values().cloned().collect()),
            PyObjectPayload::Str(s) => Ok(s.chars().map(|c| PyObject::str_val(CompactString::from(c.to_string()))).collect()),
            PyObjectPayload::Dict(m) => Ok(m.read().keys().map(|k| k.to_object()).collect()),
            PyObjectPayload::InstanceDict(attrs) => Ok(attrs.read().keys().map(|k| PyObject::str_val(k.clone())).collect()),
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                Ok(b.iter().map(|byte| PyObject::int(*byte as i64)).collect())
            }
            PyObjectPayload::Iterator(iter_data) => {
                let data = iter_data.lock().unwrap();
                match &*data {
                    IteratorData::List { items, index } => Ok(items[*index..].to_vec()),
                    IteratorData::Tuple { items, index } => Ok(items[*index..].to_vec()),
                    IteratorData::Range { current, stop, step } => {
                        let mut result = Vec::new();
                        let mut val = *current;
                        while (*step > 0 && val < *stop) || (*step < 0 && val > *stop) {
                            result.push(PyObject::int(val));
                            val += step;
                        }
                        Ok(result)
                    }
                    IteratorData::Str { chars, index } => {
                        Ok(chars[*index..].iter().map(|c| PyObject::str_val(CompactString::from(c.to_string()))).collect())
                    }
                    // Lazy iterators can't be eagerly collected from core
                    IteratorData::Enumerate { .. }
                    | IteratorData::Zip { .. }
                    | IteratorData::Map { .. }
                    | IteratorData::Filter { .. }
                    | IteratorData::Sentinel { .. } => {
                        Err(PyException::type_error("lazy iterator requires VM to collect"))
                    }
                }
            }
            _ => Err(PyException::type_error(format!("'{}' object is not iterable", self.type_name()))),
        }
    }

    fn to_int(&self) -> PyResult<i64> {
        match &self.payload {
            PyObjectPayload::Int(n) => n.to_i64().ok_or_else(|| PyException::overflow_error("int too large")),
            PyObjectPayload::Bool(b) => Ok(if *b { 1 } else { 0 }),
            PyObjectPayload::Float(f) => Ok(*f as i64),
            PyObjectPayload::Str(s) => s.trim().parse::<i64>().map_err(|_|
                PyException::value_error(format!("invalid literal for int(): '{}'", s))),
            PyObjectPayload::Instance(_) => {
                // Check for __int__ or __index__ on the instance
                if let Some(int_val) = self.get_attr("__int__") {
                    if let Some(v) = int_val.as_int() { return Ok(v); }
                }
                if let Some(idx_val) = self.get_attr("__index__") {
                    if let Some(v) = idx_val.as_int() { return Ok(v); }
                }
                Err(PyException::type_error(format!("int() argument must be a string or number, not '{}'", self.type_name())))
            }
            _ => Err(PyException::type_error(format!("int() argument must be a string or number, not '{}'", self.type_name()))),
        }
    }

    fn to_float(&self) -> PyResult<f64> {
        match &self.payload {
            PyObjectPayload::Float(f) => Ok(*f),
            PyObjectPayload::Int(n) => Ok(n.to_f64()),
            PyObjectPayload::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            PyObjectPayload::Str(s) => s.trim().parse::<f64>().map_err(|_|
                PyException::value_error(format!("could not convert string to float: '{}'", s))),
            PyObjectPayload::Instance(_) => {
                if let Some(float_val) = self.get_attr("__float__") {
                    if let PyObjectPayload::Float(f) = &float_val.payload {
                        return Ok(*f);
                    }
                }
                Err(PyException::type_error(format!("float() argument must be a string or number, not '{}'", self.type_name())))
            }
            _ => Err(PyException::type_error(format!("float() argument must be a string or number, not '{}'", self.type_name()))),
        }
    }

    fn as_int(&self) -> Option<i64> {
        match &self.payload {
            PyObjectPayload::Int(n) => n.to_i64(),
            PyObjectPayload::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match &self.payload {
            PyObjectPayload::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn to_hashable_key(&self) -> PyResult<HashableKey> { HashableKey::from_object(self) }

    // ── Arithmetic ──

    fn add(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            // Bool → Int coercion for arithmetic
            (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => Ok(PyObject::int(*a as i64 + *b as i64)),
            (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => Ok(PyInt::add_op(&PyInt::Small(*a as i64), b).to_object()),
            (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => Ok(PyInt::add_op(a, &PyInt::Small(*b as i64)).to_object()),
            (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(*a as i64 as f64 + b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => Ok(PyObject::float(a + *b as i64 as f64)),
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::add_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a + b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() + b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a + b.to_f64())),
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                Ok(PyObject::complex(ar + br, ai + bi))
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag }) => {
                Ok(PyObject::complex(a.to_f64() + real, *imag))
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
                Ok(PyObject::complex(real + b.to_f64(), *imag))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag }) => {
                Ok(PyObject::complex(a + real, *imag))
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
                Ok(PyObject::complex(real + b, *imag))
            }
            (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => {
                let mut s = a.to_string(); s.push_str(b.as_str());
                Ok(PyObject::str_val(CompactString::from(s)))
            }
            (PyObjectPayload::List(a), PyObjectPayload::List(b)) => {
                let mut r = a.read().clone(); r.extend(b.read().iter().cloned()); Ok(PyObject::list(r))
            }
            (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
                let mut r = a.clone(); r.extend(b.iter().cloned()); Ok(PyObject::tuple(r))
            }
            (PyObjectPayload::Bytes(a), PyObjectPayload::Bytes(b)) | (PyObjectPayload::ByteArray(a), PyObjectPayload::Bytes(b)) | (PyObjectPayload::Bytes(a), PyObjectPayload::ByteArray(b)) => {
                let mut r = a.clone(); r.extend(b); Ok(PyObject::bytes(r))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for +: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn sub(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => Ok(PyObject::int(*a as i64 - *b as i64)),
            (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => Ok(PyInt::sub_op(&PyInt::Small(*a as i64), b).to_object()),
            (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => Ok(PyInt::sub_op(a, &PyInt::Small(*b as i64)).to_object()),
            (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(*a as i64 as f64 - b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => Ok(PyObject::float(a - *b as i64 as f64)),
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::sub_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a - b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() - b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a - b.to_f64())),
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                Ok(PyObject::complex(ar - br, ai - bi))
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag }) => {
                Ok(PyObject::complex(a.to_f64() - real, -*imag))
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
                Ok(PyObject::complex(real - b.to_f64(), *imag))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag }) => {
                Ok(PyObject::complex(a - real, -*imag))
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
                Ok(PyObject::complex(real - b, *imag))
            }
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = IndexMap::new();
                for (k, v) in ra.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = IndexMap::new();
                for (k, v) in a.iter() { if !b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(result)))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for -: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn mul(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => Ok(PyObject::int(*a as i64 * *b as i64)),
            (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => Ok(PyInt::mul_op(&PyInt::Small(*a as i64), b).to_object()),
            (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => Ok(PyInt::mul_op(a, &PyInt::Small(*b as i64)).to_object()),
            (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(*a as i64 as f64 * b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => Ok(PyObject::float(a * *b as i64 as f64)),
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::mul_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a * b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() * b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a * b.to_f64())),
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                Ok(PyObject::complex(ar * br - ai * bi, ar * bi + ai * br))
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag }) |
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(a)) => {
                let af = a.to_f64();
                Ok(PyObject::complex(af * real, af * imag))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag }) |
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(a)) => {
                Ok(PyObject::complex(a * real, a * imag))
            }
            (PyObjectPayload::Str(s), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::Str(s)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                Ok(PyObject::str_val(CompactString::from(s.repeat(count))))
            }
            (PyObjectPayload::List(items), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::List(items)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let read = items.read();
                let mut result = Vec::with_capacity(read.len() * count);
                for _ in 0..count { result.extend(read.iter().cloned()); }
                Ok(PyObject::list(result))
            }
            (PyObjectPayload::Tuple(items), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::Tuple(items)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let mut result = Vec::with_capacity(items.len() * count);
                for _ in 0..count { result.extend(items.iter().cloned()); }
                Ok(PyObject::tuple(result))
            }
            (PyObjectPayload::Bytes(b), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::Bytes(b)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let mut result = Vec::with_capacity(b.len() * count);
                for _ in 0..count { result.extend(b); }
                Ok(PyObject::bytes(result))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for *: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn floor_div(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if b.is_zero() { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                Ok(PyInt::floor_div_op(a, b).to_object())
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => {
                if *b == 0.0 { return Err(PyException::zero_division_error("float floor division by zero")); }
                Ok(PyObject::float((a / b).floor()))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for //: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn true_div(&self, other: &Self) -> PyResult<PyObjectRef> {
        // Complex division
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                let denom = br * br + bi * bi;
                if denom == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex((ar * br + ai * bi) / denom, (ai * br - ar * bi) / denom));
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
                let bf = b.to_f64();
                if bf == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex(real / bf, imag / bf));
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
                if *b == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex(real / b, imag / b));
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real: br, imag: bi }) => {
                let af = a.to_f64();
                let denom = br * br + bi * bi;
                if denom == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex((af * br) / denom, (-af * bi) / denom));
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real: br, imag: bi }) => {
                let denom = br * br + bi * bi;
                if denom == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex((a * br) / denom, (-a * bi) / denom));
            }
            _ => {}
        }
        let a = coerce_to_f64(self)?;
        let b = coerce_to_f64(other)?;
        if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
        Ok(PyObject::float(a / b))
    }

    fn modulo(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if b.is_zero() { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                Ok(PyInt::modulo_op(a, b).to_object())
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => {
                if *b == 0.0 { return Err(PyException::zero_division_error("float modulo")); }
                Ok(PyObject::float(python_fmod(*a, *b)))
            }
            (PyObjectPayload::Str(fmt_str), _) => {
                // printf-style string formatting: "Hello %s" % "world"
                let args_list = match &other.payload {
                    PyObjectPayload::Tuple(items) => items.clone(),
                    _ => vec![other.clone()],
                };
                let mut result = String::new();
                let mut arg_idx = 0;
                let chars: Vec<char> = fmt_str.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    if chars[i] == '%' && i + 1 < chars.len() {
                        i += 1;
                        // Parse optional flags, width, precision
                        let mut spec_chars = String::new();
                        while i < chars.len() && "-+ #0123456789.".contains(chars[i]) {
                            spec_chars.push(chars[i]);
                            i += 1;
                        }
                        if i >= chars.len() { break; }
                        let conv = chars[i];
                        i += 1;
                        if conv == '%' {
                            result.push('%');
                            continue;
                        }
                        if arg_idx >= args_list.len() {
                            return Err(PyException::type_error("not enough arguments for format string"));
                        }
                        let arg = &args_list[arg_idx];
                        arg_idx += 1;
                        match conv {
                            's' => {
                                let s = arg.py_to_string();
                                if spec_chars.is_empty() {
                                    result.push_str(&s);
                                } else {
                                    result.push_str(&format_str_spec(&s, &spec_chars));
                                }
                            }
                            'r' => {
                                let s = arg.repr();
                                if spec_chars.is_empty() {
                                    result.push_str(&s);
                                } else {
                                    result.push_str(&format_str_spec(&s, &spec_chars));
                                }
                            }
                            'd' | 'i' => {
                                let n = arg.to_int()?;
                                if spec_chars.is_empty() {
                                    result.push_str(&n.to_string());
                                } else {
                                    result.push_str(&format_int_spec(n, &spec_chars));
                                }
                            }
                            'f' | 'F' => {
                                let f = arg.to_float()?;
                                if spec_chars.is_empty() {
                                    result.push_str(&format!("{:.6}", f));
                                } else {
                                    result.push_str(&format_float_spec(f, &spec_chars));
                                }
                            }
                            'x' => result.push_str(&format!("{:x}", arg.to_int()?)),
                            'X' => result.push_str(&format!("{:X}", arg.to_int()?)),
                            'o' => result.push_str(&format!("{:o}", arg.to_int()?)),
                            _ => {
                                result.push('%');
                                result.push_str(&spec_chars);
                                result.push(conv);
                            }
                        }
                    } else {
                        result.push(chars[i]);
                        i += 1;
                    }
                }
                Ok(PyObject::str_val(CompactString::from(result)))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for %: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn power(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if let Some(exp) = b.to_i64() {
                    if exp >= 0 && exp <= 63 { return Ok(PyInt::pow_op(a, exp as u32).to_object()); }
                }
                Ok(PyObject::float(a.to_f64().powf(b.to_f64())))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.powf(*b))),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64().powf(*b))),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a.powf(b.to_f64()))),
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for **: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn lshift(&self, other: &Self) -> PyResult<PyObjectRef> { int_bitop(self, other, "<<", |a, b| a << b) }
    fn rshift(&self, other: &Self) -> PyResult<PyObjectRef> { int_bitop(self, other, ">>", |a, b| a >> b) }
    fn bit_and(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = IndexMap::new();
                for (k, v) in ra.iter() { if rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            _ => int_bitop(self, other, "&", |a, b| a & b),
        }
    }
    fn bit_or(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = ra.clone();
                for (k, v) in rb.iter() { result.insert(k.clone(), v.clone()); }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            _ => int_bitop(self, other, "|", |a, b| a | b),
        }
    }
    fn bit_xor(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = IndexMap::new();
                for (k, v) in ra.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                for (k, v) in rb.iter() { if !ra.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            _ => int_bitop(self, other, "^", |a, b| a ^ b),
        }
    }

    fn negate(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(n) => Ok(n.negate().to_object()),
            PyObjectPayload::Float(f) => Ok(PyObject::float(-f)),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(-(*b as i64))),
            PyObjectPayload::Complex { real, imag } => Ok(PyObject::complex(-real, -imag)),
            _ => Err(PyException::type_error(format!("bad operand type for unary -: '{}'", self.type_name()))),
        }
    }

    fn positive(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(_) | PyObjectPayload::Float(_) | PyObjectPayload::Bool(_) |
            PyObjectPayload::Complex { .. } => Ok(self.clone()),
            _ => Err(PyException::type_error(format!("bad operand type for unary +: '{}'", self.type_name()))),
        }
    }

    fn invert(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(n) => Ok(n.invert().to_object()),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(!(*b as i64))),
            _ => Err(PyException::type_error(format!("bad operand type for unary ~: '{}'", self.type_name()))),
        }
    }

    fn py_abs(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(n) => Ok(n.abs().to_object()),
            PyObjectPayload::Float(f) => Ok(PyObject::float(f.abs())),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(*b as i64)),
            PyObjectPayload::Complex { real, imag } => Ok(PyObject::float((real * real + imag * imag).sqrt())),
            _ => Err(PyException::type_error(format!("bad operand type for abs(): '{}'", self.type_name()))),
        }
    }

    fn compare(&self, other: &Self, op: CompareOp) -> PyResult<PyObjectRef> {
        // Set comparisons: subset/superset semantics
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let result = match op {
                    CompareOp::Eq => ra.len() == rb.len() && ra.keys().all(|k| rb.contains_key(k)),
                    CompareOp::Ne => !(ra.len() == rb.len() && ra.keys().all(|k| rb.contains_key(k))),
                    CompareOp::Le => ra.keys().all(|k| rb.contains_key(k)),  // issubset
                    CompareOp::Lt => ra.len() < rb.len() && ra.keys().all(|k| rb.contains_key(k)),
                    CompareOp::Ge => rb.keys().all(|k| ra.contains_key(k)),  // issuperset
                    CompareOp::Gt => ra.len() > rb.len() && rb.keys().all(|k| ra.contains_key(k)),
                };
                return Ok(PyObject::bool_val(result));
            }
            _ => {}
        }
        let ord = partial_cmp_objects(self, other);
        let result = match op {
            // For Eq/Ne, if types don't define comparison (ord is None),
            // fall back to identity comparison (like CPython's default __eq__)
            CompareOp::Eq => match ord {
                Some(o) => o == std::cmp::Ordering::Equal,
                None => std::ptr::eq(self.as_ref(), other.as_ref()),
            },
            CompareOp::Ne => match ord {
                Some(o) => o != std::cmp::Ordering::Equal,
                None => !std::ptr::eq(self.as_ref(), other.as_ref()),
            },
            CompareOp::Lt => ord == Some(std::cmp::Ordering::Less),
            CompareOp::Le => matches!(ord, Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)),
            CompareOp::Gt => ord == Some(std::cmp::Ordering::Greater),
            CompareOp::Ge => matches!(ord, Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)),
        };
        Ok(PyObject::bool_val(result))
    }

    fn get_attr(&self, name: &str) -> Option<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Instance(inst) => {
                // Special instance attributes
                if name == "__class__" {
                    return Some(inst.class.clone());
                }
                if name == "__dict__" {
                    return Some(PyObject::wrap(PyObjectPayload::InstanceDict(inst.attrs.clone())));
                }
                // CPython descriptor protocol:
                // 1. Data descriptors (has __set__ or __delete__) from class MRO
                // 2. Instance __dict__
                // 3. Non-data descriptors (has __get__ only) and other class attrs
                let class_attr = lookup_in_class_mro(&inst.class, name);
                if let Some(ref v) = class_attr {
                    match &v.payload {
                        PyObjectPayload::Property { .. } => {
                            // Property is always a data descriptor — VM calls fget
                            return Some(v.clone());
                        }
                        PyObjectPayload::StaticMethod(func) => {
                            return Some(func.clone());
                        }
                        PyObjectPayload::ClassMethod(func) => {
                            return Some(Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: inst.class.clone(),
                                    method: func.clone(),
                                }
                            }));
                        }
                        _ => {
                            // Check if this is a custom data descriptor (has __set__ or __delete__)
                            if is_data_descriptor(v) {
                                // Data descriptor takes priority over instance dict
                                // Return it; VM's LoadAttr will call __get__
                                return Some(v.clone());
                            }
                        }
                    }
                }
                // Instance attributes (between data and non-data descriptors)
                if let Some(v) = inst.attrs.read().get(name) { return Some(v.clone()); }
                // Non-data descriptors and other class attrs
                if let Some(v) = class_attr {
                    if matches!(&v.payload, PyObjectPayload::Function(_)) {
                        return Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: self.clone(),
                                method: v.clone(),
                            }
                        }));
                    }
                    return Some(v.clone());
                }
                // Check for built-in instance methods (namedtuple, deque, hashlib)
                if let Some(result) = instance_builtin_method(self, inst, name) {
                    return Some(result);
                }
                None
            }
            PyObjectPayload::Class(cd) => {
                // Special class attributes
                if name == "__name__" {
                    return Some(PyObject::str_val(cd.name.clone()));
                }
                if name == "__bases__" {
                    return Some(PyObject::tuple(cd.bases.clone()));
                }
                if name == "__mro__" {
                    let mut mro_list = vec![self.clone()];
                    mro_list.extend(cd.mro.iter().cloned());
                    return Some(PyObject::tuple(mro_list));
                }
                if name == "__dict__" {
                    let ns = cd.namespace.read();
                    let mut map = IndexMap::new();
                    for (k, v) in ns.iter() {
                        if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                            map.insert(hk, v.clone());
                        }
                    }
                    return Some(PyObject::dict(map));
                }
                // Check own namespace first, then bases
                if let Some(v) = cd.namespace.read().get(name).cloned() {
                    match &v.payload {
                        PyObjectPayload::StaticMethod(func) => return Some(func.clone()),
                        PyObjectPayload::ClassMethod(func) => {
                            return Some(Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: self.clone(),
                                    method: func.clone(),
                                }
                            }));
                        }
                        _ => return Some(v),
                    }
                }
                for base in &cd.bases {
                    if let Some(v) = base.get_attr(name) { return Some(v); }
                }
                None
            }
            PyObjectPayload::Module(m) => m.attrs.get(name).cloned(),
            PyObjectPayload::Slice { start, stop, step } => {
                match name {
                    "start" => Some(start.clone().unwrap_or_else(PyObject::none)),
                    "stop" => Some(stop.clone().unwrap_or_else(PyObject::none)),
                    "step" => Some(step.clone().unwrap_or_else(PyObject::none)),
                    _ => None,
                }
            }
            PyObjectPayload::Complex { real, imag } => {
                match name {
                    "real" => Some(PyObject::float(*real)),
                    "imag" => Some(PyObject::float(*imag)),
                    "__class__" => Some(PyObject::builtin_type(CompactString::from("complex"))),
                    "conjugate" => Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: self.clone(),
                            method_name: CompactString::from("conjugate"),
                        }
                    })),
                    _ => None,
                }
            }
            PyObjectPayload::BuiltinType(n) => {
                match name {
                    "__name__" => Some(PyObject::str_val(n.clone())),
                    "fromkeys" if n.as_str() == "dict" => {
                        Some(PyObject::native_function("dict.fromkeys", |args| {
                            if args.is_empty() { return Err(crate::error::PyException::type_error("fromkeys() requires at least 1 argument")); }
                            let keys = args[0].to_list()?;
                            let value = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
                            let mut map = IndexMap::new();
                            for k in keys {
                                let hk = k.to_hashable_key()?;
                                map.insert(hk, value.clone());
                            }
                            Ok(PyObject::dict(map))
                        }))
                    }
                    "maketrans" if n.as_str() == "str" => {
                        Some(PyObject::native_function("str.maketrans", |args| {
                            if args.is_empty() { return Err(crate::error::PyException::type_error("maketrans() requires at least 1 argument")); }
                            let mut result_map = IndexMap::new();
                            if args.len() == 1 {
                                if let PyObjectPayload::Dict(map) = &args[0].payload {
                                    for (k, v) in map.read().iter() {
                                        let key = match k {
                                            HashableKey::Int(n) => n.clone(),
                                            HashableKey::Str(s) => {
                                                if let Some(c) = s.chars().next() { PyInt::Small(c as i64) } else { continue; }
                                            }
                                            _ => continue,
                                        };
                                        result_map.insert(HashableKey::Int(key), v.clone());
                                    }
                                }
                            } else {
                                let x = args[0].py_to_string();
                                let y = args[1].py_to_string();
                                for (cx, cy) in x.chars().zip(y.chars()) {
                                    result_map.insert(HashableKey::Int(PyInt::Small(cx as i64)), PyObject::int(cy as i64));
                                }
                                if args.len() > 2 {
                                    let z = args[2].py_to_string();
                                    for cz in z.chars() {
                                        result_map.insert(HashableKey::Int(PyInt::Small(cz as i64)), PyObject::none());
                                    }
                                }
                            }
                            Ok(PyObject::dict(result_map))
                        }))
                    }
                    // object.__setattr__(instance, name, value) — bypass custom __setattr__
                    "__setattr__" if n.as_str() == "object" => {
                        Some(PyObject::native_function("object.__setattr__", |args| {
                            if args.len() != 3 {
                                return Err(crate::error::PyException::type_error(
                                    "object.__setattr__() takes exactly 3 arguments"));
                            }
                            let name = args[1].as_str().ok_or_else(||
                                crate::error::PyException::type_error("attribute name must be string"))?;
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                inst.attrs.write().insert(CompactString::from(name), args[2].clone());
                            }
                            Ok(PyObject::none())
                        }))
                    }
                    // object.__getattribute__(instance, name) — bypass custom __getattribute__
                    "__getattribute__" if n.as_str() == "object" => {
                        Some(PyObject::native_function("object.__getattribute__", |args| {
                            if args.len() != 2 {
                                return Err(crate::error::PyException::type_error(
                                    "object.__getattribute__() takes exactly 2 arguments"));
                            }
                            let name = args[1].as_str().ok_or_else(||
                                crate::error::PyException::type_error("attribute name must be string"))?;
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                if let Some(val) = inst.attrs.read().get(name) {
                                    return Ok(val.clone());
                                }
                            }
                            args[0].get_attr(name).ok_or_else(||
                                crate::error::PyException::attribute_error(&format!(
                                    "'{}' object has no attribute '{}'", args[0].type_name(), name)))
                        }))
                    }
                    // object.__delattr__(instance, name) — bypass custom __delattr__
                    "__delattr__" if n.as_str() == "object" => {
                        Some(PyObject::native_function("object.__delattr__", |args| {
                            if args.len() != 2 {
                                return Err(crate::error::PyException::type_error(
                                    "object.__delattr__() takes exactly 2 arguments"));
                            }
                            let name = args[1].as_str().ok_or_else(||
                                crate::error::PyException::type_error("attribute name must be string"))?;
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                inst.attrs.write().shift_remove(name);
                            }
                            Ok(PyObject::none())
                        }))
                    }
                    _ => {
                        // Dunder methods BuiltinType doesn't have → return None
                        if name.starts_with("__") && name.ends_with("__") {
                            match name {
                                "__init_subclass__" | "__set_name__"
                                | "__prepare__" | "__instancecheck__"
                                | "__subclasscheck__" | "__class_getitem__"
                                => None,
                                _ => {
                                    Some(Arc::new(PyObject {
                                        payload: PyObjectPayload::BuiltinBoundMethod {
                                            receiver: self.clone(),
                                            method_name: CompactString::from(name),
                                        }
                                    }))
                                }
                            }
                        } else {
                            // Unbound method access: str.upper, list.append, etc.
                            Some(Arc::new(PyObject {
                                payload: PyObjectPayload::BuiltinBoundMethod {
                                    receiver: self.clone(),
                                    method_name: CompactString::from(name),
                                }
                            }))
                        }
                    }
                }
            }
            PyObjectPayload::Property { fget, fset, fdel } => {
                match name {
                    "setter" | "getter" | "deleter" | "fget" | "fset" | "fdel" => {
                        match name {
                            "fget" => return fget.clone().or_else(|| Some(PyObject::none())),
                            "fset" => return fset.clone().or_else(|| Some(PyObject::none())),
                            "fdel" => return fdel.clone().or_else(|| Some(PyObject::none())),
                            _ => {}
                        }
                        // Return a BuiltinBoundMethod that the VM will handle
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: self.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::ExceptionType(kind) => {
                match name {
                    "__name__" => Some(PyObject::str_val(CompactString::from(format!("{:?}", kind)))),
                    _ => None,
                }
            }
            PyObjectPayload::ExceptionInstance { kind, message, args, attrs } => {
                match name {
                    "args" => {
                        if args.is_empty() {
                            if message.is_empty() {
                                Some(PyObject::tuple(vec![]))
                            } else {
                                Some(PyObject::tuple(vec![PyObject::str_val(message.clone())]))
                            }
                        } else {
                            Some(PyObject::tuple(args.clone()))
                        }
                    }
                    "__class__" => Some(PyObject::exception_type(kind.clone())),
                    _ => {
                        // Check user-set attrs (e.g., __cause__)
                        attrs.read().get(name).cloned()
                    }
                }
            }
            // Function attributes
            PyObjectPayload::Function(f) => match name {
                "__name__" => Some(PyObject::str_val(f.name.clone())),
                "__qualname__" => Some(PyObject::str_val(f.qualname.clone())),
                "__defaults__" => {
                    if f.defaults.is_empty() { Some(PyObject::none()) }
                    else { Some(PyObject::tuple(f.defaults.clone())) }
                }
                "__module__" => Some(PyObject::str_val(CompactString::from("__main__"))),
                "__doc__" => Some(PyObject::none()),
                "__dict__" => Some(PyObject::dict(IndexMap::new())),
                "__annotations__" => {
                    let mut map = IndexMap::new();
                    for (k, v) in &f.annotations {
                        if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                            map.insert(hk, v.clone());
                        }
                    }
                    Some(PyObject::dict(map))
                }
                "__closure__" => Some(PyObject::none()),
                "__code__" => Some(PyObject::none()),
                _ => None,
            }
            PyObjectPayload::NativeFunction { name: fname, .. } => match name {
                "__name__" => Some(PyObject::str_val(CompactString::from(fname.as_str()))),
                "__qualname__" => Some(PyObject::str_val(CompactString::from(fname.as_str()))),
                "__module__" => Some(PyObject::str_val(CompactString::from("builtins"))),
                "__doc__" => Some(PyObject::none()),
                _ => None,
            }
            PyObjectPayload::BoundMethod { method, .. } => {
                method.get_attr(name)
            }
            // Int property-like attributes (return values, not bound methods)
            PyObjectPayload::Int(_n) => match name {
                "real" | "numerator" => Some(PyObject::wrap(self.payload.clone())),
                "imag" => Some(PyObject::int(0)),
                "denominator" => Some(PyObject::int(1)),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("int"))),
                _ => Some(Arc::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod {
                        receiver: self.clone(),
                        method_name: CompactString::from(name),
                    }
                })),
            },
            // Float property-like attributes
            PyObjectPayload::Float(f) => match name {
                "real" => Some(PyObject::float(*f)),
                "imag" => Some(PyObject::float(0.0)),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("float"))),
                _ => Some(Arc::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod {
                        receiver: self.clone(),
                        method_name: CompactString::from(name),
                    }
                })),
            },
            // Bool property-like attributes (bool is subtype of int)
            PyObjectPayload::Bool(b) => match name {
                "real" | "numerator" => Some(PyObject::int(if *b { 1 } else { 0 })),
                "imag" => Some(PyObject::int(0)),
                "denominator" => Some(PyObject::int(1)),
                "__class__" => Some(PyObject::builtin_type(CompactString::from("bool"))),
                _ => Some(Arc::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod {
                        receiver: self.clone(),
                        method_name: CompactString::from(name),
                    }
                })),
            },
            // Built-in type methods — return bound method names
            PyObjectPayload::Str(_) | PyObjectPayload::List(_) |
            PyObjectPayload::Dict(_) | PyObjectPayload::InstanceDict(_) | PyObjectPayload::Tuple(_) |
            PyObjectPayload::Set(_) | PyObjectPayload::Bytes(_) => {
                if name == "__class__" {
                    let type_name = self.type_name();
                    return Some(PyObject::builtin_type(CompactString::from(type_name)));
                }
                Some(Arc::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod {
                        receiver: self.clone(),
                        method_name: CompactString::from(name),
                    }
                }))
            }
            PyObjectPayload::Generator(_) => {
                match name {
                    "send" | "throw" | "close" | "__next__" => {
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: self.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::Super { cls, instance } => {
                // super() proxy: look up in the RUNTIME class MRO, skipping up to and including cls
                let runtime_cls = match &instance.payload {
                    PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
                    PyObjectPayload::Class(_) => Some(instance.clone()),
                    _ => None,
                };
                if let Some(rt_cls) = runtime_cls {
                    if let PyObjectPayload::Class(cd) = &rt_cls.payload {
                        let mro = &cd.mro;
                        // If cls IS the runtime class itself, start from index 0.
                        // Otherwise, skip entries up to and including cls in the MRO.
                        let cls_is_self = std::sync::Arc::ptr_eq(cls, &rt_cls);
                        let mut found_cls = cls_is_self;
                        for base in mro {
                            if !found_cls {
                                if std::sync::Arc::ptr_eq(base, cls) {
                                    found_cls = true;
                                }
                                continue;
                            }
                            // Look in this base's namespace directly
                            if let PyObjectPayload::Class(bcd) = &base.payload {
                                if let Some(v) = bcd.namespace.read().get(name) {
                                    if matches!(&v.payload, PyObjectPayload::Function(_)) {
                                        return Some(Arc::new(PyObject {
                                            payload: PyObjectPayload::BoundMethod {
                                                receiver: instance.clone(),
                                                method: v.clone(),
                                            }
                                        }));
                                    }
                                    return Some(v.clone());
                                }
                            }
                            // ExceptionType base: provide synthetic __init__/__str__
                            if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                                if let Some(resolved) = resolve_exception_type_method(name, instance) {
                                    // Bind to instance so self is prepended
                                    return Some(Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: instance.clone(),
                                            method: resolved,
                                        }
                                    }));
                                }
                            }
                            // BuiltinType base in MRO
                            if let PyObjectPayload::BuiltinType(bt_name) = &base.payload {
                                if let Some(resolved) = crate::object::resolve_builtin_type_method(bt_name.as_str(), name) {
                                    return Some(resolved);
                                }
                            }
                        }
                        // Fallback: if cls not found in MRO, look in cls's own bases
                        if !found_cls {
                            if let PyObjectPayload::Class(ccd) = &cls.payload {
                                for base in &ccd.bases {
                                    if let PyObjectPayload::Class(bcd) = &base.payload {
                                        if let Some(v) = bcd.namespace.read().get(name) {
                                            if matches!(&v.payload, PyObjectPayload::Function(_)) {
                                                return Some(Arc::new(PyObject {
                                                    payload: PyObjectPayload::BoundMethod {
                                                        receiver: instance.clone(),
                                                        method: v.clone(),
                                                    }
                                                }));
                                            }
                                            return Some(v.clone());
                                        }
                                    }
                                    // Check BuiltinType bases (e.g., type, object)
                                    if let PyObjectPayload::BuiltinType(bt_name) = &base.payload {
                                        if let Some(resolved) = crate::object::resolve_builtin_type_method(bt_name.as_str(), name) {
                                            return Some(resolved);
                                        }
                                    }
                                    // Check ExceptionType bases
                                    if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                                        if let Some(resolved) = resolve_exception_type_method(name, instance) {
                                            return Some(Arc::new(PyObject {
                                                payload: PyObjectPayload::BoundMethod {
                                                    receiver: instance.clone(),
                                                    method: resolved,
                                                }
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                        // Builtin __new__: object.__new__(cls) creates a new instance
                        if name == "__new__" {
                            return Some(PyObject::native_function("__new__", |args| {
                                if args.is_empty() { return Err(PyException::type_error("__new__ requires cls")); }
                                Ok(PyObject::instance(args[0].clone()))
                            }));
                        }
                        // Builtin __init_subclass__: object.__init_subclass__() is a no-op
                        if name == "__init_subclass__" {
                            return Some(PyObject::native_function("__init_subclass__", |_args| {
                                Ok(PyObject::none())
                            }));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn py_len(&self) -> PyResult<usize> {
        match &self.payload {
            PyObjectPayload::Str(s) => Ok(s.chars().count()),
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.len()),
            PyObjectPayload::List(v) => Ok(v.read().len()),
            PyObjectPayload::Tuple(v) => Ok(v.len()),
            PyObjectPayload::Set(m) => Ok(m.read().len()),
            PyObjectPayload::FrozenSet(m) => Ok(m.len()),
            PyObjectPayload::Dict(m) => {
                let map = m.read();
                let hidden = if map.contains_key(&HashableKey::Str(CompactString::from("__defaultdict_factory__"))) { 1 } else { 0 };
                Ok(map.len() - hidden)
            },
            PyObjectPayload::Iterator(iter_data) => {
                let data = iter_data.lock().unwrap();
                match &*data {
                    IteratorData::Range { current, stop, step } => {
                        if *step > 0 && *current < *stop {
                            Ok(((stop - current + step - 1) / step) as usize)
                        } else if *step < 0 && *current > *stop {
                            Ok(((current - stop - step - 1) / (-step)) as usize)
                        } else {
                            Ok(0)
                        }
                    }
                    IteratorData::List { items, index } => Ok(items.len() - index),
                    IteratorData::Tuple { items, index } => Ok(items.len() - index),
                    IteratorData::Str { chars, index } => Ok(chars.len() - index),
                    _ => Err(PyException::type_error("object of type 'iterator' has no len()")),
                }
            }
            _ => Err(PyException::type_error(format!("object of type '{}' has no len()", self.type_name()))),
        }
    }

    fn get_item(&self, key: &PyObjectRef) -> PyResult<PyObjectRef> {
        // Check for slice key first
        if let PyObjectPayload::Slice { start, stop, step } = &key.payload {
            return get_slice_impl(self, start, stop, step);
        }
        match &self.payload {
            PyObjectPayload::List(items) => {
                let items = items.read();
                let idx = key.to_int()?;
                let len = items.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("index out of range")); }
                Ok(items[actual as usize].clone())
            }
            PyObjectPayload::Tuple(items) => {
                let idx = key.to_int()?;
                let len = items.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("index out of range")); }
                Ok(items[actual as usize].clone())
            }
            PyObjectPayload::Dict(map) => {
                let hk = key.to_hashable_key()?;
                let map_r = map.read();
                if let Some(val) = map_r.get(&hk) {
                    return Ok(val.clone());
                }
                // Check for __defaultdict_factory__ (Counter / defaultdict)
                let factory_key = HashableKey::Str(CompactString::from("__defaultdict_factory__"));
                if let Some(factory) = map_r.get(&factory_key) {
                    let factory = factory.clone();
                    drop(map_r);
                    // Create default value by "calling" the factory
                    // For common factories: int -> 0, list -> [], str -> "", float -> 0.0
                    let default = match &factory.payload {
                        PyObjectPayload::BuiltinType(name) => {
                            match name.as_str() {
                                "int" => PyObject::int(0),
                                "float" => PyObject::float(0.0),
                                "str" => PyObject::str_val(CompactString::new("")),
                                "list" => PyObject::list(vec![]),
                                "bool" => PyObject::bool_val(false),
                                "tuple" => PyObject::tuple(vec![]),
                                "set" => PyObject::set(IndexMap::new()),
                                "dict" => PyObject::dict(IndexMap::new()),
                                _ => return Err(PyException::key_error(key.repr())),
                            }
                        }
                        _ => return Err(PyException::key_error(key.repr())),
                    };
                    // Store the default value
                    map.write().insert(hk, default.clone());
                    return Ok(default);
                }
                Err(PyException::key_error(key.repr()))
            }
            PyObjectPayload::Str(s) => {
                let idx = key.to_int()?;
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("string index out of range")); }
                Ok(PyObject::str_val(CompactString::from(chars[actual as usize].to_string())))
            }
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                let idx = key.to_int()?;
                let len = b.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("index out of range")); }
                Ok(PyObject::int(b[actual as usize] as i64))
            }
            PyObjectPayload::InstanceDict(attrs) => {
                let key_str = key.py_to_string();
                let attrs_r = attrs.read();
                if let Some(val) = attrs_r.get(key_str.as_str()) {
                    Ok(val.clone())
                } else {
                    Err(PyException::key_error(key.repr()))
                }
            }
            _ => Err(PyException::type_error(format!("'{}' object is not subscriptable", self.type_name()))),
        }
    }

    fn contains(&self, item: &PyObjectRef) -> PyResult<bool> {
        match &self.payload {
            PyObjectPayload::List(v) => {
                let v = v.read();
                Ok(v.iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
            }
            PyObjectPayload::Tuple(v) => {
                Ok(v.iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
            }
            PyObjectPayload::Str(haystack) => {
                if let Some(needle) = item.as_str() { Ok(haystack.contains(needle)) }
                else { Err(PyException::type_error("'in <string>' requires string as left operand")) }
            }
            PyObjectPayload::Set(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.read().contains_key(&hk))
            }
            PyObjectPayload::FrozenSet(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.contains_key(&hk))
            }
            PyObjectPayload::Dict(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.read().contains_key(&hk))
            }
            PyObjectPayload::InstanceDict(attrs) => {
                let key_str = item.py_to_string();
                Ok(attrs.read().contains_key(key_str.as_str()))
            }
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                // Support: int in bytes (single byte) or bytes in bytes (subsequence)
                match &item.payload {
                    PyObjectPayload::Int(n) => {
                        let val = n.to_i64().unwrap_or(-1);
                        if val < 0 || val > 255 { return Ok(false); }
                        Ok(b.contains(&(val as u8)))
                    }
                    PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) => {
                        if needle.is_empty() { return Ok(true); }
                        Ok(b.windows(needle.len()).any(|w| w == needle.as_slice()))
                    }
                    _ => Err(PyException::type_error("a bytes-like object is required")),
                }
            }
            PyObjectPayload::Iterator(iter_data) => {
                let data = iter_data.lock().unwrap();
                match &*data {
                    IteratorData::Range { current, stop, step } => {
                        if let Some(val) = item.as_int() {
                            if *step > 0 {
                                Ok(val >= *current && val < *stop && (val - current) % step == 0)
                            } else {
                                Ok(val <= *current && val > *stop && (current - val) % (-step) == 0)
                            }
                        } else {
                            Ok(false)
                        }
                    }
                    _ => {
                        drop(data);
                        let items = self.to_list()?;
                        Ok(items.iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
                    }
                }
            }
            _ => Err(PyException::type_error(format!("argument of type '{}' is not iterable", self.type_name()))),
        }
    }

    fn get_iter(&self) -> PyResult<PyObjectRef> {
        use std::sync::Mutex;
        match &self.payload {
            PyObjectPayload::List(items) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: items.read().clone(), index: 0 }))))),
            PyObjectPayload::Tuple(items) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::Tuple { items: items.clone(), index: 0 }))))),
            PyObjectPayload::Str(s) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::Str { chars: s.chars().collect(), index: 0 }))))),
            PyObjectPayload::Dict(m) => {
                let keys: Vec<PyObjectRef> = m.read().keys().map(|k| k.to_object()).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: keys, index: 0 })))))
            }
            PyObjectPayload::Set(m) => {
                let vals: Vec<PyObjectRef> = m.read().values().cloned().collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: vals, index: 0 })))))
            }
            PyObjectPayload::FrozenSet(m) => {
                let vals: Vec<PyObjectRef> = m.values().cloned().collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: vals, index: 0 })))))
            }
            PyObjectPayload::Iterator(_) => Ok(self.clone()),
            PyObjectPayload::Generator(_) => Ok(self.clone()), // generators are their own iterators
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                let items: Vec<PyObjectRef> = b.iter().map(|byte| PyObject::int(*byte as i64)).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items, index: 0 })))))
            }
            _ => Err(PyException::type_error(format!("'{}' object is not iterable", self.type_name()))),
        }
    }

    fn format_value(&self, spec: &str) -> PyResult<String> {
        if spec.is_empty() {
            return Ok(self.py_to_string());
        }
        // Parse format spec: [[fill]align][sign][#][0][width][grouping_option][.precision][type]
        let spec_bytes = spec.as_bytes();
        let len = spec_bytes.len();

        // Handle comma grouping: {:,} or {:,d}
        if spec.contains(',') {
            let without_comma = spec.replace(',', "");
            let base_str = if without_comma.is_empty() {
                // Just {:,} — format as integer with commas
                let n = self.to_int()?;
                n.to_string()
            } else {
                self.format_value(&without_comma)?
            };
            // Apply comma grouping to the numeric part
            return Ok(add_thousands_separator(&base_str, ','));
        }
        // Handle underscore grouping: {:_} or {:_d}
        if spec.contains('_') && !spec.contains("__") {
            let without_underscore = spec.replace('_', "");
            let base_str = if without_underscore.is_empty() {
                let n = self.to_int()?;
                n.to_string()
            } else {
                self.format_value(&without_underscore)?
            };
            return Ok(add_thousands_separator(&base_str, '_'));
        }

        // Simple parsing for common cases
        let type_char = spec_bytes[len - 1] as char;
        match type_char {
            'd' => {
                let n = self.to_int()?;
                let inner_spec = &spec[..len - 1];
                if inner_spec.is_empty() {
                    return Ok(n.to_string());
                }
                return Ok(apply_string_format_spec(&n.to_string(), inner_spec));
            }
            'f' | 'F' => {
                let f = self.to_float()?;
                let inner_spec = &spec[..len - 1];
                let use_comma = inner_spec.contains(',');
                let clean_spec: String = inner_spec.chars().filter(|c| *c != ',').collect();
                if let Some(dot_pos) = clean_spec.rfind('.') {
                    let prec: usize = clean_spec[dot_pos + 1..].parse().unwrap_or(6);
                    let num_str = format!("{:.prec$}", f, prec = prec);
                    let result = if use_comma {
                        add_thousands_separator(&num_str, ',')
                    } else {
                        num_str
                    };
                    let pre_dot = &clean_spec[..dot_pos];
                    if pre_dot.is_empty() {
                        return Ok(result);
                    }
                    return Ok(apply_string_format_spec(&result, pre_dot));
                }
                let num_str = format!("{:.6}", f);
                if use_comma {
                    return Ok(add_thousands_separator(&num_str, ','));
                }
                return Ok(num_str);
            }
            'e' | 'E' => {
                let f = self.to_float()?;
                let inner_spec = &spec[..len - 1];
                let prec = if let Some(dot_pos) = inner_spec.rfind('.') {
                    inner_spec[dot_pos + 1..].parse().unwrap_or(6)
                } else { 6 };
                if type_char == 'e' {
                    return Ok(format!("{:.prec$e}", f, prec = prec));
                } else {
                    return Ok(format!("{:.prec$E}", f, prec = prec));
                }
            }
            'b' => {
                let n = self.to_int()?;
                let digits = format!("{:b}", n);
                let inner_spec = &spec[..len - 1];
                let alt = inner_spec.contains('#');
                let raw = if alt { format!("0b{}", digits) } else { digits };
                let clean_spec: String = inner_spec.chars().filter(|c| *c != '#').collect();
                if clean_spec.is_empty() { return Ok(raw); }
                return Ok(apply_string_format_spec(&raw, &clean_spec));
            }
            'o' => {
                let n = self.to_int()?;
                let digits = format!("{:o}", n);
                let inner_spec = &spec[..len - 1];
                let alt = inner_spec.contains('#');
                let raw = if alt { format!("0o{}", digits) } else { digits };
                let clean_spec: String = inner_spec.chars().filter(|c| *c != '#').collect();
                if clean_spec.is_empty() { return Ok(raw); }
                return Ok(apply_string_format_spec(&raw, &clean_spec));
            }
            'x' => {
                let n = self.to_int()?;
                let digits = format!("{:x}", n);
                let inner_spec = &spec[..len - 1];
                let alt = inner_spec.contains('#');
                let raw = if alt { format!("0x{}", digits) } else { digits };
                let clean_spec: String = inner_spec.chars().filter(|c| *c != '#').collect();
                if clean_spec.is_empty() { return Ok(raw); }
                return Ok(apply_string_format_spec(&raw, &clean_spec));
            }
            'X' => {
                let n = self.to_int()?;
                let digits = format!("{:X}", n);
                let inner_spec = &spec[..len - 1];
                let alt = inner_spec.contains('#');
                let raw = if alt { format!("0X{}", digits) } else { digits };
                let clean_spec: String = inner_spec.chars().filter(|c| *c != '#').collect();
                if clean_spec.is_empty() { return Ok(raw); }
                return Ok(apply_string_format_spec(&raw, &clean_spec));
            }
            's' => {
                let s = self.py_to_string();
                let inner_spec = &spec[..len - 1];
                if inner_spec.is_empty() { return Ok(s); }
                return Ok(apply_string_format_spec(&s, inner_spec));
            }
            _ => {
                // No type char — treat entire spec as alignment spec
                let s = self.py_to_string();
                return Ok(apply_string_format_spec(&s, spec));
            }
        }
    }

    fn dir(&self) -> Vec<CompactString> {
        match &self.payload {
            PyObjectPayload::Instance(inst) => {
                let mut names: Vec<CompactString> = inst.attrs.read().keys().cloned().collect();
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    names.extend(cd.namespace.read().keys().cloned());
                }
                names.sort(); names.dedup(); names
            }
            PyObjectPayload::Class(cd) => { let mut n: Vec<_> = cd.namespace.read().keys().cloned().collect(); n.sort(); n }
            PyObjectPayload::Module(m) => { let mut n: Vec<_> = m.attrs.keys().cloned().collect(); n.sort(); n }
            _ => vec![],
        }
    }
}

/// Resolve methods on builtin ExceptionType bases (e.g. Exception.__init__).
/// Used by super() proxy when the parent class is a builtin exception type.
fn resolve_exception_type_method(name: &str, _instance: &PyObjectRef) -> Option<PyObjectRef> {
    match name {
        "__init__" => {
            Some(PyObject::native_function("__init__", |args| {
                // Exception.__init__(self, *args) — set self.args and self.message
                if args.is_empty() { return Ok(PyObject::none()); }
                let target = &args[0];
                if let PyObjectPayload::Instance(idata) = &target.payload {
                    let mut attrs = idata.attrs.write();
                    let exc_args: Vec<PyObjectRef> = if args.len() > 1 {
                        args[1..].to_vec()
                    } else {
                        vec![]
                    };
                    if !exc_args.is_empty() {
                        attrs.insert(CompactString::from("message"), exc_args[0].clone());
                    }
                    attrs.insert(CompactString::from("args"), PyObject::tuple(exc_args));
                }
                Ok(PyObject::none())
            }))
        }
        "__str__" => {
            Some(PyObject::native_function("__str__", |args| {
                if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
                let target = &args[0];
                if let Some(msg) = target.get_attr("message") {
                    return Ok(PyObject::str_val(CompactString::from(msg.py_to_string())));
                }
                if let Some(a) = target.get_attr("args") {
                    if let PyObjectPayload::Tuple(items) = &a.payload {
                        if items.len() == 1 {
                            return Ok(PyObject::str_val(CompactString::from(items[0].py_to_string())));
                        }
                    }
                    return Ok(PyObject::str_val(CompactString::from(a.py_to_string())));
                }
                Ok(PyObject::str_val(CompactString::from(String::new())))
            }))
        }
        "__repr__" => {
            Some(PyObject::native_function("__repr__", |args| {
                if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("Exception()"))); }
                let target = &args[0];
                let cls_name = if let PyObjectPayload::Instance(idata) = &target.payload {
                    if let PyObjectPayload::Class(cd) = &idata.class.payload {
                        cd.name.to_string()
                    } else { "Exception".to_string() }
                } else { "Exception".to_string() };
                if let Some(a) = target.get_attr("args") {
                    Ok(PyObject::str_val(CompactString::from(format!("{}({})", cls_name, a.py_to_string()))))
                } else {
                    Ok(PyObject::str_val(CompactString::from(format!("{}()", cls_name))))
                }
            }))
        }
        _ => None,
    }
}
