use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn ast_constructor_owner(cls: &PyObjectRef) -> CompactString {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            cd.name.clone()
        } else {
            CompactString::from("AST")
        }
    }

    pub(super) fn ast_exact_legacy_name(cls: &PyObjectRef) -> Option<&'static str> {
        let PyObjectPayload::Class(cd) = &cls.payload else {
            return None;
        };
        match cd.name.as_str() {
            "Num" => Some("Num"),
            "Str" => Some("Str"),
            "Bytes" => Some("Bytes"),
            "NameConstant" => Some("NameConstant"),
            "Ellipsis" => Some("Ellipsis"),
            _ => None,
        }
    }

    pub(super) fn ast_find_mro_class(cls: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if cd.name.as_str() == name {
                return Some(cls.clone());
            }
            for base in &cd.mro {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    if bcd.name.as_str() == name {
                        return Some(base.clone());
                    }
                }
            }
        }
        None
    }

    pub(super) fn ast_constant_class(cls: &PyObjectRef) -> Option<PyObjectRef> {
        Self::ast_find_mro_class(cls, "Constant").or_else(|| {
            Self::ast_find_mro_class(cls, "Num")
                .and_then(|c| Self::ast_find_mro_class(&c, "Constant"))
        })
    }

    pub(super) fn populate_ast_node_attrs(
        instance: &PyObjectRef,
        cls: &PyObjectRef,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<()> {
        if Self::ast_class_name(cls).is_none() {
            return Ok(());
        }
        let owner = Self::ast_constructor_owner(cls);
        let exact_legacy = Self::ast_exact_legacy_name(cls);
        let fields = match exact_legacy {
            Some("Num") => vec![CompactString::from("n"), CompactString::from("kind")],
            Some("Str") | Some("Bytes") => {
                vec![CompactString::from("s"), CompactString::from("kind")]
            }
            Some("Ellipsis") => vec![CompactString::from("kind")],
            _ => Self::ast_class_fields(cls),
        };
        if pos_args.len() > fields.len() {
            let constructor_name = if exact_legacy.is_some() {
                "Constant"
            } else {
                owner.as_str()
            };
            return Err(PyException::type_error(format!(
                "{} constructor takes at most {} positional argument{}",
                constructor_name,
                fields.len(),
                if fields.len() == 1 { "" } else { "s" }
            )));
        }

        let mut writes: Vec<(CompactString, PyObjectRef)> = Vec::new();
        for (i, value) in pos_args.iter().enumerate() {
            if let Some(field) = fields.get(i) {
                writes.push((Self::ast_storage_name(field.as_str()), value.clone()));
            }
        }

        for (key, value) in kwargs {
            if let Some(pos) = fields
                .iter()
                .position(|field| field.as_str() == key.as_str())
            {
                if pos < pos_args.len() {
                    let duplicate_owner =
                        if matches!(exact_legacy, Some("Num" | "Str" | "Bytes" | "Ellipsis"))
                            && key.as_str() == "kind"
                        {
                            "Constant"
                        } else {
                            owner.as_str()
                        };
                    return Err(PyException::type_error(format!(
                        "{} got multiple values for argument '{}'",
                        duplicate_owner, key
                    )));
                }
            } else if key.as_str() == "value" {
                let value_taken = match exact_legacy {
                    Some("Ellipsis") => true,
                    Some("Num") | Some("Str") | Some("Bytes") => !pos_args.is_empty(),
                    _ => fields
                        .iter()
                        .take(pos_args.len())
                        .any(|field| field.as_str() == "value"),
                };
                if value_taken {
                    let duplicate_owner =
                        if matches!(exact_legacy, Some("Num" | "Str" | "Bytes" | "Ellipsis")) {
                            "Constant"
                        } else {
                            owner.as_str()
                        };
                    return Err(PyException::type_error(format!(
                        "{} got multiple values for argument '{}'",
                        duplicate_owner, key
                    )));
                }
            } else if key.as_str() == "kind" {
                let kind_taken = fields
                    .iter()
                    .take(pos_args.len())
                    .any(|field| field.as_str() == "kind");
                if kind_taken {
                    let duplicate_owner =
                        if matches!(exact_legacy, Some("Num" | "Str" | "Bytes" | "Ellipsis")) {
                            "Constant"
                        } else {
                            owner.as_str()
                        };
                    return Err(PyException::type_error(format!(
                        "{} got multiple values for argument '{}'",
                        duplicate_owner, key
                    )));
                }
            }
            writes.push((Self::ast_storage_name(key.as_str()), value.clone()));
        }

        if let PyObjectPayload::Instance(inst) = &instance.payload {
            let mut attrs = inst.attrs.write();
            for (key, value) in writes {
                attrs.insert(key, value);
            }
        }
        Ok(())
    }

    pub(super) fn try_instantiate_ast_node(
        &mut self,
        cls: &PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<Option<PyObjectRef>> {
        if Self::ast_class_name(cls).is_none() {
            return Ok(None);
        }

        let exact_legacy = Self::ast_exact_legacy_name(cls);
        let target_cls = if exact_legacy.is_some() {
            Self::ast_constant_class(cls).unwrap_or_else(|| cls.clone())
        } else {
            cls.clone()
        };
        let instance = PyObject::instance(target_cls);

        if exact_legacy.is_none() {
            if let Some(init) = cls.get_attr("__init__") {
                let is_builtin_init = matches!(&init.payload,
                    PyObjectPayload::BuiltinBoundMethod(bbm)
                        if matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(_)));
                let is_ast_native_init = matches!(&init.payload,
                    PyObjectPayload::NativeFunction(nf) if nf.name.as_str() == "_ast.AST.__init__")
                    || matches!(&init.payload,
                        PyObjectPayload::BoundMethod { method, .. }
                            if matches!(&method.payload, PyObjectPayload::NativeFunction(nf) if nf.name.as_str() == "_ast.AST.__init__"));
                if !is_builtin_init && !is_ast_native_init {
                    let init_fn = match &init.payload {
                        PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                        _ => init.clone(),
                    };
                    let mut init_args = vec![instance.clone()];
                    init_args.extend(pos_args);
                    let result = if kwargs.is_empty() {
                        self.call_object(init_fn, init_args)?
                    } else {
                        self.call_object_kw(init_fn, init_args, kwargs)?
                    };
                    if !matches!(&result.payload, PyObjectPayload::None) {
                        return Err(PyException::type_error(
                            "__init__() should return None, not '".to_string()
                                + result.type_name()
                                + "'",
                        ));
                    }
                    return Ok(Some(instance));
                }
            }
        }

        if exact_legacy == Some("Ellipsis") {
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                inst.attrs
                    .write()
                    .insert(CompactString::from("value"), PyObject::ellipsis());
            }
        }

        Self::populate_ast_node_attrs(&instance, cls, &pos_args, &kwargs)?;

        Ok(Some(instance))
    }
}
