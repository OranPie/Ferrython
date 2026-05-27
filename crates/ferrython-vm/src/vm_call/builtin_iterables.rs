use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use std::rc::Rc;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_iterable_builtin(
        &mut self,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name.as_str() {
            "map" => {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "map() requires at least 2 arguments",
                    ));
                }
                let func_obj = args[0].clone();
                let mut sources = Vec::with_capacity(args.len() - 1);
                for a in &args[1..] {
                    sources.push(self.resolve_iterable(a)?);
                }
                if sources.len() == 1 {
                    return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                        PyCell::new(IteratorData::MapOne {
                            func: func_obj,
                            source: sources.pop().unwrap(),
                        }),
                    ))));
                }
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::Map {
                        func: func_obj,
                        sources,
                    }),
                ))));
            }
            "filter" => {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "filter() requires at least 2 arguments",
                    ));
                }
                let func_obj = args[0].clone();
                let source = self.resolve_iterable(&args[1])?;
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::Filter {
                        func: func_obj,
                        source,
                    }),
                ))));
            }
            "iter" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if let Some(raw_iter) = Self::resolve_instance_dunder(&args[0], "__iter__")
                        {
                            let iter_method = self.resolve_descriptor(&raw_iter, &args[0])?;
                            let r = self.call_object(iter_method, vec![])?;
                            return Self::ensure_iterator_result(&args[0], r);
                        }
                        if inst.dict_storage.is_some() {
                            return args[0].get_iter();
                        }
                        // Builtin base type subclass: delegate to __builtin_value__
                        if let Some(bv) = Self::get_builtin_value(&args[0]) {
                            let iter = self.resolve_iterable(&bv)?;
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                PyCell::new(IteratorData::HeldIter {
                                    iter,
                                    owner: Some(args[0].clone()),
                                }),
                            ))));
                        }
                        // Old-style sequence protocol: lazy SeqIter
                        if args[0].get_attr("__getitem__").is_some() {
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                PyCell::new(IteratorData::SeqIter {
                                    obj: args[0].clone(),
                                    index: 0,
                                    exhausted: false,
                                }),
                            ))));
                        }
                        return Err(PyException::type_error(format!(
                            "'{}' object is not iterable",
                            args[0].type_name()
                        )));
                    }
                    // Fall through to builtin dispatch for non-instances
                }
            }
            "next" => {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "next() requires at least 1 argument",
                    ));
                }
                // For generators, resume directly so StopIteration return value propagates
                if let PyObjectPayload::Generator(gen_arc) = &args[0].payload {
                    match self.resume_generator(gen_arc, PyObject::none()) {
                        Ok(value) => return Ok(value),
                        Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                            return Ok(args[1].clone());
                        }
                        Err(e) => return Err(e),
                    }
                }
                // Use vm_iter_next which handles instances and lazy iterators
                match self.vm_iter_next(&args[0]) {
                    Ok(Some(value)) => return Ok(value),
                    Ok(None) => {
                        if args.len() > 1 {
                            return Ok(args[1].clone()); // default value
                        }
                        return Err(PyException::new(ExceptionKind::StopIteration, ""));
                    }
                    Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                        return Ok(args[1].clone());
                    }
                    Err(e) => return Err(e),
                }
            }
            "reversed" => {
                if !args.is_empty() {
                    if matches!(&args[0].payload, PyObjectPayload::List(_)) {
                        return builtins::dispatch("reversed", &[args[0].clone()]);
                    }
                    // Check for __reversed__ dunder on instances
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(rev_method) =
                            Self::resolve_instance_dunder(&args[0], "__reversed__")
                        {
                            return self.call_object(rev_method, vec![]);
                        }
                        if let Some(bv) = Self::get_builtin_value(&args[0]) {
                            let items = self.collect_iterable(&bv)?;
                            let iter = builtins::dispatch("reversed", &[PyObject::list(items)])?;
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                PyCell::new(IteratorData::HeldIter {
                                    iter,
                                    owner: Some(args[0].clone()),
                                }),
                            ))));
                        }
                    }
                    let items = self.collect_iterable(&args[0])?;
                    return builtins::dispatch("reversed", &[PyObject::list(items)]);
                }
            }
            "enumerate" => {
                if !args.is_empty() {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(self.resolve_iterable(&args[0])?);
                    resolved.extend_from_slice(&args[1..]);
                    return builtins::dispatch("enumerate", &resolved);
                }
                return builtins::dispatch("enumerate", &args);
            }
            "zip" => {
                // Check for trailing kwargs dict (e.g. strict=True)
                let mut strict = false;
                let iter_end = if let Some(last) = args.last() {
                    if let PyObjectPayload::Dict(kw) = &last.payload {
                        let r = kw.read();
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("strict")))
                        {
                            strict = v.is_truthy();
                        }
                        drop(r);
                        args.len() - 1
                    } else {
                        args.len()
                    }
                } else {
                    args.len()
                };
                let resolved = self.resolve_iterables(&args[..iter_end])?;
                let mut full_args = resolved;
                if strict {
                    // Re-add kwargs dict so builtin_zip can pick it up
                    let kw = PyObject::dict(indexmap::IndexMap::from([(
                        HashableKey::str_key(CompactString::from("strict")),
                        PyObject::bool_val(true),
                    )]));
                    full_args.push(kw);
                }
                return builtins::dispatch("zip", &full_args);
            }
            _ => {}
        }
        match builtins::get_builtin_fn(name.as_str()) {
            Some(f) => f(&args),
            None => Err(PyException::type_error(format!(
                "'{}' is not callable",
                name
            ))),
        }
    }
}
