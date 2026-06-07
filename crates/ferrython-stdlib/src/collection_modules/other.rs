use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    call_callable, check_args_min, make_builtin, make_module, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

// ── queue module ──

pub fn create_queue_module() -> PyObjectRef {
    let empty_cls = make_queue_exception_class("Empty");
    let full_cls = make_queue_exception_class("Full");
    let queue_cls = make_queue_class(
        "Queue",
        false,
        false,
        false,
        empty_cls.clone(),
        full_cls.clone(),
    );
    let lifo_cls = make_queue_class(
        "LifoQueue",
        true,
        false,
        false,
        empty_cls.clone(),
        full_cls.clone(),
    );
    let priority_cls = make_queue_class(
        "PriorityQueue",
        false,
        true,
        false,
        empty_cls.clone(),
        full_cls.clone(),
    );
    let simple_cls = make_queue_class(
        "SimpleQueue",
        false,
        false,
        true,
        empty_cls.clone(),
        full_cls.clone(),
    );

    make_module(
        "queue",
        vec![
            ("Queue", queue_cls),
            ("LifoQueue", lifo_cls),
            ("PriorityQueue", priority_cls),
            ("SimpleQueue", simple_cls.clone()),
            ("_PySimpleQueue", simple_cls),
            ("Empty", empty_cls),
            ("Full", full_cls),
        ],
    )
}

fn make_queue_exception_class(name: &str) -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__module__"),
        PyObject::str_val(CompactString::from("queue")),
    );
    ns.insert(
        CompactString::from("__qualname__"),
        PyObject::str_val(CompactString::from(name)),
    );
    PyObject::class(
        CompactString::from(name),
        vec![PyObject::exception_type(ExceptionKind::Exception)],
        ns,
    )
}

fn queue_exception(cls: &PyObjectRef, message: &str) -> PyException {
    let inst = PyObject::instance(cls.clone());
    if let PyObjectPayload::Instance(data) = &inst.payload {
        data.attrs.write().insert(
            CompactString::from("args"),
            PyObject::tuple(vec![PyObject::str_val(CompactString::from(message))]),
        );
    }
    let mut exc = PyException::runtime_error(message);
    exc.original = Some(inst);
    exc
}

fn make_queue_class(
    name: &str,
    is_lifo: bool,
    is_priority: bool,
    simple: bool,
    empty_cls: PyObjectRef,
    full_cls: PyObjectRef,
) -> PyObjectRef {
    let kind = name.to_string();
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__module__"),
        PyObject::str_val(CompactString::from("queue")),
    );
    ns.insert(
        CompactString::from("__qualname__"),
        PyObject::str_val(CompactString::from(name)),
    );
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("queue.Queue.__init__", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("__init__ requires self"));
            }
            init_queue_instance(
                &args[0],
                &kind,
                &args[1..],
                simple,
                empty_cls.clone(),
                full_cls.clone(),
            )
        }),
    );
    ns.insert(
        CompactString::from("_put"),
        PyObject::native_closure("queue.Queue._put", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("_put() requires self and item"));
            }
            queue_storage(&args[0], |items| {
                let item = args[1].clone();
                if is_priority {
                    let pos = items
                        .iter()
                        .position(|x| {
                            if let (Ok(a_val), Ok(b_val)) = (item.to_float(), x.to_float()) {
                                a_val < b_val
                            } else {
                                item.py_to_string() < x.py_to_string()
                            }
                        })
                        .unwrap_or(items.len());
                    items.insert(pos, item);
                } else {
                    items.push_back(item);
                }
                Ok(PyObject::none())
            })
        }),
    );
    ns.insert(
        CompactString::from("_get"),
        PyObject::native_closure("queue.Queue._get", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("_get() requires self"));
            }
            queue_storage(&args[0], |items| {
                if is_lifo {
                    items.pop_back()
                } else {
                    items.pop_front()
                }
                .ok_or_else(|| PyException::runtime_error("queue is empty"))
            })
        }),
    );
    PyObject::class(CompactString::from(name), vec![], ns)
}

