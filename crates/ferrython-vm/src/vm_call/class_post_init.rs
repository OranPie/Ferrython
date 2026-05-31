use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    has_descriptor_get, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use crate::VirtualMachine;

impl VirtualMachine {
    fn default_object_init_error(cls: &PyObjectRef) -> PyException {
        let cls_name = match &cls.payload {
            PyObjectPayload::Class(cd) => cd.name.as_str(),
            _ => "object",
        };
        PyException::type_error(format!(
            "{}.__init__() takes exactly one argument (the instance to initialize)",
            cls_name
        ))
    }

    pub(super) fn call_user_init_for_instance(
        &mut self,
        cls: &PyObjectRef,
        instance: &PyObjectRef,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<()> {
        if let Some(init) = cls.get_attr("__init__") {
            let init = if has_descriptor_get(&init) {
                self.resolve_descriptor(&init, instance)?
            } else {
                init
            };
            if matches!(
                &init.payload,
                PyObjectPayload::NativeFunction(nf)
                    if nf.name.as_str() == "collections.deque.__init__"
            ) && matches!(
                &instance.payload,
                PyObjectPayload::Instance(inst) if inst.attrs.read().contains_key("__deque__")
            ) {
                return Ok(());
            }
            let is_builtin_init = matches!(&init.payload,
                PyObjectPayload::BuiltinBoundMethod(bbm)
                    if matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(_)));
            if !is_builtin_init {
                let init_fn = match &init.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => init.clone(),
                };
                if let PyObjectPayload::NativeFunction(nf) = &init_fn.payload {
                    if nf.name.as_str() == "__init__"
                        && (!pos_args.is_empty() || !kwargs.is_empty())
                    {
                        return Err(Self::default_object_init_error(cls));
                    }
                }
                let mut init_args = vec![instance.clone()];
                init_args.extend(pos_args.iter().cloned());
                let init_result = if kwargs.is_empty() {
                    self.call_object(init_fn, init_args)?
                } else {
                    self.call_object_kw(init_fn, init_args, kwargs.to_vec())?
                };
                if !matches!(&init_result.payload, PyObjectPayload::None) {
                    return Err(PyException::type_error(
                        "__init__() should return None, not '".to_string()
                            + init_result.type_name()
                            + "'",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn store_unused_constructor_kwargs(
        &self,
        cls: &PyObjectRef,
        instance: &PyObjectRef,
        kwargs: &[(CompactString, PyObjectRef)],
    ) {
        if !kwargs.is_empty() && cls.get_attr("__namedtuple__").is_none() {
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                let mut attrs = inst.attrs.write();
                for (k, v) in kwargs {
                    if !attrs.contains_key(k.as_str()) {
                        attrs.insert(k.clone(), v.clone());
                    }
                }
            }
        }
    }

    pub(super) fn map_pos_args_to_fields(
        &self,
        cls: &PyObjectRef,
        instance: &PyObjectRef,
        pos_args: &[PyObjectRef],
    ) {
        if !pos_args.is_empty() && cls.get_attr("__namedtuple__").is_none() {
            if let Some(fields_obj) = cls.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields_obj.payload {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        let mut attrs = inst.attrs.write();
                        for (i, field) in field_names.iter().enumerate() {
                            if i < pos_args.len() {
                                let fname = field.py_to_string();
                                if !attrs.contains_key(fname.as_str()) {
                                    attrs.insert(
                                        CompactString::from(fname.as_str()),
                                        pos_args[i].clone(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub(super) fn populate_exception_args(
        &self,
        cls: &PyObjectRef,
        instance: &PyObjectRef,
        pos_args: Vec<PyObjectRef>,
    ) {
        if Self::is_exception_class(cls) {
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                let mut attrs = inst.attrs.write();
                if !attrs.contains_key("args") {
                    attrs.insert(CompactString::from("args"), PyObject::tuple(pos_args));
                }
            }
        }
    }
}
