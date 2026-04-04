//! VM utility functions — repr, str, sort, iteration, generators.

use crate::builtins;
use crate::frame::Frame;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::code::ConstantValue;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    AsyncGenAction, GeneratorState, IteratorData, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, SharedGlobals};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

impl VirtualMachine {
    /// Install thread-local __hash__ and __eq__ dispatch callbacks for HashableKey.
    /// Called once at VM creation so all set/dict operations can resolve custom hashing.
    pub(crate) fn install_hash_eq_dispatch(&mut self) {
        let vm_ptr = self as *mut VirtualMachine;
        ferrython_core::types::set_eq_dispatch(move |a: &PyObjectRef, b: &PyObjectRef| {
            let vm = unsafe { &mut *vm_ptr };
            if let Some(eq_method) = a.get_attr("__eq__") {
                if let Ok(result) = vm.call_object(eq_method, vec![b.clone()]) {
                    return Some(result.is_truthy());
                }
            }
            None
        });

        let vm_ptr2 = self as *mut VirtualMachine;
        ferrython_core::types::set_hash_dispatch(move |obj: &PyObjectRef| {
            let vm = unsafe { &mut *vm_ptr2 };
            if let Some(hash_method) = obj.get_attr("__hash__") {
                if let Ok(result) = vm.call_object(hash_method, vec![]) {
                    return Some(result.as_int().unwrap_or(0));
                }
            }
            None
        });
    }

