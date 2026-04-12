//! Exception handling: try/except/finally, with statements, raise

use crate::frame::BlockKind;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

impl VirtualMachine {
    pub(crate) fn exec_exception_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::SetupFinally => {
                self.vm_frame().push_block(BlockKind::Finally, instr.arg as usize);
            }
            Opcode::SetupExcept => {
                self.vm_frame().push_block(BlockKind::Except, instr.arg as usize);
            }
            Opcode::PopBlock => { self.vm_frame().pop_block(); }
            Opcode::PopExcept => {
                self.vm_frame().pop_block();
                self.active_exception = None;
                ferrython_stdlib::clear_exc_info();
                ferrython_core::error::clear_thread_exc_info();
            }
            Opcode::EndFinally => {
                return self.exec_end_finally();
            }
            Opcode::BeginFinally => {
                self.vm_frame().push(PyObject::none());
            }
            Opcode::RaiseVarargs => {
                return self.exec_raise_varargs(instr.arg);
            }
            Opcode::SetupWith => {
                return self.exec_setup_with(instr.arg);
            }

            Opcode::SetupAsyncWith => {
                // At this point, __aenter__() has already been called and awaited.
                // TOS = result of __aenter__ (the value for `as` clause).
                // Below TOS = the async context manager.
                let enter_result = self.vm_pop();
                let ctx_mgr = self.vm_pop();
                if matches!(&ctx_mgr.payload, PyObjectPayload::AsyncGenerator(_)) {
                    // AsyncGenerator used as async context manager (from @asynccontextmanager)
                    // Push the generator itself — WithCleanupStart will resume/close it
                    self.vm_push(ctx_mgr.clone());
                } else {
                    let exit_raw = ctx_mgr.get_attr("__aexit__").ok_or_else(||
                        PyException::attribute_error("__aexit__"))?;
                    let exit_method = if matches!(&exit_raw.payload, PyObjectPayload::BoundMethod { .. }) {
                        exit_raw
                    } else {
                        PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: ctx_mgr.clone(),
                                method: exit_raw,
                            }
                        })
                    };
                    self.vm_push(exit_method);
                }
                let frame = self.vm_frame();
                frame.push_block(BlockKind::With, instr.arg as usize);
                frame.push(enter_result);
            }

            Opcode::WithCleanupStart => {
                let tos = self.vm_frame().peek().clone();
                // Extract __closing_thing__ from context manager (for contextlib.closing)
                // We peek at exit_fn (2nd from top) to get the receiver before consumption
                let closing_thing = {
                    let stack = &self.vm_frame().stack;
                    if stack.len() >= 2 {
                        let exit_fn_ref = &stack[stack.len() - 2];
                        if let PyObjectPayload::BoundMethod { receiver, .. } = &exit_fn_ref.payload {
                            receiver.get_attr("__closing_thing__")
                        } else { None }
                    } else { None }
                };
                if matches!(tos.payload, PyObjectPayload::None) {
                    // Normal exit (no exception)
                    self.vm_pop(); // pop None
                    let exit_fn = self.vm_pop();

                    // Restore redirected streams before calling __exit__
                    if let PyObjectPayload::BoundMethod { receiver, .. } = &exit_fn.payload {
                        self.restore_redirect(receiver);
                    }

                    if let PyObjectPayload::Generator(gen_arc) = &exit_fn.payload {
                        match self.resume_generator(gen_arc, PyObject::none()) {
                            Ok(_) => {}
                            Err(e) if e.kind == ExceptionKind::StopIteration => {}
                            Err(e) => return Err(e),
                        }
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(PyObject::none());
                    } else if let PyObjectPayload::AsyncGenerator(gen_arc) = &exit_fn.payload {
                        match self.resume_generator(gen_arc, PyObject::none()) {
                            Ok(_) => {}
                            Err(e) if e.kind == ExceptionKind::StopIteration
                                   || e.kind == ExceptionKind::StopAsyncIteration => {}
                            Err(e) => return Err(e),
                        }
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(PyObject::none());
                    } else {
                        let result = self.call_object(exit_fn, vec![
                            PyObject::none(), PyObject::none(), PyObject::none()
                        ])?;
                        // Call close() on closing thing if present
                        if let Some(thing) = &closing_thing {
                            self.call_close_on(thing)?;
                        }
                        // If __aexit__ returns a coroutine, drive it to completion
                        let result = self.maybe_await_result(result)?;
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(result);
                    }
                } else if matches!(tos.payload, PyObjectPayload::ExceptionType(_))
                       || matches!(tos.payload, PyObjectPayload::Class(_)) {
                    // Exception exit: stack has [exit_fn, tb, value, type]
                    let exc_type = self.vm_pop();
                    let exc_val = if !self.vm_frame().stack.is_empty() { self.vm_pop() } else { PyObject::none() };
                    let exc_tb = if !self.vm_frame().stack.is_empty() { self.vm_pop() } else { PyObject::none() };
                    let exit_fn = self.vm_pop();

                    // Restore redirected streams before calling __exit__
                    if let PyObjectPayload::BoundMethod { receiver, .. } = &exit_fn.payload {
                        self.restore_redirect(receiver);
                    }

                    if let PyObjectPayload::Generator(gen_arc) | PyObjectPayload::AsyncGenerator(gen_arc) = &exit_fn.payload {
                        // Throw exception into generator so its except clauses can catch it
                        let exc_kind = match &exc_type.payload {
                            PyObjectPayload::ExceptionType(k) => *k,
                            PyObjectPayload::Class(_) => Self::find_exception_kind(&exc_type),
                            _ => ExceptionKind::RuntimeError,
                        };
                        let exc_msg = match &exc_val.payload {
                            PyObjectPayload::ExceptionInstance(ei) => ei.message.clone(),
                            _ => CompactString::from(exc_val.py_to_string()),
                        };
                        let gen_arc_clone = gen_arc.clone();
                        let throw_result = self.gen_throw(&gen_arc_clone, exc_kind, exc_msg.clone());
                        match throw_result {
                            Ok(_) | Err(PyException { kind: ExceptionKind::StopIteration, .. })
                                  | Err(PyException { kind: ExceptionKind::StopAsyncIteration, .. }) => {
                                // Generator handled exception (suppressed)
                                let f = self.vm_frame();
                                f.push(PyObject::none());
                                f.push(PyObject::none());
                                f.push(PyObject::none());
                                f.push(PyObject::bool_val(true));
                            }
                            Err(_e) => {
                                // Generator re-raised or raised a different exception
                                let f = self.vm_frame();
                                f.push(exc_tb);
                                f.push(exc_val);
                                f.push(exc_type.clone());
                                f.push(PyObject::none());
                            }
                        }
                    } else {
                        let result = self.call_object(exit_fn, vec![
                            exc_type.clone(), exc_val.clone(), exc_tb.clone()
                        ])?;
                        // Call close() on closing thing if present
                        if let Some(thing) = &closing_thing {
                            let _ = self.call_close_on(thing);
                        }
                        // If __aexit__ returns a coroutine, drive it to completion
                        let result = self.maybe_await_result(result)?;
                        let f = self.vm_frame();
                        // Preserve exception info for EndFinally re-raise
                        f.push(exc_tb);
                        f.push(exc_val);
                        f.push(exc_type);
                        f.push(result);
                    }
                } else {
                    self.vm_pop();
                    let exit_fn = self.vm_pop();
                    if let PyObjectPayload::Generator(gen_arc) = &exit_fn.payload {
                        match self.resume_generator(gen_arc, PyObject::none()) {
                            Ok(_) => {}
                            Err(e) if e.kind == ExceptionKind::StopIteration => {}
                            Err(e) => return Err(e),
                        }
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(PyObject::none());
                    } else {
                        let result = self.call_object(exit_fn, vec![
                            PyObject::none(), PyObject::none(), PyObject::none()
                        ])?;
                        // Call close() on closing thing if present
                        if let Some(thing) = &closing_thing {
                            let _ = self.call_close_on(thing);
                        }
                        let result = self.maybe_await_result(result)?;
                        let f = self.vm_frame();
                        f.push(PyObject::none());
                        f.push(result);
                    }
                }
            }
            Opcode::WithCleanupFinish => {
                let frame = self.vm_frame();
                let exit_result = frame.pop();
                let exc_or_none = frame.pop();
                if !matches!(exc_or_none.payload, PyObjectPayload::None) && exit_result.is_truthy() {
                    // Exception was suppressed: clean up exception info (value, tb)
                    frame.pop(); // value
                    frame.pop(); // tb
                    frame.push(PyObject::none());
                } else if !matches!(exc_or_none.payload, PyObjectPayload::None) {
                    // Exception NOT suppressed: push type back, leave (tb, value) for EndFinally
                    frame.push(exc_or_none);
                } else {
                    // No exception
                    frame.push(exc_or_none);
                }
            }
            _ => unreachable!(),
        }
        Ok(None)
    }

    fn exec_end_finally(&mut self) -> Result<Option<PyObjectRef>, PyException> {
        let frame = self.vm_frame();
        if let Some(ret_val) = frame.pending_return.take() {
            let mut has_finally = false;
            while let Some(block) = frame.block_stack.last() {
                if block.kind == BlockKind::Finally {
                    let handler = block.handler;
                    frame.block_stack.pop();
                    frame.pending_return = Some(ret_val.clone());
                    frame.push(PyObject::none());
                    frame.ip = handler;
                    has_finally = true;
                    break;
                } else {
                    frame.block_stack.pop();
                }
            }
            if !has_finally {
                return Ok(Some(ret_val));
            }
        } else {
            if !frame.stack.is_empty() {
                let tos = frame.peek();
                match &tos.payload {
                    PyObjectPayload::ExceptionType(kind) => {
                        let kind = *kind;
                        frame.pop();
                        let value = if !frame.stack.is_empty() { frame.pop() } else { PyObject::none() };
                        if !frame.stack.is_empty() { frame.pop(); }
                        let msg = match &value.payload {
                            PyObjectPayload::ExceptionInstance(ei) => ei.message.clone(),
                            _ => CompactString::from(value.py_to_string()),
                        };
                        // Preserve original value for identity-based checks
                        // (e.g. contextlib's `exc is not value`)
                        if matches!(value.payload, PyObjectPayload::ExceptionInstance(_)
                            | PyObjectPayload::Instance(_)) {
                            return Err(PyException::with_original(kind, msg, value));
                        }
                        return Err(PyException::new(kind, msg));
                    }
                    PyObjectPayload::Class(_) => {
                        // User-defined exception class on stack — re-raise
                        let cls = frame.pop();
                        let kind = Self::find_exception_kind(&cls);
                        let value = if !frame.stack.is_empty() { frame.pop() } else { PyObject::none() };
                        if !frame.stack.is_empty() { frame.pop(); }
                        let msg = match &value.payload {
                            PyObjectPayload::ExceptionInstance(ei) => ei.message.clone(),
                            PyObjectPayload::Instance(_) => {
                                if let Some(args) = value.get_attr("args") {
                                    CompactString::from(args.py_to_string())
                                } else {
                                    CompactString::from(value.py_to_string())
                                }
                            }
                            _ => CompactString::from(value.py_to_string()),
                        };
                        return Err(PyException::with_original(kind, msg, value));
                    }
                    PyObjectPayload::None => { frame.pop(); }
                    _ => {}
                }
            }
        }
        Ok(None)
    }

    fn exec_raise_varargs(&mut self, argc: u32) -> Result<Option<PyObjectRef>, PyException> {
        let frame = self.vm_frame();
        let raise_exc = |exc: &PyObjectRef| -> PyException {
            match &exc.payload {
                PyObjectPayload::ExceptionInstance(ei) => {
                    PyException::with_original(ei.kind, ei.message.clone(), exc.clone())
                }
                PyObjectPayload::ExceptionType(kind) => {
                    PyException::new(*kind, "")
                }
                PyObjectPayload::Instance(inst) => {
                    let kind = Self::find_exception_kind(&inst.class);
                    // Derive message from args (CPython: str(exc) uses args)
                    let msg = if let Some(a) = exc.get_attr("args") {
                        if let PyObjectPayload::Tuple(items) = &a.payload {
                            if items.len() == 1 {
                                items[0].py_to_string()
                            } else if items.is_empty() {
                                String::new()
                            } else {
                                a.repr()
                            }
                        } else {
                            exc.py_to_string()
                        }
                    } else {
                        exc.py_to_string()
                    };
                    PyException::with_original(kind, msg, exc.clone())
                }
                PyObjectPayload::Class(_) => {
                    let kind = Self::find_exception_kind(exc);
                    PyException::new(kind, "")
                }
                _ => PyException::runtime_error(exc.py_to_string()),
            }
        };
        match argc {
            0 => {
                // Bare raise: re-raise the currently active exception
                if let Some(exc) = self.active_exception.clone() {
                    return Err(exc);
                }
                return Err(PyException::runtime_error("No active exception to re-raise"));
            }
            1 => {
                let exc = frame.pop();
                let py_exc = raise_exc(&exc);
                // Context chaining (__context__) is deferred to the unwind
                // path in run_vm() — avoids expensive cloning for exceptions
                // that are immediately caught in the same frame.
                return Err(py_exc);
            }
            2 => {
                let cause = frame.pop();
                let exc = frame.pop();
                let mut py_exc = raise_exc(&exc);
                // `raise X from None` suppresses the cause
                if matches!(cause.payload, PyObjectPayload::None) {
                    // Ensure we have an original ExceptionInstance to store attrs on
                    py_exc.ensure_original();
                    if let Some(ref original) = py_exc.original {
                        if let PyObjectPayload::ExceptionInstance(ei) = &original.payload {
                            let mut w = ei.attrs.write();
                            w.insert(intern_or_new("__cause__"), PyObject::none());
                            w.insert(intern_or_new("__suppress_context__"), PyObject::bool_val(true));
                        }
                    }
                } else {
                    let cause_exc = raise_exc(&cause);
                    py_exc.ensure_original();
                    if let Some(ref original) = py_exc.original {
                        if let PyObjectPayload::ExceptionInstance(ei) = &original.payload {
                            let mut w = ei.attrs.write();
                            w.insert(intern_or_new("__cause__"), cause.clone());
                            w.insert(intern_or_new("__suppress_context__"), PyObject::bool_val(true));
                        }
                    }
                    py_exc.cause = Some(Box::new(cause_exc));
                }
                // Implicit chaining: set __context__ to active exception
                if let Some(active) = &self.active_exception {
                    py_exc.context = Some(Box::new(active.clone()));
                    if let Some(ref original) = py_exc.original {
                        if let PyObjectPayload::ExceptionInstance(ei) = &original.payload {
                            // Store __context__ as the active exception's original object
                            if let Some(ref ctx_orig) = active.original {
                                ei.attrs.write().insert(intern_or_new("__context__"), ctx_orig.clone());
                            }
                        }
                    }
                }
                return Err(py_exc);
            }
            _ => return Err(PyException::runtime_error("bad RAISE_VARARGS arg")),
        }
    }

    fn exec_setup_with(&mut self, arg: u32) -> Result<Option<PyObjectRef>, PyException> {
        let ctx_mgr = self.vm_pop();
        if let PyObjectPayload::Generator(gen_arc) = &ctx_mgr.payload {
            let enter_result = match self.resume_generator(gen_arc, PyObject::none()) {
                Ok(val) => val,
                Err(e) if e.kind == ExceptionKind::StopIteration => PyObject::none(),
                Err(e) => return Err(e),
            };
            let frame = self.vm_frame();
            frame.push(ctx_mgr.clone());
            frame.push_block(BlockKind::With, arg as usize);
            frame.push(enter_result);
        } else {
            let exit_raw = ctx_mgr.get_attr("__exit__").ok_or_else(||
                PyException::attribute_error("__exit__"))?;
            // Bind exit to ctx_mgr so WithCleanupStart passes self correctly
            let exit_method = if matches!(&exit_raw.payload, PyObjectPayload::BoundMethod { .. }) {
                exit_raw
            } else {
                PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: ctx_mgr.clone(),
                        method: exit_raw,
                    }
                })
            };
            self.vm_push(exit_method);
            let enter_raw = ctx_mgr.get_attr("__enter__").ok_or_else(||
                PyException::attribute_error("__enter__"))?;
            let (enter_method, enter_args) = if matches!(&enter_raw.payload, PyObjectPayload::BoundMethod { .. }) {
                (enter_raw, vec![])
            } else {
                let bound = PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: ctx_mgr.clone(),
                        method: enter_raw,
                    }
                });
                (bound, vec![])
            };
            let enter_result = self.call_object(enter_method, enter_args)?;

            // Handle redirect_stdout/redirect_stderr: swap sys.stdout/stderr
            if let PyObjectPayload::Instance(ref inst) = ctx_mgr.payload {
                let is_redirect_stdout = inst.attrs.read().contains_key("__redirect_stdout__");
                let is_redirect_stderr = inst.attrs.read().contains_key("__redirect_stderr__");
                let stream_name = if is_redirect_stdout { Some("stdout") }
                    else if is_redirect_stderr { Some("stderr") }
                    else { None };
                if let Some(sname) = stream_name {
                    // Save old stream
                    let old_stream = self.modules.get("sys")
                        .and_then(|s| s.get_attr(sname))
                        .unwrap_or_else(PyObject::none);
                    inst.attrs.write().insert(
                        CompactString::from("_old_target"), old_stream);
                    // Set new stream
                    let new_target = inst.attrs.read().get("_new_target").cloned()
                        .unwrap_or_else(PyObject::none);
                    if let Some(sys_mod) = self.modules.get("sys") {
                        if let PyObjectPayload::Module(md) = &sys_mod.payload {
                            md.attrs.write().insert(
                                CompactString::from(sname), new_target);
                        }
                    }
                }
            }

            let frame = self.vm_frame();
            frame.push_block(BlockKind::With, arg as usize);
            frame.push(enter_result);
        }
        Ok(None)
    }

    /// Restore sys.stdout or sys.stderr when exiting a redirect context manager.
    fn restore_redirect(&mut self, ctx_mgr: &PyObjectRef) {
        if let PyObjectPayload::Instance(inst) = &ctx_mgr.payload {
            let attrs = inst.attrs.read();
            let is_stdout = attrs.contains_key("__redirect_stdout__");
            let is_stderr = attrs.contains_key("__redirect_stderr__");
            let stream_name = if is_stdout { Some("stdout") }
                else if is_stderr { Some("stderr") }
                else { None };
            if let Some(sname) = stream_name {
                if let Some(old_target) = attrs.get("_old_target").cloned() {
                    drop(attrs);
                    if let Some(sys_mod) = self.modules.get("sys") {
                        if let PyObjectPayload::Module(md) = &sys_mod.payload {
                            md.attrs.write().insert(
                                CompactString::from(sname), old_target);
                        }
                    }
                }
            }
        }
    }
}
