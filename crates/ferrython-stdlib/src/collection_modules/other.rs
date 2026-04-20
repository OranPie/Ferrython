use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyCell, 
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args_min,
};
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

// ── queue module ──

pub fn create_queue_module() -> PyObjectRef {
    let exc_base = PyObject::exception_type(ExceptionKind::Exception);
    let empty_cls = PyObject::class(CompactString::from("Empty"), vec![exc_base.clone()], IndexMap::new());
    let full_cls = PyObject::class(CompactString::from("Full"), vec![exc_base], IndexMap::new());

    // Queue constructor
    let ec1 = empty_cls.clone();
    let queue_fn = PyObject::native_closure("Queue", move |args: &[PyObjectRef]| {
        create_queue_instance_full("Queue", args, &ec1)
    });
    // LifoQueue constructor
    let ec2 = empty_cls.clone();
    let lifo_fn = PyObject::native_closure("LifoQueue", move |args: &[PyObjectRef]| {
        create_queue_instance_full("LifoQueue", args, &ec2)
    });
    // PriorityQueue constructor
    let ec3 = empty_cls.clone();
    let prio_fn = PyObject::native_closure("PriorityQueue", move |args: &[PyObjectRef]| {
        create_queue_instance_full("PriorityQueue", args, &ec3)
    });
    // SimpleQueue constructor (unbounded FIFO, no maxsize)
    let ec4 = empty_cls.clone();
    let simple_queue_fn = PyObject::native_closure("SimpleQueue", move |_args: &[PyObjectRef]| {
        create_queue_instance_full("SimpleQueue", &[PyObject::int(0)], &ec4)
    });

    make_module("queue", vec![
        ("Queue", queue_fn),
        ("LifoQueue", lifo_fn),
        ("PriorityQueue", prio_fn),
        ("SimpleQueue", simple_queue_fn),
        ("Empty", empty_cls),
        ("Full", full_cls),
    ])
}

