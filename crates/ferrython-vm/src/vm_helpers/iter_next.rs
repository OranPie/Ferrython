use crate::builtins;
use crate::builtins::advance_deque_iter;
use crate::VirtualMachine;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::{
    py_int_from_bigint, range_iter_item_bigint, range_iter_len_bigint, range_next_i64,
};
use ferrython_core::object::{
    IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, WeakKeyIterKind,
    WeakValueIterKind,
};
use std::rc::Rc;

impl VirtualMachine {
    /// Advance any iterable by one step (generators, iterators, instances with __next__).
    /// Returns Ok(Some(value)) on success, Ok(None) on exhaustion (StopIteration).
    pub(crate) fn vm_iter_next(&mut self, iter_obj: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
        match &iter_obj.payload {
            PyObjectPayload::Generator(gen_arc) => {
                match self.resume_generator(gen_arc, PyObject::none()) {
                    Ok(val) => Ok(Some(val)),
                    Err(e) if e.kind == ExceptionKind::StopIteration => Ok(None),
                    Err(e) => Err(e),
                }
            }
            PyObjectPayload::RangeIter(ri) => {
                match range_next_i64(ri.current.get(), ri.stop, ri.step) {
                    Some((value, next)) => {
                        ri.current.set(next);
                        Ok(Some(PyObject::int(value)))
                    }
                    None => Ok(None),
                }
            }
            PyObjectPayload::Instance(_) => {
                if let Some(next_method) = iter_obj.get_attr("__next__") {
                    match self.call_object(next_method, vec![]) {
                        Ok(val) => Ok(Some(val)),
                        Err(e) if e.kind == ExceptionKind::StopIteration => Ok(None),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(PyException::type_error("iterator has no __next__ method"))
                }
            }
            // Module with __next__ (e.g. file objects)
            PyObjectPayload::Module(_) => {
                if let Some(next_fn) = iter_obj.get_attr("__next__") {
                    // Module.get_attr returns the raw function (no bound-method wrapping),
                    // so pass the module itself as self.
                    match self.call_object(next_fn, vec![iter_obj.clone()]) {
                        Ok(val) => Ok(Some(val)),
                        Err(e) if e.kind == ExceptionKind::StopIteration => Ok(None),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(PyException::type_error(format!(
                        "'{}' object is not an iterator",
                        iter_obj.type_name()
                    )))
                }
            }
            PyObjectPayload::Iterator(iter_data_arc) => {
                {
                    let mut data = iter_data_arc.write();
                    match &mut *data {
                        IteratorData::BigRange(iter) => {
                            if range_iter_len_bigint(iter) == num_bigint::BigInt::from(0) {
                                return Ok(None);
                            }
                            let value = py_int_from_bigint(range_iter_item_bigint(iter));
                            iter.index += 1;
                            return Ok(Some(value));
                        }
                        IteratorData::Enumerate { .. }
                        | IteratorData::Zip { .. }
                        | IteratorData::ZipLongest { .. }
                        | IteratorData::Islice { .. }
                        | IteratorData::MapOne { .. }
                        | IteratorData::Map { .. }
                        | IteratorData::Filter { .. }
                        | IteratorData::FilterFalse { .. }
                        | IteratorData::Sentinel { .. }
                        | IteratorData::TakeWhile { .. }
                        | IteratorData::DropWhile { .. }
                        | IteratorData::Count { .. }
                        | IteratorData::Cycle { .. }
                        | IteratorData::Repeat { .. }
                        | IteratorData::Chain { .. }
                        | IteratorData::SeqIter { .. }
                        | IteratorData::Starmap { .. }
                        | IteratorData::Tee { .. }
                        | IteratorData::HeldIter { .. } => {
                            drop(data);
                            return self.advance_lazy_iterator(iter_obj);
                        }
                        _ => {}
                    }
                }
                // Standard iterators
                match builtins::iter_advance(iter_obj)? {
                    Some((_new_iter, value)) => Ok(Some(value)),
                    None => Ok(None),
                }
            }
            PyObjectPayload::VecIter(data) => {
                let idx = data.index.get();
                if idx < data.items.len() {
                    let v = data.items[idx].clone();
                    data.index.set(idx + 1);
                    Ok(Some(v))
                } else {
                    Ok(None)
                }
            }
            PyObjectPayload::WeakValueIter(data) => loop {
                let idx = data.index.get();
                if idx >= data.entries.len() {
                    return Ok(None);
                }
                data.index.set(idx + 1);
                let (key, ref_obj) = &data.entries[idx];
                let Some(target_fn) = ref_obj.get_attr("__weakref_target__") else {
                    continue;
                };
                let value = match self.call_object(target_fn, vec![]) {
                    Ok(obj) if !matches!(&obj.payload, PyObjectPayload::None) => obj,
                    Ok(_) => continue,
                    Err(_) => continue,
                };
                return Ok(Some(match data.kind {
                    WeakValueIterKind::Keys => key.clone(),
                    WeakValueIterKind::Values => value,
                    WeakValueIterKind::Items => PyObject::tuple(vec![key.clone(), value]),
                }));
            },
            PyObjectPayload::WeakKeyIter(data) => loop {
                let idx = data.index.get();
                if idx >= data.entries.len() {
                    return Ok(None);
                }
                data.index.set(idx + 1);
                let (ref_obj, value) = &data.entries[idx];
                let Some(target_fn) = ref_obj.get_attr("__weakref_target__") else {
                    continue;
                };
                let key = match self.call_object(target_fn, vec![]) {
                    Ok(obj) if !matches!(&obj.payload, PyObjectPayload::None) => obj,
                    Ok(_) => continue,
                    Err(_) => continue,
                };
                return Ok(Some(match data.kind {
                    WeakKeyIterKind::Keys => key,
                    WeakKeyIterKind::Items => PyObject::tuple(vec![key, value.clone()]),
                }));
            },
            PyObjectPayload::DequeIter(data) => advance_deque_iter(data),
            PyObjectPayload::RefIter { source, index } => {
                if index.get() == usize::MAX {
                    return Ok(None);
                }
                let idx = index.get();
                match &source.payload {
                    PyObjectPayload::List(cell) => {
                        let items = unsafe { &*cell.data_ptr() };
                        if idx < items.len() {
                            let v = items[idx].clone();
                            index.set(idx + 1);
                            Ok(Some(v))
                        } else {
                            index.set(usize::MAX);
                            Ok(None)
                        }
                    }
                    PyObjectPayload::Tuple(items) => {
                        if idx < items.len() {
                            let v = items[idx].clone();
                            index.set(idx + 1);
                            Ok(Some(v))
                        } else {
                            index.set(usize::MAX);
                            Ok(None)
                        }
                    }
                    PyObjectPayload::Dict(cell)
                    | PyObjectPayload::MappingProxy(cell)
                    | PyObjectPayload::DictKeys { map: cell, .. } => {
                        let map = unsafe { &*cell.data_ptr() };
                        if idx < map.len() {
                            let v = map.get_index(idx).unwrap().0.to_object();
                            index.set(idx + 1);
                            Ok(Some(v))
                        } else {
                            index.set(usize::MAX);
                            Ok(None)
                        }
                    }
                    _ => Ok(None),
                }
            }
            PyObjectPayload::RevRefIter { source, index } => {
                let idx = index.get();
                if idx == usize::MAX {
                    return Ok(None);
                }
                if idx == 0 {
                    index.set(usize::MAX);
                    return Ok(None);
                }
                match &source.payload {
                    PyObjectPayload::List(cell) => {
                        let pos = idx - 1;
                        let items = unsafe { &*cell.data_ptr() };
                        if pos < items.len() {
                            let v = items[pos].clone();
                            index.set(pos);
                            Ok(Some(v))
                        } else {
                            index.set(usize::MAX);
                            Ok(None)
                        }
                    }
                    _ => Ok(None),
                }
            }
            _ => Err(PyException::type_error(format!(
                "'{}' object is not an iterator",
                iter_obj.type_name()
            ))),
        }
    }

    /// Advance lazy iterator variants (Enumerate, Zip, Map, Filter).
    pub(crate) fn advance_lazy_iterator(
        &mut self,
        iter_obj: &PyObjectRef,
    ) -> PyResult<Option<PyObjectRef>> {
        let iter_data_arc = match &iter_obj.payload {
            PyObjectPayload::Iterator(arc) => arc.clone(),
            _ => return Err(PyException::type_error("not an iterator")),
        };
        let mut data = iter_data_arc.write();
        match &mut *data {
            IteratorData::Enumerate { source, index, .. } => {
                let src = source.clone();
                let idx = *index;
                *index += 1;
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(val) => Ok(Some(PyObject::tuple(vec![PyObject::int(idx), val]))),
                    None => Ok(None),
                }
            }
            IteratorData::Zip {
                sources, strict, ..
            } => {
                let srcs: Vec<PyObjectRef> = sources.clone();
                let is_strict = *strict;
                drop(data);
                let mut items = Vec::with_capacity(srcs.len());
                let mut exhausted = Vec::new();
                for (i, src) in srcs.iter().enumerate() {
                    match self.vm_iter_next(src)? {
                        Some(val) => items.push(val),
                        None => {
                            if is_strict {
                                exhausted.push(i);
                                // Continue checking remaining sources
                                items.push(PyObject::none());
                            } else {
                                return Ok(None);
                            }
                        }
                    }
                }
                if is_strict && !exhausted.is_empty() {
                    if exhausted.len() != srcs.len() {
                        return Err(PyException::value_error(
                            "zip() has arguments with different lengths",
                        ));
                    }
                    return Ok(None); // All exhausted at same time
                }
                Ok(Some(PyObject::tuple(items)))
            }
            IteratorData::ZipLongest {
                sources,
                active,
                fillvalue,
                ..
            } => {
                let srcs = sources.clone();
                let fill = fillvalue.clone();
                let mut active_state = active.clone();
                drop(data);
                if active_state.iter().all(|flag| !*flag) {
                    return Ok(None);
                }
                let mut items = Vec::with_capacity(srcs.len());
                let mut yielded_real = false;
                for (idx, src) in srcs.iter().enumerate() {
                    if !active_state.get(idx).copied().unwrap_or(false) {
                        items.push(fill.clone());
                        continue;
                    }
                    match self.vm_iter_next(src)? {
                        Some(value) => {
                            yielded_real = true;
                            items.push(value);
                        }
                        None => {
                            if let Some(flag) = active_state.get_mut(idx) {
                                *flag = false;
                            }
                            items.push(fill.clone());
                        }
                    }
                }
                let mut data = iter_data_arc.write();
                if let IteratorData::ZipLongest { active, .. } = &mut *data {
                    *active = active_state;
                }
                if yielded_real {
                    Ok(Some(PyObject::tuple(items)))
                } else {
                    Ok(None)
                }
            }
            IteratorData::Islice {
                source,
                index,
                next_yield,
                stop,
                step,
            } => {
                let src = source.clone();
                let mut idx = *index;
                let mut next = *next_yield;
                let stop_at = *stop;
                let step_by = (*step).max(1);
                drop(data);
                while idx < stop_at {
                    match self.vm_iter_next(&src)? {
                        Some(value) => {
                            if idx == next {
                                next = next.saturating_add(step_by);
                                idx = idx.saturating_add(1);
                                let mut data = iter_data_arc.write();
                                if let IteratorData::Islice {
                                    index, next_yield, ..
                                } = &mut *data
                                {
                                    *index = idx;
                                    *next_yield = next;
                                }
                                return Ok(Some(value));
                            }
                            idx = idx.saturating_add(1);
                        }
                        None => {
                            let mut data = iter_data_arc.write();
                            if let IteratorData::Islice { source, index, .. } = &mut *data {
                                *source = PyObject::none();
                                *index = stop_at;
                            }
                            return Ok(None);
                        }
                    }
                }
                let mut data = iter_data_arc.write();
                if let IteratorData::Islice { source, index, .. } = &mut *data {
                    *source = PyObject::none();
                    *index = stop_at;
                }
                Ok(None)
            }
            IteratorData::MapOne { func, source } => {
                let f = func.clone();
                let src = source.clone();
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(val) => {
                        let result = self.call_object_one_arg_fast_or_fallback(f, val)?;
                        Ok(Some(result))
                    }
                    None => Ok(None),
                }
            }
            IteratorData::Map { func, sources } => {
                let f = func.clone();
                let srcs = sources.clone();
                drop(data);
                let mut call_args = Vec::with_capacity(srcs.len());
                for src in &srcs {
                    match self.vm_iter_next(src)? {
                        Some(val) => call_args.push(val),
                        None => return Ok(None),
                    }
                }
                let result = self.call_object(f, call_args)?;
                Ok(Some(result))
            }
            IteratorData::Filter { func, source } => {
                let f = func.clone();
                let src = source.clone();
                drop(data);
                loop {
                    match self.vm_iter_next(&src)? {
                        Some(val) => {
                            let test_result = if matches!(&f.payload, PyObjectPayload::None) {
                                self.vm_is_truthy(&val)?
                            } else {
                                let r = self.call_object(f.clone(), vec![val.clone()])?;
                                self.vm_is_truthy(&r)?
                            };
                            if test_result {
                                return Ok(Some(val));
                            }
                        }
                        None => return Ok(None),
                    }
                }
            }
            IteratorData::FilterFalse { func, source } => {
                let f = func.clone();
                let src = source.clone();
                drop(data);
                loop {
                    match self.vm_iter_next(&src)? {
                        Some(val) => {
                            let test_result = if matches!(&f.payload, PyObjectPayload::None) {
                                self.vm_is_truthy(&val)?
                            } else {
                                let r = self.call_object(f.clone(), vec![val.clone()])?;
                                self.vm_is_truthy(&r)?
                            };
                            if !test_result {
                                return Ok(Some(val));
                            }
                        }
                        None => return Ok(None),
                    }
                }
            }
            IteratorData::Sentinel {
                callable,
                sentinel,
                done,
            } => {
                if *done {
                    drop(data);
                    return Ok(None);
                }
                let f = callable.clone();
                let s = sentinel.clone();
                drop(data);
                let val = self.call_object(f, vec![])?;
                let eq_result = val.compare(&s, ferrython_core::object::CompareOp::Eq)?;
                if eq_result.is_truthy() {
                    if let PyObjectPayload::Iterator(arc) = &iter_obj.payload {
                        if let IteratorData::Sentinel { done, .. } = &mut *arc.write() {
                            *done = true;
                        }
                    }
                    Ok(None)
                } else {
                    Ok(Some(val))
                }
            }
            IteratorData::TakeWhile { func, source, done } => {
                if *done {
                    drop(data);
                    return Ok(None);
                }
                let f = func.clone();
                let src = source.clone();
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(val) => {
                        let test = self.call_object(f, vec![val.clone()])?;
                        if self.vm_is_truthy(&test)? {
                            Ok(Some(val))
                        } else {
                            // Mark done
                            if let PyObjectPayload::Iterator(arc) = &iter_obj.payload {
                                if let IteratorData::TakeWhile { done, .. } = &mut *arc.write() {
                                    *done = true;
                                }
                            }
                            Ok(None)
                        }
                    }
                    None => Ok(None),
                }
            }
            IteratorData::DropWhile {
                func,
                source,
                dropping,
            } => {
                let f = func.clone();
                let src = source.clone();
                let is_dropping = *dropping;
                drop(data);
                if is_dropping {
                    loop {
                        match self.vm_iter_next(&src)? {
                            Some(val) => {
                                let test = self.call_object(f.clone(), vec![val.clone()])?;
                                if !self.vm_is_truthy(&test)? {
                                    // Stop dropping, mark state
                                    if let PyObjectPayload::Iterator(arc) = &iter_obj.payload {
                                        if let IteratorData::DropWhile { dropping, .. } =
                                            &mut *arc.write()
                                        {
                                            *dropping = false;
                                        }
                                    }
                                    return Ok(Some(val));
                                }
                                // Keep dropping
                            }
                            None => return Ok(None),
                        }
                    }
                } else {
                    // Not dropping anymore, just yield
                    self.vm_iter_next(&src)
                }
            }
            IteratorData::Count { current, step } => {
                let val = *current;
                *current += *step;
                drop(data);
                Ok(Some(PyObject::int(val)))
            }
            IteratorData::Cycle { items, index } => {
                if items.is_empty() {
                    drop(data);
                    return Ok(None);
                }
                let val = items[*index].clone();
                *index = (*index + 1) % items.len();
                drop(data);
                Ok(Some(val))
            }
            IteratorData::Repeat { item, remaining } => match remaining {
                Some(0) => {
                    drop(data);
                    Ok(None)
                }
                Some(ref mut n) => {
                    let val = item.clone();
                    *n -= 1;
                    drop(data);
                    Ok(Some(val))
                }
                None => {
                    let val = item.clone();
                    drop(data);
                    Ok(Some(val))
                }
            },
            IteratorData::Chain { sources, current } => {
                // Clone what we need, then drop lock
                let srcs = sources.clone();
                let mut cur = *current;
                drop(data);
                while cur < srcs.len() {
                    match self.vm_iter_next(&srcs[cur])? {
                        Some(val) => {
                            // Update current index
                            let mut d = iter_data_arc.write();
                            if let IteratorData::Chain { current, .. } = &mut *d {
                                *current = cur;
                            }
                            return Ok(Some(val));
                        }
                        None => {
                            cur += 1;
                        }
                    }
                }
                // All exhausted
                let mut d = iter_data_arc.write();
                if let IteratorData::Chain { current, .. } = &mut *d {
                    *current = cur;
                }
                Ok(None)
            }
            IteratorData::Starmap { func, source } => {
                let f = func.clone();
                let src = source.clone();
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(args_tuple) => {
                        let call_args = args_tuple
                            .to_list()
                            .unwrap_or_else(|_| vec![args_tuple.clone()]);
                        let result = self.call_object(f, call_args)?;
                        Ok(Some(result))
                    }
                    None => Ok(None),
                }
            }
            IteratorData::Tee {
                source,
                buffer,
                index,
            } => {
                let src_cell = Rc::clone(source);
                let buf_cell = Rc::clone(buffer);
                let idx = *index;
                drop(data);

                // Fast path: item already in shared buffer
                {
                    let buf = buf_cell.read();
                    if idx < buf.len() {
                        let val = buf[idx].clone();
                        drop(buf);
                        let mut d = iter_data_arc.write();
                        if let IteratorData::Tee { index, .. } = &mut *d {
                            *index = idx + 1;
                        }
                        return Ok(Some(val));
                    }
                }

                // Need to pull from source; we own the right to pull item `idx`
                let src = src_cell.read().clone();
                match self.vm_iter_next(&src)? {
                    Some(val) => {
                        buf_cell.write().push(val.clone());
                        let mut d = iter_data_arc.write();
                        if let IteratorData::Tee { index, .. } = &mut *d {
                            *index = idx + 1;
                        }
                        Ok(Some(val))
                    }
                    None => Ok(None),
                }
            }
            IteratorData::SeqIter {
                obj,
                index,
                exhausted,
            } => {
                if *exhausted {
                    drop(data);
                    return Ok(None);
                }
                let src = obj.clone();
                let idx = *index;
                if idx >= isize::MAX as i64 {
                    drop(data);
                    return Err(PyException::overflow_error("iter index too large"));
                }
                drop(data);
                let getitem = match src.get_attr("__getitem__") {
                    Some(f) => f,
                    None => {
                        let mut d = iter_data_arc.write();
                        if let IteratorData::SeqIter { obj, exhausted, .. } = &mut *d {
                            *exhausted = true;
                            let old_source = std::mem::replace(obj, PyObject::list(vec![]));
                            drop(d);
                            drop(old_source);
                        } else {
                            drop(d);
                        }
                        return Ok(None);
                    }
                };
                match self.call_object(getitem, vec![PyObject::int(idx)]) {
                    Ok(val) => {
                        let mut d = iter_data_arc.write();
                        if let IteratorData::SeqIter { index, .. } = &mut *d {
                            *index = idx + 1;
                        }
                        Ok(Some(val))
                    }
                    Err(e)
                        if e.kind == ExceptionKind::StopIteration
                            || e.kind == ExceptionKind::IndexError =>
                    {
                        let mut d = iter_data_arc.write();
                        if let IteratorData::SeqIter { obj, exhausted, .. } = &mut *d {
                            *exhausted = true;
                            let old_source = std::mem::replace(obj, PyObject::list(vec![]));
                            drop(d);
                            drop(old_source);
                        } else {
                            drop(d);
                        }
                        Ok(None)
                    }
                    Err(e) => Err(e),
                }
            }
            IteratorData::HeldIter { iter, .. } => {
                let inner = iter.clone();
                drop(data);
                match self.vm_iter_next(&inner)? {
                    Some(value) => Ok(Some(value)),
                    None => {
                        let mut d = iter_data_arc.write();
                        if let IteratorData::HeldIter { owner, .. } = &mut *d {
                            *owner = None;
                        }
                        Ok(None)
                    }
                }
            }
            _ => {
                drop(data);
                match builtins::iter_advance(iter_obj)? {
                    Some((_new, val)) => Ok(Some(val)),
                    None => Ok(None),
                }
            }
        }
    }
}
