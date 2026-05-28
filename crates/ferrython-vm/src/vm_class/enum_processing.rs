//! Enum and Flag class post-processing.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

impl VirtualMachine {
    /// Process enum class: transform simple attributes into enum member instances.
    pub(crate) fn process_enum_class(
        &mut self,
        cls: &PyObjectRef,
        bases: &[PyObjectRef],
    ) -> PyResult<()> {
        // Check if any base has __enum__ marker
        let is_enum = bases.iter().any(|b| {
            if let Some(marker) = b.get_attr("__enum__") {
                marker.is_truthy()
            } else {
                false
            }
        });
        if !is_enum {
            return Ok(());
        }

        let cd = match &cls.payload {
            PyObjectPayload::Class(cd) => cd,
            _ => return Ok(()),
        };

        // Collect user-defined attributes (skip dunder and callables)
        let members: Vec<(CompactString, PyObjectRef)> = {
            let ns = cd.namespace.read();
            ns.iter()
                .filter(|(k, v)| {
                    !k.starts_with('_')
                        && !matches!(
                            &v.payload,
                            PyObjectPayload::Function(_)
                                | PyObjectPayload::NativeFunction(_)
                                | PyObjectPayload::BuiltinFunction(_)
                                | PyObjectPayload::Property(_)
                                | PyObjectPayload::NativeClosure(_)
                                | PyObjectPayload::StaticMethod(_)
                                | PyObjectPayload::ClassMethod(_)
                        )
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };

        // For each member, create an instance of the enum class with name and value.
        // Resolve auto() sentinels: ("__enum_auto__", _) → sequential int starting from 1.
        let mut ns = cd.namespace.write();
        let mut member_map = IndexMap::new();
        let mut auto_counter: i64 = 1;

        // Check if this is a Flag enum (auto() should generate powers of 2)
        let is_flag = bases.iter().any(|b| {
            b.get_attr("__flag__")
                .map(|m| m.is_truthy())
                .unwrap_or(false)
        });

        // Check if class has a custom __init__ (not inherited from Enum base)
        let has_custom_init = ns.get("__init__").is_some();

        let class_name = cd.name.clone();

        for (name, value) in &members {
            // Resolve auto() sentinel
            let resolved_value = if let PyObjectPayload::Tuple(items) = &value.payload {
                if items.len() == 2 {
                    if let PyObjectPayload::Str(s) = &items[0].payload {
                        if s.as_str() == "__enum_auto__" {
                            let v = PyObject::int(auto_counter);
                            if is_flag {
                                auto_counter *= 2; // powers of 2 for Flag
                            } else {
                                auto_counter += 1;
                            }
                            v
                        } else {
                            value.clone()
                        }
                    } else {
                        value.clone()
                    }
                } else {
                    value.clone()
                }
            } else {
                // Track max int value for auto() continuation
                if let Some(iv) = value.as_int() {
                    if is_flag {
                        if iv >= auto_counter {
                            auto_counter = (iv as u64).next_power_of_two() as i64 * 2;
                        }
                    } else {
                        if iv >= auto_counter {
                            auto_counter = iv + 1;
                        }
                    }
                }
                value.clone()
            };

            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
            attrs.insert(CompactString::from("value"), resolved_value.clone());
            attrs.insert(
                CompactString::from("_name_"),
                PyObject::str_val(name.clone()),
            );
            attrs.insert(CompactString::from("_value_"), resolved_value.clone());

            // Enum __repr__: "<ClassName.MemberName: value>" (CPython format)
            // Only set default if user didn't define custom __repr__ in the class body
            if ns.get("__repr__").is_none() {
                let val_repr = resolved_value.repr();
                let full_repr =
                    CompactString::from(format!("<{}.{}: {}>", class_name, name, val_repr));
                let repr_copy = full_repr;
                attrs.insert(
                    intern_or_new("__repr__"),
                    PyObject::native_closure("__repr__", move |_args| {
                        Ok(PyObject::str_val(repr_copy.clone()))
                    }),
                );
            }
            // __str__: use custom __str__ from class body if defined, else "ClassName.MemberName"
            if ns.get("__str__").is_none() {
                let str_val = CompactString::from(format!("{}.{}", class_name, name));
                attrs.insert(
                    intern_or_new("__str__"),
                    PyObject::native_closure("__str__", move |_args| {
                        Ok(PyObject::str_val(str_val.clone()))
                    }),
                );
            }

            // If custom __init__ exists and value is a tuple, unpack it and call __init__
            if has_custom_init {
                if let PyObjectPayload::Tuple(items) = &resolved_value.payload {
                    for (i, item) in items.iter().enumerate() {
                        // Store positional args as attributes (will be overwritten by __init__)
                        attrs.insert(CompactString::from(format!("__arg{}", i)), item.clone());
                    }
                }
            }

            let member = PyObject::instance_with_attrs(cls.clone(), attrs);

            // Call custom __init__ if present
            if has_custom_init {
                let init_fn = ns.get("__init__").cloned();
                if let Some(init) = init_fn {
                    let mut call_args = vec![member.clone()];
                    if let PyObjectPayload::Tuple(items) = &resolved_value.payload {
                        call_args.extend(items.iter().cloned());
                    } else {
                        call_args.push(resolved_value.clone());
                    }
                    // Drop ns write lock before calling VM methods
                    drop(ns);
                    self.call_object(init, call_args)?;
                    // Re-acquire lock
                    let cd2 = match &cls.payload {
                        PyObjectPayload::Class(cd) => cd,
                        _ => return Ok(()),
                    };
                    ns = cd2.namespace.write();
                }
            }

            ns.insert(name.clone(), member.clone());
            member_map.insert(name.clone(), member);
        }

        // Add __members__ dict
        let pairs: Vec<(PyObjectRef, PyObjectRef)> = member_map
            .iter()
            .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
            .collect();
        ns.insert(
            intern_or_new("__members__"),
            PyObject::dict_from_pairs(pairs),
        );

        // Mark as enum
        ns.insert(intern_or_new("__enum__"), PyObject::bool_val(true));

        // For Flag-based enums, add bitwise operations (__or__, __and__, __xor__, __invert__, __contains__, __int__, __bool__)
        let is_flag = bases.iter().any(|b| {
            b.get_attr("__flag__")
                .map(|m| m.is_truthy())
                .unwrap_or(false)
        });
        if is_flag {
            ns.insert(intern_or_new("__flag__"), PyObject::bool_val(true));

            // Helper: decompose a combined flag value into member names using __members__
            fn make_flag_instance(cls: &PyObjectRef, combined: i64) -> PyObjectRef {
                let class_name = if let PyObjectPayload::Class(cd) = &cls.payload {
                    cd.name.clone()
                } else {
                    CompactString::from("Flag")
                };

                // Decompose value into member names
                let mut names = Vec::new();
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    let ns = cd.namespace.read();
                    if let Some(members) = ns.get("__members__") {
                        if let PyObjectPayload::Dict(map) = &members.payload {
                            let members_map = map.read();
                            // Collect members in definition order (IndexMap preserves insertion order)
                            let member_list: Vec<(CompactString, i64)> = members_map
                                .iter()
                                .filter_map(|(k, v)| {
                                    if let HashableKey::Str(name) = k {
                                        v.get_attr("value")
                                            .and_then(|v| v.as_int())
                                            .map(|val| (name.to_compact_string(), val))
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            // Greedy decomposition: highest values first, but output in definition order
                            let mut sorted_by_val: Vec<(usize, &CompactString, i64)> = member_list
                                .iter()
                                .enumerate()
                                .map(|(i, (n, v))| (i, n, *v))
                                .collect();
                            sorted_by_val.sort_by(|a, b| b.2.cmp(&a.2));
                            let mut remaining = combined;
                            let mut matched_indices = Vec::new();
                            for (idx, _name, val) in &sorted_by_val {
                                if *val > 0 && remaining & val == *val {
                                    matched_indices.push(*idx);
                                    remaining &= !val;
                                }
                            }
                            // Sort by definition order
                            matched_indices.sort();
                            for idx in matched_indices {
                                names.push(member_list[idx].0.clone());
                            }
                        }
                    }
                }

                let name_str = if names.is_empty() {
                    CompactString::from("None")
                } else {
                    CompactString::from(
                        names
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("|"),
                    )
                };

                let repr_str =
                    CompactString::from(format!("<{}.{}: {}>", class_name, name_str, combined));
                let str_val = CompactString::from(format!("{}.{}", class_name, name_str));
                let repr_copy = repr_str.clone();
                let str_copy = str_val.clone();

                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("value"), PyObject::int(combined));
                attrs.insert(CompactString::from("_value_"), PyObject::int(combined));
                attrs.insert(
                    CompactString::from("name"),
                    PyObject::str_val(name_str.clone()),
                );
                attrs.insert(CompactString::from("_name_"), PyObject::str_val(name_str));
                attrs.insert(
                    intern_or_new("__repr__"),
                    PyObject::native_closure("__repr__", move |_args| {
                        Ok(PyObject::str_val(repr_copy.clone()))
                    }),
                );
                attrs.insert(
                    intern_or_new("__str__"),
                    PyObject::native_closure("__str__", move |_args| {
                        Ok(PyObject::str_val(str_copy.clone()))
                    }),
                );
                PyObject::instance_with_attrs(cls.clone(), attrs)
            }

            let cls_or = cls.clone();
            ns.insert(
                intern_or_new("__or__"),
                PyObject::native_closure("Flag.__or__", move |args: &[PyObjectRef]| {
                    if args.len() < 2 {
                        return Ok(PyObject::none());
                    }
                    let a_val = args[0]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    let b_val = args[1]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    Ok(make_flag_instance(&cls_or, a_val | b_val))
                }),
            );
            let cls_and = cls.clone();
            ns.insert(
                intern_or_new("__and__"),
                PyObject::native_closure("Flag.__and__", move |args: &[PyObjectRef]| {
                    if args.len() < 2 {
                        return Ok(PyObject::none());
                    }
                    let a_val = args[0]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    let b_val = args[1]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    Ok(make_flag_instance(&cls_and, a_val & b_val))
                }),
            );
            let cls_xor = cls.clone();
            ns.insert(
                intern_or_new("__xor__"),
                PyObject::native_closure("Flag.__xor__", move |args: &[PyObjectRef]| {
                    if args.len() < 2 {
                        return Ok(PyObject::none());
                    }
                    let a_val = args[0]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    let b_val = args[1]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    Ok(make_flag_instance(&cls_xor, a_val ^ b_val))
                }),
            );
            let cls_inv = cls.clone();
            ns.insert(
                intern_or_new("__invert__"),
                PyObject::native_closure("Flag.__invert__", move |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    let self_val = args[0]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    // Compute the bitmask of all defined member values
                    let mut all_bits: i64 = 0;
                    if let PyObjectPayload::Class(cd) = &cls_inv.payload {
                        let ns = cd.namespace.read();
                        if let Some(members) = ns.get("__members__") {
                            if let PyObjectPayload::Dict(map) = &members.payload {
                                for v in map.read().values() {
                                    if let Some(val) = v.get_attr("value").and_then(|v| v.as_int())
                                    {
                                        all_bits |= val;
                                    }
                                }
                            }
                        }
                    }
                    Ok(make_flag_instance(&cls_inv, all_bits & !self_val))
                }),
            );
            ns.insert(
                intern_or_new("__contains__"),
                PyObject::native_closure("Flag.__contains__", move |args: &[PyObjectRef]| {
                    if args.len() < 2 {
                        return Ok(PyObject::bool_val(false));
                    }
                    let self_val = args[0]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    let other_val = args[1]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    Ok(PyObject::bool_val(
                        self_val & other_val == other_val && other_val != 0,
                    ))
                }),
            );
            // __int__ — allow int() on flag members
            ns.insert(
                intern_or_new("__int__"),
                PyObject::native_function("Flag.__int__", |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::int(0));
                    }
                    let val = args[0]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    Ok(PyObject::int(val))
                }),
            );
            // __bool__ — non-zero flag is truthy
            ns.insert(
                intern_or_new("__bool__"),
                PyObject::native_function("Flag.__bool__", |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let val = args[0]
                        .get_attr("value")
                        .and_then(|v| v.as_int())
                        .unwrap_or(0);
                    Ok(PyObject::bool_val(val != 0))
                }),
            );
        }

        // Namespace was mutated (__enum__, __members__, __or__ etc.) after
        // ClassData::new() built the vtable.  Invalidate caches so lookups
        // pick up the new methods instead of stale base-class versions.
        drop(ns); // release namespace write guard first
        cd.invalidate_cache();

        Ok(())
    }
}
