//! Trace, profile, breakpoint, and excepthook helpers.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::code::CodeObject;
use ferrython_core::error::PyException;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;

impl VirtualMachine {
    /// Handle a breakpoint hit — print location info and current stack state.
    pub(crate) fn handle_breakpoint_hit(&self) {
        if let Some(frame) = self.call_stack.last() {
            let lineno = ferrython_debug::resolve_lineno(&frame.code, frame.ip.saturating_sub(1));
            eprintln!(
                "*** Breakpoint hit: File \"{}\", line {}, in {} ***",
                frame.code.filename, lineno, frame.code.name
            );
            // Print local variables if in a function scope
            if frame.scope_kind == crate::frame::ScopeKind::Function {
                let mut locals_info = Vec::new();
                for (i, name) in frame.code.varnames.iter().enumerate() {
                    if let Some(val) = frame.locals.get(i).and_then(|v| v.as_ref()) {
                        locals_info.push(format!("  {} = {}", name, val.py_to_string()));
                    }
                }
                if !locals_info.is_empty() {
                    eprintln!("  Locals:");
                    for info in &locals_info {
                        eprintln!("{}", info);
                    }
                }
            }
        }
    }

    /// Build a minimal frame object for trace/profile callbacks.
    pub(crate) fn make_trace_frame(&self) -> PyObjectRef {
        self.make_trace_frame_at(self.call_stack.len() - 1, 0)
    }

    fn make_trace_frame_at(&self, depth: usize, recurse_depth: usize) -> PyObjectRef {
        let frame = &self.call_stack[depth];
        let ip = if frame.ip > 0 { frame.ip - 1 } else { 0 };
        let lineno = Self::ip_to_line(&frame.code, ip);
        let mut attrs = IndexMap::new();

        // f_code: code object with co_filename, co_name, co_firstlineno, co_varnames, co_argcount
        attrs.insert(CompactString::from("f_code"), {
            let mut code_attrs = IndexMap::new();
            code_attrs.insert(
                CompactString::from("co_filename"),
                PyObject::str_val(frame.code.filename.clone()),
            );
            code_attrs.insert(
                CompactString::from("co_name"),
                PyObject::str_val(frame.code.name.clone()),
            );
            code_attrs.insert(
                CompactString::from("co_firstlineno"),
                PyObject::int(frame.code.first_line_number as i64),
            );
            code_attrs.insert(
                CompactString::from("co_argcount"),
                PyObject::int(frame.code.arg_count as i64),
            );
            let varnames: Vec<PyObjectRef> = frame
                .code
                .varnames
                .iter()
                .map(|n| PyObject::str_val(n.clone()))
                .collect();
            code_attrs.insert(
                CompactString::from("co_varnames"),
                PyObject::tuple(varnames),
            );
            let code_class = PyObject::builtin_type(CompactString::from("code"));
            PyObject::instance_with_attrs(code_class, code_attrs)
        });

        attrs.insert(
            CompactString::from("f_lineno"),
            PyObject::int(lineno as i64),
        );
        attrs.insert(CompactString::from("f_lasti"), PyObject::int(ip as i64));

        // f_locals: real local variables from the frame
        let mut local_pairs = Vec::new();
        for (i, name) in frame.code.varnames.iter().enumerate() {
            if let Some(Some(val)) = frame.locals.get(i) {
                local_pairs.push((PyObject::str_val(name.clone()), val.clone()));
            }
        }
        for (name, val) in frame.local_names_snapshot() {
            local_pairs.push((PyObject::str_val(name.clone()), val.clone()));
        }
        attrs.insert(
            CompactString::from("f_locals"),
            PyObject::dict_from_pairs(local_pairs),
        );

        // f_globals: snapshot of globals dict
        let global_pairs: Vec<(PyObjectRef, PyObjectRef)> = frame
            .globals
            .read()
            .iter()
            .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
            .collect();
        attrs.insert(
            CompactString::from("f_globals"),
            PyObject::dict_from_pairs(global_pairs),
        );

        // f_back: parent frame (limit recursion to 10 levels to avoid stack overflow)
        let f_back = if depth > 0 && recurse_depth < 10 {
            self.make_trace_frame_at(depth - 1, recurse_depth + 1)
        } else {
            PyObject::none()
        };
        attrs.insert(CompactString::from("f_back"), f_back);

        let frame_class = PyObject::builtin_type(CompactString::from("frame"));
        PyObject::instance_with_attrs(frame_class, attrs)
    }

