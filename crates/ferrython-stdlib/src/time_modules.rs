//! Time and datetime stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use shared::{
    days_in_month, decompose_timestamp, format_time, is_leap_year, DAY_NAMES_ABBR, DAY_NAMES_FULL,
    MONTH_NAMES_ABBR, MONTH_NAMES_FULL,
};

const TIME_MAXYEAR: i64 = i32::MAX as i64;
const TIME_MINYEAR: i64 = i32::MIN as i64 + 1900;

pub fn create_time_module() -> PyObjectRef {
    // struct_time class — callable constructor + type object
    let struct_time_cls =
        PyObject::class(CompactString::from("struct_time"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = struct_time_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("__new__"),
            make_builtin(|args: &[PyObjectRef]| {
                // struct_time((y, m, d, h, mi, s, wday, yday, isdst))
                if args.is_empty() {
                    return Err(PyException::type_error("struct_time() takes a 9-sequence"));
                }
                let seq = &args[args.len().min(2) - 1]; // skip cls if 2 args
                let items = seq.to_list()?;
                if items.len() < 9 {
                    return Err(PyException::type_error("struct_time() takes a 9-sequence"));
                }
                let get = |i: usize| items[i].as_int().unwrap_or(0);
                Ok(make_struct_time(
                    get(0),
                    get(1),
                    get(2),
                    get(3),
                    get(4),
                    get(5),
                    get(6),
                    get(7),
                ))
            }),
        );
    }

    make_module(
        "time",
        vec![
            ("time", make_builtin(time_time)),
            ("sleep", make_builtin(time_sleep)),
            ("monotonic", make_builtin(time_monotonic)),
            ("perf_counter", make_builtin(time_monotonic)),
            ("monotonic_ns", make_builtin(time_monotonic_ns)),
            ("perf_counter_ns", make_builtin(time_monotonic_ns)),
            (
                "time_ns",
                make_builtin(|_args| {
                    use std::time::SystemTime;
                    let dur = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap();
                    Ok(PyObject::int(dur.as_nanos() as i64))
                }),
            ),
            ("process_time", make_builtin(time_process_time)),
            ("process_time_ns", make_builtin(time_process_time_ns)),
            ("thread_time", make_builtin(time_thread_time)),
            ("thread_time_ns", make_builtin(time_thread_time_ns)),
            ("get_clock_info", make_builtin(time_get_clock_info)),
            ("clock_gettime", make_builtin(time_clock_gettime)),
            ("clock_gettime_ns", make_builtin(time_clock_gettime_ns)),
            ("clock_getres", make_builtin(time_clock_getres)),
            ("CLOCK_REALTIME", PyObject::int(libc::CLOCK_REALTIME as i64)),
            (
                "CLOCK_MONOTONIC",
                PyObject::int(libc::CLOCK_MONOTONIC as i64),
            ),
            (
                "CLOCK_PROCESS_CPUTIME_ID",
                PyObject::int(libc::CLOCK_PROCESS_CPUTIME_ID as i64),
            ),
            (
                "CLOCK_THREAD_CPUTIME_ID",
                PyObject::int(libc::CLOCK_THREAD_CPUTIME_ID as i64),
            ),
            ("strftime", make_builtin(time_strftime)),
            ("strptime", make_builtin(time_strptime)),
            ("localtime", make_builtin(time_localtime)),
            ("gmtime", make_builtin(time_gmtime)),
            ("mktime", make_builtin(time_mktime)),
            ("ctime", make_builtin(time_ctime)),
            ("asctime", make_builtin(time_asctime)),
            ("struct_time", struct_time_cls),
            ("_STRUCT_TM_ITEMS", PyObject::int(9)),
            ("timezone", {
                // Compute actual timezone offset (seconds west of UTC)
                #[cfg(unix)]
                {
                    use std::mem::MaybeUninit;
                    let offset = unsafe {
                        let mut t: libc::time_t = 0;
                        libc::time(&mut t);
                        let mut tm = MaybeUninit::<libc::tm>::zeroed();
                        libc::localtime_r(&t, tm.as_mut_ptr());
                        -((*tm.as_ptr()).tm_gmtoff as i64)
                    };
                    PyObject::int(offset)
                }
                #[cfg(not(unix))]
                PyObject::int(0)
            }),
            ("altzone", {
                #[cfg(unix)]
                {
                    use std::mem::MaybeUninit;
                    let offset = unsafe {
                        let mut t: libc::time_t = 0;
                        libc::time(&mut t);
                        let mut tm = MaybeUninit::<libc::tm>::zeroed();
                        libc::localtime_r(&t, tm.as_mut_ptr());
                        let base = -((*tm.as_ptr()).tm_gmtoff as i64);
                        if (*tm.as_ptr()).tm_isdst > 0 {
                            base
                        } else {
                            base - 3600
                        }
                    };
                    PyObject::int(offset)
                }
                #[cfg(not(unix))]
                PyObject::int(0)
            }),
            ("daylight", {
                #[cfg(unix)]
                {
                    let has_dst = unsafe {
                        use std::mem::MaybeUninit;
                        let winter: libc::time_t = 0;
                        let mut tm_w = MaybeUninit::<libc::tm>::zeroed();
                        libc::localtime_r(&winter, tm_w.as_mut_ptr());
                        let w_off = (*tm_w.as_ptr()).tm_gmtoff;
                        let summer: libc::time_t = 15778800;
                        let mut tm_s = MaybeUninit::<libc::tm>::zeroed();
                        libc::localtime_r(&summer, tm_s.as_mut_ptr());
                        let s_off = (*tm_s.as_ptr()).tm_gmtoff;
                        w_off != s_off
                    };
                    PyObject::int(if has_dst { 1 } else { 0 })
                }
                #[cfg(not(unix))]
                PyObject::int(0)
            }),
            ("tzname", {
                #[cfg(unix)]
                {
                    let (std_name, dst_name) = unsafe {
                        use std::ffi::CStr;
                        use std::mem::MaybeUninit;
                        // Get standard time zone name (winter)
                        let winter: libc::time_t = 0;
                        let mut tm_w = MaybeUninit::<libc::tm>::zeroed();
                        libc::localtime_r(&winter, tm_w.as_mut_ptr());
                        let std_n = if (*tm_w.as_ptr()).tm_zone.is_null() {
                            "UTC".to_string()
                        } else {
                            CStr::from_ptr((*tm_w.as_ptr()).tm_zone)
                                .to_str()
                                .unwrap_or("UTC")
                                .to_string()
                        };
                        // Get DST zone name (summer)
                        let summer: libc::time_t = 15778800;
                        let mut tm_s = MaybeUninit::<libc::tm>::zeroed();
                        libc::localtime_r(&summer, tm_s.as_mut_ptr());
                        let dst_n = if (*tm_s.as_ptr()).tm_zone.is_null() {
                            "UTC".to_string()
                        } else {
                            CStr::from_ptr((*tm_s.as_ptr()).tm_zone)
                                .to_str()
                                .unwrap_or("UTC")
                                .to_string()
                        };
                        (std_n, dst_n)
                    };
                    PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(std_name)),
                        PyObject::str_val(CompactString::from(dst_name)),
                    ])
                }
                #[cfg(not(unix))]
                PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from("UTC")),
                    PyObject::str_val(CompactString::from("UTC")),
                ])
            }),
        ],
    )
}

