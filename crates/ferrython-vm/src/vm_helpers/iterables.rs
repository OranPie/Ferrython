use crate::builtins;
use crate::VirtualMachine;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, WeakKeyIterKind,
    WeakValueIterKind,
};
use indexmap::IndexMap;
use std::rc::Rc;

impl VirtualMachine {
    /// VM-level itertools.islice: lazily takes items from any iterable (including generators).
    pub(crate) fn vm_itertools_islice(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "islice() requires at least 2 arguments",
            ));
        }
        let iterable = &args[0];
        // None stop means no limit (use usize::MAX as sentinel)
        let (start, stop, step) = if args.len() == 2 {
            let stop = if matches!(&args[1].payload, PyObjectPayload::None) {
                usize::MAX
            } else {
                args[1].to_int()? as usize
            };
            (0usize, stop, 1usize)
        } else if args.len() == 3 {
            let s = if matches!(&args[1].payload, PyObjectPayload::None) {
                0
            } else {
                args[1].to_int()? as usize
            };
            let stop = if matches!(&args[2].payload, PyObjectPayload::None) {
                usize::MAX
            } else {
                args[2].to_int()? as usize
            };
            (s, stop, 1usize)
        } else {
            let s = if matches!(&args[1].payload, PyObjectPayload::None) {
                0
            } else {
                args[1].to_int()? as usize
            };
            let stop = if matches!(&args[2].payload, PyObjectPayload::None) {
                usize::MAX
            } else {
                args[2].to_int()? as usize
            };
            let st = if matches!(&args[3].payload, PyObjectPayload::None) {
                1
            } else {
                args[3].to_int()? as usize
            };
            (s, stop, st.max(1))
        };

        // For generators: consume items one at a time, only up to `stop`
        if let PyObjectPayload::Generator(gen_arc) = &iterable.payload {
            let gen_arc = gen_arc.clone();
            let mut result = Vec::new();
            let mut idx = 0usize;
            let mut next_yield = start;
            loop {
                if result.len() >= stop.saturating_sub(start) {
                    break;
                }
                if idx >= stop {
                    break;
                }
                match self.resume_generator(&gen_arc, PyObject::none()) {
                    Ok(value) => {
                        if idx == next_yield {
                            result.push(value);
                            next_yield += step;
                        }
                        idx += 1;
                    }
                    Err(e) if e.kind == ExceptionKind::StopIteration => break,
                    Err(e) => return Err(e),
                }
            }
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::List {
                    items: result,
                    index: 0,
                }),
            ))));
        }

        // For iterators with lazy data: advance one at a time
        if let PyObjectPayload::Iterator(_)
        | PyObjectPayload::RangeIter(..)
        | PyObjectPayload::VecIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. } = &iterable.payload
        {
            let mut result = Vec::new();
            let mut idx = 0usize;
            let mut next_yield = start;
            loop {
                if idx >= stop {
                    break;
                }
                match self.advance_lazy_iterator(iterable) {
                    Ok(Some(value)) => {
                        if idx == next_yield {
                            result.push(value);
                            next_yield += step;
                        }
                        idx += 1;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        // Try non-lazy advance
                        match builtins::iter_advance(iterable) {
                            Ok(Some((_, value))) => {
                                if idx == next_yield {
                                    result.push(value);
                                    next_yield += step;
                                }
                                idx += 1;
                            }
                            Ok(None) => break,
                            Err(_) => return Err(e),
                        }
                    }
                }
            }
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::List {
                    items: result,
                    index: 0,
                }),
            ))));
        }

        // For Instance with __iter__/__next__: iterate through VM
        if let PyObjectPayload::Instance(_) = &iterable.payload {
            if let Some(iter_method) = iterable.get_attr("__iter__") {
                let iter_obj = self.call_object(iter_method, vec![])?;
                // Recurse with the iterator
                let mut new_args = args.to_vec();
                new_args[0] = iter_obj;
                return self.vm_itertools_islice(&new_args);
            }
        }

        // Fallback: eagerly collect then slice (works for lists, tuples, etc.)
        let items = iterable.to_list()?;
        let result: Vec<PyObjectRef> = items
            .into_iter()
            .skip(start)
            .take(stop.saturating_sub(start))
            .step_by(step)
            .collect();
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: result,
                index: 0,
            }),
        ))))
    }

    /// Resolve an iterable object to its iterator by calling __iter__ if needed.
    /// For Instance objects with __iter__, calls __iter__() to get the real iterator.
    /// For builtin types (list, tuple, etc.), delegates to get_iter_from_obj.
    pub(crate) fn resolve_iterable(&mut self, obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            // dict subclass or namedtuple: use core get_iter
            if inst.dict_storage.is_some() || inst.class.get_attr("__namedtuple__").is_some() {
                return obj.get_iter();
            }
            // Custom __iter__: call it to get the actual iterator
            if let Some(iter_method) = obj.get_attr("__iter__") {
                let result = self.call_object(iter_method, vec![])?;
                return Self::ensure_iterator_result(obj, result);
            }
            // Builtin base type subclass: delegate to __builtin_value__
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                let iter = bv.get_iter()?;
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::HeldIter {
                        iter,
                        owner: Some(obj.clone()),
                    }),
                ))));
            }
            // Has __getitem__: use sequence protocol — return a lazy SeqIter
            if obj.get_attr("__getitem__").is_some() {
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::SeqIter {
                        obj: obj.clone(),
                        index: 0,
                        exhausted: false,
                    }),
                ))));
            }
            return Err(PyException::type_error(format!(
                "'{}' object is not iterable",
                obj.type_name()
            )));
        }
        builtins::get_iter_from_obj_pub(obj)
    }

    /// Resolve a slice of iterables, calling __iter__ on Instance objects.
    pub(crate) fn resolve_iterables(&mut self, args: &[PyObjectRef]) -> PyResult<Vec<PyObjectRef>> {
        args.iter().map(|a| self.resolve_iterable(a)).collect()
    }

    /// Collect all items from any iterable (list, tuple, generator, instance with __iter__/__next__).
    pub(crate) fn collect_iterable(&mut self, obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
        match &obj.payload {
            PyObjectPayload::List(cell) => Ok(cell.read().clone()),
            PyObjectPayload::Tuple(items) => Ok(items.to_vec()),
            PyObjectPayload::Generator(gen_arc) => {
                let gen_arc = gen_arc.clone();
                let mut items = Vec::new();
                loop {
                    match self.resume_generator(&gen_arc, PyObject::none()) {
                        Ok(value) => items.push(value),
                        Err(e) if e.kind == ExceptionKind::StopIteration => break,
                        Err(e) => return Err(e),
                    }
                }
                Ok(items)
            }
            PyObjectPayload::Instance(inst) => {
                if inst.attrs.read().contains_key("__chainmap__") {
                    if let Some(maps_obj) = obj.get_attr("maps") {
                        let maps = maps_obj.to_list()?;
                        let mut combined = IndexMap::new();
                        for mapping in maps.iter().rev() {
                            for key in mapping.to_list()? {
                                let hk = key.to_hashable_key()?;
                                combined.insert(hk, key);
                            }
                        }
                        return Ok(combined.keys().map(|k| k.to_object()).collect());
                    }
                }
                // Dict subclass: iterate over keys
                if let Some(ref ds) = inst.dict_storage {
                    return Ok(ds.read().keys().map(|k| k.to_object()).collect());
                }
                if let Some(iter_method) = obj.get_attr("__iter__") {
                    let iter_obj = self.call_object(iter_method, vec![])?;
                    // If __iter__ returned a list/tuple, convert directly
                    if matches!(
                        &iter_obj.payload,
                        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_)
                    ) {
                        return iter_obj.to_list();
                    }
                    // If __iter__ returned a builtin Iterator, use iter_advance
                    if matches!(
                        &iter_obj.payload,
                        PyObjectPayload::Iterator(_)
                            | PyObjectPayload::RangeIter(..)
                            | PyObjectPayload::VecIter(_)
                            | PyObjectPayload::WeakValueIter(_)
                            | PyObjectPayload::WeakKeyIter(_)
                            | PyObjectPayload::RefIter { .. }
                            | PyObjectPayload::RevRefIter { .. }
                    ) {
                        return self.collect_iterable(&iter_obj);
                    }
                    // If it returned a generator, collect from it
                    if let PyObjectPayload::Generator(gen_arc) = &iter_obj.payload {
                        let gen_arc = gen_arc.clone();
                        let mut items = Vec::new();
                        loop {
                            match self.resume_generator(&gen_arc, PyObject::none()) {
                                Ok(value) => items.push(value),
                                Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                Err(e) => return Err(e),
                            }
                        }
                        return Ok(items);
                    }
                    // Otherwise, it's an instance with __next__
                    let mut items = Vec::new();
                    loop {
                        if let Some(next_method) = iter_obj.get_attr("__next__") {
                            match self.call_object(next_method.clone(), vec![]) {
                                Ok(value) => items.push(value),
                                Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                Err(e) => return Err(e),
                            }
                        } else {
                            break;
                        }
                    }
                    Ok(items)
                } else if let Some(getitem) = obj.get_attr("__getitem__") {
                    // Fall back to __getitem__-based iteration (old-style sequence protocol)
                    let mut items = Vec::new();
                    let mut idx: i64 = 0;
                    loop {
                        match self.call_object(getitem.clone(), vec![PyObject::int(idx)]) {
                            Ok(val) => {
                                items.push(val);
                                idx += 1;
                            }
                            Err(e) if e.kind == ExceptionKind::IndexError => break,
                            Err(e) => return Err(e),
                        }
                    }
                    Ok(items)
                } else {
                    obj.to_list()
                }
            }
            PyObjectPayload::VecIter(data) => {
                let idx = data.index.get();
                if idx >= data.items.len() {
                    return Ok(vec![]);
                }
                let result = data.items[idx..].to_vec();
                data.index.set(usize::MAX);
                Ok(result)
            }
            PyObjectPayload::WeakValueIter(_) => {
                let mut items = Vec::new();
                while let Some(item) = self.vm_iter_next(obj)? {
                    items.push(item);
                }
                Ok(items)
            }
            PyObjectPayload::WeakKeyIter(_) => {
                let mut items = Vec::new();
                while let Some(item) = self.vm_iter_next(obj)? {
                    items.push(item);
                }
                Ok(items)
            }
            PyObjectPayload::RefIter { source, index } => {
                if index.get() == usize::MAX {
                    return Ok(vec![]);
                }
                let idx = index.get();
                match &source.payload {
                    PyObjectPayload::List(cell) => {
                        let items = unsafe { &*cell.data_ptr() };
                        if idx >= items.len() {
                            return Ok(vec![]);
                        }
                        let result = items[idx..].to_vec();
                        index.set(usize::MAX);
                        Ok(result)
                    }
                    PyObjectPayload::Tuple(items) => {
                        if idx >= items.len() {
                            return Ok(vec![]);
                        }
                        let result = items[idx..].to_vec();
                        index.set(usize::MAX);
                        Ok(result)
                    }
                    PyObjectPayload::Dict(cell)
                    | PyObjectPayload::MappingProxy(cell)
                    | PyObjectPayload::DictKeys { map: cell, .. } => {
                        let map = unsafe { &*cell.data_ptr() };
                        if idx >= map.len() {
                            return Ok(vec![]);
                        }
                        let result = map
                            .iter()
                            .skip(idx)
                            .map(|(key, _)| key.to_object())
                            .collect();
                        index.set(usize::MAX);
                        Ok(result)
                    }
                    _ => Ok(vec![]),
                }
            }
            PyObjectPayload::RevRefIter { source, index } => {
                let mut idx = index.get();
                if idx == usize::MAX || idx == 0 {
                    return Ok(vec![]);
                }
                match &source.payload {
                    PyObjectPayload::List(cell) => {
                        let items = unsafe { &*cell.data_ptr() };
                        if idx > items.len() {
                            index.set(usize::MAX);
                            return Ok(vec![]);
                        }
                        let mut result = Vec::with_capacity(idx);
                        while idx > 0 {
                            idx -= 1;
                            if idx < items.len() {
                                result.push(items[idx].clone());
                            }
                        }
                        index.set(usize::MAX);
                        Ok(result)
                    }
                    _ => Ok(vec![]),
                }
            }
            PyObjectPayload::Iterator(iter_data_arc) => {
                let map_one = {
                    let data = iter_data_arc.read();
                    if let IteratorData::MapOne { func, source } = &*data {
                        Some((func.clone(), source.clone()))
                    } else {
                        None
                    }
                };
                if let Some((func, source)) = map_one {
                    return self.collect_map_one_iterable(func, &source);
                }

                // Check for lazy iterators that need VM context
                let is_lazy = {
                    let data = iter_data_arc.read();
                    matches!(
                        &*data,
                        IteratorData::Enumerate { .. }
                            | IteratorData::Zip { .. }
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
                            | IteratorData::HeldIter { .. }
                    )
                };
                if is_lazy {
                    let mut items = Vec::new();
                    loop {
                        match self.advance_lazy_iterator(obj)? {
                            Some(value) => items.push(value),
                            None => break,
                        }
                    }
                    Ok(items)
                } else {
                    // Standard iterators — use iter_advance
                    let mut items = Vec::new();
                    loop {
                        match builtins::iter_advance(obj)? {
                            Some((_new_iter, value)) => items.push(value),
                            None => break,
                        }
                    }
                    Ok(items)
                }
            }
            PyObjectPayload::Class(_) => {
                // Class with __iter__ (e.g. Enum): call __iter__(cls)
                if let Some(iter_method) = obj.get_attr("__iter__") {
                    let result = self.call_object(iter_method, vec![obj.clone()])?;
                    return self.collect_iterable(&result);
                }
                Err(PyException::type_error(format!(
                    "'type' object is not iterable"
                )))
            }
            // Module with __iter__/__next__ (e.g. file objects created as module_with_attrs)
            PyObjectPayload::Module(_) => {
                // Module.get_attr returns raw NativeFunction (no BoundMethod wrapping),
                // so we must pass obj as self explicitly.
                if let Some(next_fn) = obj.get_attr("__next__") {
                    // Fast path: directly iterate via __next__ (file objects return self from __iter__)
                    let mut items = Vec::new();
                    loop {
                        match self.call_object(next_fn.clone(), vec![obj.clone()]) {
                            Ok(value) => items.push(value),
                            Err(e) if e.kind == ExceptionKind::StopIteration => break,
                            Err(e) => return Err(e),
                        }
                    }
                    return Ok(items);
                }
                if let Some(iter_fn) = obj.get_attr("__iter__") {
                    let iter_obj = self.call_object(iter_fn, vec![obj.clone()])?;
                    if !PyObjectRef::ptr_eq(&iter_obj, obj) {
                        return self.collect_iterable(&iter_obj);
                    }
                }
                Err(PyException::type_error(format!(
                    "'module' object is not iterable"
                )))
            }
            _ => obj.to_list(),
        }
    }

    fn collect_map_one_iterable(
        &mut self,
        func: PyObjectRef,
        source: &PyObjectRef,
    ) -> PyResult<Vec<PyObjectRef>> {
        match &source.payload {
            PyObjectPayload::Iterator(iter_data_arc) => {
                let mut data = iter_data_arc.write();
                match &mut *data {
                    IteratorData::List { items, index } | IteratorData::Tuple { items, index } => {
                        let start = *index;
                        let end = items.len();
                        let mut result = Vec::with_capacity(end.saturating_sub(start));
                        for item in &items[start..] {
                            result.push(self.call_object_one_arg_fast_or_fallback(
                                func.clone(),
                                item.clone(),
                            )?);
                        }
                        *index = end;
                        Ok(result)
                    }
                    IteratorData::DictKeys { keys, index } => {
                        let start = *index;
                        let end = keys.len();
                        let mut result = Vec::with_capacity(end.saturating_sub(start));
                        for item in &keys[start..] {
                            result.push(self.call_object_one_arg_fast_or_fallback(
                                func.clone(),
                                item.clone(),
                            )?);
                        }
                        *index = end;
                        Ok(result)
                    }
                    IteratorData::Range {
                        current,
                        stop,
                        step,
                    } => {
                        let len = if *step > 0 {
                            ((*stop - *current).max(0) + *step - 1) / *step
                        } else {
                            ((*current - *stop).max(0) + (-*step) - 1) / (-*step)
                        };
                        let mut result = Vec::with_capacity(len as usize);
                        while (*step > 0 && *current < *stop) || (*step < 0 && *current > *stop) {
                            let value = PyObject::int(*current);
                            *current += *step;
                            result.push(
                                self.call_object_one_arg_fast_or_fallback(func.clone(), value)?,
                            );
                        }
                        Ok(result)
                    }
                    _ => {
                        drop(data);
                        self.collect_map_one_iterable_slow(func, source)
                    }
                }
            }
            PyObjectPayload::RefIter {
                source: inner,
                index,
            } => {
                if index.get() == usize::MAX {
                    return Ok(vec![]);
                }
                let idx = index.get();
                match &inner.payload {
                    PyObjectPayload::List(cell) => {
                        let items = unsafe { &*cell.data_ptr() };
                        let mut result = Vec::with_capacity(items.len().saturating_sub(idx));
                        for item in &items[idx..] {
                            result.push(self.call_object_one_arg_fast_or_fallback(
                                func.clone(),
                                item.clone(),
                            )?);
                        }
                        index.set(usize::MAX);
                        Ok(result)
                    }
                    PyObjectPayload::Tuple(items) => {
                        let mut result = Vec::with_capacity(items.len().saturating_sub(idx));
                        for item in &items[idx..] {
                            result.push(self.call_object_one_arg_fast_or_fallback(
                                func.clone(),
                                item.clone(),
                            )?);
                        }
                        index.set(usize::MAX);
                        Ok(result)
                    }
                    _ => self.collect_map_one_iterable_slow(func, source),
                }
            }
            PyObjectPayload::VecIter(data) => {
                let idx = data.index.get();
                let mut result = Vec::with_capacity(data.items.len().saturating_sub(idx));
                for item in &data.items[idx..] {
                    result.push(
                        self.call_object_one_arg_fast_or_fallback(func.clone(), item.clone())?,
                    );
                }
                data.index.set(usize::MAX);
                Ok(result)
            }
            PyObjectPayload::RangeIter(ri) => {
                let mut current = ri.current.get();
                let len = if ri.step > 0 {
                    ((ri.stop - current).max(0) + ri.step - 1) / ri.step
                } else {
                    ((current - ri.stop).max(0) + (-ri.step) - 1) / (-ri.step)
                };
                let mut result = Vec::with_capacity(len as usize);
                while (ri.step > 0 && current < ri.stop) || (ri.step < 0 && current > ri.stop) {
                    result.push(self.call_object_one_arg_fast_or_fallback(
                        func.clone(),
                        PyObject::int(current),
                    )?);
                    current += ri.step;
                }
                ri.current.set(ri.stop);
                Ok(result)
            }
            _ => self.collect_map_one_iterable_slow(func, source),
        }
    }

    fn collect_map_one_iterable_slow(
        &mut self,
        func: PyObjectRef,
        source: &PyObjectRef,
    ) -> PyResult<Vec<PyObjectRef>> {
        let mut result = Vec::new();
        loop {
            match self.vm_iter_next(source)? {
                Some(value) => {
                    result.push(self.call_object_one_arg_fast_or_fallback(func.clone(), value)?);
                }
                None => break,
            }
        }
        Ok(result)
    }

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
                let current = ri.current.get();
                let done = if ri.step > 0 {
                    current >= ri.stop
                } else {
                    current <= ri.stop
                };
                if done {
                    Ok(None)
                } else {
                    ri.current.set(current + ri.step);
                    Ok(Some(PyObject::int(current)))
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
                // Check for lazy iterators first
                {
                    let data = iter_data_arc.read();
                    match &*data {
                        IteratorData::Enumerate { .. }
                        | IteratorData::Zip { .. }
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

    /// Collect any iterable into a Vec, using VM-level iteration for lazy iterators.
    /// Falls back to core `to_list()` for simple iterables.
    pub(crate) fn vm_collect_iterable(&mut self, obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
        // Try core to_list first (fast path for list, tuple, set, range, etc.)
        match obj.to_list() {
            Ok(items) => return Ok(items),
            Err(_) => {}
        }
        // Get an iterator and collect via VM
        let iter_obj = match &obj.payload {
            PyObjectPayload::Iterator(_)
            | PyObjectPayload::RangeIter(..)
            | PyObjectPayload::VecIter(_)
            | PyObjectPayload::WeakValueIter(_)
            | PyObjectPayload::WeakKeyIter(_)
            | PyObjectPayload::RefIter { .. }
            | PyObjectPayload::RevRefIter { .. }
            | PyObjectPayload::Generator(_) => obj.clone(),
            PyObjectPayload::Instance(_) => {
                if let Some(iter_fn) = obj.get_attr("__iter__") {
                    let result = self.call_object(iter_fn, vec![])?;
                    // If __iter__ returns a directly iterable type (tuple, list),
                    // collect it immediately instead of treating as an iterator.
                    if let Ok(items) = result.to_list() {
                        return Ok(items);
                    }
                    result
                } else {
                    return Err(PyException::type_error(format!(
                        "cannot unpack non-iterable {} object",
                        obj.type_name()
                    )));
                }
            }
            _ => {
                return Err(PyException::type_error(format!(
                    "cannot unpack non-iterable {} object",
                    obj.type_name()
                )));
            }
        };
        let mut items = Vec::new();
        loop {
            match self.vm_iter_next(&iter_obj)? {
                Some(val) => items.push(val),
                None => break,
            }
        }
        Ok(items)
    }
}
