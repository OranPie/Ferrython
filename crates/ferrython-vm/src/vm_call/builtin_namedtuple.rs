use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    new_fx_hashkey_map, FxHashKeyMap, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_namedtuple_builtin(
        &mut self,
        args: Vec<PyObjectRef>,
    ) -> PyResult<Option<PyObjectRef>> {
        if args.is_empty() {
            return Ok(None);
        }

        let typename = args[0].py_to_string();
        let mut field_names: Vec<CompactString> = Vec::new();

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
                                    field_names.push(CompactString::from(pair[0].py_to_string()));
                                }
                            } else {
                                field_names.push(CompactString::from(item.py_to_string()));
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
        Ok(Some(PyObject::class(
            CompactString::from(typename),
            vec![],
            ns,
        )))
    }
}
