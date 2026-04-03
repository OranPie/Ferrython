//! Type introspection & conversion methods.

use crate::error::{PyException, PyResult};
use crate::types::HashableKey;
use compact_str::CompactString;
use std::sync::Arc;

use super::payload::*;
use super::helpers::*;
use super::methods::PyObjectMethods;

pub(super) fn py_type_name(obj: &PyObjectRef) -> &'static str {
        match &obj.payload {
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
            PyObjectPayload::Range { .. } => "range",
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

pub(super) fn py_is_truthy(obj: &PyObjectRef) -> bool {
        match &obj.payload {
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

pub(super) fn py_is_callable(obj: &PyObjectRef) -> bool {
        matches!(&obj.payload, PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
            | PyObjectPayload::BuiltinType(_) | PyObjectPayload::BoundMethod { .. }
            | PyObjectPayload::BuiltinBoundMethod { .. }
            | PyObjectPayload::Class(_) | PyObjectPayload::ExceptionType(_)
            | PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. } | PyObjectPayload::Partial { .. })
            || (matches!(&obj.payload, PyObjectPayload::Instance(_)) && obj.get_attr("__call__").is_some())
}

pub(super) fn py_is_same(a: &PyObjectRef, b: &PyObjectRef) -> bool {
        Arc::ptr_eq(a, b) || matches!(
            (&a.payload, &b.payload),
            (PyObjectPayload::BuiltinType(a), PyObjectPayload::BuiltinType(b)) if a == b
        )
}

pub(super) fn py_to_string(obj: &PyObjectRef) -> String {
        match &obj.payload {
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
                // Dict subclass: display like a dict
                if let Some(ref ds) = inst.dict_storage {
                    return format_dict(&ds.read());
                }
                // Known instance types with custom __str__
                {
                    let attrs = inst.attrs.read();
                    // pathlib.Path → return _path string
                    if attrs.contains_key("__pathlib_path__") {
                        return attrs.get("_path").map(|v| v.py_to_string()).unwrap_or_default();
                    }
                    // datetime/date → format date string
                    if attrs.contains_key("__datetime__") {
                        let year = attrs.get("year").and_then(|v| v.as_int()).unwrap_or(1970);
                        let month = attrs.get("month").and_then(|v| v.as_int()).unwrap_or(1);
                        let day = attrs.get("day").and_then(|v| v.as_int()).unwrap_or(1);
                        if attrs.contains_key("__date_only__") {
                            return format!("{:04}-{:02}-{:02}", year, month, day);
                        }
                        let hour = attrs.get("hour").and_then(|v| v.as_int()).unwrap_or(0);
                        let minute = attrs.get("minute").and_then(|v| v.as_int()).unwrap_or(0);
                        let second = attrs.get("second").and_then(|v| v.as_int()).unwrap_or(0);
                        return format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, hour, minute, second);
                    }
                    // timedelta
                    if attrs.contains_key("__timedelta__") {
                        let days = attrs.get("days").and_then(|v| v.as_int()).unwrap_or(0);
                        let secs = attrs.get("seconds").and_then(|v| v.as_int()).unwrap_or(0);
                        let h = secs / 3600;
                        let m = (secs % 3600) / 60;
                        let s = secs % 60;
                        return if days != 0 {
                            format!("{} day{}, {}:{:02}:{:02}", days, if days.abs() != 1 { "s" } else { "" }, h, m, s)
                        } else {
                            format!("{}:{:02}:{:02}", h, m, s)
                        };
                    }
                }
                // Check for __str__ method first
                if let Some(str_fn) = obj.get_attr("__str__") {
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
            PyObjectPayload::Range { start, stop, step } => {
                if *step == 1 { format!("range({}, {})", start, stop) }
                else { format!("range({}, {}, {})", start, stop, step) }
            }
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
            _ => format!("<{}>", obj.type_name()),
        }
}

pub(super) fn py_repr(obj: &PyObjectRef) -> String {
        match &obj.payload {
            PyObjectPayload::Str(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
            PyObjectPayload::ExceptionInstance { kind, message, .. } => {
                if message.is_empty() {
                    format!("{}()", kind)
                } else {
                    format!("{}('{}')", kind, message)
                }
            }
            PyObjectPayload::Instance(inst) => {
                // Dict subclass: repr like a dict
                if let Some(ref ds) = inst.dict_storage {
                    let items: Vec<String> = ds.read().iter()
                        .map(|(k, v)| format!("{}: {}", k.to_object().repr(), v.repr()))
                        .collect();
                    return format!("{{{}}}", items.join(", "));
                }
                // Check for __repr__ first
                if let Some(_) = obj.get_attr("__repr__") {
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
                obj.py_to_string()
            }
            _ => obj.py_to_string(),
        }
}

pub(super) fn py_to_list(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
        match &obj.payload {
            PyObjectPayload::List(v) => Ok(v.read().clone()),
            PyObjectPayload::Tuple(v) => Ok(v.clone()),
            PyObjectPayload::Set(m) => Ok(m.read().values().cloned().collect()),
            PyObjectPayload::FrozenSet(m) => Ok(m.values().cloned().collect()),
            PyObjectPayload::Str(s) => Ok(s.chars().map(|c| PyObject::str_val(CompactString::from(c.to_string()))).collect()),
            PyObjectPayload::Dict(m) => Ok(m.read().keys().map(|k| k.to_object()).collect()),
            PyObjectPayload::Instance(inst) if inst.dict_storage.is_some() => {
                Ok(inst.dict_storage.as_ref().unwrap().read().keys().map(|k| k.to_object()).collect())
            }
            PyObjectPayload::InstanceDict(attrs) => Ok(attrs.read().keys().map(|k| PyObject::str_val(k.clone())).collect()),
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                Ok(b.iter().map(|byte| PyObject::int(*byte as i64)).collect())
            }
            PyObjectPayload::Range { start, stop, step } => {
                let mut result = Vec::new();
                let mut val = *start;
                while (*step > 0 && val < *stop) || (*step < 0 && val > *stop) {
                    result.push(PyObject::int(val));
                    val += step;
                }
                Ok(result)
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
                    | IteratorData::Sentinel { .. }
                    | IteratorData::TakeWhile { .. }
                    | IteratorData::DropWhile { .. } => {
                        Err(PyException::type_error("lazy iterator requires VM to collect"))
                    }
                }
            }
            // namedtuple instances: convert _tuple to list
            PyObjectPayload::Instance(inst) if inst.class.get_attr("__namedtuple__").is_some() => {
                if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                    if let PyObjectPayload::Tuple(items) = &tup.payload {
                        return Ok(items.clone());
                    }
                }
                Ok(vec![])
            }
            _ => Err(PyException::type_error(format!("'{}' object is not iterable", obj.type_name()))),
        }
}

pub(super) fn py_to_int(obj: &PyObjectRef) -> PyResult<i64> {
        match &obj.payload {
            PyObjectPayload::Int(n) => n.to_i64().ok_or_else(|| PyException::overflow_error("int too large")),
            PyObjectPayload::Bool(b) => Ok(if *b { 1 } else { 0 }),
            PyObjectPayload::Float(f) => Ok(*f as i64),
            PyObjectPayload::Str(s) => s.trim().parse::<i64>().map_err(|_|
                PyException::value_error(format!("invalid literal for int(): '{}'", s))),
            PyObjectPayload::Instance(_) => {
                // Check for __int__ or __index__ on the instance
                if let Some(int_val) = obj.get_attr("__int__") {
                    if let Some(v) = int_val.as_int() { return Ok(v); }
                }
                if let Some(idx_val) = obj.get_attr("__index__") {
                    if let Some(v) = idx_val.as_int() { return Ok(v); }
                }
                Err(PyException::type_error(format!("int() argument must be a string or number, not '{}'", obj.type_name())))
            }
            _ => Err(PyException::type_error(format!("int() argument must be a string or number, not '{}'", obj.type_name()))),
        }
}

pub(super) fn py_to_float(obj: &PyObjectRef) -> PyResult<f64> {
        match &obj.payload {
            PyObjectPayload::Float(f) => Ok(*f),
            PyObjectPayload::Int(n) => Ok(n.to_f64()),
            PyObjectPayload::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            PyObjectPayload::Str(s) => s.trim().parse::<f64>().map_err(|_|
                PyException::value_error(format!("could not convert string to float: '{}'", s))),
            PyObjectPayload::Instance(_) => {
                if let Some(float_val) = obj.get_attr("__float__") {
                    if let PyObjectPayload::Float(f) = &float_val.payload {
                        return Ok(*f);
                    }
                }
                Err(PyException::type_error(format!("float() argument must be a string or number, not '{}'", obj.type_name())))
            }
            _ => Err(PyException::type_error(format!("float() argument must be a string or number, not '{}'", obj.type_name()))),
        }
}

pub(super) fn py_as_int(obj: &PyObjectRef) -> Option<i64> {
        match &obj.payload {
            PyObjectPayload::Int(n) => n.to_i64(),
            PyObjectPayload::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
}

pub(super) fn py_as_str(obj: &PyObjectRef) -> Option<&str> {
        match &obj.payload {
            PyObjectPayload::Str(s) => Some(s.as_str()),
            _ => None,
        }
}

pub(super) fn py_to_hashable_key(obj: &PyObjectRef) -> PyResult<HashableKey> {
    HashableKey::from_object(obj)
}
