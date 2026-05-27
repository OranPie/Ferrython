//! VM-aware regex helpers used by native stdlib call routing.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

impl VirtualMachine {
    /// Handle re.sub/re.subn when the replacement is a callable.
    pub(crate) fn re_sub_with_callable(
        &mut self,
        args: &[PyObjectRef],
        return_count: bool,
    ) -> PyResult<PyObjectRef> {
        let pattern = args[0].py_to_string();
        let repl_fn = args[1].clone();
        let text = args[2].py_to_string();
        let max_count = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::Dict(_)) {
            args[3].to_int().unwrap_or(0) as usize
        } else {
            0
        };
        let mut flags = if args.len() > 4 && !matches!(&args[4].payload, PyObjectPayload::Dict(_)) {
            args[4].to_int().unwrap_or(0)
        } else {
            0
        };
        let mut max_count_kw = max_count;
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(map) = &last.payload {
                let map_r = map.read();
                for (k, v) in map_r.iter() {
                    if let HashableKey::Str(s) = k {
                        match s.as_str() {
                            "count" => max_count_kw = v.to_int().unwrap_or(0) as usize,
                            "flags" => flags = v.to_int().unwrap_or(0),
                            _ => {}
                        }
                    }
                }
            }
        }
        let max_count = if max_count_kw > 0 {
            max_count_kw
        } else {
            max_count
        };

        let re = if flags & 2 != 0 {
            regex::RegexBuilder::new(&pattern)
                .case_insensitive(true)
                .build()
        } else {
            regex::Regex::new(&pattern)
        }
        .map_err(|e| PyException::runtime_error(format!("regex error: {}", e)))?;

        let mut result = String::new();
        let mut last_end = 0;
        let mut count = 0;
        for caps in re.captures_iter(&text) {
            if max_count > 0 && count >= max_count {
                break;
            }
            let whole = caps.get(0).unwrap();
            result.push_str(&text[last_end..whole.start()]);

            let mut group_strs: Vec<PyObjectRef> = Vec::new();
            for i in 1..caps.len() {
                if let Some(g) = caps.get(i) {
                    group_strs.push(PyObject::str_val(CompactString::from(g.as_str())));
                } else {
                    group_strs.push(PyObject::none());
                }
            }
            let mut groupindex_map = IndexMap::new();
            for (i, name_opt) in re.capture_names().enumerate() {
                if let Some(name) = name_opt {
                    groupindex_map.insert(
                        HashableKey::str_key(CompactString::from(name)),
                        PyObject::int(i as i64),
                    );
                }
            }
            let groups_tuple = PyObject::tuple(group_strs);
            let groupindex = PyObject::dict(groupindex_map);

            let mut match_attrs = IndexMap::new();
            match_attrs.insert(
                CompactString::from("_match"),
                PyObject::str_val(CompactString::from(whole.as_str())),
            );
            match_attrs.insert(CompactString::from("_groups"), groups_tuple.clone());
            match_attrs.insert(CompactString::from("_groupindex"), groupindex);
            match_attrs.insert(
                CompactString::from("_start"),
                PyObject::int(whole.start() as i64),
            );
            match_attrs.insert(
                CompactString::from("_end"),
                PyObject::int(whole.end() as i64),
            );
            match_attrs.insert(
                CompactString::from("_text"),
                PyObject::str_val(CompactString::from(text.clone())),
            );
            match_attrs.insert(
                CompactString::from("group"),
                PyObject::native_function(
                    "Match.group",
                    ferrython_stdlib::text_modules::match_group_fn,
                ),
            );
            match_attrs.insert(
                CompactString::from("groups"),
                PyObject::native_function(
                    "Match.groups",
                    ferrython_stdlib::text_modules::match_groups_fn,
                ),
            );
            match_attrs.insert(
                CompactString::from("groupdict"),
                PyObject::native_function(
                    "Match.groupdict",
                    ferrython_stdlib::text_modules::match_groupdict_fn,
                ),
            );
            match_attrs.insert(
                CompactString::from("start"),
                PyObject::native_function(
                    "Match.start",
                    ferrython_stdlib::text_modules::match_start_fn,
                ),
            );
            match_attrs.insert(
                CompactString::from("end"),
                PyObject::native_function(
                    "Match.end",
                    ferrython_stdlib::text_modules::match_end_fn,
                ),
            );
            match_attrs.insert(
                CompactString::from("span"),
                PyObject::native_function(
                    "Match.span",
                    ferrython_stdlib::text_modules::match_span_fn,
                ),
            );
            match_attrs.insert(
                CompactString::from("_bind_methods"),
                PyObject::bool_val(true),
            );
            let match_obj = PyObject::module_with_attrs(CompactString::from("Match"), match_attrs);

            let replacement = self.call_object(repl_fn.clone(), vec![match_obj])?;
            result.push_str(&replacement.py_to_string());

            last_end = whole.end();
            count += 1;
        }
        result.push_str(&text[last_end..]);

        if return_count {
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(result)),
                PyObject::int(count as i64),
            ]))
        } else {
            Ok(PyObject::str_val(CompactString::from(result)))
        }
    }
}
