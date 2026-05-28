use super::helpers::add_method;
use super::*;

fn make_set_items(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    obj.to_list()
}

pub(super) fn add_set_methods(set_cls: &PyObjectRef, mutable_set_cls: &PyObjectRef) {
    let set_cls_for_compare = set_cls.clone();
    let mutable_set_cls_for_compare = mutable_set_cls.clone();

    let make_set_like = |cls: &PyObjectRef| {
        let op_impl = |name: &'static str, reflected: bool| {
            let set_cls_for_compare = set_cls_for_compare.clone();
            let mutable_set_cls_for_compare = mutable_set_cls_for_compare.clone();
            PyObject::native_closure(name, move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Ok(PyObject::not_implemented());
                }
                let (left, right) = if reflected {
                    (&args[1], &args[0])
                } else {
                    (&args[0], &args[1])
                };
                if matches!(name, "__le__" | "__lt__" | "__ge__" | "__gt__")
                    && (!is_set_like_for_comparison(
                        left,
                        &set_cls_for_compare,
                        &mutable_set_cls_for_compare,
                    ) || !is_set_like_for_comparison(
                        right,
                        &set_cls_for_compare,
                        &mutable_set_cls_for_compare,
                    ))
                {
                    return Ok(PyObject::not_implemented());
                }
                let left_items = match make_set_items(left) {
                    Ok(items) => items,
                    Err(_) => return Ok(PyObject::not_implemented()),
                };
                let right_items = match make_set_items(right) {
                    Ok(items) => items,
                    Err(_) => return Ok(PyObject::not_implemented()),
                };
                let right_keys: std::collections::HashSet<_> = right_items
                    .iter()
                    .filter_map(|x| x.to_hashable_key().ok())
                    .collect();
                let left_keys: std::collections::HashSet<_> = left_items
                    .iter()
                    .filter_map(|x| x.to_hashable_key().ok())
                    .collect();
                match name {
                    "__le__" => Ok(PyObject::bool_val(left_keys.is_subset(&right_keys))),
                    "__lt__" => Ok(PyObject::bool_val(
                        left_keys.len() < right_keys.len() && left_keys.is_subset(&right_keys),
                    )),
                    "__ge__" => Ok(PyObject::bool_val(left_keys.is_superset(&right_keys))),
                    "__gt__" => Ok(PyObject::bool_val(
                        left_keys.len() > right_keys.len() && left_keys.is_superset(&right_keys),
                    )),
                    "__and__" | "__rand__" => {
                        if reflected
                            && left_items.is_empty()
                            && !matches!(
                                &left.payload,
                                PyObjectPayload::Set(_)
                                    | PyObjectPayload::FrozenSet(_)
                                    | PyObjectPayload::DictKeys { .. }
                                    | PyObjectPayload::DictItems { .. }
                            )
                        {
                            return Ok(PyObject::not_implemented());
                        }
                        let mut result = Vec::new();
                        for item in &left_items {
                            if let Ok(hk) = item.to_hashable_key() {
                                if right_keys.contains(&hk) {
                                    result.push(item.clone());
                                }
                            }
                        }
                        let flat: FxHashKeyFlatMap = result
                            .into_iter()
                            .filter_map(|item| item.to_hashable_key().ok().map(|hk| (hk, item)))
                            .collect();
                        Ok(PyObject::set_from_flatmap(flat))
                    }
                    "__or__" | "__ror__" => {
                        let mut result: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
                        for item in left_items.iter().chain(right_items.iter()) {
                            if let Ok(hk) = item.to_hashable_key() {
                                result.entry(hk).or_insert_with(|| item.clone());
                            }
                        }
                        let flat: FxHashKeyFlatMap = result.into_iter().collect();
                        Ok(PyObject::set_from_flatmap(flat))
                    }
                    "__sub__" | "__rsub__" => {
                        let mut result = Vec::new();
                        for item in &left_items {
                            if let Ok(hk) = item.to_hashable_key() {
                                if !right_keys.contains(&hk) {
                                    result.push(item.clone());
                                }
                            }
                        }
                        let flat: FxHashKeyFlatMap = result
                            .into_iter()
                            .filter_map(|item| item.to_hashable_key().ok().map(|hk| (hk, item)))
                            .collect();
                        Ok(PyObject::set_from_flatmap(flat))
                    }
                    "__xor__" | "__rxor__" => {
                        let mut result: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
                        for item in &left_items {
                            if let Ok(hk) = item.to_hashable_key() {
                                if !right_keys.contains(&hk) {
                                    result.insert(hk, item.clone());
                                }
                            }
                        }
                        for item in &right_items {
                            if let Ok(hk) = item.to_hashable_key() {
                                if !left_keys.contains(&hk) {
                                    result.insert(hk, item.clone());
                                }
                            }
                        }
                        let flat: FxHashKeyFlatMap = result.into_iter().collect();
                        Ok(PyObject::set_from_flatmap(flat))
                    }
                    _ => Ok(PyObject::not_implemented()),
                }
            })
        };
        add_method(cls, "__le__", op_impl("__le__", false));
        add_method(cls, "__lt__", op_impl("__lt__", false));
        add_method(cls, "__ge__", op_impl("__ge__", false));
        add_method(cls, "__gt__", op_impl("__gt__", false));
        add_method(cls, "__and__", op_impl("__and__", false));
        add_method(cls, "__rand__", op_impl("__and__", true));
        add_method(cls, "__or__", op_impl("__or__", false));
        add_method(cls, "__ror__", op_impl("__or__", true));
        add_method(cls, "__sub__", op_impl("__sub__", false));
        add_method(cls, "__rsub__", op_impl("__sub__", true));
        add_method(cls, "__xor__", op_impl("__xor__", false));
        add_method(cls, "__rxor__", op_impl("__xor__", true));
        add_method(
            cls,
            "isdisjoint",
            PyObject::native_closure("Set.isdisjoint", move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("isdisjoint() requires 1 argument"));
                }
                let left_items = make_set_items(&args[0])?;
                let right_items = match make_set_items(&args[1]) {
                    Ok(items) => items,
                    Err(_) => return Ok(PyObject::not_implemented()),
                };
                let left_keys: std::collections::HashSet<_> = left_items
                    .iter()
                    .filter_map(|x| x.to_hashable_key().ok())
                    .collect();
                let disjoint = right_items
                    .iter()
                    .filter_map(|x| x.to_hashable_key().ok())
                    .all(|hk| !left_keys.contains(&hk));
                Ok(PyObject::bool_val(disjoint))
            }),
        );
    };
    make_set_like(&set_cls);
    make_set_like(&mutable_set_cls);
}