fn queue_block_timeout(args: &[PyObjectRef], default_block: bool) -> PyResult<(bool, Option<u64>)> {
    let mut block = default_block;
    let mut timeout_ms = None;
    let mut positional_end = args.len();
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            positional_end -= 1;
            let read = map.read();
            let block_key =
                ferrython_core::types::HashableKey::str_key(CompactString::from("block"));
            if let Some(value) = read.get(&block_key) {
                block = value.is_truthy();
            }
            let timeout_key =
                ferrython_core::types::HashableKey::str_key(CompactString::from("timeout"));
            if let Some(value) = read.get(&timeout_key) {
                if !matches!(&value.payload, PyObjectPayload::None) {
                    let t = value.to_float().unwrap_or(0.0);
                    if t < 0.0 {
                        return Err(PyException::value_error(
                            "'timeout' must be a non-negative number",
                        ));
                    }
                    timeout_ms = Some((t * 1000.0) as u64);
                }
            }
        }
    }
    if positional_end > 0 {
        block = args[0].is_truthy();
    }
    if positional_end > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
        let t = args[1].to_float().unwrap_or(0.0);
        if t < 0.0 {
            return Err(PyException::value_error(
                "'timeout' must be a non-negative number",
            ));
        }
        timeout_ms = Some((t * 1000.0) as u64);
    }
    Ok((block, timeout_ms))
}

fn queue_storage<F>(inst: &PyObjectRef, f: F) -> PyResult<PyObjectRef>
where
    F: FnOnce(&mut VecDeque<PyObjectRef>) -> PyResult<PyObjectRef>,
{
    let PyObjectPayload::Instance(data) = &inst.payload else {
        return Err(PyException::type_error(
            "queue method requires a Queue instance",
        ));
    };
    let storage = data
        .attrs
        .read()
        .get("__queue_items__")
        .cloned()
        .ok_or_else(|| PyException::type_error("uninitialized Queue instance"))?;
    let PyObjectPayload::Deque(items) = &storage.payload else {
        return Err(PyException::type_error("invalid Queue storage"));
    };
    f(&mut items.write())
}

