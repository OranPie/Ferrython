use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::helpers::range_next_i64;
use ferrython_core::object::{
    check_args, check_args_min, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{hash_key_like_python, PyInt};

pub(crate) fn builtin_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("abs", args, 1)?;
    args[0].py_abs()
}

pub(crate) fn builtin_min(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "min expected at least 1 argument, got 0",
        ));
    }
    min_max_impl(args, std::cmp::Ordering::Less, "min")
}

pub(crate) fn builtin_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "max expected at least 1 argument, got 0",
        ));
    }
    min_max_impl(args, std::cmp::Ordering::Greater, "max")
}

fn min_max_impl(
    args: &[PyObjectRef],
    target_ord: std::cmp::Ordering,
    name: &str,
) -> PyResult<PyObjectRef> {
    // Multi-arg: min(a, b, c, ...) — track best by index, clone once at end
    if args.len() > 1 {
        let mut best_idx = 0usize;
        for (i, item) in args[1..].iter().enumerate() {
            if ferrython_core::object::helpers::partial_cmp_objects(item, &args[best_idx])
                == Some(target_ord)
            {
                best_idx = i + 1;
            }
        }
        return Ok(args[best_idx].clone());
    }
    // Single-arg: direct slice access for list/tuple (avoid to_list clone)
    let items: &[PyObjectRef] = match &args[0].payload {
        PyObjectPayload::List(v) => unsafe { &*v.data_ptr() },
        PyObjectPayload::Tuple(v) => v.as_slice(),
        _ => {
            let materialized = args[0].to_list()?;
            if materialized.is_empty() {
                return Err(PyException::value_error(&format!(
                    "{}() arg is an empty sequence",
                    name
                )));
            }
            return min_max_slice(&materialized, target_ord);
        }
    };
    if items.is_empty() {
        return Err(PyException::value_error(&format!(
            "{}() arg is an empty sequence",
            name
        )));
    }
    min_max_slice(items, target_ord)
}

/// Optimized min/max over a slice: uses direct i64 comparison for homogeneous small-int lists.
fn min_max_slice(items: &[PyObjectRef], target_ord: std::cmp::Ordering) -> PyResult<PyObjectRef> {
    // Single-pass: try small-int scan, fall back to generic on first non-int
    if items.len() >= 2 {
        let is_min = target_ord == std::cmp::Ordering::Less;
        if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(first_val)) =
            &items[0].payload
        {
            let mut best_idx = 0usize;
            let mut best_val = *first_val;
            let mut all_small = true;
            for (i, item) in items[1..].iter().enumerate() {
                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(v)) = &item.payload
                {
                    if (is_min && *v < best_val) || (!is_min && *v > best_val) {
                        best_val = *v;
                        best_idx = i + 1;
                    }
                } else {
                    all_small = false;
                    break;
                }
            }
            if all_small {
                return Ok(items[best_idx].clone());
            }
        }
    }
    // Generic path: track best by index, clone only once at the end
    let mut best_idx = 0usize;
    for (i, item) in items[1..].iter().enumerate() {
        if ferrython_core::object::helpers::partial_cmp_objects(item, &items[best_idx])
            == Some(target_ord)
        {
            best_idx = i + 1;
        }
    }
    Ok(items[best_idx].clone())
}

