use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    FxAttrMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

use crate::collection_modules::{
    create_collections_module, create_operator_module, namedtuple_rebuild_field,
    namedtuple_rebuild_instance,
};
use crate::text_modules::create_re_module;

use super::shared::{pickle_exception_instance, pkl_apply_state};

// ── Protocol 0 (text) deserialization ──

#[derive(Clone)]
enum PklStackItem {
    Value(PyObjectRef),
    Mark,
    Global(String, String),
}

fn p0_read_line<'a>(data: &'a [u8], pos: &mut usize) -> &'a [u8] {
    let start = *pos;
    while *pos < data.len() && data[*pos] != b'\n' {
        *pos += 1;
    }
    let line = &data[start..*pos];
    if *pos < data.len() {
        *pos += 1;
    }
    line
}

fn p0_unescape_unicode(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => result.push('\\'),
            Some('n') => result.push('\n'),
            Some('r') => result.push('\r'),
            Some('t') => result.push('\t'),
            Some('0') => result.push('\0'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) {
                        result.push(ch);
                    }
                }
            }
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) {
                        result.push(ch);
                    }
                }
            }
            Some('U') => {
                let hex: String = chars.by_ref().take(8).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    if let Some(ch) = char::from_u32(n) {
                        result.push(ch);
                    }
                }
            }
            Some(other) => {
                result.push('\\');
                result.push(other);
            }
            None => result.push('\\'),
        }
    }
    result
}

fn p0_unescape_bytes(raw: &[u8]) -> Vec<u8> {
    if raw.len() < 2 {
        return Vec::new();
    }
    let quote = raw[0];
    if quote != b'\'' && quote != b'"' {
        return raw.to_vec();
    }
    let end = if raw.last() == Some(&quote) {
        raw.len() - 1
    } else {
        raw.len()
    };
    let inner = &raw[1..end];
    let mut result = Vec::new();
    let mut i = 0;
    while i < inner.len() {
        if inner[i] == b'\\' && i + 1 < inner.len() {
            match inner[i + 1] {
                b'\\' => {
                    result.push(b'\\');
                    i += 2;
                }
                b'\'' => {
                    result.push(b'\'');
                    i += 2;
                }
                b'"' => {
                    result.push(b'"');
                    i += 2;
                }
                b'n' => {
                    result.push(b'\n');
                    i += 2;
                }
                b'r' => {
                    result.push(b'\r');
                    i += 2;
                }
                b't' => {
                    result.push(b'\t');
                    i += 2;
                }
                b'x' if i + 3 < inner.len() => {
                    if let Ok(v) = u8::from_str_radix(
                        std::str::from_utf8(&inner[i + 2..i + 4]).unwrap_or("00"),
                        16,
                    ) {
                        result.push(v);
                    }
                    i += 4;
                }
                _ => {
                    result.push(inner[i]);
                    result.push(inner[i + 1]);
                    i += 2;
                }
            }
        } else {
            result.push(inner[i]);
            i += 1;
        }
    }
    result
}

fn pkl_pop_to_mark(stack: &mut Vec<PklStackItem>) -> PyResult<Vec<PyObjectRef>> {
    let mut items = Vec::new();
    loop {
        match stack.pop() {
            Some(PklStackItem::Mark) => break,
            Some(PklStackItem::Value(v)) => items.push(v),
            Some(PklStackItem::Global(..)) => {
                return Err(PyException::runtime_error(
                    "UnpicklingError: unexpected global on stack",
                ));
            }
            None => {
                return Err(PyException::runtime_error(
                    "UnpicklingError: MARK not found on stack",
                ))
            }
        }
    }
    items.reverse();
    Ok(items)
}

fn pkl_stack_top_value(stack: &[PklStackItem]) -> PyResult<PyObjectRef> {
    match stack.last() {
        Some(PklStackItem::Value(v)) => Ok(v.clone()),
        _ => Err(PyException::runtime_error(
            "UnpicklingError: expected value on stack top",
        )),
    }
}

fn ast_empty_fields_node_names(name: &str) -> bool {
    matches!(
        name,
        "Load"
            | "Store"
            | "Del"
            | "And"
            | "Or"
            | "Add"
            | "Sub"
            | "Mult"
            | "MatMult"
            | "Div"
            | "Mod"
            | "Pow"
            | "LShift"
            | "RShift"
            | "BitOr"
            | "BitXor"
            | "BitAnd"
            | "FloorDiv"
            | "Invert"
            | "Not"
            | "UAdd"
            | "USub"
            | "Eq"
            | "NotEq"
            | "Lt"
            | "LtE"
            | "Gt"
            | "GtE"
            | "Is"
            | "IsNot"
            | "In"
            | "NotIn"
            | "Pass"
            | "Break"
            | "Continue"
    )
}

