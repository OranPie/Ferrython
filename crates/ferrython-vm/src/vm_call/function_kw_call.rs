use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject};
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{new_fx_hashkey_map, FxHashKeyMap, PyCell, PyObject, PyObjectRef};
use ferrython_core::types::{HashableKey, SharedConstantCache, SharedGlobals};
use indexmap::IndexMap;
use std::rc::Rc;

use super::function_call::{
    check_missing_keyword_only, format_missing_required, format_too_many_positional,
};
use crate::frame::Frame;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(crate) fn call_function_kw(
        &mut self,
        code: &Rc<CodeObject>,
        func_name: CompactString,
        func_qualname: CompactString,
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

        let varargs_slot = nparams;
        let kwonly_start = if has_varargs { nparams + 1 } else { nparams };

        let npos = pos_args.len();
        let positional_count = npos.min(nparams);

        {
            let mut drain = pos_args.drain(..positional_count);
            for i in 0..positional_count {
                frame.set_local(i, drain.next().unwrap());
            }
        }

        let posonlyarg_count = code.posonlyarg_count as usize;
        let mut extra_kwargs: FxHashKeyMap = new_fx_hashkey_map();
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
                if idx < posonlyarg_count {
                    if has_varkw {
                        extra_kwargs.insert(HashableKey::str_key(name), val);
                        continue;
                    }
                    return Err(PyException::type_error(format!(
                        "{}() got some positional-only arguments passed as keyword arguments: '{}'",
                        code.name, name
                    )));
                }
                let is_positional = idx < nparams;
                let is_kwonly = idx >= kwonly_start && idx < kwonly_start + nkwonly;
                if is_positional || is_kwonly {
                    frame.set_local(idx, val);
                    continue;
                }
            }
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
                "{}() {}",
                code.name,
                format_missing_required(&missing_names, "positional")
            )));
        }

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
        check_missing_keyword_only(code, &frame.locals, kwonly_start, nkwonly)?;

        if has_varargs {
            let extra: Vec<PyObjectRef> = if npos > nparams { pos_args } else { Vec::new() };
            frame.set_local(varargs_slot, PyObject::tuple(extra));
        } else if npos > nparams {
            return Err(PyException::type_error(format_too_many_positional(
                code.name.as_str(),
                nparams,
                defaults.len(),
                npos,
            )));
        }

        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            frame.set_local(kwargs_idx, PyObject::dict(extra_kwargs));
        }

        self.install_closure_and_run(frame, code, closure, func_name, func_qualname)
    }
}
