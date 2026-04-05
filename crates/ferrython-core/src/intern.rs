//! Pre-interned strings for commonly used Python identifiers.
//!
//! Using these constants instead of `CompactString::from("__init__")` avoids
//! repeated allocations on every attribute lookup, method dispatch, and opcode
//! execution. CompactString inlines strings ≤24 bytes on the stack, so these
//! are effectively free to clone (memcpy, no heap).

use compact_str::CompactString;
use std::sync::LazyLock;

/// Macro to define a pre-interned CompactString constant.
macro_rules! intern {
    ($name:ident, $val:expr) => {
        pub static $name: LazyLock<CompactString> = LazyLock::new(|| CompactString::from($val));
    };
}

// ── Dunder methods: object protocol ──
intern!(INIT, "__init__");
intern!(NEW, "__new__");
intern!(DEL, "__del__");
intern!(REPR, "__repr__");
intern!(STR, "__str__");
intern!(BYTES, "__bytes__");
intern!(FORMAT, "__format__");
intern!(HASH, "__hash__");
intern!(BOOL, "__bool__");
intern!(SIZEOF, "__sizeof__");

// ── Dunder methods: comparison ──
intern!(EQ, "__eq__");
intern!(NE, "__ne__");
intern!(LT, "__lt__");
intern!(LE, "__le__");
intern!(GT, "__gt__");
intern!(GE, "__ge__");

// ── Dunder methods: arithmetic ──
intern!(ADD, "__add__");
intern!(RADD, "__radd__");
intern!(SUB, "__sub__");
intern!(RSUB, "__rsub__");
intern!(MUL, "__mul__");
intern!(RMUL, "__rmul__");
intern!(TRUEDIV, "__truediv__");
intern!(RTRUEDIV, "__rtruediv__");
intern!(FLOORDIV, "__floordiv__");
intern!(RFLOORDIV, "__rfloordiv__");
intern!(MOD, "__mod__");
intern!(RMOD, "__rmod__");
intern!(POW, "__pow__");
intern!(RPOW, "__rpow__");
intern!(NEG, "__neg__");
intern!(POS, "__pos__");
intern!(ABS, "__abs__");
intern!(INVERT, "__invert__");

// ── Dunder methods: augmented assignment ──
intern!(IADD, "__iadd__");
intern!(ISUB, "__isub__");
intern!(IMUL, "__imul__");
intern!(ITRUEDIV, "__itruediv__");
intern!(IFLOORDIV, "__ifloordiv__");
intern!(IMOD, "__imod__");
intern!(IPOW, "__ipow__");

// ── Dunder methods: bitwise ──
intern!(AND, "__and__");
intern!(RAND, "__rand__");
intern!(OR, "__or__");
intern!(ROR, "__ror__");
intern!(XOR, "__xor__");
intern!(RXOR, "__rxor__");
intern!(LSHIFT, "__lshift__");
intern!(RLSHIFT, "__rlshift__");
intern!(RSHIFT, "__rshift__");
intern!(RRSHIFT, "__rrshift__");
intern!(IAND, "__iand__");
intern!(IOR, "__ior__");
intern!(IXOR, "__ixor__");
intern!(ILSHIFT, "__ilshift__");
intern!(IRSHIFT, "__irshift__");

// ── Dunder methods: container ──
intern!(LEN, "__len__");
intern!(GETITEM, "__getitem__");
intern!(SETITEM, "__setitem__");
intern!(DELITEM, "__delitem__");
intern!(CONTAINS, "__contains__");
intern!(MISSING, "__missing__");

// ── Dunder methods: iteration ──
intern!(ITER, "__iter__");
intern!(NEXT, "__next__");
intern!(REVERSED, "__reversed__");

// ── Dunder methods: context manager ──
intern!(ENTER, "__enter__");
intern!(EXIT, "__exit__");
intern!(AENTER, "__aenter__");
intern!(AEXIT, "__aexit__");

// ── Dunder methods: callable / descriptor ──
intern!(CALL, "__call__");
intern!(GET, "__get__");
intern!(SET, "__set__");
intern!(DELETE, "__delete__");
intern!(SET_NAME, "__set_name__");
intern!(INIT_SUBCLASS, "__init_subclass__");

// ── Dunder methods: type conversion ──
intern!(INT, "__int__");
intern!(FLOAT, "__float__");
intern!(COMPLEX, "__complex__");
intern!(INDEX, "__index__");
intern!(ROUND, "__round__");
intern!(TRUNC, "__trunc__");
intern!(FLOOR, "__floor__");
intern!(CEIL, "__ceil__");

// ── Dunder methods: attribute access ──
intern!(GETATTR, "__getattr__");
intern!(GETATTRIBUTE, "__getattribute__");
intern!(SETATTR, "__setattr__");
intern!(DELATTR, "__delattr__");
intern!(DIR, "__dir__");

// ── Dunder methods: class infrastructure ──
intern!(CLASS, "__class__");
intern!(DICT, "__dict__");
intern!(SLOTS, "__slots__");
intern!(DOC, "__doc__");
intern!(NAME, "__name__");
intern!(QUALNAME, "__qualname__");
intern!(MODULE, "__module__");
intern!(BASES, "__bases__");
intern!(MRO_ATTR, "__mro__");
intern!(SUBCLASSES, "__subclasses__");
intern!(METACLASS, "__metaclass__");
intern!(ABSTRACTMETHODS, "__abstractmethods__");
intern!(INSTANCECHECK, "__instancecheck__");
intern!(SUBCLASSCHECK, "__subclasscheck__");

