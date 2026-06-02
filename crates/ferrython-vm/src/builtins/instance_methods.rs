//! Instance method dispatch for stdlib types (deque, CSV, hashlib, IO, pathlib, datetime, queue).
//!
//! These are method handlers for Python objects that are implemented as Instance
//! payloads with special marker attributes (e.g., __deque__, __stringio__).

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    NativeFunctionData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::{One, ToPrimitive};

mod csv;
mod datetime;
mod deque;
mod hashlib;
mod instance_dict;
mod io;
mod namedtuple;
mod pathlib;
mod queue;

pub(super) use csv::{call_csv_dictwriter_method, call_csv_writer_method};
pub(super) use datetime::{call_datetime_method, call_timedelta_method};
pub(super) use deque::call_deque_method;
pub(super) use hashlib::call_hashlib_method;
pub(super) use instance_dict::call_instance_dict_method;
pub(super) use io::{call_bytesio_method, call_stringio_method};
pub(super) use namedtuple::call_namedtuple_method;
pub(super) use pathlib::call_pathlib_method;
pub(super) use queue::call_queue_method;

use super::core_fns::{builtin_dict_fromkeys, builtin_type};

/// Resolve class-level methods on builtin types (e.g., dict.fromkeys, int.from_bytes).
pub fn resolve_type_class_method(type_name: &str, method_name: &str) -> Option<PyObjectRef> {
    match (type_name, method_name) {
        ("dict", "fromkeys") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("dict.fromkeys"),
                func: builtin_dict_fromkeys,
            },
        )))),
        (
            "dict",
            "get" | "pop" | "popitem" | "setdefault" | "update" | "clear" | "copy" | "keys"
            | "values" | "items" | "__init__" | "__getitem__" | "__setitem__" | "__delitem__"
            | "__contains__",
        ) => ferrython_core::object::helpers::resolve_builtin_type_method("dict", method_name),
        ("int", "from_bytes") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("int.from_bytes"),
                func: builtin_int_from_bytes,
            },
        )))),
        ("bool", "from_bytes") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("bool.from_bytes"),
                func: builtin_bool_from_bytes,
            },
        )))),
        ("str", "maketrans") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("str.maketrans"),
                func: builtin_str_maketrans,
            },
        )))),
        ("bytes", "fromhex") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("bytes.fromhex"),
                func: builtin_bytes_fromhex,
            },
        )))),
        ("bytes", "maketrans") | ("bytearray", "maketrans") => Some(PyObject::wrap(
            PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
                name: CompactString::from("bytes.maketrans"),
                func: builtin_bytes_maketrans,
            })),
        )),
        ("object", "__getattribute__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(
            Box::new(NativeFunctionData {
                name: CompactString::from("object.__getattribute__"),
                func: builtin_object_getattribute,
            }),
        ))),
        ("object", "__setattr__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(
            Box::new(NativeFunctionData {
                name: CompactString::from("object.__setattr__"),
                func: builtin_object_setattr,
            }),
        ))),
        ("object", "__delattr__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(
            Box::new(NativeFunctionData {
                name: CompactString::from("object.__delattr__"),
                func: builtin_object_delattr,
            }),
        ))),
        ("type", "__setattr__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("type.__setattr__"),
                func: builtin_type_setattr,
            },
        )))),
        ("type", "__delattr__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("type.__delattr__"),
                func: builtin_type_delattr,
            },
        )))),
        ("type", "__new__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("type.__new__"),
                func: builtin_type,
            },
        )))),
        ("float", "fromhex") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("float.fromhex"),
                func: builtin_float_fromhex,
            },
        )))),
        // property descriptor methods: property.__get__(self, obj, type)
        ("property", "__get__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(
            NativeFunctionData {
                name: CompactString::from("property.__get__"),
                func: |args: &[PyObjectRef]| {
                    // property.__get__(self, obj, objtype=None)
                    // self is the property object, obj is the instance
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "descriptor '__get__' requires a property object",
                        ));
                    }
                    let prop = &args[0];
                    let obj = args.get(1);
                    // If obj is None or not provided, return the property itself
                    let obj = match obj {
                        Some(o) if !matches!(&o.payload, PyObjectPayload::None) => o,
                        _ if ferrython_core::object::is_dynamic_class_attribute(prop) => {
                            return Err(PyException::attribute_error(""));
                        }
                        _ => return Ok(prop.clone()),
                    };
                    // Get the fget from the property
                    if let PyObjectPayload::Property(pd) = &prop.payload {
                        if let Some(getter) = pd.fget.as_ref() {
                            let getter = crate::builtins::core_fns::unwrap_abstract_fget(getter);
                            return Ok(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: obj.clone(),
                                    method: getter,
                                },
                            }));
                        }
                        return Err(PyException::attribute_error("unreadable attribute"));
                    }
                    // For InstanceProperty (subclass of property), look for fget in instance attrs
                    if let PyObjectPayload::Instance(inst) = &prop.payload {
                        if let Some(fget) = inst.attrs.read().get("fget").cloned() {
                            if !matches!(&fget.payload, PyObjectPayload::None) {
                                return Ok(PyObjectRef::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod {
                                        receiver: obj.clone(),
                                        method: fget,
                                    },
                                }));
                            }
                        }
                    }
                    Err(PyException::attribute_error("unreadable attribute"))
                },
            },
        )))),
        ("property", "__init__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(
            Box::new(NativeFunctionData {
                name: CompactString::from("property.__init__"),
                func: |args: &[PyObjectRef]| {
                    // property.__init__(self, fget=None, fset=None, fdel=None, doc=None)
                    // Store fget/fset/fdel on the instance so subclasses work
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    let property_arg = |idx: usize| {
                        args.get(idx).and_then(|arg| {
                            if matches!(&arg.payload, PyObjectPayload::None) {
                                None
                            } else {
                                Some(arg.clone())
                            }
                        })
                    };
                    let fget = property_arg(1);
                    let fset = property_arg(2);
                    let fdel = property_arg(3);
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        let mut w = inst.attrs.write();
                        w.insert(
                            CompactString::from("fget"),
                            fget.clone().unwrap_or_else(PyObject::none),
                        );
                        w.insert(
                            CompactString::from("fset"),
                            fset.clone().unwrap_or_else(PyObject::none),
                        );
                        w.insert(
                            CompactString::from("fdel"),
                            fdel.clone().unwrap_or_else(PyObject::none),
                        );
                    }
                    let (doc, doc_from_getter) = ferrython_core::object::property_init_doc(
                        fget.as_ref(),
                        args.get(4).cloned(),
                    );
                    if let Some(doc) = doc {
                        ferrython_core::object::property_set_doc(&args[0], doc)?;
                    }
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs.write().insert(
                            CompactString::from("__property_doc_from_getter__"),
                            PyObject::bool_val(doc_from_getter),
                        );
                    }
                    Ok(PyObject::none())
                },
            }),
        ))),
        _ => None,
    }
}

