use compact_str::CompactString;
use ferrython_core::object::{PyObjectPayload, PyObjectRef};

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
}
