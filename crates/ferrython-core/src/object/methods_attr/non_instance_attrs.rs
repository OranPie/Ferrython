use crate::error::PyException;
use crate::types::HashableKey;
use compact_str::CompactString;

use super::super::helpers::*;
use super::super::methods::PyObjectMethods;
use super::super::methods_attr_helpers::{code_object_co_code, iterator_supports_setstate};
use super::super::payload::*;
use super::{builtin_type, callable_attrs, class_attrs, exception_attrs, super_attrs};

pub(super) fn non_instance_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::Class(cd) => class_attrs::class_attr(obj, cd, name),
        PyObjectPayload::Module(m) => {
            if name == "__class__" {
                return Some(PyObject::builtin_type(CompactString::from("module")));
            }
            if name == "__dict__" {
                Some(PyObject::wrap(PyObjectPayload::InstanceDict(
                    m.attrs.clone(),
                )))
            } else {
                m.attrs.read().get(name).cloned()
            }
        }
        PyObjectPayload::Slice(sd) => match name {
            "start" => Some(sd.start.clone().unwrap_or_else(PyObject::none)),
            "stop" => Some(sd.stop.clone().unwrap_or_else(PyObject::none)),
            "step" => Some(sd.step.clone().unwrap_or_else(PyObject::none)),
            "__class__" => Some(PyObject::builtin_type(CompactString::from("slice"))),
            "indices" => {
                let s_start = sd.start.clone();
                let s_stop = sd.stop.clone();
                let s_step = sd.step.clone();
                Some(PyObject::native_closure("slice.indices", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "slice.indices() requires a length argument",
                        ));
                    }
                    let length = index_to_i64(&args[0])
                        .map_err(|_| PyException::type_error("length must be an integer"))?;
                    if length < 0 {
                        return Err(PyException::value_error("length should not be negative"));
                    }
                    let (start_val, stop_val, step_val) =
                        resolve_slice_i128(&s_start, &s_stop, &s_step, length as i128)?;
                    let int_obj = |value: i128| {
                        i64::try_from(value)
                            .map(PyObject::int)
                            .unwrap_or_else(|_| PyObject::big_int(value.into()))
                    };
                    Ok(PyObject::tuple(vec![
                        int_obj(start_val),
                        int_obj(stop_val),
                        int_obj(step_val),
                    ]))
                }))
            }
            _ => None,
        },
        PyObjectPayload::Complex { real, imag } => match name {
            "real" => Some(PyObject::float(*real)),
            "imag" => Some(PyObject::float(*imag)),
            "__class__" => Some(PyObject::builtin_type(CompactString::from("complex"))),
            "conjugate" => Some(bound_builtin(obj, "conjugate")),
            "__abs__" | "__neg__" | "__pos__" | "__bool__" | "__repr__" | "__str__"
            | "__hash__" | "__format__" | "__eq__" | "__ne__" | "__lt__" | "__le__" | "__gt__"
            | "__ge__" | "__add__" | "__sub__" | "__mul__" | "__truediv__" | "__floordiv__"
            | "__mod__" | "__pow__" | "__divmod__" | "__radd__" | "__rsub__" | "__rmul__"
            | "__rtruediv__" | "__rfloordiv__" | "__rmod__" | "__rpow__" | "__complex__"
            | "__getnewargs__" => Some(bound_builtin(obj, name)),
            _ => None,
        },
        PyObjectPayload::BuiltinType(n) => builtin_type::builtin_type_attr(obj, n, name),
        PyObjectPayload::Property(pd) => match name {
            "__doc__" => Some(pd.doc.read().clone().unwrap_or_else(PyObject::none)),
            "__isabstractmethod__" => {
                for func in [&pd.fget, &pd.fset, &pd.fdel].into_iter().flatten() {
                    if let Some(flag) = func.get_attr("__isabstractmethod__") {
                        if flag.is_truthy() {
                            return Some(PyObject::bool_val(true));
                        }
                    }
                }
                Some(PyObject::bool_val(false))
            }
            "fget" => pd.fget.clone().or_else(|| Some(PyObject::none())),
            "fset" => pd.fset.clone().or_else(|| Some(PyObject::none())),
            "fdel" => pd.fdel.clone().or_else(|| Some(PyObject::none())),
            "setter" | "getter" | "deleter" => Some(bound_builtin(obj, name)),
            _ => None,
        },
        PyObjectPayload::Partial(pd) => match name {
            "func" => Some(pd.func.clone()),
            "args" => Some(PyObject::tuple(pd.args.clone())),
            "keywords" => {
                let mut map = new_fx_hashkey_map();
                for (k, v) in &pd.kwargs {
                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                }
                Some(PyObject::dict(map))
            }
            _ => None,
        },
        PyObjectPayload::ExceptionType(kind) => {
            exception_attrs::exception_type_attr(obj, *kind, name)
        }
        PyObjectPayload::ExceptionInstance(ei) => {
            exception_attrs::exception_instance_attr(obj, ei, name)
        }
        PyObjectPayload::Function(f) => callable_attrs::function_attr(obj, f, name),
        PyObjectPayload::NativeFunction(nf) => callable_attrs::native_function_attr(obj, nf, name),
        PyObjectPayload::BuiltinFunction(fname) => {
            callable_attrs::builtin_function_attr(obj, fname, name)
        }
        PyObjectPayload::ClassMethod(func) => callable_attrs::classmethod_attr(func, name),
        PyObjectPayload::StaticMethod(func) => callable_attrs::staticmethod_attr(func, name),
        PyObjectPayload::BoundMethod { receiver, method } => {
            callable_attrs::bound_method_attr(receiver, method, name)
        }
        PyObjectPayload::Int(_) => int_attr(obj, name),
        PyObjectPayload::Float(value) => float_attr(obj, *value, name),
        PyObjectPayload::Bool(value) => bool_attr(obj, *value, name),
        PyObjectPayload::Range(rd) => range_attr(obj, rd, name),
        PyObjectPayload::Str(_) => named_builtin_attr(obj, "str", STR_METHODS, name),
        PyObjectPayload::List(_) => named_builtin_attr(obj, "list", LIST_METHODS, name),
        PyObjectPayload::Dict(_)
        | PyObjectPayload::InstanceDict(_)
        | PyObjectPayload::MappingProxy(_) => {
            let type_name = obj.type_name();
            named_builtin_attr(obj, type_name, DICT_METHODS, name)
        }
        PyObjectPayload::Tuple(_) => named_builtin_attr(obj, "tuple", TUPLE_METHODS, name),
        PyObjectPayload::Set(_) => named_builtin_attr(obj, "set", SET_METHODS, name),
        PyObjectPayload::FrozenSet(_) => {
            named_builtin_attr(obj, "frozenset", FROZENSET_METHODS, name)
        }
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => {
            let type_name = obj.type_name();
            named_builtin_attr(obj, type_name, BYTES_METHODS, name)
        }
        PyObjectPayload::None => none_attr(obj, name),
        PyObjectPayload::Generator(_) => generator_attr(obj, name),
        PyObjectPayload::Coroutine(_) => coroutine_attr(obj, name),
        PyObjectPayload::AsyncGenerator(_) => async_generator_attr(obj, name),
        PyObjectPayload::AsyncGenAwaitable { .. } => async_gen_awaitable_attr(obj, name),
        PyObjectPayload::Super { cls, instance } => {
            super_attrs::super_attr(obj, cls, instance, name)
        }
        PyObjectPayload::Code(code) => code_attr(code, name),
        PyObjectPayload::Cell(cell_ref) => match name {
            "cell_contents" => cell_ref.read().as_ref().cloned(),
            "__class__" => Some(PyObject::builtin_type(CompactString::from("cell"))),
            _ => None,
        },
        PyObjectPayload::Iterator(_)
        | PyObjectPayload::RangeIter(..)
        | PyObjectPayload::VecIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::DequeIter(_)
        | PyObjectPayload::RefIter { .. }
        | PyObjectPayload::RevRefIter { .. } => iterator_attr(obj, name),
        PyObjectPayload::BuiltinBoundMethod(_) => builtin_bound_method_attr(obj, name),
        PyObjectPayload::NativeClosure(nc) => native_closure_attr(obj, nc, name),
        _ => None,
    }
}

