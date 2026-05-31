use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    DequeIterData, IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    SyncUsize,
};
use indexmap::IndexMap;
use std::cell::Cell;
use std::rc::Rc;

use crate::builtins;
use crate::builtins::deque_storage_len;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_or_type(
        &mut self,
        func: &PyObjectRef,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if name.as_str() == "__build_class__" {
            return self.build_class(args);
        }
        if name.as_str() == "itertools._tee" {
            if args.len() != 1 {
                return Err(PyException::type_error("_tee() takes exactly one argument"));
            }
            if let PyObjectPayload::Iterator(iter_data) = &args[0].payload {
                let data = iter_data.read();
                if let IteratorData::Tee {
                    source,
                    buffer,
                    active,
                    index,
                } = &*data
                {
                    return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                        PyCell::new(IteratorData::Tee {
                            source: Rc::clone(source),
                            buffer: Rc::clone(buffer),
                            active: Rc::clone(active),
                            index: *index,
                        }),
                    ))));
                }
            }
            let source = args[0].get_iter()?;
            let source_cell = Rc::new(PyCell::new(source));
            let buffer = Rc::new(PyCell::new(Vec::new()));
            let active = Rc::new(Cell::new(false));
            return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::Tee {
                    source: source_cell,
                    buffer,
                    active,
                    index: 0,
                }),
            ))));
        }
        if matches!(name.as_str(), "_deque_iterator" | "_deque_reverse_iterator") {
            if args.len() != 1 {
                return Err(PyException::type_error(format!(
                    "{}() takes exactly one argument",
                    name
                )));
            }
            let PyObjectPayload::Instance(inst) = &args[0].payload else {
                return Err(PyException::type_error("expected deque object"));
            };
            if !inst.attrs.read().contains_key("__deque__") {
                return Err(PyException::type_error("expected deque object"));
            }
            return Ok(PyObject::tracked(PyObjectPayload::DequeIter(Box::new(
                DequeIterData {
                    source: args[0].clone(),
                    index: SyncUsize::new(0),
                    expected_len: deque_storage_len(&args[0]).unwrap_or_default(),
                    reverse: name.as_str() == "_deque_reverse_iterator",
                },
            ))));
        }
        if matches!(
            name.as_str(),
            "list" | "tuple" | "set" | "frozenset" | "dict"
        ) {
            return self.call_collection_builtin(name.as_str(), args);
        }
        if name.as_str() == "module" {
            return self.call_module_type(args);
        }
        if matches!(name.as_str(), "any" | "all" | "isinstance" | "issubclass") {
            return self.call_predicate_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "sum" | "sorted" | "min" | "max") {
            return self.call_computation_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "getattr" | "setattr" | "delattr") {
            return self.call_attr_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "globals" | "locals" | "vars" | "dir") {
            return self.call_scope_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "str" | "repr" | "mappingproxy") {
            return self.call_text_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "exec" | "eval" | "compile" | "__import__") {
            return self.call_exec_import_builtin(name.as_str(), args);
        }
        if name.as_str() == "NamedTuple" {
            if let Some(result) = self.call_namedtuple_builtin(args)? {
                return Ok(result);
            }
            return self.call_static_builtin(name.as_str(), &[]);
        }
        if matches!(
            name.as_str(),
            "map" | "filter" | "iter" | "next" | "reversed" | "enumerate" | "zip"
        ) {
            return self.call_iterable_builtin(name, args);
        }
        if matches!(
            name.as_str(),
            "len"
                | "abs"
                | "hash"
                | "bin"
                | "oct"
                | "hex"
                | "format"
                | "complex"
                | "int"
                | "float"
                | "round"
                | "bool"
        ) {
            return self.call_numeric_builtin(func, name, args);
        }
        // VM-aware builtins that need to call user-defined methods
        match name.as_str() {
            "print" => {
                return self.vm_print(&args, None, None, None, false);
            }
            "bytes" => {
                return self.vm_bytes_constructor(&args, false);
            }
            "bytearray" => {
                return self.vm_bytes_constructor(&args, true);
            }
            "super" => {
                return self.make_super(&args);
            }
            _ => {}
        }
        self.call_static_builtin(name.as_str(), &args)
    }

    fn call_module_type(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "module() missing required argument 'name'",
            ));
        }
        if args.len() > 2 {
            return Err(PyException::type_error(format!(
                "module() takes at most 2 arguments ({} given)",
                args.len()
            )));
        }
        let name = args[0].py_to_string();
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("__doc__"),
            args.get(1).cloned().unwrap_or_else(PyObject::none),
        );
        Ok(PyObject::module_with_attrs(
            CompactString::from(name),
            attrs,
        ))
    }

    fn call_static_builtin(&mut self, name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        match builtins::get_builtin_fn(name) {
            Some(f) => {
                let result = f(args);
                // Check if breakpoint() was called
                if crate::builtins::core_fns::BREAKPOINT_TRIGGERED
                    .swap(false, std::sync::atomic::Ordering::Relaxed)
                {
                    self.breakpoints.builtin_breakpoint_pending = true;
                    self.handle_breakpoint_hit();
                }
                result
            }
            None => Err(PyException::type_error(format!(
                "'{}' is not callable",
                name
            ))),
        }
    }
}
