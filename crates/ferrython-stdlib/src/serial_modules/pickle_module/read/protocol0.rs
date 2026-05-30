use super::*;

pub(super) fn pkl_parse_text_int(s: &str) -> PyResult<PyObjectRef> {
    if let Ok(value) = s.parse::<i64>() {
        Ok(PyObject::int(value))
    } else {
        let value = s.parse::<num_bigint::BigInt>().map_err(|_| {
            PyException::runtime_error(format!("UnpicklingError: invalid INT value '{}'", s))
        })?;
        Ok(PyObject::big_int(value))
    }
}

pub(super) fn pickle_loads_p0(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos: usize = 0;
    let mut stack: Vec<PklStackItem> = Vec::new();
    let mut memo: std::collections::HashMap<u32, PyObjectRef> = std::collections::HashMap::new();

    while pos < data.len() {
        let opcode = data[pos];
        pos += 1;
        match opcode {
            b'.' => break, // STOP
            b'N' => stack.push(PklStackItem::Value(PyObject::none())),
            b'I' => {
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| {
                        PyException::runtime_error("UnpicklingError: invalid INT encoding")
                    })?
                    .trim();
                if s == "01" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(true)));
                } else if s == "00" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(false)));
                } else {
                    stack.push(PklStackItem::Value(pkl_parse_text_int(s)?));
                }
            }
            b'L' => {
                // LONG — like I but for big ints, trailing L
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| {
                        PyException::runtime_error("UnpicklingError: invalid LONG encoding")
                    })?
                    .trim()
                    .trim_end_matches('L');
                stack.push(PklStackItem::Value(pkl_parse_text_int(s)?));
            }
            b'J' => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT",
                    ));
                }
                let val =
                    i32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as i64;
                stack.push(PklStackItem::Value(PyObject::int(val)));
                pos += 4;
            }
            b'K' => {
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT1",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::int(data[pos] as i64)));
                pos += 1;
            }
            b'F' => {
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line)
                    .map_err(|_| {
                        PyException::runtime_error("UnpicklingError: invalid FLOAT encoding")
                    })?
                    .trim();
                let val: f64 = match s {
                    "nan" | "NaN" => f64::NAN,
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => s.parse().map_err(|_| {
                        PyException::runtime_error(format!(
                            "UnpicklingError: invalid FLOAT value '{}'",
                            s
                        ))
                    })?,
                };
                stack.push(PklStackItem::Value(PyObject::float(val)));
            }
            b'V' => {
                // UNICODE — read raw-unicode-escape line
                let line = p0_read_line(data, &mut pos);
                let s = p0_unescape_unicode(line);
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
            }
            b'S' => {
                // STRING — read quoted string line (bytes)
                let line = p0_read_line(data, &mut pos);
                let bytes = p0_unescape_bytes(line);
                stack.push(PklStackItem::Value(PyObject::bytes(bytes)));
            }
            b'U' => {
                // SHORT_BINSTRING — protocol 1 may not carry a protocol header.
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINSTRING",
                    ));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINSTRING data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b'X' => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINUNICODE length",
                    ));
                }
                let len =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINUNICODE data",
                    ));
                }
                let s = std::str::from_utf8(&data[pos..pos + len]).map_err(|_| {
                    PyException::runtime_error("UnpicklingError: invalid utf-8 in BINUNICODE")
                })?;
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
                pos += len;
            }
            b'}' => stack.push(PklStackItem::Value(PyObject::dict_from_pairs(vec![]))),
            b'(' => stack.push(PklStackItem::Mark),
            b'l' => {
                // LIST — pop to mark, build list
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::list(items)));
            }
            b't' => {
                // TUPLE — pop to mark, build tuple
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::tuple(items)));
            }
            b'd' => {
                // DICT — pop to mark, build dict from pairs
                let items = pkl_pop_to_mark(&mut stack)?;
                let mut pairs = Vec::new();
                for chunk in items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PklStackItem::Value(PyObject::dict_from_pairs(pairs)));
            }
            b'a' => {
                // APPEND — pop item, append to list on stack
                let item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: APPEND expects value",
                        ))
                    }
                };
                // Find the list on top of the remaining stack
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().push(item);
                    }
                }
            }
            b's' => {
                // SETITEM — pop value, pop key, set on dict
                let val = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects value",
                        ))
                    }
                };
                let key = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: SETITEM expects key",
                        ))
                    }
                };
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        if let Ok(hk) = HashableKey::from_object(&key) {
                            dict_map.write().insert(hk, val);
                        }
                    }
                }
            }
            b'u' => {
                // SETITEMS — protocol 1 may appear without a protocol header.
                let items = pkl_pop_to_mark(&mut stack)?;
                if let Some(PklStackItem::Value(dict_obj)) = stack.last() {
                    if let PyObjectPayload::Dict(ref dict_map) = dict_obj.payload {
                        let mut map = dict_map.write();
                        for chunk in items.chunks_exact(2) {
                            if let Ok(hk) = HashableKey::from_object(&chunk[0]) {
                                map.insert(hk, chunk[1].clone());
                            }
                        }
                    }
                }
            }
            b'p' => {
                // PUT — memoize top of stack
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'g' => {
                // GET — recall from memo
                let line = p0_read_line(data, &mut pos);
                let id: u32 = std::str::from_utf8(line)
                    .unwrap_or("0")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'c' => {
                // GLOBAL — read module\nqualname\n
                let mod_line = p0_read_line(data, &mut pos);
                let name_line = p0_read_line(data, &mut pos);
                let module = String::from_utf8_lossy(mod_line).to_string();
                let name = String::from_utf8_lossy(name_line).to_string();
                stack.push(PklStackItem::Global(module, name));
            }
            b'R' => {
                // REDUCE — pop args tuple, pop callable, call
                let args_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects args",
                        ))
                    }
                };
                let callable = match stack.pop() {
                    Some(item) => item,
                    None => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: REDUCE expects callable",
                        ))
                    }
                };
                let result = pkl_reduce(&callable, &args_item)?;
                stack.push(PklStackItem::Value(result));
            }
            b'b' => {
                // BUILD — apply a state dict to the object on top of the stack.
                let state = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects state",
                        ))
                    }
                };
                let obj = match stack.last() {
                    Some(PklStackItem::Value(v)) => v.clone(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: BUILD expects object",
                        ))
                    }
                };
                if pkl_rebuild_uuid_from_state(&obj, &state)?.is_some() {
                    continue;
                }
                pkl_apply_state(&obj, &state)?;
            }
            b'\n' | b'\r' | b' ' => {} // skip whitespace
            _ => {
                return Err(PyException::runtime_error(format!(
                    "UnpicklingError: unknown protocol 0 opcode 0x{:02x} ('{}')",
                    opcode,
                    if opcode.is_ascii_graphic() {
                        opcode as char
                    } else {
                        '?'
                    }
                )));
            }
        }
    }

    if stack.last().is_some() {
        return pkl_stack_top_value(&stack);
    }
    Err(PyException::runtime_error(
        "UnpicklingError: empty pickle data",
    ))
}

// ── Protocol 2 (binary) deserialization ──