    /// Resolve instruction pointer to source line number.
    pub(crate) fn ip_to_line(code: &CodeObject, ip: usize) -> u32 {
        let mut line = code.first_line_number;
        for &(offset, ln) in &code.line_number_table {
            if offset as usize > ip {
                break;
            }
            line = ln;
        }
        line
    }

    /// Fire a trace event to the registered sys.settrace function.
    /// Events: "call", "line", "return", "exception"
    pub(crate) fn fire_trace_event(&mut self, event: &str, arg: PyObjectRef) {
        let trace_fn = match ferrython_stdlib::get_trace_func() {
            Some(f) => f,
            None => return,
        };
        let frame_obj = self.make_trace_frame();
        let event_str = PyObject::str_val(CompactString::from(event));
        // Call trace_fn(frame, event, arg) — ignore errors to avoid infinite recursion
        // Temporarily disable trace during callback to prevent re-entrant calls
        ferrython_stdlib::set_trace_func(None);
        let result = self.call_object(trace_fn.clone(), vec![frame_obj, event_str, arg]);
        // If the trace function returns None, tracing is disabled for this scope
        // Otherwise, re-install (could be a different local trace function)
        match result {
            Ok(ref val) if !matches!(&val.payload, PyObjectPayload::None) => {
                ferrython_stdlib::set_trace_func(Some(trace_fn));
            }
            Ok(_) => {
                // Returned None — re-install the global trace function
                ferrython_stdlib::set_trace_func(Some(trace_fn));
            }
            Err(_) => {
                // Error in trace function — disable tracing (CPython behavior)
            }
        }
    }

    /// Fire a profile event to the registered sys.setprofile function.
    /// Events: "call", "return", "c_call", "c_return", "c_exception"
    pub(crate) fn fire_profile_event(&mut self, event: &str, arg: PyObjectRef) {
        let profile_fn = match ferrython_stdlib::get_profile_func() {
            Some(f) => f,
            None => return,
        };
        let frame_obj = self.make_trace_frame();
        let event_str = PyObject::str_val(CompactString::from(event));
        ferrython_stdlib::set_profile_func(None);
        let _ = self.call_object(profile_fn.clone(), vec![frame_obj, event_str, arg]);
        ferrython_stdlib::set_profile_func(Some(profile_fn));
    }

    /// Invoke sys.excepthook if set. Returns true if the hook was called successfully.
    pub fn invoke_excepthook(&mut self, exc: &PyException) -> bool {
        // Look up excepthook from the sys module (user may have reassigned it)
        let hook = if let Some(sys_mod) = self.modules.get("sys") {
            if let Some(h) = sys_mod.get_attr("excepthook") {
                // Check if it's the default (a BuiltinFunction named "sys_excepthook_default")
                // If default, fall through to normal traceback display
                if let PyObjectPayload::BuiltinFunction(name) = &h.payload {
                    if name.contains("excepthook") {
                        return false;
                    }
                }
                Some(h)
            } else {
                None
            }
        } else {
            None
        };
        let hook = match hook {
            Some(h) => h,
            None => return false,
        };
        let exc_type = PyObject::exception_type(exc.kind);
        let exc_value = PyObject::str_val(exc.message.clone());
        let exc_tb = PyObject::none();
        self.call_object(hook, vec![exc_type, exc_value, exc_tb])
            .is_ok()
    }
}
