//! Small VM dispatch fast-path helpers.

use compact_str::CompactString;
use ferrython_core::object::{
    is_hidden_dict_key, IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    CLASS_FLAG_HAS_DESCRIPTORS, CLASS_FLAG_HAS_SETATTR, CLASS_FLAG_HAS_SLOTS,
};
use ferrython_core::types::{float_as_integer_ratio, PyInt};

#[inline(always)]
fn native_function_binds_to_class_name(
    class_name: &CompactString,
    attr_name: &CompactString,
    native_name: &str,
) -> bool {
    let expected_len = class_name.len() + attr_name.len() + 1;
    if native_name.len() != expected_len {
        return false;
    }
    native_name.starts_with(class_name.as_str())
        && native_name.as_bytes().get(class_name.len()) == Some(&b'.')
        && &native_name[class_name.len() + 1..] == attr_name.as_str()
}

#[inline(always)]
pub(crate) fn native_function_binds_to_class(
    cd: &ferrython_core::object::ClassData,
    attr_name: &CompactString,
    native_name: &str,
) -> bool {
    if native_function_binds_to_class_name(&cd.name, attr_name, native_name) {
        return true;
    }
    for base in cd.mro.iter().chain(cd.bases.iter()) {
        if let PyObjectPayload::Class(base_cd) = &base.payload {
            if native_function_binds_to_class_name(&base_cd.name, attr_name, native_name) {
                return true;
            }
        }
    }
    false
}

#[inline(always)]
pub(crate) fn fast_deque_native_closure_returns_none(name: &str, arg_count: usize) -> Option<bool> {
    match (name, arg_count) {
        ("deque.append", 1) | ("deque.appendleft", 1) => Some(true),
        ("deque.pop", 0) | ("deque.popleft", 0) => Some(false),
        _ => None,
    }
}

#[inline(always)]
pub(crate) fn try_fast_builtin_setattr_stack(
    stack: &mut Vec<PyObjectRef>,
    func_idx: usize,
) -> bool {
    if stack.len() < func_idx + 4 {
        return false;
    }

    unsafe {
        let base = stack.as_mut_ptr();
        let obj = &*base.add(func_idx + 1);
        let name_obj = &*base.add(func_idx + 2);
        let inst = match &obj.payload {
            PyObjectPayload::Instance(inst)
                if inst.class_flags
                    & (CLASS_FLAG_HAS_SETATTR
                        | CLASS_FLAG_HAS_DESCRIPTORS
                        | CLASS_FLAG_HAS_SLOTS)
                    == 0 =>
            {
                inst
            }
            _ => return false,
        };
        let name = match &name_obj.payload {
            PyObjectPayload::Str(s) => s.as_str(),
            _ => return false,
        };
        let map = &mut *inst.attrs.data_ptr();
        let value = std::ptr::read(base.add(func_idx + 3));
        if let Some(slot) = map.get_mut(name) {
            let old = std::mem::replace(slot, value);
            drop(old);
        } else {
            map.insert(CompactString::from(name), value);
        }
        let _func = std::ptr::read(base.add(func_idx));
        let _obj = std::ptr::read(base.add(func_idx + 1));
        let _name = std::ptr::read(base.add(func_idx + 2));
        stack.set_len(func_idx);
    }
    true
}

#[inline(always)]
pub(crate) fn fast_callable_bool(arg: &PyObjectRef) -> Option<bool> {
    match &arg.payload {
        PyObjectPayload::Function(_)
        | PyObjectPayload::BuiltinFunction(_)
        | PyObjectPayload::BuiltinType(_)
        | PyObjectPayload::BoundMethod { .. }
        | PyObjectPayload::BuiltinBoundMethod(_)
        | PyObjectPayload::Class(_)
        | PyObjectPayload::ExceptionType(_)
        | PyObjectPayload::NativeFunction(_)
        | PyObjectPayload::NativeClosure(_)
        | PyObjectPayload::Partial(_) => Some(true),
        PyObjectPayload::Instance(_) => None,
        _ => Some(false),
    }
}

