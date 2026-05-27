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
                self.call_native_function_object(nf_data, args)
            }
            PyObjectPayload::NativeClosure(nc) => self.call_native_closure_object(nc, args),
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
