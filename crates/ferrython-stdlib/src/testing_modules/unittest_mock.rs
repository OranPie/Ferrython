use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{
    call_callable, make_builtin, make_module, new_fx_hashkey_map, FxHashKeyMap, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

// ── unittest.mock module ──

/// Create a Mock/MagicMock instance with proper dynamic attribute access,
/// return_value support, call tracking, and assertion methods.
///
/// Design: return_value is stored directly in the instance dict so that
/// `mock.return_value = X` (a normal STORE_ATTR) updates it in-place.
/// The __call__ closure reads from the instance's attrs dict at call time
/// via a shared Rc<PyCell<IndexMap>> reference to the instance data.
fn build_mock_instance(name: &str, kwargs: &FxHashKeyMap) -> PyObjectRef {
    let mut class_namespace = IndexMap::new();
    class_namespace.insert(
        CompactString::from("__mul__"),
        PyObject::native_closure("Mock.__mul__", |args: &[PyObjectRef]| {
            call_mock_magic(args, "__mul__", 1)
        }),
    );
    class_namespace.insert(
        CompactString::from("__rmul__"),
        PyObject::native_closure("Mock.__rmul__", |args: &[PyObjectRef]| {
            call_mock_magic(args, "__rmul__", 1)
        }),
    );
    class_namespace.insert(
        CompactString::from("__hash__"),
        PyObject::native_closure("Mock.__hash__", |args: &[PyObjectRef]| {
            call_mock_magic(args, "__hash__", 0)
        }),
    );
    let cls = PyObject::class(CompactString::from(name), vec![], class_namespace);
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        let attrs_ref = d.attrs.clone(); // shared ref for closures to read live attrs

        // Shared mutable state via Arc
        let call_count: Rc<PyCell<i64>> = Rc::new(PyCell::new(0));
        let call_args_list: Rc<PyCell<Vec<PyObjectRef>>> = Rc::new(PyCell::new(vec![]));
        let children: Rc<PyCell<IndexMap<String, PyObjectRef>>> =
            Rc::new(PyCell::new(IndexMap::new()));
        let mock_name = CompactString::from(name);

        // Store return_value directly as a plain value (not a closure) so STORE_ATTR overwrites it
        let init_rv = kwargs
            .get(&HashableKey::str_key(CompactString::from("return_value")))
            .cloned()
            .unwrap_or_else(PyObject::none);
        w.insert(CompactString::from("return_value"), init_rv);

        // Store side_effect if provided
        let init_se = kwargs
            .get(&HashableKey::str_key(CompactString::from("side_effect")))
            .cloned()
            .unwrap_or_else(PyObject::none);
        w.insert(CompactString::from("side_effect"), init_se);

        // __call__ — tracks calls, checks side_effect, reads return_value from live instance attrs
        let cc3 = call_count.clone();
        let cal2 = call_args_list.clone();
        let attrs_call = attrs_ref.clone();
        w.insert(
            CompactString::from("__call__"),
            PyObject::native_closure("Mock.__call__", move |args: &[PyObjectRef]| {
                *cc3.write() += 1;
                cal2.write().push(PyObject::tuple(args.to_vec()));

                // Check side_effect first
                let se = attrs_call.read().get("side_effect").cloned();
                if let Some(ref effect) = se {
                    if !matches!(effect.payload, PyObjectPayload::None) {
                        // If it's an exception instance, raise it
                        if let Some(exc_type) = effect.get_attr("__class__") {
                            let type_name = exc_type
                                .get_attr("__name__")
                                .map(|n| n.py_to_string())
                                .unwrap_or_default();
                            // Check if it's an exception type/instance
                            if type_name.ends_with("Error")
                                || type_name.ends_with("Exception")
                                || type_name == "KeyboardInterrupt"
                                || type_name == "SystemExit"
                                || type_name == "StopIteration"
                                || type_name == "GeneratorExit"
                            {
                                let msg = effect
                                    .get_attr("args")
                                    .and_then(|a| a.get_item(&PyObject::int(0)).ok())
                                    .map(|s| s.py_to_string())
                                    .unwrap_or_default();
                                let kind = match type_name.as_str() {
                                    "ValueError" => ExceptionKind::ValueError,
                                    "TypeError" => ExceptionKind::TypeError,
                                    "KeyError" => ExceptionKind::KeyError,
                                    "IndexError" => ExceptionKind::IndexError,
                                    "AttributeError" => ExceptionKind::AttributeError,
                                    "RuntimeError" => ExceptionKind::RuntimeError,
                                    "OSError" | "IOError" => ExceptionKind::OSError,
                                    "FileNotFoundError" => ExceptionKind::FileNotFoundError,
                                    "PermissionError" => ExceptionKind::PermissionError,
                                    "NotImplementedError" => ExceptionKind::NotImplementedError,
                                    "StopIteration" => ExceptionKind::StopIteration,
                                    "AssertionError" => ExceptionKind::AssertionError,
                                    "ImportError" => ExceptionKind::ImportError,
                                    "NameError" => ExceptionKind::NameError,
                                    _ => ExceptionKind::RuntimeError,
                                };
                                return Err(PyException::new(kind, msg));
                            }
                        }
                        // If it's a callable, call it
                        // (handled at VM level if it's a Function)
                    }
                }

                // Read return_value from live instance attrs (may have been updated via STORE_ATTR)
                let rv = attrs_call
                    .read()
                    .get("return_value")
                    .cloned()
                    .unwrap_or_else(PyObject::none);
                Ok(rv)
            }),
        );

        // __getattr__ — create child mocks for unknown attributes, route properties
        let children2 = children.clone();
        let mn = mock_name.clone();
        let cc_ga = call_count.clone();
        let cal_ga = call_args_list.clone();
        w.insert(
            CompactString::from("__getattr__"),
            PyObject::native_closure("Mock.__getattr__", move |args: &[PyObjectRef]| {
                let attr_name = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    return Ok(PyObject::none());
                };
                // Don't intercept dunder methods
                if attr_name.starts_with("__") && attr_name.ends_with("__") {
                    return Err(PyException::attribute_error(format!(
                        "'{}' object has no attribute '{}'",
                        mn, attr_name
                    )));
                }
                // Route mock-specific dynamic properties
                match attr_name.as_str() {
                    "call_count" => return Ok(PyObject::int(*cc_ga.read())),
                    "call_args_list" => return Ok(PyObject::list(cal_ga.read().clone())),
                    "called" => return Ok(PyObject::bool_val(*cc_ga.read() > 0)),
                    _ => {}
                }
                let mut cache = children2.write();
                if let Some(child) = cache.get(&attr_name) {
                    return Ok(child.clone());
                }
                // Create new child mock
                let child = build_mock_instance("MagicMock", &new_fx_hashkey_map());
                cache.insert(attr_name, child.clone());
                Ok(child)
            }),
        );

        // assert_called()
        let cc_ac = call_count.clone();
        w.insert(
            CompactString::from("assert_called"),
            PyObject::native_closure("Mock.assert_called", move |_: &[PyObjectRef]| {
                if *cc_ac.read() == 0 {
                    return Err(PyException::assertion_error(
                        "Expected mock to have been called.",
                    ));
                }
                Ok(PyObject::none())
            }),
        );

        // assert_called_once()
        let cc_aco = call_count.clone();
        w.insert(
            CompactString::from("assert_called_once"),
            PyObject::native_closure("Mock.assert_called_once", move |_: &[PyObjectRef]| {
                let count = *cc_aco.read();
                if count != 1 {
                    return Err(PyException::assertion_error(format!(
                        "Expected mock to have been called once. Called {} times.",
                        count
                    )));
                }
                Ok(PyObject::none())
            }),
        );

        // assert_called_with()
        let cal_acw = call_args_list.clone();
        w.insert(
            CompactString::from("assert_called_with"),
            PyObject::native_closure("Mock.assert_called_with", move |args: &[PyObjectRef]| {
                let history = cal_acw.read();
                if history.is_empty() {
                    return Err(PyException::assertion_error(
                        "Expected mock to have been called.",
                    ));
                }
                let last_call = history.last().unwrap();
                let expected = PyObject::tuple(args.to_vec());
                if last_call.py_to_string() != expected.py_to_string() {
                    return Err(PyException::assertion_error(format!(
                        "expected call: mock{}\nActual call: mock{}",
                        expected.py_to_string(),
                        last_call.py_to_string()
                    )));
                }
                Ok(PyObject::none())
            }),
        );

        // assert_not_called()
        let cc_anc = call_count.clone();
        w.insert(
            CompactString::from("assert_not_called"),
            PyObject::native_closure("Mock.assert_not_called", move |_: &[PyObjectRef]| {
                let count = *cc_anc.read();
                if count > 0 {
                    return Err(PyException::assertion_error(format!(
                        "Expected mock to not have been called. Called {} times.",
                        count
                    )));
                }
                Ok(PyObject::none())
            }),
        );

        // reset_mock()
        let cc_rm = call_count.clone();
        let cal_rm = call_args_list.clone();
        let ch_rm = children.clone();
        let attrs_rm = attrs_ref.clone();
        w.insert(
            CompactString::from("reset_mock"),
            PyObject::native_closure("Mock.reset_mock", move |_: &[PyObjectRef]| {
                *cc_rm.write() = 0;
                cal_rm.write().clear();
                attrs_rm
                    .write()
                    .insert(CompactString::from("return_value"), PyObject::none());
                ch_rm.write().clear();
                Ok(PyObject::none())
            }),
        );

        // MagicMock gets default magic methods
        if name == "MagicMock" {
            w.insert(
                CompactString::from("__len__"),
                PyObject::native_closure("__len__", |_| Ok(PyObject::int(0))),
            );
            w.insert(
                CompactString::from("__bool__"),
                PyObject::native_closure("__bool__", |_| Ok(PyObject::bool_val(true))),
            );
            w.insert(
                CompactString::from("__iter__"),
                PyObject::native_closure("__iter__", |_| {
                    Ok(PyObject::list(vec![])
                        .get_iter()
                        .unwrap_or_else(|_| PyObject::none()))
                }),
            );
            w.insert(
                CompactString::from("__contains__"),
                PyObject::native_closure("__contains__", |_| Ok(PyObject::bool_val(false))),
            );
            w.insert(
                CompactString::from("__int__"),
                PyObject::native_closure("__int__", |_| Ok(PyObject::int(1))),
            );
            w.insert(
                CompactString::from("__float__"),
                PyObject::native_closure("__float__", |_| Ok(PyObject::float(1.0))),
            );
            w.insert(
                CompactString::from("__str__"),
                PyObject::native_closure("__str__", |_| {
                    Ok(PyObject::str_val(CompactString::from("MagicMock")))
                }),
            );
            w.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("__repr__", |_| {
                    Ok(PyObject::str_val(CompactString::from("<MagicMock>")))
                }),
            );
            w.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", |args: &[PyObjectRef]| {
                    Ok(if !args.is_empty() {
                        args[0].clone()
                    } else {
                        PyObject::none()
                    })
                }),
            );
            w.insert(
                CompactString::from("__exit__"),
                make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
            );
        }
    }
    inst
}