#[inline(always)]
pub(crate) fn fast_int_conversion(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Int(_) => Some(arg.clone()),
        PyObjectPayload::Bool(b) => Some(PyObject::int(if *b { 1 } else { 0 })),
        PyObjectPayload::Float(f) if f.is_finite() => {
            let truncated = f.trunc();
            if truncated >= -9_007_199_254_740_992.0 && truncated <= 9_007_199_254_740_992.0 {
                Some(PyObject::int(truncated as i64))
            } else {
                let (n, d) = float_as_integer_ratio(truncated);
                Some(PyObject::big_int(n / d))
            }
        }
        PyObjectPayload::Float(_) => None,
        PyObjectPayload::Str(s) => s.trim().parse::<i64>().ok().map(PyObject::int),
        _ => None,
    }
}

#[inline(always)]
pub(crate) fn fast_small_int_sequence_min_max(
    arg: &PyObjectRef,
    is_max: bool,
) -> Option<PyObjectRef> {
    let items: &[PyObjectRef] = match &arg.payload {
        PyObjectPayload::List(v) => unsafe { (&*v.data_ptr()).as_slice() },
        PyObjectPayload::Tuple(v) => v.as_slice(),
        _ => return None,
    };
    if items.is_empty() {
        return None;
    }
    let mut best_idx = 0usize;
    let mut best_val = match &items[0].payload {
        PyObjectPayload::Int(PyInt::Small(n)) => *n,
        _ => return None,
    };
    for (i, item) in items[1..].iter().enumerate() {
        let value = match &item.payload {
            PyObjectPayload::Int(PyInt::Small(n)) => *n,
            _ => return None,
        };
        if (is_max && value > best_val) || (!is_max && value < best_val) {
            best_idx = i + 1;
            best_val = value;
        }
    }
    Some(items[best_idx].clone())
}

#[inline(always)]
pub(crate) fn fast_small_int_sequence_sorted(arg: &PyObjectRef) -> Option<PyObjectRef> {
    let all_small = match &arg.payload {
        PyObjectPayload::List(v) => unsafe { &*v.data_ptr() }
            .iter()
            .all(|item| matches!(&item.payload, PyObjectPayload::Int(PyInt::Small(_)))),
        PyObjectPayload::Tuple(items) => items
            .iter()
            .all(|item| matches!(&item.payload, PyObjectPayload::Int(PyInt::Small(_)))),
        _ => return None,
    };
    if !all_small {
        return None;
    }

    let mut items = match &arg.payload {
        PyObjectPayload::List(v) if PyObjectRef::strong_count(arg) == 1 => {
            std::mem::take(unsafe { &mut *v.data_ptr() })
        }
        PyObjectPayload::List(v) => unsafe { &*v.data_ptr() }.clone(),
        PyObjectPayload::Tuple(items) => items.to_vec(),
        _ => return None,
    };
    items.sort_unstable_by(|a, b| {
        let av = if let PyObjectPayload::Int(PyInt::Small(v)) = &a.payload {
            *v
        } else {
            0
        };
        let bv = if let PyObjectPayload::Int(PyInt::Small(v)) = &b.payload {
            *v
        } else {
            0
        };
        av.cmp(&bv)
    });
    Some(PyObject::list(items))
}

#[inline(always)]
fn stack_tail_arg(stack: &[PyObjectRef], arg_count: usize, offset: usize) -> Option<&PyObjectRef> {
    let start = stack.len().checked_sub(arg_count)?;
    stack.get(start + offset)
}

#[inline(always)]
fn fast_len_value(arg: &PyObjectRef) -> Option<i64> {
    match &arg.payload {
        PyObjectPayload::List(v) => Some(unsafe { &*v.data_ptr() }.len() as i64),
        PyObjectPayload::Tuple(v) => Some(v.len() as i64),
        PyObjectPayload::Str(s) => Some(s.chars().count() as i64),
        PyObjectPayload::Dict(m) => {
            let map = unsafe { &*m.data_ptr() };
            Some(map.keys().filter(|k| !is_hidden_dict_key(k)).count() as i64)
        }
        PyObjectPayload::Set(m) => Some(unsafe { &*m.data_ptr() }.len() as i64),
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Some(b.len() as i64),
        _ => None,
    }
}

