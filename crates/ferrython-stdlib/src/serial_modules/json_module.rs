use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
mod parser;

use parser::parse_json_value;
pub use serialize::json_dumps;
use serialize::{py_to_json, try_call_default};

mod serialize;
pub fn create_json_module() -> PyObjectRef {
    // Build JSONEncoder as a proper class with methods in the namespace
    let mut enc_ns = IndexMap::new();
    enc_ns.insert(
        CompactString::from("encode"),
        PyObject::native_closure("encode", |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "JSONEncoder.encode() missing argument",
                ));
            }
            let s = py_to_json(&args[0])?;
            Ok(PyObject::str_val(CompactString::from(s)))
        }),
    );
    enc_ns.insert(
        CompactString::from("default"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            Err(PyException::type_error(format!(
                "Object of type {} is not JSON serializable",
                args[0].type_name()
            )))
        }),
    );
    let json_encoder_cls = PyObject::class(CompactString::from("JSONEncoder"), vec![], enc_ns);

    // Build JSONDecoder as a proper class with methods in the namespace
    let mut dec_ns = IndexMap::new();
    dec_ns.insert(
        CompactString::from("decode"),
        PyObject::native_closure("decode", |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "JSONDecoder.decode() missing argument",
                ));
            }
            let s = match &args[0].payload {
                PyObjectPayload::Str(s) => s.to_string(),
                _ => {
                    return Err(PyException::type_error(
                        "JSONDecoder.decode requires a string",
                    ))
                }
            };
            parse_json_value(&s, &mut 0)
        }),
    );
    dec_ns.insert(
        CompactString::from("raw_decode"),
        PyObject::native_closure("raw_decode", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("raw_decode() missing argument"));
            }
            let s = match &args[0].payload {
                PyObjectPayload::Str(s) => s.to_string(),
                _ => return Err(PyException::type_error("raw_decode requires a string")),
            };
            let mut pos = 0;
            let val = parse_json_value(&s, &mut pos)?;
            Ok(PyObject::tuple(vec![val, PyObject::int(pos as i64)]))
        }),
    );
    let json_decoder_cls = PyObject::class(CompactString::from("JSONDecoder"), vec![], dec_ns);

    make_module(
        "json",
        vec![
            ("dumps", PyObject::native_function("json.dumps", json_dumps)),
            ("loads", PyObject::native_function("json.loads", json_loads)),
            ("dump", PyObject::native_function("json.dump", json_dump)),
            ("load", PyObject::native_function("json.load", json_load)),
            ("JSONEncoder", json_encoder_cls),
            ("JSONDecoder", json_decoder_cls),
            (
                "JSONDecodeError",
                PyObject::class(
                    CompactString::from("JSONDecodeError"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
        ],
    )
}

fn json_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "json.loads requires a string argument",
        ));
    }
    let s = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        _ => return Err(PyException::type_error("json.loads requires a string")),
    };

    // Extract kwargs if present
    let kwargs = args.last().and_then(|a| {
        if let PyObjectPayload::Dict(d) = &a.payload {
            Some(d.read().clone())
        } else {
            None
        }
    });
    let object_hook = kwargs
        .as_ref()
        .and_then(|kw| {
            kw.get(&HashableKey::str_key(CompactString::from("object_hook")))
                .cloned()
        })
        .filter(|v| !matches!(&v.payload, PyObjectPayload::None));
    let parse_float = kwargs
        .as_ref()
        .and_then(|kw| {
            kw.get(&HashableKey::str_key(CompactString::from("parse_float")))
                .cloned()
        })
        .filter(|v| !matches!(&v.payload, PyObjectPayload::None));
    let parse_int = kwargs
        .as_ref()
        .and_then(|kw| {
            kw.get(&HashableKey::str_key(CompactString::from("parse_int")))
                .cloned()
        })
        .filter(|v| !matches!(&v.payload, PyObjectPayload::None));

    let result = parse_json_value(&s, &mut 0)?;

    // Apply hooks if provided
    if object_hook.is_some() || parse_float.is_some() || parse_int.is_some() {
        apply_json_hooks(&result, &object_hook, &parse_float, &parse_int)
    } else {
        Ok(result)
    }
}

fn apply_json_hooks(
    value: &PyObjectRef,
    object_hook: &Option<PyObjectRef>,
    parse_float: &Option<PyObjectRef>,
    parse_int: &Option<PyObjectRef>,
) -> PyResult<PyObjectRef> {
    match &value.payload {
        PyObjectPayload::Dict(d) => {
            // Recursively apply hooks to values
            let rd = d.read();
            let mut new_map = IndexMap::new();
            for (k, v) in rd.iter() {
                let new_v = apply_json_hooks(v, object_hook, parse_float, parse_int)?;
                new_map.insert(k.clone(), new_v);
            }
            let new_dict = PyObject::dict(new_map);
            // Apply object_hook to the dict
            if let Some(hook) = object_hook {
                try_call_default(hook, &new_dict).map(|r| r.unwrap_or(new_dict))
            } else {
                Ok(new_dict)
            }
        }
        PyObjectPayload::List(items) => {
            let r = items.read();
            let new_items: Vec<PyObjectRef> = r
                .iter()
                .map(|item| apply_json_hooks(item, object_hook, parse_float, parse_int))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyObject::list(new_items))
        }
        PyObjectPayload::Float(_) => {
            if let Some(pf) = parse_float {
                let s = PyObject::str_val(CompactString::from(value.py_to_string()));
                try_call_default(pf, &s).map(|r| r.unwrap_or_else(|| value.clone()))
            } else {
                Ok(value.clone())
            }
        }
        PyObjectPayload::Int(_) => {
            if let Some(pi) = parse_int {
                let s = PyObject::str_val(CompactString::from(value.py_to_string()));
                try_call_default(pi, &s).map(|r| r.unwrap_or_else(|| value.clone()))
            } else {
                Ok(value.clone())
            }
        }
        _ => Ok(value.clone()),
    }
}

