//! Miscellaneous operations: format, annotations, generators, async iterators

use crate::builtins;
use crate::frame::BlockKind;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::opcode::Opcode;
use ferrython_bytecode::Instruction;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;
use std::sync::Arc;

// ── Misc ops ─────────────────────────────────────────────────────────
impl VirtualMachine {
    pub(crate) fn exec_misc_ops(&mut self, instr: Instruction) -> Result<Option<PyObjectRef>, PyException> {
        match instr.op {
            Opcode::PrintExpr => {
                let frame = self.vm_frame();
                let value = frame.pop();
                if !matches!(value.payload, PyObjectPayload::None) {
                    println!("{}", value.repr());
                }
            }
            Opcode::LoadBuildClass => {
                self.vm_frame().push(PyObject::builtin_function(
                    intern_or_new("__build_class__")));
            }
            Opcode::SetupAnnotations => {
                let frame = self.vm_frame();
                // In function scope, __annotations__ may be a fast local (varname).
                // Check if it's registered as a varname and use fast locals.
                let varname_idx = frame.code.varnames.iter().position(|v| v == "__annotations__");
                if let Some(idx) = varname_idx {
                    if idx < frame.locals.len() && frame.locals[idx].is_none() {
                        frame.locals[idx] = Some(PyObject::dict(IndexMap::new()));
                    }
                } else if !frame.local_names.contains_key("__annotations__") {
                    frame.store_name(
                        intern_or_new("__annotations__"),
                        PyObject::dict(IndexMap::new()),
                    );
                }
            }
            Opcode::FormatValue => {
                let frame = self.vm_frame();
                let fmt_spec = if instr.arg & 0x04 != 0 {
                    let spec_obj = frame.pop();
                    spec_obj.as_str().unwrap_or("").to_string()
                } else {
                    String::new()
                };
                let value = frame.pop();
                let conversion = (instr.arg & 0x03) as u8;
                let base_str = match conversion {
                    1 => {
                        // !s conversion — use VM-aware str for all types
                        self.vm_str(&value)?
                    }
                    2 => {
                        // !r conversion — use VM-aware repr for all types
                        self.vm_repr(&value)?
                    }
                    3 => {
                        // !a conversion — ascii repr
                        self.vm_repr(&value)?
                    }
                    _ => {
                        if !fmt_spec.is_empty() {
                            if matches!(&value.payload, PyObjectPayload::Instance(_)) {
                                if let Some(format_method) = value.get_attr("__format__") {
                                    let spec = PyObject::str_val(CompactString::from(&fmt_spec));
                                    let r = self.call_object(format_method, vec![spec])?;
                                    self.vm_push(PyObject::str_val(CompactString::from(r.py_to_string())));
                                    return Ok(None);
                                }
                            }
                            match value.format_value(&fmt_spec) {
                                Ok(s) => s,
                                Err(_) => value.py_to_string(),
                            }
                        } else {
                            if matches!(&value.payload, PyObjectPayload::Instance(_)) {
                                if let Some(format_method) = value.get_attr("__format__") {
                                    let spec = PyObject::str_val(CompactString::from(""));
                                    let r = self.call_object(format_method, vec![spec])?;
                                    self.vm_push(PyObject::str_val(CompactString::from(r.py_to_string())));
                                    return Ok(None);
                                }
                                if let Some(str_method) = value.get_attr("__str__") {
                                    let r = self.call_object(str_method, vec![])?;
                                    let s = r.py_to_string();
                                    self.vm_push(PyObject::str_val(CompactString::from(s)));
                                    return Ok(None);
                                }
                            }
                            // Use VM-aware str for containers (list/tuple/dict)
                            // so items with __repr__ get proper representation
                            self.vm_str(&value)?
                        }
                    }
                };
                let formatted = if !fmt_spec.is_empty() && conversion != 0 {
                    use ferrython_core::object::apply_string_format_spec;
                    apply_string_format_spec(&base_str, &fmt_spec)
                } else {
                    base_str
                };
                self.vm_push(PyObject::str_val(CompactString::from(formatted)));
            }
            Opcode::ExtendedArg => {}
            Opcode::YieldValue => {
                let frame = self.vm_frame();
                let value = frame.pop();
                frame.yielded = true;
                return Ok(Some(value));
            }
            Opcode::YieldFrom => {
                let send_val = self.vm_pop();
                let sub_iter = self.vm_frame().peek().clone();

                // Handle Generator, Coroutine, and AsyncGenerator using same resume mechanism
                let gen_arc_opt = match &sub_iter.payload {
                    PyObjectPayload::Generator(ref g) => Some(g.clone()),
                    PyObjectPayload::Coroutine(ref g) => Some(g.clone()),
                    PyObjectPayload::AsyncGenerator(ref g) => Some(g.clone()),
                    _ => None,
                };

                if let Some(gen_arc) = gen_arc_opt {
                    match self.resume_generator(&gen_arc, send_val) {
                        Ok(yielded) => {
                            let frame = self.vm_frame();
                            frame.yielded = true;
                            frame.ip -= 1;
                            return Ok(Some(yielded));
                        }
                        Err(e) if e.kind == ExceptionKind::StopIteration => {
                            let frame = self.vm_frame();
                            frame.pop();
                            // yield from captures StopIteration.value as the result
                            let return_val = e.value.unwrap_or_else(|| PyObject::none());
                            frame.push(return_val);
                        }
                        Err(e) => return Err(e),
                    }
                } else if let PyObjectPayload::AsyncGenAwaitable { gen, action } = &sub_iter.payload {
                    // Drive the async generator awaitable — this is what happens when
                    // `await ag.__anext__()` is compiled as GetAwaitable + YieldFrom.
                    match self.drive_async_gen_awaitable(gen, action, send_val) {
                        Ok(yielded) => {
                            // Intermediate yield — propagate up to the driving coroutine
                            let frame = self.vm_frame();
                            frame.yielded = true;
                            frame.ip -= 1;
                            return Ok(Some(yielded));
                        }
                        Err(e) if e.kind == ExceptionKind::StopIteration => {
                            let frame = self.vm_frame();
                            frame.pop();
                            let return_val = e.value.unwrap_or_else(|| PyObject::none());
                            frame.push(return_val);
                        }
                        Err(e) => return Err(e),
                    }
                } else if let PyObjectPayload::BuiltinAwaitable(inner_val) = &sub_iter.payload {
                    // BuiltinAwaitable: immediately resolve with the stored value.
                    // If the value is a list of coroutines (from asyncio.gather),
                    // drive each one and collect results.
                    let result = if let PyObjectPayload::List(items) = &inner_val.payload {
                        let items = items.read().clone();
                        let has_awaitable = items.iter().any(|item| {
                            matches!(&item.payload, PyObjectPayload::Coroutine(_))
                            || item.get_attr("_coro").is_some()  // Task objects
                        });
                        if has_awaitable {
                            // asyncio.gather pattern: drive each coroutine/task
                            let mut results = Vec::with_capacity(items.len());
                            for item in &items {
                                // Unwrap Task → coroutine
                                let coro = if let Some(c) = item.get_attr("_coro") {
                                    c
                                } else {
                                    item.clone()
                                };
                                let r = self.maybe_await_result(coro)?;
                                results.push(r);
                            }
                            PyObject::list(results)
                        } else {
                            inner_val.clone()
                        }
                    } else {
                        inner_val.clone()
                    };
                    let frame = self.vm_frame();
                    frame.pop();
                    frame.push(result);
                } else if let PyObjectPayload::DeferredSleep { secs, result } = &sub_iter.payload {
                    // DeferredSleep: perform the actual sleep here (lazy),
                    // respecting any wait_for deadline.
                    let secs = *secs;
                    let result = result.clone();
                    let deadline = ferrython_async::get_wait_for_deadline();
                    if let Some(dl) = deadline {
                        let now = std::time::Instant::now();
                        if now >= dl {
                            ferrython_async::set_wait_for_deadline(None);
                            return Err(PyException::new(ExceptionKind::TimeoutError, ""));
                        }
                        let remaining = dl.duration_since(now).as_secs_f64();
                        if secs > remaining {
                            // Sleep would exceed deadline — sleep remaining, then timeout
                            std::thread::sleep(std::time::Duration::from_secs_f64(remaining));
                            ferrython_async::set_wait_for_deadline(None);
                            return Err(PyException::new(ExceptionKind::TimeoutError, ""));
                        }
                        std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                    } else {
                        std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                    }
                    let frame = self.vm_frame();
                    frame.pop();
                    frame.push(result);
                } else if matches!(&sub_iter.payload, PyObjectPayload::Instance(_)) {
                    if let Some(next_method) = sub_iter.get_attr("__next__") {
                        match self.call_object(next_method, vec![]) {
                            Ok(val) => {
                                let frame = self.vm_frame();
                                frame.yielded = true;
                                frame.ip -= 1;
                                return Ok(Some(val));
                            }
                            Err(e) if e.kind == ExceptionKind::StopIteration => {
                                let frame = self.vm_frame();
                                frame.pop();
                                frame.push(PyObject::none());
                            }
                            Err(e) => return Err(e),
                        }
                    } else {
                        let frame = self.vm_frame();
                        frame.pop();
                        frame.push(PyObject::none());
                    }
                } else {
                    let frame = self.vm_frame();
                    match builtins::iter_advance(&sub_iter)? {
                        Some((new_iter, value)) => {
                            frame.pop();
                            frame.push(new_iter);
                            frame.yielded = true;
                            frame.ip -= 1;
                            return Ok(Some(value));
                        }
                        None => {
                            frame.pop();
                            frame.push(PyObject::none());
                        }
                    }
                }
            }
            // ── Async opcodes ──

            Opcode::GetAwaitable => {
                // TOS is a coroutine or object with __await__. Push the awaitable iterator.
                let obj = self.vm_pop();
                match &obj.payload {
                    // Coroutine is already awaitable — push it directly
                    PyObjectPayload::Coroutine(_) => {
                        self.vm_push(obj);
                    }
                    // AsyncGenAwaitable (from __anext__, asend, athrow, aclose) is awaitable
                    PyObjectPayload::AsyncGenAwaitable { .. } => {
                        self.vm_push(obj);
                    }
                    // Generator marked as iterable_coroutine (types.coroutine)
                    PyObjectPayload::Generator(_) => {
                        self.vm_push(obj);
                    }
                    // BuiltinAwaitable — native awaitable from asyncio.sleep(), gather(), etc.
                    PyObjectPayload::BuiltinAwaitable(_) => {
                        self.vm_push(obj);
                    }
                    // DeferredSleep — deferred sleep from asyncio.sleep()
                    PyObjectPayload::DeferredSleep { .. } => {
                        self.vm_push(obj);
                    }
                    _ => {
                        // Try __await__() protocol — returns an iterator
                        if let Some(await_method) = obj.get_attr("__await__") {
                            let iter = self.call_object(await_method, vec![])?;
                            self.vm_push(iter);
                        } else {
                            return Err(PyException::type_error(format!(
                                "object {} can't be used in 'await' expression",
                                obj.type_name()
                            )));
                        }
                    }
                }
            }

            Opcode::GetAiter => {
                // TOS = async iterable. Call __aiter__() and push result.
                let obj = self.vm_pop();
                if let Some(aiter_method) = obj.get_attr("__aiter__") {
                    let aiter = self.call_object(aiter_method, vec![])?;
                    self.vm_push(aiter);
                } else {
                    return Err(PyException::type_error(format!(
                        "'{}' object is not an async iterable",
                        obj.type_name()
                    )));
                }
            }

            Opcode::GetAnext => {
                // TOS = async iterator. Call __anext__() which returns an awaitable.
                let aiter = self.vm_frame().peek().clone();
                if let Some(anext_method) = aiter.get_attr("__anext__") {
                    let awaitable = self.call_object(anext_method, vec![])?;
                    self.vm_push(awaitable);
                } else {
                    return Err(PyException::type_error(format!(
                        "'{}' object is not an async iterator",
                        aiter.type_name()
                    )));
                }
            }

            Opcode::BeforeAsyncWith => {
                // TOS = async context manager. Call __aenter__() → push awaitable result.
                // Keep ctx_mgr on stack (peek) — SetupAsyncWith will pop it later.
                let ctx_mgr = self.vm_frame().peek().clone();
                // Handle AsyncGenerator directly (from @asynccontextmanager)
                if let PyObjectPayload::AsyncGenerator(gen_arc) = &ctx_mgr.payload {
                    let enter_result = match self.resume_generator(gen_arc, PyObject::none()) {
                        Ok(val) => val,
                        Err(e) if e.kind == ExceptionKind::StopAsyncIteration
                              || e.kind == ExceptionKind::StopIteration => PyObject::none(),
                        Err(e) => return Err(e),
                    };
                    // Wrap in BuiltinAwaitable so GET_AWAITABLE + YIELD_FROM resolve it
                    self.vm_push(PyObject::builtin_awaitable(enter_result));
                } else {
                    let aenter_raw = ctx_mgr.get_attr("__aenter__").ok_or_else(||
                        PyException::type_error(format!(
                            "'{}' object does not support the async context manager protocol",
                            ctx_mgr.type_name()
                        )))?;
                    let (aenter_method, aenter_args) = if matches!(&aenter_raw.payload, PyObjectPayload::BoundMethod { .. }) {
                        (aenter_raw, vec![])
                    } else {
                        let bound = Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: ctx_mgr.clone(),
                                method: aenter_raw,
                            }
                        });
                        (bound, vec![])
                    };
                    let result = self.call_object(aenter_method, aenter_args)?;
                    self.vm_push(result);
                }
            }

            Opcode::EndAsyncFor => {
                // End of async for — check if exception is StopAsyncIteration.
                // VM pushes (traceback, value, type) at except handler.
                // Stack: [... aiter, traceback, value, type] → [...]
                let exc_type = self.vm_pop();   // type (TOS)
                let exc_value = self.vm_pop();  // value
                let _traceback = self.vm_pop(); // traceback
                let _aiter = self.vm_pop();     // async iterator
                // Check type first, then value for StopAsyncIteration
                let is_stop_async = match &exc_type.payload {
                    PyObjectPayload::ExceptionType(k) => *k == ExceptionKind::StopAsyncIteration,
                    PyObjectPayload::ExceptionInstance { kind, .. } => *kind == ExceptionKind::StopAsyncIteration,
                    _ => match &exc_value.payload {
                        PyObjectPayload::ExceptionType(k) => *k == ExceptionKind::StopAsyncIteration,
                        PyObjectPayload::ExceptionInstance { kind, .. } => *kind == ExceptionKind::StopAsyncIteration,
                        _ => false,
                    },
                };
                if !is_stop_async {
                    if let Some(ref active) = self.active_exception {
                        if active.kind != ExceptionKind::StopAsyncIteration {
                            let e = active.clone();
                            self.active_exception = None;
                            return Err(e);
                        }
                    }
                }
                // Also pop the ExceptHandler block that was pushed by unwind_except
                let frame = self.vm_frame();
                if let Some(block) = frame.block_stack.last() {
                    if matches!(block.kind, BlockKind::ExceptHandler) {
                        frame.pop_block();
                    }
                }
                self.active_exception = None;
            }
            _ => unreachable!(),
        }
        Ok(None)
    }
}
