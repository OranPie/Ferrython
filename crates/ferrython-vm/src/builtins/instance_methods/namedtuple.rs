use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    new_fx_hashkey_map, CompareOp, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{hash_key_like_python, HashableKey};
use indexmap::IndexMap;
use std::rc::Rc;

pub(crate) fn call_namedtuple_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "_asdict" => {
            if let Some(fields) = inst.class.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    let mut map = IndexMap::new();
                    if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                        if let PyObjectPayload::Tuple(items) = &tup.payload {
                            for (field, val) in field_names.iter().zip(items.iter()) {
                                let name = field.py_to_string();
                                map.insert(
                                    HashableKey::str_key(CompactString::from(name.as_str())),
                                    val.clone(),
                                );
                            }
                            return Ok(PyObject::dict(map));
                        }
                    }
                    let attrs = inst.attrs.read();
                    for field in field_names.iter() {
                        let name = field.py_to_string();
                        let val = attrs
                            .get(name.as_str())
                            .cloned()
                            .unwrap_or_else(PyObject::none);
                        map.insert(
                            HashableKey::str_key(CompactString::from(name.as_str())),
                            val,
                        );
                    }
                    return Ok(PyObject::dict(map));
                }
            }
            Ok(PyObject::dict(new_fx_hashkey_map()))
        }
        "_replace" => {
            // _replace(**kwargs) — create a new instance with some fields replaced
            // In our dispatch, kwargs are passed as a trailing dict argument
            let kwargs_dict = if !args.is_empty() {
                if let PyObjectPayload::Dict(map) = &args[0].payload {
                    Some(map.read().clone())
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(fields) = inst.class.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    let attrs = inst.attrs.read();
                    let mut new_values: Vec<PyObjectRef> = Vec::new();
                    for field in field_names.iter() {
                        let name = field.py_to_string();
                        let hk = HashableKey::str_key(CompactString::from(name.as_str()));
                        let val = if let Some(ref kw) = kwargs_dict {
                            kw.get(&hk).cloned().unwrap_or_else(|| {
                                attrs
                                    .get(name.as_str())
                                    .cloned()
                                    .unwrap_or_else(PyObject::none)
                            })
                        } else {
                            attrs
                                .get(name.as_str())
                                .cloned()
                                .unwrap_or_else(PyObject::none)
                        };
                        new_values.push(val);
                    }
                    drop(attrs);
                    // Construct a new namedtuple instance
                    let new_inst = PyObject::instance(inst.class.clone());
                    if let PyObjectPayload::Instance(ref new_data) = new_inst.payload {
                        let mut new_attrs = new_data.attrs.write();
                        new_attrs
                            .insert(CompactString::from("_tuple"), PyObject::tuple(new_values));
                    }
                    return Ok(new_inst);
                }
            }
            Ok(PyObject::none())
        }
        "_make" => {
            // _make(iterable) — create instance from iterable
            if args.is_empty() {
                return Err(PyException::type_error(
                    "_make() requires an iterable argument",
                ));
            }
            let items = args[0].to_list()?;
            if let Some(fields) = inst.class.get_attr("_fields") {
                if let PyObjectPayload::Tuple(_field_names) = &fields.payload {
                    let new_inst = PyObject::instance(inst.class.clone());
                    if let PyObjectPayload::Instance(ref new_data) = new_inst.payload {
                        let mut new_attrs = new_data.attrs.write();
                        new_attrs.insert(CompactString::from("_tuple"), PyObject::tuple(items));
                    }
                    return Ok(new_inst);
                }
            }
            Ok(PyObject::none())
        }
        "__len__" => {
            if let Some(tup) = inst.attrs.read().get("_tuple") {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    return Ok(PyObject::int(items.len() as i64));
                }
            }
            Ok(PyObject::int(0))
        }
        "__iter__" => {
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                        PyCell::new(ferrython_core::object::IteratorData::Tuple {
                            items: (**items).clone(),
                            index: 0,
                        }),
                    ))));
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(ferrython_core::object::IteratorData::Tuple {
                    items: vec![],
                    index: 0,
                }),
            ))))
        }
        "__repr__" | "__str__" => {
            let typename = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                cd.name.to_string()
            } else {
                "namedtuple".to_string()
            };
            if let Some(fields) = inst.class.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                        if let PyObjectPayload::Tuple(items) = &tup.payload {
                            let parts: Vec<String> = field_names
                                .iter()
                                .zip(items.iter())
                                .map(|(f, val)| {
                                    let name = f.py_to_string();
                                    format!("{}={}", name, val.py_to_string())
                                })
                                .collect();
                            return Ok(PyObject::str_val(CompactString::from(format!(
                                "{}({})",
                                typename,
                                parts.join(", ")
                            ))));
                        }
                    }
                    let attrs = inst.attrs.read();
                    let parts: Vec<String> = field_names
                        .iter()
                        .map(|f| {
                            let name = f.py_to_string();
                            let val = attrs
                                .get(name.as_str())
                                .cloned()
                                .unwrap_or_else(PyObject::none);
                            format!("{}={}", name, val.py_to_string())
                        })
                        .collect();
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "{}({})",
                        typename,
                        parts.join(", ")
                    ))));
                }
            }
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}()",
                typename
            ))))
        }
        "__eq__" => {
            // Compare namedtuple instances by their _tuple values
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            let other = &args[0];
            let self_tuple = inst.attrs.read().get("_tuple").cloned();
            let other_tuple = other.get_attr("_tuple");
            if let (Some(st), Some(ot)) = (self_tuple, other_tuple) {
                if let (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) =
                    (&st.payload, &ot.payload)
                {
                    if a.len() != b.len() {
                        return Ok(PyObject::bool_val(false));
                    }
                    for (av, bv) in a.iter().zip(b.iter()) {
                        if !av
                            .compare(bv, ferrython_core::object::CompareOp::Eq)?
                            .is_truthy()
                        {
                            return Ok(PyObject::bool_val(false));
                        }
                    }
                    return Ok(PyObject::bool_val(true));
                }
            }
            Ok(PyObject::bool_val(false))
        }
        "__hash__" => {
            // Hash the stored tuple payload.
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    let mut h: u64 = 0x345678;
                    let mult: u64 = 1_000_003;
                    for item in items.iter() {
                        h = h.wrapping_mul(mult)
                            ^ hash_key_like_python(&item.to_hashable_key()?) as u64;
                    }
                    return Ok(PyObject::int(h as i64));
                }
            }
            Ok(PyObject::int(0))
        }
        "__contains__" => {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    return Ok(PyObject::bool_val(items.iter().any(|x| {
                        x.compare(&args[0], CompareOp::Eq)
                            .map(|o| o.is_truthy())
                            .unwrap_or(false)
                    })));
                }
            }
            Ok(PyObject::bool_val(false))
        }
        "__getitem__" => {
            if args.is_empty() {
                return Err(PyException::type_error("__getitem__ requires an argument"));
            }
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                return tup.get_item(&args[0]);
            }
            Err(PyException::index_error("index out of range"))
        }
        _ => Err(PyException::attribute_error(format!(
            "namedtuple has no attribute '{}'",
            method
        ))),
    }
}
