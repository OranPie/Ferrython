//! Generator, coroutine, and async-generator VM lifecycle helpers.

use crate::frame::{BlockKind, Frame};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    AsyncGenAction, GeneratorState, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use std::rc::Rc;

/// Generator frame buffer pool — eliminates malloc/free per yield/resume.
/// Uses static UnsafeCell for zero-overhead access (same pattern as PyObjectRef pool).
/// SAFETY: single-threaded interpreter — only one thread runs Python bytecode.
const GEN_FRAME_POOL_CAP: usize = 32;

struct GenFramePool(std::cell::UnsafeCell<Vec<*mut Frame>>);
unsafe impl Sync for GenFramePool {}

static GEN_FRAME_POOL: GenFramePool = GenFramePool(std::cell::UnsafeCell::new(Vec::new()));

struct GeneratorExceptionContext {
    caller_exception: Option<PyException>,
    caller_stack: Vec<Option<PyException>>,
}

/// Get a heap buffer sized for Frame — from pool or fresh allocation.
#[inline(always)]
fn gen_frame_alloc() -> *mut Frame {
    unsafe {
        let pool = &mut *GEN_FRAME_POOL.0.get();
        pool.pop()
            .unwrap_or_else(|| std::alloc::alloc(std::alloc::Layout::new::<Frame>()) as *mut Frame)
    }
}

/// Return a heap buffer to the pool (or dealloc if full).
#[inline(always)]
fn gen_frame_recycle(ptr: *mut Frame) {
    unsafe {
        let pool = &mut *GEN_FRAME_POOL.0.get();
        if pool.len() < GEN_FRAME_POOL_CAP {
            pool.push(ptr);
        } else {
            std::alloc::dealloc(ptr as *mut u8, std::alloc::Layout::new::<Frame>());
        }
    }
}

/// Free a generator frame when generator is dropped (e.g. GC).
/// Must drop the Frame's contents properly before recycling the buffer.
pub(crate) fn drop_generator_frame(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let frame_ptr = ptr as *mut Frame;
    unsafe {
        std::ptr::drop_in_place(frame_ptr);
    }
    gen_frame_recycle(frame_ptr);
}

impl VirtualMachine {
    fn frame_has_exception_handler(frame: &Frame) -> bool {
        frame
            .block_stack
            .iter()
            .any(|block| matches!(block.kind(), BlockKind::ExceptHandler))
    }

    fn enter_generator_exception_state_if_suspended(
        &mut self,
        gen_arc: &Rc<PyCell<GeneratorState>>,
    ) -> Option<GeneratorExceptionContext> {
        let mut gen = gen_arc.write();
        let saved = gen.suspended_exception.take();
        let saved_stack: Vec<_> = gen.suspended_exception_stack.drain(..).collect();
        drop(gen);
        if saved.is_none() {
            return None;
        }
        let caller_exception = self.active_exception.clone();
        let caller_stack = std::mem::take(&mut self.exception_state_stack);
        self.exception_state_stack.extend(saved_stack);
        self.active_exception = saved;
        if let Some(exc) = &self.active_exception {
            ferrython_core::error::set_thread_exc_info(
                exc.kind,
                exc.message.clone(),
                exc.traceback.clone(),
            );
        } else {
            ferrython_core::error::clear_thread_exc_info();
        }
        Some(GeneratorExceptionContext {
            caller_exception,
            caller_stack,
        })
    }

    fn restore_generator_caller_exception_state(&mut self, ctx: GeneratorExceptionContext) {
        self.active_exception = ctx.caller_exception;
        self.exception_state_stack = ctx.caller_stack;
        if let Some(exc) = &self.active_exception {
            ferrython_core::error::set_thread_exc_info(
                exc.kind,
                exc.message.clone(),
                exc.traceback.clone(),
            );
        } else {
            ferrython_core::error::clear_thread_exc_info();
        }
    }

    fn save_generator_exception_state_on_yield(
        &self,
        gen: &mut GeneratorState,
        has_exception_handler: bool,
        strip_inherited_stack_prefix: Option<usize>,
    ) {
        if !has_exception_handler {
            gen.suspended_exception = None;
            gen.suspended_exception_stack.clear();
            return;
        }

        gen.suspended_exception = self.active_exception.clone();
        let mut stack = self.exception_state_stack.clone();
        if let Some(prefix_len) = strip_inherited_stack_prefix {
            if prefix_len <= stack.len() {
                stack.drain(0..prefix_len);
            } else {
                stack.clear();
            }
            if let Some(first) = stack.first_mut() {
                *first = None;
            }
        }
        gen.suspended_exception_stack = stack;
    }

