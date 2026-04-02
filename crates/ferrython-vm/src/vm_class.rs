//! Class building — __build_class__, MRO computation, enum processing.

use crate::frame::{Frame, ScopeKind};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    ClassData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
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
                let code = pyfunc.code.clone();
                let globals = pyfunc.globals.clone();
                let mut frame = Frame::new(code, globals, self.builtins.clone());
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
                frame.local_names
            }
            _ => IndexMap::new(),
        };

        // Build MRO: [self_class, ...linearized_parents, object]
        // Simple C3-like: for single inheritance just chain; for multiple use bases order
        let mro = Self::compute_mro(&bases);
        let cls = PyObject::wrap(PyObjectPayload::Class(ClassData {
            name: class_name, bases: bases.clone(), namespace: Arc::new(RwLock::new(namespace)), mro, metaclass: None,
        }));

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
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let ns_snapshot: Vec<(CompactString, PyObjectRef)> = {
                let ns = cd.namespace.read();
                ns.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            };
            for (attr_name, attr_val) in &ns_snapshot {
                // Only call __set_name__ on instance objects (descriptors)
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

        // Enum metaclass behavior: transform class attributes into enum members
        self.process_enum_class(&cls, &bases)?;

        Ok(cls)
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
                        PyObjectPayload::BuiltinFunction(_))
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        };

        // For each member, create an instance of the enum class with name and value
        let mut ns = cd.namespace.write();
        let mut member_map = IndexMap::new();

        for (name, value) in &members {
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
            attrs.insert(CompactString::from("value"), value.clone());
            attrs.insert(CompactString::from("_name_"), PyObject::str_val(name.clone()));
            attrs.insert(CompactString::from("_value_"), value.clone());
            let member = PyObject::instance_with_attrs(cls.clone(), attrs);
            ns.insert(name.clone(), member.clone());
            member_map.insert(name.clone(), member);
        }

        // Add __members__ dict
        let pairs: Vec<(PyObjectRef, PyObjectRef)> = member_map.iter()
            .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
            .collect();
        ns.insert(CompactString::from("__members__"), PyObject::dict_from_pairs(pairs));

        // Mark as enum
        ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));

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

        // Execute class body to get namespace
        let namespace = match &body_func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = pyfunc.code.clone();
                let globals = pyfunc.globals.clone();
                let mut frame = Frame::new(code, globals, self.builtins.clone());
                frame.scope_kind = ScopeKind::Class;
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
                frame.local_names
            }
            _ => IndexMap::new(),
        };

        if let Some(meta) = metaclass {
            // Metaclass provided: create a Class with metaclass info
            // In CPython: metaclass(name, bases, dict) calls type.__call__(metaclass) which does:
            //   1. metaclass.__new__(metaclass, name, bases, dict) → creates Class
            //   2. metaclass.__init__(cls, name, bases, dict) → initializes
            // We handle this by creating the Class directly with the metaclass stored.
            let bases_list: Vec<PyObjectRef> = bases.clone();
            let mro = Self::compute_mro(&bases_list);
            let cls = PyObject::wrap(PyObjectPayload::Class(ClassData {
                name: class_name.clone(),
                bases: bases_list.clone(),
                namespace: Arc::new(RwLock::new(namespace)),
                mro,
                metaclass: Some(meta.clone()),
            }));
            // Call metaclass's __init__ if it has one (rare, but some metaclasses use it)
            if let Some(init) = meta.get_attr("__init__") {
                // Only call if it's a user-defined method (not from type)
                if matches!(&init.payload, PyObjectPayload::BoundMethod { method, .. } if matches!(&method.payload, PyObjectPayload::Function(_))) {
                    let init_fn = match &init.payload {
                        PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                        _ => init,
                    };
                    let ns_dict = {
                        let mut map = IndexMap::new();
                        for (k, v) in cls.get_attr("__dict__").map(|d| {
                            if let PyObjectPayload::Dict(m) = &d.payload { m.read().clone() }
                            else { IndexMap::new() }
                        }).unwrap_or_default() {
                            map.insert(k, v);
                        }
                        PyObject::dict(map)
                    };
                    let bases_tuple = PyObject::tuple(bases_list);
                    let name_obj = PyObject::str_val(class_name);
                    self.call_object(init_fn, vec![cls.clone(), name_obj, bases_tuple, ns_dict])?;
                }
            }
            // __init_subclass__ handling
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
                        self.call_object(bound, vec![])?;
                    }
                }
            }
            Ok(cls)
        } else {
            // No metaclass: build normally
            let mro = Self::compute_mro(&bases);
            let cls = PyObject::wrap(PyObjectPayload::Class(ClassData {
                name: class_name, bases: bases.clone(),
                namespace: Arc::new(RwLock::new(namespace)), mro, metaclass: None,
            }));
            // __init_subclass__: bind to new subclass (cls), not parent
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
                    self.call_object(bound, vec![])?;
                }
            }
            Ok(cls)
        }
    }

    /// Compute a simple MRO from bases (includes bases and their ancestors, NOT self).
    /// C3 linearization for MRO computation (matches CPython).
    pub(crate) fn compute_mro(bases: &[PyObjectRef]) -> Vec<PyObjectRef> {
        if bases.is_empty() {
            return vec![];
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

    pub(crate) fn c3_merge(linearizations: &mut Vec<Vec<PyObjectRef>>) -> Vec<PyObjectRef> {
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
                // C3 linearization failure — fall back to DFS
                break;
            }
        }
        result
    }

}
