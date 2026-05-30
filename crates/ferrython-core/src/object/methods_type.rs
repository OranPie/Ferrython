//! Type introspection & conversion methods.

mod to_list;

pub(super) use to_list::py_to_list;

use crate::error::{ExceptionKind, PyException, PyResult};
use crate::types::{HashableKey, PyInt};

use super::helpers::*;
use super::methods::PyObjectMethods;
use super::payload::*;

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
        PyObjectPayload::MappingProxy(_) => "mappingproxy",
        PyObjectPayload::Function(_) => "function",
        PyObjectPayload::BuiltinFunction(_) => "builtin_function_or_method",
        PyObjectPayload::BuiltinType(_) => "type",
        PyObjectPayload::BoundMethod { .. } => "method",
        PyObjectPayload::BuiltinBoundMethod(_) => "builtin_method",
        PyObjectPayload::Code(_) => "code",
        PyObjectPayload::Class(_) => "type",
        PyObjectPayload::Instance(inst) => {
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                Box::leak(cd.name.to_string().into_boxed_str())
            } else {
                "object"
            }
        }
        PyObjectPayload::Module(_) => "module",
        PyObjectPayload::RangeIter(..) => "range_iterator",
        PyObjectPayload::VecIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::RefIter { .. } => "list_iterator",
        PyObjectPayload::DequeIter(data) => {
            if data.reverse {
                "_deque_reverse_iterator"
            } else {
                "_deque_iterator"
            }
        }
        PyObjectPayload::RevRefIter { .. } => "list_reverseiterator",
        PyObjectPayload::Iterator(iter_data) => {
            let guard = iter_data.read();
            match &*guard {
                IteratorData::MapOne { .. } | IteratorData::Map { .. } => "map",
                IteratorData::Filter { .. } => "filter",
                IteratorData::FilterFalse { .. } => "itertools.filterfalse",
                IteratorData::Zip { .. } => "zip",
                IteratorData::ZipLongest { .. } => "itertools.zip_longest",
                IteratorData::Islice { .. } => "itertools.islice",
                IteratorData::Enumerate { .. } => "enumerate",
                IteratorData::Sentinel { .. } => "callable_iterator",
                IteratorData::TakeWhile { .. } => "itertools.takewhile",
                IteratorData::DropWhile { .. } => "itertools.dropwhile",
                IteratorData::Count { .. } => "itertools.count",
                IteratorData::Cycle { .. } => "itertools.cycle",
                IteratorData::Repeat { .. } => "itertools.repeat",
                IteratorData::Chain { .. } => "itertools.chain",
                IteratorData::Starmap { .. } => "itertools.starmap",
                IteratorData::Tee { .. } => "itertools._tee",
                IteratorData::HeldIter { .. } => "iterator",
                IteratorData::Range { .. } | IteratorData::BigRange(_) => "range_iterator",
                IteratorData::List { .. } => "list_iterator",
                IteratorData::Tuple { .. } => "tuple_iterator",
                IteratorData::Str { .. } => "str_ascii_iterator",
                IteratorData::DictEntries { .. } => "dict_itemiterator",
                IteratorData::DictKeys { .. } | IteratorData::DictKeyRefs { .. } => {
                    "dict_keyiterator"
                }
                IteratorData::SeqIter { .. } => "iterator",
            }
        }
        PyObjectPayload::Range { .. } => "range",
        PyObjectPayload::Slice { .. } => "slice",
        PyObjectPayload::Cell(_) => "cell",
        PyObjectPayload::ExceptionType(_) => "type",
        PyObjectPayload::ExceptionInstance(_) => "exception",
        PyObjectPayload::Generator(_) => "generator",
        PyObjectPayload::Coroutine(_) => "coroutine",
        PyObjectPayload::AsyncGenerator(_) => "async_generator",
        PyObjectPayload::AsyncGenAwaitable { .. } => "async_generator_asend",
        PyObjectPayload::NativeFunction(_) => "builtin_function_or_method",
        PyObjectPayload::NativeClosure(_) => "builtin_function_or_method",
        PyObjectPayload::Property(_) => "property",
        PyObjectPayload::StaticMethod(_) => "staticmethod",
        PyObjectPayload::ClassMethod(_) => "classmethod",
        PyObjectPayload::Super { .. } => "super",
        PyObjectPayload::Partial(_) => "functools.partial",
        PyObjectPayload::InstanceDict(_) => "dict",
        PyObjectPayload::BuiltinAwaitable(_) => "coroutine",
        PyObjectPayload::DeferredSleep { .. } => "coroutine",
        PyObjectPayload::DictKeys { .. } => "dict_keys",
        PyObjectPayload::DictValues { .. } => "dict_values",
        PyObjectPayload::DictItems { .. } => "dict_items",
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
        PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => !m.read().is_empty(),
        PyObjectPayload::Range(rd) => !super::helpers::range_data_is_empty(rd),
        PyObjectPayload::DictKeys { map, .. }
        | PyObjectPayload::DictValues { map, .. }
        | PyObjectPayload::DictItems { map, .. } => !map.read().is_empty(),
        PyObjectPayload::Instance(inst) => {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                    return (nc.func)(&[]).map_or(false, |referent| referent.is_truthy());
                }
            }
            if inst.attrs.read().contains_key("__deque__") {
                if let Some(data) = inst.attrs.read().get("_data").cloned() {
                    if let PyObjectPayload::List(items) = &data.payload {
                        return !items.read().is_empty();
                    }
                }
                return false;
            }
            // Builtin base type subclass: delegate truthiness to __builtin_value__
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                return bv.is_truthy();
            }
            // Dict subclass: delegate to dict_storage
            if let Some(ref ds) = inst.dict_storage {
                return !ds.read().is_empty();
            }
            true
        }
        _ => true,
    }
}

