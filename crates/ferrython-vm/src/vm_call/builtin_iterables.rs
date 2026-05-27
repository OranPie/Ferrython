use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    new_fx_hashkey_flatmap, new_fx_hashkey_map, IteratorData, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_iterable_builtin(
        &mut self,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name.as_str() {
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
            _ => {}
        }
        match builtins::get_builtin_fn(name.as_str()) {
            Some(f) => f(&args),
            None => Err(PyException::type_error(format!(
                "'{}' is not callable",
                name
            ))),
        }
    }
}
