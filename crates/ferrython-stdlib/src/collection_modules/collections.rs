use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use super::chainmap::make_chainmap_class;
use super::counter::{
    collections_most_common, count_elements, counter_clear, counter_copy, counter_elements,
    counter_subtract, counter_total, counter_update, make_counter_class, make_defaultdict_class,
};
use super::deque::collections_deque;
use super::namedtuple::{
    collections_namedtuple, namedtuple_field_class, namedtuple_rebuild_class,
    namedtuple_rebuild_field, namedtuple_rebuild_instance,
};
use super::user_types::{make_user_dict_class, make_user_list_class, make_user_string_class};

pub fn create_collections_module() -> PyObjectRef {
    let abc_module = crate::type_modules::create_collections_abc_module();

    // Deprecated aliases (Python 3.3-3.9 compat — removed in 3.10 but many packages still use them)
    let get_abc_attr =
        |name: &str| -> PyObjectRef { abc_module.get_attr(name).unwrap_or(PyObject::none()) };
    let mapping = get_abc_attr("Mapping");
    let mutable_mapping = get_abc_attr("MutableMapping");
    let sequence = get_abc_attr("Sequence");
    let mutable_sequence = get_abc_attr("MutableSequence");
    let set_abc = get_abc_attr("Set");
    let mutable_set = get_abc_attr("MutableSet");
    let iterable = get_abc_attr("Iterable");
    let iterator = get_abc_attr("Iterator");
    let callable = get_abc_attr("Callable");
    let sized = get_abc_attr("Sized");
    let container = get_abc_attr("Container");
    let hashable = get_abc_attr("Hashable");
    let namedtuple_field = namedtuple_field_class();
    let namedtuple_rebuild_field_fn =
        PyObject::native_function("_namedtuple_field_rebuild", namedtuple_rebuild_field);
    let namedtuple_rebuild_class_fn =
        PyObject::native_function("_namedtuple_rebuild_class", namedtuple_rebuild_class);
    let namedtuple_rebuild_instance_fn =
        PyObject::native_function("_namedtuple_rebuild_instance", namedtuple_rebuild_instance);

    make_module(
        "collections",
        vec![
            ("abc", abc_module),
            (
                "OrderedDict",
                PyObject::native_function("collections.OrderedDict", collections_ordered_dict),
            ),
            ("defaultdict", make_defaultdict_class()),
            ("Counter", make_counter_class()),
            ("namedtuple", make_builtin(collections_namedtuple)),
            ("namedtuple_field", namedtuple_field),
            ("_namedtuple_field_rebuild", namedtuple_rebuild_field_fn),
            ("_namedtuple_rebuild_class", namedtuple_rebuild_class_fn),
            (
                "_namedtuple_rebuild_instance",
                namedtuple_rebuild_instance_fn,
            ),
            (
                "deque",
                PyObject::native_function("collections.deque", collections_deque),
            ),
            ("most_common", make_builtin(collections_most_common)),
            ("ChainMap", make_chainmap_class()),
            ("UserDict", make_user_dict_class()),
            ("UserList", make_user_list_class()),
            ("UserString", make_user_string_class()),
            // Deprecated ABC aliases for compat
            ("Mapping", mapping),
            ("MutableMapping", mutable_mapping),
            ("Sequence", sequence),
            ("MutableSequence", mutable_sequence),
            ("Set", set_abc),
            ("MutableSet", mutable_set),
            ("Iterable", iterable),
            ("Iterator", iterator),
            ("Callable", callable),
            ("Sized", sized),
            ("Container", container),
            ("Hashable", hashable),
            // Counter helper functions
            ("counter_elements", make_builtin(counter_elements)),
            ("counter_update", make_builtin(counter_update)),
            ("counter_subtract", make_builtin(counter_subtract)),
            ("counter_total", make_builtin(counter_total)),
            ("counter_copy", make_builtin(counter_copy)),
            ("counter_clear", make_builtin(counter_clear)),
            // _count_elements(mapping, iterable) — C accelerator for Counter
            ("_count_elements", make_builtin(count_elements)),
        ],
    )
}

fn collections_ordered_dict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // OrderedDict — same as dict but __eq__ compares order when both are OrderedDict
    let marker_key = HashableKey::str_key(CompactString::from("__ordered_dict__"));
    let mut pairs = Vec::new();
    if !args.is_empty() {
        let items = args[0].to_list()?;
        for item in items {
            if let PyObjectPayload::Tuple(t) = &item.payload {
                if t.len() == 2 {
                    pairs.push((t[0].clone(), t[1].clone()));
                }
            }
        }
    }
    let mut map = IndexMap::new();
    map.insert(marker_key, PyObject::bool_val(true));
    for (k, v) in pairs {
        let hk = k.to_hashable_key()?;
        map.insert(hk, v);
    }
    let od = PyObject::dict(map);

    // Install move_to_end method
    if let PyObjectPayload::Dict(dict_arc) = &od.payload {
        let d = dict_arc.clone();
        let move_fn = PyObject::native_closure("OrderedDict.move_to_end", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "move_to_end() requires a key argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let last = if args.len() > 1 {
                args[1].is_truthy()
            } else {
                true
            };
            let mut map = d.write();
            if let Some(idx) = map.get_index_of(&key) {
                let target = if last { map.len() - 1 } else { 0 };
                map.move_index(idx, target);
                Ok(PyObject::none())
            } else {
                Err(PyException::key_error(format!("{:?}", args[0])))
            }
        });
        // Store move_to_end as a hidden dict key since dicts don't have instance attrs
        dict_arc.write().insert(
            HashableKey::str_key(CompactString::from("__move_to_end_fn__")),
            move_fn,
        );
    }

    Ok(od)
}
