use compact_str::CompactString;
use ferrython_core::object::{PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn split_trailing_kwargs_dict(
        args: &[PyObjectRef],
    ) -> (Vec<PyObjectRef>, Vec<(CompactString, PyObjectRef)>) {
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(map) = &last.payload {
                let kwargs = map
                    .read()
                    .iter()
                    .filter_map(|(key, value)| match key {
                        HashableKey::Str(name) => {
                            Some((CompactString::from(name.as_str()), value.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                return (args[..args.len() - 1].to_vec(), kwargs);
            }
        }
        (args.to_vec(), Vec::new())
    }
}
