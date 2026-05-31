use super::*;

fn compare_namespaces(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Instance(ref sd), PyObjectPayload::Instance(ref od)) => {
            let sa = sd.attrs.read();
            let oa = od.attrs.read();
            let self_user: Vec<_> = sa.iter().filter(|(k, _)| !k.starts_with('_')).collect();
            let other_user: Vec<_> = oa.iter().filter(|(k, _)| !k.starts_with('_')).collect();
            if self_user.len() != other_user.len() {
                return Ok(PyObject::bool_val(false));
            }
            for (k, v) in &self_user {
                if let Some(ov) = oa.get(*k) {
                    let eq = v.compare(ov, CompareOp::Eq)?;
                    if !eq.is_truthy() {
                        return Ok(PyObject::bool_val(false));
                    }
                } else {
                    return Ok(PyObject::bool_val(false));
                }
            }
            Ok(PyObject::bool_val(true))
        }
        _ => Ok(PyObject::bool_val(false)),
    }
}

pub fn create_types_module() -> PyObjectRef {
    make_module(
        "types",
        vec![
            (
                "NoneType",
                PyObject::builtin_type(CompactString::from("NoneType")),
            ),
            (
                "FunctionType",
                PyObject::builtin_type(CompactString::from("function")),
            ),
            (
                "LambdaType",
                PyObject::builtin_type(CompactString::from("function")),
            ),
            (
                "BuiltinFunctionType",
                PyObject::builtin_type(CompactString::from("builtin_function_or_method")),
            ),
            (
                "BuiltinMethodType",
                PyObject::builtin_type(CompactString::from("builtin_function_or_method")),
            ),
            (
                "MethodType",
                PyObject::builtin_type(CompactString::from("method")),
            ),
            (
                "ModuleType",
                PyObject::builtin_type(CompactString::from("module")),
            ),
            (
                "GeneratorType",
                PyObject::builtin_type(CompactString::from("generator")),
            ),
            (
                "CodeType",
                PyObject::builtin_type(CompactString::from("code")),
            ),
            (
                "FrameType",
                PyObject::builtin_type(CompactString::from("frame")),
            ),
            (
                "TracebackType",
                PyObject::builtin_type(CompactString::from("traceback")),
            ),
            (
                "CoroutineType",
                PyObject::builtin_type(CompactString::from("coroutine")),
            ),
            (
                "AsyncGeneratorType",
                PyObject::builtin_type(CompactString::from("async_generator")),
            ),
            (
                "MappingProxyType",
                PyObject::builtin_type(CompactString::from("mappingproxy")),
            ),
            (
                "GetSetDescriptorType",
                PyObject::builtin_type(CompactString::from("getset_descriptor")),
            ),
            (
                "MemberDescriptorType",
                PyObject::builtin_type(CompactString::from("member_descriptor")),
            ),
            (
                "WrapperDescriptorType",
                PyObject::builtin_type(CompactString::from("wrapper_descriptor")),
            ),
            (
                "MethodWrapperType",
                PyObject::builtin_type(CompactString::from("method-wrapper")),
            ),
            (
                "MethodDescriptorType",
                PyObject::builtin_type(CompactString::from("method_descriptor")),
            ),
            (
                "ClassMethodDescriptorType",
                PyObject::builtin_type(CompactString::from("classmethod_descriptor")),
            ),
            (
                "CellType",
                PyObject::builtin_type(CompactString::from("cell")),
            ),
            (
                "UnionType",
                PyObject::builtin_type(CompactString::from("UnionType")),
            ),
            (
                "GenericAlias",
                make_builtin(|args| {
                    check_args_min("GenericAlias", args, 2)?;
                    let origin = args[0].clone();
                    let type_args = args[1].clone();
                    let args_tuple = match &type_args.payload {
                        PyObjectPayload::Tuple(_) => type_args.clone(),
                        _ => PyObject::tuple(vec![type_args]),
                    };
                    let origin_name = match &origin.payload {
                        PyObjectPayload::Class(cd) => cd.name.to_string(),
                        PyObjectPayload::BuiltinType(n) => n.to_string(),
                        PyObjectPayload::Str(s) => s.to_string(),
                        _ => origin.py_to_string(),
                    };
                    let args_str = match &args_tuple.payload {
                        PyObjectPayload::Tuple(items) => items
                            .iter()
                            .map(|item| match &item.payload {
                                PyObjectPayload::Class(cd) => cd.name.to_string(),
                                PyObjectPayload::BuiltinType(n) => n.to_string(),
                                _ => item.py_to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join(", "),
                        _ => args_tuple.py_to_string(),
                    };
                    let repr_str = format!("{}[{}]", origin_name, args_str);
                    let cls = PyObject::class(
                        CompactString::from("types.GenericAlias"),
                        vec![],
                        IndexMap::new(),
                    );
                    let mut attrs = IndexMap::new();
                    attrs.insert(CompactString::from("__origin__"), origin);
                    attrs.insert(CompactString::from("__args__"), args_tuple);
                    attrs.insert(
                        CompactString::from("__typing_repr__"),
                        PyObject::str_val(CompactString::from(repr_str.as_str())),
                    );
                    let repr_copy = repr_str.clone();
                    attrs.insert(
                        CompactString::from("__repr__"),
                        PyObject::native_closure("GenericAlias.__repr__", move |_| {
                            Ok(PyObject::str_val(CompactString::from(repr_copy.as_str())))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("__str__"),
                        PyObject::native_closure("GenericAlias.__str__", move |_| {
                            Ok(PyObject::str_val(CompactString::from(repr_str.as_str())))
                        }),
                    );
                    Ok(PyObject::instance_with_attrs(cls, attrs))
                }),
            ),
            (
                "EllipsisType",
                PyObject::builtin_type(CompactString::from("ellipsis")),
            ),
            (
                "NotImplementedType",
                PyObject::builtin_type(CompactString::from("NotImplementedType")),
            ),
            (
                "SimpleNamespace",
                make_builtin(|args| {
                    // Build class-level __repr__ and __eq__ so the VM can dispatch them
                    let mut methods = IndexMap::new();
                    methods.insert(
                        CompactString::from("__repr__"),
                        PyObject::native_closure(
                            "SimpleNamespace.__repr__",
                            |repr_args: &[PyObjectRef]| {
                                if repr_args.is_empty() {
                                    return Ok(PyObject::str_val(CompactString::from(
                                        "namespace()",
                                    )));
                                }
                                let self_obj = &repr_args[0];
                                if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                                    let attrs = d.attrs.read();
                                    let parts: Vec<String> = attrs
                                        .iter()
                                        .filter(|(k, _)| !k.starts_with('_'))
                                        .map(|(k, v)| format!("{}={}", k, v.repr()))
                                        .collect();
                                    Ok(PyObject::str_val(CompactString::from(format!(
                                        "namespace({})",
                                        parts.join(", ")
                                    ))))
                                } else {
                                    Ok(PyObject::str_val(CompactString::from("namespace()")))
                                }
                            },
                        ),
                    );
                    methods.insert(
                        CompactString::from("__eq__"),
                        PyObject::native_closure(
                            "SimpleNamespace.__eq__",
                            |eq_args: &[PyObjectRef]| {
                                // When called via == operator: args = [self, other]
                                // When called via ns1.__eq__(ns2): args = [ns2] (no self)
                                // We handle the 2-arg case here; 1-arg case handled at instance level
                                if eq_args.len() < 2 {
                                    return Ok(PyObject::bool_val(false));
                                }
                                compare_namespaces(&eq_args[0], &eq_args[1])
                            },
                        ),
                    );
                    let cls =
                        PyObject::class(CompactString::from("SimpleNamespace"), vec![], methods);
                    let inst = PyObject::instance(cls);
                    if let Some(last) = args.last() {
                        if let PyObjectPayload::Dict(kw) = &last.payload {
                            if let PyObjectPayload::Instance(ref d) = inst.payload {
                                let mut attrs = d.attrs.write();
                                for (k, v) in kw.read().iter() {
                                    if let HashableKey::Str(s) = k {
                                        attrs.insert(s.to_compact_string(), v.clone());
                                    }
                                }
                            }
                        }
                    }
                    // Install per-instance __eq__ capturing self for ns.__eq__(other) calls
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let self_ref = d.attrs.clone();
                        let mut attrs = d.attrs.write();
                        attrs.insert(
                            CompactString::from("__eq__"),
                            PyObject::native_closure(
                                "SimpleNamespace.__eq__",
                                move |eq_args: &[PyObjectRef]| {
                                    if eq_args.is_empty() {
                                        return Ok(PyObject::bool_val(false));
                                    }
                                    // When called as ns.__eq__(other), eq_args[0] is other
                                    // Build a fake self from our captured attrs
                                    let other = &eq_args[eq_args.len() - 1];
                                    if let PyObjectPayload::Instance(ref od) = other.payload {
                                        let sa = self_ref.read();
                                        let oa = od.attrs.read();
                                        let self_user: Vec<_> = sa
                                            .iter()
                                            .filter(|(k, _)| !k.starts_with('_'))
                                            .collect();
                                        let other_user: Vec<_> = oa
                                            .iter()
                                            .filter(|(k, _)| !k.starts_with('_'))
                                            .collect();
                                        if self_user.len() != other_user.len() {
                                            return Ok(PyObject::bool_val(false));
                                        }
                                        for (k, v) in &self_user {
                                            if let Some(ov) = oa.get(*k) {
                                                let eq = v.compare(ov, CompareOp::Eq)?;
                                                if !eq.is_truthy() {
                                                    return Ok(PyObject::bool_val(false));
                                                }
                                            } else {
                                                return Ok(PyObject::bool_val(false));
                                            }
                                        }
                                        Ok(PyObject::bool_val(true))
                                    } else {
                                        Ok(PyObject::bool_val(false))
                                    }
                                },
                            ),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "new_class",
                make_builtin(|args| {
                    check_args_min("new_class", args, 1)?;
                    let name = args[0].py_to_string();
                    let bases = if args.len() > 1 {
                        args[1].to_list().unwrap_or_default()
                    } else {
                        vec![]
                    };
                    Ok(PyObject::class(
                        CompactString::from(&name),
                        bases,
                        IndexMap::new(),
                    ))
                }),
            ),
            (
                "prepare_class",
                make_builtin(|_| {
                    Ok(PyObject::tuple(vec![
                        PyObject::none(),
                        PyObject::dict(IndexMap::new()),
                        PyObject::dict(IndexMap::new()),
                    ]))
                }),
            ),
            ("DynamicClassAttribute", dynamic_class_attribute_class()),
            (
                "coroutine",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    Ok(args[0].clone())
                }),
            ),
        ],
    )
}

fn dynamic_class_attribute_class() -> PyObjectRef {
    let mut namespace = IndexMap::new();
    namespace.insert(
        CompactString::from("__module__"),
        PyObject::str_val(CompactString::from("types")),
    );
    namespace.insert(
        CompactString::from("__dynamic_class_attribute_class__"),
        PyObject::bool_val(true),
    );
    PyObject::class(
        CompactString::from("DynamicClassAttribute"),
        vec![PyObject::builtin_type(CompactString::from("property"))],
        namespace,
    )
}
