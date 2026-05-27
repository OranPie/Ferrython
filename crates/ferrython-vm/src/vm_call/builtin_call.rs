use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    new_fx_hashkey_map, FxHashKeyMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

use crate::builtins;
use crate::vm_call::str_fast::fast_exact_str;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_or_type(
        &mut self,
        func: &PyObjectRef,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if name.as_str() == "__build_class__" {
            return self.build_class(args);
        }
        if matches!(
            name.as_str(),
            "list" | "tuple" | "set" | "frozenset" | "dict"
        ) {
            return self.call_collection_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "any" | "all" | "isinstance" | "issubclass") {
            return self.call_predicate_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "sum" | "sorted" | "min" | "max") {
            return self.call_computation_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "getattr" | "setattr" | "delattr") {
            return self.call_attr_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "globals" | "locals" | "vars" | "dir") {
            return self.call_scope_builtin(name.as_str(), args);
        }
        if matches!(
            name.as_str(),
            "map" | "filter" | "iter" | "next" | "reversed" | "enumerate" | "zip"
        ) {
            return self.call_iterable_builtin(name, args);
        }
        if matches!(
            name.as_str(),
            "len"
                | "abs"
                | "hash"
                | "bin"
                | "oct"
                | "hex"
                | "format"
                | "complex"
                | "int"
                | "float"
                | "round"
                | "bool"
        ) {
            return self.call_numeric_builtin(func, name, args);
        }
        // VM-aware builtins that need to call user-defined methods
        match name.as_str() {
            "print" => {
                return self.vm_print(&args, None, None, None, false);
            }
            "str" => {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                // str(bytes, encoding[, errors]) — decode bytes
                if args.len() >= 2 {
                    match &args[0].payload {
                        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                            let s = String::from_utf8_lossy(b);
                            return Ok(PyObject::str_val(CompactString::from(s.as_ref())));
                        }
                        _ => {}
                    }
                }
                if args.len() == 1 {
                    if let Some(result) = fast_exact_str(&args[0]) {
                        return Ok(result);
                    }
                }
                return self
                    .vm_str(&args[0])
                    .map(|s| PyObject::str_val(CompactString::from(s)));
            }
            "bytes" => {
                return self.vm_bytes_constructor(&args, false);
            }
            "bytearray" => {
                return self.vm_bytes_constructor(&args, true);
            }
            "repr" => {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                return self
                    .vm_repr(&args[0])
                    .map(|s| PyObject::str_val(CompactString::from(s)));
            }
            "mappingproxy" => {
                // types.MappingProxyType(dict) — read-only view of a dict
                if args.len() == 1 {
                    let src = &args[0];
                    let map = match &src.payload {
                        PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => {
                            m.read().clone()
                        }
                        PyObjectPayload::InstanceDict(attrs) => {
                            let rd = attrs.read();
                            let mut m = new_fx_hashkey_map();
                            for (k, v) in rd.iter() {
                                m.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                            m
                        }
                        _ => {
                            return Err(PyException::type_error(
                                "mappingproxy() argument must be a mapping, not a non-mapping type",
                            ));
                        }
                    };
                    return Ok(PyObject::wrap(PyObjectPayload::MappingProxy(Rc::new(
                        PyCell::new(map),
                    ))));
                }
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "mappingproxy() missing required argument: 'mapping'",
                    ));
                }
            }
            "super" => {
                return self.make_super(&args);
            }
            "exec" => {
                return self.builtin_exec(&args);
            }
            "eval" => {
                return self.builtin_eval(&args);
            }
            "compile" => {
                return self.builtin_compile(&args);
            }
            "__import__" => {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "__import__() requires at least 1 argument",
                    ));
                }
                let name = args[0].py_to_string();
                let level = if args.len() >= 5 {
                    args[4].as_int().unwrap_or(0) as usize
                } else {
                    0
                };
                return self.import_module_simple(&name, level);
            }
            "NamedTuple" => {
                // typing.NamedTuple('Point', [('x', int), ('y', int)]) or NamedTuple('Point', x=int, y=int)
                if !args.is_empty() {
                    let typename = args[0].py_to_string();
                    let mut field_names: Vec<CompactString> = Vec::new();

                    // Check for kwargs dict as last arg
                    let kwargs_dict: Option<FxHashKeyMap> = if args.len() >= 2 {
                        if let PyObjectPayload::Dict(d) = &args[args.len() - 1].payload {
                            Some(d.read().clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let has_kwargs = kwargs_dict.is_some();
                    let positional_end = if has_kwargs {
                        args.len() - 1
                    } else {
                        args.len()
                    };

                    if positional_end >= 2 {
                        match &args[1].payload {
                            PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
                                if let Ok(items) = args[1].to_list() {
                                    for item in &items {
                                        if let PyObjectPayload::Tuple(pair) = &item.payload {
                                            if !pair.is_empty() {
                                                field_names.push(CompactString::from(
                                                    pair[0].py_to_string(),
                                                ));
                                            }
                                        } else {
                                            field_names
                                                .push(CompactString::from(item.py_to_string()));
                                        }
                                    }
                                }
                            }
                            PyObjectPayload::Str(s) => {
                                for n in s.replace(',', " ").split_whitespace() {
                                    field_names.push(CompactString::from(n));
                                }
                            }
                            _ => {}
                        }
                    }

                    // kwargs form: NamedTuple('Point', x=int, y=int)
                    if let Some(ref kw) = kwargs_dict {
                        for (k, _v) in kw {
                            if let HashableKey::Str(fname) = k {
                                if fname.as_str() != "defaults"
                                    && fname.as_str() != "module"
                                    && fname.as_str() != "rename"
                                {
                                    let fname = fname.to_compact_string();
                                    if !field_names.contains(&fname) {
                                        field_names.push(fname);
                                    }
                                }
                            }
                        }
                    }

                    // Build namedtuple class with __namedtuple__ marker and _fields
                    let fields_tuple = PyObject::tuple(
                        field_names
                            .iter()
                            .map(|n| PyObject::str_val(n.clone()))
                            .collect(),
                    );
                    let mut ns = IndexMap::new();
                    ns.insert(
                        CompactString::from("__namedtuple__"),
                        PyObject::bool_val(true),
                    );
                    ns.insert(CompactString::from("_fields"), fields_tuple);
                    ns.insert(
                        CompactString::from("_field_defaults"),
                        PyObject::dict(new_fx_hashkey_map()),
                    );
                    return Ok(PyObject::class(CompactString::from(typename), vec![], ns));
                }
            }
            _ => {}
        }
        match builtins::get_builtin_fn(name.as_str()) {
            Some(f) => {
                let result = f(&args);
                // Check if breakpoint() was called
                if crate::builtins::core_fns::BREAKPOINT_TRIGGERED
                    .swap(false, std::sync::atomic::Ordering::Relaxed)
                {
                    self.breakpoints.builtin_breakpoint_pending = true;
                    self.handle_breakpoint_hit();
                }
                result
            }
            None => Err(PyException::type_error(format!(
                "'{}' is not callable",
                name
            ))),
        }
    }
}
