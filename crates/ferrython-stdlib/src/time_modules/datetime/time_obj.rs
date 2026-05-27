use super::*;

pub(super) fn make_time_instance(
    hour: i64,
    minute: i64,
    second: i64,
    microsecond: i64,
) -> PyResult<PyObjectRef> {
    let class = PyObject::class(CompactString::from("time"), vec![], IndexMap::new());
    let class_flags = InstanceData::compute_flags(&class);
    let inst = PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
        Box::new(InstanceData {
            class,
            attrs: new_shared_fx(),
            is_special: true,
            dict_storage: None,
            class_flags,
            finalizer_state: std::cell::Cell::new(0),
        }),
    )));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__datetime__"),
            PyObject::bool_val(true),
        );
        w.insert(
            CompactString::from("__time_only__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("hour"), PyObject::int(hour));
        w.insert(CompactString::from("minute"), PyObject::int(minute));
        w.insert(CompactString::from("second"), PyObject::int(second));
        w.insert(
            CompactString::from("microsecond"),
            PyObject::int(microsecond),
        );
        w.insert(CompactString::from("tzinfo"), PyObject::none());

        // isoformat() -> str
        let (h, mi, s, us) = (hour, minute, second, microsecond);
        w.insert(
            CompactString::from("isoformat"),
            PyObject::native_closure("time.isoformat", move |_: &[PyObjectRef]| {
                let base = format!("{:02}:{:02}:{:02}", h, mi, s);
                if us != 0 {
                    Ok(PyObject::str_val(CompactString::from(format!(
                        "{}.{:06}",
                        base, us
                    ))))
                } else {
                    Ok(PyObject::str_val(CompactString::from(base)))
                }
            }),
        );

        // strftime(format) -> str
        w.insert(
            CompactString::from("strftime"),
            PyObject::native_closure("time.strftime", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("strftime requires format string"));
                }
                let fmt = args[0].py_to_string();
                let result = format_time_us(&fmt, 1900, 1, 1, h, mi, s, us, 0, 1);
                Ok(PyObject::str_val(CompactString::from(result)))
            }),
        );

        // replace(**kwargs)
        let (rh, rmi, rs, rus) = (hour, minute, second, microsecond);
        w.insert(
            CompactString::from("replace"),
            PyObject::native_closure("time.replace", move |args: &[PyObjectRef]| {
                let mut nh = rh;
                let mut nmi = rmi;
                let mut ns = rs;
                let mut nus = rus;
                if let Some(last) = args.last() {
                    if let PyObjectPayload::Dict(kw) = &last.payload {
                        let r = kw.read();
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("hour"))) {
                            nh = v.as_int().unwrap_or(nh);
                        }
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("minute")))
                        {
                            nmi = v.as_int().unwrap_or(nmi);
                        }
                        if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("second")))
                        {
                            ns = v.as_int().unwrap_or(ns);
                        }
                        if let Some(v) =
                            r.get(&HashableKey::str_key(CompactString::from("microsecond")))
                        {
                            nus = v.as_int().unwrap_or(nus);
                        }
                    }
                }
                if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
                    if !args.is_empty() {
                        nh = args[0].as_int().unwrap_or(nh);
                    }
                    if args.len() > 1 {
                        nmi = args[1].as_int().unwrap_or(nmi);
                    }
                    if args.len() > 2 {
                        ns = args[2].as_int().unwrap_or(ns);
                    }
                    if args.len() > 3 {
                        nus = args[3].as_int().unwrap_or(nus);
                    }
                }
                make_time_instance(nh, nmi, ns, nus)
            }),
        );

        // __str__() / __repr__()
        let iso_str = if microsecond != 0 {
            format!("{:02}:{:02}:{:02}.{:06}", hour, minute, second, microsecond)
        } else {
            format!("{:02}:{:02}:{:02}", hour, minute, second)
        };
        let iso_clone = iso_str.clone();
        w.insert(
            CompactString::from("__str__"),
            PyObject::native_closure("time.__str__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(&iso_str)))
            }),
        );
        w.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("time.__repr__", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "datetime.time({})",
                    iso_clone
                ))))
            }),
        );

        // __eq__, __lt__, __le__, __gt__, __ge__
        let time_key = hour * 3600_000_000 + minute * 60_000_000 + second * 1_000_000 + microsecond;
        w.insert(
            CompactString::from("__eq__"),
            PyObject::native_closure("time.__eq__", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::bool_val(false));
                }
                let other = &args[0];
                let ok = other
                    .get_attr("hour")
                    .and_then(|h| h.as_int())
                    .and_then(|oh| {
                        let om = other
                            .get_attr("minute")
                            .and_then(|v| v.as_int())
                            .unwrap_or(-1);
                        let os = other
                            .get_attr("second")
                            .and_then(|v| v.as_int())
                            .unwrap_or(-1);
                        let ou = other
                            .get_attr("microsecond")
                            .and_then(|v| v.as_int())
                            .unwrap_or(-1);
                        Some(oh * 3600_000_000 + om * 60_000_000 + os * 1_000_000 + ou)
                    })
                    .unwrap_or(-1);
                Ok(PyObject::bool_val(time_key == ok))
            }),
        );

        // __hash__
        w.insert(
            CompactString::from("__hash__"),
            PyObject::native_closure("time.__hash__", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(time_key))
            }),
        );
    }
    Ok(inst)
}

pub(super) fn datetime_time_obj(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let hour = if !args.is_empty() {
        args[0].to_int()?
    } else {
        0
    };
    let minute = if args.len() > 1 { args[1].to_int()? } else { 0 };
    let second = if args.len() > 2 { args[2].to_int()? } else { 0 };
    let microsecond = if args.len() > 3 { args[3].to_int()? } else { 0 };
    make_time_instance(hour, minute, second, microsecond)
}
