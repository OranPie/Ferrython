//! `collections.deque` constructor.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

pub(crate) fn collections_deque(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let has_trailing_kwargs =
        !args.is_empty() && matches!(&args[args.len() - 1].payload, PyObjectPayload::Dict(_));
    let positional_count = if has_trailing_kwargs {
        args.len().saturating_sub(1)
    } else {
        args.len()
    };
    if positional_count > 2 {
        return Err(PyException::type_error(
            "deque() takes at most 2 positional arguments",
        ));
    }
    let kwargs_idx = if has_trailing_kwargs {
        args.len() - 1
    } else {
        args.len()
    };

    let items = if kwargs_idx == 0 || args.is_empty() {
        vec![]
    } else {
        args[0].to_list()?
    };

    let maxlen = if has_trailing_kwargs {
        if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
            let map = map.read();
            if let Some(v) = map.get(&HashableKey::str_key(CompactString::from("maxlen"))) {
                if matches!(&v.payload, PyObjectPayload::None) {
                    None
                } else {
                    let n = v.to_int()?;
                    if n < 0 {
                        return Err(PyException::value_error("maxlen must be non-negative"));
                    }
                    Some(n as usize)
                }
            } else {
                None
            }
        } else {
            None
        }
    } else if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        let n = args[1].to_int()?;
        if n < 0 {
            return Err(PyException::value_error("maxlen must be non-negative"));
        }
        Some(n as usize)
    } else {
        None
    };

    let items = if let Some(ml) = maxlen {
        if items.len() > ml {
            items[items.len() - ml..].to_vec()
        } else {
            items
        }
    } else {
        items
    };

    let deque_cls = PyObject::class(CompactString::from("deque"), vec![], IndexMap::new());
    let inst = PyObject::instance(deque_cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        let storage = PyObject::deque_storage(items);
        attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_data"), storage.clone());
        attrs.insert(CompactString::from("__builtin_value__"), storage);
        attrs.insert(
            CompactString::from("__maxlen__"),
            maxlen
                .map(|n| PyObject::int(n as i64))
                .unwrap_or_else(PyObject::none),
        );
    }
    Ok(inst)
}