pub(crate) fn builtin_sum(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "sum expected at least 1 argument, got 0",
        ));
    }
    let start_val = if args.len() > 1 {
        &args[1]
    } else {
        &PyObject::int(0)
    };

    // Direct accumulation over list/tuple without cloning the entire container
    let items_ref: &[PyObjectRef] = match &args[0].payload {
        PyObjectPayload::List(v) => unsafe { &*v.data_ptr() },
        PyObjectPayload::Tuple(v) => v.as_slice(),
        // O(1) range sum via arithmetic series: n * (start + stop - step) / 2
        PyObjectPayload::Range(rd) => {
            if rd.step == 0 {
                return Err(PyException::value_error("range() arg 3 must not be zero"));
            }
            let n = if rd.step > 0 {
                if rd.start >= rd.stop {
                    0i64
                } else {
                    (rd.stop - rd.start + rd.step - 1) / rd.step
                }
            } else {
                if rd.start <= rd.stop {
                    0i64
                } else {
                    (rd.start - rd.stop - rd.step - 1) / (-rd.step)
                }
            };
            if n == 0 {
                return Ok(start_val.clone());
            }
            // sum = n * start + step * n * (n - 1) / 2
            let range_sum = (n as i128) * (rd.start as i128)
                + (rd.step as i128) * (n as i128) * ((n - 1) as i128) / 2;
            let start_i = match &start_val.payload {
                PyObjectPayload::Int(PyInt::Small(s)) => *s as i128,
                PyObjectPayload::Float(f) => return Ok(PyObject::float(*f + range_sum as f64)),
                _ => {
                    // General start: fall back to materialization
                    let items = args[0].to_list()?;
                    return sum_items(&items, start_val);
                }
            };
            let total = start_i + range_sum;
            if total >= i64::MIN as i128 && total <= i64::MAX as i128 {
                return Ok(PyObject::int(total as i64));
            }
            use num_bigint::BigInt;
            return Ok(PyObject::big_int(BigInt::from(total)));
        }
        // Iterate RangeIter directly without materializing to Vec
        PyObjectPayload::RangeIter(ri) => {
            let mut current = ri.current.get();
            let mut total: i64 = match &start_val.payload {
                PyObjectPayload::Int(PyInt::Small(s)) => *s,
                _ => {
                    let items = args[0].to_list()?;
                    return sum_items(&items, start_val);
                }
            };
            while let Some((value, next)) = range_next_i64(current, ri.stop, ri.step) {
                total = total.wrapping_add(value);
                current = next;
            }
            return Ok(PyObject::int(total));
        }
        _ => {
            // Fallback: materialize to list
            let items = args[0].to_list()?;
            return sum_items(&items, start_val);
        }
    };
    sum_items(items_ref, start_val)
}

fn sum_items(items: &[PyObjectRef], start: &PyObjectRef) -> PyResult<PyObjectRef> {
    // Native i64 accumulation for homogeneous int lists
    if let PyObjectPayload::Int(PyInt::Small(s)) = &start.payload {
        let mut total: i64 = *s;
        let mut all_int = true;
        for item in items {
            if let PyObjectPayload::Int(PyInt::Small(n)) = &item.payload {
                total = total.wrapping_add(*n);
            } else {
                all_int = false;
                break;
            }
        }
        if all_int {
            return Ok(PyObject::int(total));
        }
    }

    // Native f64 accumulation for numeric lists
    let start_f64 = match &start.payload {
        PyObjectPayload::Int(PyInt::Small(s)) => Some(*s as f64),
        PyObjectPayload::Float(f) => Some(*f),
        _ => None,
    };
    if let Some(mut total) = start_f64 {
        let mut all_numeric = true;
        for item in items {
            match &item.payload {
                PyObjectPayload::Float(f) => total += f,
                PyObjectPayload::Int(PyInt::Small(n)) => total += *n as f64,
                _ => {
                    all_numeric = false;
                    break;
                }
            }
        }
        if all_numeric {
            return Ok(PyObject::float(total));
        }
    }

    // General fallback
    let mut total = start.clone();
    for item in items {
        total = total.add(item)?;
    }
    Ok(total)
}

pub(crate) fn builtin_round(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("round", args, 1)?;
    let ndigits = if args.len() >= 2 {
        Some(args[1].to_int()?)
    } else {
        None
    };
    match &args[0].payload {
        PyObjectPayload::Int(i) => {
            if let Some(n) = ndigits {
                if n < 0 {
                    let f = i.to_f64();
                    let factor = 10f64.powi((-n) as i32);
                    let rounded = ((f / factor).round() * factor) as i64;
                    Ok(PyObject::int(rounded))
                } else {
                    Ok(args[0].clone())
                }
            } else {
                Ok(args[0].clone())
            }
        }
        PyObjectPayload::Float(f) => {
            if let Some(n) = ndigits {
                if n >= 0 {
                    if n > 308 {
                        return Ok(PyObject::float(*f));
                    }
                    // Use string formatting to match CPython's rounding behavior.
                    let formatted = format!("{:.prec$}", f, prec = n as usize);
                    let rounded: f64 = formatted.parse().unwrap_or(*f);
                    Ok(PyObject::float(rounded))
                } else {
                    let factor = 10f64.powi((-n) as i32);
                    let rounded = (f / factor).round() * factor;
                    Ok(PyObject::float(rounded))
                }
            } else {
                Ok(PyObject::int(round_half_to_even(*f) as i64))
            }
        }
        PyObjectPayload::Bool(b) => Ok(PyObject::int(if *b { 1 } else { 0 })),
        _ => {
            // Check for __round__ dunder method
            if let Some(round_method) = args[0].get_attr("__round__") {
                match &round_method.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        let mut call_args = vec![args[0].clone()];
                        if args.len() >= 2 {
                            call_args.push(args[1].clone());
                        }
                        return (nf.func)(&call_args);
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        let mut call_args = vec![args[0].clone()];
                        if args.len() >= 2 {
                            call_args.push(args[1].clone());
                        }
                        return (nc.func)(&call_args);
                    }
                    _ => {}
                }
            }
            Err(PyException::type_error(format!(
                "type '{}' doesn't define __round__ method",
                args[0].type_name()
            )))
        }
    }
}

