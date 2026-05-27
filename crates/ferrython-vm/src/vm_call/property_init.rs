use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
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
            let mut attrs = inst.attrs.write();
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
            ferrython_core::object::property_set_doc(instance, doc)?;
        }
        Ok(())
    }
}
