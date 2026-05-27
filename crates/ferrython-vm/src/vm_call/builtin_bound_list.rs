use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    BuiltinBoundMethodData, IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_list_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
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
                    )
                    .map(Some);
                }
            }
        }

        if bbm.method_name.as_str() == "sort" {
            if let PyObjectPayload::List(items) = &bbm.receiver.payload {
                let mut key_fn: Option<PyObjectRef> = None;
                let mut reverse = false;
                for arg in args {
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
                    return Ok(Some(PyObject::none()));
                } else {
                    let mut w = items.write();
                    let mut v: Vec<_> = w.drain(..).collect();
                    self.vm_sort(&mut v)?;
                    if reverse {
                        v.reverse();
                    }
                    w.extend(v);
                    return Ok(Some(PyObject::none()));
                }
            }
        }

        Ok(None)
    }
}