/// IEEE 754 round-half-to-even (banker's rounding).
/// When the value is exactly halfway between two integers, round to the nearest even integer.
fn round_half_to_even(x: f64) -> f64 {
    let rounded = x.round();
    // Check if we're exactly at a .5 boundary (use strict f64 comparison)
    let frac = (x - x.floor()).abs();
    if (frac - 0.5).abs() < f64::EPSILON * x.abs().max(1.0) {
        // Exactly halfway — round to even
        if rounded as i64 % 2 != 0 {
            if x > 0.0 {
                rounded - 1.0
            } else {
                rounded + 1.0
            }
        } else {
            rounded
        }
    } else {
        rounded
    }
}

pub(crate) fn builtin_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("pow", args, 2)?;
    if args.len() >= 3 {
        // Three-argument pow(base, exp, mod) — modular exponentiation
        let modulus = &args[2];
        if matches!(&modulus.payload, PyObjectPayload::None) {
            return Ok(args[0].power(&args[1])?);
        }
        // Complex does not support 3-arg pow — raise ValueError to match CPython
        if matches!(&args[0].payload, PyObjectPayload::Complex { .. })
            || matches!(&args[1].payload, PyObjectPayload::Complex { .. })
            || matches!(&modulus.payload, PyObjectPayload::Complex { .. })
        {
            return Err(PyException::value_error("complex modulo"));
        }
        let base_i = args[0].as_int().ok_or_else(||
            PyException::type_error("pow() 1st argument not allowed for 3-argument pow() unless all arguments are integers"))?;
        let exp_i = args[1].as_int().ok_or_else(|| {
            PyException::type_error(
                "pow() 2nd argument cannot be negative when 3rd argument specified",
            )
        })?;
        let mod_i = modulus.as_int().ok_or_else(|| {
            PyException::type_error(
                "pow() 3rd argument not allowed unless all arguments are integers",
            )
        })?;
        if mod_i == 0 {
            return Err(PyException::value_error("pow() 3rd argument cannot be 0"));
        }
        if exp_i < 0 {
            // Modular inverse: pow(a, -1, m) = modular inverse of a mod m (Python 3.8+)
            // Compute over abs(m), then convert the final residue to the sign of m.
            let m = modulus_abs_i128(mod_i);
            let a = (base_i as i128).rem_euclid(m);
            let (g, x) = extended_gcd(a, m);
            if g != 1 {
                return Err(PyException::value_error(
                    "base is not invertible for the given modulus",
                ));
            }
            let inv = x.rem_euclid(m);
            // For exponents < -1, compute pow(inv, -exp, mod)
            let pos_exp = exp_i.unsigned_abs();
            if pos_exp == 1 {
                return Ok(PyObject::int(mod_residue_to_i64(inv, mod_i)));
            }
            let result = mod_pow_i128(inv, pos_exp, mod_i);
            return Ok(PyObject::int(result));
        }
        let result = mod_pow_i128(base_i as i128, exp_i as u64, mod_i);
        Ok(PyObject::int(result))
    } else {
        Ok(args[0].power(&args[1])?)
    }
}

/// Modular exponentiation: (base^exp) % modulus using repeated squaring
fn mod_pow_i128(base: i128, mut exp: u64, modulus: i64) -> i64 {
    let m_i = modulus_abs_i128(modulus);
    let m = m_i as u128;
    let mut result: u128 = 1 % m;
    let mut b = base.rem_euclid(m_i) as u128;
    while exp > 0 {
        if exp & 1 == 1 {
            result = result * b % m;
        }
        b = b * b % m;
        exp >>= 1;
    }
    mod_residue_to_i64(result as i128, modulus)
}

fn modulus_abs_i128(modulus: i64) -> i128 {
    if modulus < 0 {
        -(modulus as i128)
    } else {
        modulus as i128
    }
}

fn mod_residue_to_i64(residue: i128, modulus: i64) -> i64 {
    let signed = if modulus < 0 && residue > 0 {
        residue - modulus_abs_i128(modulus)
    } else {
        residue
    };
    signed as i64
}

