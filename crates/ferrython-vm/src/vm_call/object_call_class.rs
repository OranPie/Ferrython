use ferrython_core::error::PyResult;
use ferrython_core::object::{ClassData, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_class_object(
        &mut self,
        class_obj: &PyObjectRef,
        class_data: &ClassData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
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
}
