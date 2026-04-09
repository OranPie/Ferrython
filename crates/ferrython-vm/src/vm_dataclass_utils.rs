use ferrython_core::object::{PyObjectRef, PyObjectPayload, PyObjectMethods};
use ferrython_core::types::HashableKey;

/// Extract ordered field names from __dataclass_fields__, which may be either:
/// - Tuple of (name, has_default, default_val, init_flag) tuples (VM-native format)
/// - Dict mapping field_name → Field instance (Python dataclasses format)
pub fn extract_field_names(fields: &PyObjectRef) -> Vec<String> {
    match &fields.payload {
        PyObjectPayload::Tuple(field_tuples) => {
            field_tuples.iter().filter_map(|ft| {
                if let PyObjectPayload::Tuple(info) = &ft.payload {
                    Some(info[0].py_to_string())
                } else {
                    None
                }
            }).collect()
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            r.iter().map(|(k, field_obj)| {
                match k {
                    HashableKey::Str(s) => s.to_string(),
                    _ => field_obj.get_attr("name")
                        .map(|n| n.py_to_string())
                        .unwrap_or_default(),
                }
            }).collect()
        }
        _ => Vec::new(),
    }
}
