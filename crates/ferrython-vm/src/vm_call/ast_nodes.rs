use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn try_instantiate_ast_node(
        &mut self,
        cls: &PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(compact_str::CompactString, PyObjectRef)>,
    ) -> PyResult<Option<PyObjectRef>> {
        if Self::ast_class_name(cls).is_none() {
            return Ok(None);
        }

        let exact_legacy = Self::ast_exact_legacy_name(cls);
        let target_cls = if exact_legacy.is_some() {
            Self::ast_constant_class(cls).unwrap_or_else(|| cls.clone())
        } else {
            cls.clone()
        };
        let instance = PyObject::instance(target_cls);

        if exact_legacy.is_none() {
            if let Some(init) = cls.get_attr("__init__") {
                let is_builtin_init = matches!(&init.payload,
                    PyObjectPayload::BuiltinBoundMethod(bbm)
                        if matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(_)));
                let is_ast_native_init = matches!(&init.payload,
                    PyObjectPayload::NativeFunction(nf) if nf.name.as_str() == "_ast.AST.__init__")
                    || matches!(&init.payload,
                        PyObjectPayload::BoundMethod { method, .. }
                            if matches!(&method.payload, PyObjectPayload::NativeFunction(nf) if nf.name.as_str() == "_ast.AST.__init__"));
                if !is_builtin_init && !is_ast_native_init {
                    let init_fn = match &init.payload {
                        PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                        _ => init.clone(),
                    };
                    let mut init_args = vec![instance.clone()];
                    init_args.extend(pos_args);
                    let result = if kwargs.is_empty() {
                        self.call_object(init_fn, init_args)?
                    } else {
                        self.call_object_kw(init_fn, init_args, kwargs)?
                    };
                    if !matches!(&result.payload, PyObjectPayload::None) {
                        return Err(PyException::type_error(
                            "__init__() should return None, not '".to_string()
                                + result.type_name()
                                + "'",
                        ));
                    }
                    return Ok(Some(instance));
                }
            }
        }

        if exact_legacy == Some("Ellipsis") {
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                inst.attrs
                    .write()
                    .insert(CompactString::from("value"), PyObject::ellipsis());
            }
        }

        Self::populate_ast_node_attrs(&instance, cls, &pos_args, &kwargs)?;

        Ok(Some(instance))
    }
}