fn maybe_add_ast_empty_fields(name: &str, attrs: &mut IndexMap<CompactString, PyObjectRef>) {
    if ast_empty_fields_node_names(name) && !attrs.contains_key("_fields") {
        attrs.insert(CompactString::from("_fields"), PyObject::tuple(vec![]));
    }
}

fn pkl_class_candidate(obj: PyObjectRef) -> Option<PyObjectRef> {
    if matches!(
        &obj.payload,
        PyObjectPayload::Class(_) | PyObjectPayload::BuiltinType(_)
    ) {
        Some(obj)
    } else {
        None
    }
}

fn pkl_resolve_dotted_attr(root: PyObjectRef, path: &str) -> Option<PyObjectRef> {
    let mut obj = root;
    for part in path.split('.') {
        if part.is_empty() {
            return None;
        }
        obj = obj.get_attr(part)?;
    }
    Some(obj)
}

fn pkl_resolve_attr_map_name(attrs: &FxAttrMap, name: &str) -> Option<PyObjectRef> {
    if let Some(obj) = attrs.get(name).cloned() {
        return Some(obj);
    }
    let (root_name, rest) = name.split_once('.')?;
    let root = attrs.get(root_name).cloned()?;
    pkl_resolve_dotted_attr(root, rest)
}

fn pkl_resolve_sys_module(module: &str) -> Option<PyObjectRef> {
    let sys = crate::get_current_sys_module()?;
    let modules = sys.get_attr("modules")?;
    let PyObjectPayload::Dict(map) = &modules.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(module)))
        .cloned()
}

fn pkl_resolve_global_class(module: &str, name: &str) -> Option<PyObjectRef> {
    if let Some(module_obj) = pkl_resolve_sys_module(module) {
        if let Some(cls) = pkl_resolve_dotted_attr(module_obj, name).and_then(pkl_class_candidate) {
            return Some(cls);
        }
    }

    let globals = crate::get_current_globals()?;
    let globals_r = globals.read();
    let current_module = globals_r.get("__name__").map(|value| value.py_to_string());
    if module == "__main__" || current_module.as_deref() == Some(module) {
        if let Some(cls) = pkl_resolve_attr_map_name(&globals_r, name).and_then(pkl_class_candidate)
        {
            return Some(cls);
        }
    }

    if let Some(module_obj) = pkl_resolve_attr_map_name(&globals_r, module) {
        if let Some(cls) = pkl_resolve_dotted_attr(module_obj, name).and_then(pkl_class_candidate) {
            return Some(cls);
        }
    }

    None
}