fn call_mock_magic(
    args: &[PyObjectRef],
    method_name: &str,
    expected_operands: usize,
) -> ferrython_core::error::PyResult<PyObjectRef> {
    if args.len() < 1 + expected_operands {
        return Ok(PyObject::not_implemented());
    }
    let PyObjectPayload::Instance(inst) = &args[0].payload else {
        return Ok(PyObject::not_implemented());
    };
    let Some(method) = inst.attrs.read().get(method_name).cloned() else {
        return Ok(PyObject::not_implemented());
    };
    call_callable(&method, &args[1..1 + expected_operands])
}

/// Extract kwargs dict from trailing argument (VM passes kwargs as last Dict arg)
fn extract_mock_kwargs(args: &[PyObjectRef]) -> FxHashKeyMap {
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw_map) = &last.payload {
            return kw_map.read().clone();
        }
    }
    new_fx_hashkey_map()
}

fn strip_trailing_kwargs(args: &[PyObjectRef]) -> &[PyObjectRef] {
    if args
        .last()
        .is_some_and(|last| matches!(&last.payload, PyObjectPayload::Dict(_)))
    {
        &args[..args.len() - 1]
    } else {
        args
    }
}

fn set_mock_target_attr(target: &PyObjectRef, attr: &str, value: PyObjectRef) {
    match &target.payload {
        PyObjectPayload::Module(md) => {
            md.attrs.write().insert(CompactString::from(attr), value);
            ferrython_core::object::invalidate_global_lookups();
        }
        PyObjectPayload::Instance(inst) => {
            inst.attrs.write().insert(CompactString::from(attr), value);
        }
        PyObjectPayload::Class(cd) => {
            cd.namespace
                .write()
                .insert(CompactString::from(attr), value);
            cd.invalidate_cache();
        }
        _ => {}
    }
}