fn time_time(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    Ok(PyObject::float(dur.as_secs_f64()))
}

fn time_sleep(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("time.sleep", args, 1)?;
    let secs = args[0].to_float()?;
    if secs < 0.0 {
        return Err(PyException::value_error(
            "sleep length must be non-negative",
        ));
    }
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    Ok(PyObject::none())
}

fn time_monotonic(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::Instant;
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    Ok(PyObject::float(start.elapsed().as_secs_f64()))
}

fn time_monotonic_ns(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::Instant;
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    Ok(PyObject::int(start.elapsed().as_nanos() as i64))
}

fn clock_id_arg(name: &str, args: &[PyObjectRef]) -> PyResult<libc::clockid_t> {
    check_args(name, args, 1)?;
    Ok(args[0].to_int()? as libc::clockid_t)
}

fn clock_time(clock_id: libc::clockid_t) -> PyResult<libc::timespec> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_gettime(clock_id, &mut ts) };
    if rc == 0 {
        Ok(ts)
    } else {
        Err(PyException::os_error("clock_gettime failed"))
    }
}

fn timespec_seconds(ts: libc::timespec) -> f64 {
    ts.tv_sec as f64 + (ts.tv_nsec as f64 / 1_000_000_000.0)
}

fn timespec_ns(ts: libc::timespec) -> i64 {
    (ts.tv_sec as i64)
        .saturating_mul(1_000_000_000)
        .saturating_add(ts.tv_nsec as i64)
}