pub(super) fn py_is_callable(obj: &PyObjectRef) -> bool {
    matches!(
        &obj.payload,
        PyObjectPayload::Function(_)
            | PyObjectPayload::BuiltinFunction(_)
            | PyObjectPayload::BuiltinType(_)
            | PyObjectPayload::BoundMethod { .. }
            | PyObjectPayload::BuiltinBoundMethod(_)
            | PyObjectPayload::Class(_)
            | PyObjectPayload::ExceptionType(_)
            | PyObjectPayload::NativeFunction(_)
            | PyObjectPayload::NativeClosure(_)
            | PyObjectPayload::Partial(_)
    ) || (matches!(&obj.payload, PyObjectPayload::Instance(_))
        && obj.get_attr("__call__").is_some())
}

pub(super) fn py_is_same(a: &PyObjectRef, b: &PyObjectRef) -> bool {
    PyObjectRef::ptr_eq(a, b)
        || matches!(
            (&a.payload, &b.payload),
            (PyObjectPayload::BuiltinType(a), PyObjectPayload::BuiltinType(b)) if a == b
        )
        || matches!(
            (&a.payload, &b.payload),
            (PyObjectPayload::ExceptionType(a), PyObjectPayload::ExceptionType(b)) if a == b
        )
}

fn exception_field(
    ei: &ExceptionInstanceData,
    name: &str,
    arg_index: usize,
) -> Option<PyObjectRef> {
    ei.get_attrs()
        .and_then(|attrs| attrs.read().get(name).cloned())
        .or_else(|| ei.args.get(arg_index).cloned())
}

fn exception_field_i64(ei: &ExceptionInstanceData, name: &str, arg_index: usize) -> Option<i64> {
    exception_field(ei, name, arg_index).and_then(|value| value.as_int())
}

fn escaped_unicode_char(ch: char) -> String {
    let code = ch as u32;
    if code <= 0xff {
        format!("\\x{:02x}", code)
    } else if code <= 0xffff {
        format!("\\u{:04x}", code)
    } else {
        format!("\\U{:08x}", code)
    }
}

fn str_char_at(obj: &PyObjectRef, index: i64) -> Option<String> {
    let idx = usize::try_from(index).ok()?;
    match &obj.payload {
        PyObjectPayload::Str(s) => s.to_string().chars().nth(idx).map(escaped_unicode_char),
        _ => None,
    }
}

fn byte_at(obj: &PyObjectRef, index: i64) -> Option<u8> {
    let idx = usize::try_from(index).ok()?;
    match &obj.payload {
        PyObjectPayload::Bytes(bytes) => bytes.get(idx).copied(),
        PyObjectPayload::ByteArray(bytes) => bytes.get(idx).copied(),
        _ => None,
    }
}