fn init_queue_instance(
    inst: &PyObjectRef,
    kind: &str,
    args: &[PyObjectRef],
    simple: bool,
    empty_cls: PyObjectRef,
    full_cls: PyObjectRef,
) -> PyResult<PyObjectRef> {
    // Extract maxsize from positional args or kwargs dict
    let maxsize = if simple {
        0
    } else if !args.is_empty() {
        if let Some(n) = args[0].as_int() {
            n
        } else if let PyObjectPayload::Dict(d) = &args[0].payload {
            let d = d.read();
            d.get(&ferrython_core::types::HashableKey::str_key(
                CompactString::from("maxsize"),
            ))
            .and_then(|v| v.as_int())
            .unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };
    let items: Rc<PyCell<VecDeque<PyObjectRef>>> = Rc::new(PyCell::new(VecDeque::new()));
    let unfinished = Arc::new(Mutex::new(0i64));

    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__queue__"),
            PyObject::str_val(CompactString::from(kind)),
        );
        w.insert(CompactString::from("maxsize"), PyObject::int(maxsize));
        w.insert(
            CompactString::from("__queue_items__"),
            PyObject::wrap(PyObjectPayload::Deque(items.clone())),
        );

        // put(item)
        let it1 = items.clone();
        let uf1 = unfinished.clone();
        let ms1 = maxsize;
        let full1 = full_cls.clone();
        let self_put = inst.clone();
        w.insert(
            CompactString::from("put"),
            PyObject::native_closure("put", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("put() requires 1 argument"));
                }
                let (block, timeout_ms) = queue_block_timeout(&a[1..], true)?;
                let v = it1.write();
                if ms1 > 0 && v.len() as i64 >= ms1 {
                    drop(v);
                    if !block || timeout_ms.is_some() {
                        return Err(queue_exception(&full1, "queue is full"));
                    }
                    return Err(queue_exception(&full1, "queue is full"));
                }
                drop(v);
                if let Some(putter) = self_put.get_attr("_put") {
                    call_callable(&putter, &[a[0].clone()])?;
                }
                *uf1.lock().unwrap() += 1;
                Ok(PyObject::none())
            }),
        );

        // put_nowait(item) — same as put for single-threaded
        let it1b = items.clone();
        let uf1b = unfinished.clone();
        let ms1b = maxsize;
        let full2 = full_cls.clone();
        let self_put_nowait = inst.clone();
        w.insert(
            CompactString::from("put_nowait"),
            PyObject::native_closure("put_nowait", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("put_nowait() requires 1 argument"));
                }
                let v = it1b.write();
                if ms1b > 0 && v.len() as i64 >= ms1b {
                    return Err(queue_exception(&full2, "queue is full"));
                }
                drop(v);
                if let Some(putter) = self_put_nowait.get_attr("_put") {
                    call_callable(&putter, &[a[0].clone()])?;
                }
                *uf1b.lock().unwrap() += 1;
                Ok(PyObject::none())
            }),
        );

        // get(block=True, timeout=None)
        let it2 = items.clone();
        let empty1 = empty_cls.clone();
        let self_get = inst.clone();
        w.insert(
            CompactString::from("get"),
            PyObject::native_closure("get", move |args: &[PyObjectRef]| {
                let (block, timeout_ms) = queue_block_timeout(args, true)?;

                if block {
                    let deadline = timeout_ms
                        .map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms));
                    loop {
                        {
                            let v = it2.write();
                            if !v.is_empty() {
                                drop(v);
                                return if let Some(getter) = self_get.get_attr("_get") {
                                    call_callable(&getter, &[])
                                } else {
                                    Err(queue_exception(&empty1, "queue is empty"))
                                };
                            }
                        }
                        if deadline
                            .map(|limit| std::time::Instant::now() >= limit)
                            .unwrap_or(false)
                        {
                            return Err(queue_exception(&empty1, "queue is empty"));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                } else {
                    let v = it2.write();
                    if v.is_empty() {
                        return Err(queue_exception(&empty1, "queue is empty"));
                    }
                    drop(v);
                    if let Some(getter) = self_get.get_attr("_get") {
                        call_callable(&getter, &[])
                    } else {
                        Err(queue_exception(&empty1, "queue is empty"))
                    }
                }
            }),
        );

        // get_nowait() — same as get for single-threaded
        let it2b = items.clone();
        let empty2 = empty_cls.clone();
        let self_get_nowait = inst.clone();
        w.insert(
            CompactString::from("get_nowait"),
            PyObject::native_closure("get_nowait", move |_: &[PyObjectRef]| {
                let v = it2b.write();
                if v.is_empty() {
                    return Err(queue_exception(&empty2, "queue is empty"));
                }
                drop(v);
                if let Some(getter) = self_get_nowait.get_attr("_get") {
                    call_callable(&getter, &[])
                } else {
                    Err(queue_exception(&empty2, "queue is empty"))
                }
            }),
        );

        // qsize()
        let it3 = items.clone();
        w.insert(
            CompactString::from("qsize"),
            PyObject::native_closure("qsize", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(it3.read().len() as i64))
            }),
        );

        // empty()
        let it4 = items.clone();
        w.insert(
            CompactString::from("empty"),
            PyObject::native_closure("empty", move |_: &[PyObjectRef]| {
                Ok(PyObject::bool_val(it4.read().is_empty()))
            }),
        );

        // full()
        let it5 = items.clone();
        let ms2 = maxsize;
        w.insert(
            CompactString::from("full"),
            PyObject::native_closure("full", move |_: &[PyObjectRef]| {
                if ms2 <= 0 {
                    return Ok(PyObject::bool_val(false));
                }
                Ok(PyObject::bool_val(it5.read().len() as i64 >= ms2))
            }),
        );

        // task_done()
        let uf2 = unfinished.clone();
        w.insert(
            CompactString::from("task_done"),
            PyObject::native_closure("task_done", move |_: &[PyObjectRef]| {
                let mut u = uf2.lock().unwrap();
                if *u <= 0 {
                    return Err(PyException::value_error(
                        "task_done() called too many times",
                    ));
                }
                *u -= 1;
                Ok(PyObject::none())
            }),
        );

        // join() — blocks until all tasks done
        let uf3 = unfinished.clone();
        w.insert(
            CompactString::from("join"),
            PyObject::native_closure("join", move |_: &[PyObjectRef]| {
                // Spin-wait with backoff until unfinished tasks reach 0
                loop {
                    if *uf3.lock().unwrap() <= 0 {
                        return Ok(PyObject::none());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }),
        );

        // _items for backwards compat
        let it6 = items.clone();
        w.insert(
            CompactString::from("_items"),
            PyObject::native_closure("_items", move |_: &[PyObjectRef]| {
                Ok(PyObject::list(it6.read().iter().cloned().collect()))
            }),
        );
    }
    Ok(PyObject::none())
}