pub(super) fn builtin_int_from_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "int.from_bytes requires at least 1 argument",
        ));
    }
    let bytes = match &args[0].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        PyObjectPayload::ByteArray(b) => (**b).clone(),
        _ => {
            let mut out = Vec::new();
            for item in args[0].to_list()? {
                let Some(value) = item.as_int() else {
                    return Err(PyException::type_error(
                        "'bytes' object cannot be interpreted as an integer",
                    ));
                };
                if !(0..=255).contains(&value) {
                    return Err(PyException::value_error("bytes must be in range(0, 256)"));
                }
                out.push(value as u8);
            }
            out
        }
    };
    // Extract byteorder and signed from positional or kwargs dict
    let mut byteorder = "big".to_string();
    let mut signed = false;
    // Check if last arg is a kwargs dict
    if let Some(last) = args.last() {
        if args.len() >= 2 {
            if let PyObjectPayload::Dict(map) = &last.payload {
                let map_r = map.read();
                if let Some(bo) = map_r.get(&HashableKey::str_key(CompactString::from("byteorder")))
                {
                    byteorder = bo.py_to_string();
                }
                if let Some(s) = map_r.get(&HashableKey::str_key(CompactString::from("signed"))) {
                    signed = s.is_truthy();
                }
            } else {
                byteorder = args[1].py_to_string();
            }
        }
    }
    // Also check positional arg 2 for signed (if not from kwargs)
    if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::Dict(_)) {
        signed = args[2].is_truthy();
    }
    let mut result = BigInt::from(0u8);
    match byteorder.as_str() {
        "big" => {
            for &b in &bytes {
                result = (result << 8) + BigInt::from(b);
            }
        }
        "little" => {
            for (i, &b) in bytes.iter().enumerate() {
                result += BigInt::from(b) << (8 * i);
            }
        }
        _ => {
            return Err(PyException::value_error(
                "byteorder must be 'big' or 'little'",
            ))
        }
    }
    if signed {
        let bits = bytes.len() * 8;
        if bits > 0 {
            let sign_bit = BigInt::one() << (bits - 1);
            if (&result & &sign_bit) != BigInt::from(0u8) {
                result -= BigInt::one() << bits;
            }
        }
    }
    Ok(result
        .to_i64()
        .map(PyObject::int)
        .unwrap_or_else(|| PyObject::big_int(result)))
}

