//! Class building — __build_class__, MRO computation, enum processing.

use crate::frame::{CellRef, Frame, ScopeKind};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    ClassData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

impl VirtualMachine {
    pub(crate) fn build_class(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "__build_class__ requires at least 2 arguments"));
        }
        let body_func = args[0].clone();
        let class_name = match &args[1].payload {
            PyObjectPayload::Str(s) => s.clone(),
            _ => CompactString::from(args[1].py_to_string()),
        };
        let bases: Vec<PyObjectRef> = args[2..].to_vec();

        let namespace = match &body_func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = Arc::clone(&pyfunc.code);
                let globals = pyfunc.globals.clone();
                let cc = pyfunc.constant_cache.clone();
                let mut frame = Frame::new_from_pool(code, globals, Arc::clone(&self.builtins), cc, &mut self.frame_pool);
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
                (frame.local_names, Some((cellvar_names, cells)))
            }
            _ => (IndexMap::new(), None),
        };
        let (namespace, class_cell_info) = namespace;

        // Build MRO: [self_class, ...linearized_parents, object]
        // Simple C3-like: for single inheritance just chain; for multiple use bases order
        let mro = Self::compute_mro(&bases)?;
        let cls = PyObject::wrap(PyObjectPayload::Class(ClassData::new(
            class_name, bases.clone(), namespace, mro, None,
        )));

        // Populate __class__ cell so methods can access it via super() (PEP 3135)
        if let Some((ref cellvar_names, ref cells)) = class_cell_info {
            Self::patch_class_cell(cellvar_names, cells, &cls);
        }

        // Call __init_subclass__ on each base class (PEP 487)
        // __init_subclass__(cls) is called on the base's method with cls being the *new* subclass
        for base in &bases {
            if let Some(init_sub) = base.get_attr("__init_subclass__") {
                // If it's already a BoundMethod (e.g., classmethod), call with cls as arg
                // Otherwise, bind to the NEW class (cls) so `self` is the subclass
                let bound = if matches!(&init_sub.payload, PyObjectPayload::BoundMethod { .. }) {
                    // Re-bind to the new subclass
                    if let PyObjectPayload::BoundMethod { method, .. } = &init_sub.payload {
                        Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: cls.clone(),
                                method: method.clone(),
                            }
                        })
                    } else {
                        init_sub
                    }
                } else {
                    Arc::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: cls.clone(),
                            method: init_sub,
                        }
                    })
                };
                // __init_subclass__(cls) where cls is the new subclass
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
                bcd.subclasses.write().push(Arc::downgrade(&cls));
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
                HashableKey::Str(name.clone()),
                val.clone(),
            );
        }

        let mut ns = cd.namespace.write();
        ns.insert(intern_or_new("__namedtuple__"), PyObject::bool_val(true));
        ns.insert(CompactString::from("_fields"), fields_tuple);
        ns.insert(CompactString::from("_field_defaults"), PyObject::dict(defaults_map));

        // Dunder methods (__len__, __getitem__, __iter__, __repr__, __eq__, _asdict, _replace)
        // are handled by call_namedtuple_method() in instance_methods.rs via the
        // builtins dispatch layer, so no need to store NativeClosures here.
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
                        PyObjectPayload::NativeFunction { .. } |
                        PyObjectPayload::BuiltinFunction(_) |
                        PyObjectPayload::Property { .. } |
                        PyObjectPayload::NativeClosure { .. } |
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
            let val_repr = resolved_value.repr();
            let full_repr = CompactString::from(format!("<{}.{}: {}>", class_name, name, val_repr));
            let repr_copy = full_repr;
            attrs.insert(intern_or_new("__repr__"), PyObject::native_closure(
                "__repr__",
                move |_args| {
                    Ok(PyObject::str_val(repr_copy.clone()))
                }
            ));
            // __str__: "ClassName.MemberName"
            let str_val = CompactString::from(format!("{}.{}", class_name, name));
            attrs.insert(intern_or_new("__str__"), PyObject::native_closure(
                "__str__",
                move |_args| {
                    Ok(PyObject::str_val(str_val.clone()))
                }
            ));

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

        // For Flag-based enums, add bitwise operations (__or__, __and__, __contains__)
        let is_flag = bases.iter().any(|b| {
            b.get_attr("__flag__").map(|m| m.is_truthy()).unwrap_or(false)
        });
        if is_flag {
            ns.insert(intern_or_new("__flag__"), PyObject::bool_val(true));
            let cls_or = cls.clone();
            ns.insert(intern_or_new("__or__"), PyObject::native_closure("Flag.__or__", move |args: &[PyObjectRef]| {
                if args.len() < 2 { return Ok(PyObject::none()); }
                let a_val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                let b_val = args[1].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                let combined = a_val | b_val;
                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("value"), PyObject::int(combined));
                attrs.insert(CompactString::from("_value_"), PyObject::int(combined));
                attrs.insert(CompactString::from("name"), PyObject::none());
                attrs.insert(CompactString::from("_name_"), PyObject::none());
                Ok(PyObject::instance_with_attrs(cls_or.clone(), attrs))
            }));
            let cls_and = cls.clone();
            ns.insert(intern_or_new("__and__"), PyObject::native_closure("Flag.__and__", move |args: &[PyObjectRef]| {
                if args.len() < 2 { return Ok(PyObject::none()); }
                let a_val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                let b_val = args[1].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                let combined = a_val & b_val;
                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("value"), PyObject::int(combined));
                attrs.insert(CompactString::from("_value_"), PyObject::int(combined));
                attrs.insert(CompactString::from("name"), PyObject::none());
                attrs.insert(CompactString::from("_name_"), PyObject::none());
                Ok(PyObject::instance_with_attrs(cls_and.clone(), attrs))
            }));
            ns.insert(intern_or_new("__contains__"), PyObject::native_closure("Flag.__contains__", move |args: &[PyObjectRef]| {
                if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
                let self_val = args[0].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                let other_val = args[1].get_attr("value").and_then(|v| v.as_int()).unwrap_or(0);
                Ok(PyObject::bool_val(self_val & other_val == other_val && other_val != 0))
            }));
        }

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
            PyObjectPayload::Str(s) => s.clone(),
            _ => CompactString::from(args[1].py_to_string()),
        };
        let bases: Vec<PyObjectRef> = args[2..].to_vec();

        // Extract metaclass from kwargs
        let metaclass = kwargs.iter()
            .find(|(k, _)| k.as_str() == "metaclass")
            .map(|(_, v)| v.clone());

        // Call __prepare__ on metaclass if available (PEP 3115)
        let prepared_ns: IndexMap<CompactString, PyObjectRef> = if let Some(ref meta) = metaclass {
            if let Some(prepare) = meta.get_attr("__prepare__") {
                let name_obj = PyObject::str_val(class_name.clone());
                let bases_tuple = PyObject::tuple(bases.clone());
                let result = self.call_object(prepare, vec![name_obj, bases_tuple])?;
                // Convert returned dict to IndexMap
                match &result.payload {
                    PyObjectPayload::Dict(d) => {
                        let d = d.read();
                        let mut ns = IndexMap::new();
                        for (k, v) in d.iter() {
                            if let HashableKey::Str(s) = k {
                                ns.insert(s.clone(), v.clone());
                            }
                        }
                        ns
                    }
                    _ => IndexMap::new(),
                }
            } else {
                IndexMap::new()
            }
        } else {
            IndexMap::new()
        };

        // Execute class body to get namespace
        let namespace = match &body_func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = Arc::clone(&pyfunc.code);
                let globals = pyfunc.globals.clone();
                let cc = pyfunc.constant_cache.clone();
                let mut frame = Frame::new_from_pool(code, globals, Arc::clone(&self.builtins), cc, &mut self.frame_pool);
                frame.scope_kind = ScopeKind::Class;
                for (k, v) in &prepared_ns {
                    frame.local_names.insert(k.clone(), v.clone());
                }
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
                (frame.local_names, Some((cellvar_names, cells)))
            }
            _ => (IndexMap::new(), None),
        };
        let (namespace, class_cell_info) = namespace;

        if let Some(meta) = metaclass {
            // Metaclass provided: call metaclass.__new__(mcs, name, bases, namespace_dict)
            // which should return the class object.
            let bases_list: Vec<PyObjectRef> = bases.clone();
            let mro = Self::compute_mro(&bases_list)?;
            
            // Build namespace dict for passing to __new__
            let ns_dict = {
                let mut map = IndexMap::new();
                for (k, v) in &namespace {
                    map.insert(HashableKey::Str(k.clone()), v.clone());
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
                        PyObject::wrap(PyObjectPayload::Class(ClassData {
                            name: cd.name.clone(),
                            bases: cd.bases.clone(),
                            namespace: cd.namespace.clone(),
                            mro: cd.mro.clone(),
                            metaclass: Some(meta.clone()),
                            method_cache: Arc::new(RwLock::new(IndexMap::new())),
                            subclasses: Arc::new(RwLock::new(Vec::new())),
                            slots: cd.slots.clone(),
                            has_getattribute: cd.has_getattribute,
                        }))
                    } else {
                        result
                    }
                } else {
                    result
                }
            } else {
                // No __new__ — create class directly (like type.__new__)
                PyObject::wrap(PyObjectPayload::Class(ClassData::new(
                    class_name.clone(),
                    bases_list.clone(),
                    namespace,
                    mro,
                    Some(meta.clone()),
                )))
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
                for base in &cd.bases {
                    if let Some(init_sub) = base.get_attr("__init_subclass__") {
                        let bound = if let PyObjectPayload::BoundMethod { method, .. } = &init_sub.payload {
                            Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: cls.clone(),
                                    method: method.clone(),
                                }
                            })
                        } else {
                            Arc::new(PyObject {
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
                        bcd.subclasses.write().push(Arc::downgrade(&cls));
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
                                            .or_insert_with(|| PyObject::dict(IndexMap::new()))
                                            .clone();
                                        if let PyObjectPayload::Dict(map) = &registry.payload {
                                            let ptr = std::sync::Arc::as_ptr(subclass) as usize;
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
            let mro = Self::compute_mro(&bases)?;            let cls = PyObject::wrap(PyObjectPayload::Class(ClassData::new(
                class_name, bases.clone(), namespace, mro, None,
            )));
            // __init_subclass__: bind to new subclass (cls), not parent
            // Forward non-metaclass kwargs to __init_subclass__
            let init_sub_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs.iter()
                .filter(|(k, _)| k.as_str() != "metaclass")
                .cloned()
                .collect();
            for base in &bases {
                if let Some(init_sub) = base.get_attr("__init_subclass__") {
                    let bound = if let PyObjectPayload::BoundMethod { method, .. } = &init_sub.payload {
                        Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: cls.clone(),
                                method: method.clone(),
                            }
                        })
                    } else {
                        Arc::new(PyObject {
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
                    bcd.subclasses.write().push(Arc::downgrade(&cls));
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
            let ns_snapshot: Vec<(CompactString, PyObjectRef)> = {
                let ns = cd.namespace.read();
                ns.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            };
            for (attr_name, attr_val) in &ns_snapshot {
                if !matches!(&attr_val.payload, PyObjectPayload::Instance(_)) {
                    continue;
                }
                if let Some(set_name_method) = attr_val.get_attr("__set_name__") {
                    let bound = if matches!(&set_name_method.payload, PyObjectPayload::BoundMethod { .. }) {
                        set_name_method
                    } else {
                        Arc::new(PyObject {
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
        loop {
            // Remove empty lists
            linearizations.retain(|l| !l.is_empty());
            if linearizations.is_empty() {
                break;
            }
            // Find a good head: first element of some list that doesn't appear in the tail of any list
            let mut found = None;
            for lin in linearizations.iter() {
                let candidate = &lin[0];
                let candidate_ptr = Arc::as_ptr(candidate);
                let in_tail = linearizations.iter().any(|other| {
                    other.iter().skip(1).any(|x| Arc::as_ptr(x) == candidate_ptr)
                });
                if !in_tail {
                    found = Some(candidate.clone());
                    break;
                }
            }
            if let Some(head) = found {
                let head_ptr = Arc::as_ptr(&head);
                result.push(head);
                for lin in linearizations.iter_mut() {
                    if !lin.is_empty() && Arc::as_ptr(&lin[0]) == head_ptr {
                        lin.remove(0);
                    }
                }
            } else {
                // C3 linearization failure — raise TypeError like CPython
                return Err(PyException::type_error(
                    "Cannot create a consistent method resolution order (MRO)"
                ));
            }
        }
        Ok(result)
    }

}
