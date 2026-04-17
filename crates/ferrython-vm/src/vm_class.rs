//! Class building — __build_class__, MRO computation, enum processing.

use crate::frame::{CellRef, Frame, ScopeKind};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{ new_fx_hashkey_map, PyCell, 
    ClassData, FxAttrMap, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::Cell;
use rustc_hash::FxHashMap;
use std::rc::Rc;

impl VirtualMachine {
    /// Extract inherited metaclass from bases: if any base has a custom metaclass, return it.
    fn inherited_metaclass(bases: &[PyObjectRef]) -> Option<PyObjectRef> {
        for base in bases {
            if let PyObjectPayload::Class(cd) = &base.payload {
                if let Some(meta) = &cd.metaclass {
                    return Some(meta.clone());
                }
            }
        }
        None
    }

    /// PEP 560: resolve __mro_entries__ for generic aliases in bases.
    /// e.g. `class Foo(dict[str, int])` → bases become `[dict]`
    fn resolve_mro_entries(raw_bases: &[PyObjectRef]) -> Vec<PyObjectRef> {
        let mut resolved = Vec::with_capacity(raw_bases.len());
        for base in raw_bases {
            match &base.payload {
                PyObjectPayload::Class(_) | PyObjectPayload::BuiltinType(_) => {
                    // Already a real class, keep as-is
                    resolved.push(base.clone());
                }
                PyObjectPayload::Instance(inst) => {
                    // Check for GenericAlias (__origin__ attribute)
                    if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        if cd.name.contains("GenericAlias") || cd.name.contains("_GenericAlias")
                            || cd.name.contains("_SpecialForm")
                        {
                            // Use __origin__ as the real base class
                            if let Some(origin) = inst.attrs.read().get("__origin__").cloned() {
                                resolved.push(origin);
                                continue;
                            }
                        }
                    }
                    // Not a generic alias — might be a regular class used as base
                    resolved.push(base.clone());
                }
                _ => {
                    resolved.push(base.clone());
                }
            }
        }
        // CPython typing behavior: if Generic appears as a base and another base
        // already inherits from Generic, remove Generic (it's redundant).
        Self::deduplicate_generic_bases(&mut resolved);
        resolved
    }

    /// Remove `Generic` from bases if another base already has it in its MRO.
    fn deduplicate_generic_bases(bases: &mut Vec<PyObjectRef>) {
        if bases.len() < 2 { return; }
        // Find which bases are "Generic" (the typing.Generic class)
        let generic_indices: Vec<usize> = bases.iter().enumerate()
            .filter_map(|(i, b)| {
                if let PyObjectPayload::Class(cd) = &b.payload {
                    if cd.name == "Generic" { return Some(i); }
                }
                None
            })
            .collect();
        if generic_indices.is_empty() { return; }
        // Check if any OTHER base already has Generic in its MRO
        let other_has_generic = bases.iter().enumerate().any(|(i, b)| {
            if generic_indices.contains(&i) { return false; }
            if let PyObjectPayload::Class(cd) = &b.payload {
                cd.mro.iter().any(|m| {
                    if let PyObjectPayload::Class(mc) = &m.payload {
                        mc.name == "Generic"
                    } else { false }
                })
            } else { false }
        });
        if other_has_generic {
            // Remove Generic bases (iterate in reverse to preserve indices)
            for &idx in generic_indices.iter().rev() {
                bases.remove(idx);
            }
        }
    }

    pub(crate) fn build_class(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "__build_class__ requires at least 2 arguments"));
        }
        let bases: Vec<PyObjectRef> = Self::resolve_mro_entries(&args[2..]);

        // If any base has a custom metaclass, delegate to the kw path
        if let Some(meta) = Self::inherited_metaclass(&bases) {
            let kwargs = vec![(CompactString::from("metaclass"), meta)];
            return self.build_class_kw(args, kwargs);
        }

        let body_func = args[0].clone();
        let class_name = match &args[1].payload {
            PyObjectPayload::Str(s) => (**s).clone(),
            _ => CompactString::from(args[1].py_to_string()),
        };

        let namespace = match &body_func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = Rc::clone(&pyfunc.code);
                let globals = pyfunc.globals.clone();
                let cc = pyfunc.constant_cache.clone();
                let mut frame = Frame::new_from_pool(code, globals, self.builtins.clone(), cc, &mut self.frame_pool);
                frame.scope_kind = ScopeKind::Class;
                // Wire up closure cells from the captured function
                let n_cell = frame.code.cellvars.len();
                for (i, cell) in pyfunc.closure.iter().enumerate() {
                    let free_idx = n_cell + i;
                    if free_idx < frame.cells.len() {
                        frame.cells[free_idx] = cell.clone();
                    }
                }
                self.call_stack.push(frame);
                let _ = self.run_frame();
                let frame = self.call_stack.pop().unwrap();
                let cellvar_names: Vec<CompactString> = frame.code.cellvars.clone();
                let cells = frame.cells.clone();
                (frame.local_names.map(|b| *b).unwrap_or_default(), Some((cellvar_names, cells)))
            }
            _ => (FxAttrMap::default(), None),
        };
        let (namespace, class_cell_info) = namespace;

        // Build MRO: [self_class, ...linearized_parents, object]
        // Simple C3-like: for single inheritance just chain; for multiple use bases order
        let mro = Self::compute_mro(&bases)?;
        let cls = PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData::new(
            class_name, bases.clone(), namespace, mro, None,
        ))));

        // Populate __class__ cell so methods can access it via super() (PEP 3135)
        if let Some((ref cellvar_names, ref cells)) = class_cell_info {
            Self::patch_class_cell(cellvar_names, cells, &cls);
        }

        // Call __init_subclass__ on the first base class that defines it (PEP 487)
        // CPython calls it once via super().__init_subclass__ in type.__init__
        if let Some(base) = bases.first() {
            if let Some(init_sub) = base.get_attr("__init_subclass__") {
                // Re-bind to the NEW class (cls) so `cls` is the subclass
                let bound = if matches!(&init_sub.payload, PyObjectPayload::BoundMethod { .. }) {
                    if let PyObjectPayload::BoundMethod { method, .. } = &init_sub.payload {
                        PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: cls.clone(),
                                method: method.clone(),
                            }
                        })
                    } else {
                        init_sub
                    }
                } else {
                    PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: cls.clone(),
                            method: init_sub,
                        }
                    })
                };
                self.call_object(bound, vec![])?;
            }
        }

        // Call __set_name__ on descriptors in the class namespace (PEP 487)
        self.call_set_name_on_descriptors(&cls)?;

        // NamedTuple metaclass behavior: add __namedtuple__ marker and _fields
        self.process_namedtuple_class(&cls, &bases);

        // Enum metaclass behavior: transform class attributes into enum members
        self.process_enum_class(&cls, &bases)?;

        // Register as subclass of each base (for type.__subclasses__())
        for base in &bases {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                bcd.subclasses.write().push(PyObjectRef::downgrade(&cls));
            }
        }

        Ok(cls)
    }

    /// Process typing.NamedTuple class syntax: add __namedtuple__ marker and _fields from annotations.
    fn process_namedtuple_class(&mut self, cls: &PyObjectRef, bases: &[PyObjectRef]) {
        let is_namedtuple = bases.iter().any(|b| {
            if let PyObjectPayload::BuiltinType(name) = &b.payload {
                name.as_str() == "NamedTuple"
            } else {
                false
            }
        });
        if !is_namedtuple { return; }

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
                    d.keys().map(|k| {
                        if let HashableKey::Str(s) = k {
                            s.clone()
                        } else {
                            CompactString::from(k.to_object().py_to_string())
                        }
                    }).collect()
                } else { vec![] }
            } else { vec![] };
            // Collect defaults: field names that have a value in the namespace
            let defs: Vec<(CompactString, PyObjectRef)> = names.iter()
                .filter_map(|name| {
                    ns.get(name.as_str()).map(|v| (name.clone(), v.clone()))
                })
                .collect();
            (names, defs)
        };

        let fields_tuple = PyObject::tuple(
            field_names.iter().map(|n| PyObject::str_val(n.clone())).collect()
        );

        // Store _field_defaults as a dict
        let mut defaults_map = IndexMap::new();
        for (name, val) in &defaults {
            defaults_map.insert(
                HashableKey::str_key(name.clone()),
                val.clone(),
            );
        }

        let mut ns = cd.namespace.write();
        ns.insert(intern_or_new("__namedtuple__"), PyObject::bool_val(true));
        ns.insert(CompactString::from("_fields"), fields_tuple);
        ns.insert(CompactString::from("_field_defaults"), PyObject::dict(defaults_map));

        // Generate __init__ that stores positional args as named attributes
        let field_names_for_init = field_names.clone();
        let defaults_for_init: Vec<(CompactString, PyObjectRef)> = defaults.clone();
        ns.insert(CompactString::from("__init__"), PyObject::native_closure(
            "__init__",
            move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("__init__ missing self"));
                }
                let instance = &args[0];
                let pos_args = &args[1..];
                // Build a map of default values
                let def_map: std::collections::HashMap<&str, &PyObjectRef> = defaults_for_init.iter()
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
                                "__init__() missing required argument: '{}'", field
                            )));
                        };
                        attrs.insert(field.clone(), val);
                    }
                }
                Ok(PyObject::none())
            }
        ));

        // Generate __getitem__ for indexing by position
        let field_names_for_getitem = field_names.clone();
        ns.insert(CompactString::from("__getitem__"), PyObject::native_closure(
            "__getitem__",
            move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__getitem__ missing argument"));
                }
                let instance = &args[0];
                let idx = &args[1];
                if let Some(i) = idx.as_int() {
                    let i = if i < 0 { i + field_names_for_getitem.len() as i64 } else { i } as usize;
                    if i < field_names_for_getitem.len() {
                        if let Some(v) = instance.get_attr(&field_names_for_getitem[i]) {
                            return Ok(v);
                        }
                    }
                    return Err(PyException::index_error("tuple index out of range"));
                }
                Err(PyException::type_error("tuple indices must be integers"))
            }
        ));

        // Generate __len__
        let n_fields = field_names.len();
        ns.insert(CompactString::from("__len__"), PyObject::native_closure(
            "__len__",
            move |_args: &[PyObjectRef]| {
                Ok(PyObject::int(n_fields as i64))
            }
        ));

        // Generate __iter__ for unpacking
        let field_names_for_iter = field_names.clone();
        ns.insert(CompactString::from("__iter__"), PyObject::native_closure(
            "__iter__",
            move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("__iter__ missing self"));
                }
                let instance = &args[0];
                let vals: Vec<PyObjectRef> = field_names_for_iter.iter()
                    .map(|f| instance.get_attr(f).unwrap_or_else(PyObject::none))
                    .collect();
                Ok(PyObject::tuple(vals))
            }
        ));

        // Generate __repr__
        let class_name = cd.name.clone();
        let field_names_for_repr = field_names.clone();
        ns.insert(CompactString::from("__repr__"), PyObject::native_closure(
            "__repr__",
            move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("()")));
                }
                let instance = &args[0];
                let parts: Vec<String> = field_names_for_repr.iter()
                    .map(|f| {
                        let val = instance.get_attr(f)
                            .map(|v| v.repr())
                            .unwrap_or_else(|| "None".to_string());
                        format!("{}={}", f, val)
                    })
                    .collect();
                Ok(PyObject::str_val(CompactString::from(
                    format!("{}({})", class_name, parts.join(", "))
                )))
            }
        ));

        // Invalidate stale vtable/cache
        drop(ns);
        cd.invalidate_cache();
    }

    /// Process enum class: transform simple attributes into enum member instances.
    pub(crate) fn process_enum_class(&mut self, cls: &PyObjectRef, bases: &[PyObjectRef]) -> PyResult<()> {
        // Check if any base has __enum__ marker
        let is_enum = bases.iter().any(|b| {
            if let Some(marker) = b.get_attr("__enum__") {
                marker.is_truthy()
            } else {
                false
            }
        });
        if !is_enum { return Ok(()); }

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
                    && !matches!(&v.payload,
                        PyObjectPayload::Function(_) |
                        PyObjectPayload::NativeFunction(_) |
                        PyObjectPayload::BuiltinFunction(_) |
                        PyObjectPayload::Property(_) |
                        PyObjectPayload::NativeClosure(_) |
                        PyObjectPayload::StaticMethod(_) |
                        PyObjectPayload::ClassMethod(_))
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
            b.get_attr("__flag__").map(|m| m.is_truthy()).unwrap_or(false)
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
                    } else { value.clone() }
                } else { value.clone() }
            } else {
                // Track max int value for auto() continuation
                if let Some(iv) = value.as_int() {
                    if is_flag {
                        if iv >= auto_counter { auto_counter = (iv as u64).next_power_of_two() as i64 * 2; }
                    } else {
                        if iv >= auto_counter { auto_counter = iv + 1; }
                    }
                }
                value.clone()
            };

            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
            attrs.insert(CompactString::from("value"), resolved_value.clone());
            attrs.insert(CompactString::from("_name_"), PyObject::str_val(name.clone()));
            attrs.insert(CompactString::from("_value_"), resolved_value.clone());

            // Enum __repr__: "<ClassName.MemberName: value>" (CPython format)
            // Only set default if user didn't define custom __repr__ in the class body
            if ns.get("__repr__").is_none() {
                let val_repr = resolved_value.repr();
                let full_repr = CompactString::from(format!("<{}.{}: {}>", class_name, name, val_repr));
                let repr_copy = full_repr;
                attrs.insert(intern_or_new("__repr__"), PyObject::native_closure(
                    "__repr__",
                    move |_args| {
                        Ok(PyObject::str_val(repr_copy.clone()))
                    }
                ));
            }
            // __str__: use custom __str__ from class body if defined, else "ClassName.MemberName"
            if ns.get("__str__").is_none() {
                let str_val = CompactString::from(format!("{}.{}", class_name, name));
                attrs.insert(intern_or_new("__str__"), PyObject::native_closure(
                    "__str__",
                    move |_args| {
                        Ok(PyObject::str_val(str_val.clone()))
                    }
                ));
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
        let pairs: Vec<(PyObjectRef, PyObjectRef)> = member_map.iter()
            .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
            .collect();
        ns.insert(intern_or_new("__members__"), PyObject::dict_from_pairs(pairs));

        // Mark as enum
        ns.insert(intern_or_new("__enum__"), PyObject::bool_val(true));

        // For Flag-based enums, add bitwise operations (__or__, __and__, __xor__, __invert__, __contains__, __int__, __bool__)
        let is_flag = bases.iter().any(|b| {
            b.get_attr("__flag__").map(|m| m.is_truthy()).unwrap_or(false)
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
                            let member_list: Vec<(CompactString, i64)> = members_map.iter()
                                .filter_map(|(k, v)| {
                                    if let HashableKey::Str(name) = k {
                                        v.get_attr("value").and_then(|v| v.as_int())
                                            .map(|val| (name.clone(), val))
                                    } else { None }
                                })
                                .collect();
                            // Greedy decomposition: highest values first, but output in definition order
                            let mut sorted_by_val: Vec<(usize, &CompactString, i64)> = member_list.iter()
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
                    CompactString::from(names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("|"))
                };

                let repr_str = CompactString::from(format!("<{}.{}: {}>", class_name, name_str, combined));
                let str_val = CompactString::from(format!("{}.{}", class_name, name_str));
                let repr_copy = repr_str.clone();
                let str_copy = str_val.clone();

                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("value"), PyObject::int(combined));
                attrs.insert(CompactString::from("_value_"), PyObject::int(combined));
                attrs.insert(CompactString::from("name"), PyObject::str_val(name_str.clone()));
                attrs.insert(CompactString::from("_name_"), PyObject::str_val(name_str));
                attrs.insert(intern_or_new("__repr__"), PyObject::native_closure(
                    "__repr__", move |_args| Ok(PyObject::str_val(repr_copy.clone()))
                ));
                attrs.insert(intern_or_new("__str__"), PyObject::native_closure(
                    "__str__", move |_args| Ok(PyObject::str_val(str_copy.clone()))
                ));
                PyObject::instance_with_attrs(cls.clone(), attrs)
            }

            let cls_or = cls.clone();
            ns.insert(intern_or_new("__or__"), PyObject::native_closure("Flag.__or__", move |args: &[PyObjectRef]| {
                if args.len() < 2 { return Ok(PyObject::none()); }
                let a_val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                let b_val = args[1].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                Ok(make_flag_instance(&cls_or, a_val | b_val))
            }));
            let cls_and = cls.clone();
            ns.insert(intern_or_new("__and__"), PyObject::native_closure("Flag.__and__", move |args: &[PyObjectRef]| {
                if args.len() < 2 { return Ok(PyObject::none()); }
                let a_val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                let b_val = args[1].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                Ok(make_flag_instance(&cls_and, a_val & b_val))
            }));
            let cls_xor = cls.clone();
            ns.insert(intern_or_new("__xor__"), PyObject::native_closure("Flag.__xor__", move |args: &[PyObjectRef]| {
                if args.len() < 2 { return Ok(PyObject::none()); }
                let a_val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                let b_val = args[1].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                Ok(make_flag_instance(&cls_xor, a_val ^ b_val))
            }));
            let cls_inv = cls.clone();
            ns.insert(intern_or_new("__invert__"), PyObject::native_closure("Flag.__invert__", move |args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::none()); }
                let self_val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                // Compute the bitmask of all defined member values
                let mut all_bits: i64 = 0;
                if let PyObjectPayload::Class(cd) = &cls_inv.payload {
                    let ns = cd.namespace.read();
                    if let Some(members) = ns.get("__members__") {
                        if let PyObjectPayload::Dict(map) = &members.payload {
                            for v in map.read().values() {
                                if let Some(val) = v.get_attr("value").and_then(|v| v.as_int()) {
                                    all_bits |= val;
                                }
                            }
                        }
                    }
                }
                Ok(make_flag_instance(&cls_inv, all_bits & !self_val))
            }));
            ns.insert(intern_or_new("__contains__"), PyObject::native_closure("Flag.__contains__", move |args: &[PyObjectRef]| {
                if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
                let self_val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                let other_val = args[1].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                Ok(PyObject::bool_val(self_val & other_val == other_val && other_val != 0))
            }));
            // __int__ — allow int() on flag members
            ns.insert(intern_or_new("__int__"), PyObject::native_function(
                "Flag.__int__", |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::int(0)); }
                    let val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                    Ok(PyObject::int(val))
                }
            ));
            // __bool__ — non-zero flag is truthy
            ns.insert(intern_or_new("__bool__"), PyObject::native_function(
                "Flag.__bool__", |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::bool_val(false)); }
                    let val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                    Ok(PyObject::bool_val(val != 0))
                }
            ));
        }

        // Namespace was mutated (__enum__, __members__, __or__ etc.) after
        // ClassData::new() built the vtable.  Invalidate caches so lookups
        // pick up the new methods instead of stale base-class versions.
        drop(ns); // release namespace write guard first
        cd.invalidate_cache();

        Ok(())
    }

    /// Handle __build_class__ with keyword args (e.g., metaclass=Meta).
    pub(crate) fn build_class_kw(
        &mut self,
        args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "__build_class__ requires at least 2 arguments"));
        }
        let body_func = args[0].clone();
        let class_name = match &args[1].payload {
            PyObjectPayload::Str(s) => (**s).clone(),
            _ => CompactString::from(args[1].py_to_string()),
        };
        let bases: Vec<PyObjectRef> = Self::resolve_mro_entries(&args[2..]);

        // Extract metaclass from kwargs, falling back to inherited metaclass from bases
        let metaclass = kwargs.iter()
            .find(|(k, _)| k.as_str() == "metaclass")
            .map(|(_, v)| v.clone())
            .or_else(|| Self::inherited_metaclass(&bases));

        // Call __prepare__ on metaclass if available (PEP 3115)
        let (prepared_ns, prepare_dict_obj): (FxAttrMap, Option<PyObjectRef>) =
            if let Some(ref meta) = metaclass {
                if let Some(prepare) = meta.get_attr("__prepare__") {
                    let name_obj = PyObject::str_val(class_name.clone());
                    let bases_tuple = PyObject::tuple(bases.clone());
                    let result = self.call_object(prepare, vec![name_obj, bases_tuple])?;
                    // Extract initial contents into FxAttrMap for frame.local_names
                    let ns = match &result.payload {
                        PyObjectPayload::Dict(d) => {
                            let d = d.read();
                            let mut ns = FxAttrMap::default();
                            for (k, v) in d.iter() {
                                if let HashableKey::Str(s) = k {
                                    ns.insert(s.clone(), v.clone());
                                }
                            }
                            ns
                        }
                        _ => FxAttrMap::default(),
                    };
                    (ns, Some(result))
                } else {
                    (FxAttrMap::default(), None)
                }
            } else {
                (FxAttrMap::default(), None)
            };

        // Execute class body to get namespace
        let namespace = match &body_func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = Rc::clone(&pyfunc.code);
                let globals = pyfunc.globals.clone();
                let cc = pyfunc.constant_cache.clone();
                let mut frame = Frame::new_from_pool(code, globals, self.builtins.clone(), cc, &mut self.frame_pool);
                frame.scope_kind = ScopeKind::Class;
                for (k, v) in &prepared_ns {
                    frame.local_names_insert(k.clone(), v.clone());
                }
                // Store the __prepare__ dict on the frame (reserved for future use)
                frame.prepare_dict = prepare_dict_obj.clone();
                let n_cell = frame.code.cellvars.len();
                for (i, cell) in pyfunc.closure.iter().enumerate() {
                    let free_idx = n_cell + i;
                    if free_idx < frame.cells.len() {
                        frame.cells[free_idx] = cell.clone();
                    }
                }
                self.call_stack.push(frame);
                let _ = self.run_frame();
                let frame = self.call_stack.pop().unwrap();
                let cellvar_names: Vec<CompactString> = frame.code.cellvars.clone();
                let cells = frame.cells.clone();
                (frame.local_names.map(|b| *b).unwrap_or_default(), Some((cellvar_names, cells)))
            }
            _ => (FxAttrMap::default(), None),
        };
        let (namespace, class_cell_info) = namespace;

        if let Some(meta) = metaclass {
            // Metaclass provided: call metaclass.__new__(mcs, name, bases, namespace_dict)
            // which should return the class object.
            let bases_list: Vec<PyObjectRef> = bases.clone();
            let mro = Self::compute_mro(&bases_list)?;
            
            // Build namespace dict for passing to __new__.
            // If __prepare__ returned a dict, reuse that object so metaclass.__new__
            // receives the original (possibly custom) dict with all class body assignments.
            let ns_dict = if let Some(pd) = prepare_dict_obj {
                // Sync class-body assignments into the __prepare__ dict.
                match &pd.payload {
                    PyObjectPayload::Dict(d) => {
                        let mut map = d.write();
                        for (k, v) in &namespace {
                            map.insert(HashableKey::str_key(k.clone()), v.clone());
                        }
                    }
                    PyObjectPayload::Instance(inst) => {
                        // Dict subclass: write into dict_storage and call __setitem__
                        if let Some(ref ds) = inst.dict_storage {
                            let mut map = ds.write();
                            for (k, v) in &namespace {
                                map.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                        }
                        // Also call Python-level __setitem__ so overrides are triggered
                        if let Some(setitem) = pd.get_attr("__setitem__") {
                            for (k, v) in &namespace {
                                let _ = self.call_object(
                                    setitem.clone(),
                                    vec![PyObject::str_val(k.clone()), v.clone()],
                                );
                            }
                        }
                    }
                    _ => {}
                }
                pd
            } else {
                let mut map = IndexMap::new();
                for (k, v) in &namespace {
                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                }
                PyObject::dict(map)
            };
            let name_obj = PyObject::str_val(class_name.clone());
            let bases_tuple = PyObject::tuple(bases_list.clone());
            
            // Try calling metaclass.__new__(mcs, name, bases, namespace)
            let own_new = if let PyObjectPayload::Class(cd) = &meta.payload {
                cd.namespace.read().get("__new__").cloned()
            } else {
                None
            };
            let cls = if let Some(new_method) = own_new {
                // User-defined __new__ on the metaclass
                let new_fn = match &new_method.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => new_method,
                };
                let result = self.call_object(new_fn, vec![meta.clone(), name_obj.clone(), bases_tuple.clone(), ns_dict.clone()])?;
                // Ensure metaclass is set on the class returned by __new__
                if let PyObjectPayload::Class(cd) = &result.payload {
                    if cd.metaclass.is_none() {
                        PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData {
                            name: cd.name.clone(),
                            bases: cd.bases.clone(),
                            namespace: cd.namespace.clone(),
                            mro: cd.mro.clone(),
                            metaclass: Some(meta.clone()),
                            method_cache: Rc::new(PyCell::new(FxHashMap::default())),
                            subclasses: Rc::new(PyCell::new(Vec::new())),
                            slots: cd.slots.clone(),
                            has_getattribute: cd.has_getattribute,
                            has_getattr: cd.has_getattr,
                            has_setattr: cd.has_setattr,
                            has_descriptors: cd.has_descriptors,
                            method_vtable: cd.method_vtable.clone(),
                            attr_shape: cd.attr_shape.clone(),
                            class_version: cd.class_version,
                            is_dict_subclass: cd.is_dict_subclass,
                            expected_attrs: cd.expected_attrs,
                            is_simple_class: Cell::new(false), // has metaclass
                            is_exception_subclass: cd.is_exception_subclass,
                            instance_flags: cd.instance_flags,
                            cached_init: PyCell::new(None),
                            cached_init_inline: PyCell::new(None),
                            has_custom_new: Cell::new(cd.has_custom_new.get()),
                        })))
                    } else {
                        result
                    }
                } else {
                    result
                }
            } else {
                // No __new__ — create class directly (like type.__new__)
                PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData::new(
                    class_name.clone(),
                    bases_list.clone(),
                    namespace,
                    mro,
                    Some(meta.clone()),
                ))))
            };
            
            // Ensure metaclass is set on the returned class
            if let PyObjectPayload::Class(cd) = &cls.payload {
                if cd.metaclass.is_none() {
                    // If __new__ returned a plain class, inject metaclass
                    let ns = cd.namespace.write();
                    // merge any attrs set by __new__ into the class
                    drop(ns);
                }
            }
            
            // Call metaclass's __init__ if it has one
            if let Some(init) = meta.get_attr("__init__") {
                if matches!(&init.payload, PyObjectPayload::BoundMethod { method, .. } if matches!(&method.payload, PyObjectPayload::Function(_))) {
                    let init_fn = match &init.payload {
                        PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                        _ => init,
                    };
                    self.call_object(init_fn, vec![cls.clone(), name_obj, bases_tuple, ns_dict])?;
                }
            }
            // __init_subclass__ handling
            // Collect non-metaclass kwargs to forward to __init_subclass__
            let init_sub_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs.iter()
                .filter(|(k, _)| k.as_str() != "metaclass")
                .cloned()
                .collect();
            if let PyObjectPayload::Class(cd) = &cls.payload {
                if let Some(base) = cd.bases.first() {
                    if let Some(init_sub) = base.get_attr("__init_subclass__") {
                        let bound = if let PyObjectPayload::BoundMethod { method, .. } = &init_sub.payload {
                            PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: cls.clone(),
                                    method: method.clone(),
                                }
                            })
                        } else {
                            PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: cls.clone(),
                                    method: init_sub,
                                }
                            })
                        };
                        if init_sub_kwargs.is_empty() {
                            self.call_object(bound, vec![])?;
                        } else {
                            self.call_object_kw(bound, vec![], init_sub_kwargs.clone())?;
                        }
                    }
                }
            }
            // Call __set_name__ on descriptors in the class namespace (PEP 487)
            self.call_set_name_on_descriptors(&cls)?;

            // Populate __class__ cell (PEP 3135)
            if let Some((ref cellvar_names, ref cells)) = class_cell_info {
                Self::patch_class_cell(cellvar_names, cells, &cls);
            }
            // Register as subclass of each base
            if let PyObjectPayload::Class(cd) = &cls.payload {
                for base in &cd.bases {
                    if let PyObjectPayload::Class(bcd) = &base.payload {
                        bcd.subclasses.write().push(PyObjectRef::downgrade(&cls));
                    }
                }
                // If metaclass is ABCMeta, add register() method bound to this class
                if let Some(ref mcs) = cd.metaclass {
                    if let PyObjectPayload::Class(mcs_cd) = &mcs.payload {
                        if mcs_cd.name.as_str() == "ABCMeta" {
                            let cls_ref = cls.clone();
                            cd.namespace.write().insert(
                                CompactString::from("register"),
                                PyObject::native_closure("register", move |args: &[PyObjectRef]| {
                                    if args.is_empty() {
                                        return Err(PyException::type_error("register() requires a subclass argument"));
                                    }
                                    let subclass = &args[0];
                                    if let PyObjectPayload::Class(cd) = &cls_ref.payload {
                                        let mut ns = cd.namespace.write();
                                        let registry = ns.entry(CompactString::from("_abc_registry"))
                                            .or_insert_with(|| PyObject::dict(new_fx_hashkey_map()))
                                            .clone();
                                        if let PyObjectPayload::Dict(map) = &registry.payload {
                                            let ptr = PyObjectRef::as_ptr(subclass) as usize;
                                            map.write().insert(
                                                HashableKey::Identity(ptr, subclass.clone()),
                                                PyObject::bool_val(true),
                                            );
                                        }
                                    }
                                    Ok(subclass.clone())
                                }),
                            );
                        }
                    }
                }
            }
            Ok(cls)
        } else {
            // No metaclass: build normally
            let mro = Self::compute_mro(&bases)?;            let cls = PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData::new(
                class_name, bases.clone(), namespace, mro, None,
            ))));
            // __init_subclass__: bind to new subclass (cls), not parent
            // Forward non-metaclass kwargs to __init_subclass__
            let init_sub_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs.iter()
                .filter(|(k, _)| k.as_str() != "metaclass")
                .cloned()
                .collect();
            if let Some(base) = bases.first() {
                if let Some(init_sub) = base.get_attr("__init_subclass__") {
                    let bound = if let PyObjectPayload::BoundMethod { method, .. } = &init_sub.payload {
                        PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: cls.clone(),
                                method: method.clone(),
                            }
                        })
                    } else {
                        PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: cls.clone(),
                                method: init_sub,
                            }
                        })
                    };
                    if init_sub_kwargs.is_empty() {
                        self.call_object(bound, vec![])?;
                    } else {
                        self.call_object_kw(bound, vec![], init_sub_kwargs.clone())?;
                    }
                }
            }
            // Call __set_name__ on descriptors in the class namespace (PEP 487)
            self.call_set_name_on_descriptors(&cls)?;

            // NamedTuple metaclass behavior
            self.process_namedtuple_class(&cls, &bases);

            // Enum metaclass behavior
            self.process_enum_class(&cls, &bases)?;

            // Populate __class__ cell (PEP 3135)
            if let Some((ref cellvar_names, ref cells)) = class_cell_info {
                Self::patch_class_cell(cellvar_names, cells, &cls);
            }
            // Register as subclass of each base
            for base in &bases {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    bcd.subclasses.write().push(PyObjectRef::downgrade(&cls));
                }
            }
            Ok(cls)
        }
    }

    /// Populate the __class__ cell in the class body's cell array after class creation (PEP 3135).
    fn patch_class_cell(cellvar_names: &[CompactString], cells: &[CellRef], cls: &PyObjectRef) {
        for (i, name) in cellvar_names.iter().enumerate() {
            if name.as_str() == "__class__" {
                if let Some(cell) = cells.get(i) {
                    let mut cell_val = cell.write();
                    *cell_val = Some(cls.clone());
                }
                break;
            }
        }
    }

    /// Call __set_name__ on descriptors in the class namespace (PEP 487).
    fn call_set_name_on_descriptors(&mut self, cls: &PyObjectRef) -> PyResult<()> {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            // Quick scan: skip snapshot if no Instance values exist in namespace
            let has_instances = {
                let ns = cd.namespace.read();
                ns.values().any(|v| matches!(&v.payload, PyObjectPayload::Instance(_)))
            };
            if !has_instances { return Ok(()); }
            let ns_snapshot: Vec<(CompactString, PyObjectRef)> = {
                let ns = cd.namespace.read();
                ns.iter()
                    .filter(|(_, v)| matches!(&v.payload, PyObjectPayload::Instance(_)))
                    .map(|(k, v)| (k.clone(), v.clone())).collect()
            };
            for (attr_name, attr_val) in &ns_snapshot {
                if let Some(set_name_method) = attr_val.get_attr("__set_name__") {
                    let bound = if matches!(&set_name_method.payload, PyObjectPayload::BoundMethod { .. }) {
                        set_name_method
                    } else {
                        PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: attr_val.clone(),
                                method: set_name_method,
                            }
                        })
                    };
                    let name_arg = PyObject::str_val(attr_name.clone());
                    self.call_object(bound, vec![cls.clone(), name_arg])?;
                }
            }
        }
        Ok(())
    }

    /// Compute MRO from bases using C3 linearization (matches CPython).
    /// Returns `TypeError` for inconsistent MRO (same as CPython).
    pub(crate) fn compute_mro(bases: &[PyObjectRef]) -> PyResult<Vec<PyObjectRef>> {
        if bases.is_empty() {
            return Ok(vec![]);
        }
        // Build linearizations: L(base) for each base, plus the bases list itself
        let mut linearizations: Vec<Vec<PyObjectRef>> = Vec::new();
        for base in bases {
            let mut l = vec![base.clone()];
            if let PyObjectPayload::Class(cd) = &base.payload {
                l.extend(cd.mro.iter().cloned());
            }
            // ExceptionType/BuiltinType bases: no child MRO, just include them
            linearizations.push(l);
        }
        linearizations.push(bases.to_vec());
        Self::c3_merge(&mut linearizations)
    }

    pub(crate) fn c3_merge(linearizations: &mut Vec<Vec<PyObjectRef>>) -> PyResult<Vec<PyObjectRef>> {
        let mut result = Vec::new();
        // Track start index per linearization to avoid O(n) vec.remove(0)
        let mut starts: Vec<usize> = vec![0; linearizations.len()];
        loop {
            // Check if all lists are exhausted
            let any_remaining = starts.iter().enumerate().any(|(i, &s)| s < linearizations[i].len());
            if !any_remaining {
                break;
            }
            // Find a good head: first element of some list that doesn't appear in the tail of any list
            let mut found = None;
            for (i, lin) in linearizations.iter().enumerate() {
                if starts[i] >= lin.len() { continue; }
                let candidate_ptr = PyObjectRef::as_ptr(&lin[starts[i]]);
                let in_tail = linearizations.iter().enumerate().any(|(j, other)| {
                    let s = starts[j];
                    if s >= other.len() { return false; }
                    other[s+1..].iter().any(|x| PyObjectRef::as_ptr(x) == candidate_ptr)
                });
                if !in_tail {
                    found = Some(lin[starts[i]].clone());
                    break;
                }
            }
            if let Some(head) = found {
                let head_ptr = PyObjectRef::as_ptr(&head);
                result.push(head);
                for (i, lin) in linearizations.iter().enumerate() {
                    if starts[i] < lin.len() && PyObjectRef::as_ptr(&lin[starts[i]]) == head_ptr {
                        starts[i] += 1;
                    }
                }
            } else {
                return Err(PyException::type_error(
                    "Cannot create a consistent method resolution order (MRO)"
                ));
            }
        }
        Ok(result)
    }

}
