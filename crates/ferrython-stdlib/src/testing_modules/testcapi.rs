use ferrython_core::object::{make_module, PyObject, PyObjectRef};
use num_bigint::BigInt;

// ── _testcapi module ──

pub fn create_testcapi_module() -> PyObjectRef {
    make_module(
        "_testcapi",
        vec![
            ("INT_MIN", PyObject::int(i32::MIN as i64)),
            ("INT_MAX", PyObject::int(i32::MAX as i64)),
            ("UINT_MAX", PyObject::int(u32::MAX as i64)),
            ("LONG_MIN", PyObject::int(libc::c_long::MIN as i64)),
            ("LONG_MAX", PyObject::int(libc::c_long::MAX as i64)),
            (
                "ULONG_MAX",
                PyObject::big_int(BigInt::from(libc::c_ulong::MAX)),
            ),
            ("PY_SSIZE_T_MIN", PyObject::int(isize::MIN as i64)),
            ("PY_SSIZE_T_MAX", PyObject::int(isize::MAX as i64)),
            ("SIZEOF_PYGC_HEAD", PyObject::int(16)),
        ],
    )
}
