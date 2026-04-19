use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, CompareOp,
    make_module, make_builtin, check_args, check_args_min,
};

// ── operator module ──


pub fn create_operator_module() -> PyObjectRef {
    let m = make_module("operator", vec![
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
            Ok(PyObject::bool_val(PyObjectRef::ptr_eq(&args[0], &args[1])))
        })),
        ("is_not", make_builtin(|args| {
            check_args_min("is_not", args, 2)?;
            Ok(PyObject::bool_val(!PyObjectRef::ptr_eq(&args[0], &args[1])))
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
        ("isub", make_builtin(|args| {
            check_args_min("isub", args, 2)?;
            args[0].sub(&args[1])
        })),
        ("imul", make_builtin(|args| {
            check_args_min("imul", args, 2)?;
            args[0].mul(&args[1])
        })),
        ("itruediv", make_builtin(|args| {
            check_args_min("itruediv", args, 2)?;
            args[0].true_div(&args[1])
        })),
        ("ifloordiv", make_builtin(|args| {
            check_args_min("ifloordiv", args, 2)?;
            args[0].floor_div(&args[1])
        })),
        ("imod", make_builtin(|args| {
            check_args_min("imod", args, 2)?;
            args[0].modulo(&args[1])
        })),
        ("ipow", make_builtin(|args| {
            check_args_min("ipow", args, 2)?;
            args[0].power(&args[1])
        })),
        ("iand", make_builtin(|args| {
            check_args_min("iand", args, 2)?;
            args[0].bit_and(&args[1])
        })),
        ("ior", make_builtin(|args| {
            check_args_min("ior", args, 2)?;
            args[0].bit_or(&args[1])
        })),
        ("ixor", make_builtin(|args| {
            check_args_min("ixor", args, 2)?;
            args[0].bit_xor(&args[1])
        })),
        ("ilshift", make_builtin(|args| {
            check_args_min("ilshift", args, 2)?;
            args[0].lshift(&args[1])
        })),
        ("irshift", make_builtin(|args| {
            check_args_min("irshift", args, 2)?;
            args[0].rshift(&args[1])
        })),
        ("iconcat", make_builtin(|args| {
            check_args_min("iconcat", args, 2)?;
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
                    PyObjectPayload::NativeFunction(nf) => (nf.func)(&full_args),
                    PyObjectPayload::NativeClosure(nc) => (nc.func)(&full_args),
                    PyObjectPayload::BuiltinBoundMethod(bbm) => {
                        // Handle BuiltinBoundMethod with extra args by dispatching common methods
                        if !extra_args.is_empty() {
                            let s = bbm.receiver.py_to_string();
                            match bbm.method_name.as_str() {
                                "split" => {
                                    let sep = extra_args[0].py_to_string();
                                    let maxsplit = if extra_args.len() > 1 { extra_args[1].as_int().unwrap_or(-1) } else { -1 };
                                    let parts: Vec<&str> = if maxsplit < 0 {
                                        s.split(&*sep).collect()
                                    } else {
                                        s.splitn(maxsplit as usize + 1, &*sep).collect()
                                    };
                                    return Ok(PyObject::list(parts.into_iter().map(|p| PyObject::str_val(CompactString::from(p))).collect()));
                                }
                                "rsplit" => {
                                    let sep = extra_args[0].py_to_string();
                                    let maxsplit = if extra_args.len() > 1 { extra_args[1].as_int().unwrap_or(-1) } else { -1 };
                                    let parts: Vec<&str> = if maxsplit < 0 {
                                        s.rsplit(&*sep).collect()
                                    } else {
                                        s.rsplitn(maxsplit as usize + 1, &*sep).collect()
                                    };
                                    let mut result: Vec<PyObjectRef> = parts.into_iter().map(|p| PyObject::str_val(CompactString::from(p))).collect();
                                    result.reverse();
                                    return Ok(PyObject::list(result));
                                }
                                "replace" => {
                                    let old = extra_args[0].py_to_string();
                                    let new = if extra_args.len() > 1 { extra_args[1].py_to_string() } else { String::new() };
                                    let count = if extra_args.len() > 2 { extra_args[2].as_int().unwrap_or(-1) } else { -1 };
                                    let result = if count < 0 {
                                        s.replace(&*old, &new)
                                    } else {
                                        s.replacen(&*old, &new, count as usize)
                                    };
                                    return Ok(PyObject::str_val(CompactString::from(result)));
                                }
                                "join" => {
                                    let items = extra_args[0].to_list().unwrap_or_default();
                                    let joined: String = items.iter().map(|i| i.py_to_string()).collect::<Vec<_>>().join(&s);
                                    return Ok(PyObject::str_val(CompactString::from(joined)));
                                }
                                "encode" => {
                                    return Ok(PyObject::bytes(s.into_bytes()));
                                }
                                "find" => {
                                    let sub = extra_args[0].py_to_string();
                                    let idx = s.find(&*sub).map(|i| i as i64).unwrap_or(-1);
                                    return Ok(PyObject::int(idx));
                                }
                                "count" => {
                                    let sub = extra_args[0].py_to_string();
                                    return Ok(PyObject::int(s.matches(&*sub).count() as i64));
                                }
                                "startswith" => {
                                    let prefix = extra_args[0].py_to_string();
                                    return Ok(PyObject::bool_val(s.starts_with(&*prefix)));
                                }
                                "endswith" => {
                                    let suffix = extra_args[0].py_to_string();
                                    return Ok(PyObject::bool_val(s.ends_with(&*suffix)));
                                }
                                "center" | "ljust" | "rjust" => {
                                    let width = extra_args[0].as_int().unwrap_or(0) as usize;
                                    let fill = if extra_args.len() > 1 {
                                        extra_args[1].py_to_string().chars().next().unwrap_or(' ')
                                    } else { ' ' };
                                    let result = if s.len() >= width {
                                        s.clone()
                                    } else {
                                        let pad = width - s.len();
                                        match bbm.method_name.as_str() {
                                            "center" => {
                                                let left = pad / 2;
                                                let right = pad - left;
                                                format!("{}{}{}", fill.to_string().repeat(left), s, fill.to_string().repeat(right))
                                            }
                                            "ljust" => format!("{}{}", s, fill.to_string().repeat(pad)),
                                            "rjust" => format!("{}{}", fill.to_string().repeat(pad), s),
                                            _ => s.clone(),
                                        }
                                    };
                                    return Ok(PyObject::str_val(CompactString::from(result)));
                                }
                                _ => {
                                    // Fallback: use deferred call
                                    crate::concurrency_modules::push_deferred_call(method.clone(), extra_args.clone());
                                    return Ok(PyObject::none());
                                }
                            }
                        }
                        // No extra args: try common zero-arg string methods
                        let result_str = match bbm.method_name.as_str() {
                            "upper" => Some(bbm.receiver.py_to_string().to_uppercase()),
                            "lower" => Some(bbm.receiver.py_to_string().to_lowercase()),
                            "strip" => Some(bbm.receiver.py_to_string().trim().to_string()),
                            "lstrip" => Some(bbm.receiver.py_to_string().trim_start().to_string()),
                            "rstrip" => Some(bbm.receiver.py_to_string().trim_end().to_string()),
                            "title" => {
                                let s = bbm.receiver.py_to_string();
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
                                let s = bbm.receiver.py_to_string();
                                let mut chars = s.chars();
                                Some(match chars.next() {
                                    None => String::new(),
                                    Some(f) => f.to_uppercase().collect::<String>() + &chars.collect::<String>().to_lowercase(),
                                })
                            }
                            "swapcase" => {
                                let s = bbm.receiver.py_to_string();
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
                                // Python function — use deferred call
                                let mut bound_args = vec![receiver.clone()];
                                bound_args.extend(extra_args.iter().cloned());
                                crate::concurrency_modules::push_deferred_call(meth.clone(), bound_args);
                                Ok(PyObject::none())
                            }
                        }
                    }
                    PyObjectPayload::Function(_) => {
                        // Direct Python function call via deferred mechanism
                        let mut call_args_full = vec![obj.clone()];
                        call_args_full.extend(extra_args.iter().cloned());
                        crate::concurrency_modules::push_deferred_call(method.clone(), call_args_full);
                        Ok(PyObject::none())
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
                    PyObjectPayload::NativeFunction(nf) =>
                        (nf.func)(&[]).ok().and_then(|r| r.to_int().ok()).map(Ok),
                    PyObjectPayload::NativeClosure(nc) =>
                        (nc.func)(&[]).ok().and_then(|r| r.to_int().ok()).map(Ok),
                    PyObjectPayload::BoundMethod { receiver, method: m } => {
                        match &m.payload {
                            PyObjectPayload::NativeFunction(nf) =>
                                (nf.func)(&[receiver.clone()]).ok().and_then(|r| r.to_int().ok()).map(Ok),
                            PyObjectPayload::NativeClosure(nc) =>
                                (nc.func)(&[receiver.clone()]).ok().and_then(|r| r.to_int().ok()).map(Ok),
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
    ]);

    // Add dunder aliases: operator.__lt__ == operator.lt, etc.
    let dunder_aliases = [
        ("__lt__", "lt"), ("__le__", "le"), ("__eq__", "eq"),
        ("__ne__", "ne"), ("__gt__", "gt"), ("__ge__", "ge"),
        ("__add__", "add"), ("__sub__", "sub"), ("__mul__", "mul"),
        ("__mod__", "mod"), ("__pow__", "pow"), ("__neg__", "neg"),
        ("__pos__", "pos"), ("__abs__", "abs"), ("__not__", "not_"),
        ("__and__", "and_"), ("__or__", "or_"), ("__xor__", "xor"),
        ("__invert__", "invert"), ("__lshift__", "lshift"),
        ("__rshift__", "rshift"), ("__truediv__", "truediv"),
        ("__floordiv__", "floordiv"), ("__contains__", "contains"),
        ("__getitem__", "getitem"), ("__setitem__", "setitem"),
        ("__delitem__", "delitem"), ("__iadd__", "iadd"),
        ("__isub__", "isub"), ("__imul__", "imul"),
        ("__matmul__", "matmul"), ("__imatmul__", "imatmul"),
        ("__concat__", "concat"), ("__iconcat__", "iconcat"),
    ];
    if let PyObjectPayload::Module(ref md) = m.payload {
        for (dunder, orig) in &dunder_aliases {
            let val = md.attrs.read()
                .get(&CompactString::from(*orig))
                .cloned();
            if let Some(v) = val {
                md.attrs.write()
                    .insert(CompactString::from(*dunder), v);
            }
        }
    }

    m
}