// ── Dunder methods: pickling / copying ──
intern!(REDUCE, "__reduce__");
intern!(REDUCE_EX, "__reduce_ex__");
intern!(GETSTATE, "__getstate__");
intern!(SETSTATE, "__setstate__");
intern!(COPY, "__copy__");
intern!(DEEPCOPY, "__deepcopy__");

// ── Dunder methods: async ──
intern!(AWAIT, "__await__");
intern!(AITER, "__aiter__");
intern!(ANEXT, "__anext__");

// ── Dunder methods: math ──
intern!(MATMUL, "__matmul__");
intern!(RMATMUL, "__rmatmul__");
intern!(IMATMUL, "__imatmul__");
intern!(DIVMOD, "__divmod__");
intern!(RDIVMOD, "__rdivmod__");

// ── Common attribute names ──
intern!(SELF, "self");
intern!(CLS, "cls");
intern!(ARGS, "args");
intern!(KWARGS, "kwargs");
intern!(MESSAGE, "message");

// ── Built-in names ──
intern!(NONE_STR, "None");
intern!(TRUE_STR, "True");
intern!(FALSE_STR, "False");

/// Try to return a pre-interned CompactString for a given name.
/// Returns `Some(cloned_interned)` for known dunder names, `None` otherwise.
/// This avoids `CompactString::from(name)` allocations in hot paths like
/// method resolution cache insertions.
pub fn try_intern(name: &str) -> Option<CompactString> {
    // Only try interning for dunder names (most cache insertions)
    if !(name.starts_with("__") && name.ends_with("__") && name.len() > 4) {
        return None;
    }
    // Match against the most frequently looked-up dunders first
    match name {
        "__class__" => Some(CLASS.clone()),
        "__init__" => Some(INIT.clone()),
        "__new__" => Some(NEW.clone()),
        "__dict__" => Some(DICT.clone()),
        "__name__" => Some(NAME.clone()),
        "__str__" => Some(STR.clone()),
        "__repr__" => Some(REPR.clone()),
        "__hash__" => Some(HASH.clone()),
        "__eq__" => Some(EQ.clone()),
        "__ne__" => Some(NE.clone()),
        "__lt__" => Some(LT.clone()),
        "__le__" => Some(LE.clone()),
        "__gt__" => Some(GT.clone()),
        "__ge__" => Some(GE.clone()),
        "__add__" => Some(ADD.clone()),
        "__sub__" => Some(SUB.clone()),
        "__mul__" => Some(MUL.clone()),
        "__truediv__" => Some(TRUEDIV.clone()),
        "__floordiv__" => Some(FLOORDIV.clone()),
        "__mod__" => Some(MOD.clone()),
        "__pow__" => Some(POW.clone()),
        "__neg__" => Some(NEG.clone()),
        "__pos__" => Some(POS.clone()),
        "__abs__" => Some(ABS.clone()),
        "__invert__" => Some(INVERT.clone()),
        "__bool__" => Some(BOOL.clone()),
        "__len__" => Some(LEN.clone()),
        "__getitem__" => Some(GETITEM.clone()),
        "__setitem__" => Some(SETITEM.clone()),
        "__delitem__" => Some(DELITEM.clone()),
        "__contains__" => Some(CONTAINS.clone()),
        "__iter__" => Some(ITER.clone()),
        "__next__" => Some(NEXT.clone()),
        "__call__" => Some(CALL.clone()),
        "__enter__" => Some(ENTER.clone()),
        "__exit__" => Some(EXIT.clone()),
        "__get__" => Some(GET.clone()),
        "__set__" => Some(SET.clone()),
        "__delete__" => Some(DELETE.clone()),
        "__int__" => Some(INT.clone()),
        "__float__" => Some(FLOAT.clone()),
        "__index__" => Some(INDEX.clone()),
        "__getattr__" => Some(GETATTR.clone()),
        "__setattr__" => Some(SETATTR.clone()),
        "__delattr__" => Some(DELATTR.clone()),
        "__module__" => Some(MODULE.clone()),
        "__qualname__" => Some(QUALNAME.clone()),
        "__bases__" => Some(BASES.clone()),
        "__mro__" => Some(MRO_ATTR.clone()),
        "__doc__" => Some(DOC.clone()),
        "__slots__" => Some(SLOTS.clone()),
        "__radd__" => Some(RADD.clone()),
        "__rsub__" => Some(RSUB.clone()),
        "__rmul__" => Some(RMUL.clone()),
        "__iadd__" => Some(IADD.clone()),
        "__isub__" => Some(ISUB.clone()),
        "__imul__" => Some(IMUL.clone()),
        "__and__" => Some(AND.clone()),
        "__or__" => Some(OR.clone()),
        "__xor__" => Some(XOR.clone()),
        "__lshift__" => Some(LSHIFT.clone()),
        "__rshift__" => Some(RSHIFT.clone()),
        "__missing__" => Some(MISSING.clone()),
        "__reversed__" => Some(REVERSED.clone()),
        "__format__" => Some(FORMAT.clone()),
        "__sizeof__" => Some(SIZEOF.clone()),
        "__del__" => Some(DEL.clone()),
        "__bytes__" => Some(BYTES.clone()),
        "__set_name__" => Some(SET_NAME.clone()),
        "__init_subclass__" => Some(INIT_SUBCLASS.clone()),
        "__instancecheck__" => Some(INSTANCECHECK.clone()),
        "__subclasscheck__" => Some(SUBCLASSCHECK.clone()),
        "__await__" => Some(AWAIT.clone()),
        "__aenter__" => Some(AENTER.clone()),
        "__aexit__" => Some(AEXIT.clone()),
        "__aiter__" => Some(AITER.clone()),
        "__anext__" => Some(ANEXT.clone()),
        _ => None,
    }
}
