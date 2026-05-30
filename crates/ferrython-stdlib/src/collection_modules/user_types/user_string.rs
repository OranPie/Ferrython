use super::*;
use ferrython_core::types::hash_key_like_python;

const DATA_ATTR: &str = "data";
const BUILTIN_VALUE_ATTR: &str = "__builtin_value__";

pub(in crate::collection_modules) fn make_user_string_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        native_method("UserString", "__init__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("UserString.__init__ requires self"));
            }
            let init_value = userstring_init_value(args)?;
            set_userstring_value(&args[0], init_value)?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__str__"),
        native_method("UserString", "__str__", |args| {
            Ok(PyObject::str_val(CompactString::from(userstring_value(
                &args[0],
            )?)))
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        native_method("UserString", "__repr__", |args| {
            let data = PyObject::str_val(CompactString::from(userstring_value(&args[0])?));
            Ok(PyObject::str_val(CompactString::from(data.repr())))
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        native_method("UserString", "__len__", |args| {
            Ok(PyObject::int(
                userstring_value(&args[0])?.chars().count() as i64
            ))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        native_method("UserString", "__contains__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "UserString.__contains__ requires self and item",
                ));
            }
            let data = userstring_value(&args[0])?;
            let needle = maybe_userstring_value(&args[1]).ok_or_else(|| {
                PyException::type_error("'in <string>' requires string as left operand")
            })?;
            Ok(PyObject::bool_val(data.contains(&needle)))
        }),
    );
    ns.insert(
        CompactString::from("__hash__"),
        native_method("UserString", "__hash__", |args| {
            let key = HashableKey::str_key(CompactString::from(userstring_value(&args[0])?));
            Ok(PyObject::int(hash_key_like_python(&key)))
        }),
    );
    ns.insert(
        CompactString::from("__iadd__"),
        native_method("UserString", "__iadd__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "UserString.__iadd__ requires self and other",
                ));
            }
            let mut value = userstring_value(&args[0])?;
            value.push_str(&string_operand(&args[1]));
            set_userstring_value(&args[0], value)?;
            Ok(args[0].clone())
        }),
    );
    ns.insert(
        CompactString::from("__rmod__"),
        native_method("UserString", "__rmod__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "UserString.__rmod__ requires self and other",
                ));
            }
            let arg = PyObject::str_val(CompactString::from(userstring_value(&args[0])?));
            args[1].modulo(&arg)
        }),
    );

    PyObject::class(
        CompactString::from("UserString"),
        vec![PyObject::builtin_type(CompactString::from("str"))],
        ns,
    )
}

fn userstring_init_value(args: &[PyObjectRef]) -> PyResult<String> {
    let mut seq = args.get(1).cloned();
    if let Some(PyObjectPayload::Dict(kwargs)) = args.last().map(|obj| &obj.payload) {
        if let Some(value) = kwargs
            .read()
            .get(&HashableKey::str_key(CompactString::from("seq")))
            .cloned()
        {
            seq = Some(value);
        } else if args.len() == 2 {
            seq = None;
        }
    }
    Ok(seq.as_ref().map(string_operand).unwrap_or_default())
}

fn string_operand(obj: &PyObjectRef) -> String {
    if let Some(value) = maybe_userstring_value(obj) {
        value
    } else if let Some(s) = obj.as_str() {
        s.to_string()
    } else {
        obj.py_to_string()
    }
}

fn userstring_value(obj: &PyObjectRef) -> PyResult<String> {
    maybe_userstring_value(obj).ok_or_else(|| {
        PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'",
            obj.type_name(),
            DATA_ATTR
        ))
    })
}

fn maybe_userstring_value(obj: &PyObjectRef) -> Option<String> {
    match &obj.payload {
        PyObjectPayload::Str(s) => Some(s.to_string()),
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            attrs
                .get(DATA_ATTR)
                .or_else(|| attrs.get(BUILTIN_VALUE_ATTR))
                .and_then(|value| value.as_str().map(ToString::to_string))
        }
        _ => None,
    }
}

fn set_userstring_value(obj: &PyObjectRef, value: String) -> PyResult<()> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return Err(PyException::type_error(
            "UserString.__init__ requires an instance",
        ));
    };
    let value_obj = PyObject::str_val(CompactString::from(value));
    let mut attrs = inst.attrs.write();
    attrs.insert(CompactString::from(DATA_ATTR), value_obj.clone());
    attrs.insert(CompactString::from(BUILTIN_VALUE_ATTR), value_obj);
    Ok(())
}