#[inline(always)]
fn fast_str_value(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Str(_) => Some(arg.clone()),
        PyObjectPayload::Int(PyInt::Small(n)) => {
            let mut buf = itoa::Buffer::new();
            Some(PyObject::str_val(CompactString::from(buf.format(*n))))
        }
        PyObjectPayload::Float(f) => {
            let mut buf = ryu::Buffer::new();
            Some(PyObject::str_val(CompactString::from(buf.format(*f))))
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

#[inline(always)]
fn fast_float_value(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Float(_) => Some(arg.clone()),
        PyObjectPayload::Int(PyInt::Small(n)) => Some(PyObject::float(*n as f64)),
        PyObjectPayload::Bool(b) => Some(PyObject::float(if *b { 1.0 } else { 0.0 })),
        _ => None,
    }
}

#[inline(always)]
fn fast_bool_value(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Bool(_) => Some(arg.clone()),
        PyObjectPayload::Int(PyInt::Small(n)) => Some(PyObject::bool_val(*n != 0)),
        PyObjectPayload::Float(f) => Some(PyObject::bool_val(*f != 0.0)),
        PyObjectPayload::None => Some(PyObject::bool_val(false)),
        PyObjectPayload::Str(s) => Some(PyObject::bool_val(!s.is_empty())),
        PyObjectPayload::List(v) => Some(PyObject::bool_val(!unsafe { &*v.data_ptr() }.is_empty())),
        PyObjectPayload::Tuple(v) => Some(PyObject::bool_val(!v.is_empty())),
        PyObjectPayload::Dict(m) => Some(PyObject::bool_val(!unsafe { &*m.data_ptr() }.is_empty())),
        _ => None,
    }
}

#[inline(always)]
fn fast_abs_value(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Int(PyInt::Small(n)) => Some(PyObject::int(n.abs())),
        PyObjectPayload::Float(f) => Some(PyObject::float(f.abs())),
        _ => None,
    }
}