fn delete_mock_target_attr(target: &PyObjectRef, attr: &str) {
    match &target.payload {
        PyObjectPayload::Module(md) => {
            md.attrs.write().shift_remove(attr);
            ferrython_core::object::invalidate_global_lookups();
        }
        PyObjectPayload::Instance(inst) => {
            inst.attrs.write().shift_remove(attr);
        }
        PyObjectPayload::Class(cd) => {
            cd.namespace.write().shift_remove(attr);
            cd.invalidate_cache();
        }
        _ => {}
    }
}

type SavedPatch = (PyObjectRef, String, Option<PyObjectRef>);

fn build_patch_context(
    name: &str,
    replacements: Vec<(PyObjectRef, String, PyObjectRef)>,
    enter_value: PyObjectRef,
) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from(name), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let saved: Rc<PyCell<Vec<SavedPatch>>> = Rc::new(PyCell::new(Vec::new()));
        let saved_enter = saved.clone();
        let saved_exit = saved;
        let replacements_enter = replacements.clone();
        let replacements_exit = replacements;
        let enter_return = enter_value.clone();
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("patch.__enter__", move |_: &[PyObjectRef]| {
                let mut saved_values = Vec::new();
                for (target, attr, replacement) in &replacements_enter {
                    let old = target.get_attr(attr);
                    set_mock_target_attr(target, attr, replacement.clone());
                    saved_values.push((target.clone(), attr.clone(), old));
                }
                *saved_enter.write() = saved_values;
                Ok(enter_return.clone())
            }),
        );
        w.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("patch.__exit__", move |_: &[PyObjectRef]| {
                let mut saved_values = saved_exit.write();
                if saved_values.is_empty() {
                    for (target, attr, _) in &replacements_exit {
                        delete_mock_target_attr(target, attr);
                    }
                } else {
                    for (target, attr, old) in saved_values.drain(..).rev() {
                        if let Some(old_val) = old {
                            set_mock_target_attr(&target, &attr, old_val);
                        } else {
                            delete_mock_target_attr(&target, &attr);
                        }
                    }
                }
                Ok(PyObject::bool_val(false))
            }),
        );
    }
    inst
}

