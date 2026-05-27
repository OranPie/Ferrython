//! Inline iterator advancement helpers for hot VM paths.

use crate::VirtualMachine;
use ferrython_core::object::{IteratorData, PyObject, PyObjectPayload, PyObjectRef};

impl VirtualMachine {
    /// Advance a source iterator inline without VM dispatch.
    /// Works for List, Tuple, Range, and RangeIter sources.
    /// Returns `Some(Some(value))` if advanced, `Some(None)` if exhausted,
    /// `None` if the source type requires VM dispatch.
    #[inline(always)]
    pub(crate) fn advance_source_inline(source: &PyObjectRef) -> Option<Option<PyObjectRef>> {
        match &source.payload {
            PyObjectPayload::Iterator(arc) => {
                let mut data = arc.write();
                match &mut *data {
                    IteratorData::List { items, index } => {
                        if *index < items.len() {
                            let v = items[*index].clone();
                            *index += 1;
                            Some(Some(v))
                        } else {
                            Some(None)
                        }
                    }
                    IteratorData::Tuple { items, index } => {
                        if *index < items.len() {
                            let v = items[*index].clone();
                            *index += 1;
                            Some(Some(v))
                        } else {
                            Some(None)
                        }
                    }
                    IteratorData::Range {
                        current,
                        stop,
                        step,
                    } => {
                        let done = if *step > 0 {
                            *current >= *stop
                        } else {
                            *current <= *stop
                        };
                        if done {
                            Some(None)
                        } else {
                            let v = PyObject::int(*current);
                            *current += *step;
                            Some(Some(v))
                        }
                    }
                    _ => None,
                }
            }
            PyObjectPayload::RangeIter(ri) => {
                let cur = ri.current.get();
                let done = if ri.step > 0 {
                    cur >= ri.stop
                } else {
                    cur <= ri.stop
                };
                if done {
                    Some(None)
                } else {
                    ri.current.set(cur + ri.step);
                    Some(Some(PyObject::int(cur)))
                }
            }
            PyObjectPayload::VecIter(data) => {
                let idx = data.index.get();
                if idx < data.items.len() {
                    let v = data.items[idx].clone();
                    data.index.set(idx + 1);
                    Some(Some(v))
                } else {
                    Some(None)
                }
            }
            PyObjectPayload::RefIter { source, index } => {
                if index.get() == usize::MAX {
                    return Some(None);
                }
                let idx = index.get();
                match &source.payload {
                    PyObjectPayload::List(cell) => {
                        let items = unsafe { &*cell.data_ptr() };
                        if idx < items.len() {
                            let v = items[idx].clone();
                            index.set(idx + 1);
                            Some(Some(v))
                        } else {
                            index.set(usize::MAX);
                            Some(None)
                        }
                    }
                    PyObjectPayload::Tuple(items) => {
                        if idx < items.len() {
                            let v = items[idx].clone();
                            index.set(idx + 1);
                            Some(Some(v))
                        } else {
                            index.set(usize::MAX);
                            Some(None)
                        }
                    }
                    PyObjectPayload::Dict(cell)
                    | PyObjectPayload::MappingProxy(cell)
                    | PyObjectPayload::DictKeys { map: cell, .. } => {
                        let map = unsafe { &*cell.data_ptr() };
                        if idx < map.len() {
                            let v = map.get_index(idx).unwrap().0.to_object();
                            index.set(idx + 1);
                            Some(Some(v))
                        } else {
                            index.set(usize::MAX);
                            Some(None)
                        }
                    }
                    PyObjectPayload::DictValues { map: cell, .. } => {
                        let map = unsafe { &*cell.data_ptr() };
                        if idx < map.len() {
                            let v = map.get_index(idx).unwrap().1.clone();
                            index.set(idx + 1);
                            Some(Some(v))
                        } else {
                            index.set(usize::MAX);
                            Some(None)
                        }
                    }
                    PyObjectPayload::DictItems { map: cell, .. } => {
                        let map = unsafe { &*cell.data_ptr() };
                        if idx < map.len() {
                            let (k, v) = map.get_index(idx).unwrap();
                            let tuple = PyObject::tuple(vec![k.to_object(), v.clone()]);
                            index.set(idx + 1);
                            Some(Some(tuple))
                        } else {
                            index.set(usize::MAX);
                            Some(None)
                        }
                    }
                    _ => None,
                }
            }
            PyObjectPayload::RevRefIter { source, index } => {
                let idx = index.get();
                if idx == usize::MAX {
                    return Some(None);
                }
                if idx == 0 {
                    index.set(usize::MAX);
                    return Some(None);
                }
                match &source.payload {
                    PyObjectPayload::List(cell) => {
                        let pos = idx - 1;
                        let items = unsafe { &*cell.data_ptr() };
                        if pos < items.len() {
                            let v = items[pos].clone();
                            index.set(pos);
                            Some(Some(v))
                        } else {
                            index.set(usize::MAX);
                            Some(None)
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
