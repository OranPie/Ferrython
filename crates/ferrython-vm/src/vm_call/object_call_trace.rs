use ferrython_core::object::{PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

pub(super) struct CallObjectTraceFrame {
    prev_frame: Option<Option<ferrython_core::object::PyObjectRef>>,
}

impl VirtualMachine {
    pub(super) fn enter_call_object_trace_frame(
        &mut self,
        func: &PyObjectRef,
    ) -> CallObjectTraceFrame {
        let needs_current_frame = ferrython_stdlib::is_trace_active()
            || ferrython_stdlib::is_profile_active()
            || matches!(&func.payload, PyObjectPayload::NativeFunction(nf) if nf.name.as_str() == "sys._getframe");
        let prev_frame = if needs_current_frame {
            Some(ferrython_stdlib::get_current_frame())
        } else {
            None
        };
        if needs_current_frame
            && !self.call_stack.is_empty()
            && !(matches!(&func.payload, PyObjectPayload::NativeFunction(nf)
                if nf.name.as_str() == "sys._getframe")
                && ferrython_stdlib::get_current_frame().is_some())
        {
            ferrython_stdlib::set_current_frame(Some(self.make_trace_frame()));
        }
        CallObjectTraceFrame { prev_frame }
    }

    pub(super) fn leave_call_object_trace_frame(&self, frame: CallObjectTraceFrame) {
        if let Some(prev_frame) = frame.prev_frame {
            ferrython_stdlib::set_current_frame(prev_frame);
        }
    }
}