pub(super) fn builtin_bool_from_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let v = builtin_int_from_bytes(args)?;
    let truthy = v.is_truthy();
    Ok(PyObject::bool_val(truthy))
}

pub(super) fn builtin_str_maketrans(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "str.maketrans requires at least 1 argument",
        ));
    }
    let mut map = IndexMap::new();
    if args.len() >= 2 {
        let from = args[0].py_to_string();
        let to = args[1].py_to_string();
        for (fc, tc) in from.chars().zip(to.chars()) {
            map.insert(
                HashableKey::Int(PyInt::Small(fc as i64)),
                PyObject::str_val(CompactString::from(tc.to_string())),
            );
        }
        if args.len() >= 3 {
            let delete = args[2].py_to_string();
            for c in delete.chars() {
                map.insert(HashableKey::Int(PyInt::Small(c as i64)), PyObject::none());
            }
        }
    } else if let PyObjectPayload::Dict(d) = &args[0].payload {
        let r = d.read();
        for (k, v) in r.iter() {
            map.insert(k.clone(), v.clone());
        }
    }
    Ok(PyObject::dict(map))
}

pub(super) fn builtin_bytes_fromhex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("bytes.fromhex requires 1 argument"));
    }
    let hex_str = match &args[0].payload {
        PyObjectPayload::Str(s) => s,
        _ => {
            return Err(PyException::type_error(format!(
                "fromhex() argument must be str, not {}",
                args[0].type_name()
            )))
        }
    };
    let mut bytes = Vec::with_capacity(hex_str.len() / 2);
    let mut hi: Option<(usize, u8)> = None;
    for (pos, &byte) in hex_str.as_bytes().iter().enumerate() {
        if matches!(byte, b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r' | b' ') {
            if hi.is_some() {
                return Err(PyException::value_error(format!(
                    "non-hexadecimal number found in fromhex() arg at position {}",
                    pos
                )));
            }
            continue;
        }
        let Some(value) = (byte as char).to_digit(16).map(|v| v as u8) else {
            return Err(PyException::value_error(format!(
                "non-hexadecimal number found in fromhex() arg at position {}",
                pos
            )));
        };
        if let Some((_, high)) = hi.take() {
            bytes.push((high << 4) | value);
        } else {
            hi = Some((pos, value));
        }
    }
    if let Some((pos, _)) = hi {
        return Err(PyException::value_error(format!(
            "non-hexadecimal number found in fromhex() arg at position {}",
            pos + 1
        )));
    }
    Ok(PyObject::bytes(bytes))
}

pub(super) fn builtin_bytes_maketrans(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("maketrans requires 2 arguments"));
    }
    let from_bytes = match &args[0].payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
        _ => return Err(PyException::type_error("a bytes-like object is required")),
    };
    let to_bytes = match &args[1].payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
        _ => return Err(PyException::type_error("a bytes-like object is required")),
    };
    if from_bytes.len() != to_bytes.len() {
        return Err(PyException::value_error(
            "maketrans arguments must have same length",
        ));
    }
    let mut table: Vec<u8> = (0..=255u8).collect();
    for (f, t) in from_bytes.iter().zip(to_bytes.iter()) {
        table[*f as usize] = *t;
    }
    Ok(PyObject::bytes(table))
}

