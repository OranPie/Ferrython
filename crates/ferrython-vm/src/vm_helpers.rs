//! VM utility functions — iteration, sorting, and call hooks.

use crate::builtins;
use crate::VirtualMachine;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObjectMethods, PyObjectPayload, PyObjectRef};

mod iter_next;
mod iterables;

impl VirtualMachine {
    /// Sort items using VM-level comparison (supports custom __lt__).
    /// Uses insertion sort to allow &mut self access during comparisons.
    pub fn vm_sort(&mut self, items: &mut Vec<PyObjectRef>) -> PyResult<()> {
        let n = items.len();
        if n <= 1 {
            return Ok(());
        }
        // Fast check: peek first element to decide sort strategy.
        // If first is not an Instance, likely all are homogeneous primitives.
        let first_is_instance = matches!(&items[0].payload, PyObjectPayload::Instance(_));
        if !first_is_instance {
            // Homogeneous small-int: extract i64, sort natively, skip per-comparison matching
            if matches!(
                &items[0].payload,
                PyObjectPayload::Int(ferrython_core::types::PyInt::Small(_))
            ) {
                let all_small = items.iter().all(|x| {
                    matches!(
                        &x.payload,
                        PyObjectPayload::Int(ferrython_core::types::PyInt::Small(_))
                    )
                });
                if all_small {
                    items.sort_unstable_by(|a, b| {
                        let av =
                            if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(v)) =
                                &a.payload
                            {
                                *v
                            } else {
                                0
                            };
                        let bv =
                            if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(v)) =
                                &b.payload
                            {
                                *v
                            } else {
                                0
                            };
                        av.cmp(&bv)
                    });
                    return Ok(());
                }
            }
            items.sort_unstable_by(|a, b| {
                builtins::partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
            });
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

    /// Compare two objects using __lt__, falling back to native comparison.
    pub(crate) fn vm_lt(&mut self, a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
        use ferrython_core::object::CompareOp;
        if let PyObjectPayload::Instance(_inst) = &a.payload {
            if let Some(method) = a.get_attr("__lt__") {
                let call_res = if matches!(
                    &method.payload,
                    PyObjectPayload::NativeFunction(_)
                        | PyObjectPayload::NativeClosure(_)
                        | PyObjectPayload::Function(_)
                ) {
                    self.call_object(method, vec![a.clone(), b.clone()])
                } else {
                    self.call_object(method, vec![b.clone()])
                };
                if let Ok(result) = call_res {
                    return Ok(result.is_truthy());
                }
                // Bound builtin-method dispatch may fail (e.g. tuple subclasses
                // where __lt__ isn't wired for Instance receivers). Fall through
                // to generic compare, which handles tuple-subclass ordering.
            }
            return Ok(a
                .compare(b, CompareOp::Lt)
                .map(|v| v.is_truthy())
                .unwrap_or(false));
        }
        Ok(builtins::partial_cmp_for_sort(a, b) == Some(std::cmp::Ordering::Less))
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
        self.drain_pending_finalizers();
        Ok(result)
    }
}
