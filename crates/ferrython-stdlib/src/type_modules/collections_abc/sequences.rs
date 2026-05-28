use super::helpers::{add_method, make_index_iterator};
use super::*;

pub(super) fn add_sequence_methods(sequence_cls: &PyObjectRef, mutable_sequence_cls: &PyObjectRef) {
    add_method(
        &sequence_cls,
        "__contains__",
        PyObject::native_closure("Sequence.__contains__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let self_obj = &args[0];
            let target = &args[1];
            let len = self_obj.py_len()? as i64;
            for i in 0..len {
                let item = self_obj.get_item(&PyObject::int(i))?;
                if item
                    .compare(target, CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    return Ok(PyObject::bool_val(true));
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );
    add_method(
        &sequence_cls,
        "__iter__",
        PyObject::native_closure("Sequence.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Sequence.__iter__ requires self"));
            }
            make_index_iterator(&args[0], false)
        }),
    );
    add_method(
        &sequence_cls,
        "__reversed__",
        PyObject::native_closure("Sequence.__reversed__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "Sequence.__reversed__ requires self",
                ));
            }
            make_index_iterator(&args[0], true)
        }),
    );
    add_method(
        &sequence_cls,
        "index",
        PyObject::native_closure("Sequence.index", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("index() requires 1 argument"));
            }
            let self_obj = &args[0];
            let target = &args[1];
            let len = self_obj.py_len()? as i64;
            let start = if args.len() > 2 {
                args[2].to_int().unwrap_or(0)
            } else {
                0
            };
            let stop = if args.len() > 3 {
                args[3].to_int().unwrap_or(len)
            } else {
                len
            };
            let start = if start < 0 {
                (len + start).max(0)
            } else {
                start
            }
            .min(len);
            let stop = if stop < 0 { (len + stop).max(0) } else { stop }.min(len);
            for i in start..stop {
                let item = self_obj.get_item(&PyObject::int(i))?;
                if item
                    .compare(target, CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    return Ok(PyObject::int(i));
                }
            }
            Err(PyException::value_error(format!(
                "{} is not in sequence",
                target.py_to_string()
            )))
        }),
    );
    add_method(
        &sequence_cls,
        "count",
        PyObject::native_closure("Sequence.count", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("count() requires 1 argument"));
            }
            let self_obj = &args[0];
            let target = &args[1];
            let len = self_obj.py_len()? as i64;
            let mut count = 0i64;
            for i in 0..len {
                let item = self_obj.get_item(&PyObject::int(i))?;
                if item
                    .compare(target, CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    count += 1;
                }
            }
            Ok(PyObject::int(count))
        }),
    );

    add_method(
        &mutable_sequence_cls,
        "append",
        PyObject::native_closure("MutableSequence.append", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("append() requires 1 argument"));
            }
            let self_obj = &args[0];
            let insert = self_obj
                .get_attr("insert")
                .ok_or_else(|| PyException::attribute_error("insert"))?;
            let len = self_obj.py_len()? as i64;
            ferrython_core::object::helpers::call_callable(
                &insert,
                &[PyObject::int(len), args[1].clone()],
            )?;
            Ok(PyObject::none())
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "extend",
        PyObject::native_closure("MutableSequence.extend", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("extend() requires 1 argument"));
            }
            let self_obj = &args[0];
            let insert = self_obj
                .get_attr("insert")
                .ok_or_else(|| PyException::attribute_error("insert"))?;
            let mut idx = self_obj.py_len()? as i64;
            for item in args[1].to_list()? {
                ferrython_core::object::helpers::call_callable(
                    &insert,
                    &[PyObject::int(idx), item],
                )?;
                idx += 1;
            }
            Ok(PyObject::none())
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "pop",
        PyObject::native_closure("MutableSequence.pop", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("pop() requires self"));
            }
            let self_obj = &args[0];
            let len = self_obj.py_len()? as i64;
            if len == 0 {
                return Err(PyException::index_error("pop from empty list"));
            }
            let idx = if args.len() > 1 {
                args[1].to_int().unwrap_or(-1)
            } else {
                -1
            };
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("pop index out of range"));
            }
            let item = self_obj.get_item(&PyObject::int(actual))?;
            let del = self_obj
                .get_attr("__delitem__")
                .ok_or_else(|| PyException::attribute_error("__delitem__"))?;
            ferrython_core::object::helpers::call_callable(&del, &[PyObject::int(actual)])?;
            Ok(item)
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "remove",
        PyObject::native_closure("MutableSequence.remove", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("remove() requires 1 argument"));
            }
            let self_obj = &args[0];
            let del = self_obj
                .get_attr("__delitem__")
                .ok_or_else(|| PyException::attribute_error("__delitem__"))?;
            let len = self_obj.py_len()? as i64;
            for i in 0..len {
                let item = self_obj.get_item(&PyObject::int(i))?;
                if item
                    .compare(&args[1], CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    ferrython_core::object::helpers::call_callable(&del, &[PyObject::int(i)])?;
                    return Ok(PyObject::none());
                }
            }
            Err(PyException::value_error("list.remove(x): x not in list"))
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "clear",
        PyObject::native_closure("MutableSequence.clear", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("clear() requires self"));
            }
            let self_obj = &args[0];
            let pop = self_obj
                .get_attr("pop")
                .ok_or_else(|| PyException::attribute_error("pop"))?;
            while self_obj.py_len()? > 0 {
                ferrython_core::object::helpers::call_callable(&pop, &[])?;
            }
            Ok(PyObject::none())
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "reverse",
        PyObject::native_closure("MutableSequence.reverse", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("reverse() requires self"));
            }
            let self_obj = &args[0];
            let len = self_obj.py_len()? as i64;
            let setitem = self_obj
                .get_attr("__setitem__")
                .ok_or_else(|| PyException::attribute_error("__setitem__"))?;
            let mut items = Vec::new();
            for i in 0..len {
                items.push(self_obj.get_item(&PyObject::int(i))?);
            }
            for (i, item) in items.into_iter().rev().enumerate() {
                ferrython_core::object::helpers::call_callable(
                    &setitem,
                    &[PyObject::int(i as i64), item],
                )?;
            }
            Ok(PyObject::none())
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "__iadd__",
        PyObject::native_closure("MutableSequence.__iadd__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("__iadd__ requires other"));
            }
            let self_obj = &args[0];
            let extend = self_obj
                .get_attr("extend")
                .ok_or_else(|| PyException::attribute_error("extend"))?;
            ferrython_core::object::helpers::call_callable(&extend, &[args[1].clone()])?;
            Ok(self_obj.clone())
        }),
    );
}
