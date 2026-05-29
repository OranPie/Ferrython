use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::{checked_repeat_len, guard_eager_allocation};
use ferrython_core::object::{
    CompareOp, DequeIterData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    SyncUsize,
};
use ferrython_core::types::PyInt;
use num_traits::Signed;

pub(crate) fn call_deque_method(
    receiver: &PyObjectRef,
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    let get_data = || -> PyObjectRef {
        inst.attrs
            .read()
            .get("_data")
            .cloned()
            .unwrap_or_else(|| PyObject::list(vec![]))
    };
    let get_maxlen = || -> Option<usize> {
        inst.attrs
            .read()
            .get("__maxlen__")
            .and_then(|v| v.as_int())
            .map(|n| n as usize)
    };
    let normalize_index = |idx: i64, len: usize| -> PyResult<usize> {
        let len_i = len as i64;
        let actual = if idx < 0 { len_i + idx } else { idx };
        if actual < 0 || actual >= len_i {
            Err(PyException::new(
                ExceptionKind::IndexError,
                "deque index out of range",
            ))
        } else {
            Ok(actual as usize)
        }
    };
    let normalize_insert_index = |idx: i64, len: usize| -> usize {
        let len_i = len as i64;
        let actual = if idx < 0 {
            (len_i + idx).max(0)
        } else {
            idx.min(len_i)
        };
        actual as usize
    };
    let clamp_slice_bound = |obj: &PyObjectRef, len: usize, default: usize| -> PyResult<usize> {
        let index = if matches!(obj.payload, PyObjectPayload::None) {
            return Ok(default);
        } else {
            obj.to_index()?
        };
        match index {
            PyInt::Small(raw) => {
                let len_i = len as i64;
                let bounded = if raw < 0 {
                    len_i.saturating_add(raw).max(0)
                } else {
                    raw.min(len_i)
                };
                Ok(bounded as usize)
            }
            PyInt::Big(big) if big.is_negative() => Ok(0),
            PyInt::Big(_) => Ok(len),
        }
    };
    let deque_item_matches = |item: &PyObjectRef, target: &PyObjectRef| -> PyResult<bool> {
        if PyObjectRef::ptr_eq(item, target) {
            return Ok(true);
        }
        Ok(item.compare(target, CompareOp::Eq)?.is_truthy())
    };
    let deque_items = |obj: &PyObjectRef| -> PyResult<Vec<PyObjectRef>> {
        if let PyObjectPayload::Instance(other_inst) = &obj.payload {
            if other_inst.attrs.read().contains_key("__deque__") {
                if let Some(data) = other_inst.attrs.read().get("_data").cloned() {
                    if let PyObjectPayload::List(list) = &data.payload {
                        return Ok(list.read().clone());
                    }
                }
            }
        }
        obj.to_list()
    };
    let trim_to_maxlen = |items: &mut Vec<PyObjectRef>| {
        if let Some(ml) = get_maxlen() {
            if items.len() > ml {
                *items = items[items.len() - ml..].to_vec();
            }
        }
    };
    let build_like_self = |items: Vec<PyObjectRef>| -> PyObjectRef {
        let new_inst = PyObject::instance(inst.class.clone());
        if let PyObjectPayload::Instance(ref new_data) = new_inst.payload {
            let mut attrs = new_data.attrs.write();
            attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
            attrs.insert(CompactString::from("_data"), PyObject::list(items));
            attrs.insert(
                CompactString::from("__maxlen__"),
                inst.attrs
                    .read()
                    .get("__maxlen__")
                    .cloned()
                    .unwrap_or_else(PyObject::none),
            );
        }
        new_inst
    };
    let repeat_deque_items = |base: &[PyObjectRef], n: usize| -> PyResult<Vec<PyObjectRef>> {
        if base.is_empty() || n == 0 {
            return Ok(Vec::new());
        }
        if let Some(ml) = get_maxlen() {
            let total = base.len().checked_mul(n).ok_or_else(|| {
                PyException::new(ExceptionKind::MemoryError, "deque repeat is too large")
            })?;
            let keep = total.min(ml);
            guard_eager_allocation(keep, "deque repeat")?;
            let start = total.saturating_sub(keep);
            let mut items = Vec::with_capacity(keep);
            for pos in start..total {
                items.push(base[pos % base.len()].clone());
            }
            return Ok(items);
        }
        let len = checked_repeat_len(base.len(), n, "deque repeat")?;
        let mut items = Vec::with_capacity(len);
        for _ in 0..n {
            items.extend(base.iter().cloned());
        }
        Ok(items)
    };
    // Helper: enforce maxlen by trimming from the appropriate end
    let enforce_maxlen_right = |list: &PyCell<Vec<PyObjectRef>>| {
        if let Some(ml) = get_maxlen() {
            let mut v = list.write();
            while v.len() > ml {
                v.remove(0); // trim from left when appending to right
            }
        }
    };
    let enforce_maxlen_left = |list: &PyCell<Vec<PyObjectRef>>| {
        if let Some(ml) = get_maxlen() {
            let mut v = list.write();
            while v.len() > ml {
                v.pop(); // trim from right when appending to left
            }
        }
    };
    match method {
        "__init__" => {
            if args.len() > 2 {
                return Err(PyException::type_error(
                    "deque() expected at most 2 arguments",
                ));
            }
            let new_maxlen =
                if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
                    let raw = args[1].to_int()?;
                    if raw < 0 {
                        return Err(PyException::value_error("maxlen must be non-negative"));
                    }
                    Some(raw as usize)
                } else {
                    None
                };
            let mut items = if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None)
            {
                Vec::new()
            } else {
                args[0].to_list()?
            };
            if let Some(ml) = new_maxlen {
                if items.len() > ml {
                    items = items[items.len() - ml..].to_vec();
                }
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                *list.write() = items;
            }
            inst.attrs.write().insert(
                CompactString::from("__maxlen__"),
                new_maxlen
                    .map(|n| PyObject::int(n as i64))
                    .unwrap_or_else(PyObject::none),
            );
            Ok(PyObject::none())
        }
        "append" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "append() takes exactly one argument",
                ));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().push(args[0].clone());
                enforce_maxlen_right(list);
            }
            Ok(PyObject::none())
        }
        "appendleft" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "appendleft() takes exactly one argument",
                ));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().insert(0, args[0].clone());
                enforce_maxlen_left(list);
            }
            Ok(PyObject::none())
        }
        "pop" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                if v.is_empty() {
                    return Err(PyException::new(
                        ExceptionKind::IndexError,
                        "pop from an empty deque",
                    ));
                }
                return Ok(v.pop().unwrap());
            }
            Ok(PyObject::none())
        }
        "popleft" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                if v.is_empty() {
                    return Err(PyException::new(
                        ExceptionKind::IndexError,
                        "pop from an empty deque",
                    ));
                }
                return Ok(v.remove(0));
            }
            Ok(PyObject::none())
        }
        "extend" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "extend() takes exactly one argument",
                ));
            }
            // args[0] should be pre-collected items as a List (VM collects iterable before calling)
            let items = args[0].to_list()?;
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().extend(items);
                enforce_maxlen_right(list);
            }
            Ok(PyObject::none())
        }
        "extendleft" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "extendleft() takes exactly one argument",
                ));
            }
            let items = args[0].to_list()?;
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                // CPython: appendleft each item in order — insert(0) naturally reverses
                for item in items.into_iter() {
                    v.insert(0, item);
                }
                drop(v);
                enforce_maxlen_left(list);
            }
            Ok(PyObject::none())
        }
        "rotate" => {
            if args.len() > 1 {
                return Err(PyException::type_error(
                    "rotate() takes at most one argument",
                ));
            }
            let n = if args.is_empty() {
                1i64
            } else {
                args[0]
                    .to_int()
                    .map_err(|_| PyException::type_error("an integer is required for rotate"))?
            };
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                let len = v.len() as i64;
                if len > 0 {
                    let n = ((n % len) + len) % len;
                    let split = v.len() - n as usize;
                    let tail: Vec<_> = v.drain(split..).collect();
                    for (i, item) in tail.into_iter().enumerate() {
                        v.insert(i, item);
                    }
                }
            }
            Ok(PyObject::none())
        }
        "clear" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().clear();
            }
            Ok(PyObject::none())
        }
        "copy" | "__copy__" => {
            let data = get_data();
            let items = data.to_list()?;
            Ok(build_like_self(items))
        }
        "count" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "count() takes exactly one argument",
                ));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let expected_len = list.read().len();
                let mut count = 0usize;
                for i in 0..expected_len {
                    let item = {
                        let v = list.read();
                        if v.len() != expected_len {
                            return Err(PyException::runtime_error(
                                "deque mutated during iteration",
                            ));
                        }
                        v[i].clone()
                    };
                    if deque_item_matches(&item, &args[0])? {
                        count += 1;
                    }
                    if list.read().len() != expected_len {
                        return Err(PyException::runtime_error("deque mutated during iteration"));
                    }
                }
                return Ok(PyObject::int(count as i64));
            }
            Ok(PyObject::int(0))
        }
        "index" => {
            if args.is_empty() || args.len() > 3 {
                return Err(PyException::type_error(
                    "index() takes at least 1 and at most 3 arguments",
                ));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let expected_len = list.read().len();
                let start = args
                    .get(1)
                    .map(|arg| clamp_slice_bound(arg, expected_len, 0))
                    .transpose()?
                    .unwrap_or(0);
                let stop = args
                    .get(2)
                    .map(|arg| clamp_slice_bound(arg, expected_len, expected_len))
                    .transpose()?
                    .unwrap_or(expected_len);
                for i in start..stop {
                    let item = {
                        let v = list.read();
                        if v.len() != expected_len {
                            return Err(PyException::runtime_error(
                                "deque mutated during iteration",
                            ));
                        }
                        v[i].clone()
                    };
                    if deque_item_matches(&item, &args[0])? {
                        return Ok(PyObject::int(i as i64));
                    }
                    if list.read().len() != expected_len {
                        return Err(PyException::runtime_error("deque mutated during iteration"));
                    }
                }
                return Err(PyException::new(
                    ExceptionKind::ValueError,
                    format!("{} is not in deque", args[0].py_to_string()),
                ));
            }
            Err(PyException::new(
                ExceptionKind::ValueError,
                "deque index error",
            ))
        }
        "insert" => {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "insert() takes exactly 2 arguments",
                ));
            }
            if let Some(ml) = get_maxlen() {
                let data = get_data();
                if let PyObjectPayload::List(list) = &data.payload {
                    if list.read().len() >= ml {
                        return Err(PyException::new(
                            ExceptionKind::IndexError,
                            "deque already at its maximum size",
                        ));
                    }
                }
            }
            let idx = args[0].to_int()?;
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                let idx = normalize_insert_index(idx, v.len());
                v.insert(idx, args[1].clone());
            }
            Ok(PyObject::none())
        }
        "remove" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "remove() takes exactly one argument",
                ));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let expected_len = list.read().len();
                for pos in 0..expected_len {
                    let item = {
                        let v = list.read();
                        if v.len() != expected_len {
                            return Err(PyException::index_error("deque mutated during iteration"));
                        }
                        v[pos].clone()
                    };
                    if deque_item_matches(&item, &args[0])? {
                        if list.read().len() != expected_len {
                            return Err(PyException::index_error("deque mutated during iteration"));
                        }
                        list.write().remove(pos);
                        return Ok(PyObject::none());
                    }
                    if list.read().len() != expected_len {
                        return Err(PyException::index_error("deque mutated during iteration"));
                    }
                }
                return Err(PyException::new(
                    ExceptionKind::ValueError,
                    "deque.remove(x): x not in deque",
                ));
            }
            Ok(PyObject::none())
        }
        "reverse" => {
            if !args.is_empty() {
                return Err(PyException::type_error("reverse() takes no arguments"));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().reverse();
            }
            Ok(PyObject::none())
        }
        "__repr__" | "__str__" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let items: Vec<String> = list.read().iter().map(|item| item.repr()).collect();
                let joined = items.join(", ");
                let text = match get_maxlen() {
                    Some(m) => format!("deque([{}], maxlen={})", joined, m),
                    None => format!("deque([{}])", joined),
                };
                return Ok(PyObject::str_val(CompactString::from(text)));
            }
            Ok(PyObject::str_val(CompactString::from("deque([])")))
        }
        "__eq__" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "__eq__() takes exactly one argument",
                ));
            }
            if args[0].get_attr("__deque__").is_none() {
                return Ok(PyObject::bool_val(false));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let other = args[0].to_list()?;
                let items = list.read();
                if items.len() != other.len() {
                    return Ok(PyObject::bool_val(false));
                }
                for (left, right) in items.iter().zip(other.iter()) {
                    if !deque_item_matches(left, right)? {
                        return Ok(PyObject::bool_val(false));
                    }
                }
                return Ok(PyObject::bool_val(true));
            }
            Ok(PyObject::bool_val(false))
        }
        "__ne__" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "__ne__() takes exactly one argument",
                ));
            }
            let eq = call_deque_method(receiver, inst, "__eq__", args)?;
            Ok(PyObject::bool_val(!eq.is_truthy()))
        }
        "__lt__" | "__le__" | "__gt__" | "__ge__" => {
            if args.len() != 1 {
                return Err(PyException::type_error(format!(
                    "{}() takes exactly one argument",
                    method
                )));
            }
            if args[0].get_attr("__deque__").is_none() {
                return Ok(PyObject::not_implemented());
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let left = list.read().clone();
                let right = args[0].to_list()?;
                let mut ordering = std::cmp::Ordering::Equal;
                for (l, r) in left.iter().zip(right.iter()) {
                    if deque_item_matches(l, r)? {
                        continue;
                    }
                    ordering = if l.compare(r, CompareOp::Lt)?.is_truthy() {
                        std::cmp::Ordering::Less
                    } else {
                        std::cmp::Ordering::Greater
                    };
                    break;
                }
                if ordering == std::cmp::Ordering::Equal {
                    ordering = left.len().cmp(&right.len());
                }
                let result = match method {
                    "__lt__" => ordering == std::cmp::Ordering::Less,
                    "__le__" => !matches!(ordering, std::cmp::Ordering::Greater),
                    "__gt__" => ordering == std::cmp::Ordering::Greater,
                    "__ge__" => !matches!(ordering, std::cmp::Ordering::Less),
                    _ => unreachable!(),
                };
                return Ok(PyObject::bool_val(result));
            }
            Ok(PyObject::not_implemented())
        }
        "__add__" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "__add__() takes exactly one argument",
                ));
            }
            if args[0].get_attr("__deque__").is_none() {
                return Ok(PyObject::not_implemented());
            }
            let mut items = deque_items(&get_data())?;
            items.extend(deque_items(&args[0])?);
            trim_to_maxlen(&mut items);
            Ok(build_like_self(items))
        }
        "__mul__" | "__rmul__" => {
            if args.len() != 1 {
                return Err(PyException::type_error(format!(
                    "{}() takes exactly one argument",
                    method
                )));
            }
            let n = match args[0].as_int() {
                Some(n) => n.max(0) as usize,
                None => return Ok(PyObject::not_implemented()),
            };
            let base = deque_items(&get_data())?;
            let items = repeat_deque_items(&base, n)?;
            Ok(build_like_self(items))
        }
        "__iadd__" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "__iadd__() takes exactly one argument",
                ));
            }
            let mut items = deque_items(&args[0])?;
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                v.append(&mut items);
            }
            if let PyObjectPayload::List(list) = &data.payload {
                enforce_maxlen_right(list);
            }
            Ok(PyObject::none())
        }
        "__imul__" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "__imul__() takes exactly one argument",
                ));
            }
            let n = match args[0].as_int() {
                Some(n) => n.max(0) as usize,
                None => return Ok(PyObject::not_implemented()),
            };
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let base = list.read().clone();
                let items = repeat_deque_items(&base, n)?;
                *list.write() = items;
            }
            Ok(PyObject::none())
        }
        "maxlen" => {
            // Property-like access: return maxlen value
            let ml = inst
                .attrs
                .read()
                .get("__maxlen__")
                .cloned()
                .unwrap_or_else(PyObject::none);
            Ok(ml)
        }
        "__len__" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                return Ok(PyObject::int(list.read().len() as i64));
            }
            Ok(PyObject::int(0))
        }
        "__contains__" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "__contains__() takes exactly one argument",
                ));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let expected_len = list.read().len();
                for i in 0..expected_len {
                    let item = {
                        let v = list.read();
                        if v.len() != expected_len {
                            return Err(PyException::runtime_error(
                                "deque mutated during iteration",
                            ));
                        }
                        v[i].clone()
                    };
                    if deque_item_matches(&item, &args[0])? {
                        return Ok(PyObject::bool_val(true));
                    }
                    if list.read().len() != expected_len {
                        return Err(PyException::runtime_error("deque mutated during iteration"));
                    }
                }
            }
            Ok(PyObject::bool_val(false))
        }
        "__getitem__" => {
            if args.is_empty() {
                return Err(PyException::type_error("__getitem__() requires 1 argument"));
            }
            let idx = args[0].to_int()?;
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let v = list.read();
                let len = v.len() as i64;
                let actual_idx = if idx < 0 { len + idx } else { idx };
                if actual_idx < 0 || actual_idx >= len {
                    return Err(PyException::new(
                        ExceptionKind::IndexError,
                        "deque index out of range",
                    ));
                }
                return Ok(v[actual_idx as usize].clone());
            }
            Err(PyException::new(
                ExceptionKind::IndexError,
                "deque index out of range",
            ))
        }
        "__reversed__" => {
            let len = get_data()
                .to_list()
                .map(|items| items.len())
                .unwrap_or_default();
            Ok(PyObject::tracked(PyObjectPayload::DequeIter(Box::new(
                DequeIterData {
                    source: receiver.clone(),
                    index: SyncUsize::new(0),
                    expected_len: len,
                    reverse: true,
                },
            ))))
        }
        "__setitem__" => {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "__setitem__() takes exactly 2 arguments",
                ));
            }
            let idx = args[0].to_int()?;
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                let actual_idx = normalize_index(idx, v.len())?;
                v[actual_idx] = args[1].clone();
                return Ok(PyObject::none());
            }
            Err(PyException::new(
                ExceptionKind::IndexError,
                "deque index out of range",
            ))
        }
        "__delitem__" => {
            if args.len() != 1 {
                return Err(PyException::type_error(
                    "__delitem__() takes exactly one argument",
                ));
            }
            let idx = args[0].to_int()?;
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                let actual_idx = normalize_index(idx, v.len())?;
                v.remove(actual_idx);
                return Ok(PyObject::none());
            }
            Err(PyException::new(
                ExceptionKind::IndexError,
                "deque index out of range",
            ))
        }
        "__iter__" => {
            let len = get_data()
                .to_list()
                .map(|items| items.len())
                .unwrap_or_default();
            Ok(PyObject::tracked(PyObjectPayload::DequeIter(Box::new(
                DequeIterData {
                    source: receiver.clone(),
                    index: SyncUsize::new(0),
                    expected_len: len,
                    reverse: false,
                },
            ))))
        }
        _ => Err(PyException::attribute_error(format!(
            "deque has no attribute '{}'",
            method
        ))),
    }
}