fn pkl_reduce(callable: &PklStackItem, args: &PyObjectRef) -> PyResult<PyObjectRef> {
    if let PklStackItem::Global(module, name) = callable {
        let arg_list = match &args.payload {
            PyObjectPayload::Tuple(items) => (**items).clone(),
            _ => vec![args.clone()],
        };
        if matches!(module.as_str(), "__builtin__" | "builtins" | "exceptions") {
            if let Some(kind) = ExceptionKind::from_name(name.as_str()) {
                return Ok(pickle_exception_instance(kind, arg_list));
            }
        }
        match (module.as_str(), name.as_str()) {
            ("__builtin__" | "builtins", "set") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::List(items) = &first.payload {
                        let items = items.read();
                        let mut map = IndexMap::new();
                        for item in items.iter() {
                            if let Ok(hk) = HashableKey::from_object(item) {
                                map.insert(hk, item.clone());
                            }
                        }
                        return Ok(PyObject::set(map));
                    }
                }
                Ok(PyObject::set(IndexMap::new()))
            }
            ("__builtin__" | "builtins", "frozenset") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::List(items) = &first.payload {
                        let items = items.read();
                        let mut map = IndexMap::new();
                        for item in items.iter() {
                            if let Ok(hk) = HashableKey::from_object(item) {
                                map.insert(hk, item.clone());
                            }
                        }
                        return Ok(PyObject::frozenset(map));
                    }
                }
                Ok(PyObject::frozenset(IndexMap::new()))
            }
            ("__builtin__" | "builtins", "iter") => {
                // Reconstruct iter(list_or_tuple) as a VecIter.
                if let Some(first) = arg_list.first() {
                    let items: Vec<PyObjectRef> = match &first.payload {
                        PyObjectPayload::List(cell) => cell.read().clone(),
                        PyObjectPayload::Tuple(items) => (**items).clone(),
                        _ => vec![first.clone()],
                    };
                    use ferrython_core::object::{SyncUsize, VecIterData};
                    return Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(
                        VecIterData {
                            items,
                            index: SyncUsize::new(0),
                        },
                    ))));
                }
                use ferrython_core::object::{SyncUsize, VecIterData};
                Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(
                    VecIterData {
                        items: vec![],
                        index: SyncUsize::new(0),
                    },
                ))))
            }
            (
                "__builtin__" | "builtins",
                "int" | "float" | "str" | "list" | "dict" | "tuple" | "bool",
            ) => Ok(PyObject::builtin_type(CompactString::from(name.as_str()))),
            ("__builtin__" | "builtins", "__ferrython_seqiter__") => {
                use ferrython_core::object::IteratorData;
                let source = arg_list.first().cloned().unwrap_or_else(PyObject::none);
                let index = arg_list.get(1).and_then(|v| v.as_int()).unwrap_or(0);
                let exhausted = arg_list.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::SeqIter {
                        obj: source,
                        index,
                        exhausted,
                    }),
                ))))
            }
            ("__builtin__" | "builtins", "__ferrython_refiter__") => {
                let source = arg_list.first().cloned().unwrap_or_else(PyObject::none);
                let index = arg_list.get(1).and_then(|v| v.as_int()).unwrap_or(0);
                let exhausted = arg_list.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                let index = if exhausted || index < 0 {
                    usize::MAX
                } else {
                    index as usize
                };
                Ok(PyObject::wrap(PyObjectPayload::RefIter {
                    source,
                    index: ferrython_core::object::SyncUsize::new(index),
                }))
            }
            ("__builtin__" | "builtins", "__ferrython_revrefiter__") => {
                let source = arg_list.first().cloned().unwrap_or_else(PyObject::none);
                let index = arg_list.get(1).and_then(|v| v.as_int()).unwrap_or(0);
                let exhausted = arg_list.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                let index = if exhausted || index < 0 {
                    usize::MAX
                } else {
                    index as usize
                };
                Ok(PyObject::wrap(PyObjectPayload::RevRefIter {
                    source,
                    index: ferrython_core::object::SyncUsize::new(index),
                }))
            }
            ("collections", "defaultdict") | ("__main__", "defaultdict") => {
                let collections = create_collections_module();
                let cls = collections.get_attr("defaultdict").ok_or_else(|| {
                    PyException::runtime_error("UnpicklingError: missing collections.defaultdict")
                })?;
                let result = PyObject::instance(cls);
                let raw_factory = arg_list.first().cloned().unwrap_or_else(PyObject::none);
                let factory = if let PyObjectPayload::Str(s) = &raw_factory.payload {
                    if let Some(name) = s.as_str().strip_prefix("__ferrython_builtin_type__:") {
                        PyObject::builtin_type(CompactString::from(name))
                    } else {
                        raw_factory.clone()
                    }
                } else {
                    raw_factory.clone()
                };
                if let PyObjectPayload::Instance(inst) = &result.payload {
                    if !matches!(&factory.payload, PyObjectPayload::None) {
                        if let Some(dst) = inst.dict_storage.as_ref() {
                            dst.write().insert(
                                HashableKey::str_key(CompactString::from(
                                    "__defaultdict_factory__",
                                )),
                                factory.clone(),
                            );
                        }
                    }
                    inst.attrs
                        .write()
                        .insert(CompactString::from("default_factory"), factory);
                    if let Some(data) = arg_list.get(1) {
                        if let PyObjectPayload::Dict(map) = &data.payload {
                            if let Some(dst) = inst.dict_storage.as_ref() {
                                for (k, v) in map.read().iter() {
                                    dst.write().insert(k.clone(), v.clone());
                                }
                            }
                        }
                    }
                }
                Ok(result)
            }
            ("operator", "attrgetter" | "itemgetter" | "methodcaller") => {
                let module = create_operator_module();
                let constructor = module.get_attr(name.as_str()).ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: unsupported operator constructor {}",
                        name
                    ))
                })?;
                ferrython_core::object::call_callable(&constructor, &arg_list)
            }
            ("__main__" | "re", "Pattern") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::Dict(map) = &first.payload {
                        let map_r = map.read();
                        let pattern = map_r
                            .get(&HashableKey::str_key(CompactString::from("pattern")))
                            .cloned()
                            .unwrap_or_else(|| PyObject::str_val(CompactString::from("")));
                        let flags = map_r
                            .get(&HashableKey::str_key(CompactString::from("flags")))
                            .cloned()
                            .unwrap_or_else(|| PyObject::int(0));
                        let module = create_re_module();
                        let compile = module.get_attr("compile").ok_or_else(|| {
                            PyException::runtime_error("UnpicklingError: missing re.compile")
                        })?;
                        return ferrython_core::object::call_callable(&compile, &[pattern, flags]);
                    }
                }
                Err(PyException::runtime_error(
                    "UnpicklingError: invalid Pattern state",
                ))
            }
            (_, "namedtuple_field") => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::Dict(map) = &first.payload {
                        let map_r = map.read();
                        let idx = map_r
                            .get(&HashableKey::str_key(CompactString::from(
                                "__tuple_index__",
                            )))
                            .and_then(|v| v.as_int())
                            .unwrap_or(0);
                        let field_name = map_r
                            .get(&HashableKey::str_key(CompactString::from("__field_name__")))
                            .map(|v| v.py_to_string())
                            .unwrap_or_default();
                        let doc = map_r
                            .get(&HashableKey::str_key(CompactString::from("__doc__")))
                            .map(|v| v.py_to_string())
                            .unwrap_or_default();
                        return namedtuple_rebuild_field(&[
                            PyObject::int(idx),
                            PyObject::str_val(CompactString::from(field_name)),
                            PyObject::str_val(CompactString::from(doc)),
                        ]);
                    }
                }
                Err(PyException::runtime_error(
                    "UnpicklingError: invalid namedtuple_field state",
                ))
            }
            (_, "Counter") => {
                let class_obj = crate::get_current_globals()
                    .and_then(|globals| globals.read().get("Counter").cloned())
                    .or_else(|| {
                        crate::get_current_globals().and_then(|globals| {
                            globals
                                .read()
                                .get("collections")
                                .cloned()
                                .and_then(|collections_mod| collections_mod.get_attr("Counter"))
                        })
                    });
                let counter_cls = class_obj.unwrap_or_else(|| {
                    PyObject::class(
                        CompactString::from("Counter"),
                        vec![PyObject::builtin_type(CompactString::from("dict"))],
                        IndexMap::new(),
                    )
                });
                let result = PyObject::instance(counter_cls);
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::Dict(map) = &first.payload {
                        if let Some(dst) = match &result.payload {
                            PyObjectPayload::Instance(inst) => inst.dict_storage.as_ref(),
                            _ => None,
                        } {
                            let mut w = dst.write();
                            for (k, v) in map.read().iter() {
                                if let HashableKey::Str(s) = k {
                                    if s.as_str() == "__counter_kwargs__" {
                                        continue;
                                    }
                                }
                                w.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
                return Ok(result);
            }
            _ => {
                if let Some(first) = arg_list.first() {
                    if let PyObjectPayload::Dict(map) = &first.payload {
                        let map_r = map.read();
                        if map_r
                            .get(&HashableKey::str_key(CompactString::from("__namedtuple__")))
                            .is_some()
                        {
                            let field_names = map_r
                                .get(&HashableKey::str_key(CompactString::from("_fields")))
                                .and_then(|v| v.to_list().ok())
                                .unwrap_or_default();
                            let defaults = map_r
                                .get(&HashableKey::str_key(CompactString::from(
                                    "_field_defaults",
                                )))
                                .and_then(|v| {
                                    if let PyObjectPayload::Dict(d) = &v.payload {
                                        Some(
                                            d.read()
                                                .values()
                                                .cloned()
                                                .collect::<Vec<PyObjectRef>>(),
                                        )
                                    } else {
                                        None
                                    }
                                });
                            let module_obj = map_r
                                .get(&HashableKey::str_key(CompactString::from("__module__")))
                                .cloned()
                                .unwrap_or_else(|| PyObject::str_val(CompactString::from(module)));
                            let tuple_values = map_r
                                .get(&HashableKey::str_key(CompactString::from("_tuple")))
                                .cloned()
                                .unwrap_or_else(|| PyObject::tuple(vec![]));
                            let rebuilt = namedtuple_rebuild_instance(&[
                                PyObject::str_val(CompactString::from(name.as_str())),
                                PyObject::tuple(field_names),
                                defaults.map(PyObject::tuple).unwrap_or_else(PyObject::none),
                                module_obj,
                                tuple_values,
                            ])?;
                            if let PyObjectPayload::Instance(ref inst_data) = rebuilt.payload {
                                let mut attrs = inst_data.attrs.write();
                                for (k, v) in map_r.iter() {
                                    if let HashableKey::Str(key) = k {
                                        if matches!(
                                            key.as_str(),
                                            "__namedtuple__"
                                                | "_fields"
                                                | "_field_defaults"
                                                | "__module__"
                                                | "_tuple"
                                        ) {
                                            continue;
                                        }
                                        attrs.insert(key.to_compact_string(), v.clone());
                                    }
                                }
                            }
                            return Ok(rebuilt);
                        }
                        let mut attrs = IndexMap::new();
                        for (k, v) in map_r.iter() {
                            if let HashableKey::Str(s) = k {
                                attrs.insert(s.to_compact_string(), v.clone());
                            }
                        }
                        maybe_add_ast_empty_fields(name, &mut attrs);
                        let cls = pkl_resolve_global_class(module, name).unwrap_or_else(|| {
                            let mut class_namespace = IndexMap::new();
                            class_namespace.insert(
                                CompactString::from("__module__"),
                                PyObject::str_val(CompactString::from(module.as_str())),
                            );
                            PyObject::class(
                                CompactString::from(name.as_str()),
                                vec![],
                                class_namespace,
                            )
                        });
                        return Ok(PyObject::instance_with_attrs(cls, attrs));
                    }
                }
                Err(PyException::runtime_error(format!(
                    "UnpicklingError: unsupported global {}.{}",
                    module, name
                )))
            }
        }
    } else {
        Err(PyException::runtime_error(
            "UnpicklingError: REDUCE requires a callable",
        ))
    }
}

