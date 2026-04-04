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
