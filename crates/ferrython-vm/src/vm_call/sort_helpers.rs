use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyInt;

use crate::vm_helpers::{begin_list_sort_guard, list_sort_was_mutated};
use crate::VirtualMachine;

impl VirtualMachine {
    pub(crate) fn vm_sort_list_in_place(
        &mut self,
        receiver: &PyObjectRef,
        key_fn: Option<PyObjectRef>,
        reverse: bool,
    ) -> PyResult<()> {
        let PyObjectPayload::List(items_arc) = &receiver.payload else {
            return Err(PyException::type_error(
                "descriptor 'sort' for 'list' objects doesn't apply",
            ));
        };
        let guard = begin_list_sort_guard(receiver);
        let original = items_arc.read().clone();
        let mut items_vec = original.clone();
        let sort_result = self.sort_with_key(&mut items_vec, key_fn, reverse);
        self.drain_pending_finalizers();
        if sort_result.is_err() {
            return sort_result;
        }
        if list_sort_was_mutated(&guard) {
            *items_arc.write() = original;
            return Err(PyException::value_error("list modified during sort"));
        }
        *items_arc.write() = items_vec;
        Ok(())
    }

    /// Schwartzian transform: sort items by key function, optionally reversed.
    pub(super) fn sort_with_key(
        &mut self,
        items: &mut Vec<PyObjectRef>,
        key_fn: Option<PyObjectRef>,
        reverse: bool,
    ) -> PyResult<()> {
        if let Some(key) = key_fn {
            if matches!(&key.payload, PyObjectPayload::None) {
                self.vm_sort(items)?;
                if reverse {
                    items.reverse();
                }
                return Ok(());
            }
            // Check if key is a cmp_to_key class — use comparison function directly
            if let PyObjectPayload::Class(cd) = &key.payload {
                if let Some(cmp_func) = cd.namespace.read().get("__cmp_to_key_func__").cloned() {
                    // Sort using comparison function: cmp(a, b) < 0 means a < b
                    let mut indices: Vec<usize> = (0..items.len()).collect();
                    for i in 1..indices.len() {
                        let mut j = i;
                        while j > 0 {
                            let a = &items[indices[j]];
                            let b = &items[indices[j - 1]];
                            let result =
                                self.call_object(cmp_func.clone(), vec![a.clone(), b.clone()])?;
                            let cmp_val = result.to_int().unwrap_or(0);
                            if cmp_val < 0 {
                                indices.swap(j, j - 1);
                                j -= 1;
                            } else {
                                break;
                            }
                        }
                    }
                    *items = indices.into_iter().map(|i| items[i].clone()).collect();
                    if reverse {
                        items.reverse();
                    }
                    return Ok(());
                }
            }
            // Normal key function sort
            let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
            for item in items.iter() {
                let k = self.call_object(key.clone(), vec![item.clone()])?;
                decorated.push((k, item.clone()));
            }
            let mut indices: Vec<usize> = (0..decorated.len()).collect();
            for i in 1..indices.len() {
                let mut j = i;
                while j > 0 {
                    let cmp = if reverse {
                        // Sort descending directly for stable reverse
                        self.vm_lt(&decorated[indices[j - 1]].0, &decorated[indices[j]].0)?
                    } else {
                        self.vm_lt(&decorated[indices[j]].0, &decorated[indices[j - 1]].0)?
                    };
                    if cmp {
                        indices.swap(j, j - 1);
                        j -= 1;
                    } else {
                        break;
                    }
                }
            }
            *items = indices
                .into_iter()
                .map(|i| decorated[i].1.clone())
                .collect();
        } else {
            self.vm_sort(items)?;
            if reverse {
                items.reverse();
            }
        }
        Ok(())
    }

    /// Compute min or max from a collection, with optional key function and default value.
    pub(super) fn compute_min_max(
        &mut self,
        items: Vec<PyObjectRef>,
        is_max: bool,
        key_fn: Option<PyObjectRef>,
        default: Option<PyObjectRef>,
        func_name: &str,
    ) -> PyResult<PyObjectRef> {
        if items.is_empty() {
            return if let Some(d) = default {
                Ok(d)
            } else {
                Err(PyException::value_error(format!(
                    "{}() arg is an empty sequence",
                    func_name
                )))
            };
        }
        let mut best = items[0].clone();
        let mut best_key = if let Some(ref kf) = key_fn {
            self.call_object(kf.clone(), vec![best.clone()])?
        } else {
            best.clone()
        };
        for item in &items[1..] {
            let item_key = if let Some(ref kf) = key_fn {
                self.call_object(kf.clone(), vec![item.clone()])?
            } else {
                item.clone()
            };
            let better = if is_max {
                self.vm_lt(&best_key, &item_key)?
            } else {
                self.vm_lt(&item_key, &best_key)?
            };
            if better {
                best = item.clone();
                best_key = item_key;
            }
        }
        Ok(best)
    }

    /// Native i64 min/max for lists of small ints.
    /// Returns None if the arg is not a list or contains non-int items (caller falls back).
    pub(super) fn native_min_max_list(
        &self,
        arg: &PyObjectRef,
        is_max: bool,
    ) -> PyResult<Option<PyObjectRef>> {
        let items = match &arg.payload {
            PyObjectPayload::List(rc) => rc.read(),
            _ => return Ok(None),
        };
        if items.is_empty() {
            let name = if is_max { "max" } else { "min" };
            return Err(PyException::value_error(format!(
                "{}() arg is an empty sequence",
                name
            )));
        }
        let first = match &items[0].payload {
            PyObjectPayload::Int(PyInt::Small(n)) => *n,
            _ => return Ok(None),
        };
        let mut best = first;
        for item in &items[1..] {
            match &item.payload {
                PyObjectPayload::Int(PyInt::Small(n)) => {
                    let v = *n;
                    if is_max {
                        if v > best {
                            best = v;
                        }
                    } else {
                        if v < best {
                            best = v;
                        }
                    }
                }
                _ => return Ok(None),
            }
        }
        Ok(Some(PyObject::int(best)))
    }
}
