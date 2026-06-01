use super::*;

thread_local! {
    static ABC_CACHE_TOKEN: PyCell<i64> = PyCell::new(0);
}

pub(crate) fn bump_abc_cache_token() {
    ABC_CACHE_TOKEN.with(|token| {
        *token.write() += 1;
    });
}

fn current_abc_cache_token() -> i64 {
    ABC_CACHE_TOKEN.with(|token| *token.read())
}

pub fn create_abc_module() -> PyObjectRef {
    // ABC base class with __abstractmethods__ marker
    let abc_class = PyObject::class(CompactString::from("ABC"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = abc_class.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("__abstractmethods__"),
            PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(
                ferrython_core::object::new_fx_hashkey_flatmap(),
            )))),
        );
        // ABC.register(subclass) — registers subclass as a virtual subclass
        let abc_ref = abc_class.clone();
        let register_fn = PyObject::native_closure("register", move |args: &[PyObjectRef]| {
            // When called as Printable.register(MyInt), args = [MyInt]
            // When called bound, args = [Printable, MyInt]
            let (cls, subclass) =
                if args.len() >= 2 && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                    (args[0].clone(), args[1].clone())
                } else if args.len() == 1 {
                    // Called unbound: use the ABC class this register was defined on
                    (abc_ref.clone(), args[0].clone())
                } else {
                    return Err(PyException::type_error(
                        "register() requires a subclass argument",
                    ));
                };
            // Store virtual subclass in _abc_registry on the ABC class (Dict with Identity keys)
            if let PyObjectPayload::Class(ref cd) = cls.payload {
                let mut ns = cd.namespace.write();
                let registry = ns
                    .entry(CompactString::from("_abc_registry"))
                    .or_insert_with(|| PyObject::dict(IndexMap::new()))
                    .clone();
                if let PyObjectPayload::Dict(ref map) = registry.payload {
                    let ptr = PyObjectRef::as_ptr(&subclass) as usize;
                    map.write().insert(
                        HashableKey::Identity(ptr, subclass.clone()),
                        PyObject::bool_val(true),
                    );
                }
            }
            // Also mark the subclass with __abc_registered__ pointing to the ABC
            if let PyObjectPayload::Class(ref cd) = subclass.payload {
                cd.namespace
                    .write()
                    .insert(CompactString::from("__abc_registered__"), cls.clone());
            }
            bump_abc_cache_token();
            Ok(subclass.clone())
        });
        ns.insert(CompactString::from("register"), register_fn);
    }

    let abstractmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "abstractmethod requires 1 argument",
            ));
        }
        let func = args[0].clone();
        match &func.payload {
            PyObjectPayload::Function(f) => {
                f.attrs.write().insert(
                    CompactString::from("__isabstractmethod__"),
                    PyObject::bool_val(true),
                );
                return Ok(func);
            }
            PyObjectPayload::ClassMethod(inner) | PyObjectPayload::StaticMethod(inner) => {
                if let PyObjectPayload::Function(f) = &inner.payload {
                    f.attrs.write().insert(
                        CompactString::from("__isabstractmethod__"),
                        PyObject::bool_val(true),
                    );
                }
                return Ok(func);
            }
            _ => {}
        }
        if matches!(&func.payload, PyObjectPayload::Instance(inst)
            if ferrython_core::object::is_property_subclass_class(&inst.class))
        {
            if let PyObjectPayload::Instance(inst) = &func.payload {
                inst.attrs.write().insert(
                    CompactString::from("__isabstractmethod__"),
                    PyObject::bool_val(true),
                );
            }
            return Ok(func);
        }
        Ok(func)
    });

    let abcmeta_cls = {
        let mut ns = IndexMap::new();
        // register(cls, subclass) — register a virtual subclass
        ns.insert(
            CompactString::from("register"),
            PyObject::native_closure("ABCMeta.register", |args: &[PyObjectRef]| {
                // args: [cls (ABCMeta instance), subclass]
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "register() requires a subclass argument",
                    ));
                }
                let cls = &args[0];
                let subclass = &args[1];
                // Store in _abc_registry on the class
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    let mut ns = cd.namespace.write();
                    let registry = ns
                        .entry(CompactString::from("_abc_registry"))
                        .or_insert_with(|| PyObject::dict(IndexMap::new()))
                        .clone();
                    if let PyObjectPayload::Dict(map) = &registry.payload {
                        let ptr = PyObjectRef::as_ptr(subclass) as usize;
                        let key = HashableKey::Identity(ptr, subclass.clone());
                        map.write().insert(key, PyObject::bool_val(true));
                    }
                }
                bump_abc_cache_token();
                Ok(subclass.clone())
            }),
        );
        PyObject::class(
            CompactString::from("ABCMeta"),
            vec![PyObject::builtin_type(CompactString::from("type"))],
            ns,
        )
    };

    let abstractclassmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "abstractclassmethod requires 1 argument",
            ));
        }
        Ok(args[0].clone())
    });

    let abstractstaticmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "abstractstaticmethod requires 1 argument",
            ));
        }
        Ok(args[0].clone())
    });

    let abstractproperty_fn = make_builtin(|args: &[PyObjectRef]| {
        if !args.is_empty() {
            Ok(args[0].clone())
        } else {
            Ok(PyObject::none())
        }
    });

    let get_cache_token_fn =
        PyObject::native_closure("abc.get_cache_token", move |_args: &[PyObjectRef]| {
            Ok(PyObject::int(current_abc_cache_token()))
        });

    make_module(
        "abc",
        vec![
            ("ABC", abc_class),
            ("ABCMeta", abcmeta_cls),
            ("abstractmethod", abstractmethod_fn),
            ("abstractclassmethod", abstractclassmethod_fn),
            ("abstractstaticmethod", abstractstaticmethod_fn),
            ("abstractproperty", abstractproperty_fn),
            ("get_cache_token", get_cache_token_fn),
        ],
    )
}
