//! Function/method call dispatch, class instantiation, super().

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use std::rc::Rc;

mod ast_nodes;
mod bytes_constructor;
mod class_inline;
mod class_instantiate;
mod exception_build;
mod exception_group;
mod frame_run;
mod frameless;
mod function_call;
mod inline_simple;
mod iterator_state;
mod json_hooks;
mod locals;
mod object_call;
mod object_kw;
mod print_format;
mod property_helpers;
mod sort_helpers;
mod str_fast;
mod super_object;

use frameless::{CallObjectDepthGuard, FRAMELESS_CALL_RECURSION_LIMIT};

pub use exception_group::attach_eg_methods_pub;

impl VirtualMachine {
    #[inline]
    fn enter_frameless_call_dispatch(&self) -> PyResult<CallObjectDepthGuard> {
        let depth = Rc::clone(&self.call_object_depth);
        let next = depth.get().saturating_add(1);
        let raw_limit = ferrython_stdlib::get_recursion_limit();
        let configured_limit = if raw_limit > 0 {
            raw_limit as usize
        } else {
            self.recursion_limit
        };
        let limit = configured_limit.min(FRAMELESS_CALL_RECURSION_LIMIT);
        if next > limit {
            return Err(PyException::recursion_error(
                "maximum recursion depth exceeded",
            ));
        }
        depth.set(next);
        Ok(CallObjectDepthGuard { depth })
    }

    fn ast_class_name(cls: &PyObjectRef) -> Option<CompactString> {
        let PyObjectPayload::Class(cd) = &cls.payload else {
            return None;
        };
        if cd
            .namespace
            .read()
            .get("__ferrython_ast_node__")
            .map(|v| v.is_truthy())
            .unwrap_or(false)
        {
            return Some(cd.name.clone());
        }
        for base in &cd.mro {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                if bcd
                    .namespace
                    .read()
                    .get("__ferrython_ast_node__")
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    return Some(bcd.name.clone());
                }
            }
        }
        None
    }

    fn ast_class_fields(cls: &PyObjectRef) -> Vec<CompactString> {
        match cls.get_attr("_fields") {
            Some(fields) => match &fields.payload {
                PyObjectPayload::Tuple(items) => items
                    .iter()
                    .filter_map(|item| item.as_str().map(CompactString::from))
                    .collect(),
                _ => Vec::new(),
            },
            None => Vec::new(),
        }
    }

    fn ast_storage_name(name: &str) -> CompactString {
        if name == "n" || name == "s" {
            CompactString::from("value")
        } else {
            CompactString::from(name)
        }
    }

    pub(crate) fn property_isabstractmethod(
        &mut self,
        prop: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        for field in ["fget", "fset", "fdel"] {
            if let Some(func) = Self::property_callable_field(prop, field) {
                if let Some(flag) = func.get_attr("__isabstractmethod__") {
                    if self.vm_is_truthy(&flag)? {
                        return Ok(PyObject::bool_val(true));
                    }
                }
            }
        }
        Ok(PyObject::bool_val(false))
    }
}