fn clock_seconds(clock_id: libc::clockid_t) -> PyResult<PyObjectRef> {
    Ok(PyObject::float(timespec_seconds(clock_time(clock_id)?)))
}

fn clock_ns(clock_id: libc::clockid_t) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(timespec_ns(clock_time(clock_id)?)))
}

fn time_process_time(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    clock_seconds(libc::CLOCK_PROCESS_CPUTIME_ID)
}

fn time_process_time_ns(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    clock_ns(libc::CLOCK_PROCESS_CPUTIME_ID)
}

fn time_thread_time(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    clock_seconds(libc::CLOCK_THREAD_CPUTIME_ID)
}

fn time_thread_time_ns(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    clock_ns(libc::CLOCK_THREAD_CPUTIME_ID)
}

fn clock_info_obj(
    implementation: &str,
    monotonic: bool,
    adjustable: bool,
    resolution: f64,
) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("namespace"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("implementation"),
        PyObject::str_val(CompactString::from(implementation)),
    );
    attrs.insert(
        CompactString::from("monotonic"),
        PyObject::bool_val(monotonic),
    );
    attrs.insert(
        CompactString::from("adjustable"),
        PyObject::bool_val(adjustable),
    );
    attrs.insert(
        CompactString::from("resolution"),
        PyObject::float(resolution),
    );
    PyObject::instance_with_attrs(cls, attrs)
}

fn time_get_clock_info(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("get_clock_info", args, 1)?;
    let name = args[0].py_to_string();
    match name.as_str() {
        "time" => Ok(clock_info_obj(
            "clock_gettime(CLOCK_REALTIME)",
            false,
            true,
            1e-9,
        )),
        "monotonic" => Ok(clock_info_obj(
            "clock_gettime(CLOCK_MONOTONIC)",
            true,
            false,
            1e-9,
        )),
        "perf_counter" => Ok(clock_info_obj(
            "clock_gettime(CLOCK_MONOTONIC)",
            true,
            false,
            1e-9,
        )),
        "process_time" => Ok(clock_info_obj(
            "clock_gettime(CLOCK_PROCESS_CPUTIME_ID)",
            true,
            false,
            1e-9,
        )),
        "thread_time" => Ok(clock_info_obj(
            "clock_gettime(CLOCK_THREAD_CPUTIME_ID)",
            true,
            false,
            1e-9,
        )),
        _ => Err(PyException::value_error("unknown clock")),
    }
}

fn time_clock_gettime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let clock_id = clock_id_arg("clock_gettime", args)?;
    clock_seconds(clock_id)
}

fn time_clock_gettime_ns(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let clock_id = clock_id_arg("clock_gettime_ns", args)?;
    clock_ns(clock_id)
}

fn time_clock_getres(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let clock_id = clock_id_arg("clock_getres", args)?;
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { libc::clock_getres(clock_id, &mut ts) };
    if rc == 0 {
        Ok(PyObject::float(timespec_seconds(ts)))
    } else {
        Err(PyException::os_error("clock_getres failed"))
    }
}

fn normalize_struct_components(
    y: i64,
    mon: i64,
    day: i64,
    h: i64,
    m: i64,
    s: i64,
    wday: i64,
    yday: i64,
) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    (
        y,
        if mon == 0 { 1 } else { mon },
        if day == 0 { 1 } else { day },
        h,
        m,
        s,
        ((wday + 1).rem_euclid(7) + 6).rem_euclid(7),
        if yday == 0 { 1 } else { yday },
    )
}

fn check_struct_bounds(
    y: i64,
    mon: i64,
    day: i64,
    h: i64,
    m: i64,
    s: i64,
    wday: i64,
    yday: i64,
) -> PyResult<()> {
    if !(TIME_MINYEAR..=TIME_MAXYEAR).contains(&y) {
        return Err(PyException::overflow_error("year out of range"));
    }
    if !matches!(mon, 0..=12) {
        return Err(PyException::value_error("month out of range"));
    }
    if !matches!(day, 0..=31) {
        return Err(PyException::value_error("day of month out of range"));
    }
    if !matches!(h, 0..=23) {
        return Err(PyException::value_error("hour out of range"));
    }
    if !matches!(m, 0..=59) {
        return Err(PyException::value_error("minute out of range"));
    }
    if !matches!(s, 0..=61) {
        return Err(PyException::value_error("seconds out of range"));
    }
    if wday < -1 {
        return Err(PyException::value_error("day of week out of range"));
    }
    if !matches!(yday, 0..=366) {
        return Err(PyException::value_error("day of year out of range"));
    }
    Ok(())
}