fn unicode_error_to_string(ei: &ExceptionInstanceData) -> Option<String> {
    let reason_arg = match ei.kind {
        ExceptionKind::UnicodeEncodeError | ExceptionKind::UnicodeDecodeError => 4,
        ExceptionKind::UnicodeTranslateError => 3,
        _ => return None,
    };

    let object_arg = if ei.kind == ExceptionKind::UnicodeTranslateError {
        0
    } else {
        1
    };
    let start_arg = if ei.kind == ExceptionKind::UnicodeTranslateError {
        1
    } else {
        2
    };
    let end_arg = if ei.kind == ExceptionKind::UnicodeTranslateError {
        2
    } else {
        3
    };

    let reason = match exception_field(ei, "reason", reason_arg) {
        Some(value) => value.py_to_string(),
        None => return Some(String::new()),
    };
    let start = match exception_field_i64(ei, "start", start_arg) {
        Some(value) => value,
        None => return Some(String::new()),
    };
    let end = match exception_field_i64(ei, "end", end_arg) {
        Some(value) => value,
        None => return Some(String::new()),
    };

    let single = end == start.saturating_add(1);
    let range = if single {
        format!("position {}", start)
    } else {
        format!("position {}-{}", start, end.saturating_sub(1))
    };

    match ei.kind {
        ExceptionKind::UnicodeEncodeError => {
            let encoding = match exception_field(ei, "encoding", 0) {
                Some(value) => value.py_to_string(),
                None => return Some(String::new()),
            };
            if single {
                let object = exception_field(ei, "object", object_arg);
                let ch = object
                    .as_ref()
                    .and_then(|obj| str_char_at(obj, start))
                    .unwrap_or_else(|| "?".to_string());
                Some(format!(
                    "'{}' codec can't encode character '{}' in {}: {}",
                    encoding, ch, range, reason
                ))
            } else {
                Some(format!(
                    "'{}' codec can't encode characters in {}: {}",
                    encoding, range, reason
                ))
            }
        }
        ExceptionKind::UnicodeDecodeError => {
            let encoding = match exception_field(ei, "encoding", 0) {
                Some(value) => value.py_to_string(),
                None => return Some(String::new()),
            };
            if single {
                let object = exception_field(ei, "object", object_arg);
                let byte = object
                    .as_ref()
                    .and_then(|obj| byte_at(obj, start))
                    .unwrap_or(0);
                Some(format!(
                    "'{}' codec can't decode byte 0x{:02x} in {}: {}",
                    encoding, byte, range, reason
                ))
            } else {
                Some(format!(
                    "'{}' codec can't decode bytes in {}: {}",
                    encoding, range, reason
                ))
            }
        }
        ExceptionKind::UnicodeTranslateError => {
            if single {
                let object = exception_field(ei, "object", object_arg);
                let ch = object
                    .as_ref()
                    .and_then(|obj| str_char_at(obj, start))
                    .unwrap_or_else(|| "?".to_string());
                Some(format!(
                    "can't translate character '{}' in {}: {}",
                    ch, range, reason
                ))
            } else {
                Some(format!(
                    "can't translate characters in {}: {}",
                    range, reason
                ))
            }
        }
        _ => None,
    }
}