fn bound_builtin(obj: &PyObjectRef, name: &str) -> PyObjectRef {
    PyObjectRef::new(PyObject {
        payload: PyObjectPayload::BuiltinBoundMethod(super::super::constructors::alloc_bbm_box(
            obj.clone(),
            CompactString::from(name),
        )),
    })
}

fn named_builtin_attr(
    obj: &PyObjectRef,
    type_name: &str,
    methods: &[&str],
    name: &str,
) -> Option<PyObjectRef> {
    if name == "__class__" {
        return Some(PyObject::builtin_type(CompactString::from(type_name)));
    }
    methods.contains(&name).then(|| bound_builtin(obj, name))
}

fn int_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "real" | "numerator" => Some(PyObject::wrap(obj.payload.clone())),
        "imag" => Some(PyObject::int(0)),
        "denominator" => Some(PyObject::int(1)),
        "__class__" => Some(PyObject::builtin_type(CompactString::from("int"))),
        _ if INT_METHODS.contains(&name) => Some(bound_builtin(obj, name)),
        _ => None,
    }
}

fn float_attr(obj: &PyObjectRef, value: f64, name: &str) -> Option<PyObjectRef> {
    match name {
        "real" => Some(PyObject::float(value)),
        "imag" => Some(PyObject::float(0.0)),
        "__class__" => Some(PyObject::builtin_type(CompactString::from("float"))),
        _ if FLOAT_METHODS.contains(&name) => Some(bound_builtin(obj, name)),
        _ => None,
    }
}

