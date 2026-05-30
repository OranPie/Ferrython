//! Function/method call dispatch, class instantiation, super().

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use std::rc::Rc;

mod ast_nodes;
mod ast_nodes_lookup;
mod ast_nodes_populate;
mod builtin_attrs;
mod builtin_bound_call;
mod builtin_bound_class;
mod builtin_bound_delegate;
mod builtin_bound_fast;
mod builtin_bound_format;
mod builtin_bound_generators;
mod builtin_bound_iter;
mod builtin_bound_join;
mod builtin_bound_list;
mod builtin_call;
mod builtin_collections;
mod builtin_computation;
mod builtin_computation_order;
mod builtin_exec_import;
mod builtin_iterables;
mod builtin_iterables_enumerate_zip;
mod builtin_iterables_iter_next;
mod builtin_iterables_map_filter;
mod builtin_iterables_reversed;
mod builtin_kw;
mod builtin_kw_collections;
mod builtin_kw_fallback;
mod builtin_kw_misc;
mod builtin_kw_numeric;
mod builtin_kw_scope;
mod builtin_kw_truth;
mod builtin_namedtuple;
mod builtin_numeric;
mod builtin_numeric_complex;
mod builtin_numeric_protocol;
mod builtin_numeric_scalar;
mod builtin_predicates;
mod builtin_scope;
mod builtin_sum;
mod builtin_sum_generator;
mod builtin_sum_range;
mod builtin_sum_sequence;
mod builtin_text;
mod bytes_constructor;
mod class_abstract;
mod class_builtin_defaults;
mod class_builtin_numeric;
mod class_builtin_sets;
mod class_builtin_subclass;
mod class_builtin_values;
mod class_dataclass;
mod class_enum;
mod class_inline;
mod class_instantiate;
mod class_namedtuple_init;
mod class_post_init;
mod class_simple;
mod class_storage;
mod exception_build;
mod exception_group;
mod frame_run;
mod frameless;
mod function_call;
mod function_fast;
mod function_kw_call;
mod inline_simple;
mod iterator_state;
mod json_hooks;
mod locals;
mod native_closure_kw;
mod native_fallback_kw;
mod native_kw;
mod native_kw_collections;
mod native_kw_generic;
mod native_kw_json;
mod native_kw_regex_iter;
mod native_kw_special;
mod object_call;
mod object_call_class;
mod object_call_instance;
mod object_call_trace;
mod object_kw;
mod object_native_call;
mod object_native_finish;
mod object_native_iter;
mod object_native_special;
mod print_file;
mod print_format_map;
mod property_helpers;
mod property_init;
mod property_kwargs;
mod property_receiver;
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
        if let PyObjectPayload::Instance(inst) = &prop.payload {
            if let Some(flag) = inst.attrs.read().get("__isabstractmethod__").cloned() {
                return Ok(PyObject::bool_val(self.vm_is_truthy(&flag)?));
            }
        }
        Ok(PyObject::bool_val(false))
    }
}
