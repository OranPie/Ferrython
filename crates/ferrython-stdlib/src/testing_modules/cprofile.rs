use compact_str::CompactString;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;

// ── cProfile module ──

pub fn create_cprofile_module() -> PyObjectRef {
    let run_fn = make_builtin(|args: &[PyObjectRef]| {
        if !args.is_empty() {
            let stmt = args[0].py_to_string();
            eprintln!("         1 function calls in 0.000 seconds");
            eprintln!("");
            eprintln!("   Ordered by: standard name");
            eprintln!("");
            eprintln!("   ncalls  tottime  percall  cumtime  percall filename:lineno(function)");
            eprintln!(
                "        1    0.000    0.000    0.000    0.000 <string>:1({})",
                stmt
            );
        }
        Ok(PyObject::none())
    });

    let profile_cls_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Profile"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let stats: Rc<PyCell<Vec<(String, i64, f64)>>> = Rc::new(PyCell::new(Vec::new()));
            let enabled: Rc<PyCell<bool>> = Rc::new(PyCell::new(false));
            let start_time: Rc<PyCell<Option<std::time::Instant>>> = Rc::new(PyCell::new(None));

            let e = enabled.clone();
            let st = start_time.clone();
            w.insert(
                CompactString::from("enable"),
                PyObject::native_closure("enable", move |_: &[PyObjectRef]| {
                    *e.write() = true;
                    *st.write() = Some(std::time::Instant::now());
                    Ok(PyObject::none())
                }),
            );
            let e2 = enabled.clone();
            let st2 = start_time.clone();
            let stats2 = stats.clone();
            w.insert(
                CompactString::from("disable"),
                PyObject::native_closure("disable", move |_: &[PyObjectRef]| {
                    *e2.write() = false;
                    if let Some(start) = st2.read().as_ref() {
                        stats2.write().push((
                            "profiling".to_string(),
                            1,
                            start.elapsed().as_secs_f64(),
                        ));
                    }
                    Ok(PyObject::none())
                }),
            );
            let stats3 = stats.clone();
            w.insert(
                CompactString::from("runcall"),
                PyObject::native_closure("runcall", move |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    let func = &args[0];
                    let func_args = if args.len() > 1 { &args[1..] } else { &[] };
                    let start = std::time::Instant::now();
                    let result = match &func.payload {
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(func_args)?,
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(func_args)?,
                        _ => PyObject::none(),
                    };
                    stats3
                        .write()
                        .push(("runcall".to_string(), 1, start.elapsed().as_secs_f64()));
                    Ok(result)
                }),
            );
            let stats4 = stats.clone();
            w.insert(
                CompactString::from("print_stats"),
                PyObject::native_closure("print_stats", move |args: &[PyObjectRef]| {
                    let sort_key =
                        if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                            args[0].py_to_string()
                        } else {
                            "cumulative".to_string()
                        };
                    let st = stats4.read();
                    let total: f64 = st.iter().map(|(_, _, t)| t).sum();
                    let ncalls: i64 = st.iter().map(|(_, n, _)| n).sum();
                    let mut lines = Vec::new();
                    lines.push(format!(
                        "         {} function calls in {:.3} seconds",
                        ncalls.max(1),
                        total
                    ));
                    lines.push(String::new());
                    lines.push(format!("   Ordered by: {}", sort_key));
                    lines.push(String::new());
                    lines.push(
                        "   ncalls  tottime  percall  cumtime  percall filename:lineno(function)"
                            .to_string(),
                    );
                    for (name, calls, time) in st.iter() {
                        let percall = if *calls > 0 {
                            time / *calls as f64
                        } else {
                            0.0
                        };
                        lines.push(format!(
                            "   {:>5}    {:.3}    {:.3}    {:.3}    {:.3} <string>:1({})",
                            calls, time, percall, time, percall, name
                        ));
                    }
                    let output = lines.join("\n") + "\n";
                    // Check for stream= kwarg (may be passed as last positional arg)
                    let mut wrote_to_stream = false;
                    for arg in args.iter() {
                        if let Some(_write) = arg.get_attr("write") {
                            // Looks like a stream — write to it via deferred call
                            if let PyObjectPayload::Instance(ref d) = arg.payload {
                                // Try StringIO-like direct write
                                if let Some(buf_ref) =
                                    d.attrs.read().get(&CompactString::from("_buffer"))
                                {
                                    if let PyObjectPayload::List(items) = &buf_ref.payload {
                                        items.write().push(PyObject::str_val(CompactString::from(
                                            output.as_str(),
                                        )));
                                        wrote_to_stream = true;
                                        break;
                                    }
                                }
                            }
                            // Fallback: call the write method
                            match &_write.payload {
                                PyObjectPayload::NativeFunction(nf) => {
                                    let _ = (nf.func)(&[PyObject::str_val(CompactString::from(
                                        output.as_str(),
                                    ))]);
                                    wrote_to_stream = true;
                                    break;
                                }
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[PyObject::str_val(CompactString::from(
                                        output.as_str(),
                                    ))]);
                                    wrote_to_stream = true;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    if !wrote_to_stream {
                        eprint!("{}", output);
                    }
                    Ok(PyObject::none())
                }),
            );
            // getstats() — return stats as list of tuples
            let stats5 = stats.clone();
            w.insert(
                CompactString::from("getstats"),
                PyObject::native_closure("getstats", move |_: &[PyObjectRef]| {
                    let st = stats5.read();
                    let items: Vec<PyObjectRef> = st
                        .iter()
                        .map(|(name, calls, time)| {
                            PyObject::tuple(vec![
                                PyObject::str_val(CompactString::from(name.as_str())),
                                PyObject::int(*calls),
                                PyObject::float(*time),
                                PyObject::float(*time),
                                PyObject::list(vec![]),
                            ])
                        })
                        .collect();
                    Ok(PyObject::list(items))
                }),
            );
            w.insert(
                CompactString::from("run"),
                make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())),
            );
        }
        Ok(inst)
    });

    make_module(
        "cProfile",
        vec![("run", run_fn), ("Profile", profile_cls_fn)],
    )
}
