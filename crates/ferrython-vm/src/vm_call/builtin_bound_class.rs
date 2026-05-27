use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{BuiltinBoundMethodData, PyObject, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_class_or_property_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if let PyObjectPayload::Class(cd) = &bbm.receiver.payload {
            match bbm.method_name.as_str() {
                "__subclasses__" => {
                    let subs = cd.subclasses.read();
                    let alive: Vec<PyObjectRef> = subs.iter().filter_map(|w| w.upgrade()).collect();
                    drop(subs);
                    cd.subclasses.write().retain(|w| w.strong_count() > 0);
                    return Ok(Some(PyObject::list(alive)));
                }
                "mro" => {
                    let mut mro_list = vec![bbm.receiver.clone()];
                    mro_list.extend(cd.mro.iter().cloned());
                    return Ok(Some(PyObject::list(mro_list)));
                }
                _ => {}
            }
        }

        if ferrython_core::object::is_property_like(&bbm.receiver) && args.len() == 1 {
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
            )
            .map(Some);
        }

        Ok(None)
    }
}
