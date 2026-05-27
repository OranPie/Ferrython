use ferrython_core::error::PyResult;
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    get_builtin_base_type_name, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use super::class_builtin_values::{BuiltinSubclassStrMode, BuiltinSubclassValue};
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn init_builtin_value_for_builtin_new(
        &mut self,
        cls: &PyObjectRef,
        inst: &PyObjectRef,
        pos_args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        let PyObjectPayload::Instance(inst_data) = &inst.payload else {
            return Ok(None);
        };
        if cls.get_attr("__namedtuple__").is_some() {
            return Ok(None);
        }
        let Some(base_type) = get_builtin_base_type_name(cls) else {
            return Ok(None);
        };

        match self.build_builtin_subclass_value(
            base_type.as_str(),
            pos_args,
            BuiltinSubclassStrMode::VmAware,
        )? {
            BuiltinSubclassValue::Store(Some(value)) => {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), value);
                Ok(None)
            }
            BuiltinSubclassValue::Store(None) => Ok(None),
            BuiltinSubclassValue::Return(value) => Ok(Some(value)),
        }
    }

    pub(super) fn ensure_builtin_subclass_value(
        &mut self,
        cls: &PyObjectRef,
        instance: &PyObjectRef,
        pos_args: &[PyObjectRef],
    ) -> PyResult<()> {
        let PyObjectPayload::Instance(inst_data) = &instance.payload else {
            return Ok(());
        };
        if inst_data.attrs.read().contains_key("__builtin_value__")
            || cls.get_attr("__namedtuple__").is_some()
        {
            return Ok(());
        }
        let PyObjectPayload::Class(cd) = &cls.payload else {
            return Ok(());
        };
        let Some(base_type) = cd.builtin_base_name.as_ref() else {
            return Ok(());
        };

        if let BuiltinSubclassValue::Store(Some(value)) = self.build_builtin_subclass_value(
            base_type.as_str(),
            pos_args,
            BuiltinSubclassStrMode::Plain,
        )? {
            inst_data
                .attrs
                .write()
                .insert(intern_or_new("__builtin_value__"), value);
        }
        Ok(())
    }
}