pub(super) fn py_to_string(obj: &PyObjectRef) -> String {
    match &obj.payload {
        PyObjectPayload::None => "None".into(),
        PyObjectPayload::Bool(true) => "True".into(),
        PyObjectPayload::Bool(false) => "False".into(),
        PyObjectPayload::Int(n) => n.to_string(),
        PyObjectPayload::Float(f) => float_to_str(*f),
        PyObjectPayload::Complex { real, imag } => {
            fn ff(f: f64) -> String {
                if f.is_nan() {
                    return "nan".into();
                }
                if f.is_infinite() {
                    return if f > 0.0 { "inf".into() } else { "-inf".into() };
                }
                if f == 0.0 {
                    return if f.is_sign_negative() {
                        "-0".into()
                    } else {
                        "0".into()
                    };
                }
                if f == f.trunc() && f.abs() < 1e16 {
                    return format!("{}", f as i64);
                }
                let s = format!("{}", f);
                if s.contains('.') || s.contains('e') || s.contains('E') {
                    s
                } else {
                    format!("{}.0", s)
                }
            }
            let is_real_zero = *real == 0.0 && !real.is_sign_negative();
            if is_real_zero {
                format!("{}j", ff(*imag))
            } else {
                let imag_str = ff(*imag);
                let sep = if imag_str.starts_with('-') || imag_str.starts_with('+') {
                    ""
                } else {
                    "+"
                };
                format!("({}{}{}j)", ff(*real), sep, imag_str)
            }
        }
        PyObjectPayload::Str(s) => s.to_string(),
        PyObjectPayload::Bytes(b) => format_bytes_literal(b, "b"),
        PyObjectPayload::ByteArray(b) => {
            format!("bytearray({})", format_bytes_literal(b, "b"))
        }
        PyObjectPayload::List(items) => {
            let ptr = PyObjectRef::as_ptr(obj) as usize;
            if !repr_enter(ptr) {
                return "[...]".into();
            }
            let result = format_collection("[", "]", &items.read());
            repr_leave(ptr);
            result
        }
        PyObjectPayload::Tuple(items) => {
            if items.len() == 1 {
                format!("({},)", items[0].repr())
            } else {
                format_collection("(", ")", items)
            }
        }
        PyObjectPayload::Set(m) => {
            let m = m.read();
            if m.is_empty() {
                "set()".into()
            } else {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                if !repr_enter(ptr) {
                    return "set(...)".into();
                }
                let result = format_set_flat("{", "}", &m);
                repr_leave(ptr);
                result
            }
        }
        PyObjectPayload::FrozenSet(m) => {
            if m.is_empty() {
                "frozenset()".into()
            } else {
                format!("frozenset({})", format_set("{", "}", m))
            }
        }
        PyObjectPayload::Dict(m) => {
            let ptr = PyObjectRef::as_ptr(obj) as usize;
            if !repr_enter(ptr) {
                return "{...}".into();
            }
            let result = format_dict(&m.read());
            repr_leave(ptr);
            result
        }
        PyObjectPayload::MappingProxy(m) => {
            let inner = format_dict(&m.read());
            format!("mappingproxy({})", inner)
        }
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
        PyObjectPayload::Function(f) => format!("<function {}>", f.qualname),
        PyObjectPayload::BuiltinFunction(n) => format!("<built-in function {}>", n),
        PyObjectPayload::BuiltinType(n) => format!("<class '{}'>", n),
        PyObjectPayload::Code(c) => format!("<code object {}>", c.name),
        PyObjectPayload::Class(cd) => {
            let ns = cd.namespace.read();
            let module = ns
                .get("__module__")
                .map(|v| v.py_to_string())
                .unwrap_or_else(|| "__main__".to_string());
            let qualname = ns
                .get("__qualname__")
                .map(|v| v.py_to_string())
                .unwrap_or_else(|| cd.name.to_string());
            format!("<class '{}.{}'>", module, qualname)
        }
        PyObjectPayload::Instance(inst) => {
            // Builtin base type subclass: delegate to __builtin_value__
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                return bv.py_to_string();
            }
            // Dict subclass: display like a dict
            if let Some(ref ds) = inst.dict_storage {
                return format_dict(&ds.read());
            }
            // namedtuple: str() should match repr() — e.g., Point(x=1, y=2)
            if inst.class.get_attr("__namedtuple__").is_some() {
                if let Some(fields) = inst.class.get_attr("_fields") {
                    if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                        let attrs = inst.attrs.read();
                        let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            cd.name.to_string()
                        } else {
                            "namedtuple".to_string()
                        };
                        let parts: Vec<String> = field_names
                            .iter()
                            .map(|f| {
                                let name = f.py_to_string();
                                let val = attrs
                                    .get(name.as_str())
                                    .map(|v| v.repr())
                                    .unwrap_or_else(|| "None".to_string());
                                format!("{}={}", name, val)
                            })
                            .collect();
                        return format!("{}({})", class_name, parts.join(", "));
                    }
                }
            }
            // Known instance types with custom __str__
            {
                let attrs = inst.attrs.read();
                // typing _GenericAlias → return typing repr
                if let Some(typing_repr) = attrs.get("__typing_repr__") {
                    return typing_repr.py_to_string();
                }
                // pathlib.Path → return _path string
                if attrs.contains_key("__pathlib_path__") {
                    return attrs
                        .get("_path")
                        .map(|v| v.py_to_string())
                        .unwrap_or_default();
                }
                // datetime/date/time → format string
                if attrs.contains_key("__datetime__") {
                    if attrs.contains_key("__time_only__") {
                        let hour = attrs.get("hour").and_then(|v| v.as_int()).unwrap_or(0);
                        let minute = attrs.get("minute").and_then(|v| v.as_int()).unwrap_or(0);
                        let second = attrs.get("second").and_then(|v| v.as_int()).unwrap_or(0);
                        return format!("{:02}:{:02}:{:02}", hour, minute, second);
                    }
                    let year = attrs.get("year").and_then(|v| v.as_int()).unwrap_or(1970);
                    let month = attrs.get("month").and_then(|v| v.as_int()).unwrap_or(1);
                    let day = attrs.get("day").and_then(|v| v.as_int()).unwrap_or(1);
                    if attrs.contains_key("__date_only__") {
                        return format!("{:04}-{:02}-{:02}", year, month, day);
                    }
                    let hour = attrs.get("hour").and_then(|v| v.as_int()).unwrap_or(0);
                    let minute = attrs.get("minute").and_then(|v| v.as_int()).unwrap_or(0);
                    let second = attrs.get("second").and_then(|v| v.as_int()).unwrap_or(0);
                    let base = format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                        year, month, day, hour, minute, second
                    );
                    // Clone tzinfo out so we can drop the outer attrs lock before reading it
                    let tz_cloned = attrs.get("tzinfo").cloned();
                    drop(attrs);
                    if let Some(ref tz) = tz_cloned {
                        if !matches!(&tz.payload, PyObjectPayload::None) {
                            if let PyObjectPayload::Instance(ref tz_inst) = tz.payload {
                                let tz_attrs = tz_inst.attrs.read();
                                let offset_secs = tz_attrs
                                    .get("_offset_seconds")
                                    .and_then(|v| match &v.payload {
                                        PyObjectPayload::Float(f) => Some(*f as i64),
                                        PyObjectPayload::Int(i) => i.to_i64(),
                                        _ => None,
                                    })
                                    .unwrap_or(0);
                                let sign = if offset_secs < 0 { '-' } else { '+' };
                                let abs_secs = offset_secs.unsigned_abs();
                                let oh = abs_secs / 3600;
                                let om = (abs_secs % 3600) / 60;
                                return format!("{}{}{:02}:{:02}", base, sign, oh, om);
                            }
                        }
                    }
                    return base;
                }
                // timedelta
                if attrs.contains_key("__timedelta__") {
                    let days = attrs.get("days").and_then(|v| v.as_int()).unwrap_or(0);
                    let secs = attrs.get("seconds").and_then(|v| v.as_int()).unwrap_or(0);
                    let h = secs / 3600;
                    let m = (secs % 3600) / 60;
                    let s = secs % 60;
                    return if days != 0 {
                        format!(
                            "{} day{}, {}:{:02}:{:02}",
                            days,
                            if days.abs() != 1 { "s" } else { "" },
                            h,
                            m,
                            s
                        )
                    } else {
                        format!("{}:{:02}:{:02}", h, m, s)
                    };
                }
                // Decimal → return _value string
                if attrs.contains_key("__decimal__") {
                    return attrs
                        .get("_value")
                        .map(|v| v.py_to_string())
                        .unwrap_or_else(|| "0".to_string());
                }
                // Fraction → return n/d string
                if attrs.contains_key("__fraction__") {
                    let n = attrs.get("numerator").and_then(|v| v.as_int()).unwrap_or(0);
                    let d = attrs
                        .get("denominator")
                        .and_then(|v| v.as_int())
                        .unwrap_or(1);
                    return if d == 1 {
                        format!("{}", n)
                    } else {
                        format!("{}/{}", n, d)
                    };
                }
                // UUID → return formatted UUID string
                if attrs.contains_key("__uuid__") {
                    if let Some(v) = attrs.get("__str_val__") {
                        return v.py_to_string();
                    }
                }
                // Deque → display as deque([items])
                if attrs.contains_key("__deque__") {
                    if let Some(data) = attrs.get("_data") {
                        let maxlen = attrs.get("__maxlen__");
                        let items_str = data.py_to_string();
                        return match maxlen {
                            Some(ml) if !matches!(&ml.payload, PyObjectPayload::None) => {
                                format!("deque({}, maxlen={})", items_str, ml.py_to_string())
                            }
                            _ => format!("deque({})", items_str),
                        };
                    }
                }
            }
            // Check for __str__ method first
            if let Some(str_fn) = obj.get_attr("__str__") {
                if !matches!(&str_fn.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
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
            } else {
                "<object>".into()
            }
        }
        PyObjectPayload::Module(m) => {
            // Check for custom __repr__ NativeClosure (e.g. socket objects)
            let attrs = m.attrs.read();
            if let Some(repr_fn) = attrs.get("__repr__") {
                if let PyObjectPayload::NativeClosure(nc) = &repr_fn.payload {
                    if let Ok(result) = (nc.func)(&[obj.clone()]) {
                        return result.py_to_string();
                    }
                    if let Ok(result) = (nc.func)(&[]) {
                        return result.py_to_string();
                    }
                }
            }
            if let Some(file) = attrs.get("__file__") {
                format!("<module '{}' from '{}'>", m.name, file.py_to_string())
            } else if m.name.as_str() == "sys" || m.name.as_str() == "builtins" {
                format!("<module '{}' (built-in)>", m.name)
            } else {
                format!("<module '{}'>", m.name)
            }
        }
        PyObjectPayload::Iterator(_) => "<iterator>".into(),
        PyObjectPayload::VecIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::DequeIter(_)
        | PyObjectPayload::RefIter { .. } => "<iterator>".into(),
        PyObjectPayload::RevRefIter { .. } => "<iterator>".into(),
        PyObjectPayload::Range(rd) => {
            if rd.step == 1 {
                format!("range({}, {})", rd.start, rd.stop)
            } else {
                format!("range({}, {}, {})", rd.start, rd.stop, rd.step)
            }
        }
        PyObjectPayload::ExceptionType(kind) => format!("<class '{}'>", kind),
        PyObjectPayload::ExceptionInstance(ei) => {
            if let Some(text) = unicode_error_to_string(ei) {
                return text;
            }
            // KeyError wraps its argument in repr() for str()
            if ei.kind == crate::error::ExceptionKind::KeyError && ei.args.len() == 1 {
                return ei.args[0].repr();
            }
            // CPython: str(e) with multiple args returns repr of the args tuple
            if ei.args.len() > 1 {
                let items: Vec<String> = ei.args.iter().map(|a| a.repr()).collect();
                return format!("({})", items.join(", "));
            }
            if ei.message.is_empty() {
                String::new()
            } else {
                ei.message.as_str().to_owned()
            }
        }
        PyObjectPayload::BoundMethod { receiver, method } => bound_method_repr(receiver, method),
        PyObjectPayload::DictKeys { map, .. } => {
            let ptr = map.as_ref() as *const PyCell<FxHashKeyMap> as usize;
            if !repr_enter(ptr) {
                return "dict_keys([...])".into();
            }
            let keys: Vec<String> = map
                .read()
                .keys()
                .filter(|k| !is_hidden_dict_key(k))
                .map(|k| k.to_object().repr())
                .collect();
            let result = format!("dict_keys([{}])", keys.join(", "));
            repr_leave(ptr);
            result
        }
        PyObjectPayload::DictValues { map, .. } => {
            let ptr = map.as_ref() as *const PyCell<FxHashKeyMap> as usize;
            if !repr_enter(ptr) {
                return "dict_values([...])".into();
            }
            let vals: Vec<String> = map
                .read()
                .iter()
                .filter(|(k, _)| !is_hidden_dict_key(k))
                .map(|(_, v)| v.repr())
                .collect();
            let result = format!("dict_values([{}])", vals.join(", "));
            repr_leave(ptr);
            result
        }
        PyObjectPayload::DictItems { map, .. } => {
            let ptr = map.as_ref() as *const PyCell<FxHashKeyMap> as usize;
            if !repr_enter(ptr) {
                return "dict_items([...])".into();
            }
            let items: Vec<String> = map
                .read()
                .iter()
                .filter(|(k, _)| !is_hidden_dict_key(k))
                .map(|(k, v)| format!("({}, {})", k.to_object().repr(), v.repr()))
                .collect();
            let result = format!("dict_items([{}])", items.join(", "));
            repr_leave(ptr);
            result
        }
        _ => format!("<{}>", obj.type_name()),
    }
}