fn sys_modules_lookup(name: &str) -> Option<PyObjectRef> {
    let sys = crate::get_current_sys_module()?;
    let modules = sys.get_attr("modules")?;
    let PyObjectPayload::Dict(map) = &modules.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(name)))
        .cloned()
        .filter(|obj| !matches!(&obj.payload, PyObjectPayload::None))
}

fn sys_modules_insert(name: &str, module: PyObjectRef) {
    let Some(sys) = crate::get_current_sys_module() else {
        return;
    };
    let Some(modules) = sys.get_attr("modules") else {
        return;
    };
    let PyObjectPayload::Dict(map) = &modules.payload else {
        return;
    };
    map.write()
        .insert(HashableKey::str_key(CompactString::from(name)), module);
}

fn resolve_patch_target(path: &str) -> Option<(PyObjectRef, String)> {
    let (module_name, attr) = path.rsplit_once('.')?;
    if let Some(globals) = crate::get_current_globals() {
        let globals_r = globals.read();
        let mut parts = module_name.split('.');
        if let Some(first) = parts.next() {
            if let Some(mut obj) = globals_r.get(first).cloned() {
                for part in parts {
                    obj = obj.get_attr(part)?;
                }
                return Some((obj, attr.to_string()));
            }
        }
    }
    if module_name == "builtins" {
        if let Some(globals) = crate::get_current_globals() {
            if let Some(module) = globals.read().get("__builtins__").cloned() {
                return Some((module, attr.to_string()));
            }
        }
    }
    if let Some(module) = sys_modules_lookup(module_name) {
        return Some((module, attr.to_string()));
    }
    crate::load_module(module_name).map(|module| {
        sys_modules_insert(module_name, module.clone());
        (module, attr.to_string())
    })
}