#[inline(always)]
fn fast_small_int_sum(arg: &PyObjectRef) -> Option<i64> {
    match &arg.payload {
        PyObjectPayload::List(v) => fast_small_int_items_sum(unsafe { &*v.data_ptr() }),
        PyObjectPayload::Tuple(v) => fast_small_int_items_sum(v),
        PyObjectPayload::Range(rd) => {
            let n = if rd.step > 0 {
                if rd.start >= rd.stop {
                    0i64
                } else {
                    (rd.stop - rd.start + rd.step - 1) / rd.step
                }
            } else if rd.step < 0 {
                if rd.start <= rd.stop {
                    0i64
                } else {
                    (rd.start - rd.stop - rd.step - 1) / (-rd.step)
                }
            } else {
                0
            };
            if n == 0 {
                return Some(0);
            }
            let range_sum = (n as i128) * (rd.start as i128)
                + (rd.step as i128) * (n as i128) * ((n - 1) as i128) / 2;
            if range_sum >= i64::MIN as i128 && range_sum <= i64::MAX as i128 {
                Some(range_sum as i64)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[inline(always)]
fn fast_small_int_items_sum(items: &[PyObjectRef]) -> Option<i64> {
    let mut total: i64 = 0;
    for item in items {
        let PyObjectPayload::Int(PyInt::Small(n)) = &item.payload else {
            return None;
        };
        total = total.checked_add(*n)?;
    }
    Some(total)
}

#[inline(always)]
fn fast_two_arg_min_max(stack: &[PyObjectRef], is_max: bool) -> Option<PyObjectRef> {
    let sl = stack.len();
    let a = stack.get(sl.checked_sub(2)?)?;
    let b = stack.get(sl - 1)?;
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            Some(PyObject::int(if is_max {
                std::cmp::max(*x, *y)
            } else {
                std::cmp::min(*x, *y)
            }))
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
            Some(PyObject::float(if is_max { x.max(*y) } else { x.min(*y) }))
        }
        _ => None,
    }
}

#[inline(always)]
fn fast_builtin_type_match(obj: &PyObjectRef, builtin_type: &str) -> Option<bool> {
    match builtin_type {
        "int" => Some(matches!(
            &obj.payload,
            PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)
        )),
        "float" => Some(matches!(&obj.payload, PyObjectPayload::Float(_))),
        "str" => Some(matches!(&obj.payload, PyObjectPayload::Str(_))),
        "bool" => Some(matches!(&obj.payload, PyObjectPayload::Bool(_))),
        "list" => Some(matches!(&obj.payload, PyObjectPayload::List(_))),
        "dict" => Some(matches!(
            &obj.payload,
            PyObjectPayload::Dict(_) | PyObjectPayload::InstanceDict(_)
        )),
        "tuple" => Some(matches!(&obj.payload, PyObjectPayload::Tuple(_))),
        "set" => Some(matches!(&obj.payload, PyObjectPayload::Set(_))),
        "bytes" => Some(matches!(&obj.payload, PyObjectPayload::Bytes(_))),
        "bytearray" => Some(matches!(&obj.payload, PyObjectPayload::ByteArray(_))),
        "NoneType" => Some(matches!(&obj.payload, PyObjectPayload::None)),
        "generator" => Some(matches!(&obj.payload, PyObjectPayload::Generator(_))),
        "coroutine" => Some(matches!(&obj.payload, PyObjectPayload::Coroutine(_))),
        "async_generator" => Some(matches!(&obj.payload, PyObjectPayload::AsyncGenerator(_))),
        "frozenset" => Some(matches!(&obj.payload, PyObjectPayload::FrozenSet(_))),
        "range" => Some(matches!(&obj.payload, PyObjectPayload::Range(_))),
        "type" => Some(matches!(
            &obj.payload,
            PyObjectPayload::BuiltinType(_) | PyObjectPayload::Class(_)
        )),
        "method" => Some(matches!(&obj.payload, PyObjectPayload::BoundMethod { .. })),
        "builtin_method" => Some(matches!(
            &obj.payload,
            PyObjectPayload::BuiltinBoundMethod(_)
        )),
        "object" => Some(true),
        _ => None,
    }
}

#[inline(always)]
fn fast_class_match(obj: &PyObjectRef, cls: &PyObjectRef) -> Option<bool> {
    let PyObjectPayload::Class(cd) = &cls.payload else {
        return None;
    };
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    let PyObjectPayload::Class(obj_cd) = &inst.class.payload else {
        return None;
    };
    if obj_cd.name == cd.name {
        Some(true)
    } else if obj_cd
        .mro
        .iter()
        .any(|b| matches!(&b.payload, PyObjectPayload::Class(bc) if bc.name == cd.name))
    {
        Some(true)
    } else {
        None
    }
}

#[inline(always)]
fn fast_direct_isinstance(stack: &[PyObjectRef]) -> Option<PyObjectRef> {
    let sl = stack.len();
    let obj = stack.get(sl.checked_sub(2)?)?;
    let cls = stack.get(sl - 1)?;
    if matches!(&obj.payload, PyObjectPayload::Instance(_)) {
        return None;
    }
    let PyObjectPayload::BuiltinType(bt) = &cls.payload else {
        return None;
    };
    fast_builtin_type_match(obj, bt.as_str()).map(PyObject::bool_val)
}

#[inline(always)]
fn fast_callfunction_isinstance(stack: &[PyObjectRef]) -> Option<PyObjectRef> {
    let sl = stack.len();
    let obj = stack.get(sl.checked_sub(2)?)?;
    let cls = stack.get(sl - 1)?;
    let result = match &cls.payload {
        PyObjectPayload::BuiltinType(bt) => {
            if matches!(&obj.payload, PyObjectPayload::Instance(_)) {
                None
            } else {
                fast_builtin_type_match(obj, bt.as_str())
            }
        }
        PyObjectPayload::Class(_) => fast_class_match(obj, cls),
        _ => None,
    }?;
    Some(PyObject::bool_val(result))
}

#[inline(always)]
fn fast_loaded_type_isinstance_arg(
    func_obj: &PyObjectRef,
    stack: &[PyObjectRef],
) -> Option<PyObjectRef> {
    let sl = stack.len();
    let func = stack.get(sl.checked_sub(2)?)?;
    let obj = stack.get(sl - 1)?;
    let PyObjectPayload::BuiltinFunction(fn_name) = &func.payload else {
        return None;
    };
    if fn_name.as_str() != "isinstance" {
        return None;
    }
    match &func_obj.payload {
        PyObjectPayload::BuiltinType(bt) => {
            fast_builtin_type_match(obj, bt.as_str()).map(PyObject::bool_val)
        }
        PyObjectPayload::Class(_) => fast_class_match(obj, func_obj).map(PyObject::bool_val),
        _ => None,
    }
}

#[inline(always)]
fn fast_mixed_two_arg_min_max(stack: &[PyObjectRef], is_max: bool) -> Option<PyObjectRef> {
    let sl = stack.len();
    let a = stack.get(sl.checked_sub(2)?)?;
    let b = stack.get(sl - 1)?;
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            Some(PyObject::int(if is_max {
                std::cmp::max(*x, *y)
            } else {
                std::cmp::min(*x, *y)
            }))
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
            Some(PyObject::float(if is_max { x.max(*y) } else { x.min(*y) }))
        }
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
            let xf = *x as f64;
            Some(if (is_max && xf >= *y) || (!is_max && xf <= *y) {
                PyObject::int(*x)
            } else {
                PyObject::float(*y)
            })
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
            let yf = *y as f64;
            Some(if (is_max && *x >= yf) || (!is_max && *x <= yf) {
                PyObject::float(*x)
            } else {
                PyObject::int(*y)
            })
        }
        _ => None,
    }
}

