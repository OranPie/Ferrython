use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
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
}
