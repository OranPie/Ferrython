//! VM utility functions — repr, str, sort, iteration, generators.

use crate::builtins;
use crate::frame::Frame;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{ PyCell, 
    AsyncGenAction, GeneratorState, IteratorData, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef, FxAttrMap,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

impl VirtualMachine {
    /// Install thread-local __hash__ and __eq__ dispatch callbacks for HashableKey.
    /// Called once at VM creation so all set/dict operations can resolve custom hashing.
    pub(crate) fn install_hash_eq_dispatch(&mut self) {
        let vm_ptr = self as *mut VirtualMachine;
        ferrython_core::types::set_eq_dispatch(move |a: &PyObjectRef, b: &PyObjectRef| {
            let vm = unsafe { &mut *vm_ptr };
            if let Some(eq_method) = a.get_attr("__eq__") {
                if let Ok(result) = vm.call_object(eq_method, vec![b.clone()]) {
                    return Some(result.is_truthy());
                }
            }
            None
        });

        let vm_ptr2 = self as *mut VirtualMachine;
        ferrython_core::types::set_hash_dispatch(move |obj: &PyObjectRef| {
            let vm = unsafe { &mut *vm_ptr2 };
            if let Some(hash_method) = obj.get_attr("__hash__") {
                if let Ok(result) = vm.call_object(hash_method, vec![]) {
                    return Some(result.as_int().unwrap_or(0));
                }
            }
            None
        });

        // Register VM call dispatch so NativeClosures can call Python functions
        let vm_ptr3 = self as *mut VirtualMachine;
        ferrython_core::object::register_vm_call_dispatch(move |func: PyObjectRef, args: Vec<PyObjectRef>| {
            let vm = unsafe { &mut *vm_ptr3 };
            vm.call_object(func, args)
        });
    }

    pub(crate) fn is_exception_class(cls: &PyObjectRef) -> bool {
        if matches!(&cls.payload, PyObjectPayload::ExceptionType(_)) {
            return true;
        }
        if let PyObjectPayload::Class(cd) = &cls.payload {
            // Check if any base is an ExceptionType or an exception class
            for base in &cd.bases {
                if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                    return true;
                }
                if Self::is_exception_class(base) {
                    return true;
                }
            }
        }
        false
    }

    /// Resolve a dunder method on an Instance, skipping BuiltinBoundMethod
    /// (which comes from BuiltinType bases like list/dict and can't be called).
    /// Returns the method if it's a real callable (BoundMethod, Function, etc.).
    pub(crate) fn resolve_instance_dunder(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            // Search the class's own namespace and MRO bases for this dunder.
            // We need to walk MRO ourselves so we can detect descriptors that
            // require __get__ invocation (e.g. _ProxyLookup in werkzeug).
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                // Check class's own namespace first
                if let Some(class_val) = cd.namespace.read().get(name).cloned() {
                    return Some(Self::bind_class_val_for_instance(obj, inst, class_val));
                }
                // Walk MRO bases
                for base in &cd.mro {
                    if let PyObjectPayload::Class(bcd) = &base.payload {
                        if let Some(class_val) = bcd.namespace.read().get(name).cloned() {
                            return Some(Self::bind_class_val_for_instance(obj, inst, class_val));
                        }
                    }
                }
            }
            // Also check instance attrs directly (Python-defined __str__/__repr__)
            if let Some(method) = inst.attrs.read().get(name).cloned() {
                return Some(method);
            }
        }
        // For non-Instance payloads, just try get_attr (skipping BuiltinBoundMethod)
        if let Some(method) = obj.get_attr(name) {
            if matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                return None;
            }
            return Some(method);
        }
        None
    }

    /// Bind a class-level attribute for instance access: wrap functions as BoundMethod,
    /// and leave descriptors (Instance with __get__) as-is for the VM to invoke __get__.
    fn bind_class_val_for_instance(obj: &PyObjectRef, inst: &ferrython_core::object::InstanceData, class_val: PyObjectRef) -> PyObjectRef {
        match &class_val.payload {
            PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction(_) => {
                PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: obj.clone(),
                        method: class_val,
                    }
                })
            }
            PyObjectPayload::StaticMethod(func) => func.clone(),
            PyObjectPayload::ClassMethod(func) => PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: inst.class.clone(),
                    method: func.clone(),
                }
            }),
            // For Instance values (including descriptors like _ProxyLookup),
            // return raw — caller must invoke __get__ via the VM if needed.
            _ => class_val,
        }
    }

    /// Invoke __get__ on a descriptor to get the actual callable.
    /// Returns the original value if it's not a descriptor.
    pub(crate) fn resolve_descriptor(&mut self, val: &PyObjectRef, instance: &PyObjectRef) -> PyResult<PyObjectRef> {
        use ferrython_core::object::has_descriptor_get;
        if has_descriptor_get(val) {
            if let Some(get_method) = val.get_attr("__get__") {
                let owner = if let PyObjectPayload::Instance(inst) = &instance.payload {
                    inst.class.clone()
                } else {
                    PyObject::none()
                };
                let bound = if matches!(&get_method.payload, PyObjectPayload::BoundMethod { .. }) {
                    get_method
                } else {
                    PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: val.clone(),
                            method: get_method,
                        }
                    })
                };
                return self.call_object(bound, vec![instance.clone(), owner]);
            }
        }
        Ok(val.clone())
    }

    /// Get the __builtin_value__ from an Instance (for builtin type subclasses).
    pub(crate) fn get_builtin_value(obj: &PyObjectRef) -> Option<PyObjectRef> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            return inst.attrs.read().get("__builtin_value__").cloned();
        }
        None
    }

    pub(crate) fn vm_str(&mut self, obj: &PyObjectRef) -> PyResult<String> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                // Check for custom __str__ (skip BuiltinBoundMethod from builtin bases)
                if let Some(str_method) = Self::resolve_instance_dunder(obj, "__str__") {
                    let method = self.resolve_descriptor(&str_method, obj)?;
                    let args = match &method.payload {
                        PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) => vec![obj.clone()],
                        _ => vec![],
                    };
                    let result = self.call_object(method, args)?;
                    return Ok(result.py_to_string());
                }
                // Fall back to __repr__ before __builtin_value__: namedtuples, dataclasses, etc.
                // define custom __repr__ that should serve as str() too.
                if let Some(repr_method) = Self::resolve_instance_dunder(obj, "__repr__") {
                    let method = self.resolve_descriptor(&repr_method, obj)?;
                    let args = match &method.payload {
                        PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) => vec![obj.clone()],
                        _ => vec![],
                    };
                    let result = self.call_object(method, args)?;
                    return Ok(result.py_to_string());
                }
                // namedtuple: use BuiltinBoundMethod __str__ (dispatches to call_namedtuple_method)
                if inst.class.get_attr("__namedtuple__").is_some() {
                    if let Some(str_method) = obj.get_attr("__str__") {
                        let result = self.call_object(str_method, vec![])?;
                        return Ok(result.py_to_string());
                    }
                }
                // Builtin base type subclass: delegate to __builtin_value__
                if let Some(bv) = Self::get_builtin_value(obj) {
                    return self.vm_str(&bv);
                }
                // Exception instances: str(e) returns the message from args
                if let Some(args) = obj.get_attr("args") {
                    if let PyObjectPayload::Tuple(items) = &args.payload {
                        return match items.len() {
                            0 => Ok(String::new()),
                            1 => Ok(items[0].py_to_string()),
                            _ => self.vm_repr(&args),
                        };
                    }
                }
                // Fall back to vm_repr for dataclass/namedtuple auto-repr and generic display
                self.vm_repr(obj)
            }
            // For containers, str() is same as repr() (elements use repr)
            PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) |
            PyObjectPayload::Dict(_) | PyObjectPayload::Set(_) |
            PyObjectPayload::FrozenSet(_) => self.vm_repr(obj),
            _ => Ok(obj.py_to_string()),
        }
    }

    /// VM-aware conversion of an object to string for str.format() placeholders.
    /// Uses __str__ protocol for instances (dispatches through VM), falls back to py_to_string.
    fn vm_format_obj_str(&mut self, val: &PyObjectRef) -> PyResult<String> {
        self.vm_str(val)
    }

    fn vm_format_obj_repr(&mut self, val: &PyObjectRef) -> PyResult<String> {
        self.vm_repr(val)
    }

    /// Format a single replacement field value with optional conversion and format spec.
    fn vm_format_field(&mut self, val: &PyObjectRef, conversion: Option<&str>, spec: Option<&str>) -> PyResult<String> {
        match conversion {
            Some("r") | Some("a") => {
                let text = self.vm_format_obj_repr(val)?;
                Ok(match spec {
                    Some(s) if !s.is_empty() => crate::builtins::apply_format_spec_str(&text, s),
                    _ => text,
                })
            }
            Some("s") => {
                let text = self.vm_format_obj_str(val)?;
                Ok(match spec {
                    Some(s) if !s.is_empty() => crate::builtins::apply_format_spec_str(&text, s),
                    _ => text,
                })
            }
            _ => {
                // No conversion — apply format spec to the raw value (not str())
                match spec {
                    Some(s) if !s.is_empty() => {
                        // For instances, check __format__ first
                        if matches!(&val.payload, PyObjectPayload::Instance(_)) {
                            if let Some(format_method) = val.get_attr("__format__") {
                                let spec_obj = PyObject::str_val(CompactString::from(s));
                                let r = self.call_object(format_method, vec![spec_obj])?;
                                return Ok(r.py_to_string());
                            }
                        }
                        // Use the core format_value which handles int/float format specs
                        match val.format_value(s) {
                            Ok(formatted) => Ok(formatted),
                            Err(_) => Ok(self.vm_format_obj_str(val)?),
                        }
                    }
                    _ => self.vm_format_obj_str(val),
                }
            }
        }
    }

    /// Parse a field spec like "name!r:>10" into (field_name, conversion, format_spec).
    fn parse_format_field(field_spec: &str) -> (&str, Option<&str>, Option<&str>) {
        let (field_part, format_spec) = if let Some(cp) = field_spec.find(':') {
            (&field_spec[..cp], Some(&field_spec[cp+1..]))
        } else {
            (field_spec, None)
        };
        let (field_name, conversion) = if let Some(bp) = field_part.find('!') {
            (&field_part[..bp], Some(&field_part[bp+1..]))
        } else {
            (field_part, None)
        };
        (field_name, conversion, format_spec)
    }

    /// VM-aware str.format() with positional args only.
    pub(crate) fn vm_str_format(&mut self, fmt: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let mut result = String::new();
        let mut chars = fmt.chars().peekable();
        let mut auto_idx = 0usize;
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    result.push('{');
                } else {
                    let mut field_spec = String::new();
                    let mut depth = 1;
                    for c in chars.by_ref() {
                        if c == '{' { depth += 1; }
                        else if c == '}' { depth -= 1; if depth == 0 { break; } }
                        field_spec.push(c);
                    }
                    let (field_name, conversion, format_spec) = Self::parse_format_field(&field_spec);
                    // Resolve value
                    let val = if field_name.is_empty() {
                        let v = args.get(auto_idx).cloned();
                        auto_idx += 1;
                        v
                    } else if let Ok(idx) = field_name.parse::<usize>() {
                        args.get(idx).cloned()
                    } else {
                        // Attribute/item access: "obj.attr" or "obj[key]"
                        self.resolve_format_field(field_name, args, auto_idx, &[])
                    };
                    if let Some(val) = val {
                        result.push_str(&self.vm_format_field(&val, conversion, format_spec)?);
                    }
                }
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                result.push('}');
            } else {
                result.push(c);
            }
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }

    /// VM-aware str.format() with keyword args.
    pub(crate) fn vm_str_format_kw(
        &mut self, fmt: &str,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let mut result = String::new();
        let mut chars = fmt.chars().peekable();
        let mut auto_idx = 0usize;
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    result.push('{');
                } else {
                    let mut field_spec = String::new();
                    let mut depth = 1;
                    for c in chars.by_ref() {
                        if c == '{' { depth += 1; }
                        else if c == '}' { depth -= 1; if depth == 0 { break; } }
                        field_spec.push(c);
                    }
                    // Resolve nested braces in format spec
                    let resolved_spec = self.resolve_nested_spec(&field_spec, pos_args, kwargs);
                    let spec_str = resolved_spec.as_deref().unwrap_or(&field_spec);
                    let (field_name, conversion, format_spec) = Self::parse_format_field(spec_str);
                    // Resolve value
                    let val = if field_name.is_empty() {
                        let v = pos_args.get(auto_idx).cloned();
                        auto_idx += 1;
                        v
                    } else if let Ok(idx) = field_name.parse::<usize>() {
                        pos_args.get(idx).cloned()
                    } else {
                        kwargs.iter().find(|(k, _)| k.as_str() == field_name).map(|(_, v)| v.clone())
                    };
                    if let Some(val) = val {
                        result.push_str(&self.vm_format_field(&val, conversion, format_spec)?);
                    }
                }
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                result.push('}');
            } else {
                result.push(c);
            }
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }

    /// Resolve nested {name}/{idx} references inside a format spec.
    fn resolve_nested_spec(
        &mut self, spec: &str,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> Option<String> {
        if !spec.contains('{') { return None; }
        // Only resolve nested refs in the format_spec part (after ':')
        if let Some(colon_pos) = spec.find(':') {
            let format_part = &spec[colon_pos+1..];
            if !format_part.contains('{') { return None; }
            let mut r = spec[..=colon_pos].to_string();
            let mut sc = format_part.chars().peekable();
            while let Some(ch) = sc.next() {
                if ch == '{' {
                    let mut ref_name = String::new();
                    for ch in sc.by_ref() {
                        if ch == '}' { break; }
                        ref_name.push(ch);
                    }
                    if let Ok(idx) = ref_name.parse::<usize>() {
                        if let Some(v) = pos_args.get(idx) {
                            r.push_str(&v.py_to_string());
                        }
                    } else if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == ref_name) {
                        r.push_str(&v.py_to_string());
                    }
                } else {
                    r.push(ch);
                }
            }
            Some(r)
        } else {
            None
        }
    }

    /// Resolve attribute/item access in format fields like "{0.name}" or "{obj.attr}".
    fn resolve_format_field(
        &mut self, field_name: &str,
        pos_args: &[PyObjectRef],
        auto_idx: usize,
        _kwargs: &[(CompactString, PyObjectRef)],
    ) -> Option<PyObjectRef> {
        // Parse base name: everything before first '.' or '['
        let base_end = field_name.find(|c: char| c == '.' || c == '[').unwrap_or(field_name.len());
        let base = &field_name[..base_end];
        let rest = &field_name[base_end..];

        let mut current = if let Ok(idx) = base.parse::<usize>() {
            pos_args.get(idx)?.clone()
        } else if base.is_empty() {
            pos_args.get(auto_idx)?.clone()
        } else {
            return None;
        };

        // Process accessor chain: .attr and [key] in sequence
        let mut chars = rest.chars().peekable();
        while let Some(&c) = chars.peek() {
            if c == '.' {
                chars.next();
                let mut attr = String::new();
                while let Some(&nc) = chars.peek() {
                    if nc == '.' || nc == '[' { break; }
                    attr.push(nc);
                    chars.next();
                }
                current = current.get_attr(&attr)?;
            } else if c == '[' {
                chars.next();
                let mut key = String::new();
                for nc in chars.by_ref() {
                    if nc == ']' { break; }
                    key.push(nc);
                }
                if let Ok(idx) = key.parse::<i64>() {
                    let key_obj = PyObject::int(idx);
                    current = current.get_item(&key_obj).ok()?;
                } else {
                    let key_obj = PyObject::str_val(CompactString::from(&key));
                    current = current.get_item(&key_obj).ok()?;
                }
            } else {
                break;
            }
        }

        Some(current)
    }

    /// Call close() on an object through normal VM dispatch (used by contextlib.closing).
    pub(crate) fn call_close_on(&mut self, obj: &PyObjectRef) -> PyResult<()> {
        if let Some(close_fn) = obj.get_attr("close") {
            if matches!(&close_fn.payload, PyObjectPayload::BoundMethod { .. }) {
                let _ = self.call_object(close_fn, vec![])?;
            } else {
                let bound = PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: obj.clone(),
                        method: close_fn,
                    }
                });
                let _ = self.call_object(bound, vec![])?;
            }
        }
        Ok(())
    }

    /// Produce a repr string for an object, dispatching __repr__ on instances.
    pub(crate) fn vm_repr(&mut self, obj: &PyObjectRef) -> PyResult<String> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                // Check for custom __repr__ (skip BuiltinBoundMethod from builtin bases)
                if let Some(repr_method) = Self::resolve_instance_dunder(obj, "__repr__") {
                    // If it's a descriptor (Instance with __get__), invoke __get__
                    let method = self.resolve_descriptor(&repr_method, obj)?;
                    let args = match &method.payload {
                        PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) => vec![obj.clone()],
                        _ => vec![],
                    };
                    let result = self.call_object(method, args)?;
                    return Ok(result.py_to_string());
                }
                // Dataclass auto-repr (before __builtin_value__ delegation)
                let class = &inst.class;
                if matches!(&class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__dataclass__")) {
                    if let Some(fields) = class.get_attr("__dataclass_fields__") {
                        let field_names = crate::vm_dataclass_utils::extract_field_names(&fields);
                        if !field_names.is_empty() {
                            let class_name = if let PyObjectPayload::Class(cd) = &class.payload {
                                cd.name.to_string()
                            } else { "?".to_string() };
                            let mut parts = Vec::new();
                            let attrs = inst.attrs.read();
                            for name in &field_names {
                                if let Some(val) = attrs.get(name.as_str()) {
                                    let val_repr = self.vm_repr(val)?;
                                    parts.push(format!("{}={}", name, val_repr));
                                }
                            }
                            return Ok(format!("{}({})", class_name, parts.join(", ")));
                        }
                    }
                }
                // Namedtuple auto-repr (before __builtin_value__ delegation)
                if matches!(&class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__namedtuple__")) {
                    if let Some(fields) = class.get_attr("_fields") {
                        if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                            let class_name = if let PyObjectPayload::Class(cd) = &class.payload {
                                cd.name.to_string()
                            } else { "?".to_string() };
                            let mut parts = Vec::new();
                            let attrs = inst.attrs.read();
                            for field in field_names {
                                let name = field.py_to_string();
                                if let Some(val) = attrs.get(name.as_str()) {
                                    let val_repr = self.vm_repr(val)?;
                                    parts.push(format!("{}={}", name, val_repr));
                                }
                            }
                            return Ok(format!("{}({})", class_name, parts.join(", ")));
                        }
                    }
                }
                // Builtin base type subclass: delegate to __builtin_value__
                if let Some(bv) = Self::get_builtin_value(obj) {
                    return self.vm_repr(&bv);
                }
                Ok(obj.repr())
            }
            PyObjectPayload::List(items) => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                if !ferrython_core::object::repr_enter(ptr) { return Ok("[...]".to_string()); }
                let items = items.read().clone();
                let mut parts = Vec::new();
                for item in &items {
                    parts.push(self.vm_repr(item)?);
                }
                ferrython_core::object::repr_leave(ptr);
                Ok(format!("[{}]", parts.join(", ")))
            }
            PyObjectPayload::Tuple(items) => {
                let mut parts = Vec::new();
                for item in items {
                    parts.push(self.vm_repr(item)?);
                }
                if parts.len() == 1 {
                    Ok(format!("({},)", parts[0]))
                } else {
                    Ok(format!("({})", parts.join(", ")))
                }
            }
            PyObjectPayload::Dict(m) => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                if !ferrython_core::object::repr_enter(ptr) { return Ok("{...}".to_string()); }
                let m = m.read().clone();
                let mut parts = Vec::new();
                for (k, v) in &m {
                    if ferrython_core::object::is_hidden_dict_key(k) { continue; }
                    let kr = self.vm_repr(&k.to_object())?;
                    let vr = self.vm_repr(v)?;
                    parts.push(format!("{}: {}", kr, vr));
                }
                ferrython_core::object::repr_leave(ptr);
                Ok(format!("{{{}}}", parts.join(", ")))
            }
            PyObjectPayload::Set(m) => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                if !ferrython_core::object::repr_enter(ptr) { return Ok("set(...)".to_string()); }
                let m = m.read().clone();
                if m.is_empty() { ferrython_core::object::repr_leave(ptr); return Ok("set()".to_string()); }
                let mut parts = Vec::new();
                for v in m.values() {
                    parts.push(self.vm_repr(v)?);
                }
                ferrython_core::object::repr_leave(ptr);
                Ok(format!("{{{}}}", parts.join(", ")))
            }
            _ => Ok(obj.repr()),
        }
    }

    /// Convert a Python object to a HashableKey, calling __hash__/__eq__ on instances.
    /// Dispatches are installed at VM init, so from_object will use them automatically.
    pub(crate) fn vm_to_hashable_key(&mut self, obj: &PyObjectRef) -> PyResult<HashableKey> {
        obj.to_hashable_key()
    }

    /// Call a Python object (function, builtin, class).
    pub(crate) fn vm_functools_reduce(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("reduce() requires at least 2 arguments")); }
        let func = args[0].clone();
        let items = self.collect_iterable(&args[1])?;
        let has_initial = args.len() > 2;
        let mut acc = if has_initial {
            args[2].clone()
        } else if !items.is_empty() {
            items[0].clone()
        } else {
            return Err(PyException::type_error("reduce() of empty sequence with no initial value"));
        };
        let start_idx = if has_initial { 0 } else { 1 };
        for item in &items[start_idx..] {
            acc = self.call_object(func.clone(), vec![acc, item.clone()])?;
        }
        Ok(acc)
    }

    /// VM-level singledispatch call: dispatch based on first arg's type
    pub(crate) fn vm_singledispatch_call_instance(&mut self, dispatcher: &PyObjectRef, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("singledispatch function requires at least 1 argument"));
        }
        let type_name_str = args[0].type_name();
        let default = dispatcher.get_attr("__default__")
            .ok_or_else(|| PyException::runtime_error("singledispatch: no default function"))?;
        let registry = dispatcher.get_attr("__registry__");

        // Look up handler by type name in registry
        let handler = if let Some(ref reg) = registry {
            if let PyObjectPayload::Dict(ref map) = reg.payload {
                let m = map.read();
                m.get(&HashableKey::str_key(CompactString::from(&*type_name_str)))
                    .cloned()
                    .unwrap_or_else(|| default.clone())
            } else {
                default.clone()
            }
        } else {
            default.clone()
        };

        self.call_object(handler, args.to_vec())
    }

    /// VM-level singledispatch.register: register(type) returns decorator
    pub(crate) fn vm_singledispatch_register(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        // args[0] = self (dispatcher), args[1] = type, args[2..] = optional func
        if args.len() < 2 {
            return Err(PyException::type_error("register() requires a type argument"));
        }
        let dispatcher = args[0].clone();
        let type_obj = &args[1];
        // Extract the actual type name: check for __name__ first (for class types),
        // then fall back to py_to_string
        let type_name = type_obj.get_attr("__name__")
            .map(|n| n.py_to_string().to_string())
            .unwrap_or_else(|| {
                let s = type_obj.py_to_string().to_string();
                // Strip "<class '...'>" wrapper if present
                if s.starts_with("<class '") && s.ends_with("'>") {
                    s[8..s.len()-2].to_string()
                } else {
                    s
                }
            });

        if args.len() >= 3 {
            // register(type, func) — direct registration
            let func = args[2].clone();
            if let Some(reg) = dispatcher.get_attr("__registry__") {
                if let PyObjectPayload::Dict(ref map) = reg.payload {
                    map.write().insert(HashableKey::str_key(CompactString::from(&*type_name)), func.clone());
                }
            }
            return Ok(func);
        }

        // register(type) → return decorator closure that captures dispatcher + type_name
        let tn = type_name.to_string();
        Ok(PyObject::native_closure(
            "singledispatch.register_decorator",
            move |deco_args| {
                if deco_args.is_empty() {
                    return Err(PyException::type_error("register decorator requires 1 argument"));
                }
                let func = deco_args[0].clone();
                if let Some(reg) = dispatcher.get_attr("__registry__") {
                    if let PyObjectPayload::Dict(ref map) = reg.payload {
                        map.write().insert(HashableKey::str_key(CompactString::from(&tn)), func.clone());
                    }
                }
                Ok(func)
            },
        ))
    }

    /// VM-level itertools.islice: lazily takes items from any iterable (including generators).
    pub(crate) fn vm_itertools_islice(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("islice() requires at least 2 arguments"));
        }
        let iterable = &args[0];
        // None stop means no limit (use usize::MAX as sentinel)
        let (start, stop, step) = if args.len() == 2 {
            let stop = if matches!(&args[1].payload, PyObjectPayload::None) { usize::MAX } else { args[1].to_int()? as usize };
            (0usize, stop, 1usize)
        } else if args.len() == 3 {
            let s = if matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { args[1].to_int()? as usize };
            let stop = if matches!(&args[2].payload, PyObjectPayload::None) { usize::MAX } else { args[2].to_int()? as usize };
            (s, stop, 1usize)
        } else {
            let s = if matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { args[1].to_int()? as usize };
            let stop = if matches!(&args[2].payload, PyObjectPayload::None) { usize::MAX } else { args[2].to_int()? as usize };
            let st = if matches!(&args[3].payload, PyObjectPayload::None) { 1 } else { args[3].to_int()? as usize };
            (s, stop, st.max(1))
        };

        // For generators: consume items one at a time, only up to `stop`
        if let PyObjectPayload::Generator(gen_arc) = &iterable.payload {
            let gen_arc = gen_arc.clone();
            let mut result = Vec::new();
            let mut idx = 0usize;
            let mut next_yield = start;
            loop {
                if result.len() >= stop.saturating_sub(start) { break; }
                if idx >= stop { break; }
                match self.resume_generator(&gen_arc, PyObject::none()) {
                    Ok(value) => {
                        if idx == next_yield {
                            result.push(value);
                            next_yield += step;
                        }
                        idx += 1;
                    }
                    Err(e) if e.kind == ExceptionKind::StopIteration => break,
                    Err(e) => return Err(e),
                }
            }
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Rc::new(PyCell::new(IteratorData::List { items: result, index: 0 }))
            )));
        }

        // For iterators with lazy data: advance one at a time
        if let PyObjectPayload::Iterator(_) | PyObjectPayload::RangeIter { .. } = &iterable.payload {
            let mut result = Vec::new();
            let mut idx = 0usize;
            let mut next_yield = start;
            loop {
                if idx >= stop { break; }
                match self.advance_lazy_iterator(iterable) {
                    Ok(Some(value)) => {
                        if idx == next_yield {
                            result.push(value);
                            next_yield += step;
                        }
                        idx += 1;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        // Try non-lazy advance
                        match builtins::iter_advance(iterable) {
                            Ok(Some((_, value))) => {
                                if idx == next_yield {
                                    result.push(value);
                                    next_yield += step;
                                }
                                idx += 1;
                            }
                            Ok(None) => break,
                            Err(_) => return Err(e),
                        }
                    }
                }
            }
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Rc::new(PyCell::new(IteratorData::List { items: result, index: 0 }))
            )));
        }

        // For Instance with __iter__/__next__: iterate through VM
        if let PyObjectPayload::Instance(_) = &iterable.payload {
            if let Some(iter_method) = iterable.get_attr("__iter__") {
                let iter_obj = self.call_object(iter_method, vec![])?;
                // Recurse with the iterator
                let mut new_args = args.to_vec();
                new_args[0] = iter_obj;
                return self.vm_itertools_islice(&new_args);
            }
        }

        // Fallback: eagerly collect then slice (works for lists, tuples, etc.)
        let items = iterable.to_list()?;
        let result: Vec<PyObjectRef> = items.into_iter()
            .skip(start)
            .take(stop.saturating_sub(start))
            .step_by(step)
            .collect();
        Ok(PyObject::wrap(PyObjectPayload::Iterator(
            Rc::new(PyCell::new(IteratorData::List { items: result, index: 0 }))
        )))
    }

    /// Resolve an iterable object to its iterator by calling __iter__ if needed.
    /// For Instance objects with __iter__, calls __iter__() to get the real iterator.
    /// For builtin types (list, tuple, etc.), delegates to get_iter_from_obj.
    pub(crate) fn resolve_iterable(&mut self, obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            // dict subclass or namedtuple: use core get_iter
            if inst.dict_storage.is_some() || inst.class.get_attr("__namedtuple__").is_some() {
                return obj.get_iter();
            }
            // Custom __iter__: call it to get the actual iterator
            if let Some(iter_method) = obj.get_attr("__iter__") {
                let result = self.call_object(iter_method, vec![])?;
                // If __iter__ returns a raw Tuple/List, convert to proper iterator
                match &result.payload {
                    PyObjectPayload::Tuple(_) | PyObjectPayload::List(_) => {
                        return builtins::get_iter_from_obj_pub(&result);
                    }
                    _ => return Ok(result),
                }
            }
            // Builtin base type subclass: delegate to __builtin_value__
            if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                return bv.get_iter();
            }
            // Has __getitem__: return obj itself for sequence protocol
            if obj.get_attr("__getitem__").is_some() {
                return Ok(obj.clone());
            }
            return Err(PyException::type_error(format!(
                "'{}' object is not iterable", obj.type_name()
            )));
        }
        builtins::get_iter_from_obj_pub(obj)
    }

    /// Resolve a slice of iterables, calling __iter__ on Instance objects.
    pub(crate) fn resolve_iterables(&mut self, args: &[PyObjectRef]) -> PyResult<Vec<PyObjectRef>> {
        args.iter().map(|a| self.resolve_iterable(a)).collect()
    }

    /// Collect all items from any iterable (list, tuple, generator, instance with __iter__/__next__).
    pub(crate) fn collect_iterable(&mut self, obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
        match &obj.payload {
            PyObjectPayload::Generator(gen_arc) => {
                let gen_arc = gen_arc.clone();
                let mut items = Vec::new();
                loop {
                    match self.resume_generator(&gen_arc, PyObject::none()) {
                        Ok(value) => items.push(value),
                        Err(e) if e.kind == ExceptionKind::StopIteration => break,
                        Err(e) => return Err(e),
                    }
                }
                Ok(items)
            }
            PyObjectPayload::Instance(inst) => {
                // Dict subclass: iterate over keys
                if let Some(ref ds) = inst.dict_storage {
                    return Ok(ds.read().keys().map(|k| k.to_object()).collect());
                }
                if let Some(iter_method) = obj.get_attr("__iter__") {
                    let iter_obj = self.call_object(iter_method, vec![])?;
                    // If __iter__ returned a list/tuple, convert directly
                    if matches!(&iter_obj.payload, PyObjectPayload::List(_) | PyObjectPayload::Tuple(_)) {
                        return iter_obj.to_list();
                    }
                    // If __iter__ returned a builtin Iterator, use iter_advance
                    if matches!(&iter_obj.payload, PyObjectPayload::Iterator(_) | PyObjectPayload::RangeIter { .. }) {
                        let mut items = Vec::new();
                        loop {
                            match builtins::iter_advance(&iter_obj)? {
                                Some((_new_iter, value)) => items.push(value),
                                None => break,
                            }
                        }
                        return Ok(items);
                    }
                    // If it returned a generator, collect from it
                    if let PyObjectPayload::Generator(gen_arc) = &iter_obj.payload {
                        let gen_arc = gen_arc.clone();
                        let mut items = Vec::new();
                        loop {
                            match self.resume_generator(&gen_arc, PyObject::none()) {
                                Ok(value) => items.push(value),
                                Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                Err(e) => return Err(e),
                            }
                        }
                        return Ok(items);
                    }
                    // Otherwise, it's an instance with __next__
                    let mut items = Vec::new();
                    loop {
                        if let Some(next_method) = iter_obj.get_attr("__next__") {
                            match self.call_object(next_method.clone(), vec![]) {
                                Ok(value) => items.push(value),
                                Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                Err(e) => return Err(e),
                            }
                        } else { break; }
                    }
                    Ok(items)
                } else if let Some(getitem) = obj.get_attr("__getitem__") {
                    // Fall back to __getitem__-based iteration (old-style sequence protocol)
                    let mut items = Vec::new();
                    let mut idx: i64 = 0;
                    loop {
                        match self.call_object(getitem.clone(), vec![PyObject::int(idx)]) {
                            Ok(val) => { items.push(val); idx += 1; }
                            Err(e) if e.kind == ExceptionKind::IndexError => break,
                            Err(e) => return Err(e),
                        }
                    }
                    Ok(items)
                } else {
                    obj.to_list()
                }
            }
            PyObjectPayload::Iterator(iter_data_arc) => {
                // Check for lazy iterators that need VM context
                let is_lazy = {
                    let data = iter_data_arc.read();
                    matches!(&*data, IteratorData::Enumerate { .. }
                        | IteratorData::Zip { .. }
                        | IteratorData::Map { .. }
                        | IteratorData::Filter { .. }
                        | IteratorData::Sentinel { .. }
                        | IteratorData::TakeWhile { .. }
                        | IteratorData::DropWhile { .. }
                        | IteratorData::Count { .. }
                        | IteratorData::Cycle { .. }
                        | IteratorData::Repeat { .. }
                        | IteratorData::Chain { .. }
                        | IteratorData::Starmap { .. })
                };
                if is_lazy {
                    let mut items = Vec::new();
                    loop {
                        match self.advance_lazy_iterator(obj)? {
                            Some(value) => items.push(value),
                            None => break,
                        }
                    }
                    Ok(items)
                } else {
                    // Standard iterators — use iter_advance
                    let mut items = Vec::new();
                    loop {
                        match builtins::iter_advance(obj)? {
                            Some((_new_iter, value)) => items.push(value),
                            None => break,
                        }
                    }
                    Ok(items)
                }
            }
            PyObjectPayload::Class(_) => {
                // Class with __iter__ (e.g. Enum): call __iter__(cls)
                if let Some(iter_method) = obj.get_attr("__iter__") {
                    let result = self.call_object(iter_method, vec![obj.clone()])?;
                    return self.collect_iterable(&result);
                }
                Err(PyException::type_error(format!(
                    "'type' object is not iterable"
                )))
            }
            // Module with __iter__/__next__ (e.g. file objects created as module_with_attrs)
            PyObjectPayload::Module(_) => {
                // Module.get_attr returns raw NativeFunction (no BoundMethod wrapping),
                // so we must pass obj as self explicitly.
                if let Some(next_fn) = obj.get_attr("__next__") {
                    // Fast path: directly iterate via __next__ (file objects return self from __iter__)
                    let mut items = Vec::new();
                    loop {
                        match self.call_object(next_fn.clone(), vec![obj.clone()]) {
                            Ok(value) => items.push(value),
                            Err(e) if e.kind == ExceptionKind::StopIteration => break,
                            Err(e) => return Err(e),
                        }
                    }
                    return Ok(items);
                }
                if let Some(iter_fn) = obj.get_attr("__iter__") {
                    let iter_obj = self.call_object(iter_fn, vec![obj.clone()])?;
                    if !PyObjectRef::ptr_eq(&iter_obj, obj) {
                        return self.collect_iterable(&iter_obj);
                    }
                }
                Err(PyException::type_error(format!(
                    "'module' object is not iterable"
                )))
            }
            _ => obj.to_list(),
        }
    }

    /// Resume a generator, pushing the given `send_value` onto its stack and running
    /// until the next `YieldValue` or `ReturnValue`.
    /// Returns `Ok(value)` for yielded values, or `Err(StopIteration)` when done.
    pub(crate) fn resume_generator(
        &mut self,
        gen_arc: &Rc<PyCell<GeneratorState>>,
        send_value: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let mut gen = gen_arc.write();
        if gen.finished {
            return Err(PyException::new(ExceptionKind::StopIteration, ""));
        }
        let mut frame = match gen.frame.take() {
            Some(f) => *f.downcast::<Frame>().expect("generator frame downcast"),
            None => return Err(PyException::runtime_error("generator already executing")),
        };

        // If generator was already started, push the send value onto the frame's stack
        // (it becomes the result of the `yield` expression)
        if gen.started {
            frame.push(send_value);
        }
        gen.started = true;
        drop(gen); // release lock before executing

        self.call_stack.push(frame);
        let result = self.run_frame();
        let frame = self.call_stack.pop().unwrap();

        let mut gen = gen_arc.write();
        if frame.yielded {
            // Generator yielded — save frame for later resumption
            let mut saved_frame = frame;
            saved_frame.yielded = false;
            gen.frame = Some(Box::new(saved_frame));
            result // Ok(yielded_value)
        } else {
            // Generator finished (returned or raised)
            gen.finished = true;
            gen.frame = None;
            match result {
                Ok(return_val) => {
                    // Normal return → StopIteration with return value
                    let msg = return_val.py_to_string();
                    let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                    exc.value = Some(return_val);
                    Err(exc)
                }
                Err(e) => Err(e), // Propagate the actual exception
            }
        }
    }

    /// Throw an exception into a generator.
    /// Resumes the generator with an exception injected at the yield point.
    pub(crate) fn gen_throw(
        &mut self,
        gen_arc: &Rc<PyCell<GeneratorState>>,
        kind: ExceptionKind,
        msg: CompactString,
    ) -> PyResult<PyObjectRef> {
        self.gen_throw_with_value(gen_arc, kind, msg, None)
    }

    /// Like gen_throw but preserves an original exception value for identity-
    /// preserving re-raise (needed by contextlib._GeneratorContextManager.__exit__
    /// which does `exc is not value`).
    pub(crate) fn gen_throw_with_value(
        &mut self,
        gen_arc: &Rc<PyCell<GeneratorState>>,
        kind: ExceptionKind,
        msg: CompactString,
        original_value: Option<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        let mut gen = gen_arc.write();
        if gen.finished {
            return Err(PyException::new(kind, msg));
        }
        let frame = match gen.frame.take() {
            Some(f) => *f.downcast::<Frame>().expect("generator frame downcast"),
            None => return Err(PyException::runtime_error("generator already executing")),
        };
        gen.started = true;
        drop(gen);

        // Set up exception on the frame so VM will unwind to handler
        let mut exc = PyException::new(kind, msg.clone());
        // Preserve original exception object for identity-preserving re-raise
        if let Some(ref orig) = original_value {
            exc.original = Some(orig.clone());
        }
        self.call_stack.push(frame);
        let exc_result = Err(exc);
        // Use original value if provided (for identity-preserving throws)
        let exc_obj = original_value.clone()
            .unwrap_or_else(|| PyObject::exception_instance(kind, msg.clone()));
        let exc_type = PyObject::exception_type(kind);
        let tb = PyObject::none();

        // Try to find an exception handler in the generator's frame
        if let Some(handler_ip) = self.unwind_except() {
            let mut active = PyException::new(kind, msg);
            if let Some(ref orig) = original_value {
                active.original = Some(orig.clone());
            }
            self.active_exception = Some(active);
            let frame_ref = self.call_stack.last_mut().unwrap();
            frame_ref.push(tb);
            frame_ref.push(exc_obj);
            frame_ref.push(exc_type);
            frame_ref.ip = handler_ip;

            let result = self.run_frame();
            let frame = self.call_stack.pop().unwrap();

            let mut gen = gen_arc.write();
            if frame.yielded {
                let mut saved_frame = frame;
                saved_frame.yielded = false;
                gen.frame = Some(Box::new(saved_frame));
                result
            } else {
                gen.finished = true;
                gen.frame = None;
                // If the generator raised an exception (not caught), re-raise it
                // instead of converting to StopIteration.
                if let Err(e) = result {
                    return Err(e);
                }
                let return_val = result.ok();
                let msg = return_val.as_ref().map(|v| v.py_to_string()).unwrap_or_default();
                let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                exc.value = return_val;
                Err(exc)
            }
        } else {
            // No handler — pop frame and re-raise
            self.call_stack.pop();
            let mut gen = gen_arc.write();
            gen.finished = true;
            gen.frame = None;
            exc_result
        }
    }

    /// Parse the arguments to generator.throw() / coroutine.throw() into (ExceptionKind, message).
    pub(crate) fn parse_throw_args(args: &[PyObjectRef]) -> (ExceptionKind, CompactString) {
        let msg: CompactString = if args.len() >= 2 { args[1].py_to_string().into() } else { CompactString::new("") };
        let kind = if !args.is_empty() {
            match &args[0].payload {
                PyObjectPayload::ExceptionType(k) => *k,
                PyObjectPayload::BuiltinType(name) => {
                    ExceptionKind::from_name(name).unwrap_or(ExceptionKind::RuntimeError)
                }
                PyObjectPayload::ExceptionInstance(ei) => ei.kind,
                _ => ExceptionKind::RuntimeError,
            }
        } else {
            ExceptionKind::RuntimeError
        };
        (kind, msg)
    }

    /// Drive an AsyncGenAwaitable: execute the action on the underlying async generator.
    ///
    /// This implements the behavior of CPython's `async_generator_anext` / `async_generator_asend`
    /// / `async_generator_athrow` objects. When `send(None)` is called:
    ///   - Next/Send: resumes the async generator. Yielded value → StopIteration(value).
    ///                On exhaustion → StopAsyncIteration.
    ///   - Throw:     throws exception into generator frame.
    ///   - Close:     throws GeneratorExit; expects generator to finish.
    pub(crate) fn drive_async_gen_awaitable(
        &mut self,
        gen: &Rc<PyCell<GeneratorState>>,
        action: &AsyncGenAction,
        send_val: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        match action {
            AsyncGenAction::Next => {
                // Resume with send_val (for first call it's None, for subsequent send() it's the arg)
                match self.resume_generator(gen, send_val) {
                    Ok(yielded) => {
                        // Async generator yielded a value — propagate via StopIteration
                        let msg = yielded.py_to_string();
                        let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                        exc.value = Some(yielded);
                        Err(exc)
                    }
                    Err(e) if e.kind == ExceptionKind::StopIteration => {
                        // Async generator returned (exhausted) — raise StopAsyncIteration
                        Err(PyException::new(ExceptionKind::StopAsyncIteration, String::new()))
                    }
                    Err(e) => Err(e),
                }
            }
            AsyncGenAction::Send(val) => {
                // Like Next but with explicit value (ignore send_val from protocol, use stored val)
                match self.resume_generator(gen, val.clone()) {
                    Ok(yielded) => {
                        let msg = yielded.py_to_string();
                        let mut exc = PyException::new(ExceptionKind::StopIteration, msg);
                        exc.value = Some(yielded);
                        Err(exc)
                    }
                    Err(e) if e.kind == ExceptionKind::StopIteration => {
                        Err(PyException::new(ExceptionKind::StopAsyncIteration, String::new()))
                    }
                    Err(e) => Err(e),
                }
            }
            AsyncGenAction::Throw(exc_kind, msg) => {
                self.gen_throw(gen, *exc_kind, msg.clone())
            }
            AsyncGenAction::Close => {
                // Like generator.close(): throw GeneratorExit, expect finish
                let g = gen.read();
                if g.finished || g.frame.is_none() {
                    drop(g);
                    return Ok(PyObject::none());
                }
                drop(g);
                match self.gen_throw(gen, ExceptionKind::GeneratorExit, CompactString::new("")) {
                    Ok(_yielded) => {
                        Err(PyException::runtime_error("async generator ignored GeneratorExit"))
                    }
                    Err(e) if e.kind == ExceptionKind::GeneratorExit
                           || e.kind == ExceptionKind::StopIteration
                           || e.kind == ExceptionKind::StopAsyncIteration => {
                        let mut g = gen.write();
                        g.finished = true;
                        g.frame = None;
                        Ok(PyObject::none())
                    }
                    Err(e) => {
                        let mut g = gen.write();
                        g.finished = true;
                        g.frame = None;
                        Err(e)
                    }
                }
            }
        }
    }

    /// If a value is a Coroutine, drive it to completion and return the final value.
    /// This is used for async-with cleanup where `__aexit__` may return a coroutine.
    /// For non-coroutine values, returns the value unchanged.
    pub(crate) fn maybe_await_result(&mut self, result: PyObjectRef) -> PyResult<PyObjectRef> {
        match &result.payload {
            PyObjectPayload::Coroutine(gen_arc) => {
                // Drive the coroutine to completion: send(None) until StopIteration
                let gen_arc = gen_arc.clone();
                let mut send_val = PyObject::none();
                loop {
                    match self.resume_generator(&gen_arc, send_val) {
                        Ok(yielded) => {
                            // Coroutine yielded — send None to continue
                            send_val = PyObject::none();
                            let _ = yielded; // discard intermediate yields
                        }
                        Err(e) if e.kind == ExceptionKind::StopIteration => {
                            return Ok(e.value.unwrap_or_else(|| PyObject::none()));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            PyObjectPayload::DeferredSleep { secs, result: sleep_result } => {
                // Perform the deferred sleep now, respecting wait_for deadline
                let secs = *secs;
                let sleep_result = sleep_result.clone();
                let deadline = ferrython_async::get_wait_for_deadline();
                if let Some(dl) = deadline {
                    let now = std::time::Instant::now();
                    if now >= dl {
                        ferrython_async::set_wait_for_deadline(None);
                        return Err(PyException::new(ExceptionKind::TimeoutError, ""));
                    }
                    let remaining = dl.duration_since(now).as_secs_f64();
                    if secs > remaining {
                        std::thread::sleep(std::time::Duration::from_secs_f64(remaining));
                        ferrython_async::set_wait_for_deadline(None);
                        return Err(PyException::new(ExceptionKind::TimeoutError, ""));
                    }
                    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                } else {
                    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                }
                Ok(sleep_result)
            }
            _ => Ok(result),
        }
    }

    /// Advance any iterable by one step (generators, iterators, instances with __next__).
    /// Returns Ok(Some(value)) on success, Ok(None) on exhaustion (StopIteration).
    pub(crate) fn vm_iter_next(&mut self, iter_obj: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
        match &iter_obj.payload {
            PyObjectPayload::Generator(gen_arc) => {
                match self.resume_generator(gen_arc, PyObject::none()) {
                    Ok(val) => Ok(Some(val)),
                    Err(e) if e.kind == ExceptionKind::StopIteration => Ok(None),
                    Err(e) => Err(e),
                }
            }
            PyObjectPayload::Instance(_) => {
                if let Some(next_method) = iter_obj.get_attr("__next__") {
                    match self.call_object(next_method, vec![]) {
                        Ok(val) => Ok(Some(val)),
                        Err(e) if e.kind == ExceptionKind::StopIteration => Ok(None),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(PyException::type_error("iterator has no __next__ method"))
                }
            }
            PyObjectPayload::Iterator(iter_data_arc) => {
                // Check for lazy iterators first
                {
                    let data = iter_data_arc.read();
                    match &*data {
                        IteratorData::Enumerate { .. }
                        | IteratorData::Zip { .. }
                        | IteratorData::Map { .. }
                        | IteratorData::Filter { .. }
                        | IteratorData::Sentinel { .. }
                        | IteratorData::TakeWhile { .. }
                        | IteratorData::DropWhile { .. }
                        | IteratorData::Count { .. }
                        | IteratorData::Cycle { .. }
                        | IteratorData::Repeat { .. }
                        | IteratorData::Chain { .. }
                        | IteratorData::Starmap { .. } => {
                            drop(data);
                            return self.advance_lazy_iterator(iter_obj);
                        }
                        _ => {}
                    }
                }
                // Standard iterators
                match builtins::iter_advance(iter_obj)? {
                    Some((_new_iter, value)) => Ok(Some(value)),
                    None => Ok(None),
                }
            }
            _ => Err(PyException::type_error(format!(
                "'{}' object is not an iterator", iter_obj.type_name()
            ))),
        }
    }

    /// Advance lazy iterator variants (Enumerate, Zip, Map, Filter).
    pub(crate) fn advance_lazy_iterator(&mut self, iter_obj: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
        let iter_data_arc = match &iter_obj.payload {
            PyObjectPayload::Iterator(arc) => arc.clone(),
            _ => return Err(PyException::type_error("not an iterator")),
        };
        let mut data = iter_data_arc.write();
        match &mut *data {
            IteratorData::Enumerate { source, index, .. } => {
                let src = source.clone();
                let idx = *index;
                *index += 1;
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(val) => Ok(Some(PyObject::tuple(vec![PyObject::int(idx), val]))),
                    None => Ok(None),
                }
            }
            IteratorData::Zip { sources, strict, .. } => {
                let srcs: Vec<PyObjectRef> = sources.clone();
                let is_strict = *strict;
                drop(data);
                let mut items = Vec::with_capacity(srcs.len());
                let mut exhausted = Vec::new();
                for (i, src) in srcs.iter().enumerate() {
                    match self.vm_iter_next(src)? {
                        Some(val) => items.push(val),
                        None => {
                            if is_strict {
                                exhausted.push(i);
                                // Continue checking remaining sources
                                items.push(PyObject::none());
                            } else {
                                return Ok(None);
                            }
                        }
                    }
                }
                if is_strict && !exhausted.is_empty() {
                    if exhausted.len() != srcs.len() {
                        return Err(PyException::value_error(
                            "zip() has arguments with different lengths"
                        ));
                    }
                    return Ok(None); // All exhausted at same time
                }
                Ok(Some(PyObject::tuple(items)))
            }
            IteratorData::Map { func, source } => {
                let f = func.clone();
                let src = source.clone();
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(val) => {
                        let result = self.call_object(f, vec![val])?;
                        Ok(Some(result))
                    }
                    None => Ok(None),
                }
            }
            IteratorData::Filter { func, source } => {
                let f = func.clone();
                let src = source.clone();
                drop(data);
                loop {
                    match self.vm_iter_next(&src)? {
                        Some(val) => {
                            let test_result = if matches!(&f.payload, PyObjectPayload::None) {
                                self.vm_is_truthy(&val)?
                            } else {
                                let r = self.call_object(f.clone(), vec![val.clone()])?;
                                self.vm_is_truthy(&r)?
                            };
                            if test_result {
                                return Ok(Some(val));
                            }
                        }
                        None => return Ok(None),
                    }
                }
            }
            IteratorData::Sentinel { callable, sentinel } => {
                let f = callable.clone();
                let s = sentinel.clone();
                drop(data);
                let val = self.call_object(f, vec![])?;
                let eq_result = val.compare(&s, ferrython_core::object::CompareOp::Eq)?;
                if eq_result.is_truthy() {
                    Ok(None)
                } else {
                    Ok(Some(val))
                }
            }
            IteratorData::TakeWhile { func, source, done } => {
                if *done { drop(data); return Ok(None); }
                let f = func.clone();
                let src = source.clone();
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(val) => {
                        let test = self.call_object(f, vec![val.clone()])?;
                        if self.vm_is_truthy(&test)? {
                            Ok(Some(val))
                        } else {
                            // Mark done
                            if let PyObjectPayload::Iterator(arc) = &iter_obj.payload {
                                if let IteratorData::TakeWhile { done, .. } = &mut *arc.write() {
                                    *done = true;
                                }
                            }
                            Ok(None)
                        }
                    }
                    None => Ok(None),
                }
            }
            IteratorData::DropWhile { func, source, dropping } => {
                let f = func.clone();
                let src = source.clone();
                let is_dropping = *dropping;
                drop(data);
                if is_dropping {
                    loop {
                        match self.vm_iter_next(&src)? {
                            Some(val) => {
                                let test = self.call_object(f.clone(), vec![val.clone()])?;
                                if !self.vm_is_truthy(&test)? {
                                    // Stop dropping, mark state
                                    if let PyObjectPayload::Iterator(arc) = &iter_obj.payload {
                                        if let IteratorData::DropWhile { dropping, .. } = &mut *arc.write() {
                                            *dropping = false;
                                        }
                                    }
                                    return Ok(Some(val));
                                }
                                // Keep dropping
                            }
                            None => return Ok(None),
                        }
                    }
                } else {
                    // Not dropping anymore, just yield
                    self.vm_iter_next(&src)
                }
            }
            IteratorData::Count { current, step } => {
                let val = *current;
                *current += *step;
                drop(data);
                Ok(Some(PyObject::int(val)))
            }
            IteratorData::Cycle { items, index } => {
                if items.is_empty() {
                    drop(data);
                    return Ok(None);
                }
                let val = items[*index].clone();
                *index = (*index + 1) % items.len();
                drop(data);
                Ok(Some(val))
            }
            IteratorData::Repeat { item, remaining } => {
                match remaining {
                    Some(0) => {
                        drop(data);
                        Ok(None)
                    }
                    Some(ref mut n) => {
                        let val = item.clone();
                        *n -= 1;
                        drop(data);
                        Ok(Some(val))
                    }
                    None => {
                        let val = item.clone();
                        drop(data);
                        Ok(Some(val))
                    }
                }
            }
            IteratorData::Chain { sources, current } => {
                // Clone what we need, then drop lock
                let srcs = sources.clone();
                let mut cur = *current;
                drop(data);
                while cur < srcs.len() {
                    match self.vm_iter_next(&srcs[cur])? {
                        Some(val) => {
                            // Update current index
                            let mut d = iter_data_arc.write();
                            if let IteratorData::Chain { current, .. } = &mut *d {
                                *current = cur;
                            }
                            return Ok(Some(val));
                        }
                        None => {
                            cur += 1;
                        }
                    }
                }
                // All exhausted
                let mut d = iter_data_arc.write();
                if let IteratorData::Chain { current, .. } = &mut *d {
                    *current = cur;
                }
                Ok(None)
            }
            IteratorData::Starmap { func, source } => {
                let f = func.clone();
                let src = source.clone();
                drop(data);
                match self.vm_iter_next(&src)? {
                    Some(args_tuple) => {
                        let call_args = args_tuple.to_list().unwrap_or_else(|_| vec![args_tuple.clone()]);
                        let result = self.call_object(f, call_args)?;
                        Ok(Some(result))
                    }
                    None => Ok(None),
                }
            }
            _ => {
                drop(data);
                match builtins::iter_advance(iter_obj)? {
                    Some((_new, val)) => Ok(Some(val)),
                    None => Ok(None),
                }
            }
        }
    }

    /// Collect any iterable into a Vec, using VM-level iteration for lazy iterators.
    /// Falls back to core `to_list()` for simple iterables.
    pub(crate) fn vm_collect_iterable(&mut self, obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
        // Try core to_list first (fast path for list, tuple, set, range, etc.)
        match obj.to_list() {
            Ok(items) => return Ok(items),
            Err(_) => {}
        }
        // Get an iterator and collect via VM
        let iter_obj = match &obj.payload {
            PyObjectPayload::Iterator(_) | PyObjectPayload::RangeIter { .. } | PyObjectPayload::Generator(_) => obj.clone(),
            PyObjectPayload::Instance(_) => {
                if let Some(iter_fn) = obj.get_attr("__iter__") {
                    let result = self.call_object(iter_fn, vec![])?;
                    // If __iter__ returns a directly iterable type (tuple, list),
                    // collect it immediately instead of treating as an iterator.
                    if let Ok(items) = result.to_list() {
                        return Ok(items);
                    }
                    result
                } else {
                    return Err(PyException::type_error(format!(
                        "cannot unpack non-iterable {} object", obj.type_name()
                    )));
                }
            }
            _ => {
                return Err(PyException::type_error(format!(
                    "cannot unpack non-iterable {} object", obj.type_name()
                )));
            }
        };
        let mut items = Vec::new();
        loop {
            match self.vm_iter_next(&iter_obj)? {
                Some(val) => items.push(val),
                None => break,
            }
        }
        Ok(items)
    }

    /// Sort items using VM-level comparison (supports custom __lt__).
    /// Uses insertion sort to allow &mut self access during comparisons.
    pub fn vm_sort(&mut self, items: &mut Vec<PyObjectRef>) -> PyResult<()> {
        let n = items.len();
        if n <= 1 { return Ok(()); }
        let has_instances = items.iter().any(|x| matches!(&x.payload, PyObjectPayload::Instance(_)));
        if !has_instances {
            items.sort_by(|a, b| {
                builtins::partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
            });
            return Ok(());
        }
        // Bottom-up merge sort with VM-level __lt__ calls — O(n log n)
        let mut aux = items.clone();
        let mut width = 1usize;
        while width < n {
            let mut i = 0;
            while i < n {
                let mid = (i + width).min(n);
                let end = (i + 2 * width).min(n);
                // Merge items[i..mid] and items[mid..end] into aux[i..end]
                let (mut left, mut right) = (i, mid);
                for k in i..end {
                    if left < mid && (right >= end || !self.vm_lt(&items[right], &items[left])?) {
                        aux[k] = items[left].clone();
                        left += 1;
                    } else {
                        aux[k] = items[right].clone();
                        right += 1;
                    }
                }
                i += 2 * width;
            }
            items.clone_from_slice(&aux);
            width *= 2;
        }
        Ok(())
    }

    /// Compare two objects using __lt__, falling back to native comparison.
    pub(crate) fn vm_lt(&mut self, a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
        if let PyObjectPayload::Instance(_inst) = &a.payload {
            if let Some(method) = a.get_attr("__lt__") {
                // If method is from class namespace (not bound), pass self explicitly
                let result = if matches!(&method.payload, PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) | PyObjectPayload::Function(_)) {
                    self.call_object(method, vec![a.clone(), b.clone()])?
                } else {
                    self.call_object(method, vec![b.clone()])?
                };
                return Ok(result.is_truthy());
            }
        }
        Ok(builtins::partial_cmp_for_sort(a, b) == Some(std::cmp::Ordering::Less))
    }

    // ── Post-call intercept for VM-aware builtins ────────────────────────

    /// After every function call, check for deferred VM-aware operations.
    /// This handles builtins that need VM access but are called through the
    /// generic NativeFunction path (which doesn't pass &mut self).
    pub(crate) fn post_call_intercept(&mut self, mut result: PyObjectRef) -> PyResult<PyObjectRef> {
        // Fast path: skip all thread-local checks when no intercept is pending
        if !ferrython_core::object::check_intercept_pending() {
            return Ok(result);
        }
        // asyncio.run() intercept: drive coroutine to completion
        if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
            result = self.maybe_await_result(coro)?;
        }
        // __import__() intercept: resolve and load module
        if let Some(req) = crate::builtins::take_import_request() {
            result = self.import_module_simple(&req.name, req.level)?;
        }
        // importlib.import_module() intercept
        if let Some(req) = ferrython_stdlib::take_import_module_request() {
            let (name, level) = if req.name.starts_with('.') {
                let dots = req.name.chars().take_while(|c| *c == '.').count();
                let rest = &req.name[dots..];
                if let Some(ref pkg) = req.package {
                    let abs_name = if rest.is_empty() {
                        pkg.to_string()
                    } else {
                        format!("{}.{}", pkg, rest)
                    };
                    (abs_name, dots)
                } else {
                    (rest.to_string(), dots)
                }
            } else {
                (req.name.to_string(), 0)
            };
            result = self.import_module_simple(&name, level)?;
        }
        // importlib.reload() intercept
        if let Some(req) = ferrython_stdlib::take_reload_request() {
            result = self.reload_module(req.module)?;
        }
        Ok(result)
    }
}