#[inline(always)]
fn fast_small_int_sum_with_start(iterable: &PyObjectRef, start: i64) -> Option<i64> {
    match &iterable.payload {
        PyObjectPayload::List(v) => {
            fast_small_int_items_sum_with_start(unsafe { &*v.data_ptr() }, start)
        }
        PyObjectPayload::Tuple(v) => fast_small_int_items_sum_with_start(v, start),
        PyObjectPayload::Range(rd) => {
            let n = if rd.step > 0 {
                if rd.start >= rd.stop {
                    0i64
                } else {
                    (rd.stop - rd.start + rd.step - 1) / rd.step
                }
            } else if rd.step < 0 {
                if rd.start <= rd.stop {
                    0i64
                } else {
                    (rd.start - rd.stop - rd.step - 1) / (-rd.step)
                }
            } else {
                0
            };
            if n == 0 {
                return Some(start);
            }
            let range_sum = (n as i128) * (rd.start as i128)
                + (rd.step as i128) * (n as i128) * ((n - 1) as i128) / 2;
            let total = start as i128 + range_sum;
            if total >= i64::MIN as i128 && total <= i64::MAX as i128 {
                Some(total as i64)
            } else {
                None
            }
        }
        _ => None,
    }
}

#[inline(always)]
fn fast_small_int_items_sum_with_start(items: &[PyObjectRef], start: i64) -> Option<i64> {
    let mut total = start;
    for item in items {
        let PyObjectPayload::Int(PyInt::Small(n)) = &item.payload else {
            return None;
        };
        total = total.checked_add(*n)?;
    }
    Some(total)
}