fn bool_attr(obj: &PyObjectRef, value: bool, name: &str) -> Option<PyObjectRef> {
    match name {
        "real" | "numerator" => Some(PyObject::int(if value { 1 } else { 0 })),
        "imag" => Some(PyObject::int(0)),
        "denominator" => Some(PyObject::int(1)),
        "__class__" => Some(PyObject::builtin_type(CompactString::from("bool"))),
        _ if BOOL_METHODS.contains(&name) => Some(bound_builtin(obj, name)),
        _ => None,
    }
}

fn range_attr(obj: &PyObjectRef, rd: &RangeData, name: &str) -> Option<PyObjectRef> {
    match name {
        "start" => Some(
            rd.start_obj
                .clone()
                .unwrap_or_else(|| PyObject::int(rd.start)),
        ),
        "stop" => Some(
            rd.stop_obj
                .clone()
                .unwrap_or_else(|| PyObject::int(rd.stop)),
        ),
        "step" => Some(
            rd.step_obj
                .clone()
                .unwrap_or_else(|| PyObject::int(rd.step)),
        ),
        "__class__" => Some(PyObject::builtin_type(CompactString::from("range"))),
        _ if RANGE_METHODS.contains(&name) => Some(bound_builtin(obj, name)),
        _ => None,
    }
}

fn none_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "__class__" => Some(PyObject::builtin_type(CompactString::from("NoneType"))),
        "__bool__" | "__repr__" | "__str__" | "__hash__" | "__eq__" | "__ne__" => {
            Some(bound_builtin(obj, name))
        }
        _ => None,
    }
}

fn generator_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "send" | "throw" | "close" | "__next__" | "__enter__" | "__exit__" => {
            Some(bound_builtin(obj, name))
        }
        "__iter__" => Some(obj.clone()),
        "gi_frame" | "gi_code" => Some(PyObject::none()),
        "gi_running" => Some(PyObject::bool_val(false)),
        "gi_yieldfrom" => Some(PyObject::none()),
        _ => None,
    }
}

fn coroutine_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "send" | "throw" | "close" => Some(bound_builtin(obj, name)),
        "__await__" => Some(obj.clone()),
        "cr_frame" | "cr_code" => Some(PyObject::none()),
        "cr_running" => Some(PyObject::bool_val(false)),
        "cr_await" | "cr_origin" => Some(PyObject::none()),
        _ => None,
    }
}

fn async_generator_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "send" | "throw" | "close" | "__aiter__" | "__anext__" | "asend" | "athrow" | "aclose" => {
            Some(bound_builtin(obj, name))
        }
        "ag_frame" | "ag_code" => Some(PyObject::none()),
        "ag_running" => Some(PyObject::bool_val(false)),
        "ag_await" => Some(PyObject::none()),
        _ => None,
    }
}

fn async_gen_awaitable_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "__await__" => Some(obj.clone()),
        "send" | "throw" | "close" => Some(bound_builtin(obj, name)),
        _ => None,
    }
}

