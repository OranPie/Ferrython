//! VM text, repr, and string-format helpers.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    parse_format_usize, py_ascii_repr, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

fn marker_builtin_dunder(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    if inst.attrs.read().contains_key("__deque__") || inst.attrs.read().contains_key("__weakset__")
    {
        return obj
            .get_attr(name)
            .filter(|method| matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)));
    }
    None
}

fn class_or_mro_defines_method(cls: &PyObjectRef, name: &str) -> bool {
    let PyObjectPayload::Class(cd) = &cls.payload else {
        return false;
    };
    if cd.namespace.read().contains_key(name) {
        return true;
    }
    cd.mro.iter().any(|base| {
        if let PyObjectPayload::Class(base_cd) = &base.payload {
            base_cd.namespace.read().contains_key(name)
        } else {
            false
        }
    })
}

impl VirtualMachine {
    pub(crate) fn vm_str(&mut self, obj: &PyObjectRef) -> PyResult<String> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                if let Some(str_method) = marker_builtin_dunder(obj, "__str__")
                    .or_else(|| marker_builtin_dunder(obj, "__repr__"))
                {
                    let result = self.call_object(str_method, vec![])?;
                    if let PyObjectPayload::Str(s) = &result.payload {
                        return Ok(s.to_string());
                    }
                    return Err(PyException::type_error(format!(
                        "__str__ returned non-string (type {})",
                        result.type_name()
                    )));
                }
                if let Some(bv) = Self::get_builtin_value(obj) {
                    if matches!(
                        &bv.payload,
                        PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                    ) && !class_or_mro_defines_method(&inst.class, "__str__")
                    {
                        return self.vm_repr(obj);
                    }
                }
                // Check for custom __str__ (skip BuiltinBoundMethod from builtin bases)
                if let Some(str_method) = Self::resolve_instance_dunder(obj, "__str__") {
                    let method = self.resolve_descriptor(&str_method, obj)?;
                    let args = match &method.payload {
                        PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) => {
                            vec![obj.clone()]
                        }
                        _ => vec![],
                    };
                    let result = self.call_object(method, args)?;
                    if let PyObjectPayload::Str(s) = &result.payload {
                        return Ok(s.to_string());
                    }
                    if let Some(bv) = Self::get_builtin_value(&result) {
                        if let PyObjectPayload::Str(s) = &bv.payload {
                            return Ok(s.to_string());
                        }
                    }
                    return Err(PyException::type_error(format!(
                        "__str__ returned non-string (type {})",
                        result.type_name()
                    )));
                }
                // Fall back to __repr__ before __builtin_value__: namedtuples, dataclasses, etc.
                // define custom __repr__ that should serve as str() too.
                if let Some(repr_method) = Self::resolve_instance_dunder(obj, "__repr__") {
                    let method = self.resolve_descriptor(&repr_method, obj)?;
                    let args = match &method.payload {
                        PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) => {
                            vec![obj.clone()]
                        }
                        _ => vec![],
                    };
                    let result = self.call_object(method, args)?;
                    if let PyObjectPayload::Str(s) = &result.payload {
                        return Ok(s.to_string());
                    }
                    if let Some(bv) = Self::get_builtin_value(&result) {
                        if let PyObjectPayload::Str(s) = &bv.payload {
                            return Ok(s.to_string());
                        }
                    }
                    return Err(PyException::type_error(format!(
                        "__repr__ returned non-string (type {})",
                        result.type_name()
                    )));
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
            PyObjectPayload::List(_)
            | PyObjectPayload::Tuple(_)
            | PyObjectPayload::Dict(_)
            | PyObjectPayload::Set(_)
            | PyObjectPayload::FrozenSet(_) => self.vm_repr(obj),
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

    fn validate_format_arg_index(field_name: &str) -> PyResult<Option<usize>> {
        if !field_name.chars().all(|c| c.is_ascii_digit()) {
            return Ok(None);
        }
        let idx = parse_format_usize(field_name)?;
        if idx > isize::MAX as usize {
            return Err(PyException::value_error(
                "Too many decimal digits in format string",
            ));
        }
        Ok(Some(idx))
    }

    /// Format a single replacement field value with optional conversion and format spec.
    fn vm_format_field(
        &mut self,
        val: &PyObjectRef,
        conversion: Option<&str>,
        spec: Option<&str>,
    ) -> PyResult<String> {
        match conversion {
            Some("a") => {
                let text = py_ascii_repr(val);
                Ok(match spec {
                    Some(s) if !s.is_empty() => crate::builtins::apply_format_spec_str(&text, s)?,
                    _ => text,
                })
            }
            Some("r") => {
                let text = self.vm_format_obj_repr(val)?;
                Ok(match spec {
                    Some(s) if !s.is_empty() => crate::builtins::apply_format_spec_str(&text, s)?,
                    _ => text,
                })
            }
            Some("s") => {
                let text = self.vm_format_obj_str(val)?;
                Ok(match spec {
                    Some(s) if !s.is_empty() => crate::builtins::apply_format_spec_str(&text, s)?,
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
                        val.format_value(s)
                    }
                    _ => self.vm_format_obj_str(val),
                }
            }
        }
    }

    /// Parse a field spec like "name!r:>10" into (field_name, conversion, format_spec).
    fn parse_format_field(field_spec: &str) -> (&str, Option<&str>, Option<&str>) {
        let (field_part, format_spec) = if let Some(cp) = field_spec.find(':') {
            (&field_spec[..cp], Some(&field_spec[cp + 1..]))
        } else {
            (field_spec, None)
        };
        let (field_name, conversion) = if let Some(bp) = field_part.find('!') {
            (&field_part[..bp], Some(&field_part[bp + 1..]))
        } else {
            (field_part, None)
        };
        (field_name, conversion, format_spec)
    }

    /// VM-aware str.format() with positional args only.
    pub(crate) fn vm_str_format(
        &mut self,
        fmt: &str,
        args: &[PyObjectRef],
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
                        if c == '{' {
                            depth += 1;
                        } else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        field_spec.push(c);
                    }
                    let (field_name, conversion, format_spec) =
                        Self::parse_format_field(&field_spec);
                    // Resolve value
                    let val = if field_name.is_empty() {
                        let v = args.get(auto_idx).cloned();
                        auto_idx += 1;
                        v
                    } else if let Some(idx) = Self::validate_format_arg_index(field_name)? {
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
        &mut self,
        fmt: &str,
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
                        if c == '{' {
                            depth += 1;
                        } else if c == '}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
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
                    } else if let Some(idx) = Self::validate_format_arg_index(field_name)? {
                        pos_args.get(idx).cloned()
                    } else {
                        kwargs
                            .iter()
                            .find(|(k, _)| k.as_str() == field_name)
                            .map(|(_, v)| v.clone())
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
        &mut self,
        spec: &str,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> Option<String> {
        if !spec.contains('{') {
            return None;
        }
        // Only resolve nested refs in the format_spec part (after ':')
        if let Some(colon_pos) = spec.find(':') {
            let format_part = &spec[colon_pos + 1..];
            if !format_part.contains('{') {
                return None;
            }
            let mut r = spec[..=colon_pos].to_string();
            let mut sc = format_part.chars().peekable();
            while let Some(ch) = sc.next() {
                if ch == '{' {
                    let mut ref_name = String::new();
                    for ch in sc.by_ref() {
                        if ch == '}' {
                            break;
                        }
                        ref_name.push(ch);
                    }
                    if let Some(idx) = Self::validate_format_arg_index(&ref_name).ok().flatten() {
                        if let Some(v) = pos_args.get(idx) {
                            r.push_str(&v.py_to_string());
                        }
                    } else if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == ref_name)
                    {
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
        &mut self,
        field_name: &str,
        pos_args: &[PyObjectRef],
        auto_idx: usize,
        _kwargs: &[(CompactString, PyObjectRef)],
    ) -> Option<PyObjectRef> {
        // Parse base name: everything before first '.' or '['
        let base_end = field_name
            .find(|c: char| c == '.' || c == '[')
            .unwrap_or(field_name.len());
        let base = &field_name[..base_end];
        let rest = &field_name[base_end..];

        let mut current = if let Some(idx) = Self::validate_format_arg_index(base).ok().flatten() {
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
                    if nc == '.' || nc == '[' {
                        break;
                    }
                    attr.push(nc);
                    chars.next();
                }
                current = current.get_attr(&attr)?;
            } else if c == '[' {
                chars.next();
                let mut key = String::new();
                for nc in chars.by_ref() {
                    if nc == ']' {
                        break;
                    }
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
                    },
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
                if let Some(repr_method) = marker_builtin_dunder(obj, "__repr__") {
                    let result = self.call_object(repr_method, vec![])?;
                    if let PyObjectPayload::Str(s) = &result.payload {
                        return Ok(s.to_string());
                    }
                    return Err(PyException::type_error(format!(
                        "__repr__ returned non-string (type {})",
                        result.type_name()
                    )));
                }
                // Check for custom __repr__ (skip BuiltinBoundMethod from builtin bases)
                if let Some(repr_method) = Self::resolve_instance_dunder(obj, "__repr__") {
                    // If it's a descriptor (Instance with __get__), invoke __get__
                    let method = self.resolve_descriptor(&repr_method, obj)?;
                    let args = match &method.payload {
                        PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) => {
                            vec![obj.clone()]
                        }
                        _ => vec![],
                    };
                    let result = self.call_object(method, args)?;
                    if let PyObjectPayload::Str(s) = &result.payload {
                        return Ok(s.to_string());
                    }
                    if let Some(bv) = Self::get_builtin_value(&result) {
                        if let PyObjectPayload::Str(s) = &bv.payload {
                            return Ok(s.to_string());
                        }
                    }
                    return Err(PyException::type_error(format!(
                        "__repr__ returned non-string (type {})",
                        result.type_name()
                    )));
                }
                // Dataclass auto-repr (before __builtin_value__ delegation)
                let class = &inst.class;
                if matches!(&class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__dataclass__"))
                {
                    if let Some(fields) = class.get_attr("__dataclass_fields__") {
                        let field_names = crate::vm_dataclass_utils::extract_field_names(&fields);
                        if !field_names.is_empty() {
                            let class_name = if let PyObjectPayload::Class(cd) = &class.payload {
                                cd.name.to_string()
                            } else {
                                "?".to_string()
                            };
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
                if matches!(&class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__namedtuple__"))
                {
                    if let Some(fields) = class.get_attr("_fields") {
                        if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                            let class_name = if let PyObjectPayload::Class(cd) = &class.payload {
                                cd.name.to_string()
                            } else {
                                "?".to_string()
                            };
                            let mut parts = Vec::new();
                            let attrs = inst.attrs.read();
                            for field in field_names.iter() {
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
                    if matches!(
                        &bv.payload,
                        PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                    ) {
                        let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            cd.name.to_string()
                        } else {
                            obj.type_name().to_string()
                        };
                        let ptr = PyObjectRef::as_ptr(obj) as usize;
                        if !ferrython_core::object::repr_enter(ptr) {
                            if ferrython_core::object::helpers::repr_depth_exceeded() {
                                return Err(ferrython_core::error::PyException::recursion_error(
                                    "maximum recursion depth exceeded while getting the repr of an object",
                                ));
                            }
                            return Ok(format!("{}(...)", class_name));
                        }
                        let inner = match self.vm_repr(&bv) {
                            Ok(text) => text,
                            Err(err) => {
                                ferrython_core::object::repr_leave(ptr);
                                return Err(err);
                            }
                        };
                        ferrython_core::object::repr_leave(ptr);
                        let body = match &bv.payload {
                            PyObjectPayload::Set(_) => {
                                inner.strip_prefix('{').and_then(|s| s.strip_suffix('}'))
                            }
                            PyObjectPayload::FrozenSet(_) => inner
                                .strip_prefix("frozenset({")
                                .and_then(|s| s.strip_suffix("})")),
                            _ => None,
                        }
                        .unwrap_or(inner.as_str());
                        return Ok(format!("{}({{{}}})", class_name, body));
                    }
                    return self.vm_repr(&bv);
                }
                Ok(obj.repr())
            }
            PyObjectPayload::List(items) => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                if !ferrython_core::object::repr_enter(ptr) {
                    if ferrython_core::object::helpers::repr_depth_exceeded() {
                        return Err(ferrython_core::error::PyException::recursion_error(
                            "maximum recursion depth exceeded while getting the repr of an object",
                        ));
                    }
                    return Ok("[...]".to_string());
                }
                let items = items.read().clone();
                let mut parts = Vec::new();
                for item in &items {
                    match self.vm_repr(item) {
                        Ok(text) => parts.push(text),
                        Err(err) => {
                            ferrython_core::object::repr_leave(ptr);
                            return Err(err);
                        }
                    }
                }
                ferrython_core::object::repr_leave(ptr);
                Ok(format!("[{}]", parts.join(", ")))
            }
            PyObjectPayload::Tuple(items) => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                if !ferrython_core::object::repr_enter(ptr) {
                    if ferrython_core::object::helpers::repr_depth_exceeded() {
                        return Err(ferrython_core::error::PyException::recursion_error(
                            "maximum recursion depth exceeded while getting the repr of an object",
                        ));
                    }
                    return Ok("(...)".to_string());
                }
                let mut parts = Vec::new();
                for item in items.iter() {
                    match self.vm_repr(item) {
                        Ok(text) => parts.push(text),
                        Err(err) => {
                            ferrython_core::object::repr_leave(ptr);
                            return Err(err);
                        }
                    }
                }
                let result = if parts.len() == 1 {
                    Ok(format!("({},)", parts[0]))
                } else {
                    Ok(format!("({})", parts.join(", ")))
                };
                ferrython_core::object::repr_leave(ptr);
                result
            }
            PyObjectPayload::Dict(m) => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                if !ferrython_core::object::repr_enter(ptr) {
                    if ferrython_core::object::helpers::repr_depth_exceeded() {
                        return Err(ferrython_core::error::PyException::recursion_error(
                            "maximum recursion depth exceeded while getting the repr of an object",
                        ));
                    }
                    return Ok("{...}".to_string());
                }
                let m = m.read().clone();
                let mut parts = Vec::new();
                for (k, v) in &m {
                    if ferrython_core::object::is_hidden_dict_key(k) {
                        continue;
                    }
                    let kr = match self.vm_repr(&k.to_object()) {
                        Ok(text) => text,
                        Err(err) => {
                            ferrython_core::object::repr_leave(ptr);
                            return Err(err);
                        }
                    };
                    let vr = match self.vm_repr(v) {
                        Ok(text) => text,
                        Err(err) => {
                            ferrython_core::object::repr_leave(ptr);
                            return Err(err);
                        }
                    };
                    parts.push(format!("{}: {}", kr, vr));
                }
                ferrython_core::object::repr_leave(ptr);
                Ok(format!("{{{}}}", parts.join(", ")))
            }
            PyObjectPayload::Set(m) => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                if !ferrython_core::object::repr_enter(ptr) {
                    if ferrython_core::object::helpers::repr_depth_exceeded() {
                        return Err(ferrython_core::error::PyException::recursion_error(
                            "maximum recursion depth exceeded while getting the repr of an object",
                        ));
                    }
                    return Ok("set(...)".to_string());
                }
                let m = m.read().clone();
                if m.is_empty() {
                    ferrython_core::object::repr_leave(ptr);
                    return Ok("set()".to_string());
                }
                let mut parts = Vec::new();
                for v in m.values() {
                    match self.vm_repr(v) {
                        Ok(text) => parts.push(text),
                        Err(err) => {
                            ferrython_core::object::repr_leave(ptr);
                            return Err(err);
                        }
                    }
                }
                parts.sort();
                ferrython_core::object::repr_leave(ptr);
                Ok(format!("{{{}}}", parts.join(", ")))
            }
            PyObjectPayload::FrozenSet(m) => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                if !ferrython_core::object::repr_enter(ptr) {
                    if ferrython_core::object::helpers::repr_depth_exceeded() {
                        return Err(ferrython_core::error::PyException::recursion_error(
                            "maximum recursion depth exceeded while getting the repr of an object",
                        ));
                    }
                    return Ok("frozenset(...)".to_string());
                }
                if m.is_empty() {
                    ferrython_core::object::repr_leave(ptr);
                    return Ok("frozenset()".to_string());
                }
                let items = m.items.clone();
                let mut parts = Vec::new();
                for v in items.values() {
                    match self.vm_repr(v) {
                        Ok(text) => parts.push(text),
                        Err(err) => {
                            ferrython_core::object::repr_leave(ptr);
                            return Err(err);
                        }
                    }
                }
                parts.sort();
                ferrython_core::object::repr_leave(ptr);
                Ok(format!("frozenset({{{}}})", parts.join(", ")))
            }
            _ => Ok(obj.repr()),
        }
    }
}