#[inline(always)]
pub(crate) fn try_fast_callfunction_builtin(
    func_obj: &PyObjectRef,
    stack: &[PyObjectRef],
    arg_count: usize,
) -> Option<PyObjectRef> {
    let builtin_name = match &func_obj.payload {
        PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
            Some(name.as_str())
        }
        _ => None,
    };

    match (builtin_name, arg_count) {
        (Some("setattr"), 3) | (Some("next"), 1) | (Some("next"), 2) => None,
        (Some("isinstance"), 2) => fast_callfunction_isinstance(stack),
        (Some("hasattr"), 2) => {
            let name_arg = stack_tail_arg(stack, 2, 1)?;
            let PyObjectPayload::Str(name) = &name_arg.payload else {
                return None;
            };
            let obj = stack_tail_arg(stack, 2, 0)?;
            Some(PyObject::bool_val(ferrython_core::object::py_has_attr(
                obj,
                name.as_str(),
            )))
        }
        (Some("getattr"), 2) => {
            let name_arg = stack_tail_arg(stack, 2, 1)?;
            let PyObjectPayload::Str(name) = &name_arg.payload else {
                return None;
            };
            let obj = stack_tail_arg(stack, 2, 0)?;
            if matches!(&obj.payload, PyObjectPayload::Instance(inst)
                if inst.attrs.read().contains_key("__weakref_target__")
                    && !inst.attrs.read().contains_key("__weakref_ref__"))
            {
                return None;
            }
            let attr = obj.get_attr(name.as_str())?;
            if matches!(&obj.payload, PyObjectPayload::Instance(_))
                && ferrython_core::object::is_property_like(&attr)
            {
                return None;
            }
            if matches!(&obj.payload, PyObjectPayload::Class(_))
                && ferrython_core::object::is_dynamic_class_attribute(&attr)
            {
                return None;
            }
            Some(attr)
        }
        (Some("sum"), 1) | (Some("sum"), 2) => {
            let start = if arg_count == 2 {
                match &stack_tail_arg(stack, 2, 1)?.payload {
                    PyObjectPayload::Int(PyInt::Small(start)) => *start,
                    _ => return None,
                }
            } else {
                0
            };
            let iterable = stack_tail_arg(stack, arg_count, 0)?;
            fast_small_int_sum_with_start(iterable, start).map(PyObject::int)
        }
        (Some("min"), 2) => fast_mixed_two_arg_min_max(stack, false),
        (Some("max"), 2) => fast_mixed_two_arg_min_max(stack, true),
        _ => try_fast_global_builtin_call(func_obj, stack, arg_count),
    }
}

#[inline(always)]
pub(crate) fn try_fast_global_builtin_call(
    func_obj: &PyObjectRef,
    stack: &[PyObjectRef],
    arg_count: usize,
) -> Option<PyObjectRef> {
    let builtin_name = match &func_obj.payload {
        PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
            Some(name.as_str())
        }
        _ => None,
    };

    match (builtin_name, arg_count) {
        (Some("len"), 1) => fast_len_value(stack_tail_arg(stack, 1, 0)?).map(PyObject::int),
        (Some("range"), 1) => {
            let PyObjectPayload::Int(PyInt::Small(stop)) = &stack_tail_arg(stack, 1, 0)?.payload
            else {
                return None;
            };
            Some(PyObject::range(0, *stop, 1))
        }
        (Some("str"), 1) => fast_str_value(stack_tail_arg(stack, 1, 0)?),
        (Some("isinstance"), 2) => fast_direct_isinstance(stack),
        (Some("type"), 1) => fast_exact_type(stack_tail_arg(stack, 1, 0)?),
        (Some("int"), 1) => fast_int_conversion(stack_tail_arg(stack, 1, 0)?),
        (Some("callable"), 1) => {
            fast_callable_bool(stack_tail_arg(stack, 1, 0)?).map(PyObject::bool_val)
        }
        (Some("float"), 1) => fast_float_value(stack_tail_arg(stack, 1, 0)?),
        (Some("bool"), 1) => fast_bool_value(stack_tail_arg(stack, 1, 0)?),
        (Some("abs"), 1) => fast_abs_value(stack_tail_arg(stack, 1, 0)?),
        (Some("sum"), 1) => fast_small_int_sum(stack_tail_arg(stack, 1, 0)?).map(PyObject::int),
        (Some("min"), 1) => fast_small_int_sequence_min_max(stack_tail_arg(stack, 1, 0)?, false),
        (Some("max"), 1) => fast_small_int_sequence_min_max(stack_tail_arg(stack, 1, 0)?, true),
        (Some("sorted"), 1) => fast_small_int_sequence_sorted(stack_tail_arg(stack, 1, 0)?),
        (Some("min"), 2) => fast_two_arg_min_max(stack, false),
        (Some("max"), 2) => fast_two_arg_min_max(stack, true),
        _ if arg_count == 2 => fast_loaded_type_isinstance_arg(func_obj, stack),
        _ => None,
    }
}

