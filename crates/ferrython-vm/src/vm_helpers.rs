//! VM utility functions — iteration, sorting, and call hooks.

use crate::builtins;
use crate::VirtualMachine;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    lookup_in_class_mro, CompareOp, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use std::cell::{Cell, RefCell};

mod iter_next;
mod iterables;

thread_local! {
    static SORT_GUARDS: RefCell<Vec<SortGuardEntry>> = const { RefCell::new(Vec::new()) };
    static NEXT_SORT_GUARD_SERIAL: Cell<usize> = const { Cell::new(1) };
}

#[derive(Clone, Copy)]
struct SortGuardEntry {
    list_ptr: usize,
    serial: usize,
    mutated: bool,
}

pub(crate) struct ListSortGuard {
    serial: usize,
}

impl Drop for ListSortGuard {
    fn drop(&mut self) {
        SORT_GUARDS.with(|guards| {
            let mut guards = guards.borrow_mut();
            if let Some(pos) = guards.iter().rposition(|entry| entry.serial == self.serial) {
                guards.remove(pos);
            }
        });
    }
}

pub(crate) fn begin_list_sort_guard(list: &PyObjectRef) -> ListSortGuard {
    let list_ptr = PyObjectRef::as_ptr(list) as usize;
    let serial = NEXT_SORT_GUARD_SERIAL.with(|next| {
        let serial = next.get();
        next.set(serial.wrapping_add(1).max(1));
        serial
    });
    SORT_GUARDS.with(|guards| {
        guards.borrow_mut().push(SortGuardEntry {
            list_ptr,
            serial,
            mutated: false,
        });
    });
    ListSortGuard { serial }
}

pub(crate) fn mark_list_mutated(list: &PyObjectRef) {
    let list_ptr = PyObjectRef::as_ptr(list) as usize;
    SORT_GUARDS.with(|guards| {
        for entry in guards.borrow_mut().iter_mut() {
            if entry.list_ptr == list_ptr {
                entry.mutated = true;
            }
        }
    });
}

pub(crate) fn is_list_sort_active(list: &PyObjectRef) -> bool {
    let list_ptr = PyObjectRef::as_ptr(list) as usize;
    SORT_GUARDS.with(|guards| {
        guards
            .borrow()
            .iter()
            .any(|entry| entry.list_ptr == list_ptr)
    })
}

pub(crate) fn list_sort_was_mutated(guard: &ListSortGuard) -> bool {
    SORT_GUARDS.with(|guards| {
        guards
            .borrow()
            .iter()
            .find(|entry| entry.serial == guard.serial)
            .map(|entry| entry.mutated)
            .unwrap_or(false)
    })
}

impl VirtualMachine {
    /// Sort items using VM-level comparison (supports custom __lt__).
    pub fn vm_sort(&mut self, items: &mut Vec<PyObjectRef>) -> PyResult<()> {
        let n = items.len();
        if n <= 1 {
            return Ok(());
        }
        if self.sort_fast_primitives(items) {
            return Ok(());
        }
        // Bottom-up merge sort with VM-level __lt__ calls — O(n log n)
        let mut aux = items.clone();
        let mut width = 1usize;
        while width < n {
            let mut i = 0;
            while i < n {
                let mid = (i + width).min(n);
                let end = (i + 2 * width).min(n);
                // Merge items[i..mid] and items[mid..end] into aux[i..end]
                let (mut left, mut right) = (i, mid);
                for k in i..end {
                    if left < mid && (right >= end || !self.vm_lt(&items[right], &items[left])?) {
                        aux[k] = items[left].clone();
                        left += 1;
                    } else {
                        aux[k] = items[right].clone();
                        right += 1;
                    }
                }
                i += 2 * width;
            }
            items.clone_from_slice(&aux);
            width *= 2;
        }
        Ok(())
    }

