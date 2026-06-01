//! Class building — __build_class__, MRO computation, enum processing.

mod enum_processing;
mod namedtuple;

use crate::frame::{CellRef, Frame, ScopeKind};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    new_fx_hashkey_map, ClassData, FxAttrMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use rustc_hash::FxHashMap;
use std::cell::Cell;
use std::rc::Rc;

impl VirtualMachine {
    fn class_module_name(body_func: &PyObjectRef) -> CompactString {
        if let PyObjectPayload::Function(pyfunc) = &body_func.payload {
            if let Some(module) = pyfunc.globals.read().get("__name__") {
                return CompactString::from(module.py_to_string());
            }
        }
        CompactString::from("__main__")
    }

    fn ensure_class_module(namespace: &mut FxAttrMap, body_func: &PyObjectRef) {
        if !namespace.contains_key("__module__") {
            namespace.insert(
                intern_or_new("__module__"),
                PyObject::str_val(Self::class_module_name(body_func)),
            );
        }
    }

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
                        if cd.name.contains("GenericAlias")
                            || cd.name.contains("_GenericAlias")
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
        if bases.len() < 2 {
            return;
        }
        // Find which bases are "Generic" (the typing.Generic class)
        let generic_indices: Vec<usize> = bases
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                if let PyObjectPayload::Class(cd) = &b.payload {
                    if cd.name == "Generic" {
                        return Some(i);
                    }
                }
                None
            })
            .collect();
        if generic_indices.is_empty() {
            return;
        }
        // Check if any OTHER base already has Generic in its MRO
        let other_has_generic = bases.iter().enumerate().any(|(i, b)| {
            if generic_indices.contains(&i) {
                return false;
            }
            if let PyObjectPayload::Class(cd) = &b.payload {
                cd.mro.iter().any(|m| {
                    if let PyObjectPayload::Class(mc) = &m.payload {
                        mc.name == "Generic"
                    } else {
                        false
                    }
                })
            } else {
                false
            }
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
                "__build_class__ requires at least 2 arguments",
            ));
        }
        let bases: Vec<PyObjectRef> = Self::resolve_mro_entries(&args[2..]);

        // Reject subclassing of final builtin types (bool is not subclassable)
        for base in &bases {
            if let PyObjectPayload::BuiltinType(n) = &base.payload {
                if n.as_str() == "bool" {
                    return Err(PyException::type_error(CompactString::from(
                        "type 'bool' is not an acceptable base type",
                    )));
                }
            }
        }

        // If any base has a custom metaclass, delegate to the kw path
        if let Some(meta) = Self::inherited_metaclass(&bases) {
            let kwargs = vec![(CompactString::from("metaclass"), meta)];
            return self.build_class_kw(args, kwargs);
        }

        let body_func = args[0].clone();
        let class_name = match &args[1].payload {
            PyObjectPayload::Str(s) => s.to_compact_string(),
            _ => CompactString::from(args[1].py_to_string()),
        };

        let namespace = match &body_func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = Rc::clone(&pyfunc.code);
                let globals = pyfunc.globals.clone();
                let cc = pyfunc.constant_cache.clone();
                let mut frame = Frame::new_from_pool(
                    code,
                    globals,
                    self.builtins.clone(),
                    cc,
                    &mut self.frame_pool,
                );
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
                let body_result = self.run_frame();
                let frame = self.call_stack.pop().unwrap();
                body_result?;
                let cellvar_names: Vec<CompactString> = frame.code.cellvars.clone();
                let cells = frame.cells.clone();
                (
                    frame.local_names.map(|b| *b).unwrap_or_default(),
                    Some((cellvar_names, cells)),
                )
            }
            _ => (FxAttrMap::default(), None),
        };
        let (mut namespace, class_cell_info) = namespace;
        Self::ensure_class_module(&mut namespace, &body_func);

        // Build MRO: [self_class, ...linearized_parents, object]
        // Simple C3-like: for single inheritance just chain; for multiple use bases order
        let mro = Self::compute_mro(&bases)?;
        let cls = PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData::new(
            class_name,
            bases.clone(),
            namespace,
            mro,
            None,
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
                            },
                        })
                    } else {
                        init_sub
                    }
                } else {
                    PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: cls.clone(),
                            method: init_sub,
                        },
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

    /// Handle __build_class__ with keyword args (e.g., metaclass=Meta).
    pub(crate) fn build_class_kw(
        &mut self,
        args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "__build_class__ requires at least 2 arguments",
            ));
        }
        let body_func = args[0].clone();
        let class_name = match &args[1].payload {
            PyObjectPayload::Str(s) => s.to_compact_string(),
            _ => CompactString::from(args[1].py_to_string()),
        };
        let bases: Vec<PyObjectRef> = Self::resolve_mro_entries(&args[2..]);

        // Extract metaclass from kwargs, falling back to inherited metaclass from bases
        let metaclass = kwargs
            .iter()
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
                                    ns.insert(s.to_compact_string(), v.clone());
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
                let mut frame = Frame::new_from_pool(
                    code,
                    globals,
                    self.builtins.clone(),
                    cc,
                    &mut self.frame_pool,
                );
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
                let body_result = self.run_frame();
                let frame = self.call_stack.pop().unwrap();
                body_result?;
                let cellvar_names: Vec<CompactString> = frame.code.cellvars.clone();
                let cells = frame.cells.clone();
                (
                    frame.local_names.map(|b| *b).unwrap_or_default(),
                    Some((cellvar_names, cells)),
                )
            }
            _ => (FxAttrMap::default(), None),
        };
        let (mut namespace, class_cell_info) = namespace;
        Self::ensure_class_module(&mut namespace, &body_func);

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
            let expected_class_cell = Self::class_cell_object(class_cell_info.as_ref());
            if let Some(cell_obj) = expected_class_cell.clone() {
                Self::dict_set_str(&ns_dict, "__classcell__", cell_obj);
            }
            let name_obj = PyObject::str_val(class_name.clone());
            let bases_tuple = PyObject::tuple(bases_list.clone());

            // Try calling metaclass.__new__(mcs, name, bases, namespace)
            let own_new = if let PyObjectPayload::Class(cd) = &meta.payload {
                cd.namespace
                    .read()
                    .get("__new__")
                    .filter(|method| matches!(&method.payload, PyObjectPayload::Function(_)))
                    .cloned()
            } else {
                None
            };
            let used_custom_new = own_new.is_some();
            let cls = if let Some(new_method) = own_new {
                // User-defined __new__ on the metaclass
                let new_fn = match &new_method.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => new_method,
                };
                let result = self.call_object(
                    new_fn,
                    vec![
                        meta.clone(),
                        name_obj.clone(),
                        bases_tuple.clone(),
                        ns_dict.clone(),
                    ],
                )?;
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
                            builtin_base_name: cd.builtin_base_name.clone(),
                        })))
                    } else {
                        result
                    }
                } else {
                    result
                }
            } else {
                // No __new__ — create class directly (like type.__new__)
                let preliminary_cls =
                    PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData::new(
                        class_name.clone(),
                        bases_list.clone(),
                        namespace,
                        mro,
                        Some(meta.clone()),
                    ))));
                if let Some((ref cellvar_names, ref cells)) = class_cell_info {
                    Self::patch_class_cell(cellvar_names, cells, &preliminary_cls);
                }
                let cls = self.apply_metaclass_mro(preliminary_cls, &meta)?;
                if let Some((ref cellvar_names, ref cells)) = class_cell_info {
                    Self::patch_class_cell(cellvar_names, cells, &cls);
                }
                cls
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
                if matches!(&init.payload, PyObjectPayload::BoundMethod { method, .. } if matches!(&method.payload, PyObjectPayload::Function(_)))
                {
                    let init_fn = match &init.payload {
                        PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                        _ => init,
                    };
                    self.call_object(
                        init_fn,
                        vec![cls.clone(), name_obj, bases_tuple, ns_dict.clone()],
                    )?;
                }
            }
            // __init_subclass__ handling
            // Collect non-metaclass kwargs to forward to __init_subclass__
            let init_sub_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs
                .iter()
                .filter(|(k, _)| k.as_str() != "metaclass")
                .cloned()
                .collect();
            if let PyObjectPayload::Class(cd) = &cls.payload {
                if let Some(base) = cd.bases.first() {
                    if let Some(init_sub) = base.get_attr("__init_subclass__") {
                        let bound = if let PyObjectPayload::BoundMethod { method, .. } =
                            &init_sub.payload
                        {
                            PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: cls.clone(),
                                    method: method.clone(),
                                },
                            })
                        } else {
                            PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: cls.clone(),
                                    method: init_sub,
                                },
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
            // Populate __class__ cell (PEP 3135)
            if matches!(&cls.payload, PyObjectPayload::Class(_)) {
                if let Some((ref cellvar_names, ref cells)) = class_cell_info {
                    if used_custom_new {
                        Self::validate_propagated_class_cell(
                            &ns_dict,
                            expected_class_cell.as_ref(),
                        )?;
                    }
                    Self::validate_class_cell(cellvar_names, cells, &cls)?;
                }
            }
            // Call __set_name__ on descriptors in the class namespace (PEP 487)
            self.call_set_name_on_descriptors(&cls)?;
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
                                PyObject::native_closure(
                                    "register",
                                    move |args: &[PyObjectRef]| {
                                        if args.is_empty() {
                                            return Err(PyException::type_error(
                                                "register() requires a subclass argument",
                                            ));
                                        }
                                        let subclass = &args[0];
                                        if let PyObjectPayload::Class(cd) = &cls_ref.payload {
                                            let mut ns = cd.namespace.write();
                                            let registry = ns
                                                .entry(CompactString::from("_abc_registry"))
                                                .or_insert_with(|| {
                                                    PyObject::dict(new_fx_hashkey_map())
                                                })
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
                                    },
                                ),
                            );
                        }
                    }
                }
            }
            Ok(cls)
        } else {
            // No metaclass: build normally
            let mro = Self::compute_mro(&bases)?;
            let cls = PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData::new(
                class_name,
                bases.clone(),
                namespace,
                mro,
                None,
            ))));
            // __init_subclass__: bind to new subclass (cls), not parent
            // Forward non-metaclass kwargs to __init_subclass__
            let init_sub_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs
                .iter()
                .filter(|(k, _)| k.as_str() != "metaclass")
                .cloned()
                .collect();
            if let Some(base) = bases.first() {
                if let Some(init_sub) = base.get_attr("__init_subclass__") {
                    let bound =
                        if let PyObjectPayload::BoundMethod { method, .. } = &init_sub.payload {
                            PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: cls.clone(),
                                    method: method.clone(),
                                },
                            })
                        } else {
                            PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: cls.clone(),
                                    method: init_sub,
                                },
                            })
                        };
                    if init_sub_kwargs.is_empty() {
                        self.call_object(bound, vec![])?;
                    } else {
                        self.call_object_kw(bound, vec![], init_sub_kwargs.clone())?;
                    }
                }
            }
            // Populate __class__ cell (PEP 3135)
            if let Some((ref cellvar_names, ref cells)) = class_cell_info {
                Self::patch_class_cell(cellvar_names, cells, &cls);
                Self::validate_class_cell(cellvar_names, cells, &cls)?;
            }
            // Call __set_name__ on descriptors in the class namespace (PEP 487)
            self.call_set_name_on_descriptors(&cls)?;

            // NamedTuple metaclass behavior
            self.process_namedtuple_class(&cls, &bases);

            // Enum metaclass behavior
            self.process_enum_class(&cls, &bases)?;
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

    fn class_cell_object(info: Option<&(Vec<CompactString>, Vec<CellRef>)>) -> Option<PyObjectRef> {
        let (cellvar_names, cells) = info?;
        for (i, name) in cellvar_names.iter().enumerate() {
            if name.as_str() == "__class__" {
                return cells.get(i).map(|cell| PyObject::cell(cell.clone()));
            }
        }
        None
    }

    fn dict_set_str(dict_obj: &PyObjectRef, name: &str, value: PyObjectRef) {
        match &dict_obj.payload {
            PyObjectPayload::Dict(map) => {
                map.write()
                    .insert(HashableKey::str_key(CompactString::from(name)), value);
            }
            PyObjectPayload::Instance(inst) => {
                if let Some(ref ds) = inst.dict_storage {
                    ds.write()
                        .insert(HashableKey::str_key(CompactString::from(name)), value);
                }
            }
            _ => {}
        }
    }

    fn dict_get_str(dict_obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
        match &dict_obj.payload {
            PyObjectPayload::Dict(map) => map
                .read()
                .get(&HashableKey::str_key(CompactString::from(name)))
                .cloned(),
            PyObjectPayload::Instance(inst) => inst.dict_storage.as_ref().and_then(|ds| {
                ds.read()
                    .get(&HashableKey::str_key(CompactString::from(name)))
                    .cloned()
            }),
            _ => None,
        }
    }

    fn validate_propagated_class_cell(
        namespace: &PyObjectRef,
        expected: Option<&PyObjectRef>,
    ) -> PyResult<()> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let Some(actual) = Self::dict_get_str(namespace, "__classcell__") else {
            return Err(PyException::runtime_error(
                "__class__ not set defining class; was __classcell__ propagated?",
            ));
        };
        match (&actual.payload, &expected.payload) {
            (PyObjectPayload::Cell(actual_cell), PyObjectPayload::Cell(expected_cell))
                if Rc::ptr_eq(actual_cell, expected_cell) =>
            {
                Ok(())
            }
            (PyObjectPayload::Cell(_), PyObjectPayload::Cell(_)) => {
                Err(PyException::type_error("__class__ set to wrong class"))
            }
            _ => Err(PyException::type_error(
                "__classcell__ must be a nonlocal cell",
            )),
        }
    }

    fn validate_class_cell(
        cellvar_names: &[CompactString],
        cells: &[CellRef],
        cls: &PyObjectRef,
    ) -> PyResult<()> {
        for (i, name) in cellvar_names.iter().enumerate() {
            if name.as_str() == "__class__" {
                let Some(cell) = cells.get(i) else {
                    return Err(PyException::runtime_error(
                        "__class__ not set defining class; was __classcell__ propagated?",
                    ));
                };
                let value = cell.read().as_ref().cloned();
                match value {
                    Some(value) if PyObjectRef::ptr_eq(&value, cls) => return Ok(()),
                    Some(_) => return Err(PyException::type_error("__class__ set to wrong class")),
                    None => {
                        return Err(PyException::runtime_error(
                            "__class__ not set defining class; was __classcell__ propagated?",
                        ))
                    }
                }
            }
        }
        Ok(())
    }

    fn apply_metaclass_mro(
        &mut self,
        cls: PyObjectRef,
        meta: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let Some(mro_method) = Self::custom_metaclass_mro(meta, &cls) else {
            return Ok(cls);
        };
        let result = self.call_object(mro_method, vec![])?;
        let mut mro_items = result.to_list()?;
        if mro_items
            .first()
            .map(|item| PyObjectRef::ptr_eq(item, &cls))
            .unwrap_or(false)
        {
            mro_items.remove(0);
        }
        if !matches!(&cls.payload, PyObjectPayload::Class(_)) {
            return Ok(cls);
        }
        // Class creation is still single-threaded here.  Update the just-created
        // class in place so any code run from metaclass.mro() observes the same
        // object that will be returned by __build_class__.
        unsafe {
            let obj = &mut *(PyObjectRef::as_ptr(&cls) as *mut PyObject);
            if let PyObjectPayload::Class(cd) = &mut obj.payload {
                cd.mro = mro_items;
                cd.method_cache.write().clear();
                cd.rebuild_vtable();
            }
        }
        Ok(cls)
    }

    fn custom_metaclass_mro(meta: &PyObjectRef, cls: &PyObjectRef) -> Option<PyObjectRef> {
        let PyObjectPayload::Class(cd) = &meta.payload else {
            return None;
        };
        cd.namespace.read().get("mro").cloned().map(|method| {
            PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: cls.clone(),
                    method,
                },
            })
        })
    }

    /// Call __set_name__ on descriptors in the class namespace (PEP 487).
    fn call_set_name_on_descriptors(&mut self, cls: &PyObjectRef) -> PyResult<()> {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let ns_snapshot: Vec<(CompactString, PyObjectRef)> = {
                let ns = cd.namespace.read();
                ns.iter()
                    .filter(|(_, v)| v.get_attr("__set_name__").is_some())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            };
            for (attr_name, attr_val) in &ns_snapshot {
                if let Some(set_name_method) = attr_val.get_attr("__set_name__") {
                    let bound = if matches!(
                        &set_name_method.payload,
                        PyObjectPayload::BoundMethod { .. }
                    ) {
                        set_name_method
                    } else {
                        PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: attr_val.clone(),
                                method: set_name_method,
                            },
                        })
                    };
                    let name_arg = PyObject::str_val(attr_name.clone());
                    if let Err(err) = self.call_object(bound, vec![cls.clone(), name_arg]) {
                        let mut wrapped = PyException::runtime_error(format!(
                            "Error calling __set_name__ on '{}'",
                            attr_name
                        ));
                        wrapped.context = Some(Box::new(err));
                        return Err(wrapped);
                    }
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

    pub(crate) fn c3_merge(
        linearizations: &mut Vec<Vec<PyObjectRef>>,
    ) -> PyResult<Vec<PyObjectRef>> {
        let mut result = Vec::new();
        // Track start index per linearization to avoid O(n) vec.remove(0)
        let mut starts: Vec<usize> = vec![0; linearizations.len()];
        loop {
            // Check if all lists are exhausted
            let any_remaining = starts
                .iter()
                .enumerate()
                .any(|(i, &s)| s < linearizations[i].len());
            if !any_remaining {
                break;
            }
            // Find a good head: first element of some list that doesn't appear in the tail of any list
            let mut found = None;
            for (i, lin) in linearizations.iter().enumerate() {
                if starts[i] >= lin.len() {
                    continue;
                }
                let candidate_ptr = PyObjectRef::as_ptr(&lin[starts[i]]);
                let in_tail = linearizations.iter().enumerate().any(|(j, other)| {
                    let s = starts[j];
                    if s >= other.len() {
                        return false;
                    }
                    other[s + 1..]
                        .iter()
                        .any(|x| PyObjectRef::as_ptr(x) == candidate_ptr)
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
                    "Cannot create a consistent method resolution order (MRO)",
                ));
            }
        }
        Ok(result)
    }
}
