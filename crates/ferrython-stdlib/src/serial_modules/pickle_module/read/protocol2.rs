use super::*;
use num_traits::ToPrimitive;

// ── Protocol 2 (binary) deserialization ──

pub(super) fn pickle_loads_p2(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos: usize = 0;
    let mut stack: Vec<PklStackItem> = Vec::new();
    let mut memo: std::collections::HashMap<u32, PyObjectRef> = std::collections::HashMap::new();

    // Skip protocol header
    if pos + 1 < data.len() && data[pos] == 0x80 {
        pos += 2;
    }

    while pos < data.len() {
        let opcode = data[pos];
        pos += 1;
        match opcode {
            b'.' => break, // STOP
            b'N' => stack.push(PklStackItem::Value(PyObject::none())),
            0x95 => {
                // FRAME — protocol 4 framing metadata; payload follows inline here.
                if pos + 8 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated FRAME",
                    ));
                }
                pos += 8;
            }
            0x88 => stack.push(PklStackItem::Value(PyObject::bool_val(true))),
            0x89 => stack.push(PklStackItem::Value(PyObject::bool_val(false))),
            b'K' => {
                // BININT1 — 1-byte unsigned int
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT1",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::int(data[pos] as i64)));
                pos += 1;
            }
            b'M' => {
                // BININT2 — 2-byte LE unsigned short
                if pos + 2 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BININT2",
                    ));
                }
                let val = u16::from_le_bytes([data[pos], data[pos + 1]]) as i64;
                stack.push(PklStackItem::Value(PyObject::int(val)));
                pos += 2;
            }
            b'J' => {
                // BININT — 4-byte LE signed int
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
            b'I' => {
                // INT (text fallback) — read to newline
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line).unwrap_or("0").trim();
                if s == "01" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(true)));
                } else if s == "00" {
                    stack.push(PklStackItem::Value(PyObject::bool_val(false)));
                } else {
                    stack.push(PklStackItem::Value(super::protocol0::pkl_parse_text_int(
                        s,
                    )?));
                }
            }
            b'G' => {
                // BINFLOAT — 8-byte BE IEEE 754 double
                if pos + 8 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINFLOAT",
                    ));
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[pos..pos + 8]);
                let val = f64::from_be_bytes(bytes);
                stack.push(PklStackItem::Value(PyObject::float(val)));
                pos += 8;
            }
            b'X' => {
                // BINUNICODE — 4-byte LE len + UTF-8
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
            0x8c => {
                // SHORT_BINUNICODE — 1-byte len + UTF-8
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINUNICODE",
                    ));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINUNICODE data",
                    ));
                }
                let s = std::str::from_utf8(&data[pos..pos + len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid utf-8"))?;
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
                pos += len;
            }
            b'T' => {
                // BINSTRING — 4-byte LE len + bytes
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINSTRING length",
                    ));
                }
                let len =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINSTRING data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b'U' => {
                // SHORT_BINSTRING — 1-byte len + bytes
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
            b'B' => {
                // BINBYTES — 4-byte LE len + raw bytes
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINBYTES length",
                    ));
                }
                let len =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]])
                        as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINBYTES data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b'C' => {
                // SHORT_BINBYTES — 1-byte len + raw bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINBYTES",
                    ));
                }
                let len = data[pos] as usize;
                pos += 1;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated SHORT_BINBYTES data",
                    ));
                }
                stack.push(PklStackItem::Value(PyObject::bytes(
                    data[pos..pos + len].to_vec(),
                )));
                pos += len;
            }
            b']' => stack.push(PklStackItem::Value(PyObject::list(vec![]))),
            b'}' => stack.push(PklStackItem::Value(PyObject::dict_from_pairs(vec![]))),
            b')' => stack.push(PklStackItem::Value(PyObject::tuple(vec![]))),
            b'(' => stack.push(PklStackItem::Mark),
            b'l' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::list(items)));
            }
            b't' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                stack.push(PklStackItem::Value(PyObject::tuple(items)));
            }
            b'd' => {
                let items = pkl_pop_to_mark(&mut stack)?;
                let mut pairs = Vec::new();
                for chunk in items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PklStackItem::Value(PyObject::dict_from_pairs(pairs)));
            }
            0x85 => {
                // TUPLE1
                let v = stack
                    .pop()
                    .ok_or_else(|| {
                        PyException::runtime_error("UnpicklingError: TUPLE1 stack underflow")
                    })
                    .and_then(pkl_stack_item_value)?;
                stack.push(PklStackItem::Value(PyObject::tuple(vec![v])));
            }
            0x86 => {
                // TUPLE2
                let b_val = stack
                    .pop()
                    .ok_or_else(|| {
                        PyException::runtime_error("UnpicklingError: TUPLE2 stack underflow")
                    })
                    .and_then(pkl_stack_item_value)?;
                let a_val = stack
                    .pop()
                    .ok_or_else(|| {
                        PyException::runtime_error("UnpicklingError: TUPLE2 stack underflow")
                    })
                    .and_then(pkl_stack_item_value)?;
                stack.push(PklStackItem::Value(PyObject::tuple(vec![a_val, b_val])));
            }
            0x87 => {
                // TUPLE3
                let c_val = stack
                    .pop()
                    .ok_or_else(|| {
                        PyException::runtime_error("UnpicklingError: TUPLE3 stack underflow")
                    })
                    .and_then(pkl_stack_item_value)?;
                let b_val = stack
                    .pop()
                    .ok_or_else(|| {
                        PyException::runtime_error("UnpicklingError: TUPLE3 stack underflow")
                    })
                    .and_then(pkl_stack_item_value)?;
                let a_val = stack
                    .pop()
                    .ok_or_else(|| {
                        PyException::runtime_error("UnpicklingError: TUPLE3 stack underflow")
                    })
                    .and_then(pkl_stack_item_value)?;
                stack.push(PklStackItem::Value(PyObject::tuple(vec![
                    a_val, b_val, c_val,
                ])));
            }
            b'a' => {
                // APPEND
                let item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: APPEND expects value",
                        ))
                    }
                };
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().push(item);
                    }
                }
            }
            b'e' => {
                // APPENDS — pop items to mark, extend list
                let items = pkl_pop_to_mark(&mut stack)?;
                if let Some(PklStackItem::Value(list_obj)) = stack.last() {
                    if let PyObjectPayload::List(ref list_items) = list_obj.payload {
                        list_items.write().extend(items);
                    }
                }
            }
            b's' => {
                // SETITEM
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
                // SETITEMS — pop pairs to mark, update dict
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
            b'q' => {
                // BINPUT — 1-byte memo index
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINPUT",
                    ));
                }
                let id = data[pos] as u32;
                pos += 1;
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'h' => {
                // BINGET — 1-byte memo index
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated BINGET",
                    ));
                }
                let id = data[pos] as u32;
                pos += 1;
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            b'r' => {
                // LONG_BINPUT — 4-byte LE memo index
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG_BINPUT",
                    ));
                }
                let id =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                pos += 4;
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'j' => {
                // LONG_BINGET — 4-byte LE memo index
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG_BINGET",
                    ));
                }
                let id =
                    u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
                pos += 4;
                let val = memo.get(&id).cloned().ok_or_else(|| {
                    PyException::runtime_error(format!(
                        "UnpicklingError: memo key {} not found",
                        id
                    ))
                })?;
                stack.push(PklStackItem::Value(val));
            }
            0x94 => {
                // MEMOIZE — store top of stack at the next memo index.
                let id = memo.len() as u32;
                let val = pkl_stack_top_value(&stack)?;
                memo.insert(id, val);
            }
            b'p' => {
                // PUT (text) — read id to newline
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
                // GET (text) — read id to newline
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
                // GLOBAL — module\nqualname\n
                let mod_line = p0_read_line(data, &mut pos);
                let name_line = p0_read_line(data, &mut pos);
                let module = String::from_utf8_lossy(mod_line).to_string();
                let name = String::from_utf8_lossy(name_line).to_string();
                stack.push(PklStackItem::Global(module, name));
            }
            0x93 => {
                // STACK_GLOBAL — pop name, pop module, push global
                let name_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v.py_to_string(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: STACK_GLOBAL expects name",
                        ))
                    }
                };
                let mod_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v.py_to_string(),
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: STACK_GLOBAL expects module",
                        ))
                    }
                };
                stack.push(PklStackItem::Global(mod_item, name_item));
            }
            0x81 => {
                // NEWOBJ — create cls.__new__(cls, *args), falling back to a blank instance.
                let args_item = match stack.pop() {
                    Some(PklStackItem::Value(v)) => v,
                    _ => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: NEWOBJ expects args",
                        ))
                    }
                };
                let cls = match stack.pop() {
                    Some(item) => pkl_stack_item_value(item)?,
                    None => {
                        return Err(PyException::runtime_error(
                            "UnpicklingError: NEWOBJ expects class",
                        ))
                    }
                };
                stack.push(PklStackItem::Value(pkl_newobj(cls, args_item)?));
            }
            b'R' => {
                // REDUCE
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
            0x8a => {
                // LONG1 — 1-byte count + little-endian 2's complement bytes
                if pos >= data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG1",
                    ));
                }
                let count = data[pos] as usize;
                pos += 1;
                if pos + count > data.len() {
                    return Err(PyException::runtime_error(
                        "UnpicklingError: truncated LONG1 data",
                    ));
                }
                let bytes = &data[pos..pos + count];
                pos += count;
                if count == 0 {
                    stack.push(PklStackItem::Value(PyObject::int(0)));
                } else {
                    let negative = bytes[count - 1] & 0x80 != 0;
                    let value = if negative {
                        let mut twos = bytes.to_vec();
                        for byte in &mut twos {
                            *byte = !*byte;
                        }
                        let mut carry = 1u16;
                        for byte in &mut twos {
                            let sum = *byte as u16 + carry;
                            *byte = sum as u8;
                            carry = sum >> 8;
                            if carry == 0 {
                                break;
                            }
                        }
                        -num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, &twos)
                    } else {
                        num_bigint::BigInt::from_bytes_le(num_bigint::Sign::Plus, bytes)
                    };
                    if let Some(small) = value.to_i64() {
                        stack.push(PklStackItem::Value(PyObject::int(small)));
                    } else {
                        stack.push(PklStackItem::Value(PyObject::big_int(value)));
                    }
                }
            }
            b'V' => {
                // UNICODE (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let s = p0_unescape_unicode(line);
                stack.push(PklStackItem::Value(PyObject::str_val(CompactString::from(
                    s,
                ))));
            }
            b'S' => {
                // STRING (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let bytes = p0_unescape_bytes(line);
                stack.push(PklStackItem::Value(PyObject::bytes(bytes)));
            }
            b'F' => {
                // FLOAT (text) — in case it appears in binary stream
                let line = p0_read_line(data, &mut pos);
                let s = std::str::from_utf8(line).unwrap_or("0").trim();
                let val: f64 = match s {
                    "nan" | "NaN" => f64::NAN,
                    "inf" => f64::INFINITY,
                    "-inf" => f64::NEG_INFINITY,
                    _ => s.parse().unwrap_or(0.0),
                };
                stack.push(PklStackItem::Value(PyObject::float(val)));
            }
            b'0' => {} // POP — discard top (used after PUT sometimes)
            b'1' => {} // POP_MARK — discard stack to mark
            b'2' => {
                // DUP — duplicate top of stack
                let val = pkl_stack_top_value(&stack)?;
                stack.push(PklStackItem::Value(val));
            }
            _ => {
                return Err(PyException::runtime_error(format!(
                    "UnpicklingError: unknown opcode 0x{:02x}",
                    opcode
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

// ── Unified deserialization (auto-detects protocol) ──
