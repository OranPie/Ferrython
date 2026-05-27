//! Interned method-name cache for VM dispatch fast paths.

use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef, StrRepr};
use std::sync::OnceLock;

// ── Interned method name singletons ──
// Module-level statics for hot method names — enables pointer-identity comparison
// in CallMethodPopTop to skip string comparison in the fast path.
macro_rules! define_interned {
    ($id:ident, $s:literal) => {
        static $id: OnceLock<PyObjectRef> = OnceLock::new();
    };
}
define_interned!(N_APPEND, "append");
define_interned!(N_POP, "pop");
define_interned!(N_GET, "get");
define_interned!(N_SET, "set");
define_interned!(N_ADD, "add");
define_interned!(N_STRIP, "strip");
define_interned!(N_LSTRIP, "lstrip");
define_interned!(N_RSTRIP, "rstrip");
define_interned!(N_LOWER, "lower");
define_interned!(N_UPPER, "upper");
define_interned!(N_STARTSWITH, "startswith");
define_interned!(N_ENDSWITH, "endswith");
define_interned!(N_EXTEND, "extend");
define_interned!(N_INSERT, "insert");
define_interned!(N_REMOVE, "remove");
define_interned!(N_SORT, "sort");
define_interned!(N_REVERSE, "reverse");
define_interned!(N_COPY, "copy");
define_interned!(N_CLEAR, "clear");
define_interned!(N_UPDATE, "update");
define_interned!(N_ITEMS, "items");
define_interned!(N_KEYS, "keys");
define_interned!(N_VALUES, "values");
define_interned!(N_JOIN, "join");
define_interned!(N_SPLIT, "split");
define_interned!(N_REPLACE, "replace");
define_interned!(N_FIND, "find");
define_interned!(N_RFIND, "rfind");
define_interned!(N_INDEX, "index");
define_interned!(N_COUNT, "count");
define_interned!(N_FORMAT, "format");
define_interned!(N_ENCODE, "encode");
define_interned!(N_DECODE, "decode");
define_interned!(N_WRITE, "write");
define_interned!(N_READ, "read");
define_interned!(N_CLOSE, "close");

#[inline(always)]
fn init_interned<'a>(lock: &'a OnceLock<PyObjectRef>, name: &str) -> &'a PyObjectRef {
    lock.get_or_init(|| {
        PyObjectRef::new_immortal(PyObject {
            payload: PyObjectPayload::Str(StrRepr::from_bytes(name.as_bytes())),
        })
    })
}

/// Cached PyObjectRef for common method names — avoids heap allocation in LoadMethod
/// for builtin type method calls. Each entry is allocated once (OnceLock) and
/// subsequent uses are just pointer clones (immortal, no refcount).
#[inline]
pub(crate) fn cached_method_name(name: &str) -> Option<PyObjectRef> {
    match name {
        "append" => Some(init_interned(&N_APPEND, "append").clone()),
        "pop" => Some(init_interned(&N_POP, "pop").clone()),
        "get" => Some(init_interned(&N_GET, "get").clone()),
        "set" => Some(init_interned(&N_SET, "set").clone()),
        "add" => Some(init_interned(&N_ADD, "add").clone()),
        "strip" => Some(init_interned(&N_STRIP, "strip").clone()),
        "lstrip" => Some(init_interned(&N_LSTRIP, "lstrip").clone()),
        "rstrip" => Some(init_interned(&N_RSTRIP, "rstrip").clone()),
        "lower" => Some(init_interned(&N_LOWER, "lower").clone()),
        "upper" => Some(init_interned(&N_UPPER, "upper").clone()),
        "startswith" => Some(init_interned(&N_STARTSWITH, "startswith").clone()),
        "endswith" => Some(init_interned(&N_ENDSWITH, "endswith").clone()),
        "extend" => Some(init_interned(&N_EXTEND, "extend").clone()),
        "insert" => Some(init_interned(&N_INSERT, "insert").clone()),
        "remove" => Some(init_interned(&N_REMOVE, "remove").clone()),
        "sort" => Some(init_interned(&N_SORT, "sort").clone()),
        "reverse" => Some(init_interned(&N_REVERSE, "reverse").clone()),
        "copy" => Some(init_interned(&N_COPY, "copy").clone()),
        "clear" => Some(init_interned(&N_CLEAR, "clear").clone()),
        "update" => Some(init_interned(&N_UPDATE, "update").clone()),
        "items" => Some(init_interned(&N_ITEMS, "items").clone()),
        "keys" => Some(init_interned(&N_KEYS, "keys").clone()),
        "values" => Some(init_interned(&N_VALUES, "values").clone()),
        "join" => Some(init_interned(&N_JOIN, "join").clone()),
        "split" => Some(init_interned(&N_SPLIT, "split").clone()),
        "replace" => Some(init_interned(&N_REPLACE, "replace").clone()),
        "find" => Some(init_interned(&N_FIND, "find").clone()),
        "rfind" => Some(init_interned(&N_RFIND, "rfind").clone()),
        "index" => Some(init_interned(&N_INDEX, "index").clone()),
        "count" => Some(init_interned(&N_COUNT, "count").clone()),
        "format" => Some(init_interned(&N_FORMAT, "format").clone()),
        "encode" => Some(init_interned(&N_ENCODE, "encode").clone()),
        "decode" => Some(init_interned(&N_DECODE, "decode").clone()),
        "write" => Some(init_interned(&N_WRITE, "write").clone()),
        "read" => Some(init_interned(&N_READ, "read").clone()),
        "close" => Some(init_interned(&N_CLOSE, "close").clone()),
        _ => None,
    }
}

/// Fast pointer-identity check: is this PyObjectRef the interned "append" name?
#[inline(always)]
pub(crate) fn is_interned_append(obj: &PyObjectRef) -> bool {
    N_APPEND
        .get()
        .map_or(false, |c| PyObjectRef::ptr_eq(obj, c))
}

/// Fast pointer-identity check: is this PyObjectRef the interned "pop" name?
#[inline(always)]
pub(crate) fn is_interned_pop(obj: &PyObjectRef) -> bool {
    N_POP.get().map_or(false, |c| PyObjectRef::ptr_eq(obj, c))
}
