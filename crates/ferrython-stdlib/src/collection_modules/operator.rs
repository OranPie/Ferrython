use crate::introspection_modules::emit_deprecation_warning;
use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min, make_builtin, make_module, truthy_with_vm, CompareOp, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;
use std::convert::TryFrom;

// ── operator module ──

mod callables;
mod helpers;

use callables::{
    operator_attrgetter, operator_itemgetter, operator_length_hint, operator_methodcaller,
};
use helpers::{
    builtin_index_value, call_binary_dunder, call_dunder, call_inplace_dunder, object_index_result,
    operator_count_of, operator_index_of,
};

pub fn create_operator_module() -> PyObjectRef {
    let m = make_module(
        "operator",
        vec![
            (
                "add",
                make_builtin(|args| {
                    check_args("add", args, 2)?;
                    args[0].add(&args[1])
                }),
            ),
            (
                "sub",
                make_builtin(|args| {
                    check_args("sub", args, 2)?;
                    if let Some(result) =
                        call_binary_dunder(&args[0], &args[1], "__sub__", Some("__rsub__"))?
                    {
                        return Ok(result);
                    }
                    args[0].sub(&args[1])
                }),
            ),
            (
                "mul",
                make_builtin(|args| {
                    check_args("mul", args, 2)?;
                    let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_))
                        || matches!(&args[1].payload, PyObjectPayload::Float(_));
                    if !either_float {
                        if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                            return Ok(PyObject::int(a * b));
                        }
                    }
                    let a = args[0].to_float()?;
                    let b = args[1].to_float()?;
                    Ok(PyObject::float(a * b))
                }),
            ),
            (
                "truediv",
                make_builtin(|args| {
                    check_args("truediv", args, 2)?;
                    let a = args[0].to_float()?;
                    let b = args[1].to_float()?;
                    if b == 0.0 {
                        return Err(PyException::zero_division_error("division by zero"));
                    }
                    Ok(PyObject::float(a / b))
                }),
            ),
            (
                "floordiv",
                make_builtin(|args| {
                    check_args("floordiv", args, 2)?;
                    let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_))
                        || matches!(&args[1].payload, PyObjectPayload::Float(_));
                    if !either_float {
                        if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                            if b == 0 {
                                return Err(PyException::zero_division_error(
                                    "integer division or modulo by zero",
                                ));
                            }
                            return Ok(PyObject::int(a.div_euclid(b)));
                        }
                    }
                    let a = args[0].to_float()?;
                    let b = args[1].to_float()?;
                    if b == 0.0 {
                        return Err(PyException::zero_division_error(
                            "float floor division by zero",
                        ));
                    }
                    Ok(PyObject::float((a / b).floor()))
                }),
            ),
            (
                "mod_",
                make_builtin(|args| {
                    check_args("mod_", args, 2)?;
                    let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_))
                        || matches!(&args[1].payload, PyObjectPayload::Float(_));
                    if !either_float {
                        if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                            if b == 0 {
                                return Err(PyException::zero_division_error(
                                    "integer division or modulo by zero",
                                ));
                            }
                            return Ok(PyObject::int(a.rem_euclid(b)));
                        }
                    }
                    let a = args[0].to_float()?;
                    let b = args[1].to_float()?;
                    Ok(PyObject::float(a % b))
                }),
            ),
            // Also register as "mod" for getattr(operator, "mod") usage
            (
                "mod",
                make_builtin(|args| {
                    check_args("mod", args, 2)?;
                    let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_))
                        || matches!(&args[1].payload, PyObjectPayload::Float(_));
                    if !either_float {
                        if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                            if b == 0 {
                                return Err(PyException::zero_division_error(
                                    "integer division or modulo by zero",
                                ));
                            }
                            return Ok(PyObject::int(a.rem_euclid(b)));
                        }
                    }
                    let a = args[0].to_float()?;
                    let b = args[1].to_float()?;
                    Ok(PyObject::float(a % b))
                }),
            ),
            (
                "neg",
                make_builtin(|args| {
                    check_args("neg", args, 1)?;
                    if matches!(&args[0].payload, PyObjectPayload::Float(_)) {
                        Ok(PyObject::float(-args[0].to_float()?))
                    } else if let Ok(n) = args[0].to_int() {
                        Ok(PyObject::int(-n))
                    } else {
                        Ok(PyObject::float(-args[0].to_float()?))
                    }
                }),
            ),
            (
                "pow",
                make_builtin(|args| {
                    check_args("pow", args, 2)?;
                    if let Some(result) =
                        call_binary_dunder(&args[0], &args[1], "__pow__", Some("__rpow__"))?
                    {
                        return Ok(result);
                    }
                    let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_))
                        || matches!(&args[1].payload, PyObjectPayload::Float(_));
                    if !either_float {
                        if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                            if b >= 0 {
                                let exp = u32::try_from(b).map_err(|_| {
                                    PyException::overflow_error("integer exponent too large")
                                })?;
                                let result = PyInt::pow_op(&PyInt::Small(a), exp);
                                return Ok(match result {
                                    PyInt::Small(n) => PyObject::int(n),
                                    PyInt::Big(n) => PyObject::big_int(*n),
                                });
                            }
                            return Ok(PyObject::float((a as f64).powf(b as f64)));
                        }
                    }
                    let a = args[0].to_float()?;
                    let b = args[1].to_float()?;
                    Ok(PyObject::float(a.powf(b)))
                }),
            ),
            (
                "matmul",
                make_builtin(|args| {
                    check_args("matmul", args, 2)?;
                    Err(PyException::type_error("unsupported operand type(s) for @"))
                }),
            ),
            (
                "pos",
                make_builtin(|args| {
                    check_args("pos", args, 1)?;
                    match &args[0].payload {
                        PyObjectPayload::Bool(_) | PyObjectPayload::Int(_) => {
                            Ok(PyObject::int(args[0].to_int()?))
                        }
                        PyObjectPayload::Float(_) => Ok(PyObject::float(args[0].to_float()?)),
                        PyObjectPayload::Complex { .. } => Ok(args[0].clone()),
                        _ => Err(PyException::type_error(format!(
                            "bad operand type for unary +: '{}'",
                            args[0].type_name()
                        ))),
                    }
                }),
            ),
            (
                "not_",
                make_builtin(|args| {
                    check_args("not_", args, 1)?;
                    Ok(PyObject::bool_val(!truthy_with_vm(&args[0])?))
                }),
            ),
            (
                "eq",
                make_builtin(|args| {
                    check_args("eq", args, 2)?;
                    args[0].compare(&args[1], CompareOp::Eq)
                }),
            ),
            (
                "ne",
                make_builtin(|args| {
                    check_args("ne", args, 2)?;
                    args[0].compare(&args[1], CompareOp::Ne)
                }),
            ),
            (
                "lt",
                make_builtin(|args| {
                    check_args("lt", args, 2)?;
                    args[0].compare(&args[1], CompareOp::Lt)
                }),
            ),
            (
                "le",
                make_builtin(|args| {
                    check_args("le", args, 2)?;
                    args[0].compare(&args[1], CompareOp::Le)
                }),
            ),
            (
                "gt",
                make_builtin(|args| {
                    check_args("gt", args, 2)?;
                    args[0].compare(&args[1], CompareOp::Gt)
                }),
            ),
            (
                "ge",
                make_builtin(|args| {
                    check_args("ge", args, 2)?;
                    args[0].compare(&args[1], CompareOp::Ge)
                }),
            ),
            (
                "abs",
                make_builtin(|args| {
                    check_args_min("abs", args, 1)?;
                    check_args("abs", args, 1)?;
                    args[0].py_abs()
                }),
            ),
            (
                "contains",
                make_builtin(|args| {
                    check_args("contains", args, 2)?;
                    Ok(PyObject::bool_val(args[0].contains(&args[1])?))
                }),
            ),
            (
                "getitem",
                make_builtin(|args| {
                    check_args("getitem", args, 2)?;
                    args[0].get_item(&args[1])
                }),
            ),
            ("itemgetter", make_builtin(operator_itemgetter)),
            ("attrgetter", make_builtin(operator_attrgetter)),
            (
                "and_",
                make_builtin(|args| {
                    check_args("and_", args, 2)?;
                    if let Some(result) =
                        call_binary_dunder(&args[0], &args[1], "__and__", Some("__rand__"))?
                    {
                        return Ok(result);
                    }
                    args[0].bit_and(&args[1])
                }),
            ),
            (
                "or_",
                make_builtin(|args| {
                    check_args("or_", args, 2)?;
                    if let Some(result) =
                        call_binary_dunder(&args[0], &args[1], "__or__", Some("__ror__"))?
                    {
                        return Ok(result);
                    }
                    args[0].bit_or(&args[1])
                }),
            ),
            (
                "xor",
                make_builtin(|args| {
                    check_args("xor", args, 2)?;
                    if let Some(result) =
                        call_binary_dunder(&args[0], &args[1], "__xor__", Some("__rxor__"))?
                    {
                        return Ok(result);
                    }
                    args[0].bit_xor(&args[1])
                }),
            ),
            (
                "lshift",
                make_builtin(|args| {
                    check_args("lshift", args, 2)?;
                    let a = args[0].to_int()?;
                    let b = args[1].to_int()?;
                    if b < 0 {
                        return Err(PyException::value_error("negative shift count"));
                    }
                    let shift = u32::try_from(b)
                        .map_err(|_| PyException::overflow_error("too many digits in integer"))?;
                    match a.checked_shl(shift) {
                        Some(value) => Ok(PyObject::int(value)),
                        None => Err(PyException::overflow_error("too many digits in integer")),
                    }
                }),
            ),
            (
                "rshift",
                make_builtin(|args| {
                    check_args("rshift", args, 2)?;
                    let a = args[0].to_int()?;
                    let b = args[1].to_int()?;
                    if b < 0 {
                        return Err(PyException::value_error("negative shift count"));
                    }
                    let shift = u32::try_from(b)
                        .map_err(|_| PyException::overflow_error("too many digits in integer"))?;
                    Ok(PyObject::int(if shift >= i64::BITS {
                        if a < 0 {
                            -1
                        } else {
                            0
                        }
                    } else {
                        a >> shift
                    }))
                }),
            ),
            (
                "invert",
                make_builtin(|args| {
                    check_args("invert", args, 1)?;
                    let a = args[0].to_int()?;
                    Ok(PyObject::int(!a))
                }),
            ),
            (
                "inv",
                make_builtin(|args| {
                    check_args("inv", args, 1)?;
                    let a = args[0].to_int()?;
                    Ok(PyObject::int(!a))
                }),
            ),
            (
                "truth",
                make_builtin(|args| {
                    check_args("truth", args, 1)?;
                    if let Some(method) = args[0].get_attr("__bool__") {
                        return Ok(PyObject::bool_val(
                            ferrython_core::object::call_callable(&method, &[])?.is_truthy(),
                        ));
                    }
                    if let Some(method) = args[0].get_attr("__len__") {
                        return Ok(PyObject::bool_val(
                            ferrython_core::object::call_callable(&method, &[])?.is_truthy(),
                        ));
                    }
                    Ok(PyObject::bool_val(args[0].is_truthy()))
                }),
            ),
            (
                "is_",
                make_builtin(|args| {
                    check_args("is_", args, 2)?;
                    Ok(PyObject::bool_val(PyObjectRef::ptr_eq(&args[0], &args[1])))
                }),
            ),
            (
                "is_not",
                make_builtin(|args| {
                    check_args("is_not", args, 2)?;
                    Ok(PyObject::bool_val(!PyObjectRef::ptr_eq(&args[0], &args[1])))
                }),
            ),
            (
                "index",
                make_builtin(|args| {
                    check_args("index", args, 1)?;
                    if let Some(index) = builtin_index_value(&args[0]) {
                        return Ok(index.to_object());
                    }
                    if let Some(result) = call_dunder(&args[0], "__index__", &[])? {
                        return object_index_result(result);
                    }
                    Err(PyException::type_error(format!(
                        "'{}' object cannot be interpreted as an integer",
                        args[0].type_name()
                    )))
                }),
            ),
            (
                "setitem",
                make_builtin(|args| {
                    check_args("setitem", args, 3)?;
                    match &args[0].payload {
                        PyObjectPayload::List(items) => {
                            let idx = args[1].to_int()? as usize;
                            let mut w = items.write();
                            if idx < w.len() {
                                w[idx] = args[2].clone();
                                Ok(PyObject::none())
                            } else {
                                Err(PyException::index_error(
                                    "list assignment index out of range",
                                ))
                            }
                        }
                        PyObjectPayload::Dict(map) => {
                            let key = args[1].to_hashable_key()?;
                            map.write().insert(key, args[2].clone());
                            Ok(PyObject::none())
                        }
                        _ => Err(PyException::type_error(
                            "object does not support item assignment",
                        )),
                    }
                }),
            ),
            (
                "delitem",
                make_builtin(|args| {
                    check_args("delitem", args, 2)?;
                    match &args[0].payload {
                        PyObjectPayload::List(items) => {
                            let idx = args[1].to_int()?;
                            let mut items = items.write();
                            let len = items.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error(
                                    "list assignment index out of range",
                                ));
                            }
                            items.remove(actual as usize);
                            Ok(PyObject::none())
                        }
                        PyObjectPayload::Dict(map) => {
                            let key = args[1].to_hashable_key()?;
                            if map.write().shift_remove(&key).is_none() {
                                return Err(PyException::key_error(args[1].repr()));
                            }
                            Ok(PyObject::none())
                        }
                        _ => {
                            if let Some(result) =
                                call_dunder(&args[0], "__delitem__", &[args[1].clone()])?
                            {
                                Ok(result)
                            } else {
                                Err(PyException::type_error(
                                    "object does not support item deletion",
                                ))
                            }
                        }
                    }
                }),
            ),
            (
                "concat",
                make_builtin(|args| {
                    check_args("concat", args, 2)?;
                    match (&args[0].payload, &args[1].payload) {
                        (PyObjectPayload::Str(_), PyObjectPayload::Str(_))
                        | (PyObjectPayload::List(_), PyObjectPayload::List(_))
                        | (PyObjectPayload::Tuple(_), PyObjectPayload::Tuple(_)) => {
                            args[0].add(&args[1])
                        }
                        (PyObjectPayload::Instance(_), _) => {
                            if let Some(result) =
                                call_dunder(&args[0], "__add__", &[args[1].clone()])?
                            {
                                Ok(result)
                            } else {
                                Err(PyException::type_error(format!(
                                    "'{}' object can't be concatenated",
                                    args[0].type_name()
                                )))
                            }
                        }
                        _ => Err(PyException::type_error(format!(
                            "'{}' object can't be concatenated",
                            args[0].type_name()
                        ))),
                    }
                }),
            ),
            (
                "iadd",
                make_builtin(|args| {
                    check_args("iadd", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__iadd__", "__add__")?
                    {
                        Ok(result)
                    } else {
                        args[0].add(&args[1])
                    }
                }),
            ),
            (
                "isub",
                make_builtin(|args| {
                    check_args("isub", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__isub__", "__sub__")?
                    {
                        Ok(result)
                    } else {
                        args[0].sub(&args[1])
                    }
                }),
            ),
            (
                "imul",
                make_builtin(|args| {
                    check_args("imul", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__imul__", "__mul__")?
                    {
                        Ok(result)
                    } else {
                        args[0].mul(&args[1])
                    }
                }),
            ),
            (
                "itruediv",
                make_builtin(|args| {
                    check_args("itruediv", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__itruediv__", "__truediv__")?
                    {
                        Ok(result)
                    } else {
                        args[0].true_div(&args[1])
                    }
                }),
            ),
            (
                "ifloordiv",
                make_builtin(|args| {
                    check_args("ifloordiv", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__ifloordiv__", "__floordiv__")?
                    {
                        Ok(result)
                    } else {
                        args[0].floor_div(&args[1])
                    }
                }),
            ),
            (
                "imod",
                make_builtin(|args| {
                    check_args("imod", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__imod__", "__mod__")?
                    {
                        Ok(result)
                    } else {
                        args[0].modulo(&args[1])
                    }
                }),
            ),
            (
                "ipow",
                make_builtin(|args| {
                    check_args("ipow", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__ipow__", "__pow__")?
                    {
                        Ok(result)
                    } else {
                        args[0].power(&args[1])
                    }
                }),
            ),
            (
                "imatmul",
                make_builtin(|args| {
                    check_args("imatmul", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__imatmul__", "__matmul__")?
                    {
                        Ok(result)
                    } else {
                        Err(PyException::type_error(
                            "unsupported operand type(s) for @=",
                        ))
                    }
                }),
            ),
            (
                "iand",
                make_builtin(|args| {
                    check_args("iand", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__iand__", "__and__")?
                    {
                        Ok(result)
                    } else {
                        args[0].bit_and(&args[1])
                    }
                }),
            ),
            (
                "ior",
                make_builtin(|args| {
                    check_args("ior", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__ior__", "__or__")?
                    {
                        Ok(result)
                    } else {
                        args[0].bit_or(&args[1])
                    }
                }),
            ),
            (
                "ixor",
                make_builtin(|args| {
                    check_args("ixor", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__ixor__", "__xor__")?
                    {
                        Ok(result)
                    } else {
                        args[0].bit_xor(&args[1])
                    }
                }),
            ),
            (
                "ilshift",
                make_builtin(|args| {
                    check_args("ilshift", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__ilshift__", "__lshift__")?
                    {
                        Ok(result)
                    } else {
                        args[0].lshift(&args[1])
                    }
                }),
            ),
            (
                "irshift",
                make_builtin(|args| {
                    check_args("irshift", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__irshift__", "__rshift__")?
                    {
                        Ok(result)
                    } else {
                        args[0].rshift(&args[1])
                    }
                }),
            ),
            (
                "iconcat",
                make_builtin(|args| {
                    check_args("iconcat", args, 2)?;
                    if let Some(result) =
                        call_inplace_dunder(&args[0], &args[1], "__iadd__", "__add__")?
                    {
                        Ok(result)
                    } else {
                        args[0].add(&args[1])
                    }
                }),
            ),
            ("methodcaller", make_builtin(operator_methodcaller)),
            ("length_hint", make_builtin(operator_length_hint)),
            (
                "indexOf",
                make_builtin(|args| {
                    check_args("indexOf", args, 2)?;
                    Ok(PyObject::int(operator_index_of(&args[0], &args[1])?))
                }),
            ),
            (
                "countOf",
                make_builtin(|args| {
                    check_args("countOf", args, 2)?;
                    Ok(PyObject::int(operator_count_of(&args[0], &args[1])?))
                }),
            ),
        ],
    );

    // Add dunder aliases: operator.__lt__ == operator.lt, etc.
    let dunder_aliases = [
        ("__lt__", "lt"),
        ("__le__", "le"),
        ("__eq__", "eq"),
        ("__ne__", "ne"),
        ("__gt__", "gt"),
        ("__ge__", "ge"),
        ("__add__", "add"),
        ("__sub__", "sub"),
        ("__mul__", "mul"),
        ("__mod__", "mod"),
        ("__pow__", "pow"),
        ("__neg__", "neg"),
        ("__pos__", "pos"),
        ("__abs__", "abs"),
        ("__not__", "not_"),
        ("__and__", "and_"),
        ("__or__", "or_"),
        ("__xor__", "xor"),
        ("__invert__", "invert"),
        ("__lshift__", "lshift"),
        ("__rshift__", "rshift"),
        ("__truediv__", "truediv"),
        ("__floordiv__", "floordiv"),
        ("__contains__", "contains"),
        ("__getitem__", "getitem"),
        ("__setitem__", "setitem"),
        ("__delitem__", "delitem"),
        ("__iadd__", "iadd"),
        ("__isub__", "isub"),
        ("__imul__", "imul"),
        ("__matmul__", "matmul"),
        ("__imatmul__", "imatmul"),
        ("__concat__", "concat"),
        ("__iconcat__", "iconcat"),
    ];
    if let PyObjectPayload::Module(ref md) = m.payload {
        let attr_names: Vec<CompactString> = md.attrs.read().keys().cloned().collect();
        for attr_name in attr_names {
            let value = md.attrs.read().get(&attr_name).cloned();
            if let Some(value) = value {
                if let PyObjectPayload::NativeFunction(nf) = &value.payload {
                    if nf.name.is_empty() {
                        let named =
                            PyObject::native_function(&format!("operator.{}", attr_name), nf.func);
                        md.attrs.write().insert(attr_name, named);
                    }
                }
            }
        }
        let mod_func = md.attrs.read().get(&CompactString::from("mod")).cloned();
        if let Some(v) = mod_func {
            md.attrs.write().insert(CompactString::from("mod_"), v);
        }
        for (dunder, orig) in &dunder_aliases {
            let val = md.attrs.read().get(&CompactString::from(*orig)).cloned();
            if let Some(v) = val {
                md.attrs.write().insert(CompactString::from(*dunder), v);
            }
        }
    }

    m
}
