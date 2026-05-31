use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    call_callable, check_args, make_module, CompareOp, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

// ── heapq module ──

pub fn create_heapq_module() -> PyObjectRef {
    create_heapq_module_named("heapq")
}

pub fn create_heapq_accel_module() -> PyObjectRef {
    create_heapq_module_named("_heapq")
}

fn heapq_function(
    module: &str,
    name: &str,
    func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>,
) -> PyObjectRef {
    PyObject::native_function(&format!("{module}.{name}"), func)
}

fn create_heapq_module_named(module: &str) -> PyObjectRef {
    make_module(
        module,
        vec![
            ("heappush", heapq_function(module, "heappush", heapq_push)),
            ("heappop", heapq_function(module, "heappop", heapq_pop)),
            ("heapify", heapq_function(module, "heapify", heapq_heapify)),
            (
                "heappushpop",
                heapq_function(module, "heappushpop", heapq_pushpop),
            ),
            (
                "heapreplace",
                heapq_function(module, "heapreplace", heapq_replace),
            ),
            (
                "_heappop_max",
                heapq_function(module, "_heappop_max", heapq_pop_max),
            ),
            (
                "_heapreplace_max",
                heapq_function(module, "_heapreplace_max", heapq_replace_max),
            ),
            (
                "_heapify_max",
                heapq_function(module, "_heapify_max", heapq_heapify_max),
            ),
            (
                "nlargest",
                heapq_function(module, "nlargest", heapq_nlargest),
            ),
            (
                "nsmallest",
                heapq_function(module, "nsmallest", heapq_nsmallest),
            ),
            ("merge", heapq_function(module, "merge", heapq_merge)),
        ],
    )
}

fn heap_cmp_lt(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
    if let PyObjectPayload::Instance(_) = &a.payload {
        if let Some(method) = a.get_attr("__lt__") {
            if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                let result = call_callable(&method, std::slice::from_ref(b))?;
                if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                    return Ok(result.is_truthy());
                }
            }
        }
    }
    Ok(a.compare(b, CompareOp::Lt)?.is_truthy())
}

fn heap_cmp_lt_checked(
    heap: &PyCell<Vec<PyObjectRef>>,
    a: &PyObjectRef,
    b: &PyObjectRef,
    expected_len: usize,
) -> PyResult<bool> {
    let result = heap_cmp_lt(a, b);
    if heap.read().len() != expected_len {
        return Err(PyException::index_error(
            "list changed size during iteration",
        ));
    }
    result
}

fn heap_pair(
    heap: &PyCell<Vec<PyObjectRef>>,
    left: usize,
    right: usize,
) -> PyResult<(PyObjectRef, PyObjectRef, usize)> {
    let items = heap.read();
    if left >= items.len() || right >= items.len() {
        return Err(PyException::index_error("index out of range"));
    }
    Ok((items[left].clone(), items[right].clone(), items.len()))
}

fn heap_swap(
    heap: &PyCell<Vec<PyObjectRef>>,
    left: usize,
    right: usize,
    expected_len: usize,
) -> PyResult<()> {
    let mut items = heap.write();
    if items.len() != expected_len || left >= items.len() || right >= items.len() {
        return Err(PyException::index_error(
            "list changed size during iteration",
        ));
    }
    items.swap(left, right);
    Ok(())
}

fn heap_sift_up(heap: &PyCell<Vec<PyObjectRef>>, mut pos: usize) -> PyResult<()> {
    while pos > 0 {
        let parent = (pos - 1) / 2;
        let (item, parent_item, expected_len) = heap_pair(heap, pos, parent)?;
        if heap_cmp_lt_checked(heap, &item, &parent_item, expected_len)? {
            heap_swap(heap, pos, parent, expected_len)?;
            pos = parent;
        } else {
            break;
        }
    }
    Ok(())
}