fn instance_class_name(obj: &PyObjectRef) -> Option<String> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            return Some(cd.name.to_string());
        }
    }
    None
}

fn bound_method_name(receiver: &PyObjectRef, method: &PyObjectRef) -> String {
    match &method.payload {
        PyObjectPayload::Function(func) => {
            let qualname = func.qualname.as_str();
            if qualname.contains('.') {
                qualname.to_string()
            } else if let Some(class_name) = instance_class_name(receiver) {
                format!("{}.{}", class_name, func.name)
            } else {
                func.name.to_string()
            }
        }
        PyObjectPayload::NativeFunction(func) => func.name.to_string(),
        PyObjectPayload::NativeClosure(func) => func.name.to_string(),
        PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
            name.to_string()
        }
        PyObjectPayload::BoundMethod { method, .. } => bound_method_name(receiver, method),
        _ => method.type_name().to_string(),
    }
}

fn bound_method_receiver_repr(receiver: &PyObjectRef) -> String {
    if let Some(repr_fn) = receiver.get_attr("__repr__") {
        if !matches!(&repr_fn.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
            if let Ok(result) = call_callable(&repr_fn, &[]) {
                return result.py_to_string();
            }
        }
    }
    receiver.repr()
}

fn bound_method_repr(receiver: &PyObjectRef, method: &PyObjectRef) -> String {
    format!(
        "<bound method {} of {}>",
        bound_method_name(receiver, method),
        bound_method_receiver_repr(receiver)
    )
}

