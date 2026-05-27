use compact_str::CompactString;
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyInt;

#[inline(always)]
pub(super) fn fast_exact_str(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Str(_) => Some(arg.clone()),
        PyObjectPayload::Int(PyInt::Small(n)) => {
            let mut buf = itoa::Buffer::new();
            Some(PyObject::str_val(CompactString::from(buf.format(*n))))
        }
        PyObjectPayload::Bool(b) => Some(PyObject::str_val(CompactString::from(if *b {
            "True"
        } else {
            "False"
        }))),
        PyObjectPayload::None => Some(PyObject::str_val(CompactString::from("None"))),
        _ => None,
    }
}