fn heap_sift_down(heap: &PyCell<Vec<PyObjectRef>>, mut pos: usize, end: usize) -> PyResult<()> {
    loop {
        let mut child = 2 * pos + 1;
        if child >= end {
            break;
        }
        let right = child + 1;
        if right < end {
            let (right_item, child_item, expected_len) = heap_pair(heap, right, child)?;
            if expected_len < end {
                return Err(PyException::index_error(
                    "list changed size during iteration",
                ));
            }
            if heap_cmp_lt_checked(heap, &right_item, &child_item, expected_len)? {
                child = right;
            }
        }
        let (child_item, item, expected_len) = heap_pair(heap, child, pos)?;
        if expected_len < end {
            return Err(PyException::index_error(
                "list changed size during iteration",
            ));
        }
        if heap_cmp_lt_checked(heap, &child_item, &item, expected_len)? {
            heap_swap(heap, pos, child, expected_len)?;
            pos = child;
        } else {
            break;
        }
    }
    Ok(())
}

fn heap_sift_down_max(heap: &PyCell<Vec<PyObjectRef>>, mut pos: usize, end: usize) -> PyResult<()> {
    loop {
        let mut child = 2 * pos + 1;
        if child >= end {
            break;
        }
        let right = child + 1;
        if right < end {
            let (child_item, right_item, expected_len) = heap_pair(heap, child, right)?;
            if expected_len < end {
                return Err(PyException::index_error(
                    "list changed size during iteration",
                ));
            }
            if heap_cmp_lt_checked(heap, &child_item, &right_item, expected_len)? {
                child = right;
            }
        }
        let (item, child_item, expected_len) = heap_pair(heap, pos, child)?;
        if expected_len < end {
            return Err(PyException::index_error(
                "list changed size during iteration",
            ));
        }
        if heap_cmp_lt_checked(heap, &item, &child_item, expected_len)? {
            heap_swap(heap, pos, child, expected_len)?;
            pos = child;
        } else {
            break;
        }
    }
    Ok(())
}

fn heap_item_precedes(a: &PyObjectRef, b: &PyObjectRef, reverse: bool) -> PyResult<bool> {
    if reverse {
        if heap_cmp_lt(b, a)? {
            return Ok(true);
        }
        if heap_cmp_lt(a, b)? {
            return Ok(false);
        }
    } else {
        if heap_cmp_lt(a, b)? {
            return Ok(true);
        }
        if heap_cmp_lt(b, a)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn heap_sort_items(items: &[PyObjectRef], reverse: bool) -> PyResult<Vec<PyObjectRef>> {
    if items.len() <= 1 {
        return Ok(items.to_vec());
    }
    let mid = items.len() / 2;
    let left = heap_sort_items(&items[..mid], reverse)?;
    let right = heap_sort_items(&items[mid..], reverse)?;
    let mut merged = Vec::with_capacity(items.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if heap_item_precedes(&left[i], &right[j], reverse)? {
            merged.push(left[i].clone());
            i += 1;
        } else {
            merged.push(right[j].clone());
            j += 1;
        }
    }
    merged.extend(left[i..].iter().cloned());
    merged.extend(right[j..].iter().cloned());
    Ok(merged)
}

fn heap_key_pair_precedes(
    a: &(PyObjectRef, PyObjectRef),
    b: &(PyObjectRef, PyObjectRef),
    reverse: bool,
) -> PyResult<bool> {
    heap_item_precedes(&a.0, &b.0, reverse)
}

fn heap_sort_key_pairs(
    pairs: &[(PyObjectRef, PyObjectRef)],
    reverse: bool,
) -> PyResult<Vec<(PyObjectRef, PyObjectRef)>> {
    if pairs.len() <= 1 {
        return Ok(pairs.to_vec());
    }
    let mid = pairs.len() / 2;
    let left = heap_sort_key_pairs(&pairs[..mid], reverse)?;
    let right = heap_sort_key_pairs(&pairs[mid..], reverse)?;
    let mut merged = Vec::with_capacity(pairs.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        if heap_key_pair_precedes(&left[i], &right[j], reverse)? {
            merged.push(left[i].clone());
            i += 1;
        } else {
            merged.push(right[j].clone());
            j += 1;
        }
    }
    merged.extend(left[i..].iter().cloned());
    merged.extend(right[j..].iter().cloned());
    Ok(merged)
}

fn heap_sort_with_key(
    items: Vec<PyObjectRef>,
    key: Option<PyObjectRef>,
    reverse: bool,
) -> PyResult<Vec<PyObjectRef>> {
    let Some(key_fn) = key else {
        return heap_sort_items(&items, reverse);
    };
    if matches!(&key_fn.payload, PyObjectPayload::None) {
        return heap_sort_items(&items, reverse);
    }
    let mut pairs = Vec::with_capacity(items.len());
    for item in items {
        let key_obj = call_callable(&key_fn, std::slice::from_ref(&item))?;
        pairs.push((key_obj, item));
    }
    Ok(heap_sort_key_pairs(&pairs, reverse)?
        .into_iter()
        .map(|(_, item)| item)
        .collect())
}

fn heap_kwarg(kwargs: Option<&PyObjectRef>, name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &kwargs?.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(name)))
        .cloned()
}