    fn wrap_generator_stop_iteration(mut exc: PyException) -> PyException {
        if exc.kind != ExceptionKind::StopIteration {
            return exc;
        }
        exc.ensure_original();

        let mut runtime = PyException::runtime_error("generator raised StopIteration");
        runtime.cause = Some(Box::new(exc.clone()));
        runtime.context = Some(Box::new(exc));
        runtime
    }

    /// Resume a generator, pushing the given `send_value` onto its stack and running
    /// until the next `YieldValue` or `ReturnValue`.
    /// Returns `Ok(value)` for yielded values, or `Err(StopIteration)` when done.
    pub(crate) fn resume_generator(
        &mut self,
        gen_arc: &Rc<PyCell<GeneratorState>>,
        send_value: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let mut gen = gen_arc.write();
        if gen.finished {
            return Err(PyException::new(ExceptionKind::StopIteration, ""));
        }
        if gen.running {
            return Err(PyException::runtime_error("generator already executing"));
        }
        let frame_raw = gen.take_frame_ptr();
        if frame_raw.is_null() {
            return Err(PyException::runtime_error("generator already executing"));
        }
        gen.running = true;
        // Direct cast from raw pointer — no dyn Any downcast.
        // Push frame onto call_stack using copy_nonoverlapping (1 memcpy, no Box dealloc).
        let frame_typed = frame_raw as *mut Frame;
        self.call_stack.reserve(1);
        unsafe {
            let len = self.call_stack.len();
            let dst = self.call_stack.as_mut_ptr().add(len);
            std::ptr::copy_nonoverlapping(frame_typed, dst, 1);
            self.call_stack.set_len(len + 1);
        }
        // frame_typed now points to deallocated memory — recycle the buffer
        gen_frame_recycle(frame_typed);

        if gen.started {
            let frame = self.call_stack.last_mut().unwrap();
            frame.push(send_value);
        }
        gen.started = true;
        drop(gen); // release lock before executing

        let inherited_exception_stack_len = self.exception_state_stack.len();
        self.current_generators.push(gen_arc.clone());
        let generator_exception_ctx = self.enter_generator_exception_state_if_suspended(gen_arc);
        let result = self.run_frame();
        self.current_generators.pop();
        let cs_len = self.call_stack.len();
        let frame_yielded = self.call_stack[cs_len - 1].yielded;
        let has_exception_handler = Self::frame_has_exception_handler(&self.call_stack[cs_len - 1]);

        let mut gen = gen_arc.write();
        gen.running = false;
        if frame_yielded {
            self.save_generator_exception_state_on_yield(
                &mut gen,
                has_exception_handler,
                generator_exception_ctx
                    .is_none()
                    .then_some(inherited_exception_stack_len),
            );
            let frame_ref = &mut self.call_stack[cs_len - 1];
            frame_ref.yielded = false;
            // Copy frame from call_stack to a heap buffer (1 memcpy, reuses freelist)
            let buf = gen_frame_alloc();
            unsafe {
                std::ptr::copy_nonoverlapping(frame_ref as *const Frame, buf, 1);
                self.call_stack.set_len(cs_len - 1); // "pop" without drop
            }
            gen.set_frame_ptr(buf as *mut u8);
            drop(gen);
            if let Some(ctx) = generator_exception_ctx {
                self.restore_generator_caller_exception_state(ctx);
            }
            result // Ok(yielded_value)
        } else {
            // Generator finished — pop and recycle frame normally
            gen.finished = true;
            gen.clear_frame();
            let frame = self.call_stack.pop().unwrap();
            frame.recycle(&mut self.frame_pool);
            drop(gen);
            if let Some(ctx) = generator_exception_ctx {
                self.restore_generator_caller_exception_state(ctx);
            }
            match result {
                Ok(return_val) => {
                    let msg = return_val.py_to_string();
                    let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                    exc.value = Some(return_val);
                    Err(exc)
                }
                Err(e) => Err(Self::wrap_generator_stop_iteration(e)),
            }
        }
    }

