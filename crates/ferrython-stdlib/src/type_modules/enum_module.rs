use super::*;

pub fn create_enum_module() -> PyObjectRef {
    // Create Enum as a proper base class with __getitem__ and __iter__ support
    let mut enum_ns = IndexMap::new();
    enum_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));

    // __getitem__ on class — Color['RED'] looks up member by name
    enum_ns.insert(
        CompactString::from("__getitem__"),
        PyObject::native_function("Enum.__getitem__", |args: &[PyObjectRef]| {
            // args[0] = class (self), args[1] = name key
            check_args_min("Enum.__getitem__", args, 2)?;
            let cls = &args[0];
            let name = args[1].py_to_string();
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(member) = ns.get(name.as_str()) {
                    return Ok(member.clone());
                }
                // Check __members__ dict
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        let key = HashableKey::str_key(CompactString::from(name.as_str()));
                        if let Some(member) = map.read().get(&key) {
                            return Ok(member.clone());
                        }
                    }
                }
            }
            Err(PyException::key_error(format!("'{}'", name)))
        }),
    );

    // __call__ on class — Color(1) looks up member by value,
    // OR functional API: Enum("Name", "member1 member2") creates a new enum
    enum_ns.insert(
        CompactString::from("__call__"),
        PyObject::native_function("Enum.__call__", |args: &[PyObjectRef]| {
            check_args_min("Enum.__call__", args, 2)?;
            let cls = &args[0];
            let value = &args[1];

            // Functional API: Enum("Name", "member1 member2") or Enum("Name", ["m1", "m2"])
            if args.len() >= 3 {
                let class_name = value.py_to_string();
                let names_arg = &args[2];
                let member_names: Vec<String> = match &names_arg.payload {
                    PyObjectPayload::Str(s) => {
                        // "member1 member2" or "member1, member2"
                        s.replace(',', " ")
                            .split_whitespace()
                            .map(|s| s.to_string())
                            .collect()
                    }
                    PyObjectPayload::Tuple(items) => items
                        .iter()
                        .map(|i: &PyObjectRef| i.py_to_string())
                        .collect(),
                    PyObjectPayload::List(items) => items
                        .read()
                        .iter()
                        .map(|i: &PyObjectRef| i.py_to_string())
                        .collect(),
                    _ => vec![names_arg.py_to_string()],
                };
                // Create a new class with members
                let mut members_map: FxHashKeyMap = new_fx_hashkey_map();
                let new_cls = PyObject::class(
                    CompactString::from(class_name.as_str()),
                    vec![cls.clone()],
                    IndexMap::new(),
                );
                if let PyObjectPayload::Class(ref cd) = new_cls.payload {
                    let mut ns = cd.namespace.write();
                    for (i, mname) in member_names.iter().enumerate() {
                        let cs_name = CompactString::from(mname.as_str());
                        let mut member_attrs: IndexMap<CompactString, PyObjectRef> =
                            IndexMap::new();
                        member_attrs.insert(
                            CompactString::from("name"),
                            PyObject::str_val(cs_name.clone()),
                        );
                        member_attrs.insert(
                            CompactString::from("_name_"),
                            PyObject::str_val(cs_name.clone()),
                        );
                        member_attrs
                            .insert(CompactString::from("value"), PyObject::int(i as i64 + 1));
                        member_attrs
                            .insert(CompactString::from("_value_"), PyObject::int(i as i64 + 1));
                        let member = PyObject::instance_with_attrs(new_cls.clone(), member_attrs);
                        ns.insert(cs_name.clone(), member.clone());
                        members_map.insert(HashableKey::str_key(cs_name), member);
                    }
                    ns.insert(
                        CompactString::from("__members__"),
                        PyObject::dict(members_map),
                    );
                }
                return Ok(new_cls);
            }

            // Normal call: Color(1) looks up member by value
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        for (_, member) in map.read().iter() {
                            if let Some(v) = member.get_attr("value") {
                                if v.py_to_string() == value.py_to_string() {
                                    return Ok(member.clone());
                                }
                            }
                        }
                    }
                }
            }
            Err(PyException::value_error(format!(
                "{} is not a valid enum value",
                value.repr()
            )))
        }),
    );

    // __iter__ on class — list(Color) iterates members
    enum_ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_function("Enum.__iter__", |args: &[PyObjectRef]| {
            check_args_min("Enum.__iter__", args, 1)?;
            let cls = &args[0];
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        let items: Vec<PyObjectRef> = map.read().values().cloned().collect();
                        return Ok(PyObject::list(items));
                    }
                }
            }
            Ok(PyObject::list(vec![]))
        }),
    );

    // __len__ on class — len(Color) returns member count
    enum_ns.insert(
        CompactString::from("__len__"),
        PyObject::native_function("Enum.__len__", |args: &[PyObjectRef]| {
            check_args_min("Enum.__len__", args, 1)?;
            let cls = &args[0];
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        return Ok(PyObject::int(map.read().len() as i64));
                    }
                }
            }
            Ok(PyObject::int(0))
        }),
    );

    // __contains__ on class — Color.RED in Color
    enum_ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_function("Enum.__contains__", |args: &[PyObjectRef]| {
            check_args_min("Enum.__contains__", args, 2)?;
            let cls = &args[0];
            let item = &args[1];
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        for member in map.read().values() {
                            if PyObjectRef::ptr_eq(member, item) {
                                return Ok(PyObject::bool_val(true));
                            }
                            // Also check by value comparison
                            if let (Some(mv), Some(iv)) =
                                (member.get_attr("value"), item.get_attr("value"))
                            {
                                if mv.py_to_string() == iv.py_to_string() {
                                    return Ok(PyObject::bool_val(true));
                                }
                            }
                        }
                    }
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );

    let enum_class = PyObject::class(CompactString::from("Enum"), vec![], enum_ns);

    // IntEnum — Enum subclass where values are ints and support int operations
    let mut int_enum_ns = IndexMap::new();
    int_enum_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    int_enum_ns.insert(
        CompactString::from("__int_enum__"),
        PyObject::bool_val(true),
    );

    // Helper: extract int value from an IntEnum member or plain int
    fn int_enum_val(obj: &PyObjectRef) -> Option<i64> {
        if let Some(v) = obj.get_attr("_value_") {
            match &v.payload {
                PyObjectPayload::Int(n) => n.to_i64(),
                _ => None,
            }
        } else {
            match &obj.payload {
                PyObjectPayload::Int(n) => n.to_i64(),
                _ => None,
            }
        }
    }

    // __int__ — convert to int
    int_enum_ns.insert(
        CompactString::from("__int__"),
        PyObject::native_function("IntEnum.__int__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            Ok(PyObject::int(int_enum_val(&args[0]).unwrap_or(0)))
        }),
    );

    // __eq__ — compare with int or another IntEnum member
    int_enum_ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_function("IntEnum.__eq__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__eq__", args, 2)?;
            let a = int_enum_val(&args[0]);
            let b = int_enum_val(&args[1]);
            Ok(PyObject::bool_val(a == b))
        }),
    );

    // __lt__
    int_enum_ns.insert(
        CompactString::from("__lt__"),
        PyObject::native_function("IntEnum.__lt__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__lt__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a < b))
        }),
    );

    // __le__
    int_enum_ns.insert(
        CompactString::from("__le__"),
        PyObject::native_function("IntEnum.__le__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__le__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a <= b))
        }),
    );

    // __gt__
    int_enum_ns.insert(
        CompactString::from("__gt__"),
        PyObject::native_function("IntEnum.__gt__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__gt__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a > b))
        }),
    );

    // __ge__
    int_enum_ns.insert(
        CompactString::from("__ge__"),
        PyObject::native_function("IntEnum.__ge__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__ge__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a >= b))
        }),
    );

    // __add__ — IntEnum + int
    int_enum_ns.insert(
        CompactString::from("__add__"),
        PyObject::native_function("IntEnum.__add__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__add__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a + b))
        }),
    );

    // __sub__ — IntEnum - int
    int_enum_ns.insert(
        CompactString::from("__sub__"),
        PyObject::native_function("IntEnum.__sub__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__sub__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a - b))
        }),
    );

    // __mul__ — IntEnum * int
    int_enum_ns.insert(
        CompactString::from("__mul__"),
        PyObject::native_function("IntEnum.__mul__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__mul__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a * b))
        }),
    );

    let int_enum = PyObject::class(
        CompactString::from("IntEnum"),
        vec![
            enum_class.clone(),
            PyObject::builtin_type(CompactString::from("int")),
        ],
        int_enum_ns,
    );

    // Helper: extract int value from a Flag member or plain int
    fn flag_int_val(obj: &PyObjectRef) -> Option<i64> {
        if let Some(v) = obj.get_attr("value") {
            if let PyObjectPayload::Int(ref i) = v.payload {
                return i.to_i64();
            }
        }
        if let PyObjectPayload::Int(ref i) = obj.payload {
            return i.to_i64();
        }
        if let Some(v) = obj.get_attr("_value_") {
            if let PyObjectPayload::Int(ref i) = v.payload {
                return i.to_i64();
            }
        }
        None
    }

    // Flag — class with bitwise support
    let mut flag_ns = IndexMap::new();
    flag_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    flag_ns.insert(CompactString::from("__flag__"), PyObject::bool_val(true));

    // __or__ — combine flags with | operator
    flag_ns.insert(
        CompactString::from("__or__"),
        PyObject::native_function("Flag.__or__", |args: &[PyObjectRef]| {
            check_args("Flag.__or__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a | b))
        }),
    );

    // __and__ — bitwise AND of flags
    flag_ns.insert(
        CompactString::from("__and__"),
        PyObject::native_function("Flag.__and__", |args: &[PyObjectRef]| {
            check_args("Flag.__and__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a & b))
        }),
    );

    // __xor__ — bitwise XOR of flags
    flag_ns.insert(
        CompactString::from("__xor__"),
        PyObject::native_function("Flag.__xor__", |args: &[PyObjectRef]| {
            check_args("Flag.__xor__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a ^ b))
        }),
    );

    // __invert__ — bitwise complement
    flag_ns.insert(
        CompactString::from("__invert__"),
        PyObject::native_function("Flag.__invert__", |args: &[PyObjectRef]| {
            check_args("Flag.__invert__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::int(!v))
        }),
    );

    // __contains__ — check if one flag contains another (a & b == b)
    flag_ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_function("Flag.__contains__", |args: &[PyObjectRef]| {
            check_args("Flag.__contains__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a & b == b))
        }),
    );

    // __bool__ — Flag(0) is falsy
    flag_ns.insert(
        CompactString::from("__bool__"),
        PyObject::native_function("Flag.__bool__", |args: &[PyObjectRef]| {
            check_args("Flag.__bool__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::bool_val(v != 0))
        }),
    );

    // __repr__ — show combined flags in "Flag1|Flag2" format
    flag_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_function("Flag.__repr__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("<Flag>")));
            }
            let self_obj = &args[0];
            let val = flag_int_val(self_obj).unwrap_or(0);
            // Try to get the name directly (single member)
            if let Some(name) = self_obj.get_attr("name") {
                let name_s = name.py_to_string();
                if name_s != "None" && !name_s.is_empty() {
                    // Get class name if available
                    let cls_name = self_obj
                        .get_attr("__class__")
                        .and_then(|c| c.get_attr("__name__"))
                        .map(|n| n.py_to_string())
                        .unwrap_or_else(|| "Flag".to_string());
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "<{}.{}: {}>",
                        cls_name, name_s, val
                    ))));
                }
            }
            // Combined flags — try to decompose by iterating class members
            if val == 0 {
                let cls_name = self_obj
                    .get_attr("__class__")
                    .and_then(|c| c.get_attr("__name__"))
                    .map(|n| n.py_to_string())
                    .unwrap_or_else(|| "Flag".to_string());
                return Ok(PyObject::str_val(CompactString::from(format!(
                    "<{}: 0>",
                    cls_name
                ))));
            }
            Ok(PyObject::str_val(CompactString::from(format!(
                "<Flag: {}>",
                val
            ))))
        }),
    );

    let flag_class = PyObject::class(
        CompactString::from("Flag"),
        vec![
            enum_class.clone(),
            PyObject::builtin_type(CompactString::from("int")),
        ],
        flag_ns,
    );

    // IntFlag — Flag subclass with int arithmetic support
    let mut int_flag_ns = IndexMap::new();
    int_flag_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    int_flag_ns.insert(CompactString::from("__flag__"), PyObject::bool_val(true));
    int_flag_ns.insert(
        CompactString::from("__int_enum__"),
        PyObject::bool_val(true),
    );

    // Bitwise ops (duplicated from Flag since Ferrython doesn't do full MRO for class namespaces)
    int_flag_ns.insert(
        CompactString::from("__or__"),
        PyObject::native_function("IntFlag.__or__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__or__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a | b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__and__"),
        PyObject::native_function("IntFlag.__and__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__and__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a & b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__xor__"),
        PyObject::native_function("IntFlag.__xor__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__xor__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a ^ b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__invert__"),
        PyObject::native_function("IntFlag.__invert__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__invert__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::int(!v))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_function("IntFlag.__contains__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__contains__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a & b == b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__bool__"),
        PyObject::native_function("IntFlag.__bool__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__bool__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::bool_val(v != 0))
        }),
    );

    // Int conversion
    int_flag_ns.insert(
        CompactString::from("__int__"),
        PyObject::native_function("IntFlag.__int__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            Ok(PyObject::int(flag_int_val(&args[0]).unwrap_or(0)))
        }),
    );

    // Comparison ops (same as IntEnum)
    int_flag_ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_function("IntFlag.__eq__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__eq__", args, 2)?;
            let a = flag_int_val(&args[0]);
            let b = flag_int_val(&args[1]);
            Ok(PyObject::bool_val(a == b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__lt__"),
        PyObject::native_function("IntFlag.__lt__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__lt__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a < b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__le__"),
        PyObject::native_function("IntFlag.__le__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__le__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a <= b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__gt__"),
        PyObject::native_function("IntFlag.__gt__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__gt__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a > b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__ge__"),
        PyObject::native_function("IntFlag.__ge__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__ge__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a >= b))
        }),
    );

    // Arithmetic ops (IntFlag acts as int)
    int_flag_ns.insert(
        CompactString::from("__add__"),
        PyObject::native_function("IntFlag.__add__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__add__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a + b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__sub__"),
        PyObject::native_function("IntFlag.__sub__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__sub__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a - b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__mul__"),
        PyObject::native_function("IntFlag.__mul__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__mul__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a * b))
        }),
    );

    // __repr__ for IntFlag
    int_flag_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_function("IntFlag.__repr__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("<IntFlag>")));
            }
            let self_obj = &args[0];
            let val = flag_int_val(self_obj).unwrap_or(0);
            if let Some(name) = self_obj.get_attr("name") {
                let name_s = name.py_to_string();
                if name_s != "None" && !name_s.is_empty() {
                    let cls_name = self_obj
                        .get_attr("__class__")
                        .and_then(|c| c.get_attr("__name__"))
                        .map(|n| n.py_to_string())
                        .unwrap_or_else(|| "IntFlag".to_string());
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "<{}.{}: {}>",
                        cls_name, name_s, val
                    ))));
                }
            }
            if val == 0 {
                let cls_name = self_obj
                    .get_attr("__class__")
                    .and_then(|c| c.get_attr("__name__"))
                    .map(|n| n.py_to_string())
                    .unwrap_or_else(|| "IntFlag".to_string());
                return Ok(PyObject::str_val(CompactString::from(format!(
                    "<{}: 0>",
                    cls_name
                ))));
            }
            Ok(PyObject::str_val(CompactString::from(format!(
                "<IntFlag: {}>",
                val
            ))))
        }),
    );

    let int_flag_class = PyObject::class(
        CompactString::from("IntFlag"),
        vec![flag_class.clone()],
        int_flag_ns,
    );

    // auto() counter — returns a sentinel that process_enum_class resolves
    static AUTO_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

    // unique decorator — validates all values in enum are unique
    let unique_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Ok(PyObject::none());
        }
        let cls = &args[0];
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let ns = cd.namespace.read();
            if let Some(members) = ns.get("__members__") {
                if let PyObjectPayload::Dict(map) = &members.payload {
                    let members_map = map.read();
                    let mut seen_values = Vec::new();
                    for (_, member) in members_map.iter() {
                        if let Some(v) = member.get_attr("value") {
                            let val_str = v.py_to_string();
                            if seen_values.contains(&val_str) {
                                return Err(PyException::value_error(format!(
                                    "duplicate values found in enum {}",
                                    cd.name
                                )));
                            }
                            seen_values.push(val_str);
                        }
                    }
                }
            }
        }
        Ok(args[0].clone())
    });

    // StrEnum (Python 3.11+) — enum where members are also strings
    let mut str_enum_ns = IndexMap::new();
    str_enum_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    str_enum_ns.insert(
        CompactString::from("__str_enum__"),
        PyObject::bool_val(true),
    );
    let str_enum = PyObject::class(
        CompactString::from("StrEnum"),
        vec![
            enum_class.clone(),
            PyObject::builtin_type(CompactString::from("str")),
        ],
        str_enum_ns,
    );

    make_module(
        "enum",
        vec![
            ("Enum", enum_class),
            ("IntEnum", int_enum),
            ("Flag", flag_class),
            ("IntFlag", int_flag_class),
            ("StrEnum", str_enum),
            (
                "auto",
                make_builtin(|_| {
                    // Return a sentinel tuple ("__enum_auto__", counter_value)
                    // process_enum_class will detect this and assign sequential values
                    let val = AUTO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from("__enum_auto__")),
                        PyObject::int(val),
                    ]))
                }),
            ),
            ("unique", unique_fn),
            // sentinel — creates a unique sentinel value (Python 3.13+)
            (
                "sentinel",
                make_builtin(|args: &[PyObjectRef]| {
                    let name = if !args.is_empty() {
                        args[0].py_to_string()
                    } else {
                        "MISSING".to_string()
                    };
                    let mut attrs = IndexMap::new();
                    attrs.insert(
                        CompactString::from("_name"),
                        PyObject::str_val(CompactString::from(name.clone())),
                    );
                    attrs.insert(
                        CompactString::from("__repr__"),
                        PyObject::native_closure("sentinel.__repr__", {
                            let n = name.clone();
                            move |_| Ok(PyObject::str_val(CompactString::from(format!("<{}>", n))))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("__bool__"),
                        make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
                    );
                    Ok(PyObject::module_with_attrs(
                        CompactString::from(name),
                        attrs,
                    ))
                }),
            ),
        ],
    )
}

// ── types module ──