fn pickle_loads_p0(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos: usize = 0;
    let mut stack: Vec<PklStackItem> = Vec::new();
    let mut memo: std::collections::HashMap<u32, PyObjectRef> = std::collections::HashMap::new();

    while pos < data.len() {
        let opcode = data[pos];
        pos += 1;
        match opcode {
            b'.' => break, // STOP
            b'N' => stack.push(PklStackItem::Value(PyObject::none())),
            b'I' => {
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| {
                        PyException::runtime_error("UnpicklingError: invalid INT encoding")
                    })?
                    .trim();
                if s == "01" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(true)));
                } else if s == "00" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(false)));
                } else {
                    let val: i64 = s.parse().map_err(|_| {
                        PyException::runtime_error(format!(
                            "UnpicklingError: invalid INT value '{}'",
                            s
                        ))
                    })?;
                    stack.push(PklStackItem::Value(PyObject::int(val)));
                }
            }
            b'L' => {
                // LONG — like I but for big ints, trailing L
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| {
                        PyException::runtime_error("UnpicklingError: invalid LONG encoding")
                    })?
                    .trim()
                    .trim_end_matches('L');
                let val: i64 = s.parse().unwrap_or(0);
                stack.push(PklStackItem::Value(PyObject::int(val)));
            }
            b'F' => {
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| {
                        PyException::runtime_error("UnpicklingError: invalid FLOAT encoding")
                    })?
                    .trim();
                let val: f64 = match s {
                    "nan" | "NaN" => f64::NAN,
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => s.parse().map_err(|_| {
                        PyException::runtime_error(format!(
                            "UnpicklingError: invalid FLOAT value '{}'",
                            s
                        ))
                    })?,
                };
                stack.push(PklStackItem::Value(PyObject::float(val)));
            }
            b'V' => {
                // UNICODE — read raw-unicode-escape line
                let line = p0_read_line(data, &mut pos);
                let s = p0_unescape_unicode(line);
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
            }
            b'S' => {
                // STRING — read quoted string line (bytes)
                let line = p0_read_line(data, &mut pos);
                let bytes = p0_unescape_bytes(line);
                stack.push(PklStackItem::Value(PyObject::bytes(bytes)));
            }
            b'(' => stack.push(PklStackItem::Mark),
            b'l' => {
                // LIST — pop to mark, build list
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::list(items)));
            }
            b't' => {
                // TUPLE — pop to mark, build tuple
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::tuple(items)));
            }
            b'd' => {
                // DICT — pop to mark, build dict from pairs
                let items = pkl_pop_to_mark(&mut stack)?;
                let mut pairs = Vec::new();
                for chunk in items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PklStackItem::Value(PyObject::dict_from_pairs(pairs)));
            }
            b'a' => {
                // APPEND — pop item, append to list on stack
                let item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: APPEND expects value",
                        ))
                    }
                };
                // Find the list on top of the remaining stack
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().push(item);
                    }
                }
            }
            b's' => {
                // SETITEM — pop value, pop key, set on dict
                let val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects value",
                        ))
                    }
                };
                let key = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects key",
                        ))
                    }
                };
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        if let Ok(hk) = HashableKey::from_object(&key) {
                            dict_map.write().insert(hk, val);
                        }
                    }
                }
            }
            b'p' => {
                // PUT — memoize top of stack
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'g' => {
                // GET — recall from memo
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'c' => {
                // GLOBAL — read module\nqualname\n
                let mod_line = p0_read_line(data, &mut pos);
                let name_line = p0_read_line(data, &mut pos);
                let module = String::from_utf8_lossy(mod_line).to_string();
                let name = String::from_utf8_lossy(name_line).to_string();
                stack.push(PklStackItem::Global(module, name));
            }
            b'R' => {
                // REDUCE — pop args tuple, pop callable, call
                let args_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects args",
                        ))
                    }
                };
                let callable = match stack.pop() {
                    Some(item) => item,
                    None => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects callable",
                        ))
                    }
                };
                let result = pkl_reduce(&callable, &args_item)?;
                stack.push(PklStackItem::Value(result));
            }
            b'b' => {
                // BUILD — apply a state dict to the object on top of the stack.
                let state = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects state",
                        ))
                    }
                };
                let obj = match stack.last() {
                    Some(PklStackItem::Value(v)) => v.clone(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects object",
                        ))
                    }
                };
                pkl_apply_state(&obj, &state)?;
            }
            b'\n' | b'\r' | b' ' => {} // skip whitespace
            _ => {
                return Err(PyException::runtime_error(format!(
                    "UnpicklingError: unknown protocol 0 opcode 0x{:02x} ('{}')",
                    opcode,
                    if opcode.is_ascii_graphic() {
                        opcode as char
                    } else {
                        '?'
                    }
                )));
            }
        }
    }

    // Return top of stack
    for item in stack.iter().rev() {
        if let PklStackItem::Value(v) = item {
            return Ok(v.clone());
        }
    }
    Err(PyException::runtime_error(
        "UnpicklingError: empty pickle data",
    ))
}

