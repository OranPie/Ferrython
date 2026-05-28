//! NamedTuple class post-processing.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

impl VirtualMachine {
    /// Process typing.NamedTuple class syntax: add __namedtuple__ marker and _fields from annotations.
    pub(super) fn process_namedtuple_class(&mut self, cls: &PyObjectRef, bases: &[PyObjectRef]) {
        let is_namedtuple = bases.iter().any(|b| {
            if let PyObjectPayload::BuiltinType(name) = &b.payload {
                name.as_str() == "NamedTuple"
            } else {
                false
            }
        });
        if !is_namedtuple {
            return;
        }

        let cd = match &cls.payload {
            PyObjectPayload::Class(cd) => cd,
            _ => return,
        };

        // Extract field names from __annotations__ and defaults from namespace
        let (field_names, defaults): (Vec<CompactString>, Vec<(CompactString, PyObjectRef)>) = {
            let ns = cd.namespace.read();
            let names: Vec<CompactString> = if let Some(ann) = ns.get("__annotations__") {
                if let PyObjectPayload::Dict(d) = &ann.payload {
                    let d = d.read();
                    d.keys()
                        .map(|k| {
                            if let HashableKey::Str(s) = k {
                                s.to_compact_string()
                            } else {
                                CompactString::from(k.to_object().py_to_string())
                            }
                        })
                        .collect()
                } else {
                    vec![]
                }
            } else {
                vec![]
            };
            // Collect defaults: field names that have a value in the namespace
            let defs: Vec<(CompactString, PyObjectRef)> = names
                .iter()
                .filter_map(|name| ns.get(name.as_str()).map(|v| (name.clone(), v.clone())))
                .collect();
            (names, defs)
        };

        let fields_tuple = PyObject::tuple(
            field_names
                .iter()
                .map(|n| PyObject::str_val(n.clone()))
                .collect(),
        );

        // Store _field_defaults as a dict
        let mut defaults_map = IndexMap::new();
        for (name, val) in &defaults {
            defaults_map.insert(HashableKey::str_key(name.clone()), val.clone());
        }

        let mut ns = cd.namespace.write();
        ns.insert(intern_or_new("__namedtuple__"), PyObject::bool_val(true));
        ns.insert(CompactString::from("_fields"), fields_tuple);
        ns.insert(
            CompactString::from("_field_defaults"),
            PyObject::dict(defaults_map),
        );

        // Generate __init__ that stores positional args as named attributes
        let field_names_for_init = field_names.clone();
        let defaults_for_init: Vec<(CompactString, PyObjectRef)> = defaults.clone();
        ns.insert(
            CompactString::from("__init__"),
            PyObject::native_closure("__init__", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("__init__ missing self"));
                }
                let instance = &args[0];
                let pos_args = &args[1..];
                // Build a map of default values
                let def_map: std::collections::HashMap<&str, &PyObjectRef> = defaults_for_init
                    .iter()
                    .map(|(k, v)| (k.as_str(), v))
                    .collect();
                if let PyObjectPayload::Instance(inst) = &instance.payload {
                    let mut attrs = inst.attrs.write();
                    for (i, field) in field_names_for_init.iter().enumerate() {
                        let val = if i < pos_args.len() {
                            pos_args[i].clone()
                        } else if let Some(def) = def_map.get(field.as_str()) {
                            (*def).clone()
                        } else {
                            return Err(PyException::type_error(format!(
                                "__init__() missing required argument: '{}'",
                                field
                            )));
                        };
                        attrs.insert(field.clone(), val);
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // Generate __getitem__ for indexing by position
        let field_names_for_getitem = field_names.clone();
        ns.insert(
            CompactString::from("__getitem__"),
            PyObject::native_closure("__getitem__", move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__getitem__ missing argument"));
                }
                let instance = &args[0];
                let idx = &args[1];
                if let Some(i) = idx.as_int() {
                    let i = if i < 0 {
                        i + field_names_for_getitem.len() as i64
                    } else {
                        i
                    } as usize;
                    if i < field_names_for_getitem.len() {
                        if let Some(v) = instance.get_attr(&field_names_for_getitem[i]) {
                            return Ok(v);
                        }
                    }
                    return Err(PyException::index_error("tuple index out of range"));
                }
                Err(PyException::type_error("tuple indices must be integers"))
            }),
        );

        // Generate __len__
        let n_fields = field_names.len();
        ns.insert(
            CompactString::from("__len__"),
            PyObject::native_closure("__len__", move |_args: &[PyObjectRef]| {
                Ok(PyObject::int(n_fields as i64))
            }),
        );

        // Generate __iter__ for unpacking
        let field_names_for_iter = field_names.clone();
        ns.insert(
            CompactString::from("__iter__"),
            PyObject::native_closure("__iter__", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("__iter__ missing self"));
                }
                let instance = &args[0];
                let vals: Vec<PyObjectRef> = field_names_for_iter
                    .iter()
                    .map(|f| instance.get_attr(f).unwrap_or_else(PyObject::none))
                    .collect();
                Ok(PyObject::tuple(vals))
            }),
        );

        // Generate __repr__
        let class_name = cd.name.clone();
        let field_names_for_repr = field_names.clone();
        ns.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("__repr__", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("()")));
                }
                let instance = &args[0];
                let parts: Vec<String> = field_names_for_repr
                    .iter()
                    .map(|f| {
                        let val = instance
                            .get_attr(f)
                            .map(|v| v.repr())
                            .unwrap_or_else(|| "None".to_string());
                        format!("{}={}", f, val)
                    })
                    .collect();
                Ok(PyObject::str_val(CompactString::from(format!(
                    "{}({})",
                    class_name,
                    parts.join(", ")
                ))))
            }),
        );

        // Invalidate stale vtable/cache
        drop(ns);
        cd.invalidate_cache();
    }
}