pub(super) fn py_repr(obj: &PyObjectRef) -> String {
    match &obj.payload {
        PyObjectPayload::Str(s) => {
            // Escape special characters first
            let mut escaped = String::with_capacity(s.len() + 4);
            for c in s.chars() {
                match c {
                    '\\' => escaped.push_str("\\\\"),
                    '\n' => escaped.push_str("\\n"),
                    '\t' => escaped.push_str("\\t"),
                    '\r' => escaped.push_str("\\r"),
                    '\x07' => escaped.push_str("\\a"),
                    '\x08' => escaped.push_str("\\b"),
                    '\x0C' => escaped.push_str("\\f"),
                    '\x0B' => escaped.push_str("\\v"),
                    '\0' => escaped.push_str("\\x00"),
                    _ if c.is_control() => escaped.push_str(&format!("\\x{:02x}", c as u32)),
                    _ => escaped.push(c),
                }
            }
            let has_single = escaped.contains('\'');
            let has_double = escaped.contains('"');
            if has_single && !has_double {
                format!("\"{}\"", escaped)
            } else {
                // Escape single quotes in the escaped string
                let escaped = escaped.replace('\'', "\\'");
                format!("'{}'", escaped)
            }
        }
        PyObjectPayload::ExceptionInstance(ei) => {
            if ei.args.is_empty() {
                format!("{}()", ei.kind)
            } else if ei.args.len() == 1 {
                format!("{}({})", ei.kind, ei.args[0].repr())
            } else {
                let args = ei.args.iter().map(|arg| arg.repr()).collect::<Vec<_>>();
                format!("{}({})", ei.kind, args.join(", "))
            }
        }
        PyObjectPayload::BoundMethod { receiver, method } => bound_method_repr(receiver, method),
        PyObjectPayload::NativeClosure(nc) => {
            if let Some((type_name, method_name)) = nc.name.rsplit_once('.') {
                format!("<method '{}' of '{}' objects>", method_name, type_name)
            } else {
                format!("<built-in function {}>", nc.name)
            }
        }
        PyObjectPayload::NativeFunction(nf) => {
            if let Some((type_name, method_name)) = nf.name.rsplit_once('.') {
                format!("<method '{}' of '{}' objects>", method_name, type_name)
            } else {
                format!("<built-in function {}>", nf.name)
            }
        }
        PyObjectPayload::BuiltinFunction(name) => {
            format!("<built-in function {}>", name)
        }
        PyObjectPayload::BuiltinBoundMethod(bbm) => {
            format!(
                "<built-in method {} of {} object at 0x{:x}>",
                bbm.method_name,
                bbm.receiver.type_name(),
                PyObjectRef::as_ptr(&bbm.receiver) as usize
            )
        }
        PyObjectPayload::StaticMethod(_) => {
            format!(
                "<staticmethod object at 0x{:x}>",
                PyObjectRef::as_ptr(obj) as usize
            )
        }
        PyObjectPayload::ClassMethod(_) => {
            format!(
                "<classmethod object at 0x{:x}>",
                PyObjectRef::as_ptr(obj) as usize
            )
        }
        PyObjectPayload::Cell(cell) => {
            let cell_addr = PyObjectRef::as_ptr(obj) as usize;
            if let Some(value) = cell.read().as_ref() {
                let value_addr = PyObjectRef::as_ptr(value) as usize;
                format!(
                    "<cell at 0x{:x}: {} object at 0x{:x}>",
                    cell_addr,
                    value.type_name(),
                    value_addr
                )
            } else {
                format!("<cell at 0x{:x}: empty>", cell_addr)
            }
        }
        PyObjectPayload::Instance(inst) => {
            // Builtin base type subclass: delegate to __builtin_value__
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                return bv.repr();
            }
            // Dict subclass: repr like a dict
            if let Some(ref ds) = inst.dict_storage {
                let items: Vec<String> = ds
                    .read()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.to_object().repr(), v.repr()))
                    .collect();
                return format!("{{{}}}", items.join(", "));
            }
            // typing _GenericAlias repr
            if let Some(typing_repr) = inst.attrs.read().get("__typing_repr__").cloned() {
                return typing_repr.py_to_string();
            }
            // Decimal repr
            if inst.attrs.read().contains_key("__decimal__") {
                let v = inst
                    .attrs
                    .read()
                    .get("_value")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "0".to_string());
                return format!("Decimal('{}')", v);
            }
            // Enum member repr: <ClassName.NAME: value>
            {
                let attrs = inst.attrs.read();
                if let (Some(name_val), Some(value_val)) =
                    (attrs.get("_name_"), attrs.get("_value_"))
                {
                    let name = name_val.py_to_string();
                    let value = value_val.repr();
                    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        cd.name.to_string()
                    } else {
                        "Enum".to_string()
                    };
                    return format!("<{}.{}: {}>", class_name, name, value);
                }
            }
            // Check for __repr__: if it's a native closure, call it directly
            if let Some(repr_fn) = obj.get_attr("__repr__") {
                match &repr_fn.payload {
                    PyObjectPayload::NativeClosure(nc) => {
                        if let Ok(result) = (nc.func)(&[obj.clone()]) {
                            return result.py_to_string();
                        }
                    }
                    PyObjectPayload::BoundMethod { method, receiver } => {
                        if let PyObjectPayload::NativeClosure(nc) = &method.payload {
                            if let Ok(result) = (nc.func)(&[receiver.clone()]) {
                                return result.py_to_string();
                            }
                        }
                    }
                    _ => {}
                }
            }
            // For exception-like instances, show ClassName(message)
            if let Some(args) = inst.attrs.read().get("args") {
                if let PyObjectPayload::Tuple(items) = &args.payload {
                    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        cd.name.as_str()
                    } else {
                        "object"
                    };
                    if items.is_empty() {
                        return format!("{}()", class_name);
                    } else {
                        let args_str: Vec<String> = items.iter().map(|a| a.repr()).collect();
                        return format!("{}({})", class_name, args_str.join(", "));
                    }
                }
            }
            let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                let ns = cd.namespace.read();
                let module = ns
                    .get("__module__")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "__main__".to_string());
                let qualname = ns
                    .get("__qualname__")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| cd.name.to_string());
                format!("{}.{}", module, qualname)
            } else {
                inst.class.type_name().to_string()
            };
            format!(
                "<{} object at 0x{:x}>",
                class_name,
                PyObjectRef::as_ptr(obj) as usize
            )
        }
        _ => obj.py_to_string(),
    }
}