fn code_attr(code: &ferrython_bytecode::CodeObject, name: &str) -> Option<PyObjectRef> {
    match name {
        "co_name" => Some(PyObject::str_val(code.name.clone())),
        "co_qualname" => Some(PyObject::str_val(code.qualname.clone())),
        "co_filename" => Some(PyObject::str_val(code.filename.clone())),
        "co_firstlineno" => Some(PyObject::int(code.first_line_number as i64)),
        "co_argcount" => Some(PyObject::int(code.arg_count as i64)),
        "co_posonlyargcount" => Some(PyObject::int(code.posonlyarg_count as i64)),
        "co_kwonlyargcount" => Some(PyObject::int(code.kwonlyarg_count as i64)),
        "co_nlocals" => Some(PyObject::int(code.num_locals as i64)),
        "co_stacksize" => Some(PyObject::int(code.max_stack_size as i64)),
        "co_flags" => Some(PyObject::int(code.flags.bits() as i64)),
        "co_code" => Some(PyObject::bytes(code_object_co_code(code))),
        "co_lnotab" => Some(PyObject::bytes(Vec::new())),
        "co_varnames" => Some(PyObject::tuple(
            code.varnames
                .iter()
                .map(|s| PyObject::str_val(s.clone()))
                .collect(),
        )),
        "co_names" => Some(PyObject::tuple(
            code.names
                .iter()
                .map(|s| PyObject::str_val(s.clone()))
                .collect(),
        )),
        "co_freevars" => Some(PyObject::tuple(
            code.freevars
                .iter()
                .map(|s| PyObject::str_val(s.clone()))
                .collect(),
        )),
        "co_cellvars" => Some(PyObject::tuple(
            code.cellvars
                .iter()
                .map(|s| PyObject::str_val(s.clone()))
                .collect(),
        )),
        "co_consts" => Some(PyObject::tuple(
            code.constants.iter().map(constant_value_to_obj).collect(),
        )),
        "__class__" => Some(PyObject::builtin_type(CompactString::from("code"))),
        _ => None,
    }
}

fn constant_value_to_obj(c: &ferrython_bytecode::code::ConstantValue) -> PyObjectRef {
    use ferrython_bytecode::code::ConstantValue;
    match c {
        ConstantValue::None => PyObject::none(),
        ConstantValue::Bool(b) => PyObject::bool_val(*b),
        ConstantValue::Integer(n) => PyObject::int(*n),
        ConstantValue::BigInteger(n) => PyObject::big_int(n.as_ref().clone()),
        ConstantValue::Float(f) => PyObject::float(*f),
        ConstantValue::Complex { real, imag } => PyObject::complex(*real, *imag),
        ConstantValue::Str(s) => PyObject::str_val(s.clone()),
        ConstantValue::Bytes(b) => PyObject::bytes(b.clone()),
        ConstantValue::Ellipsis => PyObject::ellipsis(),
        ConstantValue::Code(co) => PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::clone(co))),
        ConstantValue::Tuple(items) => {
            PyObject::tuple(items.iter().map(constant_value_to_obj).collect())
        }
        ConstantValue::FrozenSet(items) => {
            let mut set = crate::object::new_fx_hashkey_map();
            for item in items {
                let obj = constant_value_to_obj(item);
                if let Ok(key) = obj.to_hashable_key() {
                    set.insert(key, obj);
                }
            }
            PyObject::frozenset(set)
        }
    }
}

fn iterator_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "__next__" | "__iter__" | "__length_hint__" => Some(bound_builtin(obj, name)),
        "__reduce__" | "__reduce_ex__" | "__copy__" | "__deepcopy__"
            if iterator_supports_reduce(obj) =>
        {
            Some(bound_builtin(obj, name))
        }
        "__setstate__" if iterator_supports_setstate(obj) => Some(bound_builtin(obj, name)),
        "__class__" => Some(PyObject::builtin_type(CompactString::from("iterator"))),
        _ => None,
    }
}

