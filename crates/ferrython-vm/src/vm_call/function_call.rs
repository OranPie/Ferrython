use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject};
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    new_fx_hashkey_map, FxHashKeyMap, PyCell, PyObject, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyFunction, SharedConstantCache, SharedGlobals};
use indexmap::IndexMap;
use std::rc::Rc;

use crate::frame::{Frame, ScopeKind};
use crate::VirtualMachine;

impl VirtualMachine {
    pub(crate) fn call_function(
        &mut self,
        code: &Rc<CodeObject>,
        mut args: Vec<PyObjectRef>,
        defaults: &[PyObjectRef],
        kw_defaults: &IndexMap<CompactString, PyObjectRef>,
        globals: SharedGlobals,
        closure: &[Rc<PyCell<Option<PyObjectRef>>>],
        constant_cache: &SharedConstantCache,
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new_from_pool(
            Rc::clone(code),
            globals,
            self.builtins.clone(),
            Rc::clone(constant_cache),
            &mut self.frame_pool,
        );
        let nparams = code.arg_count as usize;
        let nkwonly = code.kwonlyarg_count as usize;
        let has_varargs = code.flags.contains(CodeFlags::VARARGS);
        let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);

        let nargs = args.len();
        let positional_count = nargs.min(nparams);

        // Move positional args into locals (zero-clone via drain)
        {
            let mut drain = args.drain(..positional_count);
            for i in 0..positional_count {
                frame.set_local(i, drain.next().unwrap());
            }
        }
        // `args` now contains only surplus positional args (if any)

        // Fill in defaults for missing positional args
        if nargs < nparams && !defaults.is_empty() {
            let ndefaults = defaults.len();
            let first_default_param = nparams - ndefaults;
            for i in nargs..nparams {
                if i >= first_default_param {
                    let default_idx = i - first_default_param;
                    frame.set_local(i, defaults[default_idx].clone());
                }
            }
        }