fn make_struct_time(
    y: i64,
    mon: i64,
    day: i64,
    h: i64,
    m: i64,
    s: i64,
    wday: i64,
    yday: i64,
) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("struct_time"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref data) = inst.payload {
        let mut attrs = data.attrs.write();
        attrs.insert(CompactString::from("tm_year"), PyObject::int(y));
        attrs.insert(CompactString::from("tm_mon"), PyObject::int(mon));
        attrs.insert(CompactString::from("tm_mday"), PyObject::int(day));
        attrs.insert(CompactString::from("tm_hour"), PyObject::int(h));
        attrs.insert(CompactString::from("tm_min"), PyObject::int(m));
        attrs.insert(CompactString::from("tm_sec"), PyObject::int(s));
        attrs.insert(CompactString::from("tm_wday"), PyObject::int(wday));
        attrs.insert(CompactString::from("tm_yday"), PyObject::int(yday));
        attrs.insert(CompactString::from("tm_isdst"), PyObject::int(-1));
        // Also support indexing as tuple
        let items = vec![
            PyObject::int(y),
            PyObject::int(mon),
            PyObject::int(day),
            PyObject::int(h),
            PyObject::int(m),
            PyObject::int(s),
            PyObject::int(wday),
            PyObject::int(yday),
            PyObject::int(-1),
        ];
        attrs.insert(
            CompactString::from("__tuple__"),
            PyObject::tuple(items.clone()),
        );
        // __repr__
        let repr = format!(
            "time.struct_time(tm_year={}, tm_mon={}, tm_mday={}, tm_hour={}, tm_min={}, tm_sec={}, tm_wday={}, tm_yday={}, tm_isdst=-1)",
            y, mon, day, h, m, s, wday, yday
        );
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::str_val(CompactString::from(repr)),
        );
        // __len__ and __getitem__ for tuple-like access
        attrs.insert(
            CompactString::from("__len__"),
            make_builtin(|_| Ok(PyObject::int(9))),
        );
        let items_ref = PyObject::tuple(items);
        attrs.insert(
            CompactString::from("__getitem__"),
            PyObject::native_closure("__getitem__", move |args: &[PyObjectRef]| {
                let key = if args.len() > 1 { &args[1] } else { &args[0] };
                if let PyObjectPayload::Tuple(t) = &items_ref.payload {
                    if let PyObjectPayload::Slice(sd) = &key.payload {
                        let len = t.len() as i64;
                        let is_none = |value: &Option<PyObjectRef>| {
                            value
                                .as_ref()
                                .map(|v| matches!(&v.payload, PyObjectPayload::None))
                                .unwrap_or(true)
                        };
                        let to_i64 = |value: &Option<PyObjectRef>, default: i64| {
                            value
                                .as_ref()
                                .and_then(|v| v.to_int().ok())
                                .unwrap_or(default)
                        };
                        let step = to_i64(&sd.step, 1);
                        if step == 0 {
                            return Err(PyException::value_error("slice step cannot be zero"));
                        }
                        let mut start = if is_none(&sd.start) {
                            if step > 0 {
                                0
                            } else {
                                len - 1
                            }
                        } else {
                            to_i64(&sd.start, 0)
                        };
                        let mut stop = if is_none(&sd.stop) {
                            if step > 0 {
                                len
                            } else {
                                -1
                            }
                        } else {
                            to_i64(&sd.stop, len)
                        };
                        if start < 0 {
                            start += len;
                        }
                        if stop < 0 && !is_none(&sd.stop) {
                            stop += len;
                        }
                        if step > 0 {
                            start = start.max(0).min(len);
                            stop = stop.max(0).min(len);
                        } else {
                            start = start.max(-1).min(len - 1);
                            stop = stop.max(-1).min(len - 1);
                        }
                        let mut result = Vec::new();
                        let mut i = start;
                        if step > 0 {
                            while i < stop {
                                result.push(t[i as usize].clone());
                                i += step;
                            }
                        } else {
                            while i > stop {
                                result.push(t[i as usize].clone());
                                i += step;
                            }
                        }
                        return Ok(PyObject::tuple(result));
                    }
                    if let Some(idx) = key.as_int() {
                        let i = if idx < 0 {
                            (t.len() as i64 + idx) as usize
                        } else {
                            idx as usize
                        };
                        return Ok(t.get(i).cloned().unwrap_or_else(PyObject::none));
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }
    inst
}

fn get_epoch_secs(args: &[PyObjectRef]) -> u64 {
    if let Some(secs_arg) = args.first() {
        if matches!(&secs_arg.payload, PyObjectPayload::None) {
            return current_epoch_secs();
        }
        if let Ok(f) = secs_arg.to_float() {
            if !f.is_finite() || f < 0.0 || f > u64::MAX as f64 {
                return 0;
            }
            return f as u64;
        }
        if let Some(i) = secs_arg.as_int() {
            if i < 0 {
                return 0;
            }
            return i as u64;
        }
    }
    current_epoch_secs()
}

fn checked_epoch_secs(args: &[PyObjectRef]) -> PyResult<Option<i64>> {
    if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
        return Ok(None);
    }
    let value = args[0].to_float()?;
    if !value.is_finite() || value < i64::MIN as f64 || value > i64::MAX as f64 {
        return Err(PyException::overflow_error("timestamp out of range"));
    }
    Ok(Some(value as i64))
}

fn current_epoch_secs() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn decompose_signed_timestamp(epoch_secs: i64) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    let days = epoch_secs.div_euclid(86400);
    let rem = epoch_secs.rem_euclid(86400);
    let (y, mon, day) = shared::days_to_ymd(days + 719_468);
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let wday = (days + 3).rem_euclid(7);
    let md = days_in_month(y);
    let yday = (0..(mon - 1) as usize).map(|i| md[i]).sum::<i64>() + day;
    (y, mon, day, h, m, s, wday, yday)
}

fn ensure_text_arg(func: &str, obj: &PyObjectRef) -> PyResult<String> {
    match &obj.payload {
        PyObjectPayload::Str(_) => Ok(obj.py_to_string()),
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => Err(PyException::type_error(
            format!("{}() argument must be str, not bytes", func),
        )),
        _ => Ok(obj.py_to_string()),
    }
}

fn time_strftime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("strftime requires a format string"));
    }
    let fmt = ensure_text_arg("strftime", &args[0])?;
    if fmt.contains('\0') {
        return Err(PyException::value_error("embedded null character"));
    }
    // Use struct_time arg if provided, otherwise current time
    let (y, mon, day, h, m, s, wday, yday) = if args.len() >= 2 {
        let raw = extract_struct_time(&args[1])?;
        check_struct_bounds(raw.0, raw.1, raw.2, raw.3, raw.4, raw.5, raw.6, raw.7)?;
        normalize_struct_components(raw.0, raw.1, raw.2, raw.3, raw.4, raw.5, raw.6, raw.7)
    } else {
        let secs = get_epoch_secs(&[]);
        decompose_timestamp(secs)
    };
    let result = format_time(&fmt, y, mon, day, h, m, s, wday, yday);
    Ok(PyObject::str_val(CompactString::from(result)))
}