fn create_queue_instance_full(kind: &str, args: &[PyObjectRef], empty_cls: &PyObjectRef) -> PyResult<PyObjectRef> {
    // Extract maxsize from positional args or kwargs dict
    let maxsize = if !args.is_empty() {
        if let Some(n) = args[0].as_int() {
            n
        } else if let PyObjectPayload::Dict(d) = &args[0].payload {
            let d = d.read();
            d.get(&ferrython_core::types::HashableKey::str_key(CompactString::from("maxsize")))
                .and_then(|v| v.as_int())
                .unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };
    let class = PyObject::class(CompactString::from(kind), vec![], IndexMap::new());
    let inst = PyObject::instance(class);
    let items: Rc<PyCell<Vec<PyObjectRef>>> = Rc::new(PyCell::new(Vec::new()));
    let unfinished = Arc::new(Mutex::new(0i64));
    let is_lifo = kind == "LifoQueue";
    let is_priority = kind == "PriorityQueue";
    let empty_cls = empty_cls.clone();

    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__queue__"), PyObject::str_val(CompactString::from(kind)));
        w.insert(CompactString::from("maxsize"), PyObject::int(maxsize));

        // put(item)
        let it1 = items.clone();
        let uf1 = unfinished.clone();
        let ms1 = maxsize;
        w.insert(CompactString::from("put"), PyObject::native_closure(
            "put", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("put() requires 1 argument")); }
                let mut v = it1.write();
                if ms1 > 0 && v.len() as i64 >= ms1 {
                    return Err(PyException::runtime_error("queue is full"));
                }
                if is_priority {
                    // Insert in sorted order (min-heap via sorted Vec)
                    let item = a[0].clone();
                    let pos = v.iter().position(|x| {
                        // Compare: try numeric first, then string
                        if let (Ok(a_val), Ok(b_val)) = (item.to_float(), x.to_float()) {
                            a_val < b_val
                        } else {
                            item.py_to_string() < x.py_to_string()
                        }
                    }).unwrap_or(v.len());
                    v.insert(pos, item);
                } else {
                    v.push(a[0].clone());
                }
                *uf1.lock().unwrap() += 1;
                Ok(PyObject::none())
            }));

        // put_nowait(item) — same as put for single-threaded
        let it1b = items.clone();
        let uf1b = unfinished.clone();
        let ms1b = maxsize;
        w.insert(CompactString::from("put_nowait"), PyObject::native_closure(
            "put_nowait", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("put_nowait() requires 1 argument")); }
                let mut v = it1b.write();
                if ms1b > 0 && v.len() as i64 >= ms1b {
                    return Err(PyException::runtime_error("queue is full"));
                }
                v.push(a[0].clone());
                *uf1b.lock().unwrap() += 1;
                Ok(PyObject::none())
            }));

        // get(block=True, timeout=None)
        let it2 = items.clone();
        let ec_get = empty_cls.clone();
        w.insert(CompactString::from("get"), PyObject::native_closure(
            "get", move |args: &[PyObjectRef]| {
                // Parse block and timeout from kwargs-style positional args
                let block = if args.len() > 0 {
                    args[0].is_truthy()
                } else {
                    true
                };
                let timeout_ms: Option<u64> = if args.len() > 1 {
                    args[1].to_float().ok().map(|t| (t * 1000.0) as u64)
                } else {
                    None
                };

                if block && timeout_ms.is_some() {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.unwrap());
                    loop {
                        {
                            let mut v = it2.write();
                            if !v.is_empty() {
                                return if is_lifo {
                                    Ok(v.pop().unwrap())
                                } else {
                                    Ok(v.remove(0))
                                };
                            }
                        }
                        if std::time::Instant::now() >= deadline {
                            let empty_inst = PyObject::instance(ec_get.clone());
                            return Err(PyException::with_original(ExceptionKind::RuntimeError, "queue.Empty", empty_inst));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                } else {
                    let mut v = it2.write();
                    if v.is_empty() {
                        let empty_inst = PyObject::instance(ec_get.clone());
                        return Err(PyException::with_original(ExceptionKind::RuntimeError, "queue.Empty", empty_inst));
                    }
                    if is_lifo {
                        Ok(v.pop().unwrap())
                    } else {
                        Ok(v.remove(0))
                    }
                }
            }));

        // get_nowait() — same as get for single-threaded
        let it2b = items.clone();
        let ec_gn = empty_cls.clone();
        w.insert(CompactString::from("get_nowait"), PyObject::native_closure(
            "get_nowait", move |_: &[PyObjectRef]| {
                let mut v = it2b.write();
                if v.is_empty() {
                    let empty_inst = PyObject::instance(ec_gn.clone());
                    return Err(PyException::with_original(ExceptionKind::RuntimeError, "queue.Empty", empty_inst));
                }
                if is_lifo {
                    Ok(v.pop().unwrap())
                } else {
                    Ok(v.remove(0))
                }
            }));

        // qsize()
        let it3 = items.clone();
        w.insert(CompactString::from("qsize"), PyObject::native_closure(
            "qsize", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(it3.read().len() as i64))
            }));

        // empty()
        let it4 = items.clone();
        w.insert(CompactString::from("empty"), PyObject::native_closure(
            "empty", move |_: &[PyObjectRef]| {
                Ok(PyObject::bool_val(it4.read().is_empty()))
            }));

        // full()
        let it5 = items.clone();
        let ms2 = maxsize;
        w.insert(CompactString::from("full"), PyObject::native_closure(
            "full", move |_: &[PyObjectRef]| {
                if ms2 <= 0 { return Ok(PyObject::bool_val(false)); }
                Ok(PyObject::bool_val(it5.read().len() as i64 >= ms2))
            }));

        // task_done()
        let uf2 = unfinished.clone();
        w.insert(CompactString::from("task_done"), PyObject::native_closure(
            "task_done", move |_: &[PyObjectRef]| {
                let mut u = uf2.lock().unwrap();
                if *u <= 0 {
                    return Err(PyException::value_error("task_done() called too many times"));
                }
                *u -= 1;
                Ok(PyObject::none())
            }));

        // join() — blocks until all tasks done
        let uf3 = unfinished.clone();
        w.insert(CompactString::from("join"), PyObject::native_closure(
            "join", move |_: &[PyObjectRef]| {
                // Spin-wait with backoff until unfinished tasks reach 0
                loop {
                    if *uf3.lock().unwrap() <= 0 {
                        return Ok(PyObject::none());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }));

        // _items for backwards compat
        let it6 = items.clone();
        w.insert(CompactString::from("_items"), PyObject::native_closure(
            "_items", move |_: &[PyObjectRef]| {
                Ok(PyObject::list(it6.read().clone()))
            }));
    }
    Ok(inst)
}

// ── array module ─────────────────────────────────────────────────────
pub fn create_array_module() -> PyObjectRef {
    make_module("array", vec![
        ("array", make_builtin(array_array)),
        ("typecodes", PyObject::str_val(CompactString::from("bBuhHiIlLqQfd"))),
    ])
}

