use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    /// Unified class instantiation: __new__, dataclass/namedtuple auto-init, __init__, exception attrs.
    pub(crate) fn instantiate_class(
        &mut self,
        cls: &PyObjectRef,
        mut pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if let Some(instance) =
            self.try_instantiate_ast_node(cls, pos_args.clone(), kwargs.clone())?
        {
            return Ok(instance);
        }

        if ferrython_core::object::is_property_subclass_class(cls) {
            let instance = PyObject::instance(cls.clone());
            Self::init_property_instance_attrs(&instance, &pos_args, &kwargs)?;
            return Ok(instance);
        }

        if let Some(instance) = self.try_instantiate_simple_class(cls, &mut pos_args, &kwargs)? {
            return Ok(instance);
        }
        self.check_abstract_class_instantiation(cls)?;
        if let Some(instance) = self.try_instantiate_enum(cls, &pos_args, &kwargs)? {
            return Ok(instance);
        }
        // __new__
        let instance = if cls.get_attr("__namedtuple__").is_some() {
            PyObject::instance(cls.clone())
        } else if let Some(new_method) = cls.get_attr("__new__") {
            // If __new__ is from a BuiltinType base (dict, list, etc.), just create instance
            let is_builtin_new = matches!(&new_method.payload,
                PyObjectPayload::BuiltinBoundMethod(bbm)
                    if matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(_))
            );
            // Also recognize builtin __new__ NativeFunctions (tuple.__new__, list.__new__, etc.)
            let is_native_builtin_new = matches!(&new_method.payload,
                PyObjectPayload::NativeFunction(nf)
                    if nf.name.ends_with(".__new__") && matches!(nf.name.as_str(),
                        "tuple.__new__" | "list.__new__" | "str.__new__" | "int.__new__"
                        | "float.__new__" | "complex.__new__" | "object.__new__")
            );
            if is_builtin_new || is_native_builtin_new {
                let inst = PyObject::instance(cls.clone());
                if let Some(result) =
                    self.init_builtin_value_for_builtin_new(cls, &inst, &pos_args)?
                {
                    return Ok(result);
                }
                inst
            } else {
                let new_fn = match &new_method.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => new_method.clone(),
                };
                let mut new_args = vec![cls.clone()];
                new_args.extend(pos_args.clone());
                // Forward kwargs to __new__
                if kwargs.is_empty() {
                    self.call_object(new_fn, new_args)?
                } else {
                    self.call_object_kw(new_fn, new_args, kwargs.clone())?
                }
            }
        } else {
            PyObject::instance(cls.clone())
        };

        self.ensure_builtin_subclass_value(cls, &instance, &pos_args)?;

        let is_dataclass = Self::class_has_namespace_key(cls, "__dataclass__");
        let has_user_init = cls.get_attr("__init__").is_some();
        let default_dict_init = cls
            .get_attr("__init__")
            .map(|init| match &init.payload {
                PyObjectPayload::NativeFunction(nf) => nf.name.as_str() == "dict.__init__",
                PyObjectPayload::BuiltinBoundMethod(bbm) => {
                    bbm.method_name.as_str() == "__init__"
                        && matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(name) if name.as_str() == "dict")
                }
                _ => false,
            })
            .unwrap_or(false);

        if is_dataclass && !has_user_init {
            self.init_dataclass_instance(cls, &instance, &pos_args, &kwargs)?;
        } else if Self::class_has_namespace_key(cls, "__namedtuple__") {
            self.init_namedtuple_instance(cls, &instance, &pos_args, &kwargs)?;
        } else if cls.get_attr("__init__").is_some() {
            self.call_user_init_for_instance(cls, &instance, &pos_args, &kwargs)?;
        }

        if default_dict_init {
            self.populate_dict_subclass_storage(&instance, &pos_args, &kwargs)?;
        }

        self.store_unused_constructor_kwargs(cls, &instance, &kwargs);

        self.map_pos_args_to_fields(cls, &instance, &pos_args);

        self.populate_exception_args(cls, &instance, pos_args);

        Ok(instance)
    }
}