/// Extract (y, mon, day, h, m, s, wday, yday) from a struct_time or tuple
fn extract_struct_time(obj: &PyObjectRef) -> PyResult<(i64, i64, i64, i64, i64, i64, i64, i64)> {
    match &obj.payload {
        PyObjectPayload::Tuple(t) if t.len() == 9 => Ok((
            t[0].as_int().unwrap_or(1970),
            t[1].as_int().unwrap_or(1),
            t[2].as_int().unwrap_or(1),
            t[3].as_int().unwrap_or(0),
            t[4].as_int().unwrap_or(0),
            t[5].as_int().unwrap_or(0),
            t[6].as_int().unwrap_or(0),
            t[7].as_int().unwrap_or(1),
        )),
        PyObjectPayload::Tuple(t) => Err(PyException::type_error(format!(
            "time tuple must have exactly 9 elements, not {}",
            t.len()
        ))),
        PyObjectPayload::Instance(data) => {
            let attrs = data.attrs.read();
            if let Some(tup) = attrs.get("__tuple__") {
                if let PyObjectPayload::Tuple(t) = &tup.payload {
                    if t.len() >= 9 {
                        return Ok((
                            t[0].as_int().unwrap_or(1970),
                            t[1].as_int().unwrap_or(1),
                            t[2].as_int().unwrap_or(1),
                            t[3].as_int().unwrap_or(0),
                            t[4].as_int().unwrap_or(0),
                            t[5].as_int().unwrap_or(0),
                            t[6].as_int().unwrap_or(0),
                            t[7].as_int().unwrap_or(1),
                        ));
                    }
                }
            }
            // Try named attrs
            let y = attrs
                .get("tm_year")
                .and_then(|v| v.as_int())
                .unwrap_or(1970);
            let mon = attrs.get("tm_mon").and_then(|v| v.as_int()).unwrap_or(1);
            let day = attrs.get("tm_mday").and_then(|v| v.as_int()).unwrap_or(1);
            let h = attrs.get("tm_hour").and_then(|v| v.as_int()).unwrap_or(0);
            let m = attrs.get("tm_min").and_then(|v| v.as_int()).unwrap_or(0);
            let s = attrs.get("tm_sec").and_then(|v| v.as_int()).unwrap_or(0);
            let wday = attrs.get("tm_wday").and_then(|v| v.as_int()).unwrap_or(0);
            let yday = attrs.get("tm_yday").and_then(|v| v.as_int()).unwrap_or(1);
            Ok((y, mon, day, h, m, s, wday, yday))
        }
        _ => Err(PyException::type_error("expected struct_time or 9-tuple")),
    }
}