        // Check for missing required positional args
        if nargs < nparams {
            let ndefaults = defaults.len();
            let required = nparams - ndefaults;
            if nargs < required {
                let missing = required - nargs;
                let fname = code.name.as_str();
                let missing_names: Vec<&str> = (nargs..required)
                    .filter_map(|i| code.varnames.get(i).map(|s| s.as_str()))
                    .collect();
                return Err(PyException::type_error(format!(
                    "{}() missing {} required positional argument{}: {}",
                    fname,
                    missing,
                    if missing == 1 { "" } else { "s" },
                    missing_names
                        .iter()
                        .map(|n| format!("'{}'", n))
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
        }

        // Pack extra positional args into *args tuple, or raise TypeError
        if has_varargs {
            let extra: Vec<PyObjectRef> = if nargs > nparams {
                args // already drained to only surplus args
            } else {
                Vec::new()
            };
            frame.set_local(nparams, PyObject::tuple(extra));
        } else if nargs > nparams {
            let fname = code.name.as_str();
            return Err(PyException::type_error(format!(
                "{}() takes {} positional argument{} but {} {} given",
                fname,
                nparams,
                if nparams == 1 { "" } else { "s" },
                nargs,
                if nargs == 1 { "was" } else { "were" }
            )));
        }

        // Fill in kw_defaults for keyword-only args
        let kwonly_start = if has_varargs { nparams + 1 } else { nparams };
        for i in 0..nkwonly {
            let slot = kwonly_start + i;
            if frame.locals.get(slot).map_or(true, |v| v.is_none()) {
                if let Some(varname) = code.varnames.get(slot) {
                    if let Some(default_val) = kw_defaults.get(varname.as_str()) {
                        frame.set_local(slot, default_val.clone());
                    }
                }
            }
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            if frame.locals.get(kwargs_idx).map_or(true, |v| v.is_none()) {
                frame.set_local(kwargs_idx, PyObject::dict(new_fx_hashkey_map()));
            }
        }

        self.install_closure_and_run(frame, code, closure)
    }

    pub(crate) fn call_function_kw(
        &mut self,
        code: &Rc<CodeObject>,
        mut pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
        defaults: &[PyObjectRef],
        kw_defaults: &IndexMap<CompactString, PyObjectRef>,
        globals: SharedGlobals,
        closure: &[Rc<PyCell<Option<PyObjectRef>>>],
        constant_cache: &SharedConstantCache,
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new_from_pool(
            Rc::clone(code),
            globals,
            self.builtins.clone(),
            Rc::clone(constant_cache),
            &mut self.frame_pool,
        );
        let nparams = code.arg_count as usize;
        let nkwonly = code.kwonlyarg_count as usize;
        let has_varargs = code.flags.contains(CodeFlags::VARARGS);
        let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);

        // Total named parameters (positional + keyword-only)
        let _total_named = nparams + nkwonly;
        // Varargs slot comes after positional params
        let varargs_slot = nparams;
        // Keyword-only params start after *args slot (if present)
        let kwonly_start = if has_varargs { nparams + 1 } else { nparams };

        let npos = pos_args.len();
        let positional_count = npos.min(nparams);

        // Move positional args into locals (zero-clone via drain)
        {
            let mut drain = pos_args.drain(..positional_count);
            for i in 0..positional_count {
                frame.set_local(i, drain.next().unwrap());
            }
        }
        // `pos_args` now contains only surplus positional args (if any)

        // Place keyword args at their correct parameter positions
        // Build a name→index lookup for O(1) kwarg matching
        let posonlyarg_count = code.posonlyarg_count as usize;
        let mut extra_kwargs: FxHashKeyMap = new_fx_hashkey_map();
        // Pre-build varname→index map for fast lookup when kwargs > 2
        let varname_map: Option<std::collections::HashMap<&str, usize>> = if kwargs.len() > 2 {
            Some(
                code.varnames
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (v.as_str(), i))
                    .collect(),
            )
        } else {
            None
        };
        for (name, val) in kwargs {
            let found_idx = if let Some(ref map) = varname_map {
                map.get(name.as_str()).copied()
            } else {
                code.varnames
                    .iter()
                    .position(|v| v.as_str() == name.as_str())
            };
            if let Some(idx) = found_idx {
                // Reject positional-only parameters passed as keyword arguments
                if idx < posonlyarg_count {
                    return Err(PyException::type_error(format!(
                        "{}() got some positional-only arguments passed as keyword arguments: '{}'",
                        code.name, name
                    )));
                }
                // Accept both positional params (< nparams) and kwonly params
                let is_positional = idx < nparams;
                let is_kwonly = idx >= kwonly_start && idx < kwonly_start + nkwonly;
                if is_positional || is_kwonly {
                    frame.set_local(idx, val);
                    continue;
                }
            }
            // Not a known parameter — goes into **kwargs
            extra_kwargs.insert(HashableKey::str_key(name), val);
        }
        if !has_varkw && !extra_kwargs.is_empty() {
            let unexpected = extra_kwargs
                .keys()
                .find_map(|key| match key {
                    HashableKey::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("?");
            return Err(PyException::type_error(format!(
                "{}() got an unexpected keyword argument '{}'",
                code.name, unexpected
            )));
        }

        // Fill in defaults for missing positional args
        if !defaults.is_empty() {
            let ndefaults = defaults.len();
            let first_default_param = nparams - ndefaults;
            for i in 0..nparams {
                if frame.locals[i].is_none() && i >= first_default_param {
                    let default_idx = i - first_default_param;
                    frame.set_local(i, defaults[default_idx].clone());
                }
            }
        }
        let ndefaults = defaults.len();
        let required = nparams - ndefaults;
        let missing_names: Vec<&str> = (0..required)
            .filter(|&i| frame.locals.get(i).map_or(true, |slot| slot.is_none()))
            .filter_map(|i| code.varnames.get(i).map(|s| s.as_str()))
            .collect();
        if !missing_names.is_empty() {
            return Err(PyException::type_error(format!(
                "{}() missing {} required positional argument{}: {}",
                code.name,
                missing_names.len(),
                if missing_names.len() == 1 { "" } else { "s" },
                missing_names
                    .iter()
                    .map(|n| format!("'{}'", n))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        // Fill in kw_defaults for missing keyword-only args
        for i in 0..nkwonly {
            let slot = kwonly_start + i;
            if frame.locals.get(slot).map_or(true, |v| v.is_none()) {
                if let Some(varname) = code.varnames.get(slot) {
                    if let Some(default_val) = kw_defaults.get(varname.as_str()) {
                        frame.set_local(slot, default_val.clone());
                    }
                }
            }
        }

        // Pack extra positional args into *args tuple, or raise TypeError
        if has_varargs {
            let extra: Vec<PyObjectRef> = if npos > nparams {
                pos_args // already drained to only surplus args
            } else {
                Vec::new()
            };
            frame.set_local(varargs_slot, PyObject::tuple(extra));
        } else if npos > nparams {
            let fname = code.name.as_str();
            return Err(PyException::type_error(format!(
                "{}() takes {} positional argument{} but {} {} given",
                fname,
                nparams,
                if nparams == 1 { "" } else { "s" },
                npos,
                if npos == 1 { "was" } else { "were" }
            )));
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            frame.set_local(kwargs_idx, PyObject::dict(extra_kwargs));
        }

        self.install_closure_and_run(frame, code, closure)
    }

    pub(crate) fn call_object_one_arg_fast_or_fallback(
        &mut self,
        func: PyObjectRef,
        arg: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let pyfunc_ptr = match &func.payload {
            PyObjectPayload::Function(pyfunc)
                if pyfunc.is_simple
                    && pyfunc.code.arg_count == 1
                    && pyfunc.defaults.is_empty()
                    && pyfunc.kw_defaults.is_empty()
                    && pyfunc.closure.is_empty() =>
            {
                &**pyfunc as *const PyFunction
            }
            _ => return self.call_object(func, vec![arg]),
        };

        if ferrython_stdlib::is_trace_active() || ferrython_stdlib::is_profile_active() {
            return self.call_object(func, vec![arg]);
        }

        // `func` owns the payload behind this pointer and is moved into the borrowed
        // frame below, so the code/globals/constant cache stay alive while it runs.
        let pyfunc = unsafe { &*pyfunc_ptr };
        if let Some(result) = Self::try_inline_simple_function_one_arg(pyfunc, &arg) {
            return Ok(result);
        }

        let mut frame =
            unsafe { Frame::new_borrowed(pyfunc, func, &self.builtins, &mut self.frame_pool) };
        frame.locals[0] = Some(arg);
        frame.scope_kind = ScopeKind::Function;

        self.call_stack.push(frame);
        if self.call_stack.len() > self.recursion_limit {
            if let Some(frame) = self.call_stack.pop() {
                frame.recycle(&mut self.frame_pool);
            }
            return Err(PyException::recursion_error(
                "maximum recursion depth exceeded",
            ));
        }
        let result = self.run_frame();
        if let Some(frame) = self.call_stack.pop() {
            frame.recycle(&mut self.frame_pool);
        }
        result
    }
}