    fn sort_fast_primitives(&self, items: &mut [PyObjectRef]) -> bool {
        if items.iter().all(|x| {
            matches!(
                &x.payload,
                PyObjectPayload::Int(ferrython_core::types::PyInt::Small(_))
            )
        }) {
            items.sort_by(|a, b| {
                let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(av)) = &a.payload
                else {
                    unreachable!()
                };
                let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(bv)) = &b.payload
                else {
                    unreachable!()
                };
                av.cmp(bv)
            });
            return true;
        }
        if items
            .iter()
            .all(|x| matches!(&x.payload, PyObjectPayload::Str(_)))
        {
            items.sort_by(|a, b| {
                let PyObjectPayload::Str(av) = &a.payload else {
                    unreachable!()
                };
                let PyObjectPayload::Str(bv) = &b.payload else {
                    unreachable!()
                };
                av.cmp(bv)
            });
            return true;
        }
        if items
            .iter()
            .all(|x| matches!(&x.payload, PyObjectPayload::Bytes(_)))
        {
            items.sort_by(|a, b| {
                let PyObjectPayload::Bytes(av) = &a.payload else {
                    unreachable!()
                };
                let PyObjectPayload::Bytes(bv) = &b.payload else {
                    unreachable!()
                };
                av.cmp(bv)
            });
            return true;
        }
        false
    }

    /// Compare two objects using Python's `<` semantics.
    pub(crate) fn vm_lt(&mut self, a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
        self.vm_rich_lt(a, b)
    }

    fn vm_rich_lt(&mut self, a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if x.is_nan() || y.is_nan() => {
                return Ok(false);
            }
            (PyObjectPayload::Tuple(a_items), PyObjectPayload::Tuple(b_items)) => {
                return self.vm_sequence_lt(a_items, b_items);
            }
            (PyObjectPayload::List(a_items), PyObjectPayload::List(b_items)) => {
                let left = a_items.read().clone();
                let right = b_items.read().clone();
                return self.vm_sequence_lt(&left, &right);
            }
            _ => {}
        }
        if let PyObjectPayload::Instance(_) = &a.payload {
            if let Some(method) = self.sort_instance_lt_method(a) {
                let result = self.call_object(method, vec![b.clone()])?;
                if matches!(&result.payload, PyObjectPayload::NotImplemented) {
                    return Err(sort_type_error(a, b));
                }
                return Ok(result.is_truthy());
            }
            if let Some((left, right)) = self.sort_builtin_value_operands(a, b) {
                return self.vm_rich_lt(&left, &right);
            }
        }
        if let Some(ordering) = builtins::partial_cmp_for_sort(a, b) {
            return Ok(ordering == std::cmp::Ordering::Less);
        }
        let result = a.compare(b, CompareOp::Lt)?;
        if result.is_truthy() {
            Ok(true)
        } else if matches!(
            (&a.payload, &b.payload),
            (PyObjectPayload::Instance(_), _) | (_, PyObjectPayload::Instance(_))
        ) {
            let ge = a.compare(b, CompareOp::Ge)?;
            if ge.is_truthy() {
                Ok(false)
            } else {
                Err(sort_type_error(a, b))
            }
        } else {
            Err(sort_type_error(a, b))
        }
    }

    fn vm_sequence_lt(&mut self, left: &[PyObjectRef], right: &[PyObjectRef]) -> PyResult<bool> {
        for (a_item, b_item) in left.iter().zip(right.iter()) {
            if a_item.compare(b_item, CompareOp::Eq)?.is_truthy() {
                continue;
            }
            return self.vm_rich_lt(a_item, b_item);
        }
        Ok(left.len() < right.len())
    }

    fn sort_instance_lt_method(&self, obj: &PyObjectRef) -> Option<PyObjectRef> {
        let PyObjectPayload::Instance(inst) = &obj.payload else {
            return None;
        };
        let effective_class = inst
            .attrs
            .read()
            .get("__class__")
            .filter(|class| matches!(&class.payload, PyObjectPayload::Class(_)))
            .cloned()
            .unwrap_or_else(|| inst.class.clone());
        if let Some(method) = lookup_in_class_mro(&effective_class, "__lt__") {
            return Some(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: obj.clone(),
                    method,
                },
            }));
        }
        obj.get_attr("__lt__").and_then(|method| {
            if matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                None
            } else {
                Some(method)
            }
        })
    }

    fn sort_builtin_value_operands(
        &self,
        a: &PyObjectRef,
        b: &PyObjectRef,
    ) -> Option<(PyObjectRef, PyObjectRef)> {
        let a_value = sort_sequence_backing(a);
        let b_value = sort_sequence_backing(b);
        match (a_value, b_value) {
            (Some(left), Some(right))
                if sort_is_sequence_value(&left) && sort_is_sequence_value(&right) =>
            {
                Some((left, right))
            }
            (Some(left), None) if sort_is_sequence_value(&left) && sort_is_sequence_value(b) => {
                Some((left, b.clone()))
            }
            (None, Some(right)) if sort_is_sequence_value(a) && sort_is_sequence_value(&right) => {
                Some((a.clone(), right))
            }
            _ => None,
        }
    }

    // ── Post-call intercept for VM-aware builtins ────────────────────────

    /// After every function call, check for deferred VM-aware operations.
    /// This handles builtins that need VM access but are called through the
    /// generic NativeFunction path (which doesn't pass &mut self).
    pub(crate) fn post_call_intercept(&mut self, mut result: PyObjectRef) -> PyResult<PyObjectRef> {
        // Fast path: skip all thread-local checks when no intercept is pending
        if !ferrython_core::object::check_intercept_pending() {
            return Ok(result);
        }
        // asyncio.run() intercept: drive coroutine to completion
        if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
            result = self.maybe_await_result(coro)?;
        }
        // __import__() intercept: resolve and load module
        if let Some(req) = crate::builtins::take_import_request() {
            result = self.import_module_simple(&req.name, req.level)?;
        }
        // importlib.import_module() intercept
        if let Some(req) = ferrython_stdlib::take_import_module_request() {
            let (name, level) = if req.name.starts_with('.') {
                let dots = req.name.chars().take_while(|c| *c == '.').count();
                let rest = &req.name[dots..];
                if let Some(ref pkg) = req.package {
                    let abs_name = if rest.is_empty() {
                        pkg.to_string()
                    } else {
                        format!("{}.{}", pkg, rest)
                    };
                    (abs_name, dots)
                } else {
                    (rest.to_string(), dots)
                }
            } else {
                (req.name.to_string(), 0)
            };
            result = self.import_module_simple(&name, level)?;
        }
        // importlib.reload() intercept
        if let Some(req) = ferrython_stdlib::take_reload_request() {
            result = self.reload_module(req.module)?;
        }
        let deferred = ferrython_stdlib::drain_deferred_calls();
        for (func, args) in deferred {
            self.call_object(func, args)?;
        }
        self.drain_pending_finalizers();
        Ok(result)
    }
}

fn sort_type_error(a: &PyObjectRef, b: &PyObjectRef) -> PyException {
    PyException::type_error(format!(
        "'<' not supported between instances of '{}' and '{}'",
        a.type_name(),
        b.type_name()
    ))
}

fn sort_is_sequence_value(obj: &PyObjectRef) -> bool {
    matches!(
        &obj.payload,
        PyObjectPayload::Tuple(_) | PyObjectPayload::List(_)
    )
}

fn sort_sequence_backing(obj: &PyObjectRef) -> Option<PyObjectRef> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    let attrs = inst.attrs.read();
    attrs
        .get("__builtin_value__")
        .or_else(|| attrs.get("_tuple"))
        .cloned()
}
