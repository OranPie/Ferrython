use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    has_descriptor_get, is_data_descriptor, lookup_in_class_mro, new_fx_hashkey_flatmap,
    new_fx_hashkey_map, FxHashKeyMap, IteratorData, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef, CLASS_FLAG_HAS_DESCRIPTORS, CLASS_FLAG_HAS_SETATTR,
    CLASS_FLAG_HAS_SLOTS,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

use crate::builtins;
use crate::frame::ScopeKind;
use crate::vm_call::str_fast::fast_exact_str;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_or_type(
        &mut self,
        func: &PyObjectRef,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if name.as_str() == "__build_class__" {
            return self.build_class(args);
        }
        // VM-aware builtins that need to call user-defined methods
        match name.as_str() {
            "globals" => {
                // Return an InstanceDict that shares the frame's globals Arc.
                // This means mutations via globals()['key'] = value propagate directly.
                if let Some(frame) = self.call_stack.last() {
                    if let Some(globals_obj) = &frame.exec_globals {
                        return Ok(globals_obj.clone());
                    }
                    let globals_arc = frame.globals.clone();
                    return Ok(PyObject::wrap(PyObjectPayload::InstanceDict(globals_arc)));
                }
                return Ok(PyObject::dict(new_fx_hashkey_map()));
            }
            "locals" => {
                if let Some(frame) = self.call_stack.last() {
                    if let Some(locals) = &frame.exec_locals {
                        return Ok(locals.clone());
                    }
                    if matches!(frame.scope_kind, ScopeKind::Module) {
                        if let Some(globals_obj) = &frame.exec_globals {
                            return Ok(globals_obj.clone());
                        }
                    }
                    let mut map = IndexMap::new();
                    // Include function-scope locals (varnames → locals array)
                    for (i, name) in frame.code.varnames.iter().enumerate() {
                        if let Some(Some(val)) = frame.locals.get(i) {
                            map.insert(HashableKey::str_key(name.clone()), val.clone());
                        }
                    }
                    // If no varnames (module scope), include globals + local_names
                    if frame.code.varnames.is_empty() {
                        let g = frame.globals.read();
                        for (k, v) in g.iter() {
                            map.insert(HashableKey::str_key(k.clone()), v.clone());
                        }
                        drop(g);
                        for (k, v) in frame.local_names_iter() {
                            map.insert(HashableKey::str_key(k.clone()), v.clone());
                        }
                    }
                    return Ok(PyObject::dict(map));
                }
                return Ok(PyObject::dict(new_fx_hashkey_map()));
            }
            "print" => {
                return self.vm_print(&args, None, None, None, false);
            }
            "str" => {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                // str(bytes, encoding[, errors]) — decode bytes
                if args.len() >= 2 {
                    match &args[0].payload {
                        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                            let s = String::from_utf8_lossy(b);
                            return Ok(PyObject::str_val(CompactString::from(s.as_ref())));
                        }
                        _ => {}
                    }
                }
                if args.len() == 1 {
                    if let Some(result) = fast_exact_str(&args[0]) {
                        return Ok(result);
                    }
                }
                return self
                    .vm_str(&args[0])
                    .map(|s| PyObject::str_val(CompactString::from(s)));
            }
            "bytes" => {
                return self.vm_bytes_constructor(&args, false);
            }
            "bytearray" => {
                return self.vm_bytes_constructor(&args, true);
            }
            "repr" => {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                return self
                    .vm_repr(&args[0])
                    .map(|s| PyObject::str_val(CompactString::from(s)));
            }
            "map" => {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "map() requires at least 2 arguments",
                    ));
                }
                let func_obj = args[0].clone();
                let mut sources = Vec::with_capacity(args.len() - 1);
                for a in &args[1..] {
                    sources.push(self.resolve_iterable(a)?);
                }
                if sources.len() == 1 {
                    return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                        PyCell::new(IteratorData::MapOne {
                            func: func_obj,
                            source: sources.pop().unwrap(),
                        }),
                    ))));
                }
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::Map {
                        func: func_obj,
                        sources,
                    }),
                ))));
            }
            "filter" => {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "filter() requires at least 2 arguments",
                    ));
                }
                let func_obj = args[0].clone();
                let source = self.resolve_iterable(&args[1])?;
                return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                    PyCell::new(IteratorData::Filter {
                        func: func_obj,
                        source,
                    }),
                ))));
            }
            "iter" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if let Some(raw_iter) = Self::resolve_instance_dunder(&args[0], "__iter__")
                        {
                            let iter_method = self.resolve_descriptor(&raw_iter, &args[0])?;
                            let r = self.call_object(iter_method, vec![])?;
                            return Self::ensure_iterator_result(&args[0], r);
                        }
                        if inst.dict_storage.is_some() {
                            return args[0].get_iter();
                        }
                        // Builtin base type subclass: delegate to __builtin_value__
                        if let Some(bv) = Self::get_builtin_value(&args[0]) {
                            let iter = self.resolve_iterable(&bv)?;
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                PyCell::new(IteratorData::HeldIter {
                                    iter,
                                    owner: Some(args[0].clone()),
                                }),
                            ))));
                        }
                        // Old-style sequence protocol: lazy SeqIter
                        if args[0].get_attr("__getitem__").is_some() {
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                PyCell::new(IteratorData::SeqIter {
                                    obj: args[0].clone(),
                                    index: 0,
                                    exhausted: false,
                                }),
                            ))));
                        }
                        return Err(PyException::type_error(format!(
                            "'{}' object is not iterable",
                            args[0].type_name()
                        )));
                    }
                    // Fall through to builtin dispatch for non-instances
                }
            }
            "next" => {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "next() requires at least 1 argument",
                    ));
                }
                // For generators, resume directly so StopIteration return value propagates
                if let PyObjectPayload::Generator(gen_arc) = &args[0].payload {
                    match self.resume_generator(gen_arc, PyObject::none()) {
                        Ok(value) => return Ok(value),
                        Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                            return Ok(args[1].clone());
                        }
                        Err(e) => return Err(e),
                    }
                }
                // Use vm_iter_next which handles instances and lazy iterators
                match self.vm_iter_next(&args[0]) {
                    Ok(Some(value)) => return Ok(value),
                    Ok(None) => {
                        if args.len() > 1 {
                            return Ok(args[1].clone()); // default value
                        }
                        return Err(PyException::new(ExceptionKind::StopIteration, ""));
                    }
                    Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                        return Ok(args[1].clone());
                    }
                    Err(e) => return Err(e),
                }
            }
            "list" => {
                if args.is_empty() {
                    return Ok(PyObject::list(vec![]));
                }
                let items = self.collect_iterable(&args[0])?;
                return Ok(PyObject::list(items));
            }
            "tuple" => {
                if args.is_empty() {
                    return Ok(PyObject::tuple(vec![]));
                }
                let items = self.collect_iterable(&args[0])?;
                return Ok(PyObject::tuple(items));
            }
            "sum" => {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "sum() requires at least 1 argument",
                    ));
                }
                let start = if args.len() > 1 {
                    args[1].clone()
                } else {
                    PyObject::int(0)
                };
                let mut total = start;
                // Inline lazy iteration — avoid materializing entire iterable
                match &args[0].payload {
                    PyObjectPayload::List(cell) => {
                        let items = cell.read();
                        if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(s)) =
                            &total.payload
                        {
                            let mut acc: i64 = *s;
                            let mut fallback_idx = items.len();
                            for (i, item) in items.iter().enumerate() {
                                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(
                                    n,
                                )) = &item.payload
                                {
                                    acc = acc.wrapping_add(*n);
                                } else {
                                    total = PyObject::int(acc);
                                    total = self.vm_add(&total, item)?;
                                    fallback_idx = i + 1;
                                    break;
                                }
                            }
                            if fallback_idx < items.len() {
                                for item in &items[fallback_idx..] {
                                    total = self.vm_add(&total, item)?;
                                }
                            } else {
                                total = PyObject::int(acc);
                            }
                        } else {
                            for item in items.iter() {
                                total = self.vm_add(&total, item)?;
                            }
                        }
                    }
                    PyObjectPayload::Tuple(items) => {
                        if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(s)) =
                            &total.payload
                        {
                            let mut acc: i64 = *s;
                            let mut fallback_idx = items.len();
                            for (i, item) in items.iter().enumerate() {
                                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(
                                    n,
                                )) = &item.payload
                                {
                                    acc = acc.wrapping_add(*n);
                                } else {
                                    total = PyObject::int(acc);
                                    total = self.vm_add(&total, item)?;
                                    fallback_idx = i + 1;
                                    break;
                                }
                            }
                            if fallback_idx < items.len() {
                                for item in &items[fallback_idx..] {
                                    total = self.vm_add(&total, item)?;
                                }
                            } else {
                                total = PyObject::int(acc);
                            }
                        } else {
                            for item in items.iter() {
                                total = self.vm_add(&total, item)?;
                            }
                        }
                    }
                    PyObjectPayload::Range(rd) => {
                        // O(1) arithmetic sum for integer ranges
                        let (s, e, st) = (rd.start, rd.stop, rd.step);
                        let n = if st > 0 {
                            if e > s {
                                (e - s - 1) / st + 1
                            } else {
                                0
                            }
                        } else if st < 0 {
                            if s > e {
                                (s - e - 1) / (-st) + 1
                            } else {
                                0
                            }
                        } else {
                            0
                        };
                        if n > 0 {
                            // Gauss: sum = n*start + step*n*(n-1)/2
                            let range_sum = n
                                .wrapping_mul(s)
                                .wrapping_add(st.wrapping_mul(n).wrapping_mul(n - 1) / 2);
                            total = self.vm_add(&total, &PyObject::int(range_sum))?;
                        }
                    }
                    PyObjectPayload::RangeIter(ri) => {
                        // O(1) arithmetic sum for range iterators
                        let c = ri.current.get();
                        let s = ri.stop;
                        let st = ri.step;
                        let n = if st > 0 {
                            if s > c {
                                (s - c - 1) / st + 1
                            } else {
                                0
                            }
                        } else if st < 0 {
                            if c > s {
                                (c - s - 1) / (-st) + 1
                            } else {
                                0
                            }
                        } else {
                            0
                        };
                        if n > 0 {
                            let range_sum = n
                                .wrapping_mul(c)
                                .wrapping_add(st.wrapping_mul(n).wrapping_mul(n - 1) / 2);
                            total = self.vm_add(&total, &PyObject::int(range_sum))?;
                            ri.current.set(c + st * n); // advance iterator to exhaustion
                        }
                    }
                    PyObjectPayload::Iterator(_) => {
                        let items = self.collect_iterable(&args[0])?;
                        // Native i64 accumulation for homogeneous int iterators
                        if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(s)) =
                            &total.payload
                        {
                            let mut acc: i64 = *s;
                            let mut fallback_idx = items.len();
                            for (i, item) in items.iter().enumerate() {
                                if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(
                                    n,
                                )) = &item.payload
                                {
                                    acc = acc.wrapping_add(*n);
                                } else {
                                    total = PyObject::int(acc);
                                    total = self.vm_add(&total, &item)?;
                                    fallback_idx = i + 1;
                                    break;
                                }
                            }
                            if fallback_idx < items.len() {
                                for item in &items[fallback_idx..] {
                                    total = self.vm_add(&total, &item)?;
                                }
                            } else {
                                total = PyObject::int(acc);
                            }
                        } else {
                            for item in items {
                                total = self.vm_add(&total, &item)?;
                            }
                        }
                    }
                    PyObjectPayload::Generator(gen_arc) => {
                        let gen_arc = gen_arc.clone();
                        // Native i64 accumulation for homogeneous int generators
                        if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(s)) =
                            &total.payload
                        {
                            let mut acc: i64 = *s;
                            let mut use_native = true;
                            loop {
                                match self.resume_generator_for_iter(&gen_arc) {
                                    Ok(Some(item)) => {
                                        if let PyObjectPayload::Int(
                                            ferrython_core::types::PyInt::Small(n),
                                        ) = &item.payload
                                        {
                                            acc = acc.wrapping_add(*n);
                                        } else {
                                            // Switch to generic accumulation
                                            total = PyObject::int(acc);
                                            total = self.vm_add(&total, &item)?;
                                            use_native = false;
                                            break;
                                        }
                                    }
                                    Ok(None) => break,
                                    Err(e) => return Err(e),
                                }
                            }
                            if use_native {
                                return Ok(PyObject::int(acc));
                            }
                            // Fell through — continue with generic total
                            loop {
                                match self.resume_generator_for_iter(&gen_arc) {
                                    Ok(Some(item)) => {
                                        total = self.vm_add(&total, &item)?;
                                    }
                                    Ok(None) => break,
                                    Err(e) => return Err(e),
                                }
                            }
                        } else {
                            loop {
                                match self.resume_generator_for_iter(&gen_arc) {
                                    Ok(Some(item)) => {
                                        total = self.vm_add(&total, &item)?;
                                    }
                                    Ok(None) => break,
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                    }
                    _ => {
                        let items = self.collect_iterable(&args[0])?;
                        for item in items {
                            total = self.vm_add(&total, &item)?;
                        }
                    }
                }
                return Ok(total);
            }
            "sorted" => {
                if !args.is_empty() {
                    // Steal contents if list is temporary (refcount==1) — avoids clone
                    let mut items = if let PyObjectPayload::List(ref cell) = args[0].payload {
                        if PyObjectRef::strong_count(&args[0]) == 1 {
                            std::mem::take(&mut *cell.write())
                        } else {
                            cell.read().clone()
                        }
                    } else if let PyObjectPayload::Tuple(ref t) = args[0].payload {
                        t.to_vec()
                    } else {
                        self.collect_iterable(&args[0])?
                    };
                    self.vm_sort(&mut items)?;
                    return Ok(PyObject::list(items));
                }
            }
            "set" => {
                if args.len() > 1 {
                    return builtins::dispatch("set", &args);
                }
                if args.is_empty() {
                    return builtins::dispatch("set", &[]);
                }
                if let PyObjectPayload::Dict(items) = &args[0].payload {
                    let read = items.read();
                    let mut map = new_fx_hashkey_flatmap();
                    map.reserve(read.len());
                    for key in read.keys() {
                        map.insert(key.clone(), key.to_object());
                    }
                    return Ok(PyObject::set_from_flatmap(map));
                }
                let items = self.collect_iterable(&args[0])?;
                return builtins::dispatch("set", &[PyObject::list(items)]);
            }
            "frozenset" => {
                if args.len() > 1 {
                    return builtins::dispatch("frozenset", &args);
                }
                if args.is_empty() {
                    return builtins::dispatch("frozenset", &[]);
                }
                if let PyObjectPayload::Dict(items) = &args[0].payload {
                    let read = items.read();
                    let mut map = new_fx_hashkey_map();
                    for key in read.keys() {
                        map.insert(key.clone(), key.to_object());
                    }
                    return Ok(PyObject::frozenset(map));
                }
                let items = self.collect_iterable(&args[0])?;
                return builtins::dispatch("frozenset", &[PyObject::list(items)]);
            }
            "dict" => {
                if args.is_empty() {
                    return Ok(PyObject::dict(new_fx_hashkey_map()));
                }
                // dict(mapping) — handle Dict payload
                if let PyObjectPayload::Dict(_) = &args[0].payload {
                    return builtins::dispatch("dict", &args);
                }
                // dict(MappingProxy) — e.g., cls.__dict__
                if let PyObjectPayload::MappingProxy(src) = &args[0].payload {
                    return Ok(PyObject::dict(src.read().clone()));
                }
                // dict(InstanceDict) — e.g., obj.__dict__
                if let PyObjectPayload::InstanceDict(src) = &args[0].payload {
                    let read = src.read();
                    let mut map = IndexMap::new();
                    for (k, v) in read.iter() {
                        map.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                    return Ok(PyObject::dict(map));
                }
                // dict(instance_with_dict_storage) — e.g., defaultdict, OrderedDict
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if let Some(ref ds) = inst.dict_storage {
                        let mut map = IndexMap::new();
                        for (k, v) in ds.read().iter() {
                            map.insert(k.clone(), v.clone());
                        }
                        return Ok(PyObject::dict(map));
                    }
                    if let Some(keys_method) = args[0].get_attr("keys") {
                        let keys_obj = self.call_object(keys_method, vec![])?;
                        let keys = self.collect_iterable(&keys_obj)?;
                        let mut map = IndexMap::new();
                        for key_obj in keys {
                            let value = args[0].get_item(&key_obj)?;
                            map.insert(key_obj.to_hashable_key()?, value);
                        }
                        return Ok(PyObject::dict(map));
                    }
                    if inst.attrs.read().contains_key("__chainmap__") {
                        if let Some(items_method) = args[0].get_attr("items") {
                            let items_obj = self.call_object(items_method, vec![])?;
                            let items = self.collect_iterable(&items_obj)?;
                            let mut map = IndexMap::new();
                            for item in &items {
                                let kv = item.to_list()?;
                                if kv.len() == 2 {
                                    let key = kv[0].to_hashable_key()?;
                                    map.insert(key, kv[1].clone());
                                }
                            }
                            return Ok(PyObject::dict(map));
                        }
                    }
                }
                // dict(iterable_of_pairs)
                let items = self.collect_iterable(&args[0])?;
                return builtins::dispatch("dict", &[PyObject::list(items)]);
            }
            "any" => {
                if !args.is_empty() {
                    let iter_obj = builtins::get_iter_from_obj_pub(&args[0])?;
                    loop {
                        match self.vm_iter_next(&iter_obj)? {
                            Some(item) => {
                                if item.is_truthy() {
                                    return Ok(PyObject::bool_val(true));
                                }
                            }
                            None => return Ok(PyObject::bool_val(false)),
                        }
                    }
                }
            }
            "all" => {
                if !args.is_empty() {
                    let iter_obj = builtins::get_iter_from_obj_pub(&args[0])?;
                    loop {
                        match self.vm_iter_next(&iter_obj)? {
                            Some(item) => {
                                if !item.is_truthy() {
                                    return Ok(PyObject::bool_val(false));
                                }
                            }
                            None => return Ok(PyObject::bool_val(true)),
                        }
                    }
                }
            }
            "isinstance" => {
                if args.len() == 2 {
                    let cls = &args[1];
                    // Check for metaclass __instancecheck__ on user-defined classes
                    if let PyObjectPayload::Class(cd) = &cls.payload {
                        if let Some(ref metaclass) = cd.metaclass {
                            if let Some(ic) = metaclass.get_attr("__instancecheck__") {
                                let result =
                                    self.call_object(ic, vec![cls.clone(), args[0].clone()])?;
                                return Ok(PyObject::bool_val(result.is_truthy()));
                            }
                        }
                        // Check __subclasshook__ on the class (ABC protocol)
                        if let Some(hook) = cls.get_attr("__subclasshook__") {
                            // Pass the type of the object being checked
                            let obj = &args[0];
                            let obj_type = match &obj.payload {
                                PyObjectPayload::Instance(inst) => inst.class.clone(),
                                _ => PyObject::builtin_type(CompactString::from(obj.type_name())),
                            };
                            if let Ok(result) = self.call_object(hook, vec![obj_type]) {
                                if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                    return Ok(PyObject::bool_val(result.is_truthy()));
                                }
                            }
                        }
                        // Check for runtime_checkable Protocol — structural subtyping
                        let ns = cd.namespace.read();
                        if ns
                            .get("_is_runtime_checkable")
                            .map_or(false, |v| v.is_truthy())
                        {
                            if let Some(protocol_attrs) = ns.get("__protocol_attrs__") {
                                if let PyObjectPayload::Tuple(required) = &protocol_attrs.payload {
                                    let obj = &args[0];
                                    let has_all = required.iter().all(|attr_name| {
                                        let name = attr_name.py_to_string();
                                        obj.get_attr(&name).is_some()
                                    });
                                    return Ok(PyObject::bool_val(has_all));
                                }
                            }
                        }
                    }
                }
            }
            "issubclass" => {
                if args.len() == 2 {
                    let sup = &args[1];
                    if let PyObjectPayload::Class(cd) = &sup.payload {
                        // Check metaclass __subclasscheck__ first
                        if let Some(ref metaclass) = cd.metaclass {
                            if let Some(sc) = metaclass.get_attr("__subclasscheck__") {
                                let result =
                                    self.call_object(sc, vec![sup.clone(), args[0].clone()])?;
                                return Ok(PyObject::bool_val(result.is_truthy()));
                            }
                        }
                        // Check __subclasshook__ on the superclass (ABC protocol)
                        if let Some(hook) = sup.get_attr("__subclasshook__") {
                            if let Ok(result) = self.call_object(hook, vec![args[0].clone()]) {
                                if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                    return Ok(PyObject::bool_val(result.is_truthy()));
                                }
                            }
                        }
                    }
                }
            }
            "min" => {
                if args.len() == 1 {
                    if let Some(r) = self.native_min_max_list(&args[0], false)? {
                        return Ok(r);
                    }
                    let items = self.collect_iterable(&args[0])?;
                    return self.compute_min_max(items, false, None, None, "min");
                }
            }
            "max" => {
                if args.len() == 1 {
                    if let Some(r) = self.native_min_max_list(&args[0], true)? {
                        return Ok(r);
                    }
                    let items = self.collect_iterable(&args[0])?;
                    return self.compute_min_max(items, true, None, None, "max");
                }
            }
            "reversed" => {
                if !args.is_empty() {
                    if matches!(&args[0].payload, PyObjectPayload::List(_)) {
                        return builtins::dispatch("reversed", &[args[0].clone()]);
                    }
                    // Check for __reversed__ dunder on instances
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(rev_method) =
                            Self::resolve_instance_dunder(&args[0], "__reversed__")
                        {
                            return self.call_object(rev_method, vec![]);
                        }
                        if let Some(bv) = Self::get_builtin_value(&args[0]) {
                            let items = self.collect_iterable(&bv)?;
                            let iter = builtins::dispatch("reversed", &[PyObject::list(items)])?;
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                                PyCell::new(IteratorData::HeldIter {
                                    iter,
                                    owner: Some(args[0].clone()),
                                }),
                            ))));
                        }
                    }
                    let items = self.collect_iterable(&args[0])?;
                    return builtins::dispatch("reversed", &[PyObject::list(items)]);
                }
            }
            "enumerate" => {
                if !args.is_empty() {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(self.resolve_iterable(&args[0])?);
                    resolved.extend_from_slice(&args[1..]);
                    return builtins::dispatch("enumerate", &resolved);
                }
                return builtins::dispatch("enumerate", &args);
            }
            "zip" => {
                // Check for trailing kwargs dict (e.g. strict=True)
                let mut strict = false;
                let iter_end = if let Some(last) = args.last() {
                    if let PyObjectPayload::Dict(kw) = &last.payload {
                        let r = kw.read();
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("strict")))
                        {
                            strict = v.is_truthy();
                        }
                        drop(r);
                        args.len() - 1
                    } else {
                        args.len()
                    }
                } else {
                    args.len()
                };
                let resolved = self.resolve_iterables(&args[..iter_end])?;
                let mut full_args = resolved;
                if strict {
                    // Re-add kwargs dict so builtin_zip can pick it up
                    let kw = PyObject::dict(indexmap::IndexMap::from([(
                        HashableKey::str_key(CompactString::from("strict")),
                        PyObject::bool_val(true),
                    )]));
                    full_args.push(kw);
                }
                return builtins::dispatch("zip", &full_args);
            }
            "len" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if inst.attrs.read().contains_key("__chainmap__") {
                            if let Some(method) = args[0].get_attr("__len__") {
                                let result = self.call_object(method, vec![])?;
                                return Ok(result);
                            }
                        }
                        // Dict subclass: use dict_storage length
                        if let Some(ref ds) = inst.dict_storage {
                            return Ok(PyObject::int(ds.read().len() as i64));
                        }
                        // Namedtuple: delegate to call_namedtuple_method
                        if inst.class.get_attr("__namedtuple__").is_some() {
                            return builtins::call_method(&args[0], "__len__", &[]);
                        }
                        // Check for custom __len__ (skip BuiltinBoundMethod from BuiltinType base)
                        if let Some(method) = args[0].get_attr("__len__") {
                            if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                                let ca = if matches!(
                                    &method.payload,
                                    PyObjectPayload::BoundMethod { .. }
                                ) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                                return self.call_object(method, ca);
                            }
                        }
                        // Builtin base type subclass (list, tuple, etc.)
                        if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if let Ok(n) = bv.py_len() {
                                return Ok(PyObject::int(n as i64));
                            }
                        }
                    }
                }
            }
            "abs" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__abs__") {
                            let call_args =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, call_args);
                        }
                    }
                }
            }
            "hash" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__hash__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca);
                        }
                    }
                }
            }
            "bin" | "oct" | "hex" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__index__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            let idx_val = self.call_object(method, ca)?;
                            // Re-call bin/oct/hex with the resolved int
                            return self.call_object(func.clone(), vec![idx_val]);
                        }
                    }
                }
            }
            "format" => {
                if !args.is_empty() {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__format__")
                        {
                            let spec = if args.len() > 1 {
                                args[1].clone()
                            } else {
                                PyObject::str_val(CompactString::from(""))
                            };
                            let mut ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            ca.push(spec);
                            return self.call_object(method, ca);
                        }
                        // No __format__: use __str__ for empty/no spec (CPython default __format__)
                        let has_spec = args.len() > 1 && !args[1].py_to_string().is_empty();
                        if !has_spec {
                            let s = self.vm_str(&args[0])?;
                            return Ok(PyObject::str_val(CompactString::from(s)));
                        }
                    }
                    // Fall through to native format
                }
            }
            "complex" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        // Check for user-defined __complex__ FIRST (takes priority over __builtin_value__)
                        let has_user_complex = inst.class.get_attr("__complex__").is_some() && {
                            // Distinguish user-defined from inherited builtin
                            let m = Self::resolve_instance_dunder(&args[0], "__complex__");
                            matches!(
                                m.as_ref().map(|o| &o.payload),
                                Some(
                                    PyObjectPayload::BoundMethod { .. }
                                        | PyObjectPayload::Function(_)
                                )
                            )
                        };
                        if has_user_complex {
                            if let Some(method) =
                                Self::resolve_instance_dunder(&args[0], "__complex__")
                            {
                                let ca = if matches!(
                                    &method.payload,
                                    PyObjectPayload::BoundMethod { .. }
                                ) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                                let result = self.call_object(method, ca)?;
                                match &result.payload {
                                    PyObjectPayload::Complex { .. } => return Ok(result),
                                    PyObjectPayload::Instance(i2) => {
                                        // subclass of complex — extract via __builtin_value__
                                        if let Some(v) =
                                            i2.attrs.read().get("__builtin_value__").cloned()
                                        {
                                            if matches!(&v.payload, PyObjectPayload::Complex { .. })
                                            {
                                                return Ok(v);
                                            }
                                        }
                                        return Err(PyException::type_error(format!(
                                            "__complex__ returned non-complex (type {})",
                                            result.type_name()
                                        )));
                                    }
                                    _ => {
                                        return Err(PyException::type_error(format!(
                                            "__complex__ returned non-complex (type {})",
                                            result.type_name()
                                        )))
                                    }
                                }
                            }
                        }
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Complex { .. }) {
                                return Ok(val);
                            }
                        }
                        // Fallback: __float__
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            let result = self.call_object(method, ca)?;
                            match &result.payload {
                                PyObjectPayload::Float(f) => return Ok(PyObject::complex(*f, 0.0)),
                                PyObjectPayload::Int(n) => {
                                    return Ok(PyObject::complex(n.to_f64(), 0.0))
                                }
                                PyObjectPayload::Bool(b) => {
                                    return Ok(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0))
                                }
                                _ => {
                                    return Err(PyException::type_error(format!(
                                        "__float__ returned non-float (type {})",
                                        result.type_name()
                                    )))
                                }
                            }
                        }
                        // Fallback: __index__
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__index__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            let result = self.call_object(method, ca)?;
                            match &result.payload {
                                PyObjectPayload::Int(n) => {
                                    let f = n.to_f64();
                                    if f.is_infinite() {
                                        return Err(PyException::overflow_error(
                                            "int too large to convert to float",
                                        ));
                                    }
                                    return Ok(PyObject::complex(f, 0.0));
                                }
                                PyObjectPayload::Bool(b) => {
                                    return Ok(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0))
                                }
                                _ => {
                                    return Err(PyException::type_error(format!(
                                        "__index__ returned non-int (type {})",
                                        result.type_name()
                                    )))
                                }
                            }
                        }
                        return Err(PyException::type_error(format!(
                            "complex() first argument must be a string or a number, not '{}'",
                            args[0].type_name()
                        )));
                    }
                } else if args.len() == 2 {
                    // Handle instances as either arg via __float__/__index__/__complex__
                    let coerce_for_complex = |vm: &mut Self,
                                              obj: &PyObjectRef,
                                              which: &str|
                     -> PyResult<PyObjectRef> {
                        if matches!(
                            &obj.payload,
                            PyObjectPayload::Complex { .. }
                                | PyObjectPayload::Int(_)
                                | PyObjectPayload::Float(_)
                                | PyObjectPayload::Bool(_)
                        ) {
                            return Ok(obj.clone());
                        }
                        if let PyObjectPayload::Instance(inst) = &obj.payload {
                            if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                                if matches!(
                                    &val.payload,
                                    PyObjectPayload::Complex { .. }
                                        | PyObjectPayload::Int(_)
                                        | PyObjectPayload::Float(_)
                                ) {
                                    return Ok(val);
                                }
                            }
                            for dunder in &["__complex__", "__float__", "__index__"] {
                                if let Some(method) = Self::resolve_instance_dunder(obj, dunder) {
                                    let ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![obj.clone()]
                                    };
                                    let res = vm.call_object(method, ca)?;
                                    if matches!(
                                        &res.payload,
                                        PyObjectPayload::Complex { .. }
                                            | PyObjectPayload::Int(_)
                                            | PyObjectPayload::Float(_)
                                            | PyObjectPayload::Bool(_)
                                    ) {
                                        return Ok(res);
                                    }
                                }
                            }
                        }
                        Err(PyException::type_error(format!(
                            "complex() {} argument must be a number, not '{}'",
                            which,
                            obj.type_name()
                        )))
                    };
                    let has_inst = matches!(&args[0].payload, PyObjectPayload::Instance(_))
                        || matches!(&args[1].payload, PyObjectPayload::Instance(_));
                    if has_inst {
                        let which_first = if matches!(&args[0].payload, PyObjectPayload::Str(_)) {
                            ""
                        } else {
                            "first"
                        };
                        let which_second = "second";
                        let a = coerce_for_complex(
                            self,
                            &args[0],
                            if which_first.is_empty() {
                                "first"
                            } else {
                                which_first
                            },
                        )?;
                        let b = coerce_for_complex(self, &args[1], which_second)?;
                        return crate::builtins::core_fns::builtin_complex(&[a, b]);
                    }
                }
            }
            "int" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        // Check for __builtin_value__ first (int subclass)
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Int(_)) {
                                return Ok(val);
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__int__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca);
                        }
                    }
                }
            }
            "float" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        // Check for __builtin_value__ first (float subclass)
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Float(_)) {
                                return Ok(val);
                            }
                            // int subclass → convert to float
                            if let PyObjectPayload::Int(n) = &val.payload {
                                return Ok(PyObject::float(n.to_f64()));
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca);
                        }
                    }
                }
            }
            "round" => {
                if !args.is_empty() {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__round__") {
                            let mut ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            if args.len() >= 2 {
                                ca.push(args[1].clone());
                            }
                            return self.call_object(method, ca);
                        }
                    }
                }
            }
            "bool" => {
                if args.len() == 1 {
                    let obj = &args[0];
                    if let ferrython_core::object::PyObjectPayload::Instance(inst) = &obj.payload {
                        if let Some(target_fn) =
                            inst.attrs.read().get("__weakref_target__").cloned()
                        {
                            let referent = self.call_object(target_fn, vec![])?;
                            return Ok(PyObject::bool_val(self.vm_is_truthy(&referent)?));
                        }
                    }
                    // Instance with __bool__: call it and enforce return type == bool
                    if let ferrython_core::object::PyObjectPayload::Instance(_) = &obj.payload {
                        if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__bool__") {
                            let method = self.resolve_descriptor(&raw_method, obj)?;
                            let result = self.call_object(method, vec![])?;
                            if !matches!(
                                &result.payload,
                                ferrython_core::object::PyObjectPayload::Bool(_)
                            ) {
                                let tn = result.type_name();
                                return Err(ferrython_core::error::PyException::type_error(
                                    compact_str::CompactString::from(format!(
                                        "__bool__ should return bool, returned {}",
                                        tn
                                    )),
                                ));
                            }
                            return Ok(result);
                        }
                        if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__len__") {
                            let method = self.resolve_descriptor(&raw_method, obj)?;
                            let result = self.call_object(method, vec![])?;
                            match &result.payload {
                                ferrython_core::object::PyObjectPayload::Int(n) => {
                                    let is_neg = n.to_i64().map(|v| v < 0).unwrap_or(false);
                                    if is_neg {
                                        return Err(
                                            ferrython_core::error::PyException::value_error(
                                                compact_str::CompactString::from(
                                                    "__len__() should return >= 0",
                                                ),
                                            ),
                                        );
                                    }
                                    return Ok(PyObject::bool_val(!n.is_zero()));
                                }
                                ferrython_core::object::PyObjectPayload::Bool(b) => {
                                    return Ok(PyObject::bool_val(*b));
                                }
                                _ => {
                                    let tn = result.type_name();
                                    return Err(ferrython_core::error::PyException::type_error(
                                        compact_str::CompactString::from(format!(
                                            "__len__() should return >= 0, returned {}",
                                            tn
                                        )),
                                    ));
                                }
                            }
                        }
                    }
                    return Ok(PyObject::bool_val(self.vm_is_truthy(obj)?));
                }
            }
            "mappingproxy" => {
                // types.MappingProxyType(dict) — read-only view of a dict
                if args.len() == 1 {
                    let src = &args[0];
                    let map = match &src.payload {
                        PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => {
                            m.read().clone()
                        }
                        PyObjectPayload::InstanceDict(attrs) => {
                            let rd = attrs.read();
                            let mut m = new_fx_hashkey_map();
                            for (k, v) in rd.iter() {
                                m.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                            m
                        }
                        _ => {
                            return Err(PyException::type_error(
                                "mappingproxy() argument must be a mapping, not a non-mapping type",
                            ));
                        }
                    };
                    return Ok(PyObject::wrap(PyObjectPayload::MappingProxy(Rc::new(
                        PyCell::new(map),
                    ))));
                }
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "mappingproxy() missing required argument: 'mapping'",
                    ));
                }
            }
            "dir" => {
                if args.is_empty() {
                    if let Some(locals) = self.call_stack.last().and_then(|f| f.exec_locals.clone())
                    {
                        let mut names: Vec<String> = self
                            .exec_locals_keys(&locals)?
                            .into_iter()
                            .map(|key| key.py_to_string())
                            .collect();
                        names.sort();
                        let items = names
                            .into_iter()
                            .map(|n| PyObject::str_val(CompactString::from(n)))
                            .collect();
                        return Ok(PyObject::list(items));
                    }
                    // dir() with no args: return sorted local variable names
                    let locals = self.collect_locals_dict()?;
                    if let PyObjectPayload::Dict(map) = &locals.payload {
                        let mut names: Vec<String> = map
                            .read()
                            .keys()
                            .map(|k| k.to_object().py_to_string())
                            .collect();
                        names.sort();
                        let items = names
                            .into_iter()
                            .map(|n| PyObject::str_val(CompactString::from(n)))
                            .collect();
                        return Ok(PyObject::list(items));
                    }
                }
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__dir__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca);
                        }
                    }
                }
            }
            "super" => {
                return self.make_super(&args);
            }
            "exec" => {
                return self.builtin_exec(&args);
            }
            "eval" => {
                return self.builtin_eval(&args);
            }
            "compile" => {
                return self.builtin_compile(&args);
            }
            "__import__" => {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "__import__() requires at least 1 argument",
                    ));
                }
                let name = args[0].py_to_string();
                let level = if args.len() >= 5 {
                    args[4].as_int().unwrap_or(0) as usize
                } else {
                    0
                };
                return self.import_module_simple(&name, level);
            }
            "vars" => {
                if args.is_empty() {
                    return self.collect_locals_dict();
                }
                // vars(obj) — fall through to static builtin_vars
            }
            "getattr" => {
                if args.len() < 2 || args.len() > 3 {
                    return Err(PyException::type_error("getattr expected 2 or 3 arguments"));
                }
                let attr_name = args[1].as_str().ok_or_else(|| {
                    PyException::type_error("getattr(): attribute name must be string")
                })?;
                if attr_name == "__isabstractmethod__"
                    && ferrython_core::object::is_property_like(&args[0])
                {
                    return self.property_isabstractmethod(&args[0]);
                }
                // Use get_attr which handles MRO + data descriptors
                match args[0].get_attr(attr_name) {
                    Some(v) => {
                        // Invoke descriptor protocol (Property, custom __get__)
                        if ferrython_core::object::is_property_like(&v) {
                            if matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                                return Ok(v);
                            }
                            if let Some(getter) = ferrython_core::object::property_field(&v, "fget")
                            {
                                if matches!(&getter.payload, PyObjectPayload::None) {
                                    return Err(PyException::attribute_error(format!(
                                        "unreadable attribute '{}'",
                                        attr_name
                                    )));
                                }
                                let getter = crate::builtins::unwrap_abstract_fget(&getter);
                                return self.call_object(getter, vec![args[0].clone()]);
                            }
                            return Err(PyException::attribute_error(format!(
                                "unreadable attribute '{}'",
                                attr_name
                            )));
                        }
                        if has_descriptor_get(&v) {
                            if let Some(get_method) = v.get_attr("__get__") {
                                let (inst_arg, owner_arg) = match &args[0].payload {
                                    PyObjectPayload::Instance(inst) => {
                                        (args[0].clone(), inst.class.clone())
                                    }
                                    PyObjectPayload::Class(_) => {
                                        (PyObject::none(), args[0].clone())
                                    }
                                    _ => (args[0].clone(), PyObject::none()),
                                };
                                // get_method is already a BoundMethod if from class MRO
                                return self.call_object(get_method, vec![inst_arg, owner_arg]);
                            }
                        }
                        return Ok(v);
                    }
                    None => {
                        // Try __getattr__ fallback
                        if let PyObjectPayload::Instance(_) = &args[0].payload {
                            if let Some(ga) = args[0].get_attr("__getattr__") {
                                let name_arg = PyObject::str_val(CompactString::from(attr_name));
                                return self.call_object(ga, vec![name_arg]);
                            }
                        }
                        if args.len() > 2 {
                            return Ok(args[2].clone());
                        }
                        return Err(PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'",
                            args[0].type_name(),
                            attr_name
                        )));
                    }
                }
            }
            "setattr" => {
                if args.len() != 3 {
                    return Err(PyException::type_error(
                        "setattr() takes exactly 3 arguments",
                    ));
                }
                let attr_name = args[1].py_to_string();
                let value = args[2].clone();
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if inst.class_flags
                        & (CLASS_FLAG_HAS_SETATTR
                            | CLASS_FLAG_HAS_DESCRIPTORS
                            | CLASS_FLAG_HAS_SLOTS)
                        == 0
                    {
                        inst.attrs
                            .write()
                            .insert(CompactString::from(attr_name.as_str()), value);
                        return Ok(PyObject::none());
                    }
                    if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                        if ferrython_core::object::is_property_like(&desc) {
                            if let Some(setter) =
                                ferrython_core::object::property_field(&desc, "fset")
                            {
                                if matches!(&setter.payload, PyObjectPayload::None) {
                                    return Err(PyException::attribute_error(format!(
                                        "can't set attribute '{}'",
                                        attr_name
                                    )));
                                }
                                self.call_object(setter, vec![args[0].clone(), value])?;
                                return Ok(PyObject::none());
                            } else {
                                return Err(PyException::attribute_error(format!(
                                    "can't set attribute '{}'",
                                    attr_name
                                )));
                            }
                        }
                        if is_data_descriptor(&desc) {
                            if let Some(set_method) = desc.get_attr("__set__") {
                                // set_method is already bound to desc
                                self.call_object(set_method, vec![args[0].clone(), value])?;
                                return Ok(PyObject::none());
                            }
                        }
                    }
                    if let Some(sa) = lookup_in_class_mro(&inst.class, "__setattr__") {
                        if matches!(&sa.payload, PyObjectPayload::Function(_)) {
                            let method = PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: args[0].clone(),
                                    method: sa,
                                },
                            });
                            self.call_object(
                                method,
                                vec![PyObject::str_val(CompactString::from(&attr_name)), value],
                            )?;
                            return Ok(PyObject::none());
                        }
                    }
                }
                return builtins::dispatch("setattr", &args);
            }
            "delattr" => {
                if args.len() != 2 {
                    return Err(PyException::type_error(
                        "delattr() takes exactly 2 arguments",
                    ));
                }
                let attr_name = args[1].py_to_string();
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                        if ferrython_core::object::is_property_like(&desc) {
                            if let Some(deleter) =
                                ferrython_core::object::property_field(&desc, "fdel")
                            {
                                if matches!(&deleter.payload, PyObjectPayload::None) {
                                    return Err(PyException::attribute_error(format!(
                                        "can't delete attribute '{}'",
                                        attr_name
                                    )));
                                }
                                self.call_object(deleter, vec![args[0].clone()])?;
                                return Ok(PyObject::none());
                            }
                            return Err(PyException::attribute_error(format!(
                                "can't delete attribute '{}'",
                                attr_name
                            )));
                        }
                    }
                }
                return builtins::dispatch("delattr", &args);
            }
            "NamedTuple" => {
                // typing.NamedTuple('Point', [('x', int), ('y', int)]) or NamedTuple('Point', x=int, y=int)
                if !args.is_empty() {
                    let typename = args[0].py_to_string();
                    let mut field_names: Vec<CompactString> = Vec::new();

                    // Check for kwargs dict as last arg
                    let kwargs_dict: Option<FxHashKeyMap> = if args.len() >= 2 {
                        if let PyObjectPayload::Dict(d) = &args[args.len() - 1].payload {
                            Some(d.read().clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    let has_kwargs = kwargs_dict.is_some();
                    let positional_end = if has_kwargs {
                        args.len() - 1
                    } else {
                        args.len()
                    };

                    if positional_end >= 2 {
                        match &args[1].payload {
                            PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
                                if let Ok(items) = args[1].to_list() {
                                    for item in &items {
                                        if let PyObjectPayload::Tuple(pair) = &item.payload {
                                            if !pair.is_empty() {
                                                field_names.push(CompactString::from(
                                                    pair[0].py_to_string(),
                                                ));
                                            }
                                        } else {
                                            field_names
                                                .push(CompactString::from(item.py_to_string()));
                                        }
                                    }
                                }
                            }
                            PyObjectPayload::Str(s) => {
                                for n in s.replace(',', " ").split_whitespace() {
                                    field_names.push(CompactString::from(n));
                                }
                            }
                            _ => {}
                        }
                    }

                    // kwargs form: NamedTuple('Point', x=int, y=int)
                    if let Some(ref kw) = kwargs_dict {
                        for (k, _v) in kw {
                            if let HashableKey::Str(fname) = k {
                                if fname.as_str() != "defaults"
                                    && fname.as_str() != "module"
                                    && fname.as_str() != "rename"
                                {
                                    let fname = fname.to_compact_string();
                                    if !field_names.contains(&fname) {
                                        field_names.push(fname);
                                    }
                                }
                            }
                        }
                    }

                    // Build namedtuple class with __namedtuple__ marker and _fields
                    let fields_tuple = PyObject::tuple(
                        field_names
                            .iter()
                            .map(|n| PyObject::str_val(n.clone()))
                            .collect(),
                    );
                    let mut ns = IndexMap::new();
                    ns.insert(
                        CompactString::from("__namedtuple__"),
                        PyObject::bool_val(true),
                    );
                    ns.insert(CompactString::from("_fields"), fields_tuple);
                    ns.insert(
                        CompactString::from("_field_defaults"),
                        PyObject::dict(new_fx_hashkey_map()),
                    );
                    return Ok(PyObject::class(CompactString::from(typename), vec![], ns));
                }
            }
            _ => {}
        }
        match builtins::get_builtin_fn(name.as_str()) {
            Some(f) => {
                let result = f(&args);
                // Check if breakpoint() was called
                if crate::builtins::core_fns::BREAKPOINT_TRIGGERED
                    .swap(false, std::sync::atomic::Ordering::Relaxed)
                {
                    self.breakpoints.builtin_breakpoint_pending = true;
                    self.handle_breakpoint_hit();
                }
                result
            }
            None => Err(PyException::type_error(format!(
                "'{}' is not callable",
                name
            ))),
        }
    }
}