impl VirtualMachine {
    // ── exec/eval/compile helpers (moved from vm_call.rs) ──

    pub(crate) fn builtin_exec(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() || args.len() > 3 {
            return Err(PyException::type_error("exec() takes 1 to 3 arguments"));
        }
        let code = if let PyObjectPayload::Code(co) = &args[0].payload {
            Rc::clone(co)
        } else {
            let code_str = args[0].as_str().ok_or_else(||
                PyException::type_error("exec() arg 1 must be a string or code object"))?;
            let module = ferrython_parser::parse(code_str, "<string>")
                .map_err(|e| PyException::syntax_error(format!("exec: {}", e)))?;
            let mut compiler = ferrython_compiler::Compiler::new("<string>".to_string());
            Rc::new(compiler.compile_module(&module)
                .map_err(|e| PyException::syntax_error(format!("exec: {}", e)))?)
        };
        if args.len() >= 2 {
            if let PyObjectPayload::Dict(ref map) = args[1].payload {
                let mut new_globals = FxAttrMap::default();
                let m = map.read();
                for (k, v) in m.iter() {
                    let key_str = match k {
                        HashableKey::Str(s) => CompactString::clone(s),
                        _ => CompactString::from(format!("{:?}", k)),
                    };
                    new_globals.insert(key_str, v.clone());
                }
                drop(m);
                // Merge locals dict into globals for execution scope
                // Track original global keys so we can separate results later
                let original_global_keys: Vec<CompactString> = new_globals.keys().cloned().collect();
                if args.len() >= 3 {
                    if let PyObjectPayload::Dict(ref lmap) = args[2].payload {
                        let lm = lmap.read();
                        for (k, v) in lm.iter() {
                            let key_str = match k {
                                HashableKey::Str(s) => CompactString::clone(s),
                                _ => CompactString::from(format!("{:?}", k)),
                            };
                            new_globals.insert(key_str, v.clone());
                        }
                        drop(lm);
                    }
                }
                let shared = Rc::new(PyCell::new(new_globals));
                self.execute_with_globals(code, shared.clone())?;
                let results = shared.read();
                if args.len() >= 3 {
                    // Separate globals/locals: only write back original global keys to globals,
                    // and all new/modified keys to locals
                    let mut gm = map.write();
                    for (k, v) in results.iter() {
                        if original_global_keys.contains(k) {
                            gm.insert(HashableKey::str_key(k.clone()), v.clone());
                        }
                    }
                    drop(gm);
                    if let PyObjectPayload::Dict(ref lmap) = args[2].payload {
                        let mut lm = lmap.write();
                        for (k, v) in results.iter() {
                            if !original_global_keys.contains(k) || lm.contains_key(&HashableKey::str_key(k.clone())) {
                                lm.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                        }
                    }
                } else {
                    // No separate locals — write everything back to globals
                    let mut m = map.write();
                    for (k, v) in results.iter() {
                        m.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                }
            } else {
                return Err(PyException::type_error("exec() globals must be a dict"));
            }
        } else {
            let globals = self.call_stack.last().unwrap().globals.clone();
            self.execute_with_globals(code, globals)?;
        }
        Ok(PyObject::none())
    }

    pub(crate) fn builtin_eval(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() || args.len() > 3 {
            return Err(PyException::type_error("eval() takes 1 to 3 arguments"));
        }
        // Accept either a string or a code object (from compile())
        let code = if let PyObjectPayload::Code(co) = &args[0].payload {
            Rc::clone(co)
        } else {
            let code_str = args[0].as_str().ok_or_else(||
                PyException::type_error("eval() arg 1 must be a string, bytes or code object"))?;
            let wrapped = format!("__eval_result__ = ({})", code_str);
            let module = ferrython_parser::parse(&wrapped, "<string>")
                .map_err(|e| PyException::syntax_error(format!("eval: {}", e)))?;
            let mut compiler = ferrython_compiler::Compiler::new("<string>".to_string());
            Rc::new(compiler.compile_module(&module)
                .map_err(|e| PyException::syntax_error(format!("eval: {}", e)))?)
        };
        let is_code_obj = matches!(&args[0].payload, PyObjectPayload::Code(_));
        if args.len() >= 2 {
            if let PyObjectPayload::Dict(ref globs_map) = args[1].payload {
                let mut new_globals = FxAttrMap::default();
                let gm = globs_map.read();
                for (k, v) in gm.iter() {
                    let key_str = match k {
                        HashableKey::Str(s) => CompactString::clone(s),
                        _ => CompactString::from(format!("{:?}", k)),
                    };
                    new_globals.insert(key_str, v.clone());
                }
                drop(gm);

                // Check if we have a separate locals dict (args[2] that is not None)
                let has_separate_locals = args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::None);
                let locals_dict = if has_separate_locals {
                    if let PyObjectPayload::Dict(ref lm) = args[2].payload {
                        Some(lm.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Merge locals entries into globals for name resolution
                let original_global_keys: std::collections::HashSet<CompactString> =
                    new_globals.keys().cloned().collect();
                if let Some(ref locals_arc) = locals_dict {
                    let lm = locals_arc.read();
                    for (k, v) in lm.iter() {
                        let key_str = match k {
                            HashableKey::Str(s) => CompactString::clone(s),
                            _ => CompactString::from(format!("{:?}", k)),
                        };
                        new_globals.insert(key_str, v.clone());
                    }
                }

                let shared = Rc::new(PyCell::new(new_globals));
                let exec_result = self.execute_with_globals(code, shared.clone())?;

                // Check for __eval_result__ (compile(mode='eval') wrapping)
                let eval_result = shared.read().get("__eval_result__").cloned();

                // Write results back to the appropriate dicts
                let results = shared.read();
                if let Some(ref locals_arc) = locals_dict {
                    // Separate locals: new defs go to locals, existing globals updated in globals
                    let mut gm = globs_map.write();
                    let mut lm = locals_arc.write();
                    for (k, v) in results.iter() {
                        let hk = HashableKey::str_key(k.clone());
                        if original_global_keys.contains(k) {
                            // Update existing global entry
                            gm.insert(hk, v.clone());
                        } else {
                            // New definition goes to locals
                            lm.insert(hk, v.clone());
                        }
                    }
                } else {
                    // No separate locals: write everything back to globals
                    let mut gm = globs_map.write();
                    for (k, v) in results.iter() {
                        gm.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                }
                drop(results);

                if let Some(val) = eval_result {
                    return Ok(val);
                }
                if is_code_obj {
                    return Ok(exec_result);
                }
                Ok(PyObject::none())
            } else {
                Err(PyException::type_error("eval() globals must be a dict"))
            }
        } else {
            let globals = self.call_stack.last().unwrap().globals.clone();
            let exec_result = self.execute_with_globals(code, globals.clone())?;
            // Check for __eval_result__ in globals (set by compile(mode='eval'))
            if let Some(val) = globals.read().get("__eval_result__").cloned() {
                return Ok(val);
            }
            if is_code_obj {
                return Ok(exec_result);
            }
            let result = globals.read().get("__eval_result__").cloned()
                .unwrap_or_else(PyObject::none);
            Ok(result)
        }
    }

    pub(crate) fn builtin_compile(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 {
            return Err(PyException::type_error("compile() requires at least 3 arguments"));
        }
        let filename = args[1].py_to_string();
        let mode = args[2].py_to_string();

        // Check if the argument is an AST object (Instance), not a string
        let is_ast_obj = matches!(&args[0].payload, PyObjectPayload::Instance(_));

        if is_ast_obj {
            // Try direct PyObject AST → Rust AST conversion first.
            // This handles non-standard identifiers (e.g. werkzeug's `<builder:...>`)
            // that can't survive source-code roundtrip.
            let has_stored_source = if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                attrs.get("__source__").map(|s| !s.py_to_string().is_empty()).unwrap_or(false)
            } else { false };

            if has_stored_source {
                // Has original source from ast.parse() — use it (fast path)
                let source = if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    inst.attrs.read().get("__source__").unwrap().py_to_string()
                } else { unreachable!() };
                let effective = if mode == "eval" {
                    format!("__eval_result__ = ({})", source)
                } else { source };
                let module = ferrython_parser::parse(&effective, &filename)
                    .map_err(|e| PyException::syntax_error(format!("compile: {}", e)))?;
                let mut compiler = ferrython_compiler::Compiler::new(filename.clone());
                let code = compiler.compile_module(&module)
                    .map_err(|e| PyException::syntax_error(format!("compile: {}", e)))?;
                return Ok(PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::new(code))));
            }

            // No stored source — convert AST objects directly to Rust AST
            match ferrython_stdlib::pyobj_ast_to_module(&args[0]) {
                Ok(module) => {
                    let mut compiler = ferrython_compiler::Compiler::new(filename.clone());
                    let code = compiler.compile_module(&module)
                        .map_err(|e| PyException::syntax_error(format!("compile: {}", e)))?;
                    return Ok(PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::new(code))));
                }
                Err(_e) => {
                    // Fallback: try unparse → reparse
                    let source = ferrython_stdlib::ast_unparse_module(&args[0]);
                    let effective = if mode == "eval" {
                        format!("__eval_result__ = ({})", source)
                    } else { source };
                    let module = ferrython_parser::parse(&effective, &filename)
                        .map_err(|e| PyException::syntax_error(format!("compile: {}", e)))?;
                    let mut compiler = ferrython_compiler::Compiler::new(filename.clone());
                    let code = compiler.compile_module(&module)
                        .map_err(|e| PyException::syntax_error(format!("compile: {}", e)))?;
                    return Ok(PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::new(code))));
                }
            }
        }

        // String source code
        let source = if let Some(s) = args[0].as_str() {
            s.to_string()
        } else {
            return Err(PyException::type_error(
                "compile() arg 1 must be a string, bytes, or AST object"));
        };
        let effective_source = if mode == "eval" {
            format!("__eval_result__ = ({})", source)
        } else {
            source
        };
        let module = ferrython_parser::parse(&effective_source, &filename)
            .map_err(|e| PyException::syntax_error(format!("compile: {}", e)))?;
        let mut compiler = ferrython_compiler::Compiler::new(filename);
        let code = compiler.compile_module(&module)
            .map_err(|e| PyException::syntax_error(format!("compile: {}", e)))?;
        Ok(PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::new(code))))
    }

    // ── Regex helpers (moved from vm_call.rs) ──

    /// Handle re.sub/re.subn when the replacement is a callable
    pub(crate) fn re_sub_with_callable(&mut self, args: &[PyObjectRef], return_count: bool) -> PyResult<PyObjectRef> {
        let pattern = args[0].py_to_string();
        let repl_fn = args[1].clone();
        let text = args[2].py_to_string();
        let max_count = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::Dict(_)) {
            args[3].to_int().unwrap_or(0) as usize
        } else { 0 };
        let mut flags = if args.len() > 4 && !matches!(&args[4].payload, PyObjectPayload::Dict(_)) {
            args[4].to_int().unwrap_or(0)
        } else { 0 };
        // Check trailing kwargs dict
        let mut max_count_kw = max_count;
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(map) = &last.payload {
                let map_r = map.read();
                for (k, v) in map_r.iter() {
                    if let HashableKey::Str(s) = k {
                        match s.as_str() {
                            "count" => max_count_kw = v.to_int().unwrap_or(0) as usize,
                            "flags" => flags = v.to_int().unwrap_or(0),
                            _ => {}
                        }
                    }
                }
            }
        }
        let max_count = if max_count_kw > 0 { max_count_kw } else { max_count };

        let mut re_pattern = pattern.clone();
        re_pattern = re_pattern.replace("(?P<", "(?P<");
        let re = if flags & 2 != 0 {
            regex::RegexBuilder::new(&re_pattern).case_insensitive(true).build()
        } else {
            regex::Regex::new(&re_pattern)
        }.map_err(|e| PyException::runtime_error(format!("regex error: {}", e)))?;

        let mut result = String::new();
        let mut last_end = 0;
        let mut count = 0;
        for caps in re.captures_iter(&text) {
            if max_count > 0 && count >= max_count { break; }
            let whole = caps.get(0).unwrap();
            result.push_str(&text[last_end..whole.start()]);

            let match_text = whole.as_str().to_string();
            // Collect capture groups
            let mut group_strs: Vec<PyObjectRef> = Vec::new();
            for i in 1..caps.len() {
                if let Some(g) = caps.get(i) {
                    group_strs.push(PyObject::str_val(CompactString::from(g.as_str())));
                } else {
                    group_strs.push(PyObject::none());
                }
            }
            // Build name→index mapping
            let mut groupindex_map = IndexMap::new();
            for (i, name_opt) in re.capture_names().enumerate() {
                if let Some(name) = name_opt {
                    groupindex_map.insert(
                        HashableKey::str_key(CompactString::from(name)),
                        PyObject::int(i as i64),
                    );
                }
            }
            let groups_tuple = PyObject::tuple(group_strs);
            let groupindex = PyObject::dict(groupindex_map);

            let mut match_attrs = IndexMap::new();
            match_attrs.insert(CompactString::from("_match"), PyObject::str_val(CompactString::from(match_text.clone())));
            match_attrs.insert(CompactString::from("_groups"), groups_tuple.clone());
            match_attrs.insert(CompactString::from("_groupindex"), groupindex);
            match_attrs.insert(CompactString::from("_start"), PyObject::int(whole.start() as i64));
            match_attrs.insert(CompactString::from("_end"), PyObject::int(whole.end() as i64));
            match_attrs.insert(CompactString::from("_text"), PyObject::str_val(CompactString::from(text.clone())));
            match_attrs.insert(CompactString::from("group"), PyObject::native_function("Match.group", ferrython_stdlib::text_modules::match_group_fn));
            match_attrs.insert(CompactString::from("groups"), PyObject::native_function("Match.groups", ferrython_stdlib::text_modules::match_groups_fn));
            match_attrs.insert(CompactString::from("groupdict"), PyObject::native_function("Match.groupdict", ferrython_stdlib::text_modules::match_groupdict_fn));
            match_attrs.insert(CompactString::from("start"), PyObject::native_function("Match.start", ferrython_stdlib::text_modules::match_start_fn));
            match_attrs.insert(CompactString::from("end"), PyObject::native_function("Match.end", ferrython_stdlib::text_modules::match_end_fn));
            match_attrs.insert(CompactString::from("span"), PyObject::native_function("Match.span", ferrython_stdlib::text_modules::match_span_fn));
            match_attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
            let match_obj = PyObject::module_with_attrs(CompactString::from("Match"), match_attrs);

            let replacement = self.call_object(repl_fn.clone(), vec![match_obj])?;
            result.push_str(&replacement.py_to_string());

            last_end = whole.end();
            count += 1;
        }
        result.push_str(&text[last_end..]);

        if return_count {
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(result)),
                PyObject::int(count as i64),
            ]))
        } else {
            Ok(PyObject::str_val(CompactString::from(result)))
        }
    }

    // ── Itertools helpers (moved from vm_call.rs) ──

    pub(crate) fn vm_itertools_groupby(&mut self, args: &[PyObjectRef], key_fn: Option<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("groupby requires iterable"));
        }
        let items = args[0].to_list()?;
        if items.is_empty() {
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
                IteratorData::List { items: vec![], index: 0 }
            )))));
        }

        let mut result = Vec::new();
        let first_key = if let Some(ref kf) = key_fn {
            self.call_object(kf.clone(), vec![items[0].clone()])?
        } else {
            items[0].clone()
        };
        let mut current_key = first_key;
        let mut current_group = vec![items[0].clone()];

        for item in &items[1..] {
            let k = if let Some(ref kf) = key_fn {
                self.call_object(kf.clone(), vec![item.clone()])?
            } else {
                item.clone()
            };
            if k.py_to_string() == current_key.py_to_string() {
                current_group.push(item.clone());
            } else {
                let group_iter = PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
                    IteratorData::List { items: current_group, index: 0 }
                ))));
                result.push(PyObject::tuple(vec![current_key, group_iter]));
                current_key = k;
                current_group = vec![item.clone()];
            }
        }
        let group_iter = PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
            IteratorData::List { items: current_group, index: 0 }
        ))));
        result.push(PyObject::tuple(vec![current_key, group_iter]));
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
            IteratorData::List { items: result, index: 0 }
        )))))
    }

    pub(crate) fn vm_itertools_filterfalse(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let pred = args[0].clone();
        let items = self.collect_iterable(&args[1])?;
        let mut result = Vec::new();
        let is_none = matches!(&pred.payload, PyObjectPayload::None);
        for item in &items {
            let val = if is_none {
                item.is_truthy()
            } else {
                let r = self.call_object(pred.clone(), vec![item.clone()])?;
                r.is_truthy()
            };
            if !val {
                result.push(item.clone());
            }
        }
        Ok(PyObject::list(result))
    }

    pub(crate) fn vm_itertools_starmap(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let func = args[0].clone();
        let items = self.collect_iterable(&args[1])?;
        let mut result = Vec::new();
        for item in &items {
            let call_args = item.to_list().unwrap_or_else(|_| vec![item.clone()]);
            let r = self.call_object(func.clone(), call_args)?;
            result.push(r);
        }
        Ok(PyObject::list(result))
    }

    pub(crate) fn vm_itertools_accumulate(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let items = args[0].to_list()?;
        if items.is_empty() { return Ok(PyObject::list(vec![])); }
        let func = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None | PyObjectPayload::Dict(_)) {
            Some(args[1].clone())
        } else {
            None
        };
        let mut result = Vec::new();
        let mut acc = items[0].clone();
        result.push(acc.clone());
        for item in &items[1..] {
            acc = if let Some(ref f) = func {
                self.call_object(f.clone(), vec![acc, item.clone()])?
            } else {
                acc.add(item)?
            };
            result.push(acc.clone());
        }
        Ok(PyObject::list(result))
    }

    /// RawIOBase.read(size=-1): calls self.readinto() to read data.
    pub(crate) fn rawiobase_read(&mut self, this: &PyObjectRef, size: i64) -> PyResult<PyObjectRef> {
        if size < 0 {
            return self.rawiobase_readall(this);
        }
        let buf = PyObject::wrap(PyObjectPayload::ByteArray(vec![0u8; size as usize]));
        let readinto = self.exec_load_attr_value(this, "readinto")?;
        let n_obj = self.call_object(readinto, vec![buf.clone()])?;
        let n = n_obj.as_int().unwrap_or(0).max(0) as usize;
        if let PyObjectPayload::ByteArray(data) = &buf.payload {
            Ok(PyObject::bytes(data[..n.min(size as usize)].to_vec()))
        } else {
            Ok(PyObject::bytes(vec![]))
        }
    }

    /// RawIOBase.readall(): reads until EOF by calling readinto() in chunks.
    pub(crate) fn rawiobase_readall(&mut self, this: &PyObjectRef) -> PyResult<PyObjectRef> {
        let readinto = self.exec_load_attr_value(this, "readinto")?;
        let mut result = Vec::new();
        loop {
            let buf = PyObject::wrap(PyObjectPayload::ByteArray(vec![0u8; 8192]));
            let n_obj = self.call_object(readinto.clone(), vec![buf.clone()])?;
            let n = n_obj.as_int().unwrap_or(0).max(0) as usize;
            if n == 0 { break; }
            if let PyObjectPayload::ByteArray(data) = &buf.payload {
                result.extend_from_slice(&data[..n.min(data.len())]);
            }
        }
        Ok(PyObject::bytes(result))
    }

    /// Helper to load an attribute via the VM's full resolution (descriptor protocol etc.)
    fn exec_load_attr_value(&mut self, obj: &PyObjectRef, name: &str) -> PyResult<PyObjectRef> {
        // Use core get_attr first, which handles most cases
        if let Some(val) = obj.get_attr(name) {
            return Ok(val);
        }
        Err(PyException::attribute_error(format!("'{}' object has no attribute '{}'", obj.type_name(), name)))
    }
}