    /// Specialized generator resume for ForIter: returns Ok(Some(value)) on yield,
    /// Ok(None) on generator completion (avoids creating StopIteration exception),
    /// Err(e) on actual errors from within the generator.
    pub(crate) fn resume_generator_for_iter(
        &mut self,
        gen_arc: &Rc<PyCell<GeneratorState>>,
    ) -> Result<Option<PyObjectRef>, PyException> {
        let mut gen = gen_arc.write();
        if gen.finished {
            return Ok(None);
        }
        if gen.running {
            return Err(PyException::runtime_error("generator already executing"));
        }
        let frame_raw = gen.take_frame_ptr();
        if frame_raw.is_null() {
            return Err(PyException::runtime_error("generator already executing"));
        }
        gen.running = true;
        let frame_typed = frame_raw as *mut Frame;
        self.call_stack.reserve(1);
        unsafe {
            let len = self.call_stack.len();
            let dst = self.call_stack.as_mut_ptr().add(len);
            std::ptr::copy_nonoverlapping(frame_typed, dst, 1);
            self.call_stack.set_len(len + 1);
        }
        gen_frame_recycle(frame_typed);

        if gen.started {
            let frame = self.call_stack.last_mut().unwrap();
            frame.push(PyObject::none());
        }
        gen.started = true;
        drop(gen);

        let inherited_exception_stack_len = self.exception_state_stack.len();
        self.current_generators.push(gen_arc.clone());
        let generator_exception_ctx = self.enter_generator_exception_state_if_suspended(gen_arc);
        let result = self.run_frame();
        self.current_generators.pop();
        let cs_len = self.call_stack.len();
        let frame_yielded = self.call_stack[cs_len - 1].yielded;
        let has_exception_handler = Self::frame_has_exception_handler(&self.call_stack[cs_len - 1]);

        let mut gen = gen_arc.write();
        gen.running = false;
        if frame_yielded {
            self.save_generator_exception_state_on_yield(
                &mut gen,
                has_exception_handler,
                generator_exception_ctx
                    .is_none()
                    .then_some(inherited_exception_stack_len),
            );
            let frame_ref = &mut self.call_stack[cs_len - 1];
            frame_ref.yielded = false;
            let buf = gen_frame_alloc();
            unsafe {
                std::ptr::copy_nonoverlapping(frame_ref as *const Frame, buf, 1);
                self.call_stack.set_len(cs_len - 1);
            }
            gen.set_frame_ptr(buf as *mut u8);
            drop(gen);
            if let Some(ctx) = generator_exception_ctx {
                self.restore_generator_caller_exception_state(ctx);
            }
            result.map(Some)
        } else {
            gen.finished = true;
            gen.clear_frame();
            let frame = self.call_stack.pop().unwrap();
            frame.recycle(&mut self.frame_pool);
            drop(gen);
            if let Some(ctx) = generator_exception_ctx {
                self.restore_generator_caller_exception_state(ctx);
            }
            match result {
                Ok(_) => Ok(None), // Generator finished — no StopIteration needed
                Err(e) if e.kind == ExceptionKind::StopIteration => {
                    Err(Self::wrap_generator_stop_iteration(e))
                }
                Err(e) => Err(e),
            }
        }
    }

    /// Throw an exception into a generator.
    /// Resumes the generator with an exception injected at the yield point.
    pub(crate) fn gen_throw(
        &mut self,
        gen_arc: &Rc<PyCell<GeneratorState>>,
        kind: ExceptionKind,
        msg: CompactString,
    ) -> PyResult<PyObjectRef> {
        self.gen_throw_with_value(gen_arc, kind, msg, None)
    }

