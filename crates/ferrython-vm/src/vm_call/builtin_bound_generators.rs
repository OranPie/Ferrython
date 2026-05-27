use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    AsyncGenAction, BuiltinBoundMethodData, PyObject, PyObjectPayload, PyObjectRef,
};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_generator_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        let gen_kind = match &bbm.receiver.payload {
            PyObjectPayload::Generator(g) => Some(("generator", g.clone())),
            PyObjectPayload::Coroutine(g) => Some(("coroutine", g.clone())),
            PyObjectPayload::AsyncGenerator(g) => Some(("async_generator", g.clone())),
            _ => None,
        };
        if let Some((kind, ref gen_arc)) = gen_kind {
            match bbm.method_name.as_str() {
                "send" => {
                    let val = if args.is_empty() {
                        PyObject::none()
                    } else {
                        args[0].clone()
                    };
                    return self.resume_generator(gen_arc, val).map(Some);
                }
                "throw" => {
                    let (exc_kind, msg) = Self::parse_throw_args(args);
                    let original_value = Self::parse_throw_original_value(args);
                    return self
                        .gen_throw_with_value(gen_arc, exc_kind, msg, original_value)
                        .map(Some);
                }
                "close" => {
                    let gen = gen_arc.read();
                    if gen.finished || !gen.has_frame() {
                        drop(gen);
                        return Ok(Some(PyObject::none()));
                    }
                    drop(gen);
                    match self.gen_throw(
                        gen_arc,
                        ExceptionKind::GeneratorExit,
                        CompactString::new(""),
                    ) {
                        Ok(_yielded) => {
                            return Err(PyException::runtime_error(
                                "generator ignored GeneratorExit",
                            ));
                        }
                        Err(e)
                            if e.kind == ExceptionKind::GeneratorExit
                                || e.kind == ExceptionKind::StopIteration =>
                        {
                            let mut gen = gen_arc.write();
                            gen.finished = true;
                            gen.clear_frame();
                            return Ok(Some(PyObject::none()));
                        }
                        Err(e) => {
                            let mut gen = gen_arc.write();
                            gen.finished = true;
                            gen.clear_frame();
                            return Err(e);
                        }
                    }
                }
                "__next__" if kind != "async_generator" => {
                    return self.resume_generator(gen_arc, PyObject::none()).map(Some);
                }
                "__enter__" if kind == "generator" => {
                    return self.resume_generator(gen_arc, PyObject::none()).map(Some);
                }
                "__exit__" if kind == "generator" => {
                    let has_exc =
                        !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None);
                    if has_exc {
                        let (exc_kind, msg) = Self::parse_throw_args(args);
                        let original_value = Self::parse_throw_original_value(args);
                        match self.gen_throw_with_value(gen_arc, exc_kind, msg, original_value) {
                            Ok(_) => return Ok(Some(PyObject::bool_val(true))),
                            Err(e) if e.kind == ExceptionKind::StopIteration => {
                                return Ok(Some(PyObject::bool_val(true)));
                            }
                            Err(e) => return Err(e),
                        }
                    } else {
                        match self.resume_generator(gen_arc, PyObject::none()) {
                            Ok(_) => {
                                return Err(PyException::runtime_error("generator didn't stop"));
                            }
                            Err(e) if e.kind == ExceptionKind::StopIteration => {
                                return Ok(Some(PyObject::bool_val(false)));
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }
                "__aiter__" if kind == "async_generator" => {
                    return Ok(Some(bbm.receiver.clone()));
                }
                "__anext__" if kind == "async_generator" => {
                    return Ok(Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::AsyncGenAwaitable {
                            gen: gen_arc.clone(),
                            action: Box::new(AsyncGenAction::Next),
                        },
                    })));
                }
                "asend" if kind == "async_generator" => {
                    let val = if args.is_empty() {
                        PyObject::none()
                    } else {
                        args[0].clone()
                    };
                    return Ok(Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::AsyncGenAwaitable {
                            gen: gen_arc.clone(),
                            action: Box::new(AsyncGenAction::Send(val)),
                        },
                    })));
                }
                "athrow" if kind == "async_generator" => {
                    let (exc_kind, msg) = Self::parse_throw_args(args);
                    return Ok(Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::AsyncGenAwaitable {
                            gen: gen_arc.clone(),
                            action: Box::new(AsyncGenAction::Throw(
                                exc_kind,
                                CompactString::from(msg),
                            )),
                        },
                    })));
                }
                "aclose" if kind == "async_generator" => {
                    return Ok(Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::AsyncGenAwaitable {
                            gen: gen_arc.clone(),
                            action: Box::new(AsyncGenAction::Close),
                        },
                    })));
                }
                _ => {}
            }
        }

        if let PyObjectPayload::AsyncGenAwaitable { gen, action } = &bbm.receiver.payload {
            match bbm.method_name.as_str() {
                "send" => {
                    let send_val = if args.is_empty() {
                        PyObject::none()
                    } else {
                        args[0].clone()
                    };
                    return self
                        .drive_async_gen_awaitable(gen, action, send_val)
                        .map(Some);
                }
                "throw" => {
                    let (exc_kind, msg) = Self::parse_throw_args(args);
                    let original_value = Self::parse_throw_original_value(args);
                    return self
                        .gen_throw_with_value(gen, exc_kind, msg, original_value)
                        .map(Some);
                }
                "close" => {
                    return Ok(Some(PyObject::none()));
                }
                _ => {}
            }
        }

        Ok(None)
    }
}
