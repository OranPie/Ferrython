use compact_str::CompactString;
use ferrython_core::object::{make_module, PyObject, PyObjectRef};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;

// ── errno module ──

pub fn create_errno_module() -> PyObjectRef {
    let mut errorcode = IndexMap::new();
    let codes: Vec<(i64, &str)> = vec![
        (1, "EPERM"),
        (2, "ENOENT"),
        (3, "ESRCH"),
        (4, "EINTR"),
        (10, "ECHILD"),
        (11, "EAGAIN"),
        (13, "EACCES"),
        (17, "EEXIST"),
        (20, "ENOTDIR"),
        (21, "EISDIR"),
        (22, "EINVAL"),
        (32, "EPIPE"),
        (103, "ECONNABORTED"),
        (104, "ECONNRESET"),
        (108, "ESHUTDOWN"),
        (110, "ETIMEDOUT"),
        (111, "ECONNREFUSED"),
        (114, "EALREADY"),
        (115, "EINPROGRESS"),
    ];
    for (num, name) in codes {
        errorcode.insert(
            HashableKey::Int(PyInt::Small(num)),
            PyObject::str_val(CompactString::from(name)),
        );
    }
    make_module(
        "errno",
        vec![
            ("EPERM", PyObject::int(1)),
            ("ENOENT", PyObject::int(2)),
            ("ESRCH", PyObject::int(3)),
            ("EINTR", PyObject::int(4)),
            ("EIO", PyObject::int(5)),
            ("ENXIO", PyObject::int(6)),
            ("E2BIG", PyObject::int(7)),
            ("ENOEXEC", PyObject::int(8)),
            ("EBADF", PyObject::int(9)),
            ("ECHILD", PyObject::int(10)),
            ("EAGAIN", PyObject::int(11)),
            ("ENOMEM", PyObject::int(12)),
            ("EACCES", PyObject::int(13)),
            ("EFAULT", PyObject::int(14)),
            ("EBUSY", PyObject::int(16)),
            ("EEXIST", PyObject::int(17)),
            ("EXDEV", PyObject::int(18)),
            ("ENODEV", PyObject::int(19)),
            ("ENOTDIR", PyObject::int(20)),
            ("EISDIR", PyObject::int(21)),
            ("EINVAL", PyObject::int(22)),
            ("ENFILE", PyObject::int(23)),
            ("EMFILE", PyObject::int(24)),
            ("ENOTTY", PyObject::int(25)),
            ("EFBIG", PyObject::int(27)),
            ("ENOSPC", PyObject::int(28)),
            ("ESPIPE", PyObject::int(29)),
            ("EROFS", PyObject::int(30)),
            ("EMLINK", PyObject::int(31)),
            ("EPIPE", PyObject::int(32)),
            ("EDOM", PyObject::int(33)),
            ("ERANGE", PyObject::int(34)),
            ("EDEADLK", PyObject::int(35)),
            ("ENAMETOOLONG", PyObject::int(36)),
            ("ENOLCK", PyObject::int(37)),
            ("ENOSYS", PyObject::int(38)),
            ("ENOTEMPTY", PyObject::int(39)),
            ("ECONNREFUSED", PyObject::int(111)),
            ("ETIMEDOUT", PyObject::int(110)),
            ("EWOULDBLOCK", PyObject::int(11)),
            ("EINPROGRESS", PyObject::int(115)),
            ("EALREADY", PyObject::int(114)),
            ("ECONNRESET", PyObject::int(104)),
            ("ECONNABORTED", PyObject::int(103)),
            ("ENETUNREACH", PyObject::int(101)),
            ("EHOSTUNREACH", PyObject::int(113)),
            ("ENOTCONN", PyObject::int(107)),
            ("EADDRINUSE", PyObject::int(98)),
            ("EADDRNOTAVAIL", PyObject::int(99)),
            ("EISCONN", PyObject::int(106)),
            ("EPFNOSUPPORT", PyObject::int(96)),
            ("EAFNOSUPPORT", PyObject::int(97)),
            ("ENOBUFS", PyObject::int(105)),
            ("EPROTONOSUPPORT", PyObject::int(93)),
            ("ESHUTDOWN", PyObject::int(108)),
            ("EMSGSIZE", PyObject::int(90)),
            ("ENOTSOCK", PyObject::int(88)),
            ("EDESTADDRREQ", PyObject::int(89)),
            ("errorcode", PyObject::dict(errorcode)),
        ],
    )
}

// ── atexit module ──