fn time_strptime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "strptime() takes exactly 2 arguments",
        ));
    }
    let date_str = ensure_text_arg("strptime", &args[0])?;
    let fmt = ensure_text_arg("strptime", &args[1])?;
    if fmt.ends_with('%') && !fmt.ends_with("%%") {
        return Err(strptime_value_error());
    }

    let mut y: i64 = 1900;
    let mut mon: i64 = 1;
    let mut day: i64 = 1;
    let mut h: i64 = 0;
    let mut m: i64 = 0;
    let mut s: i64 = 0;

    // Parse format string and extract values from date_str
    let mut fi = fmt.chars().peekable();
    let mut di = date_str.chars().peekable();
    let value_error = strptime_value_error;

    while let Some(fc) = fi.next() {
        if fc == '%' {
            match fi.next() {
                Some('Y') => {
                    y = parse_digits(&mut di, 4)?;
                }
                Some('y') => {
                    let v = parse_digits(&mut di, 2)?;
                    y = if v >= 69 { 1900 + v } else { 2000 + v };
                }
                Some('D') => {
                    let mo = parse_digits(&mut di, 2)?;
                    if di.peek() == Some(&'/') {
                        di.next();
                    }
                    let da = parse_digits(&mut di, 2)?;
                    if di.peek() == Some(&'/') {
                        di.next();
                    }
                    let yy = parse_digits(&mut di, 2)?;
                    mon = mo;
                    day = da;
                    y = if yy >= 69 { 1900 + yy } else { 2000 + yy };
                }
                Some('x') => {
                    let mo = parse_digits(&mut di, 2)?;
                    if di.peek() == Some(&'/') {
                        di.next();
                    }
                    let da = parse_digits(&mut di, 2)?;
                    if di.peek() == Some(&'/') {
                        di.next();
                    }
                    let yy = parse_digits(&mut di, 2)?;
                    mon = mo;
                    day = da;
                    y = if yy >= 69 { 1900 + yy } else { 2000 + yy };
                }
                Some('m') => {
                    mon = parse_digits(&mut di, 2)?;
                }
                Some('d') => {
                    day = parse_digits(&mut di, 2)?;
                }
                Some('H') => {
                    h = parse_digits(&mut di, 2)?;
                }
                Some('I') => {
                    h = parse_digits(&mut di, 2)?;
                }
                Some('c') => {
                    let _ = parse_name(&mut di, &DAY_NAMES_ABBR, &DAY_NAMES_FULL)?;
                    consume_spaces(&mut di);
                    mon = parse_name(&mut di, &MONTH_NAMES_ABBR, &MONTH_NAMES_FULL)?;
                    consume_spaces(&mut di);
                    day = parse_digits(&mut di, 2)?;
                    consume_spaces(&mut di);
                    h = parse_digits(&mut di, 2)?;
                    expect_char(&mut di, ':')?;
                    m = parse_digits(&mut di, 2)?;
                    expect_char(&mut di, ':')?;
                    s = parse_digits(&mut di, 2)?;
                    consume_spaces(&mut di);
                    y = parse_digits(&mut di, 4)?;
                }
                Some('M') => {
                    m = parse_digits(&mut di, 2)?;
                }
                Some('S') => {
                    s = parse_digits(&mut di, 2)?;
                }
                Some('p') => {
                    // AM/PM
                    let a: String = (&mut di).take(2).collect();
                    if a.eq_ignore_ascii_case("PM") && h < 12 {
                        h += 12;
                    } else if a.eq_ignore_ascii_case("AM") && h == 12 {
                        h = 0;
                    }
                }
                Some('j') => {
                    let _ = parse_digits(&mut di, 3)?;
                } // yday - skip
                Some('U') | Some('W') | Some('w') => {
                    let _ = parse_digits(&mut di, 2)?;
                }
                Some('X') => {
                    h = parse_digits(&mut di, 2)?;
                    expect_char(&mut di, ':')?;
                    m = parse_digits(&mut di, 2)?;
                    expect_char(&mut di, ':')?;
                    s = parse_digits(&mut di, 2)?;
                }
                Some('Z') => {
                    let name = parse_alpha_word(&mut di);
                    if name.is_empty() {
                        return Err(value_error());
                    }
                }
                Some('b') | Some('B') => {
                    mon = parse_name(&mut di, &MONTH_NAMES_ABBR, &MONTH_NAMES_FULL)?;
                }
                Some('a') | Some('A') => {
                    let _ = parse_name(&mut di, &DAY_NAMES_ABBR, &DAY_NAMES_FULL)?;
                }
                Some('%') => {
                    if di.next() != Some('%') {
                        return Err(value_error());
                    }
                }
                Some(other) => {
                    return Err(PyException::value_error(format!(
                        "bad directive in format: %{}",
                        other
                    )));
                }
                None => return Err(PyException::value_error("stray % in format")),
            }
        } else {
            if di.next() != Some(fc) {
                return Err(value_error());
            }
        }
    }
    if di.next().is_some() {
        return Err(value_error());
    }
    if !matches!(mon, 1..=12)
        || !matches!(day, 1..=31)
        || !matches!(h, 0..=23)
        || !matches!(m, 0..=59)
        || !matches!(s, 0..=61)
    {
        return Err(value_error());
    }

    // Compute wday and yday
    let md = days_in_month(y);
    let yday = {
        let mut yd = day;
        for i in 0..(mon - 1) as usize {
            if i < 12 {
                yd += md[i];
            }
        }
        yd
    };
    // Compute day of week using Tomohiko Sakamoto's algorithm
    let wday = {
        let t = [0i64, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let yy = if mon < 3 { y - 1 } else { y };
        let w = (yy + yy / 4 - yy / 100 + yy / 400 + t[(mon - 1) as usize] + day) % 7;
        (w + 6) % 7 // convert Sunday=0 to Monday=0
    };

    Ok(make_struct_time(y, mon, day, h, m, s, wday, yday))
}