// ── array module ─────────────────────────────────────────────────────
pub fn create_array_module() -> PyObjectRef {
    make_module(
        "array",
        vec![
            ("array", make_builtin(array_array)),
            (
                "typecodes",
                PyObject::str_val(CompactString::from("bBuhHiIlLqQfd")),
            ),
        ],
    )
}

fn array_itemsize(typecode: &str) -> usize {
    match typecode {
        "b" | "B" => 1,
        "u" | "h" | "H" => 2,
        "i" | "I" | "l" | "L" | "f" => 4,
        "q" | "Q" | "d" => 8,
        _ => 1,
    }
}

fn array_value_from_ne_bytes(typecode: &str, chunk: &[u8]) -> PyObjectRef {
    match typecode {
        "b" => PyObject::int(i8::from_ne_bytes(chunk.try_into().unwrap()) as i64),
        "B" => PyObject::int(chunk[0] as i64),
        "h" => PyObject::int(i16::from_ne_bytes(chunk.try_into().unwrap()) as i64),
        "H" => PyObject::int(u16::from_ne_bytes(chunk.try_into().unwrap()) as i64),
        "i" | "l" => PyObject::int(i32::from_ne_bytes(chunk.try_into().unwrap()) as i64),
        "I" | "L" => PyObject::int(u32::from_ne_bytes(chunk.try_into().unwrap()) as i64),
        "q" => PyObject::int(i64::from_ne_bytes(chunk.try_into().unwrap())),
        "Q" => PyObject::int(u64::from_ne_bytes(chunk.try_into().unwrap()) as i64),
        "f" => PyObject::float(f32::from_ne_bytes(chunk.try_into().unwrap()) as f64),
        "d" => PyObject::float(f64::from_ne_bytes(chunk.try_into().unwrap())),
        _ => PyObject::int(chunk[0] as i64),
    }
}

fn array_value_to_ne_bytes(typecode: &str, value: &PyObjectRef) -> Vec<u8> {
    let v = value.to_int().unwrap_or(0);
    match typecode {
        "b" => vec![v as i8 as u8],
        "B" => vec![v as u8],
        "h" => (v as i16).to_ne_bytes().to_vec(),
        "H" => (v as u16).to_ne_bytes().to_vec(),
        "i" | "l" => (v as i32).to_ne_bytes().to_vec(),
        "I" | "L" => (v as u32).to_ne_bytes().to_vec(),
        "q" => v.to_ne_bytes().to_vec(),
        "Q" => (v as u64).to_ne_bytes().to_vec(),
        "f" => {
            let fv = value.to_float().unwrap_or(0.0) as f32;
            fv.to_ne_bytes().to_vec()
        }
        "d" => {
            let fv = value.to_float().unwrap_or(0.0);
            fv.to_ne_bytes().to_vec()
        }
        _ => vec![v as u8],
    }
}

fn array_extend_from_bytes(
    array: &PyObjectRef,
    typecode: &str,
    input_bytes: &[u8],
) -> PyResult<()> {
    let itemsize = array_itemsize(typecode);
    if input_bytes.len() % itemsize != 0 {
        return Err(PyException::value_error(
            "bytes length not a multiple of item size",
        ));
    }
    if let Some(data) = array.get_attr("_data") {
        if let PyObjectPayload::List(items) = &data.payload {
            let mut w = items.write();
            for chunk in input_bytes.chunks(itemsize) {
                w.push(array_value_from_ne_bytes(typecode, chunk));
            }
            return Ok(());
        }
    }
    Err(PyException::type_error("corrupted array"))
}