    /// Like gen_throw but preserves an original exception value for identity-
    /// preserving re-raise (needed by contextlib._GeneratorContextManager.__exit__
    /// which does `exc is not value`).
    pub(crate) fn gen_throw_with_value(
        &mut self,
        gen_arc: &Rc<PyCell<GeneratorState>>,
        kind: ExceptionKind,
        msg: CompactString,
        original_value: Option<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        let mut gen = gen_arc.write();
        if gen.finished {
            return Err(PyException::new(kind, msg));
        }
        if gen.running {
            return Err(PyException::runtime_error("generator already executing"));
        }
        let frame_raw = gen.take_frame_ptr();
        if frame_raw.is_null() {
            return Err(PyException::runtime_error("generator already executing"));
        }
        gen.running = true;
        // Push frame from raw pointer to call_stack (1 memcpy, no downcast)
        let frame_typed = frame_raw as *mut Frame;
        self.call_stack.reserve(1);
        unsafe {
            let len = self.call_stack.len();
            let dst = self.call_stack.as_mut_ptr().add(len);
            std::ptr::copy_nonoverlapping(frame_typed, dst, 1);
            self.call_stack.set_len(len + 1);
        }
        gen_frame_recycle(frame_typed);

        gen.started = true;
        drop(gen);

        // Set up exception on the frame so VM will unwind to handler
        let inherited_exception_stack_len = self.exception_state_stack.len();
        let mut exc = PyException::new(kind, msg.clone());
        if let Some(ref orig) = original_value {
            exc.original = Some(orig.clone());
        }
        let exc_result = Err(exc);
        let (exc_obj, exc_type) = if let Some(orig) = &original_value {
            let typ = match &orig.payload {
                PyObjectPayload::Instance(inst) => inst.class.clone(),
                PyObjectPayload::ExceptionInstance(ei) => PyObject::exception_type(ei.kind),
                _ => PyObject::exception_type(kind),
            };
            (orig.clone(), typ)
        } else {
            (
                PyObject::exception_instance(kind, msg.clone()),
                PyObject::exception_type(kind),
            )
        };
        let tb = PyObject::none();

        // Try to find an exception handler in the generator's frame
        if let Some(handler_ip) = self.unwind_except() {
            let mut active = PyException::new(kind, msg);
            if let Some(ref orig) = original_value {
                active.original = Some(orig.clone());
                active.kind = match &orig.payload {
                    PyObjectPayload::Instance(inst) => Self::find_exception_kind(&inst.class),
                    PyObjectPayload::ExceptionInstance(ei) => ei.kind,
                    _ => active.kind,
                };
            }
            self.enter_exception_handler(active);
            let frame_ref = self.call_stack.last_mut().unwrap();
            frame_ref.push(tb);
            frame_ref.push(exc_obj);
            frame_ref.push(exc_type);
            frame_ref.ip = handler_ip;

            self.current_generators.push(gen_arc.clone());
            let result = self.run_frame();
            self.current_generators.pop();
            let cs_len = self.call_stack.len();
            let frame_yielded = self.call_stack[cs_len - 1].yielded;
            let has_exception_handler =
                Self::frame_has_exception_handler(&self.call_stack[cs_len - 1]);

            let mut gen = gen_arc.write();
            gen.running = false;
            if frame_yielded {
                self.save_generator_exception_state_on_yield(
                    &mut gen,
                    has_exception_handler,
                    Some(inherited_exception_stack_len),
                );
                let frame_ref = &mut self.call_stack[cs_len - 1];
                frame_ref.yielded = false;
                let buf = gen_frame_alloc();
                unsafe {
                    std::ptr::copy_nonoverlapping(frame_ref as *const Frame, buf, 1);
                    self.call_stack.set_len(cs_len - 1);
                }
                gen.set_frame_ptr(buf as *mut u8);
                drop(gen);
                self.restore_previous_exception();
                result
            } else {
                gen.finished = true;
                gen.clear_frame();
                let frame = self.call_stack.pop().unwrap();
                frame.recycle(&mut self.frame_pool);
                drop(gen);
                self.restore_previous_exception();
                if let Err(e) = result {
                    return Err(Self::wrap_generator_stop_iteration(e));
                }
                let return_val = result.ok();
                let msg = return_val
                    .as_ref()
                    .map(|v| v.py_to_string())
                    .unwrap_or_default();
                let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                exc.value = return_val;
                Err(exc)
            }
        } else {
            // No handler — pop frame and re-raise
            let frame = self.call_stack.pop().unwrap();
            frame.recycle(&mut self.frame_pool);
            let mut gen = gen_arc.write();
            gen.running = false;
            gen.finished = true;
            gen.clear_frame();
            drop(gen);
            self.restore_previous_exception();
            match exc_result {
                Err(e) => Err(Self::wrap_generator_stop_iteration(e)),
                Ok(value) => Ok(value),
            }
        }
    }