fn heap_split_kwargs(args: &[PyObjectRef]) -> (&[PyObjectRef], Option<PyObjectRef>) {
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let read = map.read();
            if read.contains_key(&HashableKey::str_key(CompactString::from("key")))
                || read.contains_key(&HashableKey::str_key(CompactString::from("reverse")))
            {
                return (&args[..args.len() - 1], Some(last.clone()));
            }
        }
    }
    (args, None)
}

fn heap_collect_via_list(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    let list_type = PyObject::builtin_type(CompactString::from("list"));
    call_callable(&list_type, std::slice::from_ref(obj))?.to_list()
}

fn heap_collect_iterable(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    if let PyObjectPayload::Instance(_) = &obj.payload {
        if let Some(iter_method) = obj.get_attr("__iter__") {
            let iter = call_callable(&iter_method, &[])?;
            if iter.get_attr("__next__").is_none() {
                return Err(PyException::type_error(format!(
                    "iter() returned non-iterator of type '{}'",
                    iter.type_name()
                )));
            }
            return heap_collect_via_list(&iter);
        }
        if obj.get_attr("__next__").is_some() {
            return Err(PyException::type_error(format!(
                "'{}' object is not iterable",
                obj.type_name()
            )));
        }
    }
    if matches!(
        &obj.payload,
        PyObjectPayload::Instance(_)
            | PyObjectPayload::Generator(_)
            | PyObjectPayload::Iterator(_)
            | PyObjectPayload::RangeIter(..)
            | PyObjectPayload::VecIter(_)
            | PyObjectPayload::DictValueIter(_)
            | PyObjectPayload::RefIter { .. }
    ) {
        return heap_collect_via_list(obj);
    }
    match obj.to_list() {
        Ok(items) => Ok(items),
        Err(_) => heap_collect_via_list(obj),
    }
}

fn heapq_push(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heappush", args, 2)?;
    let list_obj = &args[0];
    if let PyObjectPayload::List(lock) = &list_obj.payload {
        let pos = {
            let mut items = lock.write();
            items.push(args[1].clone());
            items.len() - 1
        };
        heap_sift_up(lock, pos)?;
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error(
            "heappush: first arg must be a list",
        ))
    }
}

fn heapq_pop(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heappop", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let (result, n) = {
            let mut items = lock.write();
            if items.is_empty() {
                return Err(PyException::index_error("index out of range"));
            }
            let len = items.len();
            if len == 1 {
                return Ok(items.pop().unwrap());
            }
            let result = items[0].clone();
            let last = items.pop().unwrap();
            items[0] = last;
            (result, items.len())
        };
        heap_sift_down(lock, 0, n)?;
        Ok(result)
    } else {
        Err(PyException::type_error("heappop: arg must be a list"))
    }
}

fn heapq_heapify(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heapify", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let n = lock.read().len();
        for i in (0..n / 2).rev() {
            heap_sift_down(lock, i, n)?;
        }
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error("heapify: arg must be a list"))
    }
}

