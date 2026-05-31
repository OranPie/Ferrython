use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject};
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{new_fx_hashkey_map, PyCell, PyObject, PyObjectRef};
use ferrython_core::types::{SharedConstantCache, SharedGlobals};
use indexmap::IndexMap;
use std::rc::Rc;

use crate::frame::Frame;
use crate::VirtualMachine;

fn required_arg_names<'a>(
    code: &'a CodeObject,
    start: usize,
    end: usize,
    locals: &[Option<PyObjectRef>],
) -> Vec<&'a str> {
    (start..end)
        .filter(|&i| locals.get(i).map_or(true, |slot| slot.is_none()))
        .filter_map(|i| code.varnames.get(i).map(|s| s.as_str()))
        .collect()
}

pub(super) fn format_missing_required(names: &[&str], kind: &str) -> String {
    let quoted = names
        .iter()
        .map(|n| format!("'{}'", n))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "missing {} required {} argument{}: {}",
        names.len(),
        kind,
        if names.len() == 1 { "" } else { "s" },
        quoted
    )
}

pub(super) fn format_too_many_positional(
    fname: &str,
    nparams: usize,
    ndefaults: usize,
    nargs: usize,
) -> String {
    let required = nparams.saturating_sub(ndefaults);
    if ndefaults > 0 && required != nparams {
        format!(
            "{}() takes from {} to {} positional arguments but {} {} given",
            fname,
            required,
            nparams,
            nargs,
            if nargs == 1 { "was" } else { "were" }
        )
    } else {
        format!(
            "{}() takes {} positional argument{} but {} {} given",
            fname,
            nparams,
            if nparams == 1 { "" } else { "s" },
            nargs,
            if nargs == 1 { "was" } else { "were" }
        )
    }
}

pub(super) fn check_missing_keyword_only(
    code: &CodeObject,
    locals: &[Option<PyObjectRef>],
    kwonly_start: usize,
    nkwonly: usize,
) -> PyResult<()> {
    let missing = required_arg_names(code, kwonly_start, kwonly_start + nkwonly, locals);
    if missing.is_empty() {
        Ok(())
    } else {
        Err(PyException::type_error(format!(
            "{}() {}",
            code.name,
            format_missing_required(&missing, "keyword-only")
        )))
    }
}

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
            return Err(PyException::type_error(format_too_many_positional(
                code.name.as_str(),
                nparams,
                defaults.len(),
                nargs,
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
        check_missing_keyword_only(code, &frame.locals, kwonly_start, nkwonly)?;

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            if frame.locals.get(kwargs_idx).map_or(true, |v| v.is_none()) {
                frame.set_local(kwargs_idx, PyObject::dict(new_fx_hashkey_map()));
            }
        }

        self.install_closure_and_run(frame, code, closure)
    }
}
