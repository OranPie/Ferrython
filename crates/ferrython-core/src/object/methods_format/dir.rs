use super::*;

pub(crate) fn py_dir(obj: &PyObjectRef) -> Vec<CompactString> {
    // Common dunders shared by all types
    let common_dunders = &[
        "__class__",
        "__delattr__",
        "__dir__",
        "__doc__",
        "__eq__",
        "__format__",
        "__ge__",
        "__getattribute__",
        "__gt__",
        "__hash__",
        "__init__",
        "__init_subclass__",
        "__le__",
        "__lt__",
        "__ne__",
        "__new__",
        "__reduce__",
        "__reduce_ex__",
        "__repr__",
        "__setattr__",
        "__sizeof__",
        "__str__",
        "__subclasshook__",
    ];
    match &obj.payload {
        PyObjectPayload::Instance(inst) => {
            let mut names: Vec<CompactString> = inst.attrs.read().keys().cloned().collect();
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                names.extend(cd.namespace.read().keys().cloned());
                // Walk MRO for inherited names
                for base in &cd.mro {
                    if let PyObjectPayload::Class(bcd) = &base.payload {
                        names.extend(bcd.namespace.read().keys().cloned());
                    }
                }
            }
            for d in common_dunders {
                names.push(CompactString::from(*d));
            }
            names.sort();
            names.dedup();
            names
        }
        PyObjectPayload::Class(cd) => {
            let mut n: Vec<_> = cd.namespace.read().keys().cloned().collect();
            // Walk MRO for inherited names
            for base in &cd.mro {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    n.extend(bcd.namespace.read().keys().cloned());
                }
            }
            for d in common_dunders {
                n.push(CompactString::from(*d));
            }
            n.sort();
            n.dedup();
            n
        }
        PyObjectPayload::Module(m) => {
            let mut n: Vec<_> = m.attrs.read().keys().cloned().collect();
            n.sort();
            n
        }
        PyObjectPayload::Function(f) => {
            let mut n: Vec<CompactString> = f.attrs.read().keys().cloned().collect();
            if let Some(dict_obj) = f.attrs.read().get("__dict__").cloned() {
                if let PyObjectPayload::Dict(map) = &dict_obj.payload {
                    for key in map.read().keys() {
                        n.push(CompactString::from(key.to_object().py_to_string()));
                    }
                }
            }
            for d in common_dunders {
                n.push(CompactString::from(*d));
            }
            n.sort();
            n.dedup();
            n
        }
        PyObjectPayload::BoundMethod { method, .. } => {
            let mut n = method.dir();
            if let PyObjectPayload::Function(f) = &method.payload {
                n.extend(f.attrs.read().keys().cloned());
                if let Some(dict_obj) = f.attrs.read().get("__dict__").cloned() {
                    if let PyObjectPayload::Dict(map) = &dict_obj.payload {
                        for key in map.read().keys() {
                            n.push(CompactString::from(key.to_object().py_to_string()));
                        }
                    }
                }
            }
            n.push(CompactString::from("__func__"));
            n.push(CompactString::from("__self__"));
            for d in common_dunders {
                n.push(CompactString::from(*d));
            }
            n.sort();
            n.dedup();
            n
        }
        PyObjectPayload::List(_) => {
            let mut v: Vec<&str> = vec![
                "append",
                "clear",
                "copy",
                "count",
                "extend",
                "index",
                "insert",
                "pop",
                "remove",
                "reverse",
                "sort",
                "__add__",
                "__contains__",
                "__getitem__",
                "__iadd__",
                "__imul__",
                "__iter__",
                "__len__",
                "__mul__",
                "__reversed__",
                "__rmul__",
                "__setitem__",
                "__delitem__",
                "__bool__",
            ];
            v.extend_from_slice(common_dunders);
            v.sort();
            v.dedup();
            v.into_iter().map(CompactString::from).collect()
        }
        PyObjectPayload::Dict(_) => {
            let mut v: Vec<&str> = vec![
                "clear",
                "copy",
                "fromkeys",
                "get",
                "items",
                "keys",
                "pop",
                "popitem",
                "setdefault",
                "update",
                "values",
                "__contains__",
                "__getitem__",
                "__setitem__",
                "__delitem__",
                "__iter__",
                "__len__",
                "__bool__",
            ];
            v.extend_from_slice(common_dunders);
            v.sort();
            v.dedup();
            v.into_iter().map(CompactString::from).collect()
        }
        PyObjectPayload::Str(_) => {
            let mut v: Vec<&str> = vec![
                "capitalize",
                "casefold",
                "center",
                "count",
                "encode",
                "endswith",
                "expandtabs",
                "find",
                "format",
                "format_map",
                "index",
                "isalnum",
                "isalpha",
                "isascii",
                "isdecimal",
                "isdigit",
                "isidentifier",
                "islower",
                "isnumeric",
                "isprintable",
                "isspace",
                "istitle",
                "isupper",
                "join",
                "ljust",
                "lower",
                "lstrip",
                "maketrans",
                "partition",
                "removeprefix",
                "removesuffix",
                "replace",
                "rfind",
                "rindex",
                "rjust",
                "rpartition",
                "rsplit",
                "rstrip",
                "split",
                "splitlines",
                "startswith",
                "strip",
                "swapcase",
                "title",
                "translate",
                "upper",
                "zfill",
                "__add__",
                "__contains__",
                "__getitem__",
                "__iter__",
                "__len__",
                "__mod__",
                "__mul__",
                "__rmul__",
                "__bool__",
            ];
            v.extend_from_slice(common_dunders);
            v.sort();
            v.dedup();
            v.into_iter().map(CompactString::from).collect()
        }
        PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => {
            let mut v: Vec<&str> = vec![
                "bit_length",
                "conjugate",
                "denominator",
                "imag",
                "numerator",
                "real",
                "to_bytes",
                "from_bytes",
                "__abs__",
                "__add__",
                "__and__",
                "__bool__",
                "__ceil__",
                "__divmod__",
                "__float__",
                "__floor__",
                "__floordiv__",
                "__index__",
                "__int__",
                "__invert__",
                "__lshift__",
                "__mod__",
                "__mul__",
                "__neg__",
                "__or__",
                "__pos__",
                "__pow__",
                "__radd__",
                "__rand__",
                "__rfloordiv__",
                "__rlshift__",
                "__rmod__",
                "__rmul__",
                "__ror__",
                "__rpow__",
                "__rrshift__",
                "__rshift__",
                "__rsub__",
                "__rtruediv__",
                "__rxor__",
                "__sub__",
                "__truediv__",
                "__trunc__",
                "__xor__",
            ];
            v.extend_from_slice(common_dunders);
            v.sort();
            v.dedup();
            v.into_iter().map(CompactString::from).collect()
        }
        PyObjectPayload::Float(_) => {
            let mut v: Vec<&str> = vec![
                "as_integer_ratio",
                "conjugate",
                "hex",
                "imag",
                "is_integer",
                "real",
                "__abs__",
                "__add__",
                "__bool__",
                "__divmod__",
                "__float__",
                "__floordiv__",
                "__int__",
                "__mod__",
                "__mul__",
                "__neg__",
                "__pos__",
                "__pow__",
                "__radd__",
                "__rfloordiv__",
                "__rmod__",
                "__rmul__",
                "__rpow__",
                "__rsub__",
                "__rtruediv__",
                "__sub__",
                "__truediv__",
                "__trunc__",
            ];
            v.extend_from_slice(common_dunders);
            v.sort();
            v.dedup();
            v.into_iter().map(CompactString::from).collect()
        }
        PyObjectPayload::Tuple(_) => {
            let mut v: Vec<&str> = vec![
                "count",
                "index",
                "__add__",
                "__contains__",
                "__getitem__",
                "__iter__",
                "__len__",
                "__mul__",
                "__rmul__",
                "__bool__",
            ];
            v.extend_from_slice(common_dunders);
            v.sort();
            v.dedup();
            v.into_iter().map(CompactString::from).collect()
        }
        PyObjectPayload::Set(_) => {
            let mut v: Vec<&str> = vec![
                "add",
                "clear",
                "copy",
                "difference",
                "discard",
                "intersection",
                "isdisjoint",
                "issubset",
                "issuperset",
                "pop",
                "remove",
                "symmetric_difference",
                "union",
                "update",
                "__and__",
                "__contains__",
                "__iand__",
                "__ior__",
                "__isub__",
                "__iter__",
                "__ixor__",
                "__len__",
                "__or__",
                "__sub__",
                "__xor__",
                "__bool__",
            ];
            v.extend_from_slice(common_dunders);
            v.sort();
            v.dedup();
            v.into_iter().map(CompactString::from).collect()
        }
        PyObjectPayload::FrozenSet(_) => {
            let mut v: Vec<&str> = vec![
                "copy",
                "difference",
                "intersection",
                "isdisjoint",
                "issubset",
                "issuperset",
                "symmetric_difference",
                "union",
                "__and__",
                "__contains__",
                "__iter__",
                "__len__",
                "__or__",
                "__sub__",
                "__xor__",
                "__bool__",
            ];
            v.extend_from_slice(common_dunders);
            v.sort();
            v.dedup();
            v.into_iter().map(CompactString::from).collect()
        }
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => {
            let mut v: Vec<&str> = vec![
                "capitalize",
                "center",
                "count",
                "decode",
                "endswith",
                "expandtabs",
                "find",
                "fromhex",
                "hex",
                "index",
                "isalnum",
                "isalpha",
                "isdigit",
                "islower",
                "isspace",
                "istitle",
                "isupper",
                "join",
                "ljust",
                "lower",
                "lstrip",
                "partition",
                "removeprefix",
                "removesuffix",
                "replace",
                "rfind",
                "rindex",
                "rjust",
                "rpartition",
                "rsplit",
                "rstrip",
                "split",
                "splitlines",
                "startswith",
                "strip",
                "swapcase",
                "title",
                "upper",
                "zfill",
                "append",
                "extend",
                "insert",
                "pop",
                "clear",
                "copy",
                "reverse",
                "__add__",
                "__contains__",
                "__getitem__",
                "__iter__",
                "__len__",
                "__mul__",
                "__rmod__",
                "__rmul__",
                "__bool__",
            ];
            v.extend_from_slice(common_dunders);
            v.sort();
            v.dedup();
            v.into_iter().map(CompactString::from).collect()
        }
        _ => common_dunders
            .iter()
            .map(|s| CompactString::from(*s))
            .collect(),
    }
}
