use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

use crate::vm_call::exception_build::build_builtin_exception_instance;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(crate) fn call_object(
        &mut self,
        func: PyObjectRef,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        let needs_current_frame = ferrython_stdlib::is_trace_active()
            || ferrython_stdlib::is_profile_active()
            || matches!(&func.payload, PyObjectPayload::NativeFunction(nf) if nf.name.as_str() == "sys._getframe");
        let prev_frame = if needs_current_frame {
            ferrython_stdlib::get_current_frame()
        } else {
            None
        };
        if needs_current_frame && !self.call_stack.is_empty() {
            ferrython_stdlib::set_current_frame(Some(self.make_trace_frame()));
        }
        let result = match &func.payload {
            PyObjectPayload::Function(pyfunc) => {
                // Borrow fields directly from the Arc-backed func instead of cloning
                // expensive Vec/IndexMap payloads. Only globals needs cloning (moved into frame).
                let globals = pyfunc.globals.clone();
                self.call_function(
                    &pyfunc.code,
                    args,
                    &pyfunc.defaults,
                    &pyfunc.kw_defaults,
                    globals,
                    &pyfunc.closure,
                    &pyfunc.constant_cache,
                )
            }
            PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
                self.call_builtin_or_type(&func, name, args)
            }
            PyObjectPayload::Class(cd) => {
                // If the metaclass defines its own __call__ (not just type.__call__),
                // dispatch through it.
                if let Some(meta) = &cd.metaclass {
                    if let Some(call_method) = meta.get_attr("__call__") {
                        // Skip if this is just the inherited type.__call__ builtin
                        let is_inherited_type_call = matches!(
                            &call_method.payload,
                            PyObjectPayload::BuiltinBoundMethod(bbm)
                                if bbm.method_name.as_str() == "__call__"
                                && matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(t) if t.as_str() == "type")
                        );
                        if !is_inherited_type_call {
                            let mut call_args = vec![func.clone()];
                            call_args.extend(args);
                            return self.call_object(call_method, call_args);
                        }
                    }
                }
                self.instantiate_class(&func, args, vec![])
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                // VM intercept: RawIOBase.read(size=-1) → calls self.readinto()
                if let PyObjectPayload::NativeFunction(nf) = &method.payload {
                    if nf.name.as_str() == "RawIOBase.read" {
                        let size: i64 = args.first().and_then(|a| a.as_int()).unwrap_or(-1);
                        return self.rawiobase_read(receiver, size);
                    }
                    if nf.name.as_str() == "RawIOBase.readall" {
                        return self.rawiobase_readall(receiver);
                    }
                }
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(args);
                self.call_object(method.clone(), bound_args)
            }
            PyObjectPayload::BuiltinBoundMethod(bbm) => self.call_builtin_bound_method(bbm, args),
            PyObjectPayload::ExceptionType(kind) => {
                build_builtin_exception_instance(*kind, args, &[])
            }
            PyObjectPayload::NativeFunction(nf_data) => {
                // Intercept functions that need VM access to call Python callables
                if nf_data.name.as_str() == "_ast.AST.__init__" {
                    if args.is_empty() {
                        return Err(PyException::type_error("__init__ requires self"));
                    }
                    let (pos_args, kwargs) = Self::split_trailing_kwargs_dict(&args);
                    if pos_args.is_empty() {
                        return Err(PyException::type_error("__init__ requires self"));
                    }
                    let instance = &pos_args[0];
                    let cls = match &instance.payload {
                        PyObjectPayload::Instance(inst) => inst.class.clone(),
                        _ => {
                            return Err(PyException::type_error(
                                "AST.__init__ requires an AST instance",
                            ))
                        }
                    };
                    Self::populate_ast_node_attrs(instance, &cls, &pos_args[1..], &kwargs)?;
                    return Ok(PyObject::none());
                }
                if nf_data.name.as_str() == "_ast.AST.__new__" {
                    if args.is_empty() {
                        return Err(PyException::type_error("__new__ requires cls"));
                    }
                    let (pos_args, kwargs) = Self::split_trailing_kwargs_dict(&args);
                    if pos_args.is_empty() {
                        return Err(PyException::type_error("__new__ requires cls"));
                    }
                    let cls = pos_args[0].clone();
                    let pos_args = pos_args[1..].to_vec();
                    return Ok(self
                        .try_instantiate_ast_node(&cls, pos_args, kwargs)?
                        .unwrap_or_else(|| PyObject::instance(cls)));
                }
                // property.__get__(self, obj, objtype) — must call fget(obj) and return result
                if nf_data.name.as_str() == "property.__get__" {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "descriptor '__get__' requires a property object",
                        ));
                    }
                    let prop = &args[0];
                    let obj = args.get(1);
                    let is_none_obj = match obj {
                        Some(o) => matches!(&o.payload, PyObjectPayload::None),
                        None => true,
                    };
                    if is_none_obj {
                        return Ok(prop.clone());
                    }
                    let obj = obj.unwrap();
                    // Try native Property payload first
                    if let PyObjectPayload::Property(pd) = &prop.payload {
                        if let Some(getter) = pd.fget.as_ref() {
                            let getter = crate::builtins::unwrap_abstract_fget(getter);
                            return self.call_object(getter, vec![obj.clone()]);
                        }
                        return Err(PyException::attribute_error("unreadable attribute"));
                    }
                    // Instance subclass of property — look for fget in instance attrs
                    if let PyObjectPayload::Instance(inst) = &prop.payload {
                        if let Some(fget) = inst.attrs.read().get("fget").cloned() {
                            if !matches!(&fget.payload, PyObjectPayload::None) {
                                return self.call_object(fget, vec![obj.clone()]);
                            }
                        }
                    }
                    return Err(PyException::attribute_error("unreadable attribute"));
                }
                if nf_data.name.as_str() == "functools.reduce" {
                    return self.vm_functools_reduce(&args);
                }
                if nf_data.name.as_str() == "itertools.islice" {
                    return self.vm_itertools_islice(&args);
                }
                // singledispatch.register: register(type) → decorator
                if nf_data.name.as_str() == "singledispatch.register" {
                    return self.vm_singledispatch_register(&args);
                }
                // type.__call__(cls, *args) — standard class instantiation protocol
                if nf_data.name.as_str() == "__type_call__" {
                    if args.is_empty() {
                        return Err(PyException::type_error("type.__call__ requires cls"));
                    }
                    let cls = args[0].clone();
                    let rest = args[1..].to_vec();
                    return self.instantiate_class(&cls, rest, vec![]);
                }
                // re.sub / re.subn with callable replacement
                if (nf_data.name.as_str() == "re.sub" || nf_data.name.as_str() == "re.subn")
                    && args.len() >= 3
                {
                    let repl = &args[1];
                    let is_callable = matches!(
                        &repl.payload,
                        PyObjectPayload::Function(_)
                            | PyObjectPayload::BuiltinFunction(_)
                            | PyObjectPayload::NativeFunction(_)
                            | PyObjectPayload::NativeClosure(_)
                            | PyObjectPayload::Partial(_)
                    );
                    if is_callable {
                        return self
                            .re_sub_with_callable(&args, nf_data.name.as_str() == "re.subn");
                    }
                }
                if nf_data.name.as_str() == "itertools.groupby" {
                    let mut key_fn = None;
                    let mut iterable_end = args.len();
                    // Check last arg for kwargs dict with "key"
                    if let Some(last) = args.last() {
                        if let PyObjectPayload::Dict(map) = &last.payload {
                            let map_r = map.read();
                            key_fn = map_r
                                .get(&HashableKey::str_key(CompactString::from("key")))
                                .cloned();
                            if key_fn.is_some() {
                                iterable_end = args.len() - 1;
                            }
                        }
                    }
                    // Check for positional key arg (2nd arg, not a dict)
                    if key_fn.is_none() && iterable_end >= 2 {
                        key_fn = Some(args[1].clone());
                        iterable_end = 1;
                    }
                    return self.vm_itertools_groupby(&args[..iterable_end], key_fn);
                }
                if nf_data.name.as_str() == "itertools.filterfalse" && args.len() >= 2 {
                    return self.vm_itertools_filterfalse(&args);
                }
                if nf_data.name.as_str() == "itertools.starmap" && args.len() >= 2 {
                    return self.vm_itertools_starmap(&args);
                }
                if nf_data.name.as_str() == "itertools.accumulate" && args.len() >= 2 {
                    return self.vm_itertools_accumulate(&args);
                }
                if nf_data.name.as_str() == "dict.fromkeys"
                    && !args.is_empty()
                    && matches!(
                        args[0].payload,
                        PyObjectPayload::Generator(_)
                            | PyObjectPayload::Instance(_)
                            | PyObjectPayload::Iterator(_)
                    )
                {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                    resolved.extend_from_slice(&args[1..]);
                    return (nf_data.func)(&resolved);
                }
                // math.trunc / math.floor / math.ceil — dispatch to __trunc__ / __floor__ / __ceil__
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        let dunder = match nf_data.name.as_str() {
                            "math.trunc" => Some("__trunc__"),
                            "math.floor" => Some("__floor__"),
                            "math.ceil" => Some("__ceil__"),
                            _ => None,
                        };
                        if let Some(dunder_name) = dunder {
                            if let Some(method) = args[0].get_attr(dunder_name) {
                                let ca = if matches!(
                                    &method.payload,
                                    PyObjectPayload::BoundMethod { .. }
                                ) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                                return self.call_object(method, ca);
                            }
                        }
                    }
                }
                // os.fspath — dispatch to __fspath__
                if nf_data.name.as_str() == "os.fspath" && args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = args[0].get_attr("__fspath__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca);
                        }
                    }
                }
                // Resolve generators to lists for stdlib NativeFunctions
                // that expect iterables (e.g. Counter, deque, OrderedDict, set)
                if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Generator(_)) {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                    resolved.extend_from_slice(&args[1..]);
                    return (nf_data.func)(&resolved);
                }
                let result = (nf_data.func)(&args)?;
                // Check if native function requested VM method calls
                let collect_mode = ferrython_core::error::take_collect_vm_call_results();
                if collect_mode {
                    let mut collected = Vec::new();
                    while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call()
                    {
                        collected.push(self.call_object(method, margs)?);
                    }
                    if !collected.is_empty() {
                        return Ok(PyObject::list(collected));
                    }
                }
                while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                    self.call_object(method, margs)?;
                }
                // Execute any deferred calls (e.g., HTMLParser.feed() callbacks)
                let deferred = ferrython_stdlib::drain_deferred_calls();
                for (dfunc, dargs) in deferred {
                    self.call_object(dfunc, dargs)?;
                }
                Ok(result)
            }
            PyObjectPayload::NativeClosure(nc) => {
                // Resolve generators to lists for NativeClosure functions
                let args = if !args.is_empty()
                    && matches!(&args[0].payload, PyObjectPayload::Generator(_))
                {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                    resolved.extend_from_slice(&args[1..]);
                    resolved
                } else {
                    args
                };
                let result = (nc.func)(&args)?;
                // Check if stdlib requested VM method calls (loop for multiple)
                let collect_mode = ferrython_core::error::take_collect_vm_call_results();
                if collect_mode {
                    let mut collected = Vec::new();
                    while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call()
                    {
                        collected.push(self.call_object(method, margs)?);
                    }
                    if !collected.is_empty() {
                        return Ok(PyObject::list(collected));
                    }
                }
                while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                    self.call_object(method, margs)?;
                }
                // Execute any deferred calls (e.g., Thread.start() calling Python functions)
                let deferred = ferrython_stdlib::drain_deferred_calls();
                for (dfunc, dargs) in deferred {
                    self.call_object(dfunc, dargs)?;
                }
                // Check if asyncio.run() was invoked — drive the coroutine to completion
                if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
                    return self.maybe_await_result(coro);
                }
                Ok(result)
            }
            PyObjectPayload::Partial(pd) => {
                let partial_func = pd.func.clone();
                let mut combined_args = pd.args.clone();
                combined_args.extend(args);
                if !pd.kwargs.is_empty() {
                    let kw: Vec<(CompactString, PyObjectRef)> = pd
                        .kwargs
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    self.call_object_kw(partial_func, combined_args, kw)
                } else {
                    self.call_object(partial_func, combined_args)
                }
            }
            PyObjectPayload::Instance(_inst) => {
                // lru_cache wrapper: check _cache + __wrapped__
                if let Some(cache_obj) = func.get_attr("_cache") {
                    if let Some(wrapped) = func.get_attr("__wrapped__") {
                        if let PyObjectPayload::Dict(cache_map) = &cache_obj.payload {
                            // Build cache key from stringified args
                            let key_str =
                                args.iter().map(|a| a.repr()).collect::<Vec<_>>().join(",");
                            let cache_key = HashableKey::str_key(CompactString::from(&key_str));
                            // Check cache
                            let cached_val = cache_map.read().get(&cache_key).cloned();
                            if let Some(cached) = cached_val {
                                // Cache hit: move to MRU position (re-insert at end) for LRU eviction
                                {
                                    let mut cw = cache_map.write();
                                    cw.shift_remove(&cache_key);
                                    cw.insert(cache_key, cached.clone());
                                }
                                // Increment _hits counter
                                if let PyObjectPayload::Instance(ref d) = func.payload {
                                    let mut w = d.attrs.write();
                                    let hits = w
                                        .get(&intern_or_new("_hits"))
                                        .and_then(|v| v.as_int())
                                        .unwrap_or(0);
                                    w.insert(intern_or_new("_hits"), PyObject::int(hits + 1));
                                }
                                return Ok(cached);
                            }
                            // Cache miss: call the wrapped function, increment _misses
                            if let PyObjectPayload::Instance(ref d) = func.payload {
                                let mut w = d.attrs.write();
                                let misses = w
                                    .get(&intern_or_new("_misses"))
                                    .and_then(|v| v.as_int())
                                    .unwrap_or(0);
                                w.insert(intern_or_new("_misses"), PyObject::int(misses + 1));
                            }
                            let result = self.call_object(wrapped, args)?;
                            // Enforce maxsize: evict LRU entry (first in insertion order) when cache is full
                            {
                                let mut cache_w = cache_map.write();
                                if let PyObjectPayload::Instance(ref d) = func.payload {
                                    let maxsize = d
                                        .attrs
                                        .read()
                                        .get(&intern_or_new("_maxsize"))
                                        .and_then(|v| v.as_int());
                                    if let Some(max) = maxsize {
                                        if max >= 0 {
                                            while cache_w.len() >= max as usize {
                                                cache_w.shift_remove_index(0);
                                            }
                                        }
                                    }
                                }
                                cache_w.insert(cache_key, result.clone());
                            }
                            return Ok(result);
                        }
                    }
                }
                // Callable instances: check for __call__
                if func.get_attr("__singledispatch__").is_some() {
                    // singledispatch: dispatch based on first arg type
                    return self.vm_singledispatch_call_instance(&func, &args);
                }
                if let Some(method) = func.get_attr("__call__") {
                    let _dispatch_guard = self.enter_frameless_call_dispatch()?;
                    let result = self.call_object(method, args);
                    drop(func);
                    result
                } else {
                    Err(PyException::type_error(format!(
                        "'{}' object is not callable",
                        func.type_name()
                    )))
                }
            }
            _ => Err(PyException::type_error(format!(
                "'{}' object is not callable",
                func.type_name()
            ))),
        };
        if needs_current_frame {
            ferrython_stdlib::set_current_frame(prev_frame);
        }
        result
    }
}
