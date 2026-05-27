use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    BuiltinBoundMethodData, IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if let Some(result) = self.call_builtin_bound_fast_path(bbm, &args)? {
            return Ok(result);
        }
        if let Some(result) = self.call_generator_bound_method(bbm, &args)? {
            return Ok(result);
        }

        if let Some(result) = self.call_iterator_or_range_bound_method(bbm, &args)? {
            return Ok(result);
        }

        // VM-level methods that need iterable collection
        if bbm.method_name.as_str() == "join" {
            if let PyObjectPayload::Str(sep) = &bbm.receiver.payload {
                if !args.is_empty() {
                    let items = self.collect_iterable(&args[0])?;
                    let strs: Result<Vec<String>, _> = items
                        .iter()
                        .map(|x| {
                            x.as_str().map(String::from).ok_or_else(|| {
                                ferrython_core::error::PyException::type_error(
                                    "sequence item: expected str",
                                )
                            })
                        })
                        .collect();
                    return Ok(PyObject::str_val(CompactString::from(
                        strs?.join(sep.as_str()),
                    )));
                }
            }
            if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) =
                &bbm.receiver.payload
            {
                if !args.is_empty() {
                    let sep = sep.clone();
                    let mutable_result =
                        matches!(&bbm.receiver.payload, PyObjectPayload::ByteArray(_));
                    let items = self.collect_iterable(&args[0])?;
                    let mut result = Vec::new();
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            result.extend_from_slice(&sep);
                        }
                        if let Some(data) = Self::bytes_like_data(item) {
                            result.extend_from_slice(&data);
                        } else {
                            return Err(PyException::type_error(
                                "sequence item: expected a bytes-like object",
                            ));
                        }
                    }
                    return Ok(if mutable_result {
                        PyObject::bytearray(result)
                    } else {
                        PyObject::bytes(result)
                    });
                }
            }
        }
        // VM-level list.sort with key function
        if bbm.method_name.as_str() == "sort" {
            if matches!(&bbm.receiver.payload, PyObjectPayload::List(_)) {
                let mut items_vec = if let PyObjectPayload::List(items) = &bbm.receiver.payload {
                    items.read().clone()
                } else {
                    Vec::new()
                };
                self.vm_sort(&mut items_vec)?;
                if let PyObjectPayload::List(items) = &bbm.receiver.payload {
                    *items.write() = items_vec;
                }
                return Ok(PyObject::none());
            }
        }
        // Class introspection methods
        if let PyObjectPayload::Class(cd) = &bbm.receiver.payload {
            match bbm.method_name.as_str() {
                "__subclasses__" => {
                    let subs = cd.subclasses.read();
                    let alive: Vec<PyObjectRef> = subs.iter().filter_map(|w| w.upgrade()).collect();
                    drop(subs);
                    // Prune dead weak refs periodically
                    cd.subclasses.write().retain(|w| w.strong_count() > 0);
                    return Ok(PyObject::list(alive));
                }
                "mro" => {
                    let mut mro_list = vec![bbm.receiver.clone()];
                    mro_list.extend(cd.mro.iter().cloned());
                    return Ok(PyObject::list(mro_list));
                }
                _ => {}
            }
        }
        // Property descriptor methods: setter/getter/deleter
        if ferrython_core::object::is_property_like(&bbm.receiver) {
            if args.len() == 1 {
                let func = args[0].clone();
                let old_fget = Self::property_callable_field(&bbm.receiver, "fget");
                let old_fset = Self::property_callable_field(&bbm.receiver, "fset");
                let old_fdel = Self::property_callable_field(&bbm.receiver, "fdel");
                let doc_from_getter = Self::property_doc_from_getter_flag(&bbm.receiver);
                let (fget, fset, fdel, doc, new_doc_from_getter) = match bbm.method_name.as_str() {
                    "setter" => {
                        let doc = if doc_from_getter {
                            ferrython_core::object::property_doc_from_getter(old_fget.as_ref())
                        } else {
                            ferrython_core::object::property_field(&bbm.receiver, "__doc__")
                        };
                        (old_fget, Some(func), old_fdel, doc, doc_from_getter)
                    }
                    "getter" => {
                        let doc = if doc_from_getter {
                            ferrython_core::object::property_doc_from_getter(Some(&func))
                        } else {
                            ferrython_core::object::property_field(&bbm.receiver, "__doc__")
                        };
                        (Some(func), old_fset, old_fdel, doc, doc_from_getter)
                    }
                    "deleter" => {
                        let doc = if doc_from_getter {
                            ferrython_core::object::property_doc_from_getter(old_fget.as_ref())
                        } else {
                            ferrython_core::object::property_field(&bbm.receiver, "__doc__")
                        };
                        (old_fget, old_fset, Some(func), doc, doc_from_getter)
                    }
                    _ => {
                        return Err(PyException::attribute_error(format!(
                            "property has no attribute '{}'",
                            bbm.method_name
                        )))
                    }
                };
                return Self::make_property_like(
                    &bbm.receiver,
                    fget,
                    fset,
                    fdel,
                    doc,
                    new_doc_from_getter,
                );
            }
        }
        // namedtuple methods — delegated to builtins
        if let PyObjectPayload::Instance(inst) = &bbm.receiver.payload {
            if matches!(&inst.class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__namedtuple__"))
                || inst.attrs.read().contains_key("__deque__")
            {
                // deque extend/extendleft need iterable collection via VM
                if inst.attrs.read().contains_key("__deque__")
                    && matches!(bbm.method_name.as_str(), "extend" | "extendleft")
                {
                    let items = self.collect_iterable(&args[0])?;
                    return builtins::call_method(
                        &bbm.receiver,
                        bbm.method_name.as_str(),
                        &[PyObject::list(items)],
                    );
                }
                return builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), &args);
            }
            // Hashlib methods — delegated to builtins
            let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                cd.name.to_string()
            } else {
                String::new()
            };
            if matches!(
                class_name.as_str(),
                "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512"
            ) {
                return builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), &args);
            }
        }
        // Unbound method call: str.upper("hello") → call_method("hello", "upper", [])
        if let PyObjectPayload::BuiltinType(tn) = &bbm.receiver.payload {
            // type.__call__(cls, *args) → instantiate the class
            if tn.as_str() == "type" && bbm.method_name.as_str() == "__call__" && !args.is_empty() {
                if matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                    let cls = args[0].clone();
                    let mut rest = args[1..].to_vec();
                    // Unpack trailing kwargs dict (produced by call_object_kw fallback)
                    let kw = {
                        let mut extracted = vec![];
                        let should_pop = if let Some(last) = rest.last() {
                            if let PyObjectPayload::Dict(map) = &last.payload {
                                let rd = map.read();
                                let all_str = rd.keys().all(|k| matches!(k, HashableKey::Str(_)));
                                if all_str && !rd.is_empty() {
                                    for (k, v) in rd.iter() {
                                        if let HashableKey::Str(s) = k {
                                            extracted.push((s.to_compact_string(), v.clone()));
                                        }
                                    }
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if should_pop {
                            rest.pop();
                        }
                        extracted
                    };
                    return self.instantiate_class(&cls, rest, kw);
                }
            }
            // Class methods (e.g., int.from_bytes, dict.fromkeys)
            if let Some(class_method) =
                builtins::resolve_type_class_method(tn, bbm.method_name.as_str())
            {
                if let PyObjectPayload::NativeFunction(nf) = &class_method.payload {
                    if nf.name.as_str() == "dict.fromkeys"
                        && !args.is_empty()
                        && matches!(
                            args[0].payload,
                            PyObjectPayload::Generator(_)
                                | PyObjectPayload::Instance(_)
                                | PyObjectPayload::Iterator(_)
                        )
                    {
                        let mut resolved = Vec::with_capacity(args.len());
                        resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                        resolved.extend_from_slice(&args[1..]);
                        return (nf.func)(&resolved);
                    }
                    return (nf.func)(&args);
                }
            }
            if matches!(tn.as_str(), "bytes" | "bytearray") && bbm.method_name.as_str() == "hex" {
                let (instance, rest_args) = Self::builtin_type_instance_operand(
                    tn.as_str(),
                    bbm.method_name.as_str(),
                    &args,
                )?;
                return builtins::call_method(&instance, bbm.method_name.as_str(), &rest_args);
            }
            if !args.is_empty() {
                let instance = args[0].clone();
                let rest_args = if args.len() > 1 {
                    args[1..].to_vec()
                } else {
                    vec![]
                };
                return builtins::call_method(&instance, bbm.method_name.as_str(), &rest_args);
            }
        }
        // list.extend with generator/lazy iterator/instance needs VM-level collection
        if bbm.method_name.as_str() == "extend" && !args.is_empty() {
            if matches!(bbm.receiver.payload, PyObjectPayload::List(_)) {
                if matches!(
                    args[0].payload,
                    PyObjectPayload::Generator(_) | PyObjectPayload::Instance(_)
                ) || (matches!(&args[0].payload, PyObjectPayload::Iterator(ref d) if {
                    let data = d.read();
                    matches!(&*data, IteratorData::Enumerate { .. } | IteratorData::Zip { .. }
                        | IteratorData::MapOne { .. }
                        | IteratorData::Map { .. } | IteratorData::Filter { .. }
                        | IteratorData::FilterFalse { .. }
                        | IteratorData::Sentinel { .. })
                })) {
                    let items = self.collect_iterable(&args[0])?;
                    return builtins::call_method(
                        &bbm.receiver,
                        "extend",
                        &[PyObject::list(items)],
                    );
                }
            }
        }
        // list.sort(key=, reverse=) needs VM for key function calls
        if bbm.method_name.as_str() == "sort" {
            if let PyObjectPayload::List(items) = &bbm.receiver.payload {
                // Extract key and reverse from trailing kwargs dict
                let mut key_fn: Option<PyObjectRef> = None;
                let mut reverse = false;
                for arg in &args {
                    if let PyObjectPayload::Dict(d) = &arg.payload {
                        let rd = d.read();
                        if let Some(v) =
                            rd.get(&HashableKey::str_key(CompactString::from("reverse")))
                        {
                            reverse = v.is_truthy();
                        }
                        if let Some(v) = rd.get(&HashableKey::str_key(CompactString::from("key"))) {
                            if !matches!(v.payload, PyObjectPayload::None) {
                                key_fn = Some(v.clone());
                            }
                        }
                    }
                }
                if let Some(key) = key_fn {
                    // Decorate-sort-undecorate (Schwartzian transform)
                    let mut w = items.write();
                    let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
                    for item in w.iter() {
                        let k = self.call_object(key.clone(), vec![item.clone()])?;
                        decorated.push((k, item.clone()));
                    }
                    let keys: Vec<PyObjectRef> = decorated.iter().map(|(k, _)| k.clone()).collect();
                    let mut indices: Vec<usize> = (0..decorated.len()).collect();
                    for i in 1..indices.len() {
                        let mut j = i;
                        while j > 0 {
                            if self.vm_lt(&keys[indices[j]], &keys[indices[j - 1]])? {
                                indices.swap(j, j - 1);
                                j -= 1;
                            } else {
                                break;
                            }
                        }
                    }
                    w.clear();
                    for i in indices {
                        w.push(decorated[i].1.clone());
                    }
                    if reverse {
                        w.reverse();
                    }
                    return Ok(PyObject::none());
                } else {
                    let mut w = items.write();
                    let mut v: Vec<_> = w.drain(..).collect();
                    self.vm_sort(&mut v)?;
                    if reverse {
                        v.reverse();
                    }
                    w.extend(v);
                    return Ok(PyObject::none());
                }
            }
        }
        // str.format with positional args: needs VM for __str__ on instances
        if bbm.method_name.as_str() == "format" {
            if let PyObjectPayload::Str(s) = &bbm.receiver.payload {
                return self.vm_str_format(s, &args);
            }
        }
        // str.format_map with dict subclass: needs VM for __missing__ calls
        if bbm.method_name.as_str() == "format_map" && !args.is_empty() {
            if let PyObjectPayload::Str(s) = &bbm.receiver.payload {
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if let Some(ref ds) = inst.dict_storage {
                        return self.vm_format_map(s, &args[0], ds, &inst.class);
                    }
                }
                // Handle defaultdict (Dict payload with __defaultdict_factory__)
                if let PyObjectPayload::Dict(m) = &args[0].payload {
                    let factory_key = ferrython_core::types::HashableKey::str_key(
                        CompactString::from("__defaultdict_factory__"),
                    );
                    if m.read().contains_key(&factory_key) {
                        return self.vm_format_map_dict(s, &args[0], m);
                    }
                }
            }
        }
        builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), &args)
    }
}