fn parse_digits(chars: &mut std::iter::Peekable<std::str::Chars>, max: usize) -> PyResult<i64> {
    let mut s = String::new();
    consume_spaces(chars);
    for _ in 0..max {
        match chars.peek() {
            Some(c) if c.is_ascii_digit() => s.push(chars.next().unwrap()),
            _ => break,
        }
    }
    if s.is_empty() {
        return Err(strptime_value_error());
    }
    s.parse::<i64>().map_err(|_| strptime_value_error())
}

fn consume_spaces(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while chars.peek().map_or(false, |c| *c == ' ') {
        chars.next();
    }
}

fn expect_char(chars: &mut std::iter::Peekable<std::str::Chars>, expected: char) -> PyResult<()> {
    if chars.next() == Some(expected) {
        Ok(())
    } else {
        Err(strptime_value_error())
    }
}

fn parse_name(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    abbrs: &[&str],
    full_names: &[&str],
) -> PyResult<i64> {
    let name = parse_alpha_word(chars);
    if name.is_empty() {
        return Err(strptime_value_error());
    }
    let lower = name.to_lowercase();
    for (i, abbr) in abbrs.iter().enumerate() {
        if lower == abbr.to_lowercase()
            || full_names
                .get(i)
                .is_some_and(|full| lower == full.to_lowercase())
        {
            return Ok(i as i64 + 1);
        }
    }
    Err(strptime_value_error())
}

