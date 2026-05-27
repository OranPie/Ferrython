use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PropertyData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use std::cell::Cell;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn builtin_type_instance_operand(
        type_name: &str,
        method_name: &str,
        args: &[PyObjectRef],
    ) -> PyResult<(PyObjectRef, Vec<PyObjectRef>)> {
        let Some(instance) = args.first() else {
            return Err(PyException::type_error(format!(
                "unbound method {}.{}() needs an argument",
                type_name, method_name
            )));
        };
        let matches_receiver = match type_name {
            "bytes" => matches!(&instance.payload, PyObjectPayload::Bytes(_)),
            "bytearray" => matches!(&instance.payload, PyObjectPayload::ByteArray(_)),
            _ => false,
        };
        if matches_receiver {
            return Ok((instance.clone(), args[1..].to_vec()));
        }
        if let PyObjectPayload::Instance(inst) = &instance.payload {
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                if cd.builtin_base_name.as_ref().map(|s| s.as_str()) == Some(type_name) {
                    if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                        return Ok((value, args[1..].to_vec()));
                    }
                }
            }
        }
        Err(PyException::type_error(format!(
            "descriptor '{}' for '{}' objects doesn't apply to a '{}' object",
            method_name,
            type_name,
            instance.type_name()
        )))
    }

    pub(super) fn split_trailing_kwargs_dict(
        args: &[PyObjectRef],
    ) -> (Vec<PyObjectRef>, Vec<(CompactString, PyObjectRef)>) {
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(map) = &last.payload {
                let kwargs = map
                    .read()
                    .iter()
                    .filter_map(|(key, value)| match key {
                        HashableKey::Str(name) => {
                            Some((CompactString::from(name.as_str()), value.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                return (args[..args.len() - 1].to_vec(), kwargs);
            }
        }
        (args.to_vec(), Vec::new())
    }

    pub(super) fn property_arg(
        args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
        idx: usize,
        name: &str,
    ) -> Option<PyObjectRef> {
        kwargs
            .iter()
            .find(|(k, _)| k.as_str() == name)
            .map(|(_, v)| v.clone())
            .or_else(|| args.get(idx).cloned())
            .filter(|v| !matches!(&v.payload, PyObjectPayload::None))
    }

    pub(super) fn raw_property_arg(
        args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
        idx: usize,
        name: &str,
    ) -> Option<PyObjectRef> {
        kwargs
            .iter()
            .find(|(k, _)| k.as_str() == name)
            .map(|(_, v)| v.clone())
            .or_else(|| args.get(idx).cloned())
    }

    pub(super) fn init_property_instance_attrs(
        instance: &PyObjectRef,
        args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<()> {
        let fget = Self::property_arg(args, kwargs, 0, "fget");
        let fset = Self::property_arg(args, kwargs, 1, "fset");
        let fdel = Self::property_arg(args, kwargs, 2, "fdel");
        let doc_arg = Self::raw_property_arg(args, kwargs, 3, "doc");
        let (doc, doc_from_getter) =
            ferrython_core::object::property_init_doc(fget.as_ref(), doc_arg);
        if let PyObjectPayload::Instance(inst) = &instance.payload {
            let mut w = inst.attrs.write();
            w.insert(
                CompactString::from("fget"),
                fget.unwrap_or_else(PyObject::none),
            );
            w.insert(
                CompactString::from("fset"),
                fset.unwrap_or_else(PyObject::none),
            );
            w.insert(
                CompactString::from("fdel"),
                fdel.unwrap_or_else(PyObject::none),
            );
            w.insert(
                CompactString::from("__property_doc_from_getter__"),
                PyObject::bool_val(doc_from_getter),
            );
        }
        if let Some(doc) = doc {
            ferrython_core::object::property_set_doc(instance, doc)?;
        }
        Ok(())
    }

    pub(super) fn property_callable_field(prop: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
        ferrython_core::object::property_field(prop, name)
            .filter(|v| !matches!(&v.payload, PyObjectPayload::None))
    }

    pub(super) fn property_doc_from_getter_flag(prop: &PyObjectRef) -> bool {
        match &prop.payload {
            PyObjectPayload::Property(pd) => pd.doc_from_getter.get(),
            PyObjectPayload::Instance(inst)
                if ferrython_core::object::is_property_subclass_class(&inst.class) =>
            {
                inst.attrs
                    .read()
                    .get("__property_doc_from_getter__")
                    .map(|v| v.is_truthy())
                    .unwrap_or(true)
            }
            _ => false,
        }
    }

    pub(super) fn make_property_like(
        template: &PyObjectRef,
        fget: Option<PyObjectRef>,
        fset: Option<PyObjectRef>,
        fdel: Option<PyObjectRef>,
        doc: Option<PyObjectRef>,
        doc_from_getter: bool,
    ) -> PyResult<PyObjectRef> {
        match &template.payload {
            PyObjectPayload::Property(_) => Ok(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::Property(Box::new(PropertyData {
                    fget,
                    fset,
                    fdel,
                    doc: PyCell::new(doc),
                    doc_from_getter: Cell::new(doc_from_getter),
                })),
            })),
            PyObjectPayload::Instance(inst)
                if ferrython_core::object::is_property_subclass_class(&inst.class) =>
            {
                let obj = PyObject::instance(inst.class.clone());
                if let PyObjectPayload::Instance(new_inst) = &obj.payload {
                    let mut attrs = new_inst.attrs.write();
                    attrs.insert(
                        CompactString::from("fget"),
                        fget.unwrap_or_else(PyObject::none),
                    );
                    attrs.insert(
                        CompactString::from("fset"),
                        fset.unwrap_or_else(PyObject::none),
                    );
                    attrs.insert(
                        CompactString::from("fdel"),
                        fdel.unwrap_or_else(PyObject::none),
                    );
                    attrs.insert(
                        CompactString::from("__property_doc_from_getter__"),
                        PyObject::bool_val(doc_from_getter),
                    );
                }
                if let Some(doc) = doc {
                    ferrython_core::object::property_set_doc(&obj, doc)?;
                }
                Ok(obj)
            }
            _ => Err(PyException::attribute_error(format!(
                "'{}' object has no property methods",
                template.type_name()
            ))),
        }
    }
}
