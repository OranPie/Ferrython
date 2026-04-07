use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, CompareOp,
    make_module, make_builtin, check_args, check_args_min,
};

// ── operator module ──


pub fn create_operator_module() -> PyObjectRef {
    make_module("operator", vec![
        ("add", make_builtin(|args| {
            check_args_min("add", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a + b));
                }
            }
            if let (Ok(a), Ok(b)) = (args[0].to_float(), args[1].to_float()) {
                Ok(PyObject::float(a + b))
            } else {
                let a = args[0].py_to_string();
                let b = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(format!("{}{}", a, b))))
            }
        })),
        ("sub", make_builtin(|args| {
            check_args_min("sub", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a - b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a - b))
        })),
        ("mul", make_builtin(|args| {
            check_args_min("mul", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a * b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a * b))
        })),
        ("truediv", make_builtin(|args| {
            check_args_min("truediv", args, 2)?;
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
            Ok(PyObject::float(a / b))
        })),
        ("floordiv", make_builtin(|args| {
            check_args_min("floordiv", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.div_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("float floor division by zero")); }
            Ok(PyObject::float((a / b).floor()))
        })),
        ("mod_", make_builtin(|args| {
            check_args_min("mod_", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        // Also register as "mod" for getattr(operator, "mod") usage
        ("mod", make_builtin(|args| {
            check_args_min("mod", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        ("neg", make_builtin(|args| {
            check_args_min("neg", args, 1)?;
            if matches!(&args[0].payload, PyObjectPayload::Float(_)) {
                Ok(PyObject::float(-args[0].to_float()?))
            } else if let Ok(n) = args[0].to_int() {
                Ok(PyObject::int(-n))
            } else {
                Ok(PyObject::float(-args[0].to_float()?))
            }
        })),
        ("pow", make_builtin(|args| {
            check_args_min("pow", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
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
        })),
        ("pos", make_builtin(|args| {
            check_args_min("pos", args, 1)?;
            Ok(args[0].clone())
        })),
        ("not_", make_builtin(|args| {
            check_args_min("not_", args, 1)?;
            Ok(PyObject::bool_val(!args[0].is_truthy()))
        })),
        ("eq", make_builtin(|args| {
            check_args_min("eq", args, 2)?;
            args[0].compare(&args[1], CompareOp::Eq)
        })),
        ("ne", make_builtin(|args| {
            check_args_min("ne", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ne)
        })),
        ("lt", make_builtin(|args| {
            check_args_min("lt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Lt)
        })),
        ("le", make_builtin(|args| {
            check_args_min("le", args, 2)?;
            args[0].compare(&args[1], CompareOp::Le)
        })),
        ("gt", make_builtin(|args| {
            check_args_min("gt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Gt)
        })),
        ("ge", make_builtin(|args| {
            check_args_min("ge", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ge)
        })),
        ("abs", make_builtin(|args| {
            check_args_min("abs", args, 1)?;
            check_args("abs", args, 1)?;
            args[0].py_abs()
        })),
        ("contains", make_builtin(|args| {
            check_args_min("contains", args, 2)?;
            Ok(PyObject::bool_val(args[0].contains(&args[1])?))
        })),
        ("getitem", make_builtin(|args| {
            check_args_min("getitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.read().get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("list index out of range"))
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.read().get(&key).cloned()
                        .ok_or_else(|| PyException::key_error(args[1].repr()))
                }
                PyObjectPayload::Tuple(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("tuple index out of range"))
                }
                _ => Err(PyException::type_error("object is not subscriptable")),
            }
        })),
        ("itemgetter", make_builtin(|args| {
            check_args_min("itemgetter", args, 1)?;
            let keys: Vec<PyObjectRef> = args.to_vec();
            Ok(PyObject::native_closure("operator.itemgetter", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("itemgetter expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                if keys.len() == 1 {
                    obj.get_item(&keys[0])
                } else {
                    let items: Vec<PyObjectRef> = keys.iter()
                        .map(|k| obj.get_item(k))
                        .collect::<PyResult<Vec<_>>>()?;
                    Ok(PyObject::tuple(items))
                }
            }))
        })),
        ("attrgetter", make_builtin(|args| {
            check_args_min("attrgetter", args, 1)?;
            let attr_names: Vec<String> = args.iter().map(|a| a.py_to_string()).collect();
            Ok(PyObject::native_closure("operator.attrgetter", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("attrgetter expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                // Helper: resolve dotted attribute path (e.g. "a.b.c")
                let resolve = |name: &str, obj: &PyObjectRef| -> PyResult<PyObjectRef> {
                    let parts: Vec<&str> = name.split('.').collect();
                    let mut cur = obj.clone();
                    for part in &parts {
                        cur = cur.get_attr(part).ok_or_else(|| PyException::attribute_error(
                            format!("'{}' object has no attribute '{}'", cur.type_name(), part)
                        ))?;
                    }
                    Ok(cur)
                };
                if attr_names.len() == 1 {
                    resolve(&attr_names[0], obj)
                } else {
                    let items: Vec<PyObjectRef> = attr_names.iter()
                        .map(|name| resolve(name, obj))
                        .collect::<PyResult<Vec<_>>>()?;
                    Ok(PyObject::tuple(items))
                }
            }))
        })),
        ("and_", make_builtin(|args| {
            check_args_min("and_", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a & b))
        })),
        ("or_", make_builtin(|args| {
            check_args_min("or_", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a | b))
        })),
        ("xor", make_builtin(|args| {
            check_args_min("xor", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a ^ b))
        })),
        ("lshift", make_builtin(|args| {
            check_args_min("lshift", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a << b))
        })),
        ("rshift", make_builtin(|args| {
            check_args_min("rshift", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a >> b))
        })),
        ("invert", make_builtin(|args| {
            check_args_min("invert", args, 1)?;
            let a = args[0].to_int()?;
            Ok(PyObject::int(!a))
        })),
        ("inv", make_builtin(|args| {
            check_args_min("inv", args, 1)?;
            let a = args[0].to_int()?;
            Ok(PyObject::int(!a))
        })),
        ("truth", make_builtin(|args| {
            check_args_min("truth", args, 1)?;
            Ok(PyObject::bool_val(args[0].is_truthy()))
        })),
        ("is_", make_builtin(|args| {
            check_args_min("is_", args, 2)?;
            Ok(PyObject::bool_val(std::sync::Arc::ptr_eq(&args[0], &args[1])))
        })),
        ("is_not", make_builtin(|args| {
            check_args_min("is_not", args, 2)?;
            Ok(PyObject::bool_val(!std::sync::Arc::ptr_eq(&args[0], &args[1])))
        })),
        ("index", make_builtin(|args| {
            check_args_min("index", args, 1)?;
            args[0].to_int().map(PyObject::int)
        })),
        ("setitem", make_builtin(|args| {
            check_args_min("setitem", args, 3)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    let mut w = items.write();
                    if idx < w.len() {
                        w[idx] = args[2].clone();
                        Ok(PyObject::none())
                    } else {
                        Err(PyException::index_error("list assignment index out of range"))
                    }
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.write().insert(key, args[2].clone());
                    Ok(PyObject::none())
                }
                _ => Err(PyException::type_error("object does not support item assignment")),
            }
        })),
        ("delitem", make_builtin(|args| {
            check_args_min("delitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.write().shift_remove(&key);
                    Ok(PyObject::none())
                }
                _ => Err(PyException::type_error("object does not support item deletion")),
            }
        })),
        ("concat", make_builtin(|args| {
            check_args_min("concat", args, 2)?;
            args[0].add(&args[1])
        })),
        ("iadd", make_builtin(|args| {
            check_args_min("iadd", args, 2)?;
            args[0].add(&args[1])
        })),
        ("methodcaller", make_builtin(|args| {
            check_args_min("methodcaller", args, 1)?;
            let method_name = args[0].py_to_string();
            let extra_args: Vec<PyObjectRef> = if args.len() > 1 { args[1..].to_vec() } else { vec![] };
            Ok(PyObject::native_closure("operator.methodcaller", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("methodcaller expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                let method = obj.get_attr(&method_name)
                    .ok_or_else(|| PyException::attribute_error(format!(
                        "'{}' object has no attribute '{}'", obj.type_name(), method_name
                    )))?;
                // Build full args: [obj, ...extra_args]
                let mut full_args = vec![obj.clone()];
                full_args.extend(extra_args.iter().cloned());
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => func(&full_args),
                    PyObjectPayload::NativeClosure { func, .. } => func(&full_args),
                    PyObjectPayload::BuiltinBoundMethod { receiver, method_name, .. } => {
                        // Try to resolve common methods without VM
                        let result_str = match method_name.as_str() {
                            "upper" => Some(receiver.py_to_string().to_uppercase()),
                            "lower" => Some(receiver.py_to_string().to_lowercase()),
                            "strip" => Some(receiver.py_to_string().trim().to_string()),
                            "lstrip" => Some(receiver.py_to_string().trim_start().to_string()),
                            "rstrip" => Some(receiver.py_to_string().trim_end().to_string()),
                            "title" => {
                                let s = receiver.py_to_string();
                                let mut result = String::with_capacity(s.len());
                                let mut capitalize_next = true;
                                for c in s.chars() {
                                    if c.is_alphanumeric() {
                                        if capitalize_next { result.extend(c.to_uppercase()); capitalize_next = false; }
                                        else { result.extend(c.to_lowercase()); }
                                    } else { result.push(c); capitalize_next = true; }
                                }
                                Some(result)
                            }
                            "capitalize" => {
                                let s = receiver.py_to_string();
                                let mut chars = s.chars();
                                Some(match chars.next() {
                                    None => String::new(),
                                    Some(f) => f.to_uppercase().collect::<String>() + &chars.collect::<String>().to_lowercase(),
                                })
                            }
                            "swapcase" => {
                                let s = receiver.py_to_string();
                                Some(s.chars().map(|c| {
                                    if c.is_uppercase() { c.to_lowercase().collect::<String>() }
                                    else { c.to_uppercase().collect::<String>() }
                                }).collect())
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
                    PyObjectPayload::BoundMethod { receiver, method: meth, .. } => {
                        match &meth.payload {
                            PyObjectPayload::NativeFunction { func, .. } => {
                                let mut bound_args = vec![receiver.clone()];
                                bound_args.extend(extra_args.iter().cloned());
                                func(&bound_args)
                            }
                            _ => Ok(method),
                        }
                    }
                    _ => Ok(method),
                }
            }))
        })),
        ("length_hint", make_builtin(|args| {
            check_args_min("length_hint", args, 1)?;
            let default = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
            // Try to call a method natively; fall back to VM callback for Python funcs.
            let try_dunder = |method: &PyObjectRef| -> Option<Result<i64, ()>> {
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } =>
                        func(&[]).ok().and_then(|r| r.to_int().ok()).map(Ok),
                    PyObjectPayload::NativeClosure { func, .. } =>
                        func(&[]).ok().and_then(|r| r.to_int().ok()).map(Ok),
                    PyObjectPayload::BoundMethod { receiver, method: m } => {
                        match &m.payload {
                            PyObjectPayload::NativeFunction { func, .. } =>
                                func(&[receiver.clone()]).ok().and_then(|r| r.to_int().ok()).map(Ok),
                            PyObjectPayload::NativeClosure { func, .. } =>
                                func(&[receiver.clone()]).ok().and_then(|r| r.to_int().ok()).map(Ok),
                            _ => Some(Err(())), // needs VM
                        }
                    }
                    _ => Some(Err(())), // needs VM
                }
            };
            // Try __length_hint__ first, then __len__
            for dunder in &["__length_hint__", "__len__"] {
                if let Some(method) = args[0].get_attr(dunder) {
                    match try_dunder(&method) {
                        Some(Ok(n)) => return Ok(PyObject::int(n)),
                        Some(Err(())) => {
                            // Python function — request VM to call it
                            ferrython_core::error::request_vm_call(method, vec![]);
                            return Ok(PyObject::none());
                        }
                        None => {} // call failed, try next
                    }
                }
            }
            // Try len() directly
            match args[0].py_len() {
                Ok(n) => Ok(PyObject::int(n as i64)),
                Err(_) => Ok(PyObject::int(default)),
            }
        })),
        ("indexOf", make_builtin(|args| {
            check_args_min("indexOf", args, 2)?;
            let target = &args[1];
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    for (i, item) in items.read().iter().enumerate() {
                        if item.compare(target, CompareOp::Eq).map(|v| v.is_truthy()).unwrap_or(false) {
                            return Ok(PyObject::int(i as i64));
                        }
                    }
                    Err(PyException::value_error("sequence.index(x): x not in sequence"))
                }
                PyObjectPayload::Tuple(items) => {
                    for (i, item) in items.iter().enumerate() {
                        if item.compare(target, CompareOp::Eq).map(|v| v.is_truthy()).unwrap_or(false) {
                            return Ok(PyObject::int(i as i64));
                        }
                    }
                    Err(PyException::value_error("sequence.index(x): x not in sequence"))
                }
                _ => Err(PyException::type_error("indexOf requires a sequence")),
            }
        })),
        ("countOf", make_builtin(|args| {
            check_args_min("countOf", args, 2)?;
            let target = &args[1];
            let mut count = 0i64;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    for item in items.read().iter() {
                        if item.compare(target, CompareOp::Eq).map(|v| v.is_truthy()).unwrap_or(false) { count += 1; }
                    }
                }
                PyObjectPayload::Tuple(items) => {
                    for item in items.iter() {
                        if item.compare(target, CompareOp::Eq).map(|v| v.is_truthy()).unwrap_or(false) { count += 1; }
                    }
                }
                PyObjectPayload::Str(s) => {
                    let t = target.py_to_string();
                    count = s.matches(&*t).count() as i64;
                }
                _ => {}
            }
            Ok(PyObject::int(count))
        })),
    ])
}