fn heapq_pushpop(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heappushpop", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let root_and_len = {
            let items = lock.read();
            if items.is_empty() {
                return Ok(args[1].clone());
            }
            (items[0].clone(), items.len())
        };
        let (root, expected_len) = root_and_len;
        if !heap_cmp_lt_checked(lock, &root, &args[1], expected_len)? {
            return Ok(args[1].clone());
        }
        {
            let mut items = lock.write();
            if items.len() != expected_len || items.is_empty() {
                return Err(PyException::index_error(
                    "list changed size during iteration",
                ));
            }
            items[0] = args[1].clone();
        }
        heap_sift_down(lock, 0, expected_len)?;
        Ok(root)
    } else {
        Err(PyException::type_error(
            "heappushpop: first arg must be a list",
        ))
    }
}

fn heapq_replace(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("heapreplace", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let (result, n) = {
            let mut items = lock.write();
            if items.is_empty() {
                return Err(PyException::index_error("index out of range"));
            }
            let result = std::mem::replace(&mut items[0], args[1].clone());
            (result, items.len())
        };
        heap_sift_down(lock, 0, n)?;
        Ok(result)
    } else {
        Err(PyException::type_error(
            "heapreplace: first arg must be a list",
        ))
    }
}

fn heapq_pop_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("_heappop_max", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let (result, n) = {
            let mut items = lock.write();
            if items.is_empty() {
                return Err(PyException::index_error("index out of range"));
            }
            let len = items.len();
            if len == 1 {
                return Ok(items.pop().unwrap());
            }
            let result = items[0].clone();
            let last = items.pop().unwrap();
            items[0] = last;
            (result, items.len())
        };
        heap_sift_down_max(lock, 0, n)?;
        Ok(result)
    } else {
        Err(PyException::type_error("_heappop_max: arg must be a list"))
    }
}

fn heapq_replace_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("_heapreplace_max", args, 2)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let (result, n) = {
            let mut items = lock.write();
            if items.is_empty() {
                return Err(PyException::index_error("index out of range"));
            }
            let result = std::mem::replace(&mut items[0], args[1].clone());
            (result, items.len())
        };
        heap_sift_down_max(lock, 0, n)?;
        Ok(result)
    } else {
        Err(PyException::type_error(
            "_heapreplace_max: first arg must be a list",
        ))
    }
}

fn heapq_heapify_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("_heapify_max", args, 1)?;
    if let PyObjectPayload::List(lock) = &args[0].payload {
        let n = lock.read().len();
        for i in (0..n / 2).rev() {
            heap_sift_down_max(lock, i, n)?;
        }
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error("_heapify_max: arg must be a list"))
    }
}

fn heapq_nlargest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = heap_split_kwargs(args);
    check_args("nlargest", pos, 2)?;
    let n = pos[0].to_int()? as usize;
    let items = heap_collect_iterable(&pos[1])?;
    let key = heap_kwarg(kwargs.as_ref(), "key");
    let mut sorted = heap_sort_with_key(items, key, true)?;
    sorted.truncate(n);
    Ok(PyObject::list(sorted))
}

fn heapq_nsmallest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = heap_split_kwargs(args);
    check_args("nsmallest", pos, 2)?;
    let n = pos[0].to_int()? as usize;
    let items = heap_collect_iterable(&pos[1])?;
    let key = heap_kwarg(kwargs.as_ref(), "key");
    let mut sorted = heap_sort_with_key(items, key, false)?;
    sorted.truncate(n);
    Ok(PyObject::list(sorted))
}

fn heapq_merge(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (pos, kwargs) = heap_split_kwargs(args);
    let key = heap_kwarg(kwargs.as_ref(), "key");
    let reverse = heap_kwarg(kwargs.as_ref(), "reverse")
        .map(|v| v.is_truthy())
        .unwrap_or(false);
    let mut all = Vec::new();
    for arg in pos {
        all.extend(heap_collect_iterable(arg)?);
    }
    all = heap_sort_with_key(all, key, reverse)?;
    Ok(PyObject::list(all))
}