pub(super) fn py_to_int(obj: &PyObjectRef) -> PyResult<i64> {
    match &obj.payload {
        PyObjectPayload::Int(n) => n
            .to_i64()
            .ok_or_else(|| PyException::overflow_error("int too large")),
        PyObjectPayload::Bool(b) => Ok(if *b { 1 } else { 0 }),
        PyObjectPayload::Float(f) => Ok(*f as i64),
        PyObjectPayload::Str(s) => s
            .trim()
            .parse::<i64>()
            .map_err(|_| PyException::value_error(format!("invalid literal for int(): '{}'", s))),
        PyObjectPayload::Instance(inst) => {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                    return (nc.func)(&[])?.to_int();
                }
            }
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                return bv.to_int();
            }
            // Check for __int__ or __index__ on the instance
            if let Some(int_val) = obj.get_attr("__int__") {
                if let Some(v) = int_val.as_int() {
                    return Ok(v);
                }
            }
            if let Some(idx_val) = obj.get_attr("__index__") {
                if let Some(v) = idx_val.as_int() {
                    return Ok(v);
                }
            }
            Err(PyException::type_error(format!(
                "int() argument must be a string or number, not '{}'",
                obj.type_name()
            )))
        }
        _ => Err(PyException::type_error(format!(
            "int() argument must be a string or number, not '{}'",
            obj.type_name()
        ))),
    }
}