fn bytes_from_file_method_result(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok((**b).clone()),
        PyObjectPayload::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(PyException::type_error("read() did not return bytes")),
    }
}

fn call_array_file_read(
    file: &PyObjectRef,
    read: &PyObjectRef,
    byte_count: usize,
) -> PyResult<PyObjectRef> {
    let size_arg = PyObject::int(byte_count as i64);
    match &read.payload {
        PyObjectPayload::NativeFunction(_) => {
            ferrython_core::object::call_callable(read, &[file.clone(), size_arg])
        }
        _ => ferrython_core::object::call_callable(read, &[size_arg]),
    }
}

fn array_array(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "array() requires at least 1 argument",
        ));
    }
    let typecode = args[0].py_to_string();
    if typecode.len() != 1 || !"bBuhHiIlLqQfd".contains(&typecode) {
        return Err(PyException::value_error(format!(
            "bad typecode (must be b, B, u, h, H, i, I, l, L, q, Q, f, or d): '{}'",
            typecode
        )));
    }
    let items = if args.len() > 1 {
        args[1].to_list()?
    } else {
        vec![]
    };

    let cls = PyObject::class(CompactString::from("array"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("typecode"),
            PyObject::str_val(CompactString::from(&typecode)),
        );
        attrs.insert(CompactString::from("_data"), PyObject::list(items));
        attrs.insert(CompactString::from("__array__"), PyObject::bool_val(true));
        attrs.insert(
            CompactString::from("itemsize"),
            PyObject::int(match typecode.as_str() {
                tc => array_itemsize(tc) as i64,
            }),
        );

        let self_ref = inst.clone();

        attrs.insert(
            CompactString::from("append"),
            PyObject::native_closure("array.append", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.append", args, 1)?;
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            items.write().push(args[0].clone());
                        }
                    }
                    Ok(PyObject::none())
                }
            }),
        );

        attrs.insert(
            CompactString::from("extend"),
            PyObject::native_closure("array.extend", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.extend", args, 1)?;
                    let new_items = args[0].to_list()?;
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let mut w = items.write();
                            for item in new_items {
                                w.push(item);
                            }
                        }
                    }
                    Ok(PyObject::none())
                }
            }),
        );

        attrs.insert(
            CompactString::from("pop"),
            PyObject::native_closure("array.pop", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let mut w = items.write();
                            if w.is_empty() {
                                return Err(PyException::index_error("pop from empty array"));
                            }
                            let idx = if !args.is_empty() {
                                let i = args[0].to_int()? as isize;
                                if i < 0 {
                                    (w.len() as isize + i) as usize
                                } else {
                                    i as usize
                                }
                            } else {
                                w.len() - 1
                            };
                            if idx >= w.len() {
                                return Err(PyException::index_error("pop index out of range"));
                            }
                            return Ok(w.remove(idx));
                        }
                    }
                    Err(PyException::type_error("corrupted array"))
                }
            }),
        );

        attrs.insert(
            CompactString::from("insert"),
            PyObject::native_closure("array.insert", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.insert", args, 2)?;
                    let idx = args[0].to_int()? as usize;
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let mut w = items.write();
                            let pos = idx.min(w.len());
                            w.insert(pos, args[1].clone());
                        }
                    }
                    Ok(PyObject::none())
                }
            }),
        );

        attrs.insert(
            CompactString::from("remove"),
            PyObject::native_closure("array.remove", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.remove", args, 1)?;
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let mut w = items.write();
                            let target = &args[0];
                            if let Some(pos) = w
                                .iter()
                                .position(|x| x.py_to_string() == target.py_to_string())
                            {
                                w.remove(pos);
                                return Ok(PyObject::none());
                            }
                            return Err(PyException::value_error(
                                "array.remove(x): x not in array",
                            ));
                        }
                    }
                    Err(PyException::type_error("corrupted array"))
                }
            }),
        );

        attrs.insert(
            CompactString::from("index"),
            PyObject::native_closure("array.index", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.index", args, 1)?;
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let r = items.read();
                            let target = &args[0];
                            if let Some(pos) = r
                                .iter()
                                .position(|x| x.py_to_string() == target.py_to_string())
                            {
                                return Ok(PyObject::int(pos as i64));
                            }
                            return Err(PyException::value_error("array.index(x): x not in array"));
                        }
                    }
                    Err(PyException::type_error("corrupted array"))
                }
            }),
        );

        attrs.insert(
            CompactString::from("count"),
            PyObject::native_closure("array.count", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.count", args, 1)?;
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let r = items.read();
                            let target = &args[0];
                            let n = r
                                .iter()
                                .filter(|x| x.py_to_string() == target.py_to_string())
                                .count();
                            return Ok(PyObject::int(n as i64));
                        }
                    }
                    Ok(PyObject::int(0))
                }
            }),
        );

        attrs.insert(
            CompactString::from("reverse"),
            PyObject::native_closure("array.reverse", {
                let s = self_ref.clone();
                move |_args: &[PyObjectRef]| {
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            items.write().reverse();
                        }
                    }
                    Ok(PyObject::none())
                }
            }),
        );

        attrs.insert(
            CompactString::from("tolist"),
            PyObject::native_closure("array.tolist", {
                let s = self_ref.clone();
                move |_args: &[PyObjectRef]| {
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            return Ok(PyObject::list(items.read().clone()));
                        }
                    }
                    Ok(PyObject::list(vec![]))
                }
            }),
        );

        attrs.insert(
            CompactString::from("tobytes"),
            PyObject::native_closure("array.tobytes", {
                let s = self_ref.clone();
                move |_args: &[PyObjectRef]| {
                    let typecode = s
                        .get_attr("typecode")
                        .map(|v| v.py_to_string())
                        .unwrap_or_default();
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let r = items.read();
                            let mut bytes = Vec::new();
                            for x in r.iter() {
                                bytes.extend_from_slice(&array_value_to_ne_bytes(&typecode, x));
                            }
                            return Ok(PyObject::bytes(bytes));
                        }
                    }
                    Ok(PyObject::bytes(vec![]))
                }
            }),
        );

        attrs.insert(
            CompactString::from("frombytes"),
            PyObject::native_closure("array.frombytes", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.frombytes", args, 1)?;
                    let input_bytes = if let PyObjectPayload::Bytes(b) = &args[0].payload {
                        (**b).clone()
                    } else {
                        return Err(PyException::type_error(
                            "frombytes requires a bytes argument",
                        ));
                    };
                    let typecode = s
                        .get_attr("typecode")
                        .map(|v| v.py_to_string())
                        .unwrap_or_default();
                    array_extend_from_bytes(&s, &typecode, &input_bytes)?;
                    Ok(PyObject::none())
                }
            }),
        );

        attrs.insert(
            CompactString::from("fromfile"),
            PyObject::native_closure("array.fromfile", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.fromfile", args, 2)?;
                    let count = args[1].to_int()?;
                    if count < 0 {
                        return Err(PyException::value_error("negative count"));
                    }
                    let typecode = s
                        .get_attr("typecode")
                        .map(|v| v.py_to_string())
                        .unwrap_or_default();
                    let byte_count = count as usize * array_itemsize(&typecode);
                    let read = args[0]
                        .get_attr("read")
                        .ok_or_else(|| PyException::type_error("file object has no read()"))?;
                    let result = call_array_file_read(&args[0], &read, byte_count)?;
                    let input_bytes = bytes_from_file_method_result(&result)?;
                    if input_bytes.len() < byte_count {
                        return Err(PyException::new(
                            ExceptionKind::EOFError,
                            "not enough items in file",
                        ));
                    }
                    array_extend_from_bytes(&s, &typecode, &input_bytes[..byte_count])?;
                    Ok(PyObject::none())
                }
            }),
        );

        attrs.insert(
            CompactString::from("byteswap"),
            PyObject::native_closure("array.byteswap", {
                let s = self_ref.clone();
                move |_args: &[PyObjectRef]| {
                    let typecode = s
                        .get_attr("typecode")
                        .map(|v| v.py_to_string())
                        .unwrap_or_default();
                    let itemsize = array_itemsize(&typecode);
                    if itemsize == 1 {
                        return Ok(PyObject::none());
                    }
                    if itemsize != 2 && itemsize != 4 && itemsize != 8 {
                        return Err(PyException::runtime_error("byteswap not supported"));
                    }
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let mut w = items.write();
                            for item in w.iter_mut() {
                                let mut bytes = array_value_to_ne_bytes(&typecode, item);
                                bytes.reverse();
                                *item = array_value_from_ne_bytes(&typecode, &bytes);
                            }
                            return Ok(PyObject::none());
                        }
                    }
                    Err(PyException::type_error("corrupted array"))
                }
            }),
        );

        // __repr__: array('i', [1, 2, 3])
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("array.__repr__", {
                let s = self_ref.clone();
                move |_args: &[PyObjectRef]| {
                    let tc = s
                        .get_attr("typecode")
                        .map(|v| v.py_to_string())
                        .unwrap_or_default();
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let r = items.read();
                            let items_str: Vec<String> = r.iter().map(|x| x.repr()).collect();
                            return Ok(PyObject::str_val(CompactString::from(format!(
                                "array('{}', [{}])",
                                tc,
                                items_str.join(", ")
                            ))));
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(format!(
                        "array('{}')",
                        tc
                    ))))
                }
            }),
        );

        attrs.insert(
            CompactString::from("__len__"),
            PyObject::native_closure("array.__len__", {
                let s = self_ref.clone();
                move |_args: &[PyObjectRef]| {
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            return Ok(PyObject::int(items.read().len() as i64));
                        }
                    }
                    Ok(PyObject::int(0))
                }
            }),
        );

        attrs.insert(
            CompactString::from("__getitem__"),
            PyObject::native_closure("array.__getitem__", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.__getitem__", args, 1)?;
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let r = items.read();
                            let i = args[0].to_int()? as isize;
                            let idx = if i < 0 {
                                (r.len() as isize + i) as usize
                            } else {
                                i as usize
                            };
                            if idx >= r.len() {
                                return Err(PyException::index_error("array index out of range"));
                            }
                            return Ok(r[idx].clone());
                        }
                    }
                    Err(PyException::type_error("corrupted array"))
                }
            }),
        );

        attrs.insert(
            CompactString::from("__setitem__"),
            PyObject::native_closure("array.__setitem__", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.__setitem__", args, 2)?;
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let mut w = items.write();
                            let i = args[0].to_int()? as isize;
                            let idx = if i < 0 {
                                (w.len() as isize + i) as usize
                            } else {
                                i as usize
                            };
                            if idx >= w.len() {
                                return Err(PyException::index_error(
                                    "array assignment index out of range",
                                ));
                            }
                            w[idx] = args[1].clone();
                            return Ok(PyObject::none());
                        }
                    }
                    Err(PyException::type_error("corrupted array"))
                }
            }),
        );

        attrs.insert(
            CompactString::from("__contains__"),
            PyObject::native_closure("array.__contains__", {
                let s = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("array.__contains__", args, 1)?;
                    if let Some(data) = s.get_attr("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let r = items.read();
                            let target = &args[0];
                            return Ok(PyObject::bool_val(
                                r.iter().any(|x| x.py_to_string() == target.py_to_string()),
                            ));
                        }
                    }
                    Ok(PyObject::bool_val(false))
                }
            }),
        );

        attrs.insert(
            CompactString::from("__iter__"),
            PyObject::native_closure("array.__iter__", {
                let s = self_ref.clone();
                move |_args: &[PyObjectRef]| {
                    if let Some(data) = s.get_attr("_data") {
                        return data.get_iter();
                    }
                    Err(PyException::type_error("corrupted array"))
                }
            }),
        );
    }
    Ok(inst)
}
