//! PyObjectMethods trait — thin dispatch to sub-modules.

use crate::error::PyResult;
use crate::types::HashableKey;
use compact_str::CompactString;

use super::payload::PyObjectRef;

// ── Extension trait for methods on PyObjectRef ──

pub trait PyObjectMethods {
    fn type_name(&self) -> &'static str;
    fn is_truthy(&self) -> bool;
    fn is_callable(&self) -> bool;
    fn is_same(&self, other: &Self) -> bool;
    fn py_to_string(&self) -> String;
    fn repr(&self) -> String;
    fn to_list(&self) -> PyResult<Vec<PyObjectRef>>;
    fn to_int(&self) -> PyResult<i64>;
    fn to_float(&self) -> PyResult<f64>;
    fn as_int(&self) -> Option<i64>;
    fn as_str(&self) -> Option<&str>;
    fn to_hashable_key(&self) -> PyResult<HashableKey>;
    fn add(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn sub(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn mul(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn floor_div(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn true_div(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn modulo(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn power(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn lshift(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn rshift(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn bit_and(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn bit_or(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn bit_xor(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn negate(&self) -> PyResult<PyObjectRef>;
    fn positive(&self) -> PyResult<PyObjectRef>;
    fn invert(&self) -> PyResult<PyObjectRef>;
    fn py_abs(&self) -> PyResult<PyObjectRef>;
    fn compare(&self, other: &Self, op: CompareOp) -> PyResult<PyObjectRef>;
    fn get_attr(&self, name: &str) -> Option<PyObjectRef>;
    fn py_len(&self) -> PyResult<usize>;
    fn get_item(&self, key: &PyObjectRef) -> PyResult<PyObjectRef>;
    fn contains(&self, item: &PyObjectRef) -> PyResult<bool>;
    fn get_iter(&self) -> PyResult<PyObjectRef>;
    fn format_value(&self, spec: &str) -> PyResult<String>;
    fn dir(&self) -> Vec<CompactString>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp { Lt, Le, Eq, Ne, Gt, Ge }

impl PyObjectMethods for PyObjectRef {
    fn type_name(&self) -> &'static str { super::methods_type::py_type_name(self) }
    fn is_truthy(&self) -> bool { super::methods_type::py_is_truthy(self) }
    fn is_callable(&self) -> bool { super::methods_type::py_is_callable(self) }
    fn is_same(&self, other: &Self) -> bool { super::methods_type::py_is_same(self, other) }
    fn py_to_string(&self) -> String { super::methods_type::py_to_string(self) }
    fn repr(&self) -> String { super::methods_type::py_repr(self) }
    fn to_list(&self) -> PyResult<Vec<PyObjectRef>> { super::methods_type::py_to_list(self) }
    fn to_int(&self) -> PyResult<i64> { super::methods_type::py_to_int(self) }
    fn to_float(&self) -> PyResult<f64> { super::methods_type::py_to_float(self) }
    fn as_int(&self) -> Option<i64> { super::methods_type::py_as_int(self) }
    fn as_str(&self) -> Option<&str> { super::methods_type::py_as_str(self) }
    fn to_hashable_key(&self) -> PyResult<HashableKey> { super::methods_type::py_to_hashable_key(self) }
    fn add(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_add(self, other) }
    fn sub(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_sub(self, other) }
    fn mul(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_mul(self, other) }
    fn floor_div(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_floor_div(self, other) }
    fn true_div(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_true_div(self, other) }
    fn modulo(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_modulo(self, other) }
    fn power(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_power(self, other) }
    fn lshift(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_lshift(self, other) }
    fn rshift(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_rshift(self, other) }
    fn bit_and(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_bit_and(self, other) }
    fn bit_or(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_bit_or(self, other) }
    fn bit_xor(&self, other: &Self) -> PyResult<PyObjectRef> { super::methods_arith::py_bit_xor(self, other) }
    fn negate(&self) -> PyResult<PyObjectRef> { super::methods_arith::py_negate(self) }
    fn positive(&self) -> PyResult<PyObjectRef> { super::methods_arith::py_positive(self) }
    fn invert(&self) -> PyResult<PyObjectRef> { super::methods_arith::py_invert(self) }
    fn py_abs(&self) -> PyResult<PyObjectRef> { super::methods_arith::py_abs(self) }
    fn compare(&self, other: &Self, op: CompareOp) -> PyResult<PyObjectRef> { super::methods_compare::py_compare(self, other, op) }
    fn get_attr(&self, name: &str) -> Option<PyObjectRef> { super::methods_attr::py_get_attr(self, name) }
    fn py_len(&self) -> PyResult<usize> { super::methods_container::py_len(self) }
    fn get_item(&self, key: &PyObjectRef) -> PyResult<PyObjectRef> { super::methods_container::py_get_item(self, key) }
    fn contains(&self, item: &PyObjectRef) -> PyResult<bool> { super::methods_container::py_contains(self, item) }
    fn get_iter(&self) -> PyResult<PyObjectRef> { super::methods_container::py_get_iter(self) }
    fn format_value(&self, spec: &str) -> PyResult<String> { super::methods_format::py_format_value(self, spec) }
    fn dir(&self) -> Vec<CompactString> { super::methods_format::py_dir(self) }
}
