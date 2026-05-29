use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{NativeFunctionData, PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_regex_or_iter_native_kw(
        &mut self,
        nf_data: &NativeFunctionData,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<Option<PyObjectRef>> {
        if (nf_data.name.as_str() == "re.sub" || nf_data.name.as_str() == "re.subn")
            && pos_args.len() >= 3
        {
            let repl = &pos_args[1];
            let is_callable = matches!(
                &repl.payload,
                PyObjectPayload::Function(_)
                    | PyObjectPayload::BuiltinFunction(_)
                    | PyObjectPayload::NativeFunction(_)
                    | PyObjectPayload::NativeClosure(_)
                    | PyObjectPayload::Partial(_)
            );
            if is_callable {
                let mut merged = pos_args.to_vec();
                if !kwargs.is_empty() {
                    let mut kw_map = IndexMap::new();
                    for (k, v) in kwargs {
                        kw_map.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                    merged.push(PyObject::dict(kw_map));
                }
                return self
                    .re_sub_with_callable(&merged, nf_data.name.as_str() == "re.subn")
                    .map(Some);
            }
        }

        if nf_data.name.starts_with("re.") {
            if let Some((_, flags_val)) = kwargs.iter().find(|(k, _)| k.as_str() == "flags") {
                let mut all = pos_args.to_vec();
                let flags_index = match nf_data.name.as_str() {
                    "re.compile" => 1,
                    "re.sub" | "re.subn" => 4,
                    "re.split" => 3,
                    _ => 2,
                };
                while all.len() <= flags_index {
                    all.push(PyObject::int(0));
                }
                if matches!(nf_data.name.as_str(), "re.sub" | "re.subn") {
                    if let Some((_, count_val)) = kwargs.iter().find(|(k, _)| k.as_str() == "count")
                    {
                        while all.len() <= 3 {
                            all.push(PyObject::int(0));
                        }
                        all[3] = count_val.clone();
                    }
                } else if nf_data.name.as_str() == "re.split" {
                    if let Some((_, maxsplit_val)) =
                        kwargs.iter().find(|(k, _)| k.as_str() == "maxsplit")
                    {
                        while all.len() <= 2 {
                            all.push(PyObject::int(0));
                        }
                        all[2] = maxsplit_val.clone();
                    }
                }
                all[flags_index] = flags_val.clone();
                return (nf_data.func)(&all).map(Some);
            }
        }

        if nf_data.name.as_str() == "itertools.groupby" && !pos_args.is_empty() {
            let key_fn = kwargs
                .iter()
                .find(|(k, _)| k.as_str() == "key")
                .map(|(_, v)| v.clone())
                .or_else(|| {
                    if pos_args.len() >= 2 {
                        Some(pos_args[1].clone())
                    } else {
                        None
                    }
                });
            let iterable = vec![pos_args[0].clone()];
            return self.vm_itertools_groupby(&iterable, key_fn).map(Some);
        }

        if nf_data.name.as_str() == "itertools.zip_longest" && !kwargs.is_empty() {
            let mut all = pos_args.to_vec();
            let mut kw_map = IndexMap::new();
            for (k, v) in kwargs {
                kw_map.insert(HashableKey::str_key(k.clone()), v.clone());
            }
            kw_map.insert(
                HashableKey::str_key(CompactString::from("__itertools_zip_longest_kwargs__")),
                PyObject::bool_val(true),
            );
            all.push(PyObject::dict(kw_map));
            return (nf_data.func)(&all).map(Some);
        }

        if nf_data.name.as_str() == "itertools.accumulate"
            && !kwargs.is_empty()
            && !pos_args.is_empty()
        {
            let initial = kwargs
                .iter()
                .find(|(k, _)| k.as_str() == "initial")
                .map(|(_, v)| v.clone());
            let func_arg =
                if pos_args.len() >= 2 && !matches!(&pos_args[1].payload, PyObjectPayload::None) {
                    Some(pos_args[1].clone())
                } else {
                    None
                };
            let mut all = vec![pos_args[0].clone()];
            all.push(func_arg.unwrap_or_else(PyObject::none));
            all.push(initial.unwrap_or_else(PyObject::none));
            return (nf_data.func)(&all).map(Some);
        }

        if nf_data.name.as_str() == "re.split" && !kwargs.is_empty() {
            let mut all = pos_args.to_vec();
            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "maxsplit") {
                while all.len() < 3 {
                    all.push(PyObject::int(0));
                }
                all[2] = v.clone();
            }
            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "flags") {
                while all.len() < 4 {
                    all.push(PyObject::int(0));
                }
                all[3] = v.clone();
            }
            return (nf_data.func)(&all).map(Some);
        }

        if nf_data.name.as_str() == "re.sub" && !kwargs.is_empty() {
            let mut all = pos_args.to_vec();
            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "count") {
                while all.len() < 4 {
                    all.push(PyObject::int(0));
                }
                all[3] = v.clone();
            }
            return (nf_data.func)(&all).map(Some);
        }

        Ok(None)
    }
}
