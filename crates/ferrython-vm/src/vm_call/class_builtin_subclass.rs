use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    get_builtin_base_type_name, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
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
        if cls.get_attr("__namedtuple__").is_some() {
            return Ok(());
        }
        let PyObjectPayload::Class(cd) = &cls.payload else {
            return Ok(());
        };
        let Some(base_type) = cd.builtin_base_name.as_ref() else {
            return Ok(());
        };

        if base_type.as_str() == "deque" {
            self.ensure_deque_subclass_storage(inst_data, pos_args)?;
            return Ok(());
        }

        if inst_data.attrs.read().contains_key("__builtin_value__") {
            return Ok(());
        }

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

    fn ensure_deque_subclass_storage(
        &mut self,
        inst_data: &ferrython_core::object::InstanceData,
        pos_args: &[PyObjectRef],
    ) -> PyResult<()> {
        if inst_data.attrs.read().contains_key("__deque__") {
            return Ok(());
        }
        let mut maxlen = None;
        if pos_args.len() >= 2 && !matches!(&pos_args[1].payload, PyObjectPayload::None) {
            let raw = pos_args[1].to_int()?;
            if raw < 0 {
                return Err(PyException::value_error("maxlen must be non-negative"));
            }
            maxlen = Some(raw as usize);
        }
        let mut items =
            if pos_args.is_empty() || matches!(&pos_args[0].payload, PyObjectPayload::None) {
                Vec::new()
            } else {
                let iter = self.resolve_iterable(&pos_args[0])?;
                self.collect_iterable(&iter)?
            };
        if let Some(ml) = maxlen {
            if items.len() > ml {
                items = items[items.len() - ml..].to_vec();
            }
        }
        let storage = PyObject::deque_storage(items);
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_data"), storage.clone());
        attrs.insert(intern_or_new("__builtin_value__"), storage);
        attrs.insert(
            CompactString::from("__maxlen__"),
            maxlen
                .map(|n| PyObject::int(n as i64))
                .unwrap_or_else(PyObject::none),
        );
        Ok(())
    }
}
