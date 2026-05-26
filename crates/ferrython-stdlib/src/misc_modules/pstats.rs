use compact_str::CompactString;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;

// ── pstats module ──

pub fn create_pstats_module() -> PyObjectRef {
    make_module(
        "pstats",
        vec![
            (
                "Stats",
                make_builtin(|args: &[PyObjectRef]| {
                    let cls =
                        PyObject::class(CompactString::from("Stats"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref data) = inst.payload {
                        let mut attrs = data.attrs.write();
                        if !args.is_empty() {
                            attrs.insert(CompactString::from("_data"), args[0].clone());
                        }
                        // Stats methods return self for chaining
                        let self_ref = inst.clone();
                        let s = self_ref.clone();
                        attrs.insert(
                            CompactString::from("sort_stats"),
                            PyObject::native_closure(
                                "Stats.sort_stats",
                                move |args: &[PyObjectRef]| {
                                    // Store sort key for reference
                                    if let PyObjectPayload::Instance(ref d) = s.payload {
                                        if !args.is_empty() {
                                            d.attrs.write().insert(
                                                CompactString::from("_sort_key"),
                                                args[0].clone(),
                                            );
                                        }
                                    }
                                    Ok(s.clone())
                                },
                            ),
                        );
                        let s = self_ref.clone();
                        attrs.insert(
                            CompactString::from("print_stats"),
                            PyObject::native_closure(
                                "Stats.print_stats",
                                move |_: &[PyObjectRef]| Ok(s.clone()),
                            ),
                        );
                        let s = self_ref.clone();
                        attrs.insert(
                            CompactString::from("print_callers"),
                            PyObject::native_closure(
                                "Stats.print_callers",
                                move |_: &[PyObjectRef]| Ok(s.clone()),
                            ),
                        );
                        let s = self_ref.clone();
                        attrs.insert(
                            CompactString::from("print_callees"),
                            PyObject::native_closure(
                                "Stats.print_callees",
                                move |_: &[PyObjectRef]| Ok(s.clone()),
                            ),
                        );
                        let s = self_ref.clone();
                        attrs.insert(
                            CompactString::from("strip_dirs"),
                            PyObject::native_closure(
                                "Stats.strip_dirs",
                                move |_: &[PyObjectRef]| Ok(s.clone()),
                            ),
                        );
                    }
                    Ok(inst)
                }),
            ),
            ("SortKey", {
                let cls = PyObject::class(CompactString::from("SortKey"), vec![], IndexMap::new());
                let inst = PyObject::instance(cls);
                if let PyObjectPayload::Instance(ref data) = inst.payload {
                    let mut attrs = data.attrs.write();
                    attrs.insert(
                        CompactString::from("CALLS"),
                        PyObject::str_val(CompactString::from("calls")),
                    );
                    attrs.insert(
                        CompactString::from("CUMULATIVE"),
                        PyObject::str_val(CompactString::from("cumulative")),
                    );
                    attrs.insert(
                        CompactString::from("TIME"),
                        PyObject::str_val(CompactString::from("time")),
                    );
                    attrs.insert(
                        CompactString::from("NAME"),
                        PyObject::str_val(CompactString::from("name")),
                    );
                }
                inst
            }),
        ],
    )
}
