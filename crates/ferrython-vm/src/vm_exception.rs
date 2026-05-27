//! Exception traceback and unwind helpers.

use crate::frame::BlockKind;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{new_fx_hashkey_map, PyObject, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;

impl VirtualMachine {
    /// Attach traceback entries from the current call stack to an exception.
    pub(crate) fn attach_traceback(&self, exc: &mut PyException) {
        use ferrython_core::error::TracebackEntry;
        for frame in &self.call_stack {
            let lineno = ferrython_debug::resolve_lineno(&frame.code, frame.ip.saturating_sub(1));
            exc.traceback.push(TracebackEntry {
                filename: frame.code.filename.to_string(),
                function: frame.code.name.to_string(),
                lineno,
            });
        }
    }

    /// Build a Python-level traceback object chain (CPython-compatible).
    /// The returned object is the outermost frame, with tb_next pointing towards
    /// the innermost frame (matching CPython's `sys.exc_info()[2]` chain order).
    pub(crate) fn build_traceback_object(
        entries: &[ferrython_core::error::TracebackEntry],
    ) -> PyObjectRef {
        if entries.is_empty() {
            return PyObject::none();
        }
        // entries are ordered [outermost, ..., innermost].
        // CPython chain: outermost -> ... -> innermost -> None
        // Build from innermost to outermost so tb_next links are correct.
        let tb_class = PyObject::builtin_type(CompactString::from("traceback"));
        let frame_class = PyObject::builtin_type(CompactString::from("frame"));
        let mut tb_next = PyObject::none();
        for entry in entries.iter().rev() {
            // Build a minimal frame-like object for tb_frame
            let mut frame_attrs = IndexMap::new();
            frame_attrs.insert(
                CompactString::from("f_lineno"),
                PyObject::int(entry.lineno as i64),
            );
            let mut code_attrs = IndexMap::new();
            code_attrs.insert(
                CompactString::from("co_filename"),
                PyObject::str_val(CompactString::from(&entry.filename)),
            );
            code_attrs.insert(
                CompactString::from("co_name"),
                PyObject::str_val(CompactString::from(&entry.function)),
            );
            let code_class = PyObject::builtin_type(CompactString::from("code"));
            let code_obj = PyObject::instance_with_attrs(code_class, code_attrs);
            frame_attrs.insert(CompactString::from("f_code"), code_obj);
            frame_attrs.insert(
                CompactString::from("f_locals"),
                PyObject::dict(new_fx_hashkey_map()),
            );
            frame_attrs.insert(
                CompactString::from("f_globals"),
                PyObject::dict(new_fx_hashkey_map()),
            );
            let frame_obj = PyObject::instance_with_attrs(frame_class.clone(), frame_attrs);

            let mut attrs = IndexMap::new();
            attrs.insert(
                CompactString::from("tb_lineno"),
                PyObject::int(entry.lineno as i64),
            );
            attrs.insert(CompactString::from("tb_frame"), frame_obj);
            attrs.insert(CompactString::from("tb_next"), tb_next);
            attrs.insert(
                CompactString::from("tb_filename"),
                PyObject::str_val(CompactString::from(&entry.filename)),
            );
            attrs.insert(
                CompactString::from("tb_name"),
                PyObject::str_val(CompactString::from(&entry.function)),
            );
            tb_next = PyObject::instance_with_attrs(tb_class.clone(), attrs);
        }
        tb_next
    }

    /// Store an attribute on an exception value object (works for both Instance and ExceptionInstance).
    pub(crate) fn store_exc_attr(exc_value: &PyObjectRef, name: &str, value: PyObjectRef) {
        match &exc_value.payload {
            PyObjectPayload::Instance(inst) => {
                unsafe { &mut *inst.attrs.data_ptr() }.insert(CompactString::from(name), value);
            }
            PyObjectPayload::ExceptionInstance(ei) => {
                ei.ensure_attrs()
                    .write()
                    .insert(CompactString::from(name), value);
            }
            _ => {}
        }
    }

    /// Find an exception handler on the block stack. Returns handler IP if found.
    pub(crate) fn unwind_except(&mut self) -> Option<usize> {
        loop {
            let restore_interrupted_handler = {
                let frame = self.call_stack.last_mut()?;
                let block = frame.pop_block()?;
                match block.kind() {
                    BlockKind::Except | BlockKind::Finally => {
                        // Unwind value stack to block level
                        while frame.stack.len() > block.stack_level() {
                            frame.pop();
                        }
                        // Push an ExceptHandler block so PopExcept can find it
                        frame.push_block(BlockKind::ExceptHandler, 0);
                        return Some(block.handler());
                    }
                    BlockKind::ExceptHandler => {
                        // Clean up a previous except handler (exception in except body)
                        while frame.stack.len() > block.stack_level() {
                            frame.pop();
                        }
                        true
                    }
                    BlockKind::Loop => {
                        while frame.stack.len() > block.stack_level() {
                            frame.pop();
                        }
                        false
                    }
                    BlockKind::With => {
                        // With block exception — jump to cleanup handler which will
                        // call __exit__ with exception info
                        while frame.stack.len() > block.stack_level() {
                            frame.pop();
                        }
                        return Some(block.handler());
                    }
                }
            };
            if restore_interrupted_handler {
                self.restore_previous_exception();
            }
        }
    }
}