/// Extended Euclidean algorithm: returns (gcd, x) such that a*x = gcd (mod b).
fn extended_gcd(a: i128, b: i128) -> (i128, i128) {
    let (mut old_r, mut r) = (a, b);
    let (mut old_s, mut s) = (1i128, 0i128);
    while r != 0 {
        let q = old_r / r;
        (old_r, r) = (r, old_r - q * r);
        (old_s, s) = (s, old_s - q * s);
    }
    (old_r.abs(), old_s)
}

pub(crate) fn builtin_divmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("divmod", args, 2)?;
    let q = args[0].floor_div(&args[1])?;
    let r = args[0].modulo(&args[1])?;
    Ok(PyObject::tuple(vec![q, r]))
}

pub(crate) fn builtin_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hash", args, 1)?;
    if let PyObjectPayload::Instance(inst) = &args[0].payload {
        if inst.attrs.read().contains_key("__deque__") {
            return Err(PyException::type_error("unhashable type: 'deque'"));
        }
        let is_weak_ref_like = {
            let attrs = inst.attrs.read();
            attrs.contains_key("__weakref_ref__") || attrs.contains_key("__weakmethod__")
        };
        if !is_weak_ref_like && crate::VirtualMachine::class_blocks_hash(&inst.class) {
            return Err(PyException::type_error(format!(
                "unhashable type: '{}'",
                args[0].type_name()
            )));
        }
        let is_weak_method = inst.attrs.read().contains_key("__weakmethod__");
        let weak_call = {
            let attrs = inst.attrs.read();
            if is_weak_method {
                attrs.get("__call__").cloned()
            } else if attrs.contains_key("__weakref_ref__") {
                attrs.get("__weakref_target__").cloned()
            } else {
                None
            }
        };
        if let Some(call) = weak_call {
            let referent = ferrython_core::object::call_callable(&call, &[])?;
            if matches!(&referent.payload, PyObjectPayload::None) {
                if let Some(cached) = inst.attrs.read().get("__weakref_hash__").cloned() {
                    return Ok(cached);
                }
                return Err(PyException::type_error(
                    "weak object has gone away".to_string(),
                ));
            }
            let hash = if let PyObjectPayload::BoundMethod { receiver, method } = &referent.payload
            {
                let receiver_key = receiver.to_hashable_key()?;
                let receiver_hash = hash_key_like_python(&receiver_key) as u64;
                let method_hash = match &method.payload {
                    PyObjectPayload::Function(func) => {
                        let mut h: u64 = 5381;
                        for c in func.name.as_bytes() {
                            h = h.wrapping_mul(33).wrapping_add(*c as u64);
                        }
                        for c in func.qualname.as_bytes() {
                            h = h.wrapping_mul(33).wrapping_add(*c as u64);
                        }
                        h = h
                            .wrapping_mul(33)
                            .wrapping_add(func.code.first_line_number as u64);
                        h
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        let mut h: u64 = 5381;
                        for c in nc.name.as_bytes() {
                            h = h.wrapping_mul(33).wrapping_add(*c as u64);
                        }
                        h
                    }
                    PyObjectPayload::NativeFunction(nf) => {
                        let mut h: u64 = 5381;
                        for c in nf.name.as_bytes() {
                            h = h.wrapping_mul(33).wrapping_add(*c as u64);
                        }
                        h
                    }
                    _ => PyObjectRef::as_ptr(method) as u64,
                };
                PyObject::int((receiver_hash ^ method_hash) as i64)
            } else {
                let key = referent.to_hashable_key()?;
                PyObject::int(hash_key_like_python(&key))
            };
            inst.attrs
                .write()
                .insert(CompactString::from("__weakref_hash__"), hash.clone());
            return Ok(hash);
        }
    }
    if let PyObjectPayload::FrozenSet(items) = &args[0].payload {
        return Ok(PyObject::int(items.py_hash()));
    }
    let key = args[0].to_hashable_key()?;
    Ok(PyObject::int(hash_key_like_python(&key)))
}

pub(crate) fn builtin_callable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("callable", args, 1)?;
    Ok(PyObject::bool_val(args[0].is_callable()))
}

