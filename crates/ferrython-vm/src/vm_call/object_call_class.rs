use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    get_builtin_base_type_name, ClassData, FxAttrMap, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_class_object(
        &mut self,
        class_obj: &PyObjectRef,
        class_data: &ClassData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if let Some(cls) = self.try_call_type_subclass_as_metaclass(class_obj, class_data, &args)? {
            return Ok(cls);
        }
        if class_data.name == "TypedDict" {
            if let Some(cls) = self.call_typeddict_builtin(&args)? {
                return Ok(cls);
            }
        }
        if get_builtin_base_type_name(class_obj).as_deref() == Some("enumerate") {
            let value = crate::builtins::dispatch("enumerate", &args)?;
            let inst = PyObject::instance(class_obj.clone());
            if let PyObjectPayload::Instance(inst_data) = &inst.payload {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), value);
            }
            return Ok(inst);
        }
        if let Some(meta) = &class_data.metaclass {
            if let Some(call_method) = meta.get_attr("__call__") {
                let is_inherited_type_call = matches!(
                    &call_method.payload,
                    PyObjectPayload::BuiltinBoundMethod(bbm)
                        if bbm.method_name.as_str() == "__call__"
                        && matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(t) if t.as_str() == "type")
                );
                if !is_inherited_type_call {
                    let mut call_args = vec![class_obj.clone()];
                    call_args.extend(args);
                    return self.call_object(call_method, call_args);
                }
            }
        }
        self.instantiate_class(class_obj, args, vec![])
    }

    fn try_call_type_subclass_as_metaclass(
        &mut self,
        meta: &PyObjectRef,
        meta_data: &ClassData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if !class_inherits_type(meta_data) {
            return Ok(None);
        }
        if args.len() != 3 {
            return Ok(None);
        }
        let Some(class_name) = args[0].as_str().map(CompactString::from) else {
            return Ok(None);
        };
        let PyObjectPayload::Tuple(bases_tuple) = &args[1].payload else {
            return Ok(None);
        };
        let bases = bases_tuple.to_vec();
        let namespace = namespace_from_mapping(&args[2])?;
        let mro = VirtualMachine::compute_mro(&bases)?;
        let cls = PyObject::wrap(PyObjectPayload::Class(Box::new(ClassData::new(
            class_name,
            bases.clone(),
            namespace,
            mro,
            Some(meta.clone()),
        ))));
        if let Some(init) = meta_data.namespace.read().get("__init__").cloned() {
            self.call_object(
                init,
                vec![
                    cls.clone(),
                    args[0].clone(),
                    args[1].clone(),
                    args[2].clone(),
                ],
            )?;
        }
        for base in &bases {
            if let PyObjectPayload::Class(base_cd) = &base.payload {
                base_cd
                    .subclasses
                    .write()
                    .push(PyObjectRef::downgrade(&cls));
            }
        }
        Ok(Some(cls))
    }
}

fn class_inherits_type(class_data: &ClassData) -> bool {
    class_data
        .bases
        .iter()
        .chain(class_data.mro.iter())
        .any(|base| match &base.payload {
            PyObjectPayload::BuiltinType(name) => name.as_str() == "type",
            PyObjectPayload::Class(cd) => class_inherits_type(cd),
            _ => false,
        })
}

fn namespace_from_mapping(mapping: &PyObjectRef) -> PyResult<FxAttrMap> {
    match &mapping.payload {
        PyObjectPayload::Dict(map) => {
            let read = map.read();
            let mut namespace = FxAttrMap::default();
            for (key, value) in read.iter() {
                let HashableKey::Str(name) = key else {
                    return Err(PyException::type_error(
                        "type.__new__() argument 3 must be dict with string keys",
                    ));
                };
                namespace.insert(name.to_compact_string(), value.clone());
            }
            Ok(namespace)
        }
        PyObjectPayload::Instance(inst) => {
            let Some(storage) = inst.dict_storage.as_ref() else {
                return Err(PyException::type_error(
                    "type.__new__() argument 3 must be dict",
                ));
            };
            let read = storage.read();
            let mut namespace = FxAttrMap::default();
            for (key, value) in read.iter() {
                let HashableKey::Str(name) = key else {
                    return Err(PyException::type_error(
                        "type.__new__() argument 3 must be dict with string keys",
                    ));
                };
                namespace.insert(name.to_compact_string(), value.clone());
            }
            Ok(namespace)
        }
        _ => Err(PyException::type_error(
            "type.__new__() argument 3 must be dict",
        )),
    }
}