pub(super) fn builtin_float_fromhex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("float.fromhex requires 1 argument"));
    }
    let hex_str = args[0].py_to_string().trim().to_lowercase();
    // Handle special values
    match hex_str.as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => return Ok(PyObject::float(f64::INFINITY)),
        "-inf" | "-infinity" => return Ok(PyObject::float(f64::NEG_INFINITY)),
        "nan" | "+nan" | "-nan" => return Ok(PyObject::float(f64::NAN)),
        _ => {}
    }
    // Parse hex float format: [sign] "0x" hex_mantissa "p" exp
    let (sign, rest) = if hex_str.starts_with('-') {
        (-1.0f64, &hex_str[1..])
    } else if hex_str.starts_with('+') {
        (1.0, &hex_str[1..])
    } else {
        (1.0, hex_str.as_str())
    };
    let rest = rest.strip_prefix("0x").unwrap_or(rest);
    let (mantissa_str, exp_str) = if let Some(p_idx) = rest.find('p') {
        (&rest[..p_idx], Some(&rest[p_idx + 1..]))
    } else {
        (rest, None)
    };
    if mantissa_str.is_empty() {
        return Err(PyException::value_error(
            "invalid hexadecimal floating-point string",
        ));
    }
    let exp: i32 = match exp_str {
        Some(s) => s
            .parse()
            .map_err(|_| PyException::value_error("invalid hexadecimal floating-point string"))?,
        None => 0,
    };
    let (int_part, frac_part) = if let Some(dot) = mantissa_str.find('.') {
        (&mantissa_str[..dot], &mantissa_str[dot + 1..])
    } else {
        (mantissa_str, "")
    };
    if int_part.is_empty() && frac_part.is_empty() {
        Err(PyException::value_error(
            "invalid hexadecimal floating-point string",
        ))
    } else if !int_part.chars().all(|c| c.is_ascii_hexdigit())
        || !frac_part.chars().all(|c| c.is_ascii_hexdigit())
    {
        Err(PyException::value_error(
            "invalid hexadecimal floating-point string",
        ))
    } else {
        let int_val = if int_part.is_empty() {
            0.0
        } else {
            i64::from_str_radix(int_part, 16).map_err(|_| {
                PyException::value_error("invalid hexadecimal floating-point string")
            })? as f64
        };
        let frac_val: f64 = if frac_part.is_empty() {
            0.0
        } else {
            let frac_int = i64::from_str_radix(frac_part, 16).map_err(|_| {
                PyException::value_error("invalid hexadecimal floating-point string")
            })?;
            frac_int as f64 / (16.0f64).powi(frac_part.len() as i32)
        };
        let value = sign * (int_val + frac_val) * (2.0f64).powi(exp);
        Ok(PyObject::float(value))
    }
}
pub(super) fn builtin_object_getattribute(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "object.__getattribute__ requires 2 arguments",
        ));
    }
    let obj = &args[0];
    let name = args[1].py_to_string();
    match obj.get_attr(&name) {
        Some(v) => Ok(v),
        None => Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'",
            obj.type_name(),
            name
        ))),
    }
}

/// object.__setattr__(self, name, value)
pub(super) fn builtin_object_setattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "object.__setattr__ requires 3 arguments",
        ));
    }
    let obj = &args[0];
    let name = args[1].py_to_string();
    let value = args[2].clone();
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs.write().insert(CompactString::from(name), value);
        Ok(PyObject::none())
    } else if let PyObjectPayload::ExceptionInstance(ei) = &obj.payload {
        ei.ensure_attrs()
            .write()
            .insert(CompactString::from(name), value);
        Ok(PyObject::none())
    } else if let PyObjectPayload::Function(f) = &obj.payload {
        f.attrs.write().insert(CompactString::from(name), value);
        Ok(PyObject::none())
    } else if matches!(
        &obj.payload,
        PyObjectPayload::NativeFunction(_)
            | PyObjectPayload::NativeClosure(_)
            | PyObjectPayload::BuiltinFunction(_)
    ) {
        // Silently accept for native functions
        Ok(PyObject::none())
    } else {
        Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute assignment",
            obj.type_name()
        )))
    }
}

/// object.__delattr__(self, name)
pub(super) fn builtin_object_delattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "object.__delattr__ requires 2 arguments",
        ));
    }
    let obj = &args[0];
    let name = args[1].py_to_string();
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if inst.attrs.write().swap_remove(name.as_str()).is_some() {
            Ok(PyObject::none())
        } else {
            Err(PyException::attribute_error(format!(
                "'{}' object has no attribute '{}'",
                obj.type_name(),
                name
            )))
        }
    } else {
        Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute deletion",
            obj.type_name()
        )))
    }
}

fn builtin_type_setattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 3 {
        return Err(PyException::type_error(
            "type.__setattr__() takes exactly 3 arguments",
        ));
    }
    let attr_name = match &args[1].payload {
        PyObjectPayload::Str(s) => s.to_compact_string(),
        _ => return Err(PyException::type_error("attribute name must be string")),
    };
    let PyObjectPayload::Class(cd) = &args[0].payload else {
        return Err(PyException::type_error(
            "descriptor '__setattr__' requires a 'type' object",
        ));
    };
    cd.namespace.write().insert(attr_name, args[2].clone());
    cd.invalidate_cache();
    Ok(PyObject::none())
}

fn builtin_type_delattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 2 {
        return Err(PyException::type_error(
            "type.__delattr__() takes exactly 2 arguments",
        ));
    }
    let attr_name = match &args[1].payload {
        PyObjectPayload::Str(s) => s.to_compact_string(),
        _ => return Err(PyException::type_error("attribute name must be string")),
    };
    let PyObjectPayload::Class(cd) = &args[0].payload else {
        return Err(PyException::type_error(
            "descriptor '__delattr__' requires a 'type' object",
        ));
    };
    if cd
        .namespace
        .write()
        .shift_remove(attr_name.as_str())
        .is_none()
    {
        return Err(PyException::attribute_error(attr_name.to_string()));
    }
    cd.invalidate_cache();
    Ok(PyObject::none())
}
