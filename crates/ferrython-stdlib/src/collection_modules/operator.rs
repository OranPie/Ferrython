use crate::introspection_modules::emit_deprecation_warning;
use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min, make_builtin, make_module, CompareOp, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;

// ── operator module ──

mod helpers;

use helpers::{
    builtin_index_value, call_dunder, call_inplace_dunder, object_index_result, operator_count_of,
    operator_index_of,
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
                    let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_))
                        || matches!(&args[1].payload, PyObjectPayload::Float(_));
                    if !either_float {
                        if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                            return Ok(PyObject::int(a - b));
                        }
                    }
                    let a = args[0].to_float()?;
                    let b = args[1].to_float()?;
                    Ok(PyObject::float(a - b))
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
                    let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_))
                        || matches!(&args[1].payload, PyObjectPayload::Float(_));
                    if !either_float {
                        if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                            if b >= 0 {
                                return Ok(PyObject::int(a.pow(b as u32)));
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
                    Ok(PyObject::bool_val(!args[0].is_truthy()))
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
            (
                "itemgetter",
                make_builtin(|args| {
                    check_args_min("itemgetter", args, 1)?;
                    let keys: Vec<PyObjectRef> = args.to_vec();
                    Ok(PyObject::native_closure_with_pickle_args(
                        "operator.itemgetter",
                        keys.clone(),
                        move |call_args| {
                            check_args("itemgetter", call_args, 1)?;
                            let obj = &call_args[0];
                            if keys.len() == 1 {
                                match obj.get_item(&keys[0]) {
                                    Err(err) if err.kind == ExceptionKind::ValueError => {
                                        Err(PyException::type_error(err.message))
                                    }
                                    result => result,
                                }
                            } else {
                                let items: Vec<PyObjectRef> = keys
                                    .iter()
                                    .map(|k| match obj.get_item(k) {
                                        Err(err) if err.kind == ExceptionKind::ValueError => {
                                            Err(PyException::type_error(err.message))
                                        }
                                        result => result,
                                    })
                                    .collect::<PyResult<Vec<_>>>()?;
                                Ok(PyObject::tuple(items))
                            }
                        },
                    ))
                }),
            ),
            (
                "attrgetter",
                make_builtin(|args| {
                    check_args_min("attrgetter", args, 1)?;
                    let mut attr_names = Vec::with_capacity(args.len());
                    for arg in args {
                        let name = arg.as_str().ok_or_else(|| {
                            PyException::type_error("attribute name must be a string")
                        })?;
                        attr_names.push(name.to_string());
                    }
                    let pickle_args = attr_names
                        .iter()
                        .map(|name| PyObject::str_val(CompactString::from(name.as_str())))
                        .collect();
                    Ok(PyObject::native_closure_with_pickle_args(
                        "operator.attrgetter",
                        pickle_args,
                        move |call_args| {
                            check_args("attrgetter", call_args, 1)?;
                            let obj = &call_args[0];
                            // Helper: resolve dotted attribute path (e.g. "a.b.c")
                            let resolve =
                                |name: &str, obj: &PyObjectRef| -> PyResult<PyObjectRef> {
                                    let parts: Vec<&str> = name.split('.').collect();
                                    let mut cur = obj.clone();
                                    for part in &parts {
                                        if let Some(attr) = cur.get_attr(part) {
                                            cur = attr;
                                        } else if let Some(getattr) = cur.get_attr("__getattr__") {
                                            cur = ferrython_core::object::call_callable(
                                                &getattr,
                                                &[PyObject::str_val(CompactString::from(*part))],
                                            )?;
                                        } else {
                                            return Err(PyException::attribute_error(format!(
                                                "'{}' object has no attribute '{}'",
                                                cur.type_name(),
                                                part
                                            )));
                                        }
                                    }
                                    Ok(cur)
                                };
                            if attr_names.len() == 1 {
                                resolve(&attr_names[0], obj)
                            } else {
                                let items: Vec<PyObjectRef> = attr_names
                                    .iter()
                                    .map(|name| resolve(name, obj))
                                    .collect::<PyResult<Vec<_>>>()?;
                                Ok(PyObject::tuple(items))
                            }
                        },
                    ))
                }),
            ),
            (
                "and_",
                make_builtin(|args| {
                    check_args("and_", args, 2)?;
                    let a = args[0].to_int()?;
                    let b = args[1].to_int()?;
                    Ok(PyObject::int(a & b))
                }),
            ),
            (
                "or_",
                make_builtin(|args| {
                    check_args("or_", args, 2)?;
                    let a = args[0].to_int()?;
                    let b = args[1].to_int()?;
                    Ok(PyObject::int(a | b))
                }),
            ),
            (
                "xor",
                make_builtin(|args| {
                    check_args("xor", args, 2)?;
                    let a = args[0].to_int()?;
                    let b = args[1].to_int()?;
                    Ok(PyObject::int(a ^ b))
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
            (
                "methodcaller",
                make_builtin(|args| {
                    check_args_min("methodcaller", args, 1)?;
                    let method_name = args[0]
                        .as_str()
                        .ok_or_else(|| PyException::type_error("method name must be a string"))?
                        .to_string();
                    let extra_args: Vec<PyObjectRef> = if args.len() > 1 {
                        args[1..].to_vec()
                    } else {
                        vec![]
                    };
                    let (extra_args, kw_args) =
                        if let Some((last, positional)) = extra_args.split_last() {
                            if let PyObjectPayload::Dict(map) = &last.payload {
                                let kw_args = map
                                    .read()
                                    .iter()
                                    .map(|(k, v)| match k {
                                        ferrython_core::types::HashableKey::Str(name) => {
                                            Ok((name.to_compact_string(), v.clone()))
                                        }
                                        _ => Err(PyException::type_error(
                                            "methodcaller keywords must be strings",
                                        )),
                                    })
                                    .collect::<PyResult<Vec<_>>>()?;
                                (positional.to_vec(), kw_args)
                            } else {
                                (extra_args, Vec::new())
                            }
                        } else {
                            (extra_args, Vec::new())
                        };
                    let mut pickle_args =
                        vec![PyObject::str_val(CompactString::from(method_name.as_str()))];
                    pickle_args.extend(extra_args.iter().cloned());
                    if !kw_args.is_empty() {
                        let mut kw_map = ferrython_core::object::new_fx_hashkey_map();
                        for (key, value) in &kw_args {
                            kw_map.insert(
                                ferrython_core::types::HashableKey::str_key(key.clone()),
                                value.clone(),
                            );
                        }
                        pickle_args.push(PyObject::dict(kw_map));
                    }
                    Ok(PyObject::native_closure_with_pickle_args(
                        "operator.methodcaller",
                        pickle_args,
                        move |call_args| {
                            check_args("methodcaller", call_args, 1)?;
                            let obj = &call_args[0];
                            let method = obj.get_attr(&method_name).ok_or_else(|| {
                                PyException::attribute_error(format!(
                                    "'{}' object has no attribute '{}'",
                                    obj.type_name(),
                                    method_name
                                ))
                            })?;
                            // Build full args: [obj, ...extra_args]
                            let mut full_args = vec![obj.clone()];
                            full_args.extend(extra_args.iter().cloned());
                            match &method.payload {
                                PyObjectPayload::NativeFunction(nf) => (nf.func)(&full_args),
                                PyObjectPayload::NativeClosure(nc) => (nc.func)(&full_args),
                                PyObjectPayload::BuiltinBoundMethod(bbm) => {
                                    // Handle BuiltinBoundMethod with extra args by dispatching common methods
                                    if !extra_args.is_empty() {
                                        let s = bbm.receiver.py_to_string();
                                        match bbm.method_name.as_str() {
                                            "split" => {
                                                let sep = extra_args[0].py_to_string();
                                                let maxsplit = if extra_args.len() > 1 {
                                                    extra_args[1].as_int().unwrap_or(-1)
                                                } else {
                                                    -1
                                                };
                                                let parts: Vec<&str> = if maxsplit < 0 {
                                                    s.split(&*sep).collect()
                                                } else {
                                                    s.splitn(maxsplit as usize + 1, &*sep).collect()
                                                };
                                                return Ok(PyObject::list(
                                                    parts
                                                        .into_iter()
                                                        .map(|p| {
                                                            PyObject::str_val(CompactString::from(
                                                                p,
                                                            ))
                                                        })
                                                        .collect(),
                                                ));
                                            }
                                            "rsplit" => {
                                                let sep = extra_args[0].py_to_string();
                                                let maxsplit = if extra_args.len() > 1 {
                                                    extra_args[1].as_int().unwrap_or(-1)
                                                } else {
                                                    -1
                                                };
                                                let parts: Vec<&str> = if maxsplit < 0 {
                                                    s.rsplit(&*sep).collect()
                                                } else {
                                                    s.rsplitn(maxsplit as usize + 1, &*sep)
                                                        .collect()
                                                };
                                                let mut result: Vec<PyObjectRef> = parts
                                                    .into_iter()
                                                    .map(|p| {
                                                        PyObject::str_val(CompactString::from(p))
                                                    })
                                                    .collect();
                                                result.reverse();
                                                return Ok(PyObject::list(result));
                                            }
                                            "replace" => {
                                                let old = extra_args[0].py_to_string();
                                                let new = if extra_args.len() > 1 {
                                                    extra_args[1].py_to_string()
                                                } else {
                                                    String::new()
                                                };
                                                let count = if extra_args.len() > 2 {
                                                    extra_args[2].as_int().unwrap_or(-1)
                                                } else {
                                                    -1
                                                };
                                                let result = if count < 0 {
                                                    s.replace(&*old, &new)
                                                } else {
                                                    s.replacen(&*old, &new, count as usize)
                                                };
                                                return Ok(PyObject::str_val(CompactString::from(
                                                    result,
                                                )));
                                            }
                                            "join" => {
                                                let items =
                                                    extra_args[0].to_list().unwrap_or_default();
                                                let joined: String = items
                                                    .iter()
                                                    .map(|i| i.py_to_string())
                                                    .collect::<Vec<_>>()
                                                    .join(&s);
                                                return Ok(PyObject::str_val(CompactString::from(
                                                    joined,
                                                )));
                                            }
                                            "encode" => {
                                                return Ok(PyObject::bytes(s.into_bytes()));
                                            }
                                            "find" => {
                                                let sub = extra_args[0].py_to_string();
                                                let idx =
                                                    s.find(&*sub).map(|i| i as i64).unwrap_or(-1);
                                                return Ok(PyObject::int(idx));
                                            }
                                            "count" => {
                                                let sub = extra_args[0].py_to_string();
                                                return Ok(PyObject::int(
                                                    s.matches(&*sub).count() as i64
                                                ));
                                            }
                                            "startswith" => {
                                                let prefix = extra_args[0].py_to_string();
                                                return Ok(PyObject::bool_val(
                                                    s.starts_with(&*prefix),
                                                ));
                                            }
                                            "endswith" => {
                                                let suffix = extra_args[0].py_to_string();
                                                return Ok(PyObject::bool_val(
                                                    s.ends_with(&*suffix),
                                                ));
                                            }
                                            "center" | "ljust" | "rjust" => {
                                                let width =
                                                    extra_args[0].as_int().unwrap_or(0) as usize;
                                                let fill = if extra_args.len() > 1 {
                                                    extra_args[1]
                                                        .py_to_string()
                                                        .chars()
                                                        .next()
                                                        .unwrap_or(' ')
                                                } else {
                                                    ' '
                                                };
                                                let result = if s.len() >= width {
                                                    s.clone()
                                                } else {
                                                    let pad = width - s.len();
                                                    match bbm.method_name.as_str() {
                                                        "center" => {
                                                            let left = pad / 2;
                                                            let right = pad - left;
                                                            format!(
                                                                "{}{}{}",
                                                                fill.to_string().repeat(left),
                                                                s,
                                                                fill.to_string().repeat(right)
                                                            )
                                                        }
                                                        "ljust" => format!(
                                                            "{}{}",
                                                            s,
                                                            fill.to_string().repeat(pad)
                                                        ),
                                                        "rjust" => format!(
                                                            "{}{}",
                                                            fill.to_string().repeat(pad),
                                                            s
                                                        ),
                                                        _ => s.clone(),
                                                    }
                                                };
                                                return Ok(PyObject::str_val(CompactString::from(
                                                    result,
                                                )));
                                            }
                                            _ => {
                                                // Fallback: use deferred call
                                                crate::concurrency_modules::push_deferred_call(
                                                    method.clone(),
                                                    extra_args.clone(),
                                                );
                                                return Ok(PyObject::none());
                                            }
                                        }
                                    }
                                    // No extra args: try common zero-arg string methods
                                    let result_str = match bbm.method_name.as_str() {
                                        "upper" => Some(bbm.receiver.py_to_string().to_uppercase()),
                                        "lower" => Some(bbm.receiver.py_to_string().to_lowercase()),
                                        "strip" => {
                                            Some(bbm.receiver.py_to_string().trim().to_string())
                                        }
                                        "lstrip" => Some(
                                            bbm.receiver.py_to_string().trim_start().to_string(),
                                        ),
                                        "rstrip" => {
                                            Some(bbm.receiver.py_to_string().trim_end().to_string())
                                        }
                                        "title" => {
                                            let s = bbm.receiver.py_to_string();
                                            let mut result = String::with_capacity(s.len());
                                            let mut capitalize_next = true;
                                            for c in s.chars() {
                                                if c.is_alphanumeric() {
                                                    if capitalize_next {
                                                        result.extend(c.to_uppercase());
                                                        capitalize_next = false;
                                                    } else {
                                                        result.extend(c.to_lowercase());
                                                    }
                                                } else {
                                                    result.push(c);
                                                    capitalize_next = true;
                                                }
                                            }
                                            Some(result)
                                        }
                                        "capitalize" => {
                                            let s = bbm.receiver.py_to_string();
                                            let mut chars = s.chars();
                                            Some(match chars.next() {
                                                None => String::new(),
                                                Some(f) => {
                                                    f.to_uppercase().collect::<String>()
                                                        + &chars.collect::<String>().to_lowercase()
                                                }
                                            })
                                        }
                                        "swapcase" => {
                                            let s = bbm.receiver.py_to_string();
                                            Some(
                                                s.chars()
                                                    .map(|c| {
                                                        if c.is_uppercase() {
                                                            c.to_lowercase().collect::<String>()
                                                        } else {
                                                            c.to_uppercase().collect::<String>()
                                                        }
                                                    })
                                                    .collect(),
                                            )
                                        }
                                        _ => None,
                                    };
                                    if let Some(s) = result_str {
                                        Ok(PyObject::str_val(CompactString::from(s)))
                                    } else {
                                        // Can't dispatch BuiltinBoundMethod from NativeClosure — return method ref
                                        Ok(method)
                                    }
                                }
                                PyObjectPayload::BoundMethod {
                                    receiver,
                                    method: meth,
                                    ..
                                } => match &meth.payload {
                                    PyObjectPayload::NativeFunction(nf) => {
                                        let mut bound_args = vec![receiver.clone()];
                                        bound_args.extend(extra_args.iter().cloned());
                                        (nf.func)(&bound_args)
                                    }
                                    PyObjectPayload::NativeClosure(nc) => {
                                        let mut bound_args = vec![receiver.clone()];
                                        bound_args.extend(extra_args.iter().cloned());
                                        (nc.func)(&bound_args)
                                    }
                                    _ => {
                                        let mut bound_args = vec![receiver.clone()];
                                        bound_args.extend(extra_args.iter().cloned());
                                        ferrython_core::object::call_callable_kw(
                                            meth,
                                            &bound_args,
                                            kw_args.clone(),
                                        )
                                    }
                                },
                                PyObjectPayload::Function(_) => {
                                    let mut call_args_full = vec![obj.clone()];
                                    call_args_full.extend(extra_args.iter().cloned());
                                    ferrython_core::object::call_callable_kw(
                                        &method,
                                        &call_args_full,
                                        kw_args.clone(),
                                    )
                                }
                                _ => Ok(method),
                            }
                        },
                    ))
                }),
            ),
            (
                "length_hint",
                make_builtin(|args| {
                    check_args_min("length_hint", args, 1)?;
                    let default = if args.len() > 1 {
                        args[1].to_int().unwrap_or(0)
                    } else {
                        0
                    };
                    let validate_hint = |value: PyObjectRef| -> PyResult<Option<i64>> {
                        if matches!(&value.payload, PyObjectPayload::NotImplemented) {
                            return Ok(None);
                        }
                        let n = value.as_int().ok_or_else(|| {
                            PyException::type_error("__length_hint__ must be an integer")
                        })?;
                        if n < 0 {
                            return Err(PyException::value_error(
                                "__length_hint__() should return >= 0",
                            ));
                        }
                        Ok(Some(n))
                    };
                    // Try __length_hint__ first, then __len__
                    for dunder in &["__length_hint__", "__len__"] {
                        if let Some(method) = args[0].get_attr(dunder) {
                            match ferrython_core::object::call_callable(&method, &[]) {
                                Ok(value) => {
                                    if let Some(n) = validate_hint(value)? {
                                        return Ok(PyObject::int(n));
                                    }
                                    return Ok(PyObject::int(default));
                                }
                                Err(err) if err.kind == ExceptionKind::TypeError => {
                                    return Ok(PyObject::int(default));
                                }
                                Err(err) => return Err(err),
                            }
                        }
                    }
                    // Try len() directly
                    match args[0].py_len() {
                        Ok(n) => Ok(PyObject::int(n as i64)),
                        Err(_) => Ok(PyObject::int(default)),
                    }
                }),
            ),
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