/// json.dump(obj, fp, **kwargs) — serialize obj as JSON and write to fp.write()
fn json_dump(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "json.dump() missing required arguments: 'obj' and 'fp'",
        ));
    }
    // Reuse json_dumps for serialization: pass obj + remaining kwargs
    let mut dump_args = vec![args[0].clone()];
    if args.len() > 2 {
        dump_args.push(args[2].clone());
    }
    let json_str = json_dumps(&dump_args)?;
    // Call fp.write(json_str)
    let fp = &args[1];
    if let Some(write_fn) = fp.get_attr("write") {
        match &write_fn.payload {
            PyObjectPayload::NativeFunction(nf) => {
                (nf.func)(&[fp.clone(), json_str])?;
            }
            PyObjectPayload::NativeClosure(nc) => {
                (nc.func)(&[json_str])?;
            }
            _ => {} // user-defined write — best-effort
        }
    }
    Ok(PyObject::none())
}

/// json.load(fp) — read JSON from fp.read() and deserialize
fn json_load(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "json.load() missing required argument: 'fp'",
        ));
    }
    let fp = &args[0];
    if let Some(read_fn) = fp.get_attr("read") {
        let data = match &read_fn.payload {
            PyObjectPayload::NativeFunction(nf) => (nf.func)(&[fp.clone()])?,
            PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
            _ => return Err(PyException::type_error("fp.read() is not callable")),
        };
        let s = match &data.payload {
            PyObjectPayload::Str(s) => s.to_string(),
            _ => return Err(PyException::type_error("fp.read() must return a string")),
        };
        return parse_json_value(&s, &mut 0);
    }
    Err(PyException::attribute_error(
        "'fp' object has no attribute 'read'",
    ))
}

/// json.decoder submodule — exposes JSONDecoder and JSONDecodeError
pub fn create_json_decoder_module() -> PyObjectRef {
    let mut dec_ns = IndexMap::new();
    dec_ns.insert(
        CompactString::from("decode"),
        PyObject::native_closure("decode", |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "JSONDecoder.decode() missing argument",
                ));
            }
            let s = args[0].py_to_string();
            parse_json_value(&s, &mut 0)
        }),
    );
    dec_ns.insert(
        CompactString::from("raw_decode"),
        PyObject::native_closure("raw_decode", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("raw_decode() missing argument"));
            }
            let s = args[0].py_to_string();
            let mut pos = 0;
            let val = parse_json_value(&s, &mut pos)?;
            Ok(PyObject::tuple(vec![val, PyObject::int(pos as i64)]))
        }),
    );
    let json_decoder_cls = PyObject::class(CompactString::from("JSONDecoder"), vec![], dec_ns);
    let json_decode_error = PyObject::class(
        CompactString::from("JSONDecodeError"),
        vec![],
        IndexMap::new(),
    );

    make_module(
        "json.decoder",
        vec![
            ("JSONDecoder", json_decoder_cls),
            ("JSONDecodeError", json_decode_error),
        ],
    )
}

/// json.encoder submodule — exposes JSONEncoder
pub fn create_json_encoder_module() -> PyObjectRef {
    let mut enc_ns = IndexMap::new();
    enc_ns.insert(
        CompactString::from("encode"),
        PyObject::native_closure("encode", |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "JSONEncoder.encode() missing argument",
                ));
            }
            let s = py_to_json(&args[0])?;
            Ok(PyObject::str_val(CompactString::from(s)))
        }),
    );
    enc_ns.insert(
        CompactString::from("default"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            Err(PyException::type_error(format!(
                "Object of type {} is not JSON serializable",
                args[0].type_name()
            )))
        }),
    );
    let json_encoder_cls = PyObject::class(CompactString::from("JSONEncoder"), vec![], enc_ns);

    // ESCAPE_DCT — mapping of control characters to escape sequences
    let mut escape_dct = IndexMap::new();
    for i in 0u8..0x20 {
        let key = HashableKey::str_key(CompactString::from(String::from(i as char)));
        let val = PyObject::str_val(CompactString::from(format!("\\u{:04x}", i)));
        escape_dct.insert(key, val);
    }
    escape_dct.insert(
        HashableKey::str_key(CompactString::from("\\")),
        PyObject::str_val(CompactString::from("\\\\")),
    );
    escape_dct.insert(
        HashableKey::str_key(CompactString::from("\"")),
        PyObject::str_val(CompactString::from("\\\"")),
    );

    make_module(
        "json.encoder",
        vec![
            ("JSONEncoder", json_encoder_cls),
            ("ESCAPE_DCT", PyObject::dict(escape_dct)),
            ("INFINITY", PyObject::float(f64::INFINITY)),
        ],
    )
}