fn parse_alpha_word(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut name = String::new();
    while chars.peek().is_some_and(|c| c.is_alphabetic()) {
        name.push(chars.next().unwrap());
    }
    name
}

fn strptime_value_error() -> PyException {
    let message = CompactString::from("time data does not match format");
    let original = PyObject::exception_instance(ExceptionKind::ValueError, message.clone());
    if let PyObjectPayload::ExceptionInstance(ei) = &original.payload {
        ei.ensure_attrs().write().insert(
            CompactString::from("__suppress_context__"),
            PyObject::bool_val(true),
        );
    }
    PyException::with_original(ExceptionKind::ValueError, message, original)
}

fn time_localtime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (y, mon, day, h, m, s, wday, yday) = match checked_epoch_secs(args)? {
        Some(secs) => decompose_signed_timestamp(secs),
        None => decompose_timestamp(current_epoch_secs()),
    };
    Ok(make_struct_time(y, mon, day, h, m, s, wday, yday))
}

fn time_gmtime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (y, mon, day, h, m, s, wday, yday) = match checked_epoch_secs(args)? {
        Some(secs) => decompose_signed_timestamp(secs),
        None => decompose_timestamp(current_epoch_secs()),
    };
    Ok(make_struct_time(y, mon, day, h, m, s, wday, yday))
}

fn time_mktime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "mktime() requires a struct_time argument",
        ));
    }
    let raw = extract_struct_time(&args[0])?;
    check_struct_bounds(raw.0, raw.1, raw.2, raw.3, raw.4, raw.5, raw.6, raw.7)?;
    let (y, mon, day, h, m, s, _wday, _yday) =
        normalize_struct_components(raw.0, raw.1, raw.2, raw.3, raw.4, raw.5, raw.6, raw.7);
    // Convert to epoch seconds
    let mut total_days: i64 = 0;
    for yr in 1970..y {
        total_days += if is_leap_year(yr) { 366 } else { 365 };
    }
    if y < 1970 {
        for yr in y..1970 {
            total_days -= if is_leap_year(yr) { 366 } else { 365 };
        }
    }
    let md = days_in_month(y);
    for i in 0..(mon - 1) as usize {
        if i < 12 {
            total_days += md[i];
        }
    }
    total_days += day - 1;
    let epoch = total_days * 86400 + h * 3600 + m * 60 + s;
    Ok(PyObject::float(epoch as f64))
}

fn time_ctime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (y, mon, day, h, m, s, wday, _yday) = match checked_epoch_secs(args)? {
        Some(secs) => decompose_signed_timestamp(secs),
        None => decompose_timestamp(current_epoch_secs()),
    };
    let result = format!(
        "{} {} {:2} {:02}:{:02}:{:02} {}",
        DAY_NAMES_ABBR[wday as usize % 7],
        MONTH_NAMES_ABBR[(mon - 1) as usize % 12],
        day,
        h,
        m,
        s,
        y
    );
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn time_asctime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() > 1 {
        return Err(PyException::type_error(
            "asctime() takes at most 1 argument",
        ));
    }
    let (y, mon, day, h, m, s, wday, _yday) = if args.is_empty() {
        let secs = get_epoch_secs(&[]);
        decompose_timestamp(secs)
    } else {
        let raw = extract_struct_time(&args[0])?;
        check_struct_bounds(raw.0, raw.1, raw.2, raw.3, raw.4, raw.5, raw.6, raw.7)?;
        normalize_struct_components(raw.0, raw.1, raw.2, raw.3, raw.4, raw.5, raw.6, raw.7)
    };
    let result = format!(
        "{} {} {:2} {:02}:{:02}:{:02} {}",
        DAY_NAMES_ABBR[wday as usize % 7],
        MONTH_NAMES_ABBR[(mon - 1) as usize % 12],
        day,
        h,
        m,
        s,
        y
    );
    Ok(PyObject::str_val(CompactString::from(result)))
}

mod datetime;
mod shared;
mod strptime;
mod zoneinfo;

pub use datetime::create_datetime_module;
pub use strptime::create_strptime_module;
pub use zoneinfo::create_zoneinfo_module;