#[inline(always)]
pub(crate) fn fast_exact_type(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::None => Some(PyObject::builtin_type_by_name("NoneType")),
        PyObjectPayload::Ellipsis => Some(PyObject::builtin_type_by_name("ellipsis")),
        PyObjectPayload::NotImplemented => {
            Some(PyObject::builtin_type_by_name("NotImplementedType"))
        }
        PyObjectPayload::Bool(_) => Some(PyObject::builtin_type_by_name("bool")),
        PyObjectPayload::Int(_) => Some(PyObject::builtin_type_by_name("int")),
        PyObjectPayload::Float(_) => Some(PyObject::builtin_type_by_name("float")),
        PyObjectPayload::Complex { .. } => Some(PyObject::builtin_type_by_name("complex")),
        PyObjectPayload::Str(_) => Some(PyObject::builtin_type_by_name("str")),
        PyObjectPayload::Bytes(_) => Some(PyObject::builtin_type_by_name("bytes")),
        PyObjectPayload::ByteArray(_) => Some(PyObject::builtin_type_by_name("bytearray")),
        PyObjectPayload::List(_) => Some(PyObject::builtin_type_by_name("list")),
        PyObjectPayload::Deque(_) => Some(PyObject::builtin_type_by_name("deque")),
        PyObjectPayload::Tuple(_) => Some(PyObject::builtin_type_by_name("tuple")),
        PyObjectPayload::Set(_) => Some(PyObject::builtin_type_by_name("set")),
        PyObjectPayload::FrozenSet(_) => Some(PyObject::builtin_type_by_name("frozenset")),
        PyObjectPayload::Dict(_) | PyObjectPayload::InstanceDict(_) => {
            Some(PyObject::builtin_type_by_name("dict"))
        }
        PyObjectPayload::MappingProxy(_) => Some(PyObject::builtin_type_by_name("mappingproxy")),
        PyObjectPayload::Function(_) => Some(PyObject::builtin_type_by_name("function")),
        PyObjectPayload::BuiltinFunction(_)
        | PyObjectPayload::NativeFunction(_)
        | PyObjectPayload::NativeClosure(_) => {
            Some(PyObject::builtin_type_by_name("builtin_function_or_method"))
        }
        PyObjectPayload::BuiltinType(_)
        | PyObjectPayload::Class(_)
        | PyObjectPayload::ExceptionType(_) => Some(PyObject::builtin_type_by_name("type")),
        PyObjectPayload::BoundMethod { .. } => Some(PyObject::builtin_type_by_name("method")),
        PyObjectPayload::BuiltinBoundMethod(_) => {
            Some(PyObject::builtin_type_by_name("builtin_method"))
        }
        PyObjectPayload::Code(_) => Some(PyObject::builtin_type_by_name("code")),
        PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
        PyObjectPayload::Module(_) => Some(PyObject::builtin_type_by_name("module")),
        PyObjectPayload::Iterator(iter_data) => {
            let guard = iter_data.read();
            let name = match &*guard {
                IteratorData::MapOne { .. } | IteratorData::Map { .. } => "map",
                IteratorData::Filter { .. } => "filter",
                IteratorData::Zip { .. } => "zip",
                IteratorData::ZipLongest { .. } => "itertools.zip_longest",
                IteratorData::Islice { .. } => "itertools.islice",
                IteratorData::Enumerate { .. } => "enumerate",
                IteratorData::Range { .. } | IteratorData::BigRange(_) => "range_iterator",
                IteratorData::List { .. } => "list_iterator",
                IteratorData::Tuple { .. } => "tuple_iterator",
                IteratorData::Str { .. } => "str_ascii_iterator",
                IteratorData::DictEntries { .. } => "dict_itemiterator",
                IteratorData::DictKeys { .. } | IteratorData::DictKeyRefs { .. } => {
                    "dict_keyiterator"
                }
                IteratorData::SetRefs { .. } | IteratorData::FrozenSetItems { .. } => {
                    "set_iterator"
                }
                IteratorData::Sentinel { .. } => "callable_iterator",
                IteratorData::Tee { .. } => "itertools._tee",
                _ => "iterator",
            };
            Some(PyObject::builtin_type_by_name(name))
        }
        PyObjectPayload::RangeIter(_) => Some(PyObject::builtin_type_by_name("range_iterator")),
        PyObjectPayload::VecIter(_)
        | PyObjectPayload::DictValueIter(_)
        | PyObjectPayload::WeakValueIter(_)
        | PyObjectPayload::WeakKeyIter(_)
        | PyObjectPayload::RefIter { .. } => Some(PyObject::builtin_type_by_name("list_iterator")),
        PyObjectPayload::DequeIter(data) => {
            let name = if data.reverse {
                "_deque_reverse_iterator"
            } else {
                "_deque_iterator"
            };
            Some(PyObject::builtin_type_by_name(name))
        }
        PyObjectPayload::RevRefIter { .. } => {
            Some(PyObject::builtin_type_by_name("list_reverseiterator"))
        }
        PyObjectPayload::Slice(_) => Some(PyObject::builtin_type_by_name("slice")),
        PyObjectPayload::Cell(_) => Some(PyObject::builtin_type_by_name("cell")),
        PyObjectPayload::ExceptionInstance(ei) => Some(PyObject::exception_type(ei.kind)),
        PyObjectPayload::Generator(_) => Some(PyObject::builtin_type_by_name("generator")),
        PyObjectPayload::Coroutine(_) | PyObjectPayload::BuiltinAwaitable(_) => {
            Some(PyObject::builtin_type_by_name("coroutine"))
        }
        PyObjectPayload::AsyncGenerator(_) => {
            Some(PyObject::builtin_type_by_name("async_generator"))
        }
        PyObjectPayload::AsyncGenAwaitable { .. } => {
            Some(PyObject::builtin_type_by_name("async_generator_asend"))
        }
        PyObjectPayload::Property(_) => Some(PyObject::builtin_type_by_name("property")),
        PyObjectPayload::StaticMethod(_) => Some(PyObject::builtin_type_by_name("staticmethod")),
        PyObjectPayload::ClassMethod(_) => Some(PyObject::builtin_type_by_name("classmethod")),
        PyObjectPayload::Super { .. } => Some(PyObject::builtin_type_by_name("super")),
        PyObjectPayload::Partial(_) => Some(PyObject::builtin_type_by_name("functools.partial")),
        PyObjectPayload::Range(_) => Some(PyObject::builtin_type_by_name("range")),
        PyObjectPayload::DeferredSleep { .. } => Some(PyObject::builtin_type_by_name("coroutine")),
        PyObjectPayload::DictKeys { .. } => Some(PyObject::builtin_type_by_name("dict_keys")),
        PyObjectPayload::DictValues { .. } => Some(PyObject::builtin_type_by_name("dict_values")),
        PyObjectPayload::DictItems { .. } => Some(PyObject::builtin_type_by_name("dict_items")),
    }
}
