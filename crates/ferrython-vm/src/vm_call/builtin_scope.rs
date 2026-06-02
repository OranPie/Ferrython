use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    new_fx_hashkey_map, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::builtins;
use crate::frame::ScopeKind;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_scope_builtin(
        &mut self,
        name: &str,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name {
            "globals" => Ok(self.current_globals_object()),
            "locals" => self.current_locals_object(),
            "vars" if args.is_empty() => self.collect_locals_dict(),
            "vars" => fallback_scope_builtin(name, &args),
            "dir" => self.call_dir_builtin(args),
            _ => unreachable!("non-scope builtin routed to scope dispatch"),
        }
    }

    fn current_globals_object(&self) -> PyObjectRef {
        if let Some(frame) = self.call_stack.last() {
            let globals_arc = frame.globals.clone();
            return PyObject::wrap(PyObjectPayload::InstanceDict(globals_arc));
        }
        PyObject::dict(new_fx_hashkey_map())
    }

    fn current_locals_object(&self) -> PyResult<PyObjectRef> {
        if let Some(frame) = self.call_stack.last() {
            if let Some(locals) = &frame.exec_locals {
                return Ok(locals.clone());
            }
            if matches!(frame.scope_kind, ScopeKind::Module) {
                if let Some(globals_obj) = &frame.exec_globals {
                    return Ok(globals_obj.clone());
                }
            }
            let mut map = IndexMap::new();
            for (i, name) in frame.code.varnames.iter().enumerate() {
                if let Some(Some(val)) = frame.locals.get(i) {
                    map.insert(HashableKey::str_key(name.clone()), val.clone());
                }
            }
            if frame.code.varnames.is_empty() {
                let g = frame.globals.read();
                for (k, v) in g.iter() {
                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                }
                drop(g);
                for (k, v) in frame.local_names_iter() {
                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                }
            }
            return Ok(PyObject::dict(map));
        }
        Ok(PyObject::dict(new_fx_hashkey_map()))
    }

    fn call_dir_builtin(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            if let Some(locals) = self.call_stack.last().and_then(|f| f.exec_locals.clone()) {
                let mut names: Vec<String> = self
                    .exec_locals_keys(&locals)?
                    .into_iter()
                    .map(|key| key.py_to_string())
                    .collect();
                names.sort();
                let items = names
                    .into_iter()
                    .map(|n| PyObject::str_val(CompactString::from(n)))
                    .collect();
                return Ok(PyObject::list(items));
            }
            let locals = self.collect_locals_dict()?;
            if let PyObjectPayload::Dict(map) = &locals.payload {
                let mut names: Vec<String> = map
                    .read()
                    .keys()
                    .map(|k| k.to_object().py_to_string())
                    .collect();
                names.sort();
                let items = names
                    .into_iter()
                    .map(|n| PyObject::str_val(CompactString::from(n)))
                    .collect();
                return Ok(PyObject::list(items));
            }
        }
        if args.len() == 1 {
            if let PyObjectPayload::Instance(_) = &args[0].payload {
                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__dir__") {
                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                        vec![]
                    } else {
                        vec![args[0].clone()]
                    };
                    return self.call_object(method, ca);
                }
            }
        }
        fallback_scope_builtin("dir", &args)
    }
}

fn fallback_scope_builtin(name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match builtins::get_builtin_fn(name) {
        Some(f) => f(args),
        None => unreachable!("scope builtin missing fallback"),
    }
}
