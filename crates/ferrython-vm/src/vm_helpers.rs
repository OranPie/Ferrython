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
use ferrython_core::types::HashableKey;
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
                    // NativeFunction from class namespace needs self as first arg
                    let args = match &str_method.payload {
                        PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. } => vec![obj.clone()],
                        _ => vec![],
                    };
                    let result = self.call_object(str_method, args)?;
                    return Ok(result.py_to_string());
                }
                if let Some(repr_method) = obj.get_attr("__repr__") {
                    let args = match &repr_method.payload {
                        PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. } => vec![obj.clone()],
                        _ => vec![],
                    };
                    let result = self.call_object(repr_method, args)?;
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
                // Fall back to py_to_string which handles datetime/time/date/timedelta markers
                Ok(obj.py_to_string())
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
                    // Hide internal marker keys
                    if let HashableKey::Str(s) = k {
                        let key = s.as_str();
                        if key == "__defaultdict_factory__" || key == "__counter__" { continue; }
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
                } else if let Some(getitem) = obj.get_attr("__getitem__") {
                    // Fall back to __getitem__-based iteration (old-style sequence protocol)
                    let mut items = Vec::new();
                    let mut idx: i64 = 0;
                    loop {
                        match self.call_object(getitem.clone(), vec![PyObject::int(idx)]) {
                            Ok(val) => { items.push(val); idx += 1; }
                            Err(e) if e.kind == ExceptionKind::IndexError => break,
                            Err(e) => return Err(e),
                        }
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
        let frame = match gen.frame.take() {
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
            PyObjectPayload::Iterator(_) | PyObjectPayload::Generator(_) => obj.clone(),
            PyObjectPayload::Instance(_) => {
                if let Some(iter_fn) = obj.get_attr("__iter__") {
                    self.call_object(iter_fn, vec![])?
                } else {
                    return Err(PyException::type_error(format!(
                        "cannot unpack non-iterable {} object", obj.type_name()
                    )));
                }
            }
            _ => {
                return Err(PyException::type_error(format!(
                    "cannot unpack non-iterable {} object", obj.type_name()
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
        if let PyObjectPayload::Instance(_inst) = &a.payload {
            if let Some(method) = a.get_attr("__lt__") {
                // If method is from class namespace (not bound), pass self explicitly
                let result = if matches!(&method.payload, PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. } | PyObjectPayload::Function(_)) {
                    self.call_object(method, vec![a.clone(), b.clone()])?
                } else {
                    self.call_object(method, vec![b.clone()])?
                };
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
            result = self.import_module_simple(&req.name, req.level)?;
        }
        // importlib.import_module() intercept
        if let Some(req) = ferrython_stdlib::take_import_module_request() {
            let (name, level) = if req.name.starts_with('.') {
                let dots = req.name.chars().take_while(|c| *c == '.').count();
                let rest = &req.name[dots..];
                if let Some(ref pkg) = req.package {
                    let abs_name = if rest.is_empty() {
                        pkg.to_string()
                    } else {
                        format!("{}.{}", pkg, rest)
                    };
                    (abs_name, dots)
                } else {
                    (rest.to_string(), dots)
                }
            } else {
                (req.name.to_string(), 0)
            };
            result = self.import_module_simple(&name, level)?;
        }
        // importlib.reload() intercept
        if let Some(req) = ferrython_stdlib::take_reload_request() {
            result = self.reload_module(req.module)?;
        }
        Ok(result)
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

impl VirtualMachine {
    // ── exec/eval/compile helpers (moved from vm_call.rs) ──

    pub(crate) fn builtin_exec(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() || args.len() > 3 {
            return Err(PyException::type_error("exec() takes 1 to 3 arguments"));
        }
        let code = if let PyObjectPayload::Code(co) = &args[0].payload {
            (**co).clone()
        } else {
            let code_str = args[0].as_str().ok_or_else(||
                PyException::type_error("exec() arg 1 must be a string or code object"))?;
            let module = ferrython_parser::parse(code_str, "<string>")
                .map_err(|e| PyException::syntax_error(format!("exec: {}", e)))?;
            let mut compiler = ferrython_compiler::Compiler::new("<string>".to_string());
            compiler.compile_module(&module)
                .map_err(|_| PyException::syntax_error("exec: compilation failed"))?
        };
        if args.len() >= 2 {
            if let PyObjectPayload::Dict(ref map) = args[1].payload {
                let mut new_globals = IndexMap::new();
                let m = map.read();
                for (k, v) in m.iter() {
                    let key_str = match k {
                        HashableKey::Str(s) => s.clone(),
                        _ => CompactString::from(format!("{:?}", k)),
                    };
                    new_globals.insert(key_str, v.clone());
                }
                drop(m);
                let shared = Arc::new(RwLock::new(new_globals));
                self.execute_with_globals(code, shared.clone())?;
                let results = shared.read();
                let mut m = map.write();
                for (k, v) in results.iter() {
                    m.insert(HashableKey::Str(k.clone()), v.clone());
                }
            } else {
                return Err(PyException::type_error("exec() globals must be a dict"));
            }
        } else {
            let globals = self.call_stack.last().unwrap().globals.clone();
            self.execute_with_globals(code, globals)?;
        }
        Ok(PyObject::none())
    }

    pub(crate) fn builtin_eval(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() || args.len() > 3 {
            return Err(PyException::type_error("eval() takes 1 to 3 arguments"));
        }
        let code_str = args[0].as_str().ok_or_else(||
            PyException::type_error("eval() arg 1 must be a string"))?;
        let wrapped = format!("__eval_result__ = ({})", code_str);
        let module = ferrython_parser::parse(&wrapped, "<string>")
            .map_err(|e| PyException::syntax_error(format!("eval: {}", e)))?;
        let mut compiler = ferrython_compiler::Compiler::new("<string>".to_string());
        let code = compiler.compile_module(&module)
            .map_err(|_| PyException::syntax_error("eval: compilation failed"))?;
        if args.len() >= 2 {
            if let PyObjectPayload::Dict(ref map) = args[1].payload {
                let mut new_globals = IndexMap::new();
                let m = map.read();
                for (k, v) in m.iter() {
                    let key_str = match k {
                        HashableKey::Str(s) => s.clone(),
                        _ => CompactString::from(format!("{:?}", k)),
                    };
                    new_globals.insert(key_str, v.clone());
                }
                drop(m);
                let shared = Arc::new(RwLock::new(new_globals));
                self.execute_with_globals(code, shared.clone())?;
                let result = shared.read().get("__eval_result__").cloned()
                    .unwrap_or_else(PyObject::none);
                let results = shared.read();
                let mut m = map.write();
                for (k, v) in results.iter() {
                    m.insert(HashableKey::Str(k.clone()), v.clone());
                }
                Ok(result)
            } else {
                Err(PyException::type_error("eval() globals must be a dict"))
            }
        } else {
            let globals = self.call_stack.last().unwrap().globals.clone();
            self.execute_with_globals(code, globals.clone())?;
            let result = globals.read().get("__eval_result__").cloned()
                .unwrap_or_else(PyObject::none);
            Ok(result)
        }
    }

    pub(crate) fn builtin_compile(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 {
            return Err(PyException::type_error("compile() requires at least 3 arguments"));
        }
        let source = args[0].as_str().ok_or_else(||
            PyException::type_error("compile() arg 1 must be a string"))?;
        let filename = args[1].py_to_string();
        let _mode = args[2].py_to_string();
        let module = ferrython_parser::parse(source, &filename)
            .map_err(|e| PyException::syntax_error(format!("compile: {}", e)))?;
        let mut compiler = ferrython_compiler::Compiler::new(filename);
        let code = compiler.compile_module(&module)
            .map_err(|_| PyException::syntax_error("compile: compilation failed"))?;
        Ok(PyObject::wrap(PyObjectPayload::Code(Box::new(code))))
    }

    // ── Regex helpers (moved from vm_call.rs) ──

    /// Handle re.sub/re.subn when the replacement is a callable
    pub(crate) fn re_sub_with_callable(&mut self, args: &[PyObjectRef], return_count: bool) -> PyResult<PyObjectRef> {
        let pattern = args[0].py_to_string();
        let repl_fn = args[1].clone();
        let text = args[2].py_to_string();
        let flags = if args.len() > 4 { args[4].to_int().unwrap_or(0) } else { 0 };

        let mut re_pattern = pattern.clone();
        re_pattern = re_pattern.replace("(?P<", "(?P<");
        let re = if flags & 2 != 0 {
            regex::RegexBuilder::new(&re_pattern).case_insensitive(true).build()
        } else {
            regex::Regex::new(&re_pattern)
        }.map_err(|e| PyException::runtime_error(format!("regex error: {}", e)))?;

        let mut result = String::new();
        let mut last_end = 0;
        let mut count = 0;
        for m in re.find_iter(&text) {
            result.push_str(&text[last_end..m.start()]);

            let match_text = m.as_str().to_string();
            let mut match_attrs = IndexMap::new();
            match_attrs.insert(CompactString::from("_match_str"), PyObject::str_val(CompactString::from(match_text.clone())));
            match_attrs.insert(CompactString::from("group"), PyObject::native_closure("group", {
                let mt = match_text.clone();
                move |args| {
                    let idx = if args.is_empty() { 0 } else { args[0].to_int().unwrap_or(0) };
                    if idx == 0 {
                        Ok(PyObject::str_val(CompactString::from(mt.clone())))
                    } else {
                        Ok(PyObject::none())
                    }
                }
            }));
            match_attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
            let match_obj = PyObject::module_with_attrs(CompactString::from("_match"), match_attrs);

            let replacement = self.call_object(repl_fn.clone(), vec![match_obj])?;
            result.push_str(&replacement.py_to_string());

            last_end = m.end();
            count += 1;
        }
        result.push_str(&text[last_end..]);

        if return_count {
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(result)),
                PyObject::int(count),
            ]))
        } else {
            Ok(PyObject::str_val(CompactString::from(result)))
        }
    }

    // ── Itertools helpers (moved from vm_call.rs) ──

    pub(crate) fn vm_itertools_groupby(&mut self, args: &[PyObjectRef], key_fn: Option<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("groupby requires iterable"));
        }
        let items = args[0].to_list()?;
        if items.is_empty() {
            return Ok(PyObject::list(vec![]));
        }

        let mut result = Vec::new();
        let first_key = if let Some(ref kf) = key_fn {
            self.call_object(kf.clone(), vec![items[0].clone()])?
        } else {
            items[0].clone()
        };
        let mut current_key = first_key;
        let mut current_group = vec![items[0].clone()];

        for item in &items[1..] {
            let k = if let Some(ref kf) = key_fn {
                self.call_object(kf.clone(), vec![item.clone()])?
            } else {
                item.clone()
            };
            if k.py_to_string() == current_key.py_to_string() {
                current_group.push(item.clone());
            } else {
                result.push(PyObject::tuple(vec![
                    current_key,
                    PyObject::list(current_group),
                ]));
                current_key = k;
                current_group = vec![item.clone()];
            }
        }
        result.push(PyObject::tuple(vec![
            current_key,
            PyObject::list(current_group),
        ]));
        Ok(PyObject::list(result))
    }

    pub(crate) fn vm_itertools_filterfalse(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let pred = args[0].clone();
        let items = self.collect_iterable(&args[1])?;
        let mut result = Vec::new();
        let is_none = matches!(&pred.payload, PyObjectPayload::None);
        for item in &items {
            let val = if is_none {
                item.is_truthy()
            } else {
                let r = self.call_object(pred.clone(), vec![item.clone()])?;
                r.is_truthy()
            };
            if !val {
                result.push(item.clone());
            }
        }
        Ok(PyObject::list(result))
    }

    pub(crate) fn vm_itertools_starmap(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let func = args[0].clone();
        let items = self.collect_iterable(&args[1])?;
        let mut result = Vec::new();
        for item in &items {
            let call_args = item.to_list().unwrap_or_else(|_| vec![item.clone()]);
            let r = self.call_object(func.clone(), call_args)?;
            result.push(r);
        }
        Ok(PyObject::list(result))
    }

    pub(crate) fn vm_itertools_accumulate(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let items = args[0].to_list()?;
        if items.is_empty() { return Ok(PyObject::list(vec![])); }
        let func = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None | PyObjectPayload::Dict(_)) {
            Some(args[1].clone())
        } else {
            None
        };
        let mut result = Vec::new();
        let mut acc = items[0].clone();
        result.push(acc.clone());
        for item in &items[1..] {
            acc = if let Some(ref f) = func {
                self.call_object(f.clone(), vec![acc, item.clone()])?
            } else {
                acc.add(item)?
            };
            result.push(acc.clone());
        }
        Ok(PyObject::list(result))
    }
}
