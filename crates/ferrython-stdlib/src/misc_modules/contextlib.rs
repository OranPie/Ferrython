use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use indexmap::IndexMap;

// ── contextlib module ──

#[allow(dead_code)]
pub fn create_contextlib_module() -> PyObjectRef {
    // suppress(*exceptions) — context manager that suppresses specified exceptions
    let suppress_fn = make_builtin(|args: &[PyObjectRef]| {
        let exceptions: Vec<PyObjectRef> = args.to_vec();
        let suppress_cls =
            PyObject::class(CompactString::from("suppress"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("__suppress_exceptions__"),
            PyObject::list(exceptions),
        );
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_function("suppress.__enter__", |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                Ok(args[0].clone())
            }),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_function("suppress.__exit__", |args: &[PyObjectRef]| {
                // args: self, exc_type, exc_val, exc_tb
                if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
                    let exc_type = &args[1];
                    // Get the exception kind from the exception type
                    let exc_kind = match &exc_type.payload {
                        PyObjectPayload::ExceptionType(k) => Some(k.clone()),
                        _ => {
                            // Fall back to name-based lookup
                            let name = exc_type.py_to_string();
                            ferrython_core::error::ExceptionKind::from_name(
                                name.trim_start_matches("<class '").trim_end_matches("'>"),
                            )
                        }
                    };
                    if let Some(exc_kind) = exc_kind {
                        if let Some(suppressed) = args[0].get_attr("__suppress_exceptions__") {
                            if let Ok(exc_list) = suppressed.to_list() {
                                for allowed in &exc_list {
                                    let allowed_kind = match &allowed.payload {
                                        PyObjectPayload::ExceptionType(k) => Some(k.clone()),
                                        _ => {
                                            let name = allowed.py_to_string();
                                            ferrython_core::error::ExceptionKind::from_name(
                                                name.trim_start_matches("<class '")
                                                    .trim_end_matches("'>"),
                                            )
                                        }
                                    };
                                    if let Some(allowed_kind) = allowed_kind {
                                        if exc_kind.is_subclass_of(&allowed_kind) {
                                            return Ok(PyObject::bool_val(true));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(PyObject::bool_val(false))
            }),
        );
        Ok(PyObject::instance_with_attrs(suppress_cls, attrs))
    });

    // ExitStack — real context manager with callback registration
    let exit_stack_cls = {
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__exitstack__"),
            PyObject::bool_val(true),
        );
        PyObject::class(CompactString::from("ExitStack"), vec![], ns)
    };

    let exit_stack_cls_clone = exit_stack_cls.clone();
    let exit_stack_fn = PyObject::native_closure("ExitStack", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(exit_stack_cls_clone.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("_callbacks"), PyObject::list(vec![]));

            // ExitStack methods are stored as instance attrs. Instance attr lookup
            // does NOT bind self, so we capture inst via closure (NativeClosure).
            let self_ref = inst.clone();
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("ExitStack.__enter__", {
                    let self_ref = self_ref.clone();
                    move |_args: &[PyObjectRef]| Ok(self_ref.clone())
                }),
            );

            attrs.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("ExitStack.__exit__", {
                    let self_ref = self_ref.clone();
                    move |_args: &[PyObjectRef]| {
                        if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let Ok(items) = cbs.to_list() {
                                for cb in items.iter().rev() {
                                    match &cb.payload {
                                        PyObjectPayload::NativeFunction(nf) => {
                                            let _ = (nf.func)(&[]);
                                        }
                                        PyObjectPayload::NativeClosure(nc) => {
                                            let _ = (nc.func)(&[]);
                                        }
                                        PyObjectPayload::Function(_) => {
                                            // Python function callback — use request_vm_call
                                            ferrython_core::error::request_vm_call(
                                                cb.clone(),
                                                vec![],
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Ok(PyObject::bool_val(false))
                    }
                }),
            );

            attrs.insert(
                CompactString::from("push"),
                PyObject::native_closure("ExitStack.push", {
                    let self_ref = self_ref.clone();
                    move |args: &[PyObjectRef]| {
                        check_args_min("ExitStack.push", args, 1)?;
                        let callback = &args[0];
                        if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let PyObjectPayload::List(items) = &cbs.payload {
                                items.write().push(callback.clone());
                            }
                        }
                        Ok(callback.clone())
                    }
                }),
            );

            attrs.insert(
                CompactString::from("callback"),
                PyObject::native_closure("ExitStack.callback", {
                    let self_ref = self_ref.clone();
                    move |args: &[PyObjectRef]| {
                        check_args_min("ExitStack.callback", args, 1)?;
                        let func = args[0].clone();
                        let extra_args: Vec<PyObjectRef> = args[1..].to_vec();
                        // Wrap callback+args into a NativeClosure so __exit__ can call it
                        let wrapper = PyObject::native_closure(
                            "_callback_wrapper",
                            move |_: &[PyObjectRef]| match &func.payload {
                                PyObjectPayload::NativeFunction(nf) => (nf.func)(&extra_args),
                                PyObjectPayload::NativeClosure(nc) => (nc.func)(&extra_args),
                                PyObjectPayload::BoundMethod {
                                    method, receiver, ..
                                } => {
                                    let mut call_args = vec![(*receiver).clone()];
                                    call_args.extend(extra_args.iter().cloned());
                                    match &method.payload {
                                        PyObjectPayload::NativeFunction(nf) => {
                                            (nf.func)(&call_args)
                                        }
                                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args),
                                        _ => {
                                            ferrython_core::error::request_vm_call(
                                                (*method).clone(),
                                                call_args,
                                            );
                                            Ok(PyObject::none())
                                        }
                                    }
                                }
                                _ => {
                                    let call_args = extra_args.clone();
                                    ferrython_core::error::request_vm_call(func.clone(), call_args);
                                    Ok(PyObject::none())
                                }
                            },
                        );
                        if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let PyObjectPayload::List(items) = &cbs.payload {
                                items.write().push(wrapper);
                            }
                        }
                        Ok(PyObject::none())
                    }
                }),
            );

            attrs.insert(
                CompactString::from("enter_context"),
                PyObject::native_closure("ExitStack.enter_context", {
                    let self_ref = self_ref.clone();
                    move |args: &[PyObjectRef]| {
                        check_args_min("ExitStack.enter_context", args, 1)?;
                        let cm = &args[0];
                        // Call __enter__
                        let result = if let Some(enter) = cm.get_attr("__enter__") {
                            match &enter.payload {
                                PyObjectPayload::NativeFunction(nf) => (nf.func)(&[cm.clone()])?,
                                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[cm.clone()])?,
                                PyObjectPayload::BuiltinBoundMethod(_) => {
                                    // Generator __enter__/__exit__ — needs VM dispatch
                                    ferrython_core::error::request_vm_call(enter, vec![cm.clone()]);
                                    PyObject::none() // placeholder; VM will execute
                                }
                                _ => cm.clone(),
                            }
                        } else {
                            PyObject::none()
                        };
                        // Register __exit__ as callback
                        if let Some(exit_fn) = cm.get_attr("__exit__") {
                            if let Some(cbs) = self_ref.get_attr("_callbacks") {
                                if let PyObjectPayload::List(items) = &cbs.payload {
                                    items.write().push(exit_fn);
                                }
                            }
                        }
                        Ok(result)
                    }
                }),
            );

            // close() — immediately unwinds the callback stack
            attrs.insert(
                CompactString::from("close"),
                PyObject::native_closure("ExitStack.close", {
                    let self_ref = self_ref.clone();
                    move |_args: &[PyObjectRef]| {
                        // Invoke __exit__ with (None, None, None)
                        if let Some(exit_fn) = self_ref.get_attr("__exit__") {
                            match &exit_fn.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    let none = PyObject::none();
                                    (nc.func)(&[none.clone(), none.clone(), none])?;
                                }
                                _ => {}
                            }
                        }
                        Ok(PyObject::none())
                    }
                }),
            );

            // pop_all() — transfer callbacks to a new ExitStack, clearing this one
            attrs.insert(
                CompactString::from("pop_all"),
                PyObject::native_closure("ExitStack.pop_all", {
                    let self_ref = self_ref.clone();
                    let cls_for_pop = exit_stack_cls_clone.clone();
                    move |_args: &[PyObjectRef]| {
                        // Get current callbacks
                        let callbacks = if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let Ok(items) = cbs.to_list() {
                                items
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        };
                        // Clear our callbacks
                        if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let PyObjectPayload::List(items) = &cbs.payload {
                                items.write().clear();
                            }
                        }
                        // Create new ExitStack instance with the transferred callbacks
                        let new_inst = PyObject::instance(cls_for_pop.clone());
                        if let PyObjectPayload::Instance(ref inst_data) = new_inst.payload {
                            let mut new_attrs = inst_data.attrs.write();
                            new_attrs.insert(
                                CompactString::from("_callbacks"),
                                PyObject::list(callbacks),
                            );
                        }
                        Ok(new_inst)
                    }
                }),
            );
        }
        Ok(inst)
    });

    // nullcontext(enter_result=None) — no-op context manager
    let nullcontext_fn = make_builtin(|args: &[PyObjectRef]| {
        let enter_result = if !args.is_empty() {
            args[0].clone()
        } else {
            PyObject::none()
        };
        let cls = PyObject::class(CompactString::from("nullcontext"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let enter_val = enter_result.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("nullcontext.__enter__", move |_args: &[PyObjectRef]| {
                Ok(enter_val.clone())
            }),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_function("nullcontext.__exit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }),
        );
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // closing(thing) — context manager that calls thing.close() on exit
    // Uses __closing_thing__ marker so the VM can call close() through normal dispatch
    let closing_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("closing requires 1 argument"));
        }
        let thing = args[0].clone();
        let cls = PyObject::class(CompactString::from("closing"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let thing_enter = thing.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("closing.__enter__", move |_args: &[PyObjectRef]| {
                Ok(thing_enter.clone())
            }),
        );
        // __exit__ is a no-op; the VM handles calling close() via __closing_thing__ marker
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_function("closing.__exit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }),
        );
        attrs.insert(CompactString::from("__closing_thing__"), thing);
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // redirect_stdout(new_target) — context manager that swaps sys.stdout
    // Uses the global STDOUT_OVERRIDE stack so print() picks it up.
    let redirect_stdout_fn = make_builtin(|args: &[PyObjectRef]| {
        let target = if !args.is_empty() {
            args[0].clone()
        } else {
            PyObject::none()
        };
        let cls = PyObject::class(
            CompactString::from("redirect_stdout"),
            vec![],
            IndexMap::new(),
        );
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("__redirect_stdout__"),
            PyObject::bool_val(true),
        );
        attrs.insert(CompactString::from("_new_target"), target.clone());
        let inst = PyObject::instance_with_attrs(cls, attrs);
        if let PyObjectPayload::Instance(ref idata) = inst.payload {
            let t = target.clone();
            idata.attrs.write().insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("redirect_stdout.__enter__", move |_args| {
                    crate::push_stdout_override(t.clone());
                    Ok(t.clone())
                }),
            );
            idata.attrs.write().insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("redirect_stdout.__exit__", move |_args| {
                    crate::pop_stdout_override();
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    // redirect_stderr(new_target) — same pattern for stderr
    let redirect_stderr_fn = make_builtin(|args: &[PyObjectRef]| {
        let target = if !args.is_empty() {
            args[0].clone()
        } else {
            PyObject::none()
        };
        let cls = PyObject::class(
            CompactString::from("redirect_stderr"),
            vec![],
            IndexMap::new(),
        );
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("__redirect_stderr__"),
            PyObject::bool_val(true),
        );
        attrs.insert(CompactString::from("_new_target"), target.clone());
        let inst = PyObject::instance_with_attrs(cls, attrs);
        if let PyObjectPayload::Instance(ref idata) = inst.payload {
            let t = target.clone();
            idata.attrs.write().insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("redirect_stderr.__enter__", move |_args| {
                    crate::push_stderr_override(t.clone());
                    Ok(t.clone())
                }),
            );
            idata.attrs.write().insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("redirect_stderr.__exit__", move |_args| {
                    crate::pop_stderr_override();
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    // asynccontextmanager — same as contextmanager but for async generators
    let asynccontextmanager_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "asynccontextmanager requires 1 argument",
            ));
        }
        Ok(args[0].clone())
    });

    // AbstractContextManager — base class with __enter__ returning self
    let acm_cls = {
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__enter__"),
            PyObject::native_function(
                "AbstractContextManager.__enter__",
                |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    Ok(args[0].clone())
                },
            ),
        );
        ns.insert(
            CompactString::from("__exit__"),
            PyObject::native_function(
                "AbstractContextManager.__exit__",
                |_args: &[PyObjectRef]| Ok(PyObject::none()),
            ),
        );
        PyObject::class(CompactString::from("AbstractContextManager"), vec![], ns)
    };

    // AbstractAsyncContextManager
    let aacm_cls = {
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__aenter__"),
            PyObject::native_function(
                "AbstractAsyncContextManager.__aenter__",
                |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    Ok(args[0].clone())
                },
            ),
        );
        ns.insert(
            CompactString::from("__aexit__"),
            PyObject::native_function(
                "AbstractAsyncContextManager.__aexit__",
                |_args: &[PyObjectRef]| Ok(PyObject::none()),
            ),
        );
        PyObject::class(
            CompactString::from("AbstractAsyncContextManager"),
            vec![],
            ns,
        )
    };

    // AsyncExitStack — async version of ExitStack
    let async_exit_stack_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(
            CompactString::from("AsyncExitStack"),
            vec![],
            IndexMap::new(),
        );
        let mut attrs = IndexMap::new();
        let callbacks: PyObjectRef = PyObject::list(vec![]);
        let cb_ref = callbacks.clone();
        attrs.insert(CompactString::from("_callbacks"), callbacks.clone());
        attrs.insert(
            CompactString::from("__aenter__"),
            PyObject::native_closure("AsyncExitStack.__aenter__", {
                let inst_placeholder = PyObject::none();
                move |_args| Ok(inst_placeholder.clone())
            }),
        );
        let cb_exit = cb_ref.clone();
        attrs.insert(
            CompactString::from("__aexit__"),
            PyObject::native_closure("AsyncExitStack.__aexit__", move |_args| {
                // Pop and call all callbacks (simplified — sync-only for now)
                if let PyObjectPayload::List(list) = &cb_exit.payload {
                    let mut w = list.write();
                    while let Some(_cb) = w.pop() {
                        // Would need to await async callbacks
                    }
                }
                Ok(PyObject::bool_val(false))
            }),
        );
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // ContextDecorator: mixin class for context managers that can also be used as decorators
    let context_decorator_cls = PyObject::class(
        CompactString::from("ContextDecorator"),
        vec![],
        IndexMap::new(),
    );

    make_module(
        "contextlib",
        vec![
            ("contextmanager", make_builtin(contextlib_contextmanager)),
            ("asynccontextmanager", asynccontextmanager_fn),
            ("suppress", suppress_fn),
            ("closing", closing_fn),
            ("ExitStack", exit_stack_fn),
            ("AsyncExitStack", async_exit_stack_fn),
            ("nullcontext", nullcontext_fn),
            ("redirect_stdout", redirect_stdout_fn),
            ("redirect_stderr", redirect_stderr_fn),
            ("AbstractContextManager", acm_cls.clone()),
            ("AbstractAsyncContextManager", aacm_cls),
            ("ContextDecorator", context_decorator_cls),
        ],
    )
}

#[allow(dead_code)]
fn contextlib_contextmanager(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // contextmanager decorator — returns the function unchanged.
    // The function is a generator function. When called, it returns a Generator.
    // The VM's SetupWith handles Generator objects as context managers directly.
    if args.is_empty() {
        return Err(PyException::type_error(
            "contextmanager requires 1 argument",
        ));
    }
    Ok(args[0].clone())
}