fn builtin_bound_method_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "__class__" => Some(PyObject::builtin_type(CompactString::from(
            "builtin_function_or_method",
        ))),
        "__name__" => {
            if let PyObjectPayload::BuiltinBoundMethod(bbm) = &obj.payload {
                Some(PyObject::str_val(bbm.method_name.clone()))
            } else {
                None
            }
        }
        "__self__" => {
            if let PyObjectPayload::BuiltinBoundMethod(bbm) = &obj.payload {
                Some(bbm.receiver.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

fn native_closure_attr(
    obj: &PyObjectRef,
    nc: &NativeClosureData,
    name: &str,
) -> Option<PyObjectRef> {
    match name {
        "__name__" | "__qualname__" => Some(PyObject::str_val(nc.name.clone())),
        "__class__" => Some(PyObject::builtin_type(CompactString::from(
            "builtin_function_or_method",
        ))),
        "__doc__" => Some(PyObject::none()),
        "__call__" => Some(obj.clone()),
        "__get__" => {
            let func = obj.clone();
            Some(PyObject::native_closure("__get__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "__get__ requires at least 1 argument",
                    ));
                }
                let instance = &args[0];
                if matches!(&instance.payload, PyObjectPayload::None) {
                    return Ok(func.clone());
                }
                Ok(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: instance.clone(),
                        method: func.clone(),
                    },
                }))
            }))
        }
        _ => None,
    }
}

const INT_METHODS: &[&str] = &[
    "bit_length",
    "bit_count",
    "to_bytes",
    "conjugate",
    "__abs__",
    "__int__",
    "__float__",
    "__index__",
    "__bool__",
    "__neg__",
    "__pos__",
    "__invert__",
    "__repr__",
    "__str__",
    "__hash__",
    "__format__",
    "__eq__",
    "__ne__",
    "__lt__",
    "__le__",
    "__gt__",
    "__ge__",
    "__add__",
    "__sub__",
    "__mul__",
    "__truediv__",
    "__floordiv__",
    "__mod__",
    "__pow__",
    "__divmod__",
    "__lshift__",
    "__rshift__",
    "__and__",
    "__or__",
    "__xor__",
    "__ceil__",
    "__floor__",
    "__round__",
    "__trunc__",
    "__sizeof__",
    "as_integer_ratio",
];

const FLOAT_METHODS: &[&str] = &[
    "is_integer",
    "conjugate",
    "hex",
    "__abs__",
    "__int__",
    "__float__",
    "__bool__",
    "__index__",
    "__neg__",
    "__pos__",
    "__repr__",
    "__str__",
    "__hash__",
    "__format__",
    "__eq__",
    "__ne__",
    "__lt__",
    "__le__",
    "__gt__",
    "__ge__",
    "__add__",
    "__sub__",
    "__mul__",
    "__truediv__",
    "__floordiv__",
    "__mod__",
    "__pow__",
    "__divmod__",
    "__round__",
    "__ceil__",
    "__floor__",
    "__trunc__",
    "__sizeof__",
    "as_integer_ratio",
    "fromhex",
];

const BOOL_METHODS: &[&str] = &[
    "bit_length",
    "bit_count",
    "to_bytes",
    "conjugate",
    "__abs__",
    "__int__",
    "__float__",
    "__index__",
    "__bool__",
    "__repr__",
    "__str__",
    "__hash__",
    "__format__",
    "__sizeof__",
];

const RANGE_METHODS: &[&str] = &[
    "count",
    "index",
    "__contains__",
    "__iter__",
    "__reversed__",
    "__len__",
    "__getitem__",
];

const STR_METHODS: &[&str] = &[
    "upper",
    "lower",
    "strip",
    "lstrip",
    "rstrip",
    "split",
    "rsplit",
    "join",
    "replace",
    "find",
    "rfind",
    "index",
    "rindex",
    "count",
    "startswith",
    "endswith",
    "isdigit",
    "isalpha",
    "isalnum",
    "isspace",
    "isupper",
    "islower",
    "istitle",
    "isprintable",
    "isidentifier",
    "isascii",
    "isdecimal",
    "isnumeric",
    "title",
    "capitalize",
    "swapcase",
    "center",
    "ljust",
    "rjust",
    "zfill",
    "expandtabs",
    "encode",
    "partition",
    "rpartition",
    "casefold",
    "removeprefix",
    "removesuffix",
    "splitlines",
    "format",
    "format_map",
    "translate",
    "maketrans",
    "__len__",
    "__contains__",
    "__iter__",
    "__getitem__",
    "__hash__",
    "__eq__",
    "__ne__",
    "__lt__",
    "__le__",
    "__gt__",
    "__ge__",
    "__repr__",
    "__str__",
    "__format__",
    "__add__",
    "__mul__",
    "__rmul__",
    "__mod__",
    "__bool__",
    "__sizeof__",
];