    pub(crate) fn is_exception_class(cls: &PyObjectRef) -> bool {
        if matches!(&cls.payload, PyObjectPayload::ExceptionType(_)) {
            return true;
        }
        if let PyObjectPayload::Class(cd) = &cls.payload {
            // Check if any base is an ExceptionType or an exception class
            for base in &cd.bases {
                if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                    return true;
                }
                if Self::is_exception_class(base) {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn vm_str(&mut self, obj: &PyObjectRef) -> PyResult<String> {
        match &obj.payload {
            PyObjectPayload::Instance(_) => {
                if let Some(str_method) = obj.get_attr("__str__") {
                    let result = self.call_object(str_method, vec![])?;
                    return Ok(result.py_to_string());
                }
                // Fall back to __repr__ if no __str__
                if let Some(repr_method) = obj.get_attr("__repr__") {
                    let result = self.call_object(repr_method, vec![])?;
                    return Ok(result.py_to_string());
                }
                // Exception instances: str(e) returns the message from args
                if let Some(args) = obj.get_attr("args") {
                    if let PyObjectPayload::Tuple(items) = &args.payload {
                        return match items.len() {
                            0 => Ok(String::new()),
                            1 => Ok(items[0].py_to_string()),
                            _ => self.vm_repr(&args),
                        };
                    }
                }
                // Fall back to vm_repr (handles namedtuple, dataclass, etc.)
                // Decimal instances: str() returns the value
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if inst.attrs.read().contains_key("__decimal__") {
                        return Ok(inst.attrs.read().get("_value").map(|v| v.py_to_string()).unwrap_or_else(|| "0".to_string()));
                    }
                }
                self.vm_repr(obj)
            }
            // For containers, str() is same as repr() (elements use repr)
            PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) |
            PyObjectPayload::Dict(_) | PyObjectPayload::Set(_) |
            PyObjectPayload::FrozenSet(_) => self.vm_repr(obj),
            _ => Ok(obj.py_to_string()),
        }
    }

    /// Produce a repr string for an object, dispatching __repr__ on instances.
    pub(crate) fn vm_repr(&mut self, obj: &PyObjectRef) -> PyResult<String> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                if let Some(repr_method) = obj.get_attr("__repr__") {
                    let result = self.call_object(repr_method, vec![])?;
                    return Ok(result.py_to_string());
                }
                // Dataclass auto-repr
                let class = &inst.class;
                if matches!(&class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__dataclass__")) {
                    if let Some(fields) = class.get_attr("__dataclass_fields__") {
                        if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                            let class_name = if let PyObjectPayload::Class(cd) = &class.payload {
                                cd.name.to_string()
                            } else { "?".to_string() };
                            let mut parts = Vec::new();
                            let attrs = inst.attrs.read();
                            for ft in field_tuples {
                                if let PyObjectPayload::Tuple(info) = &ft.payload {
                                    let name = info[0].py_to_string();
                                    if let Some(val) = attrs.get(name.as_str()) {
                                        let val_repr = self.vm_repr(val)?;
                                        parts.push(format!("{}={}", name, val_repr));
                                    }
                                }
                            }
                            return Ok(format!("{}({})", class_name, parts.join(", ")));
                        }
                    }
                }
                // Namedtuple auto-repr
                if matches!(&class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__namedtuple__")) {
                    if let Some(fields) = class.get_attr("_fields") {
                        if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                            let class_name = if let PyObjectPayload::Class(cd) = &class.payload {
                                cd.name.to_string()
                            } else { "?".to_string() };
                            let mut parts = Vec::new();
                            let attrs = inst.attrs.read();
                            for field in field_names {
                                let name = field.py_to_string();
                                if let Some(val) = attrs.get(name.as_str()) {
                                    let val_repr = self.vm_repr(val)?;
                                    parts.push(format!("{}={}", name, val_repr));
                                }
                            }
                            return Ok(format!("{}({})", class_name, parts.join(", ")));
                        }
                    }
                }
                Ok(obj.repr())
            }
            PyObjectPayload::List(items) => {
                let items = items.read().clone();
                let mut parts = Vec::new();
                for item in &items {
                    parts.push(self.vm_repr(item)?);
                }
                Ok(format!("[{}]", parts.join(", ")))
            }
            PyObjectPayload::Tuple(items) => {
                let mut parts = Vec::new();
                for item in items {
                    parts.push(self.vm_repr(item)?);
                }
                if parts.len() == 1 {
                    Ok(format!("({},)", parts[0]))
                } else {
                    Ok(format!("({})", parts.join(", ")))
                }
            }
            PyObjectPayload::Dict(m) => {
                let m = m.read().clone();
                let mut parts = Vec::new();
                for (k, v) in &m {
                    // Hide defaultdict internal factory key
                    if let HashableKey::Str(s) = k {
                        if s.as_str() == "__defaultdict_factory__" { continue; }
                    }
                    let kr = self.vm_repr(&k.to_object())?;
                    let vr = self.vm_repr(v)?;
                    parts.push(format!("{}: {}", kr, vr));
                }
                Ok(format!("{{{}}}", parts.join(", ")))
            }
            PyObjectPayload::Set(m) => {
                let m = m.read().clone();
                if m.is_empty() { return Ok("set()".to_string()); }
                let mut parts = Vec::new();
                for v in m.values() {
                    parts.push(self.vm_repr(v)?);
                }
                Ok(format!("{{{}}}", parts.join(", ")))
            }
            _ => Ok(obj.repr()),
        }
    }

    /// Convert a Python object to a HashableKey, calling __hash__/__eq__ on instances.
    /// Dispatches are installed at VM init, so from_object will use them automatically.
    pub(crate) fn vm_to_hashable_key(&mut self, obj: &PyObjectRef) -> PyResult<HashableKey> {
        obj.to_hashable_key()
    }

    /// Call a Python object (function, builtin, class).
    pub(crate) fn vm_functools_reduce(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("reduce() requires at least 2 arguments")); }
        let func = args[0].clone();
        let items = self.collect_iterable(&args[1])?;
        let has_initial = args.len() > 2;
        let mut acc = if has_initial {
            args[2].clone()
        } else if !items.is_empty() {
            items[0].clone()
        } else {
            return Err(PyException::type_error("reduce() of empty sequence with no initial value"));
        };
        let start_idx = if has_initial { 0 } else { 1 };
        for item in &items[start_idx..] {
            acc = self.call_object(func.clone(), vec![acc, item.clone()])?;
        }
        Ok(acc)
    }

    /// VM-level itertools.islice: lazily takes items from any iterable (including generators).
    pub(crate) fn vm_itertools_islice(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("islice() requires at least 2 arguments"));
        }
        let iterable = &args[0];
        let (start, stop, step) = if args.len() == 2 {
            (0usize, args[1].to_int()? as usize, 1usize)
        } else if args.len() == 3 {
            let s = if matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { args[1].to_int()? as usize };
            (s, args[2].to_int()? as usize, 1usize)
        } else {
            let s = if matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { args[1].to_int()? as usize };
            let st = if matches!(&args[3].payload, PyObjectPayload::None) { 1 } else { args[3].to_int()? as usize };
            (s, args[2].to_int()? as usize, st.max(1))
        };

        // For generators: consume items one at a time, only up to `stop`
        if let PyObjectPayload::Generator(gen_arc) = &iterable.payload {
            let gen_arc = gen_arc.clone();
            let mut result = Vec::new();
            let mut idx = 0usize;
            let mut next_yield = start;
            loop {
                if result.len() >= stop - start { break; }
                if idx >= stop { break; }
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
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(std::sync::Mutex::new(IteratorData::List { items: result, index: 0 }))
            )));
        }

        // For iterators with lazy data: advance one at a time
        if let PyObjectPayload::Iterator(_) = &iterable.payload {
            let mut result = Vec::new();
            let mut idx = 0usize;
            let mut next_yield = start;
            loop {
                if idx >= stop { break; }
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
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(std::sync::Mutex::new(IteratorData::List { items: result, index: 0 }))
            )));
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
        let result: Vec<PyObjectRef> = items.into_iter()
            .skip(start)
            .take(stop - start)
            .step_by(step)
            .collect();
        Ok(PyObject::wrap(PyObjectPayload::Iterator(
            Arc::new(std::sync::Mutex::new(IteratorData::List { items: result, index: 0 }))
        )))
    }

    /// Collect all items from any iterable (list, tuple, generator, instance with __iter__/__next__).
    pub(crate) fn collect_iterable(&mut self, obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
        match &obj.payload {
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
                // Dict subclass: iterate over keys
                if let Some(ref ds) = inst.dict_storage {
                    return Ok(ds.read().keys().map(|k| k.to_object()).collect());
                }
                // Deque: directly return internal data as list
                if inst.attrs.read().contains_key("__deque__") {
                    if let Some(data) = inst.attrs.read().get("_data").cloned() {
                        return data.to_list();
                    }
                }
                if let Some(iter_method) = obj.get_attr("__iter__") {
                    let iter_obj = self.call_object(iter_method, vec![])?;
                    // If __iter__ returned a builtin Iterator, use iter_advance
                    if matches!(&iter_obj.payload, PyObjectPayload::Iterator(_)) {
                        let mut items = Vec::new();
                        loop {
                            match builtins::iter_advance(&iter_obj)? {
                                Some((_new_iter, value)) => items.push(value),
                                None => break,
                            }
                        }
                        return Ok(items);
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
                        } else { break; }
                    }
                    Ok(items)
                } else {
                    obj.to_list()
                }
            }
            PyObjectPayload::Iterator(iter_data_arc) => {
                // Check for lazy iterators that need VM context
                let is_lazy = {
                    let data = iter_data_arc.lock().unwrap();
                    matches!(&*data, IteratorData::Enumerate { .. }
                        | IteratorData::Zip { .. }
                        | IteratorData::Map { .. }
                        | IteratorData::Filter { .. }
                        | IteratorData::Sentinel { .. }
                        | IteratorData::TakeWhile { .. }
                        | IteratorData::DropWhile { .. })
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
            _ => obj.to_list(),
        }
    }

    /// Resume a generator, pushing the given `send_value` onto its stack and running
    /// until the next `YieldValue` or `ReturnValue`.
    /// Returns `Ok(value)` for yielded values, or `Err(StopIteration)` when done.
    pub(crate) fn resume_generator(
        &mut self,
        gen_arc: &Arc<RwLock<GeneratorState>>,
        send_value: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let mut gen = gen_arc.write();
        if gen.finished {
            return Err(PyException::new(ExceptionKind::StopIteration, ""));
        }
        let mut frame = match gen.frame.take() {
            Some(f) => *f.downcast::<Frame>().expect("generator frame downcast"),
            None => return Err(PyException::runtime_error("generator already executing")),
        };

        // If generator was already started, push the send value onto the frame's stack
        // (it becomes the result of the `yield` expression)
        if gen.started {
            frame.push(send_value);
        }
        gen.started = true;
        drop(gen); // release lock before executing

        self.call_stack.push(frame);
        let result = self.run_frame();
        let frame = self.call_stack.pop().unwrap();

        let mut gen = gen_arc.write();
        if frame.yielded {
            // Generator yielded — save frame for later resumption
            let mut saved_frame = frame;
            saved_frame.yielded = false;
            gen.frame = Some(Box::new(saved_frame));
            result // Ok(yielded_value)
        } else {
            // Generator finished (returned or raised)
            gen.finished = true;
            gen.frame = None;
            match result {
                Ok(return_val) => {
                    // Normal return → StopIteration with return value
                    let msg = return_val.py_to_string();
                    let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                    exc.value = Some(return_val);
                    Err(exc)
                }
                Err(e) => Err(e), // Propagate the actual exception
            }
        }
    }

    /// Throw an exception into a generator.
    /// Resumes the generator with an exception injected at the yield point.
    pub(crate) fn gen_throw(
        &mut self,
        gen_arc: &Arc<RwLock<GeneratorState>>,
        kind: ExceptionKind,
        msg: String,
    ) -> PyResult<PyObjectRef> {
        let mut gen = gen_arc.write();
        if gen.finished {
            return Err(PyException::new(kind, msg));
        }
        let mut frame = match gen.frame.take() {
            Some(f) => *f.downcast::<Frame>().expect("generator frame downcast"),
            None => return Err(PyException::runtime_error("generator already executing")),
        };
        gen.started = true;
        drop(gen);

        // Set up exception on the frame so VM will unwind to handler
        let exc = PyException::new(kind.clone(), msg.clone());
        self.call_stack.push(frame);
        let exc_result = Err(exc);
        let exc_obj = PyObject::exception_instance(kind.clone(), msg.clone());
        let exc_type = PyObject::exception_type(kind.clone());
        let tb = PyObject::none();

        // Try to find an exception handler in the generator's frame
        if let Some(handler_ip) = self.unwind_except() {
            self.active_exception = Some(PyException::new(kind, msg));
            let frame_ref = self.call_stack.last_mut().unwrap();
            frame_ref.push(tb);
            frame_ref.push(exc_obj);
            frame_ref.push(exc_type);
            frame_ref.ip = handler_ip;

            let result = self.run_frame();
            let frame = self.call_stack.pop().unwrap();

            let mut gen = gen_arc.write();
            if frame.yielded {
                let mut saved_frame = frame;
                saved_frame.yielded = false;
                gen.frame = Some(Box::new(saved_frame));
                result
            } else {
                gen.finished = true;
                gen.frame = None;
                let return_val = result.ok();
                let msg = return_val.as_ref().map(|v| v.py_to_string()).unwrap_or_default();
                let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                exc.value = return_val;
                Err(exc)
            }
        } else {
            // No handler — pop frame and re-raise
            self.call_stack.pop();
            let mut gen = gen_arc.write();
            gen.finished = true;
            gen.frame = None;
            exc_result
        }
    }

    /// Parse the arguments to generator.throw() / coroutine.throw() into (ExceptionKind, message).
    pub(crate) fn parse_throw_args(args: &[PyObjectRef]) -> (ExceptionKind, String) {
        let msg = if args.len() >= 2 { args[1].py_to_string() } else { String::new() };
        let kind = if !args.is_empty() {
            match &args[0].payload {
                PyObjectPayload::ExceptionType(k) => k.clone(),
                PyObjectPayload::BuiltinType(name) => {
                    ExceptionKind::from_name(name).unwrap_or(ExceptionKind::RuntimeError)
                }
                PyObjectPayload::ExceptionInstance { kind, .. } => kind.clone(),
                _ => ExceptionKind::RuntimeError,
            }
        } else {
            ExceptionKind::RuntimeError
        };
        (kind, msg)
    }

    /// Drive an AsyncGenAwaitable: execute the action on the underlying async generator.
    ///
    /// This implements the behavior of CPython's `async_generator_anext` / `async_generator_asend`
    /// / `async_generator_athrow` objects. When `send(None)` is called:
    ///   - Next/Send: resumes the async generator. Yielded value → StopIteration(value).
    ///                On exhaustion → StopAsyncIteration.
    ///   - Throw:     throws exception into generator frame.
    ///   - Close:     throws GeneratorExit; expects generator to finish.
    pub(crate) fn drive_async_gen_awaitable(
        &mut self,
        gen: &Arc<RwLock<GeneratorState>>,
        action: &AsyncGenAction,
        send_val: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        match action {
            AsyncGenAction::Next => {
                // Resume with send_val (for first call it's None, for subsequent send() it's the arg)
                match self.resume_generator(gen, send_val) {
                    Ok(yielded) => {
                        // Async generator yielded a value — propagate via StopIteration
                        let msg = yielded.py_to_string();
                        let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                        exc.value = Some(yielded);
                        Err(exc)
                    }
                    Err(e) if e.kind == ExceptionKind::StopIteration => {
                        // Async generator returned (exhausted) — raise StopAsyncIteration
                        Err(PyException::new(ExceptionKind::StopAsyncIteration, String::new()))
                    }
                    Err(e) => Err(e),
                }
            }
            AsyncGenAction::Send(val) => {
                // Like Next but with explicit value (ignore send_val from protocol, use stored val)
                match self.resume_generator(gen, val.clone()) {
                    Ok(yielded) => {
                        let msg = yielded.py_to_string();
                        let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                        exc.value = Some(yielded);
                        Err(exc)
                    }
                    Err(e) if e.kind == ExceptionKind::StopIteration => {
                        Err(PyException::new(ExceptionKind::StopAsyncIteration, String::new()))
                    }
                    Err(e) => Err(e),
                }
            }
            AsyncGenAction::Throw(exc_kind, msg) => {
                self.gen_throw(gen, exc_kind.clone(), msg.to_string())
            }
            AsyncGenAction::Close => {
                // Like generator.close(): throw GeneratorExit, expect finish
                let g = gen.read();
                if g.finished || g.frame.is_none() {
                    drop(g);
                    return Ok(PyObject::none());
                }
                drop(g);
                match self.gen_throw(gen, ExceptionKind::GeneratorExit, String::new()) {
                    Ok(_yielded) => {
                        Err(PyException::runtime_error("async generator ignored GeneratorExit"))
                    }
                    Err(e) if e.kind == ExceptionKind::GeneratorExit
                           || e.kind == ExceptionKind::StopIteration
                           || e.kind == ExceptionKind::StopAsyncIteration => {
                        let mut g = gen.write();
                        g.finished = true;
                        g.frame = None;
                        Ok(PyObject::none())
                    }
                    Err(e) => {
                        let mut g = gen.write();
                        g.finished = true;
                        g.frame = None;
                        Err(e)
                    }
                }
            }
        }
    }

    /// If a value is a Coroutine, drive it to completion and return the final value.
    /// This is used for async-with cleanup where `__aexit__` may return a coroutine.
    /// For non-coroutine values, returns the value unchanged.
    pub(crate) fn maybe_await_result(&mut self, result: PyObjectRef) -> PyResult<PyObjectRef> {
        match &result.payload {
            PyObjectPayload::Coroutine(gen_arc) => {
                // Drive the coroutine to completion: send(None) until StopIteration
                let gen_arc = gen_arc.clone();
                let mut send_val = PyObject::none();
                loop {
                    match self.resume_generator(&gen_arc, send_val) {
                        Ok(yielded) => {
                            // Coroutine yielded — send None to continue
                            send_val = PyObject::none();
                            let _ = yielded; // discard intermediate yields
                        }
                        Err(e) if e.kind == ExceptionKind::StopIteration => {
                            return Ok(e.value.unwrap_or_else(|| PyObject::none()));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            _ => Ok(result),
        }
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
            PyObjectPayload::Iterator(iter_data_arc) => {
                // Check for lazy iterators first
                {
                    let data = iter_data_arc.lock().unwrap();
                    match &*data {
                        IteratorData::Enumerate { .. }
                        | IteratorData::Zip { .. }
                        | IteratorData::Map { .. }
                        | IteratorData::Filter { .. }
                        | IteratorData::Sentinel { .. }
                        | IteratorData::TakeWhile { .. }
                        | IteratorData::DropWhile { .. } => {
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
            _ => Err(PyException::type_error(format!(
                "'{}' object is not an iterator", iter_obj.type_name()
            ))),
        }
    }

    /// Advance lazy iterator variants (Enumerate, Zip, Map, Filter).
    pub(crate) fn advance_lazy_iterator(&mut self, iter_obj: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
        let iter_data_arc = match &iter_obj.payload {
            PyObjectPayload::Iterator(arc) => arc.clone(),
            _ => return Err(PyException::type_error("not an iterator")),
        };
        let mut data = iter_data_arc.lock().unwrap();
        match &mut *data {
            IteratorData::Enumerate { source, index } => {
                let src = source.clone();
                let idx = *index;
                *index += 1;
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(val) => Ok(Some(PyObject::tuple(vec![PyObject::int(idx), val]))),
                    None => Ok(None),
                }
            }
            IteratorData::Zip { sources } => {
                let srcs: Vec<PyObjectRef> = sources.clone();
                drop(data);
                let mut items = Vec::with_capacity(srcs.len());
                for src in &srcs {
                    match self.vm_iter_next(src)? {
                        Some(val) => items.push(val),
                        None => return Ok(None),
                    }
                }
                Ok(Some(PyObject::tuple(items)))
            }
            IteratorData::Map { func, source } => {
                let f = func.clone();
                let src = source.clone();
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(val) => {
                        let result = self.call_object(f, vec![val])?;
                        Ok(Some(result))
                    }
                    None => Ok(None),
                }
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
            IteratorData::Sentinel { callable, sentinel } => {
                let f = callable.clone();
                let s = sentinel.clone();
                drop(data);
                let val = self.call_object(f, vec![])?;
                let eq_result = val.compare(&s, ferrython_core::object::CompareOp::Eq)?;
                if eq_result.is_truthy() {
                    Ok(None)
                } else {
                    Ok(Some(val))
                }
            }
            IteratorData::TakeWhile { func, source, done } => {
                if *done { drop(data); return Ok(None); }
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
                                if let IteratorData::TakeWhile { done, .. } = &mut *arc.lock().unwrap() {
                                    *done = true;
                                }
                            }
                            Ok(None)
                        }
                    }
                    None => Ok(None),
                }
            }
            IteratorData::DropWhile { func, source, dropping } => {
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
                                        if let IteratorData::DropWhile { dropping, .. } = &mut *arc.lock().unwrap() {
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
            _ => {
                drop(data);
                match builtins::iter_advance(iter_obj)? {
                    Some((_new, val)) => Ok(Some(val)),
                    None => Ok(None),
                }
            }
        }
    }

    /// Sort items using VM-level comparison (supports custom __lt__).
    /// Uses insertion sort to allow &mut self access during comparisons.
    pub fn vm_sort(&mut self, items: &mut Vec<PyObjectRef>) -> PyResult<()> {
        let n = items.len();
        if n <= 1 { return Ok(()); }
        let has_instances = items.iter().any(|x| matches!(&x.payload, PyObjectPayload::Instance(_)));
        if !has_instances {
            items.sort_by(|a, b| {
                builtins::partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
            });
            return Ok(());
        }
        // Insertion sort with VM-level __lt__ calls
        for i in 1..n {
            let mut j = i;
            while j > 0 {
                let is_less = self.vm_lt(&items[j], &items[j - 1])?;
                if is_less {
                    items.swap(j, j - 1);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Compare two objects using __lt__, falling back to native comparison.
    pub(crate) fn vm_lt(&mut self, a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
        if let PyObjectPayload::Instance(_) = &a.payload {
            if let Some(method) = a.get_attr("__lt__") {
                let result = self.call_object(method, vec![b.clone()])?;
                return Ok(result.is_truthy());
            }
        }
        Ok(builtins::partial_cmp_for_sort(a, b) == Some(std::cmp::Ordering::Less))
    }

    // ── Post-call intercept for VM-aware builtins ────────────────────────

    /// After every function call, check for deferred VM-aware operations.
    /// This handles builtins that need VM access but are called through the
    /// generic NativeFunction path (which doesn't pass &mut self).
    pub(crate) fn post_call_intercept(&mut self, mut result: PyObjectRef) -> PyResult<PyObjectRef> {
        // asyncio.run() intercept: drive coroutine to completion
        if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
            result = self.maybe_await_result(coro)?;
        }
        // __import__() intercept: resolve and load module
        if let Some(req) = crate::builtins::take_import_request() {
            result = self.execute_import(&req.name, req.level)?;
        }
        Ok(result)
    }

    /// Execute an import by name and level, used by __import__ intercept.
    fn execute_import(&mut self, name: &str, level: usize) -> PyResult<PyObjectRef> {
        self.ensure_sys_modules();

        // Check cache first
        let top_name = name.split('.').next().unwrap_or(name);
        if let Some(module) = self.modules.get(top_name) {
            return Ok(module.clone());
        }

        // Try stdlib
        if let Some(module) = ferrython_stdlib::load_module(top_name) {
            self.cache_module(top_name, &module);
            return Ok(module);
        }

        // Determine importer filename from current frame
        let importer_file = self.call_stack.last()
            .map(|f| f.code.filename.as_str().to_string())
            .unwrap_or_default();

        // Try filesystem import
        let resolved = if level > 0 {
            ferrython_import::resolve_relative_import(name, &importer_file, level)?
        } else {
            ferrython_import::resolve_module(name, &importer_file)?
        };

        match resolved {
            ferrython_import::ResolvedModule::Builtin(m) => {
                self.cache_module(name, &m);
                Ok(m)
            }
            ferrython_import::ResolvedModule::Source { code, name: mod_name, file_path } => {
                self.exec_module_from_source(name, code, mod_name, file_path)
            }
        }
    }

    /// Create a module from compiled source, execute it, and cache it.
    fn exec_module_from_source(
        &mut self,
        cache_name: &str,
        code: ferrython_bytecode::CodeObject,
        mod_name: CompactString,
        file_path: Option<CompactString>,
    ) -> PyResult<PyObjectRef> {
        use crate::frame::Frame;

        // Create shared globals for the module
        let globals: SharedGlobals = Arc::new(RwLock::new(IndexMap::new()));
        {
            let mut g = globals.write();
            g.insert(CompactString::from("__name__"), PyObject::str_val(mod_name.clone()));
            if let Some(ref fp) = file_path {
                g.insert(CompactString::from("__file__"), PyObject::str_val(fp.clone()));
            }
            let pkg = if let Some(pos) = cache_name.rfind('.') {
                &cache_name[..pos]
            } else {
                ""
            };
            g.insert(CompactString::from("__package__"), PyObject::str_val(CompactString::from(pkg)));
        }

        // Pre-cache a partial module (circular import protection)
        let partial_mod = PyObject::module_with_attrs(mod_name.clone(), globals.read().clone());
        self.cache_module(cache_name, &partial_mod);

        // Execute the module body
        let frame = Frame::new(code, globals.clone(), self.builtins.clone());
        self.call_stack.push(frame);
        let exec_result = self.run_frame();
        self.call_stack.pop();

        // Build the final module from the globals after execution
        let final_mod = PyObject::module_with_attrs(mod_name, globals.read().clone());
        self.cache_module(cache_name, &final_mod);

        exec_result?;
        Ok(final_mod)
    }

    // ── Module cache management ─────────────────────────────────────────

    /// Cache a module in both VM.modules and sys.modules dict.
    pub(crate) fn cache_module(&mut self, name: &str, module: &PyObjectRef) {
        self.modules.insert(CompactString::from(name), module.clone());
        // Sync with sys.modules dict if available
        if let Some(ref sys_mod_dict) = self.sys_modules_dict {
            if let PyObjectPayload::Dict(ref d) = sys_mod_dict.payload {
                d.write().insert(
                    HashableKey::Str(CompactString::from(name)),
                    module.clone(),
                );
            }
        }
    }

    /// Initialize sys.modules reference from the sys module.
    /// Called lazily on first import to avoid circular initialization.
    pub(crate) fn ensure_sys_modules(&mut self) {
        if self.sys_modules_dict.is_some() { return; }
        // Look up the sys module from our cache or create it
        let sys_mod = if let Some(m) = self.modules.get("sys") {
            m.clone()
        } else if let Some(m) = ferrython_stdlib::load_module("sys") {
            self.modules.insert(CompactString::from("sys"), m.clone());
            m
        } else {
            return;
        };
        // Extract sys.modules dict
        if let Some(modules_dict) = sys_mod.get_attr("modules") {
            if matches!(&modules_dict.payload, PyObjectPayload::Dict(_)) {
                self.sys_modules_dict = Some(modules_dict.clone());
                // Populate it with all currently cached modules
                if let PyObjectPayload::Dict(ref d) = modules_dict.payload {
                    for (name, module) in &self.modules {
                        d.write().insert(
                            HashableKey::Str(name.clone()),
                            module.clone(),
                        );
                    }
                }
            }
        }
    }
}

/// Convert a bytecode constant to a runtime PyObject.
pub(crate) fn constant_to_object(constant: &ConstantValue) -> PyObjectRef {
    match constant {
        ConstantValue::None => PyObject::none(),
        ConstantValue::Bool(b) => PyObject::bool_val(*b),
        ConstantValue::Integer(n) => PyObject::int(*n),
        ConstantValue::BigInteger(n) => PyObject::big_int(n.as_ref().clone()),
        ConstantValue::Float(f) => PyObject::float(*f),
        ConstantValue::Complex { real, imag } => PyObject::complex(*real, *imag),
        ConstantValue::Str(s) => PyObject::str_val(s.clone()),
        ConstantValue::Bytes(b) => PyObject::bytes(b.clone()),
        ConstantValue::Ellipsis => PyObject::ellipsis(),
        ConstantValue::Code(code) => PyObject::code(*code.clone()),
        ConstantValue::Tuple(items) => {
            let objs: Vec<PyObjectRef> = items.iter().map(constant_to_object).collect();
            PyObject::tuple(objs)
        }
        ConstantValue::FrozenSet(items) => {
            let mut set = IndexMap::new();
            for item in items {
                let obj = constant_to_object(item);
                if let Ok(key) = obj.to_hashable_key() {
                    set.insert(key, obj);
                }
            }
            PyObject::set(set)
        }
    }
}