pub(crate) fn builtin_input(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if !args.is_empty() {
        print!("{}", args[0].py_to_string());
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_line(&mut buf)
        .map_err(|e| PyException::runtime_error(format!("input error: {}", e)))?;
    if buf.ends_with('\n') {
        buf.pop();
    }
    if buf.ends_with('\r') {
        buf.pop();
    }
    Ok(PyObject::str_val(CompactString::from(buf)))
}

pub(crate) fn builtin_ord(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("ord", args, 1)?;
    // Accept both str and bytes (CPython: ord('a') == ord(b'a') == 97)
    match &args[0].payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            if b.len() != 1 {
                return Err(PyException::type_error(format!(
                    "ord() expected a character, but bytes of length {} found",
                    b.len()
                )));
            }
            return Ok(PyObject::int(b[0] as i64));
        }
        PyObjectPayload::Int(n) => {
            // bytearray indexing returns int in Python 3
            let v = n.to_i64().unwrap_or(0);
            return Ok(PyObject::int(v));
        }
        _ => {}
    }
    let s = args[0].as_str().ok_or_else(|| {
        PyException::type_error("ord() expected string of length 1, but found non-string")
    })?;
    let mut chars = s.chars();
    let c = chars
        .next()
        .ok_or_else(|| PyException::type_error("ord() expected a character"))?;
    if chars.next().is_some() {
        return Err(PyException::type_error(
            "ord() expected a character, but string of length > 1 found",
        ));
    }
    Ok(PyObject::int(c as i64))
}

pub(crate) fn builtin_chr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("chr", args, 1)?;
    let n = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("chr() expects int"))?;
    if n < 0 || n > 0x10FFFF {
        return Err(PyException::value_error(format!(
            "chr() arg not in range(0x110000): {}",
            n
        )));
    }
    // Rust char doesn't allow surrogates (0xD800-0xDFFF), but CPython does
    let s = if let Some(c) = char::from_u32(n as u32) {
        c.to_string()
    } else {
        // Surrogate codepoint — encode as replacement char
        String::from('\u{FFFD}')
    };
    Ok(PyObject::str_val(CompactString::from(s)))
}

/// Resolve an integer from an object, trying `as_int()` first then `__index__`.
fn resolve_index(obj: &PyObjectRef, func_name: &str) -> PyResult<i64> {
    if let Some(n) = obj.as_int() {
        return Ok(n);
    }
    // Try __index__ protocol on instances
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        // Look up __index__ in the instance's class or attrs
        let index_fn = {
            let attrs = inst.attrs.read();
            attrs.get("__index__").cloned()
        }
        .or_else(|| obj.get_attr("__index__"));
        if let Some(func) = index_fn {
            let result = match &func.payload {
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                PyObjectPayload::NativeFunction(nf) => (nf.func)(&[])?,
                PyObjectPayload::BoundMethod {
                    receiver: _,
                    method,
                } => {
                    // Call the bound method — for simple __index__ methods that
                    // just return an int, we can try NativeFunction/NativeClosure
                    match &method.payload {
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[obj.clone()])?,
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[obj.clone()])?,
                        // Python-defined __index__ needs VM; we can't call it here.
                        // Fall through to error.
                        _ => {
                            return Err(PyException::type_error(format!(
                                "'{}'() integer argument expected, got '{}'",
                                func_name,
                                obj.type_name()
                            )))
                        }
                    }
                }
                PyObjectPayload::Function(_) => {
                    // Python function needs VM to call — can't do it from here.
                    // But for the common case of __index__ defined in the class,
                    // it'll be accessed as BoundMethod via get_attr, handled above.
                    return Err(PyException::type_error(format!(
                        "'{}'() integer argument expected, got '{}'",
                        func_name,
                        obj.type_name()
                    )));
                }
                _ => {
                    return Err(PyException::type_error(format!(
                        "'{}'() integer argument expected, got '{}'",
                        func_name,
                        obj.type_name()
                    )))
                }
            };
            if let Some(n) = result.as_int() {
                return Ok(n);
            }
        }
    }
    Err(PyException::type_error(format!(
        "'{}'() integer argument expected, got '{}'",
        func_name,
        obj.type_name()
    )))
}

pub(crate) fn builtin_hex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hex", args, 1)?;
    let n = resolve_index(&args[0], "hex")?;
    let s = if n < 0 {
        format!("-0x{:x}", -n)
    } else {
        format!("0x{:x}", n)
    };
    Ok(PyObject::str_val(CompactString::from(s)))
}

pub(crate) fn builtin_oct(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("oct", args, 1)?;
    let n = resolve_index(&args[0], "oct")?;
    let s = if n < 0 {
        format!("-0o{:o}", -n)
    } else {
        format!("0o{:o}", n)
    };
    Ok(PyObject::str_val(CompactString::from(s)))
}

pub(crate) fn builtin_bin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("bin", args, 1)?;
    let n = resolve_index(&args[0], "bin")?;
    let s = if n < 0 {
        format!("-0b{:b}", -n)
    } else {
        format!("0b{:b}", n)
    };
    Ok(PyObject::str_val(CompactString::from(s)))
}
