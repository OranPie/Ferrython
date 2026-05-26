//! `_strptime` compatibility module implementation.

use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

pub fn create_strptime_module() -> PyObjectRef {
    make_module(
        "_strptime",
        vec![
            (
                "_strptime_datetime",
                make_builtin(|args| {
                    // _strptime_datetime(cls, data_string, format) → datetime
                    if args.len() < 3 {
                        return Err(PyException::type_error(
                            "_strptime_datetime requires 3 arguments",
                        ));
                    }
                    let _cls = &args[0];
                    let data = args[1].py_to_string();
                    let fmt = args[2].py_to_string();
                    // Minimal implementation: parse common patterns
                    let mut year = 1900i64;
                    let mut month = 1i64;
                    let mut day = 1i64;
                    let mut hour = 0i64;
                    let mut minute = 0i64;
                    let mut second = 0i64;

                    // Try YYYY-MM-DD HH:MM:SS
                    if fmt.contains("%Y") && fmt.contains("%m") && fmt.contains("%d") {
                        let parts: Vec<&str> = data.split(|c: char| !c.is_ascii_digit()).collect();
                        let nums: Vec<i64> = parts
                            .iter()
                            .filter(|s| !s.is_empty())
                            .filter_map(|s| s.parse::<i64>().ok())
                            .collect();
                        if nums.len() >= 3 {
                            year = nums[0];
                            month = nums[1];
                            day = nums[2];
                        }
                        if nums.len() >= 6 {
                            hour = nums[3];
                            minute = nums[4];
                            second = nums[5];
                        }
                    }

                    // Return a datetime-like tuple (year, month, day, hour, minute, second)
                    Ok(PyObject::tuple(vec![
                        PyObject::int(year),
                        PyObject::int(month),
                        PyObject::int(day),
                        PyObject::int(hour),
                        PyObject::int(minute),
                        PyObject::int(second),
                    ]))
                }),
            ),
            (
                "_strptime_time",
                make_builtin(|_args| {
                    Ok(PyObject::tuple(vec![
                        PyObject::int(0),
                        PyObject::int(0),
                        PyObject::int(0),
                        PyObject::int(0),
                        PyObject::int(0),
                        PyObject::int(0),
                        PyObject::int(0),
                        PyObject::int(0),
                        PyObject::int(-1),
                    ]))
                }),
            ),
            (
                "_getlang",
                make_builtin(|_args| {
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from("en_US")),
                        PyObject::str_val(CompactString::from("UTF-8")),
                    ]))
                }),
            ),
            (
                "_TimeRE",
                make_builtin(|_args| Ok(PyObject::dict_from_pairs(vec![]))),
            ),
            ("LocaleTime", make_builtin(|_args| Ok(PyObject::none()))),
        ],
    )
}
