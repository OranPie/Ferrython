use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    new_fx_hashkey_map, FxHashKeyMap, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_typeddict_builtin(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if args.is_empty() {
            return Ok(None);
        }

        let typename = args[0].py_to_string();
        let has_kwargs = args
            .last()
            .is_some_and(|arg| matches!(arg.payload, PyObjectPayload::Dict(_)));
        let positional_end = args.len() - usize::from(has_kwargs);
        if positional_end > 2 {
            return Err(PyException::type_error(
                "TypedDict takes at most two positional arguments",
            ));
        }

        let mut annotations = new_fx_hashkey_map();
        if positional_end == 2 {
            typed_dict_annotations_from_fields(&args[1], &mut annotations)?;
        }

        let mut total = true;
        if has_kwargs {
            if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
                for (key, value) in map.read().iter() {
                    let key_name = match key {
                        HashableKey::Str(s) => s.as_str(),
                        _ => "",
                    };
                    if key_name == "total" {
                        total = value.is_truthy();
                    } else {
                        annotations.insert(key.clone(), value.clone());
                    }
                }
            }
        }

        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__module__"),
            PyObject::str_val(CompactString::from("__main__")),
        );
        ns.insert(
            CompactString::from("__annotations__"),
            PyObject::dict(annotations),
        );
        ns.insert(CompactString::from("__total__"), PyObject::bool_val(total));
        ns.insert(
            CompactString::from("__typed_dict__"),
            PyObject::bool_val(true),
        );
        ns.insert(CompactString::from("__new__"), typed_dict_new());
        Ok(Some(PyObject::class(
            CompactString::from(typename.as_str()),
            vec![PyObject::builtin_type(CompactString::from("dict"))],
            ns,
        )))
    }
}

fn typed_dict_annotations_from_fields(
    fields: &PyObjectRef,
    annotations: &mut FxHashKeyMap,
) -> PyResult<()> {
    match &fields.payload {
        PyObjectPayload::Dict(map) => {
            annotations.extend(map.read().iter().map(|(k, v)| (k.clone(), v.clone())));
            Ok(())
        }
        PyObjectPayload::List(items) => {
            for item in items.read().iter() {
                typed_dict_insert_field_pair(item, annotations)?;
            }
            Ok(())
        }
        PyObjectPayload::Tuple(items) => {
            for item in items.iter() {
                typed_dict_insert_field_pair(item, annotations)?;
            }
            Ok(())
        }
        PyObjectPayload::None => Ok(()),
        _ => Err(PyException::type_error(
            "TypedDict fields must be a dict or list of pairs",
        )),
    }
}

fn typed_dict_insert_field_pair(
    item: &PyObjectRef,
    annotations: &mut FxHashKeyMap,
) -> PyResult<()> {
    let pair = item.to_list()?;
    if pair.len() != 2 {
        return Err(PyException::type_error(
            "TypedDict field entries must be name/type pairs",
        ));
    }
    let key = HashableKey::str_key(CompactString::from(pair[0].py_to_string()));
    annotations.insert(key, pair[1].clone());
    Ok(())
}

fn typed_dict_new() -> PyObjectRef {
    PyObject::native_closure("TypedDict.__new__", |args: &[PyObjectRef]| {
        let mut data = new_fx_hashkey_map();
        let positional = args
            .iter()
            .skip(1)
            .filter(|arg| !matches!(arg.payload, PyObjectPayload::Dict(_)))
            .cloned()
            .collect::<Vec<_>>();
        if let Some(first) = positional.first() {
            if let PyObjectPayload::Dict(map) = &first.payload {
                data.extend(map.read().iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(map) = &last.payload {
                data.extend(map.read().iter().map(|(k, v)| (k.clone(), v.clone())));
            }
        }
        Ok(PyObject::dict(data))
    })
}