pub fn create_unittest_mock_module() -> PyObjectRef {
    let make_mock = |name: &'static str| -> PyObjectRef {
        PyObject::native_closure(name, move |args: &[PyObjectRef]| {
            let kwargs = extract_mock_kwargs(args);
            Ok(build_mock_instance(name, &kwargs))
        })
    };

    // patch function — context manager that temporarily replaces a target attribute
    let patch_fn = make_builtin(|args: &[PyObjectRef]| {
        let target = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            String::new()
        };
        let kwargs = extract_mock_kwargs(args);
        let pos_args = strip_trailing_kwargs(args);
        let cls = PyObject::class(CompactString::from("_patch"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("attribute"),
                PyObject::str_val(CompactString::from(target.as_str())),
            );
            let mock_for_enter = build_mock_instance("MagicMock", &kwargs);
            let replacement = if pos_args.len() >= 2 {
                pos_args[1].clone()
            } else {
                mock_for_enter.clone()
            };
            let target_path = target.clone();
            let repl_enter = replacement.clone();
            let saved: Rc<PyCell<Option<(PyObjectRef, String, Option<PyObjectRef>)>>> =
                Rc::new(PyCell::new(None));
            let saved_for_exit = saved.clone();
            w.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("patch.__enter__", move |_: &[PyObjectRef]| {
                    if let Some((target_obj, attr_name)) = resolve_patch_target(&target_path) {
                        let old = target_obj.get_attr(&attr_name);
                        set_mock_target_attr(&target_obj, &attr_name, repl_enter.clone());
                        *saved.write() = Some((target_obj, attr_name, old));
                    }
                    Ok(repl_enter.clone())
                }),
            );
            w.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("patch.__exit__", move |_: &[PyObjectRef]| {
                    if let Some((target_obj, attr_name, old)) = saved_for_exit.write().take() {
                        if let Some(old_val) = old {
                            set_mock_target_attr(&target_obj, &attr_name, old_val);
                        } else {
                            delete_mock_target_attr(&target_obj, &attr_name);
                        }
                    }
                    Ok(PyObject::bool_val(false))
                }),
            );
            // As decorator: patch(target)(func) → wrapped func
            let mock_for_deco = mock_for_enter;
            w.insert(
                CompactString::from("__call__"),
                PyObject::native_closure("patch.__call__", move |args: &[PyObjectRef]| {
                    if !args.is_empty() {
                        // Decorator mode: return the function unchanged (mock passed as extra arg)
                        Ok(args[0].clone())
                    } else {
                        Ok(mock_for_deco.clone())
                    }
                }),
            );
        }
        Ok(inst)
    });

    // sentinel — attribute access returns unique sentinels
    let sentinel_cls = PyObject::class(CompactString::from("_Sentinel"), vec![], IndexMap::new());
    let sentinel = PyObject::instance(sentinel_cls);
    if let PyObjectPayload::Instance(ref d) = sentinel.payload {
        let sentinel_cache: Rc<PyCell<IndexMap<String, PyObjectRef>>> =
            Rc::new(PyCell::new(IndexMap::new()));
        let sc = sentinel_cache;
        d.attrs.write().insert(
            CompactString::from("__getattr__"),
            PyObject::native_closure("_Sentinel.__getattr__", move |args: &[PyObjectRef]| {
                let name = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    return Ok(PyObject::none());
                };
                if name.starts_with("__") && name.ends_with("__") {
                    return Err(PyException::attribute_error(format!(
                        "_Sentinel has no attribute '{}'",
                        name
                    )));
                }
                let mut cache = sc.write();
                if let Some(obj) = cache.get(&name) {
                    return Ok(obj.clone());
                }
                let cls = PyObject::class(
                    CompactString::from("_SentinelObject"),
                    vec![],
                    IndexMap::new(),
                );
                let obj = PyObject::instance(cls);
                if let PyObjectPayload::Instance(ref d) = obj.payload {
                    let n = name.clone();
                    d.attrs.write().insert(
                        CompactString::from("name"),
                        PyObject::str_val(CompactString::from(n.as_str())),
                    );
                    let n2 = name.clone();
                    d.attrs.write().insert(
                        CompactString::from("__repr__"),
                        PyObject::native_closure("__repr__", move |_| {
                            Ok(PyObject::str_val(CompactString::from(format!(
                                "sentinel.{}",
                                n2
                            ))))
                        }),
                    );
                }
                cache.insert(name, obj.clone());
                Ok(obj)
            }),
        );
    }

    // call — call record
    let call_fn = make_builtin(|args: &[PyObjectRef]| Ok(PyObject::tuple(args.to_vec())));

    // ANY — matches anything
    let any_cls = PyObject::class(CompactString::from("_ANY"), vec![], IndexMap::new());
    let any_obj = PyObject::instance(any_cls);
    if let PyObjectPayload::Instance(ref d) = any_obj.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__eq__"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(true))),
        );
        w.insert(
            CompactString::from("__ne__"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
        );
        w.insert(
            CompactString::from("__repr__"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::str_val(CompactString::from("ANY")))),
        );
    }

    // patch.object(target, attribute, new=DEFAULT, **kwargs) — context manager
    let patch_object_fn = make_builtin(|args: &[PyObjectRef]| {
        // args: target_obj, attribute_name, [new], **kwargs
        if args.len() < 2 {
            return Err(PyException::type_error(
                "patch.object requires at least 2 arguments".to_string(),
            ));
        }
        let target = args[0].clone();
        let attr_name = args[1].py_to_string();
        let kwargs = extract_mock_kwargs(&args[2..]);
        let pos_args = strip_trailing_kwargs(args);
        let rv_key = HashableKey::str_key(CompactString::from("return_value"));
        // Build replacement value
        let replacement = if let Some(_rv) = kwargs.get(&rv_key) {
            build_mock_instance("MagicMock", &kwargs)
        } else if pos_args.len() >= 3 {
            pos_args[2].clone()
        } else {
            build_mock_instance("MagicMock", &kwargs)
        };

        let cls = PyObject::class(
            CompactString::from("_patch_object"),
            vec![],
            IndexMap::new(),
        );
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let target_enter = target.clone();
            let attr_enter = attr_name.clone();
            let repl_enter = replacement.clone();
            let saved: Rc<PyCell<Option<PyObjectRef>>> = Rc::new(PyCell::new(None));
            let saved_for_exit = saved.clone();
            let target_exit = target.clone();
            let attr_exit = attr_name.clone();

            w.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("patch.object.__enter__", move |_: &[PyObjectRef]| {
                    // Save old value
                    let old = target_enter.get_attr(&attr_enter);
                    *saved.write() = old;
                    // Set new value
                    set_mock_target_attr(&target_enter, &attr_enter, repl_enter.clone());
                    Ok(repl_enter.clone())
                }),
            );
            w.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("patch.object.__exit__", move |_: &[PyObjectRef]| {
                    // Restore old value
                    let old = saved_for_exit.read().clone();
                    if let Some(old_val) = old {
                        set_mock_target_attr(&target_exit, &attr_exit, old_val);
                    } else {
                        delete_mock_target_attr(&target_exit, &attr_exit);
                    }
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    let patch_multiple_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "patch.multiple requires target".to_string(),
            ));
        }
        let kwargs = extract_mock_kwargs(&args[1..]);
        let target = args[0].clone();
        let target_obj = if let PyObjectPayload::Str(path) = &target.payload {
            crate::load_module(path.as_str()).ok_or_else(|| {
                PyException::attribute_error(format!("module '{}' not found", path))
            })?
        } else {
            target
        };
        let mut replacements = Vec::new();
        let mut returned = IndexMap::new();
        for (key, value) in kwargs.iter() {
            let HashableKey::Str(attr_key) = key else {
                continue;
            };
            let attr = attr_key.to_string();
            replacements.push((target_obj.clone(), attr.clone(), value.clone()));
            returned.insert(
                HashableKey::str_key(CompactString::from(attr)),
                value.clone(),
            );
        }
        Ok(build_patch_context(
            "_patch_multiple",
            replacements,
            PyObject::dict(returned),
        ))
    });

    // patch.dict — context manager for dict patching
    let patch_dict_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "patch.dict requires at least 1 argument".to_string(),
            ));
        }
        let cls = PyObject::class(CompactString::from("_patch_dict"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("__enter__"),
                make_builtin(|args: &[PyObjectRef]| {
                    if !args.is_empty() {
                        Ok(args[0].clone())
                    } else {
                        Ok(PyObject::none())
                    }
                }),
            );
            w.insert(
                CompactString::from("__exit__"),
                make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
            );
        }
        Ok(inst)
    });

    // Make patch a callable object with .object and .dict attributes
    let patch_cls = PyObject::class(CompactString::from("_patcher"), vec![], IndexMap::new());
    let patch_obj = PyObject::instance(patch_cls);
    if let PyObjectPayload::Instance(ref d) = patch_obj.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__call__"), patch_fn);
        w.insert(CompactString::from("object"), patch_object_fn);
        w.insert(CompactString::from("multiple"), patch_multiple_fn);
        w.insert(CompactString::from("dict"), patch_dict_fn);
    }

    make_module(
        "unittest.mock",
        vec![
            ("Mock", make_mock("Mock")),
            ("MagicMock", make_mock("MagicMock")),
            ("patch", patch_obj),
            ("sentinel", sentinel),
            ("call", call_fn),
            ("ANY", any_obj),
            ("DEFAULT", PyObject::str_val(CompactString::from("DEFAULT"))),
            ("PropertyMock", make_mock("PropertyMock")),
        ],
    )
}