const LIST_METHODS: &[&str] = &[
    "append",
    "extend",
    "insert",
    "pop",
    "remove",
    "reverse",
    "sort",
    "clear",
    "copy",
    "count",
    "index",
    "__len__",
    "__contains__",
    "__iter__",
    "__getitem__",
    "__setitem__",
    "__delitem__",
    "__eq__",
    "__ne__",
    "__lt__",
    "__le__",
    "__gt__",
    "__ge__",
    "__repr__",
    "__str__",
    "__add__",
    "__mul__",
    "__rmul__",
    "__iadd__",
    "__imul__",
    "__reversed__",
    "__bool__",
    "__hash__",
    "__sizeof__",
];

const DICT_METHODS: &[&str] = &[
    "keys",
    "values",
    "items",
    "get",
    "copy",
    "update",
    "subtract",
    "pop",
    "setdefault",
    "clear",
    "popitem",
    "most_common",
    "elements",
    "move_to_end",
    "__len__",
    "__contains__",
    "__iter__",
    "__getitem__",
    "__setitem__",
    "__delitem__",
    "__eq__",
    "__ne__",
    "__repr__",
    "__str__",
    "__or__",
    "__ior__",
    "__bool__",
    "__hash__",
    "__sizeof__",
];

const TUPLE_METHODS: &[&str] = &[
    "count",
    "index",
    "__len__",
    "__contains__",
    "__iter__",
    "__getitem__",
    "__hash__",
    "__eq__",
    "__ne__",
    "__lt__",
    "__le__",
    "__gt__",
    "__ge__",
    "__repr__",
    "__str__",
    "__add__",
    "__mul__",
    "__rmul__",
    "__bool__",
    "__sizeof__",
];

const SET_METHODS: &[&str] = &[
    "add",
    "remove",
    "discard",
    "pop",
    "clear",
    "copy",
    "update",
    "union",
    "intersection",
    "difference",
    "symmetric_difference",
    "issubset",
    "issuperset",
    "isdisjoint",
    "intersection_update",
    "difference_update",
    "symmetric_difference_update",
    "__init__",
    "__len__",
    "__contains__",
    "__iter__",
    "__or__",
    "__and__",
    "__sub__",
    "__xor__",
    "__eq__",
    "__ne__",
    "__lt__",
    "__le__",
    "__gt__",
    "__ge__",
    "__repr__",
    "__str__",
    "__bool__",
    "__hash__",
    "__sizeof__",
];

const FROZENSET_METHODS: &[&str] = &[
    "copy",
    "union",
    "intersection",
    "difference",
    "symmetric_difference",
    "issubset",
    "issuperset",
    "isdisjoint",
    "__init__",
    "__len__",
    "__contains__",
    "__iter__",
    "__or__",
    "__and__",
    "__sub__",
    "__xor__",
    "__eq__",
    "__ne__",
    "__hash__",
    "__repr__",
    "__str__",
    "__bool__",
    "__sizeof__",
];

const BYTES_METHODS: &[&str] = &[
    "decode",
    "hex",
    "count",
    "find",
    "rfind",
    "index",
    "rindex",
    "startswith",
    "endswith",
    "upper",
    "lower",
    "strip",
    "lstrip",
    "rstrip",
    "split",
    "join",
    "replace",
    "isdigit",
    "isalpha",
    "isalnum",
    "isspace",
    "islower",
    "isupper",
    "istitle",
    "swapcase",
    "title",
    "capitalize",
    "center",
    "ljust",
    "rjust",
    "zfill",
    "expandtabs",
    "partition",
    "rpartition",
    "removeprefix",
    "removesuffix",
    "rsplit",
    "splitlines",
    "translate",
    "tobytes",
    "tolist",
    "release",
    "append",
    "extend",
    "pop",
    "insert",
    "clear",
    "reverse",
    "copy",
    "__len__",
    "__contains__",
    "__iter__",
    "__getitem__",
    "__setitem__",
    "__eq__",
    "__ne__",
    "__repr__",
    "__str__",
    "__add__",
    "__mul__",
    "__rmul__",
    "__rmod__",
    "__bool__",
    "__hash__",
    "__sizeof__",
];
