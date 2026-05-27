use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn check_abstract_class_instantiation(&self, cls: &PyObjectRef) -> PyResult<()> {
        // ── ABC check (only for non-simple classes) ──
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let is_abstract_marker = |val: &PyObjectRef| -> bool {
                if let PyObjectPayload::Tuple(items) = &val.payload {
                    items.len() == 2 && items[0].as_str() == Some("__abstract__")
                } else if let PyObjectPayload::Property(pd) = &val.payload {
                    if let Some(fg) = &pd.fget {
                        if let PyObjectPayload::Tuple(items) = &fg.payload {
                            return items.len() == 2 && items[0].as_str() == Some("__abstract__");
                        }
                    }
                    false
                } else {
                    false
                }
            };
            let mut abstract_names: Vec<String> = Vec::new();
            let mut lineage: Vec<PyObjectRef> = cd.mro.iter().rev().cloned().collect();
            lineage.push(cls.clone());
            for class_obj in lineage {
                let PyObjectPayload::Class(class_cd) = &class_obj.payload else {
                    continue;
                };
                let ns = class_cd.namespace.read();
                let mut class_abstract_names: Vec<String> = Vec::new();
                if let Some(abs_methods) = ns.get("__abstractmethods__") {
                    match &abs_methods.payload {
                        PyObjectPayload::Set(set) => {
                            for key in set.read().keys() {
                                if let HashableKey::Str(name) = key {
                                    if !class_abstract_names
                                        .iter()
                                        .any(|existing| existing == name.as_str())
                                    {
                                        class_abstract_names.push(name.to_string());
                                    }
                                }
                            }
                        }
                        PyObjectPayload::FrozenSet(set) => {
                            for key in set.keys() {
                                if let HashableKey::Str(name) = key {
                                    if !class_abstract_names
                                        .iter()
                                        .any(|existing| existing == name.as_str())
                                    {
                                        class_abstract_names.push(name.to_string());
                                    }
                                }
                            }
                        }
                        PyObjectPayload::Tuple(items) => {
                            for item in items.iter() {
                                let name = item.py_to_string();
                                if !class_abstract_names
                                    .iter()
                                    .any(|existing| existing == &name)
                                {
                                    class_abstract_names.push(name);
                                }
                            }
                        }
                        PyObjectPayload::List(items) => {
                            for item in items.read().iter() {
                                let name = item.py_to_string();
                                if !class_abstract_names
                                    .iter()
                                    .any(|existing| existing == &name)
                                {
                                    class_abstract_names.push(name);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                for (name, val) in ns.iter() {
                    if is_abstract_marker(val) {
                        if !class_abstract_names
                            .iter()
                            .any(|existing| existing == name.as_str())
                        {
                            class_abstract_names.push(name.to_string());
                        }
                    } else if !class_abstract_names
                        .iter()
                        .any(|existing| existing == name.as_str())
                    {
                        abstract_names.retain(|existing| existing != name.as_str());
                    }
                }
                for name in class_abstract_names {
                    if !abstract_names.iter().any(|existing| existing == &name) {
                        abstract_names.push(name);
                    }
                }
            }
            if !abstract_names.is_empty() {
                abstract_names.sort();
                return Err(PyException::type_error(format!(
                    "Can't instantiate abstract class {} with abstract method{}{}",
                    cd.name,
                    if abstract_names.len() > 1 { "s " } else { " " },
                    abstract_names.join(", ")
                )));
            }
        }
        Ok(())
    }
}