// ── Protocol 2 (binary) deserialization ──

fn pickle_loads_p2(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos: usize = 0;
    let mut stack: Vec<PklStackItem> = Vec::new();
    let mut memo: std::collections::HashMap<u32, PyObjectRef> = std::collections::HashMap::new();

    // Skip protocol header
    if pos + 1 < data.len() && data[pos] == 0x80 {
        pos += 2;
    }

    while pos < data.len() {
        let opcode = data[pos];
        pos += 1;
        match opcode {
            b'.' => break, // STOP
            b'N' => stack.push(PklStackItem::Value(PyObject::none())),
            0x88 => stack.push(PklStackItem::Value(PyObject::bool_val(true))),
            0x89 => stack.push(PklStackItem::Value(PyObject::bool_val(false))),
            b'K' => {
                // BININT1 — 1-byte unsigned int
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT1",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::int(data[pos] as i64)));
                pos += 1;
            }
            b'M' => {
                // BININT2 — 2-byte LE unsigned short
                if pos + 2 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT2",
                    ));
                }
                let val = u16::from_le_bytes([data[pos], data[pos + 1]]) as i64;
                stack.push(PklStackItem::Value(PyObject::int(val)));
                pos += 2;
            }
            b'J' => {
                // BININT — 4-byte LE signed int
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT",
                    ));
                }
                let val =
                    i32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as i64;
                stack.push(PklStackItem::Value(PyObject::int(val)));
                pos += 4;
            }
            b'I' => {
                // INT (text fallback) — read to newline
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line).unwrap_or("0").trim();
                if s == "01" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(true)));
                } else if s == "00" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(false)));
                } else {
                    let val: i64 = s.parse().unwrap_or(0);
                    stack.push(PklStackItem::Value(PyObject::int(val)));
                }
            }
            b'G' => {
                // BINFLOAT — 8-byte BE IEEE 754 double
                if pos + 8 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINFLOAT",
                    ));
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[pos..pos + 8]);
                let val = f64::from_be_bytes(bytes);
                stack.push(PklStackItem::Value(PyObject::float(val)));
                pos += 8;
            }
            b'X' => {
                // BINUNICODE — 4-byte LE len + UTF-8
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINUNICODE length",
                    ));
                }
                let len =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINUNICODE data",
                    ));
                }
                let s = std::str::from_utf8(&data[pos..pos + len]).map_err(|_| {
                    PyException::runtime_error("UnpicklingError: invalid utf-8 in BINUNICODE")
                })?;
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
                pos += len;
            }
            0x8c => {
                // SHORT_BINUNICODE — 1-byte len + UTF-8
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINUNICODE",
                    ));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINUNICODE data",
                    ));
                }
                let s = std::str::from_utf8(&data[pos..pos + len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid utf-8"))?;
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
                pos += len;
            }
            b'T' => {
                // BINSTRING — 4-byte LE len + bytes
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINSTRING length",
                    ));
                }
                let len =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINSTRING data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b'U' => {
                // SHORT_BINSTRING — 1-byte len + bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINSTRING",
                    ));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINSTRING data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b'B' => {
                // BINBYTES — 4-byte LE len + raw bytes
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINBYTES length",
                    ));
                }
                let len =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINBYTES data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b'C' => {
                // SHORT_BINBYTES — 1-byte len + raw bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINBYTES",
                    ));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINBYTES data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b']' => stack.push(PklStackItem::Value(PyObject::list(vec![]))),
            b'}' => stack.push(PklStackItem::Value(PyObject::dict_from_pairs(vec![]))),
            b')' => stack.push(PklStackItem::Value(PyObject::tuple(vec![]))),
            b'(' => stack.push(PklStackItem::Mark),
            b'l' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::list(items)));
            }
            b't' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::tuple(items)));
            }
            b'd' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                let mut pairs = Vec::new();
                for chunk in items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PklStackItem::Value(PyObject::dict_from_pairs(pairs)));
            }
            0x85 => {
                // TUPLE1
                let v = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE1 stack underflow",
                        ))
                    }
                };
                stack.push(PklStackItem::Value(PyObject::tuple(vec![v])));
            }
            0x86 => {
                // TUPLE2
                let b_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE2 stack underflow",
                        ))
                    }
                };
                let a_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE2 stack underflow",
                        ))
                    }
                };
                stack.push(PklStackItem::Value(PyObject::tuple(vec![a_val, b_val])));
            }
            0x87 => {
                // TUPLE3
                let c_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE3 stack underflow",
                        ))
                    }
                };
                let b_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE3 stack underflow",
                        ))
                    }
                };
                let a_val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: TUPLE3 stack underflow",
                        ))
                    }
                };
                stack.push(PklStackItem::Value(PyObject::tuple(vec![
                    a_val, b_val, c_val,
                ])));
            }
            b'a' => {
                // APPEND
                let item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: APPEND expects value",
                        ))
                    }
                };
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().push(item);
                    }
                }
            }
            b'e' => {
                // APPENDS — pop items to mark, extend list
                let items = pkl_pop_to_mark(&mut stack)?;
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().extend(items);
                    }
                }
            }
            b's' => {
                // SETITEM
                let val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects value",
                        ))
                    }
                };
                let key = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects key",
                        ))
                    }
                };
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        if let Ok(hk) = HashableKey::from_object(&key) {
                            dict_map.write().insert(hk, val);
                        }
                    }
                }
            }
            b'u' => {
                // SETITEMS — pop pairs to mark, update dict
                let items = pkl_pop_to_mark(&mut stack)?;
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        let mut map = dict_map.write();
                        for chunk in items.chunks_exact(2) {
                            if let Ok(hk) = HashableKey::from_object(&chunk[0]) {
                                map.insert(hk, chunk[1].clone());
                            }
                        }
                    }
                }
            }
            b'q' => {
                // BINPUT — 1-byte memo index
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINPUT",
                    ));
                }
                let id = data[pos] as u32;
                pos += 1;
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'h' => {
                // BINGET — 1-byte memo index
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINGET",
                    ));
                }
                let id = data[pos] as u32;
                pos += 1;
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'r' => {
                // LONG_BINPUT — 4-byte LE memo index
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG_BINPUT",
                    ));
                }
                let id =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                pos += 4;
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'j' => {
                // LONG_BINGET — 4-byte LE memo index
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG_BINGET",
                    ));
                }
                let id =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                pos += 4;
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'p' => {
                // PUT (text) — read id to newline
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'g' => {
                // GET (text) — read id to newline
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'c' => {
                // GLOBAL — module\nqualname\n
                let mod_line = p0_read_line(data, &mut pos);
                let name_line = p0_read_line(data, &mut pos);
                let module = String::from_utf8_lossy(mod_line).to_string();
                let name = String::from_utf8_lossy(name_line).to_string();
                stack.push(PklStackItem::Global(module, name));
            }
            0x93 => {
                // STACK_GLOBAL — pop name, pop module, push global
                let name_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v.py_to_string(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: STACK_GLOBAL expects name",
                        ))
                    }
                };
                let mod_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v.py_to_string(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: STACK_GLOBAL expects module",
                        ))
                    }
                };
                stack.push(PklStackItem::Global(mod_item, name_item));
            }
            b'R' => {
                // REDUCE
                let args_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects args",
                        ))
                    }
                };
                let callable = match stack.pop() {
                    Some(item) => item,
                    None => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects callable",
                        ))
                    }
                };
                let result = pkl_reduce(&callable, &args_item)?;
                stack.push(PklStackItem::Value(result));
            }
            b'b' => {
                // BUILD — apply a state dict to the object on top of the stack.
                let state = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects state",
                        ))
                    }
                };
                let obj = match stack.last() {
                    Some(PklStackItem::Value(v)) => v.clone(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects object",
                        ))
                    }
                };
                pkl_apply_state(&obj, &state)?;
            }
            0x8a => {
                // LONG1 — 1-byte count + little-endian 2's complement bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG1",
                    ));
                }
                let count = data[pos] as usize;
                pos += 1;
                if pos + count > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG1 data",
                    ));
                }
                let bytes = &data[pos..pos + count];
                pos += count;
                if count == 0 {
                    stack.push(PklStackItem::Value(PyObject::int(0)));
                } else {
                    // Little-endian 2's complement
                    let mut val: i64 = 0;
                    for (i, &b) in bytes.iter().enumerate() {
                        val |= (b as i64) << (i * 8);
                    }
                    // Sign extend if high bit set
                    if bytes[count - 1] & 0x80 != 0 {
                        for i in count..8 {
                            val |= 0xffi64 << (i * 8);
                        }
                    }
                    stack.push(PklStackItem::Value(PyObject::int(val)));
                }
            }
            b'V' => {
                // UNICODE (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let s = p0_unescape_unicode(line);
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
            }
            b'S' => {
                // STRING (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let bytes = p0_unescape_bytes(line);
                stack.push(PklStackItem::Value(PyObject::bytes(bytes)));
            }
            b'F' => {
                // FLOAT (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line).unwrap_or("0").trim();
                let val: f64 = match s {
                    "nan" | "NaN" => f64::NAN,
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => s.parse().unwrap_or(0.0),
                };
                stack.push(PklStackItem::Value(PyObject::float(val)));
            }
            b'0' => {} // POP — discard top (used after PUT sometimes)
            b'1' => {} // POP_MARK — discard stack to mark
            b'2' => {
                // DUP — duplicate top of stack
                let val = pkl_stack_top_value(&stack)?;
                stack.push(PklStackItem::Value(val));
            }
            _ => {
                return Err(PyException::runtime_error(format!(
                    "UnpicklingError: unknown opcode 0x{:02x}",
                    opcode
                )));
            }
        }
    }

    for item in stack.iter().rev() {
        if let PklStackItem::Value(v) = item {
            return Ok(v.clone());
        }
    }
    Err(PyException::runtime_error(
        "UnpicklingError: empty pickle data",
    ))
}

// ── Unified deserialization (auto-detects protocol) ──

pub(in crate::serial_modules) fn pickle_loads_stack(data: &[u8]) -> PyResult<PyObjectRef> {
    if data.is_empty() {
        return Err(PyException::runtime_error(
            "UnpicklingError: empty pickle data",
        ));
    }
    if data[0] == 0x80 {
        pickle_loads_p2(data)
    } else {
        pickle_loads_p0(data)
    }
}
