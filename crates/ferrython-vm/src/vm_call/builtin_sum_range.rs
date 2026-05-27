use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectRef, RangeIterData};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn sum_range(
        &mut self,
        mut total: PyObjectRef,
        start: i64,
        stop: i64,
        step: i64,
    ) -> PyResult<PyObjectRef> {
        let n = range_len(start, stop, step);
        if n > 0 {
            let range_sum = arithmetic_progression_sum(start, step, n);
            total = self.vm_add(&total, &PyObject::int(range_sum))?;
        }
        Ok(total)
    }

    pub(super) fn sum_range_iter(
        &mut self,
        total: PyObjectRef,
        iter: &RangeIterData,
    ) -> PyResult<PyObjectRef> {
        let current = iter.current.get();
        let n = range_len(current, iter.stop, iter.step);
        let total = self.sum_range(total, current, iter.stop, iter.step)?;
        if n > 0 {
            iter.current.set(current + iter.step * n);
        }
        Ok(total)
    }
}

fn range_len(start: i64, stop: i64, step: i64) -> i64 {
    if step > 0 {
        if stop > start {
            (stop - start - 1) / step + 1
        } else {
            0
        }
    } else if step < 0 {
        if start > stop {
            (start - stop - 1) / (-step) + 1
        } else {
            0
        }
    } else {
        0
    }
}

fn arithmetic_progression_sum(start: i64, step: i64, len: i64) -> i64 {
    len.wrapping_mul(start)
        .wrapping_add(step.wrapping_mul(len).wrapping_mul(len - 1) / 2)
}