pub(super) fn py_to_index(obj: &PyObjectRef) -> PyResult<PyInt> {
    match &obj.payload {
        PyObjectPayload::Int(n) => Ok(n.clone()),
        PyObjectPayload::Bool(b) => Ok(PyInt::Small(if *b { 1 } else { 0 })),
        PyObjectPayload::Instance(inst) => {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                    return (nc.func)(&[])?.to_index();
                }
            }
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                return value.to_index();
            }
            if let Some(method) = obj.get_attr("__index__") {
                let result = call_callable(&method, &[])?;
                if let PyObjectPayload::Instance(result_inst) = &result.payload {
                    if let Some(value) = result_inst.attrs.read().get("__builtin_value__").cloned()
                    {
                        if matches!(value.payload, PyObjectPayload::Int(_)) {
                            return value.to_index();
                        }
                    }
                }
                return match &result.payload {
                    PyObjectPayload::Int(n) => Ok(n.clone()),
                    PyObjectPayload::Bool(b) => Ok(PyInt::Small(if *b { 1 } else { 0 })),
                    _ => Err(PyException::type_error(format!(
                        "__index__ returned non-int (type {})",
                        result.type_name()
                    ))),
                };
            }
            Err(PyException::type_error(format!(
                "'{}' object cannot be interpreted as an integer",
                obj.type_name()
            )))
        }
        _ => Err(PyException::type_error(format!(
            "'{}' object cannot be interpreted as an integer",
            obj.type_name()
        ))),
    }
}

pub(super) fn py_to_float(obj: &PyObjectRef) -> PyResult<f64> {
    match &obj.payload {
        PyObjectPayload::Float(f) => Ok(*f),
        PyObjectPayload::Int(n) => Ok(n.to_f64()),
        PyObjectPayload::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        PyObjectPayload::Str(s) => s.trim().parse::<f64>().map_err(|_| {
            PyException::value_error(format!("could not convert string to float: '{}'", s))
        }),
        PyObjectPayload::Instance(_) => {
            if let Some(float_val) = obj.get_attr("__float__") {
                if let PyObjectPayload::Float(f) = &float_val.payload {
                    return Ok(*f);
                }
            }
            Err(PyException::type_error(format!(
                "float() argument must be a string or number, not '{}'",
                obj.type_name()
            )))
        }
        _ => Err(PyException::type_error(format!(
            "float() argument must be a string or number, not '{}'",
            obj.type_name()
        ))),
    }
}

pub(super) fn py_as_int(obj: &PyObjectRef) -> Option<i64> {
    match &obj.payload {
        PyObjectPayload::Int(n) => n.to_i64(),
        PyObjectPayload::Bool(b) => Some(if *b { 1 } else { 0 }),
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__builtin_value__")
            .and_then(|v| v.as_int()),
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
