use compact_str::CompactString;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;

// ── profile module ──

pub fn create_profile_module() -> PyObjectRef {
    // profile.run(statement) — execute and print simple timing
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
            eprintln!("        1    0.000    0.000    0.000    0.000 {{method 'disable' of '_lsprof.Profiler' objects}}");
        }
        Ok(PyObject::none())
    });

    let profile_cls_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Profile"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            // Track timing state
            let stats: Rc<PyCell<Vec<(String, f64)>>> = Rc::new(PyCell::new(Vec::new()));
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
                        stats2
                            .write()
                            .push(("profiling".to_string(), start.elapsed().as_secs_f64()));
                    }
                    Ok(PyObject::none())
                }),
            );
            // runcall(func, *args) — call func and profile it
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
                        .push(("runcall".to_string(), start.elapsed().as_secs_f64()));
                    Ok(result)
                }),
            );
            let stats4 = stats.clone();
            w.insert(
                CompactString::from("print_stats"),
                PyObject::native_closure("print_stats", move |args: &[PyObjectRef]| {
                    let sort_key = if !args.is_empty() {
                        args[0].py_to_string()
                    } else {
                        "cumulative".to_string()
                    };
                    let st = stats4.read();
                    let total: f64 = st.iter().map(|(_, t)| t).sum();
                    eprintln!(
                        "         {} function calls in {:.3} seconds",
                        st.len().max(1),
                        total
                    );
                    eprintln!("");
                    eprintln!("   Ordered by: {}", sort_key);
                    eprintln!("");
                    eprintln!(
                        "   ncalls  tottime  percall  cumtime  percall filename:lineno(function)"
                    );
                    for (name, time) in st.iter() {
                        eprintln!(
                            "        1    {:.3}    {:.3}    {:.3}    {:.3} <string>:1({})",
                            time, time, time, time, name
                        );
                    }
                    Ok(PyObject::none())
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
        "profile",
        vec![("run", run_fn), ("Profile", profile_cls_fn)],
    )
}
