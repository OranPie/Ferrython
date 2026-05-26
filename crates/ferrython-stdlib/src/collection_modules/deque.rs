//! `collections.deque` implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    CompareOp, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::collections::VecDeque;
use std::rc::Rc;

fn element_matches(item: &PyObjectRef, target: &PyObjectRef) -> bool {
    PyObjectRef::ptr_eq(item, target)
        || item
            .compare(target, CompareOp::Eq)
            .map_or(false, |v| v.is_truthy())
}

pub(super) fn collections_deque(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Extract maxlen from last arg if it's a kwargs dict
    let has_trailing_kwargs =
        !args.is_empty() && matches!(&args[args.len() - 1].payload, PyObjectPayload::Dict(_));
    let positional_count = if has_trailing_kwargs {
        args.len().saturating_sub(1)
    } else {
        args.len()
    };
    if positional_count > 2 {
        return Err(PyException::type_error(
            "deque() takes at most 2 positional arguments",
        ));
    }
    let kwargs_idx = if has_trailing_kwargs {
        args.len() - 1
    } else {
        args.len()
    };

    let items = if kwargs_idx == 0 || args.is_empty() {
        vec![]
    } else {
        args[0].to_list()?
    };

    // Extract maxlen from positional arg or trailing kwargs dict
    let maxlen = if has_trailing_kwargs {
        if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
            let map = map.read();
            if let Some(v) = map.get(&HashableKey::str_key(CompactString::from("maxlen"))) {
                if matches!(&v.payload, PyObjectPayload::None) {
                    None
                } else {
                    let n = v.to_int()?;
                    if n < 0 {
                        return Err(PyException::value_error("maxlen must be non-negative"));
                    }
                    Some(n as usize)
                }
            } else {
                None
            }
        } else {
            None
        }
    } else if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        let n = args[1].to_int()?;
        if n < 0 {
            return Err(PyException::value_error("maxlen must be non-negative"));
        }
        Some(n as usize)
    } else {
        None
    };
    // Enforce maxlen on initial items
    let items = if let Some(ml) = maxlen {
        if items.len() > ml {
            items[items.len() - ml..].to_vec()
        } else {
            items
        }
    } else {
        items
    };

    let initial_items = items.clone();
    let data = Rc::new(PyCell::new(items.into_iter().collect::<VecDeque<_>>()));

    // Build instance methods that share the data list
    let mut cls_ns = IndexMap::new();

    // append(x)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(
        CompactString::from("append"),
        PyObject::native_closure("deque.append", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("append requires argument"));
            }
            let mut w = d.write();
            w.push_back(args[0].clone());
            if let Some(m) = ml {
                while w.len() > m {
                    w.pop_front();
                }
            }
            Ok(PyObject::none())
        }),
    );

    // appendleft(x)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(
        CompactString::from("appendleft"),
        PyObject::native_closure("deque.appendleft", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("appendleft requires argument"));
            }
            let mut w = d.write();
            w.push_front(args[0].clone());
            if let Some(m) = ml {
                while w.len() > m {
                    w.pop_back();
                }
            }
            Ok(PyObject::none())
        }),
    );

    // pop()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("pop"),
        PyObject::native_closure("deque.pop", move |_: &[PyObjectRef]| {
            let mut w = d.write();
            w.pop_back()
                .ok_or_else(|| PyException::index_error("pop from an empty deque"))
        }),
    );

    // popleft()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("popleft"),
        PyObject::native_closure("deque.popleft", move |_: &[PyObjectRef]| {
            let mut w = d.write();
            if w.is_empty() {
                return Err(PyException::index_error("pop from an empty deque"));
            }
            Ok(w.pop_front().unwrap())
        }),
    );

    // extend(iterable)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(
        CompactString::from("extend"),
        PyObject::native_closure("deque.extend", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("extend requires argument"));
            }
            let items = args[0].to_list()?;
            let mut w = d.write();
            w.extend(items);
            if let Some(m) = ml {
                while w.len() > m {
                    w.pop_front();
                }
            }
            Ok(PyObject::none())
        }),
    );

    // extendleft(iterable)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(
        CompactString::from("extendleft"),
        PyObject::native_closure("deque.extendleft", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("extendleft requires argument"));
            }
            let items = args[0].to_list()?;
            let mut w = d.write();
            // CPython: appendleft each item in order — insert(0) naturally reverses
            for item in items.into_iter() {
                w.push_front(item);
            }
            if let Some(m) = ml {
                while w.len() > m {
                    w.pop_back();
                }
            }
            Ok(PyObject::none())
        }),
    );

    // rotate(n=1)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("rotate"),
        PyObject::native_closure("deque.rotate", move |args: &[PyObjectRef]| {
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
            let mut w = d.write();
            let len = w.len();
            if len == 0 {
                return Ok(PyObject::none());
            }
            let n = ((n % len as i64) + len as i64) as usize % len;
            if n > 0 {
                w.rotate_right(n);
            }
            Ok(PyObject::none())
        }),
    );

    // clear()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("clear"),
        PyObject::native_closure("deque.clear", move |_: &[PyObjectRef]| {
            d.write().clear();
            Ok(PyObject::none())
        }),
    );

    // count(x)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("count"),
        PyObject::native_closure("deque.count", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("count requires argument"));
            }
            let target = &args[0];
            let r = d.read();
            let c = r
                .iter()
                .filter(|item| element_matches(item, target))
                .count();
            Ok(PyObject::int(c as i64))
        }),
    );

    // index(x)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("index"),
        PyObject::native_closure("deque.index", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("index requires argument"));
            }
            let target = &args[0];
            let r = d.read();
            for (i, item) in r.iter().enumerate() {
                if element_matches(item, target) {
                    return Ok(PyObject::int(i as i64));
                }
            }
            Err(PyException::value_error("value not in deque"))
        }),
    );

    // remove(x)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("remove"),
        PyObject::native_closure("deque.remove", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("remove requires argument"));
            }
            let target = &args[0];
            let expected_len = d.read().len();
            for i in 0..expected_len {
                let item = {
                    let r = d.read();
                    if r.len() != expected_len {
                        return Err(PyException::index_error("deque mutated during iteration"));
                    }
                    r[i].clone()
                };
                if PyObjectRef::ptr_eq(&item, target)
                    || item.compare(target, CompareOp::Eq)?.is_truthy()
                {
                    if d.read().len() != expected_len {
                        return Err(PyException::index_error("deque mutated during iteration"));
                    }
                    d.write().remove(i);
                    return Ok(PyObject::none());
                }
                if d.read().len() != expected_len {
                    return Err(PyException::index_error("deque mutated during iteration"));
                }
            }
            Err(PyException::value_error("deque.remove(x): x not in deque"))
        }),
    );

    // reverse()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("reverse"),
        PyObject::native_closure("deque.reverse", move |_: &[PyObjectRef]| {
            d.write().make_contiguous().reverse();
            Ok(PyObject::none())
        }),
    );

    // copy()
    let d = data.clone();
    let ml2 = maxlen;
    cls_ns.insert(
        CompactString::from("copy"),
        PyObject::native_closure("deque.copy", move |_: &[PyObjectRef]| {
            let items: Vec<_> = d.read().iter().cloned().collect();
            let mut new_args = vec![PyObject::list(items)];
            if let Some(m) = ml2 {
                new_args.push(PyObject::int(m as i64));
            }
            collections_deque(&new_args)
        }),
    );

    // __len__()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("deque.__len__", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(d.read().len() as i64))
        }),
    );

    // __bool__()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__bool__"),
        PyObject::native_closure("deque.__bool__", move |_: &[PyObjectRef]| {
            Ok(PyObject::bool_val(!d.read().is_empty()))
        }),
    );

    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_closure("deque.__eq__", move |args: &[PyObjectRef]| {
            let other = if args.len() >= 2 {
                args[1].clone()
            } else {
                args.first().cloned().unwrap_or_else(PyObject::none)
            };
            if other.get_attr("__deque__").is_none() {
                return Ok(PyObject::not_implemented());
            }
            let other_items = other.to_list()?;
            let items = d.read();
            if items.len() != other_items.len() {
                return Ok(PyObject::bool_val(false));
            }
            Ok(PyObject::bool_val(
                items
                    .iter()
                    .zip(other_items.iter())
                    .all(|(left, right)| element_matches(left, right)),
            ))
        }),
    );

    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__ne__"),
        PyObject::native_closure("deque.__ne__", move |args: &[PyObjectRef]| {
            let other = if args.len() >= 2 {
                args[1].clone()
            } else {
                args.first().cloned().unwrap_or_else(PyObject::none)
            };
            if other.get_attr("__deque__").is_none() {
                return Ok(PyObject::not_implemented());
            }
            let other_items = other.to_list()?;
            let items = d.read();
            if items.len() != other_items.len() {
                return Ok(PyObject::bool_val(true));
            }
            Ok(PyObject::bool_val(
                !items
                    .iter()
                    .zip(other_items.iter())
                    .all(|(left, right)| element_matches(left, right)),
            ))
        }),
    );

    // __repr__()
    let d = data.clone();
    let ml3 = maxlen;
    cls_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("deque.__repr__", move |_: &[PyObjectRef]| {
            let r = d.read();
            let items_str: Vec<String> = r.iter().map(|i| i.py_to_string()).collect();
            let base = format!("deque([{}])", items_str.join(", "));
            if let Some(m) = ml3 {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "deque([{}], maxlen={})",
                    items_str.join(", "),
                    m
                ))))
            } else {
                Ok(PyObject::str_val(CompactString::from(base)))
            }
        }),
    );

    // __iter__()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("deque.__iter__", move |_: &[PyObjectRef]| {
            let snapshot: Vec<_> = d.read().iter().cloned().collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(ferrython_core::object::IteratorData::List {
                    items: snapshot,
                    index: 0,
                }),
            ))))
        }),
    );

    // __contains__(x) - needed for 'in' operator
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_closure("deque.__contains__", move |args: &[PyObjectRef]| {
            // Called as unbound method: args = [self, value] or directly: args = [value]
            let target = if args.len() >= 2 {
                &args[1]
            } else if !args.is_empty() {
                &args[0]
            } else {
                return Ok(PyObject::bool_val(false));
            };
            let r = d.read();
            for item in r.iter() {
                if element_matches(item, target) {
                    return Ok(PyObject::bool_val(true));
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );

    // __getitem__(index)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("deque.__getitem__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("__getitem__ requires index"));
            }
            // Called as unbound method: args = [self, index] or directly: args = [index]
            let idx_arg = if args.len() >= 2 { &args[1] } else { &args[0] };
            let idx = idx_arg.to_int()?;
            let r = d.read();
            let len = r.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("deque index out of range"));
            }
            Ok(r.get(actual as usize).unwrap().clone())
        }),
    );

    let deque_cls = PyObject::class(CompactString::from("deque"), vec![], cls_ns);
    let inst = PyObject::instance(deque_cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
        // Store a reference list for _data (closures share the backing Rc<PyCell> directly)
        attrs.insert(CompactString::from("_data"), PyObject::list(initial_items));
        attrs.insert(
            CompactString::from("__maxlen__"),
            match maxlen {
                Some(n) => PyObject::int(n as i64),
                None => PyObject::none(),
            },
        );
    }
    Ok(inst)
}
