use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    DequeIterData, FxAttrMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    SyncUsize,
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

mod protocol0;
mod protocol2;

use protocol0::pickle_loads_p0;
use protocol2::pickle_loads_p2;

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
            ("__builtin__" | "builtins", "__ferrython_dequeiter__") => {
                let source = arg_list.first().cloned().unwrap_or_else(PyObject::none);
                let index = arg_list.get(1).and_then(|v| v.as_int()).unwrap_or(0);
                let exhausted = arg_list.get(2).map(|v| v.is_truthy()).unwrap_or(false);
                let reverse = arg_list.get(3).map(|v| v.is_truthy()).unwrap_or(false);
                let index = if exhausted || index < 0 {
                    usize::MAX
                } else {
                    index as usize
                };
                let expected_len = if exhausted {
                    0
                } else if let PyObjectPayload::Instance(inst) = &source.payload {
                    inst.attrs
                        .read()
                        .get("_data")
                        .and_then(|data| {
                            if let PyObjectPayload::List(items) = &data.payload {
                                Some(items.read().len())
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0)
                } else {
                    0
                };
                Ok(PyObject::tracked(PyObjectPayload::DequeIter(Box::new(
                    DequeIterData {
                        source,
                        index: SyncUsize::new(index),
                        expected_len,
                        reverse,
                    },
                ))))
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
                        let mut attrs = IndexMap::new();
                        for (k, v) in map_r.iter() {
                            if let HashableKey::Str(s) = k {
                                attrs.insert(s.to_compact_string(), v.clone());
                            }
                        }
                        maybe_add_ast_empty_fields(name, &mut attrs);
                        return Ok(PyObject::instance_with_attrs(cls, attrs));
                    }
                }
                let cls = pkl_resolve_global_class(module, name).unwrap_or_else(|| {
                    let mut class_namespace = IndexMap::new();
                    class_namespace.insert(
                        CompactString::from("__module__"),
                        PyObject::str_val(CompactString::from(module.as_str())),
                    );
                    PyObject::class(CompactString::from(name.as_str()), vec![], class_namespace)
                });
                Ok(PyObject::instance(cls))
            }
        }
    } else {
        Err(PyException::runtime_error(
            "UnpicklingError: REDUCE requires a callable",
        ))
    }
}

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
