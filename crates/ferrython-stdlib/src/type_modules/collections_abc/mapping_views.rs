use super::helpers::add_method;
use super::*;

pub(super) fn add_mapping_view_methods(
    mapping_view_cls: &PyObjectRef,
    keys_view_cls: &PyObjectRef,
    items_view_cls: &PyObjectRef,
    values_view_cls: &PyObjectRef,
) {
    let make_mapping_view = |cls: &PyObjectRef, kind: &'static str| {
        add_method(
            cls,
            "__init__",
            PyObject::native_closure(kind, move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("view requires mapping"));
                }
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    inst.attrs
                        .write()
                        .insert(CompactString::from("_mapping"), args[1].clone());
                }
                Ok(PyObject::none())
            }),
        );
        add_method(
            cls,
            "__len__",
            PyObject::native_closure(kind, move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("view requires self"));
                }
                let mapping = args[0]
                    .get_attr("_mapping")
                    .or_else(|| args[0].get_attr("mapping"))
                    .unwrap_or_else(PyObject::none);
                Ok(PyObject::int(mapping.py_len()? as i64))
            }),
        );
    };

    make_mapping_view(&mapping_view_cls, "MappingView.__init__");
    make_mapping_view(&keys_view_cls, "KeysView.__init__");
    make_mapping_view(&items_view_cls, "ItemsView.__init__");
    make_mapping_view(&values_view_cls, "ValuesView.__init__");

    add_method(
        &keys_view_cls,
        "__iter__",
        PyObject::native_closure("KeysView.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("KeysView.__iter__ requires self"));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            match &mapping.payload {
                PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => Ok(
                    PyObject::list(map.read().keys().map(|k| k.to_object()).collect()),
                ),
                PyObjectPayload::InstanceDict(attrs) => {
                    let keys = attrs
                        .read()
                        .keys()
                        .map(|k| PyObject::str_val(k.clone()))
                        .collect();
                    Ok(PyObject::list(keys))
                }
                _ => Ok(PyObject::list(mapping.to_list()?)),
            }
        }),
    );
    add_method(
        &keys_view_cls,
        "__contains__",
        PyObject::native_closure("KeysView.__contains__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            Ok(PyObject::bool_val(mapping.contains(&args[1])?))
        }),
    );
    add_method(
        &items_view_cls,
        "__iter__",
        PyObject::native_closure("ItemsView.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("ItemsView.__iter__ requires self"));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            if let PyObjectPayload::Dict(map) = &mapping.payload {
                let items: Vec<PyObjectRef> = map
                    .read()
                    .iter()
                    .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()]))
                    .collect();
                Ok(PyObject::list(items))
            } else {
                Ok(PyObject::list(mapping.to_list()?))
            }
        }),
    );
    add_method(
        &items_view_cls,
        "__contains__",
        PyObject::native_closure("ItemsView.__contains__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            if let PyObjectPayload::Dict(map) = &mapping.payload {
                let pair = args[1].to_list()?;
                if pair.len() != 2 {
                    return Ok(PyObject::bool_val(false));
                }
                let hk = pair[0].to_hashable_key()?;
                if let Some(v) = map.read().get(&hk) {
                    return Ok(PyObject::bool_val(
                        v.compare(&pair[1], CompareOp::Eq)
                            .map(|r| r.is_truthy())
                            .unwrap_or(false),
                    ));
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );
    add_method(
        &values_view_cls,
        "__iter__",
        PyObject::native_closure("ValuesView.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("ValuesView.__iter__ requires self"));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            match &mapping.payload {
                PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => {
                    Ok(PyObject::list(map.read().values().cloned().collect()))
                }
                PyObjectPayload::InstanceDict(attrs) => {
                    Ok(PyObject::list(attrs.read().values().cloned().collect()))
                }
                _ => Ok(PyObject::list(mapping.to_list()?)),
            }
        }),
    );
    add_method(
        &values_view_cls,
        "__contains__",
        PyObject::native_closure("ValuesView.__contains__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            Ok(PyObject::bool_val(mapping.to_list()?.iter().any(|v| {
                v.compare(&args[1], CompareOp::Eq)
                    .map(|r| r.is_truthy())
                    .unwrap_or(false)
            })))
        }),
    );
}