fn array_array(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("array() requires at least 1 argument"));
    }
    let typecode = args[0].py_to_string();
    if typecode.len() != 1 || !"bBuhHiIlLqQfd".contains(&typecode) {
        return Err(PyException::value_error(format!(
            "bad typecode (must be b, B, u, h, H, i, I, l, L, q, Q, f, or d): '{}'", typecode
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
        attrs.insert(CompactString::from("typecode"), PyObject::str_val(CompactString::from(&typecode)));
        attrs.insert(CompactString::from("_data"), PyObject::list(items));
        attrs.insert(CompactString::from("__array__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("itemsize"), PyObject::int(match typecode.as_str() {
            "b" | "B" => 1, "u" | "h" | "H" => 2, "i" | "I" | "l" | "L" | "f" => 4,
            "q" | "Q" | "d" => 8, _ => 1,
        }));

        let self_ref = inst.clone();

        attrs.insert(CompactString::from("append"), PyObject::native_closure(
            "array.append", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                check_args_min("array.append", args, 1)?;
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        items.write().push(args[0].clone());
                    }
                }
                Ok(PyObject::none())
            }}
        ));

        attrs.insert(CompactString::from("extend"), PyObject::native_closure(
            "array.extend", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                check_args_min("array.extend", args, 1)?;
                let new_items = args[0].to_list()?;
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let mut w = items.write();
                        for item in new_items { w.push(item); }
                    }
                }
                Ok(PyObject::none())
            }}
        ));

        attrs.insert(CompactString::from("pop"), PyObject::native_closure(
            "array.pop", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let mut w = items.write();
                        if w.is_empty() {
                            return Err(PyException::index_error("pop from empty array"));
                        }
                        let idx = if !args.is_empty() {
                            let i = args[0].to_int()? as isize;
                            if i < 0 { (w.len() as isize + i) as usize } else { i as usize }
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
            }}
        ));

        attrs.insert(CompactString::from("insert"), PyObject::native_closure(
            "array.insert", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
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
            }}
        ));

        attrs.insert(CompactString::from("remove"), PyObject::native_closure(
            "array.remove", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                check_args_min("array.remove", args, 1)?;
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let mut w = items.write();
                        let target = &args[0];
                        if let Some(pos) = w.iter().position(|x| x.py_to_string() == target.py_to_string()) {
                            w.remove(pos);
                            return Ok(PyObject::none());
                        }
                        return Err(PyException::value_error("array.remove(x): x not in array"));
                    }
                }
                Err(PyException::type_error("corrupted array"))
            }}
        ));

        attrs.insert(CompactString::from("index"), PyObject::native_closure(
            "array.index", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                check_args_min("array.index", args, 1)?;
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let r = items.read();
                        let target = &args[0];
                        if let Some(pos) = r.iter().position(|x| x.py_to_string() == target.py_to_string()) {
                            return Ok(PyObject::int(pos as i64));
                        }
                        return Err(PyException::value_error("array.index(x): x not in array"));
                    }
                }
                Err(PyException::type_error("corrupted array"))
            }}
        ));

        attrs.insert(CompactString::from("count"), PyObject::native_closure(
            "array.count", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                check_args_min("array.count", args, 1)?;
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let r = items.read();
                        let target = &args[0];
                        let n = r.iter().filter(|x| x.py_to_string() == target.py_to_string()).count();
                        return Ok(PyObject::int(n as i64));
                    }
                }
                Ok(PyObject::int(0))
            }}
        ));

        attrs.insert(CompactString::from("reverse"), PyObject::native_closure(
            "array.reverse", { let s = self_ref.clone(); move |_args: &[PyObjectRef]| {
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        items.write().reverse();
                    }
                }
                Ok(PyObject::none())
            }}
        ));

        attrs.insert(CompactString::from("tolist"), PyObject::native_closure(
            "array.tolist", { let s = self_ref.clone(); move |_args: &[PyObjectRef]| {
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        return Ok(PyObject::list(items.read().clone()));
                    }
                }
                Ok(PyObject::list(vec![]))
            }}
        ));

        attrs.insert(CompactString::from("tobytes"), PyObject::native_closure(
            "array.tobytes", { let s = self_ref.clone(); move |_args: &[PyObjectRef]| {
                let typecode = s.get_attr("typecode").map(|v| v.py_to_string()).unwrap_or_default();
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let r = items.read();
                        let mut bytes = Vec::new();
                        for x in r.iter() {
                            let v = x.to_int().unwrap_or(0);
                            match typecode.as_str() {
                                "b" => bytes.push(v as i8 as u8),
                                "B" => bytes.push(v as u8),
                                "h" => bytes.extend_from_slice(&(v as i16).to_ne_bytes()),
                                "H" => bytes.extend_from_slice(&(v as u16).to_ne_bytes()),
                                "i" | "l" => bytes.extend_from_slice(&(v as i32).to_ne_bytes()),
                                "I" | "L" => bytes.extend_from_slice(&(v as u32).to_ne_bytes()),
                                "q" => bytes.extend_from_slice(&v.to_ne_bytes()),
                                "Q" => bytes.extend_from_slice(&(v as u64).to_ne_bytes()),
                                "f" => {
                                    let fv = x.to_float().unwrap_or(0.0) as f32;
                                    bytes.extend_from_slice(&fv.to_ne_bytes());
                                }
                                "d" => {
                                    let fv = x.to_float().unwrap_or(0.0);
                                    bytes.extend_from_slice(&fv.to_ne_bytes());
                                }
                                _ => bytes.push(v as u8),
                            }
                        }
                        return Ok(PyObject::bytes(bytes));
                    }
                }
                Ok(PyObject::bytes(vec![]))
            }}
        ));

        attrs.insert(CompactString::from("frombytes"), PyObject::native_closure(
            "array.frombytes", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                check_args_min("array.frombytes", args, 1)?;
                let input_bytes = if let PyObjectPayload::Bytes(b) = &args[0].payload {
                    (**b).clone()
                } else {
                    return Err(PyException::type_error("frombytes requires a bytes argument"));
                };
                let typecode = s.get_attr("typecode").map(|v| v.py_to_string()).unwrap_or_default();
                let itemsize: usize = match typecode.as_str() {
                    "b" | "B" => 1, "h" | "H" => 2, "i" | "I" | "l" | "L" | "f" => 4,
                    "q" | "Q" | "d" => 8, _ => 1,
                };
                if input_bytes.len() % itemsize != 0 {
                    return Err(PyException::value_error("bytes length not a multiple of item size"));
                }
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let mut w = items.write();
                        for chunk in input_bytes.chunks(itemsize) {
                            let val: PyObjectRef = match typecode.as_str() {
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
                            };
                            w.push(val);
                        }
                    }
                }
                Ok(PyObject::none())
            }}
        ));

        // __repr__: array('i', [1, 2, 3])
        attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
            "array.__repr__", { let s = self_ref.clone(); move |_args: &[PyObjectRef]| {
                let tc = s.get_attr("typecode").map(|v| v.py_to_string()).unwrap_or_default();
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let r = items.read();
                        let items_str: Vec<String> = r.iter().map(|x| x.repr()).collect();
                        return Ok(PyObject::str_val(CompactString::from(
                            format!("array('{}', [{}])", tc, items_str.join(", "))
                        )));
                    }
                }
                Ok(PyObject::str_val(CompactString::from(format!("array('{}')", tc))))
            }}
        ));

        attrs.insert(CompactString::from("__len__"), PyObject::native_closure(
            "array.__len__", { let s = self_ref.clone(); move |_args: &[PyObjectRef]| {
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        return Ok(PyObject::int(items.read().len() as i64));
                    }
                }
                Ok(PyObject::int(0))
            }}
        ));

        attrs.insert(CompactString::from("__getitem__"), PyObject::native_closure(
            "array.__getitem__", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                check_args_min("array.__getitem__", args, 1)?;
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let r = items.read();
                        let i = args[0].to_int()? as isize;
                        let idx = if i < 0 { (r.len() as isize + i) as usize } else { i as usize };
                        if idx >= r.len() {
                            return Err(PyException::index_error("array index out of range"));
                        }
                        return Ok(r[idx].clone());
                    }
                }
                Err(PyException::type_error("corrupted array"))
            }}
        ));

        attrs.insert(CompactString::from("__setitem__"), PyObject::native_closure(
            "array.__setitem__", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                check_args_min("array.__setitem__", args, 2)?;
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let mut w = items.write();
                        let i = args[0].to_int()? as isize;
                        let idx = if i < 0 { (w.len() as isize + i) as usize } else { i as usize };
                        if idx >= w.len() {
                            return Err(PyException::index_error("array assignment index out of range"));
                        }
                        w[idx] = args[1].clone();
                        return Ok(PyObject::none());
                    }
                }
                Err(PyException::type_error("corrupted array"))
            }}
        ));

        attrs.insert(CompactString::from("__contains__"), PyObject::native_closure(
            "array.__contains__", { let s = self_ref.clone(); move |args: &[PyObjectRef]| {
                check_args_min("array.__contains__", args, 1)?;
                if let Some(data) = s.get_attr("_data") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        let r = items.read();
                        let target = &args[0];
                        return Ok(PyObject::bool_val(
                            r.iter().any(|x| x.py_to_string() == target.py_to_string())
                        ));
                    }
                }
                Ok(PyObject::bool_val(false))
            }}
        ));

        attrs.insert(CompactString::from("__iter__"), PyObject::native_closure(
            "array.__iter__", { let s = self_ref.clone(); move |_args: &[PyObjectRef]| {
                if let Some(data) = s.get_attr("_data") {
                    return data.get_iter();
                }
                Err(PyException::type_error("corrupted array"))
            }}
        ));
    }
    Ok(inst)
}