    /// Parse the arguments to generator.throw() / coroutine.throw() into (ExceptionKind, message).
    pub(crate) fn parse_throw_args(args: &[PyObjectRef]) -> (ExceptionKind, CompactString) {
        let msg: CompactString = match args {
            [first] => match &first.payload {
                PyObjectPayload::ExceptionInstance(ei) => ei.message.clone(),
                PyObjectPayload::Instance(_) => first.py_to_string().into(),
                _ => CompactString::new(""),
            },
            [_, value, ..] => value.py_to_string().into(),
            [] => CompactString::new(""),
        };
        let kind = if !args.is_empty() {
            match &args[0].payload {
                PyObjectPayload::ExceptionType(k) => *k,
                PyObjectPayload::BuiltinType(name) => {
                    ExceptionKind::from_name(name).unwrap_or(ExceptionKind::RuntimeError)
                }
                PyObjectPayload::ExceptionInstance(ei) => ei.kind,
                PyObjectPayload::Instance(inst) if Self::is_exception_class(&inst.class) => {
                    Self::find_exception_kind(&inst.class)
                }
                PyObjectPayload::Class(_) if Self::is_exception_class(&args[0]) => {
                    Self::find_exception_kind(&args[0])
                }
                _ => ExceptionKind::RuntimeError,
            }
        } else {
            ExceptionKind::RuntimeError
        };
        (kind, msg)
    }

    pub(crate) fn parse_throw_original_value(args: &[PyObjectRef]) -> Option<PyObjectRef> {
        let candidate = match args {
            [first, ..] if Self::is_exception_value(first) => Some(first),
            [_, value, ..] if Self::is_exception_value(value) => Some(value),
            _ => None,
        }?;
        Some(candidate.clone())
    }

    fn is_exception_value(obj: &PyObjectRef) -> bool {
        match &obj.payload {
            PyObjectPayload::ExceptionInstance(_) => true,
            PyObjectPayload::Instance(inst) => Self::is_exception_class(&inst.class),
            _ => false,
        }
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
        gen: &Rc<PyCell<GeneratorState>>,
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
                        Err(PyException::new(
                            ExceptionKind::StopAsyncIteration,
                            String::new(),
                        ))
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
                    Err(e) if e.kind == ExceptionKind::StopIteration => Err(PyException::new(
                        ExceptionKind::StopAsyncIteration,
                        String::new(),
                    )),
                    Err(e) => Err(e),
                }
            }
            AsyncGenAction::Throw(exc_kind, msg) => self.gen_throw(gen, *exc_kind, msg.clone()),
            AsyncGenAction::Close => {
                // Like generator.close(): throw GeneratorExit, expect finish
                let g = gen.read();
                if g.finished || !g.has_frame() {
                    drop(g);
                    return Ok(PyObject::none());
                }
                drop(g);
                match self.gen_throw(gen, ExceptionKind::GeneratorExit, CompactString::new("")) {
                    Ok(_yielded) => Err(PyException::runtime_error(
                        "async generator ignored GeneratorExit",
                    )),
                    Err(e)
                        if e.kind == ExceptionKind::GeneratorExit
                            || e.kind == ExceptionKind::StopIteration
                            || e.kind == ExceptionKind::StopAsyncIteration =>
                    {
                        let mut g = gen.write();
                        g.finished = true;
                        g.clear_frame();
                        Ok(PyObject::none())
                    }
                    Err(e) => {
                        let mut g = gen.write();
                        g.finished = true;
                        g.clear_frame();
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
            PyObjectPayload::DeferredSleep {
                secs,
                result: sleep_result,
            } => {
                // Perform the deferred sleep now, respecting wait_for deadline
                let secs = *secs;
                let sleep_result = sleep_result.clone();
                let deadline = ferrython_async::get_wait_for_deadline();
                if let Some(dl) = deadline {
                    let now = std::time::Instant::now();
                    if now >= dl {
                        ferrython_async::set_wait_for_deadline(None);
                        return Err(PyException::new(ExceptionKind::TimeoutError, ""));
                    }
                    let remaining = dl.duration_since(now).as_secs_f64();
                    if secs > remaining {
                        std::thread::sleep(std::time::Duration::from_secs_f64(remaining));
                        ferrython_async::set_wait_for_deadline(None);
                        return Err(PyException::new(ExceptionKind::TimeoutError, ""));
                    }
                    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                } else {
                    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                }
                Ok(sleep_result)
            }
            _ => Ok(result),
        }
    }
}
