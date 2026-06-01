use crate::builtins;
use crate::builtins::advance_deque_iter;
use crate::VirtualMachine;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::{dict_storage_version, range_len, range_next_i64};
use ferrython_core::object::{
    IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;
use indexmap::IndexMap;
use std::rc::Rc;

fn islice_index_arg(obj: &PyObjectRef, default: usize, allow_none: bool) -> PyResult<usize> {
    if allow_none && matches!(&obj.payload, PyObjectPayload::None) {
        return Ok(default);
    }
    match obj.to_index() {
        Ok(PyInt::Small(n)) if n >= 0 => Ok(n as usize),
        Ok(PyInt::Small(_)) | Ok(PyInt::Big(_)) | Err(_) => Err(PyException::value_error(
            "Indices for islice() must be None or an integer: 0 <= x <= sys.maxsize.",
        )),
    }
}

impl VirtualMachine {
    /// VM-level itertools.islice: lazily takes items from any iterable (including generators).
    pub(crate) fn vm_itertools_islice(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "islice() requires at least 2 arguments",
            ));
        }
        if args.len() > 4 {
            return Err(PyException::type_error(
                "islice() takes at most 4 arguments",
            ));
        }
        let iterable = &args[0];
        // None stop means no limit (use usize::MAX as sentinel)
        let (start, stop, step) = if args.len() == 2 {
            let stop = islice_index_arg(&args[1], usize::MAX, true)?;
            (0usize, stop, 1usize)
        } else if args.len() == 3 {
            let s = islice_index_arg(&args[1], 0, true)?;
            let stop = islice_index_arg(&args[2], usize::MAX, true)?;
            (s, stop, 1usize)
        } else {
            let s = islice_index_arg(&args[1], 0, true)?;
            let stop = islice_index_arg(&args[2], usize::MAX, true)?;
            let st = islice_index_arg(&args[3], 1, true)?;
            if st == 0 {
                return Err(PyException::value_error(
                    "Step for islice() must be a positive integer or None.",
                ));
            }
            (s, stop, st)
        };

        let source = if let PyObjectPayload::Instance(_) = &iterable.payload {
            if let Some(iter_method) = iterable.get_attr("__iter__") {
                self.call_object(iter_method, vec![])?
            } else {
                iterable.get_iter()?
            }
        } else {
            iterable.get_iter()?
        };
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::Islice {
                source,
                index: 0,
                next_yield: start,
                stop,
                step,
            }),
        ))))
    }

    /// Resolve an iterable object to its iterator by calling __iter__ if needed.
    /// For Instance objects with __iter__, calls __iter__() to get the real iterator.
    /// For builtin types (list, tuple, etc.), delegates to get_iter_from_obj.
    pub(crate) fn resolve_iterable(&mut self, obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            // dict subclass or namedtuple: use core get_iter
            if inst.dict_storage.is_some()
                || inst.class.get_attr("__namedtuple__").is_some()
                || inst.attrs.read().contains_key("__deque__")
            {
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
                if obj.get_attr("__next__").is_some() && obj.get_attr("__iter__").is_none() {
                    return Err(PyException::type_error(format!(
                        "'{}' object is not iterable",
                        obj.type_name()
                    )));
                }
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
                            | PyObjectPayload::DictValueIter(_)
                            | PyObjectPayload::WeakValueIter(_)
                            | PyObjectPayload::WeakKeyIter(_)
                            | PyObjectPayload::DequeIter(_)
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
                    if iter_obj.get_attr("__next__").is_none() {
                        return Err(PyException::type_error(format!(
                            "iter() returned non-iterator of type '{}'",
                            iter_obj.type_name()
                        )));
                    }
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
            PyObjectPayload::DictValueIter(_) => {
                let mut items = Vec::new();
                while let Some(item) = self.vm_iter_next(obj)? {
                    items.push(item);
                }
                Ok(items)
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
            PyObjectPayload::DequeIter(data) => {
                let mut items = Vec::new();
                while let Some(item) = advance_deque_iter(data)? {
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
            PyObjectPayload::RevRefIter { source, index, .. } => {
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
                    IteratorData::DictKeyRefs {
                        source,
                        index,
                        expected_len,
                        expected_version,
                    } => {
                        let map = source.read();
                        if map.len() != *expected_len
                            || dict_storage_version(source) != *expected_version
                        {
                            return Err(PyException::runtime_error(
                                "dictionary changed size during iteration",
                            ));
                        }
                        let start = *index;
                        let end = map.len();
                        let mut result = Vec::with_capacity(end.saturating_sub(start));
                        for (key, _) in map.iter().skip(start) {
                            result.push(self.call_object_one_arg_fast_or_fallback(
                                func.clone(),
                                key.to_object(),
                            )?);
                        }
                        *index = end;
                        Ok(result)
                    }
                    IteratorData::SetRefs {
                        source,
                        index,
                        expected_len,
                    } => {
                        let map = source.read();
                        if map.len() != *expected_len {
                            return Err(PyException::runtime_error(
                                "Set changed size during iteration",
                            ));
                        }
                        let start = *index;
                        let end = map.len();
                        let mut result = Vec::with_capacity(end.saturating_sub(start));
                        for (_, value) in map.iter().skip(start) {
                            result.push(self.call_object_one_arg_fast_or_fallback(
                                func.clone(),
                                value.clone(),
                            )?);
                        }
                        *index = end;
                        Ok(result)
                    }
                    IteratorData::FrozenSetItems { items, index } => {
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
                    IteratorData::Range {
                        current,
                        stop,
                        step,
                    } => {
                        let len = range_len(*current, *stop, *step);
                        let mut result = Vec::with_capacity(len as usize);
                        while let Some((value, next)) = range_next_i64(*current, *stop, *step) {
                            let value = PyObject::int(value);
                            *current = next;
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
            PyObjectPayload::DictValueIter(_) => self.collect_map_one_iterable_slow(func, source),
            PyObjectPayload::RangeIter(ri) => {
                let mut current = ri.current.get();
                let len = range_len(current, ri.stop, ri.step);
                let mut result = Vec::with_capacity(len as usize);
                while let Some((value, next)) = range_next_i64(current, ri.stop, ri.step) {
                    result.push(self.call_object_one_arg_fast_or_fallback(
                        func.clone(),
                        PyObject::int(value),
                    )?);
                    current = next;
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
            | PyObjectPayload::DictValueIter(_)
            | PyObjectPayload::WeakValueIter(_)
            | PyObjectPayload::WeakKeyIter(_)
            | PyObjectPayload::DequeIter(_)
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
