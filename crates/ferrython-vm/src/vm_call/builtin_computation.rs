use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_computation_builtin(
        &mut self,
        name: &str,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name {
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
                            let range_sum = n
                                .wrapping_mul(s)
                                .wrapping_add(st.wrapping_mul(n).wrapping_mul(n - 1) / 2);
                            total = self.vm_add(&total, &PyObject::int(range_sum))?;
                        }
                    }
                    PyObjectPayload::RangeIter(ri) => {
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
                            ri.current.set(c + st * n);
                        }
                    }
                    PyObjectPayload::Iterator(_) => {
                        let items = self.collect_iterable(&args[0])?;
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
                Ok(total)
            }
            "sorted" => {
                if !args.is_empty() {
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
                fallback_computation_builtin(name, &args)
            }
            "min" => {
                if args.len() == 1 {
                    if let Some(r) = self.native_min_max_list(&args[0], false)? {
                        return Ok(r);
                    }
                    let items = self.collect_iterable(&args[0])?;
                    return self.compute_min_max(items, false, None, None, "min");
                }
                fallback_computation_builtin(name, &args)
            }
            "max" => {
                if args.len() == 1 {
                    if let Some(r) = self.native_min_max_list(&args[0], true)? {
                        return Ok(r);
                    }
                    let items = self.collect_iterable(&args[0])?;
                    return self.compute_min_max(items, true, None, None, "max");
                }
                fallback_computation_builtin(name, &args)
            }
            _ => unreachable!("non-computation builtin routed to computation dispatch"),
        }
    }
}

fn fallback_computation_builtin(name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match crate::builtins::get_builtin_fn(name) {
        Some(f) => f(args),
        None => Err(PyException::type_error(format!(
            "'{}' is not callable",
            name
        ))),
    }
}
