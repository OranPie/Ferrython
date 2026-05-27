use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PropertyData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use std::cell::Cell;

use crate::VirtualMachine;

impl VirtualMachine {
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
