use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    get_builtin_base_type_name, new_fx_hashkey_flatmap, new_fx_hashkey_map, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use crate::VirtualMachine;

enum BuiltinSubclassStrMode {
    VmAware,
    Plain,
}

enum BuiltinSubclassValue {
    Store(Option<PyObjectRef>),
    Return(PyObjectRef),
}

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

    fn build_builtin_subclass_value(
        &mut self,
        base_type: &str,
        pos_args: &[PyObjectRef],
        str_mode: BuiltinSubclassStrMode,
    ) -> PyResult<BuiltinSubclassValue> {
        if pos_args.is_empty() {
            return Ok(BuiltinSubclassValue::Store(default_builtin_subclass_value(
                base_type,
            )));
        }

        let value = match base_type {
            "int" => int_builtin_value(&pos_args[0]),
            "float" => float_builtin_value(&pos_args[0]),
            "str" => match str_mode {
                BuiltinSubclassStrMode::VmAware => {
                    if pos_args.len() >= 2 {
                        match &pos_args[0].payload {
                            PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
                                let s = String::from_utf8_lossy(bytes);
                                return Ok(BuiltinSubclassValue::Return(PyObject::str_val(
                                    CompactString::from(s.as_ref()),
                                )));
                            }
                            _ => {}
                        }
                    }
                    let value = match self.vm_str(&pos_args[0]) {
                        Ok(s) => PyObject::str_val(CompactString::from(s)),
                        Err(_) => {
                            PyObject::str_val(CompactString::from(pos_args[0].py_to_string()))
                        }
                    };
                    Some(value)
                }
                BuiltinSubclassStrMode::Plain => Some(PyObject::str_val(CompactString::from(
                    pos_args[0].py_to_string(),
                ))),
            },
            "complex" => complex_builtin_value(pos_args),
            "list" => Some(PyObject::list(
                self.collect_iterable(&pos_args[0]).unwrap_or_default(),
            )),
            "tuple" => {
                if pos_args.len() > 1 {
                    Some(PyObject::tuple(pos_args.to_vec()))
                } else {
                    Some(PyObject::tuple(
                        self.collect_iterable(&pos_args[0]).unwrap_or_default(),
                    ))
                }
            }
            "set" => Some(set_builtin_value(self, &pos_args[0])),
            "frozenset" => Some(frozenset_builtin_value(self, &pos_args[0])),
            "bytes" | "bytearray" => Some(pos_args[0].clone()),
            _ => None,
        };

        Ok(BuiltinSubclassValue::Store(value))
    }
}

fn default_builtin_subclass_value(base_type: &str) -> Option<PyObjectRef> {
    match base_type {
        "list" => Some(PyObject::list(vec![])),
        "set" => Some(PyObject::set_from_flatmap(new_fx_hashkey_flatmap())),
        "frozenset" => Some(PyObject::frozenset(new_fx_hashkey_map())),
        "tuple" => Some(PyObject::tuple(vec![])),
        "int" => Some(PyObject::int(0)),
        "float" => Some(PyObject::float(0.0)),
        "str" => Some(PyObject::str_val(CompactString::from(""))),
        "bytes" => Some(PyObject::bytes(vec![])),
        "bytearray" => Some(PyObject::bytes(vec![])),
        _ => None,
    }
}

fn int_builtin_value(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => Some(arg.clone()),
        PyObjectPayload::Float(f) => Some(PyObject::int(*f as i64)),
        PyObjectPayload::Str(s) => s.trim().parse::<i64>().ok().map(PyObject::int),
        _ => None,
    }
}

fn float_builtin_value(arg: &PyObjectRef) -> Option<PyObjectRef> {
    match &arg.payload {
        PyObjectPayload::Float(_) => Some(arg.clone()),
        PyObjectPayload::Int(n) => Some(PyObject::float(n.to_f64())),
        PyObjectPayload::Bool(b) => Some(PyObject::float(if *b { 1.0 } else { 0.0 })),
        PyObjectPayload::Str(s) => s.trim().parse::<f64>().ok().map(PyObject::float),
        _ => None,
    }
}

fn complex_builtin_value(pos_args: &[PyObjectRef]) -> Option<PyObjectRef> {
    let to_ri = |obj: &PyObjectRef| -> Option<(f64, f64)> {
        match &obj.payload {
            PyObjectPayload::Complex { real, imag } => Some((*real, *imag)),
            PyObjectPayload::Int(n) => Some((n.to_f64(), 0.0)),
            PyObjectPayload::Float(f) => Some((*f, 0.0)),
            PyObjectPayload::Bool(b) => Some((if *b { 1.0 } else { 0.0 }, 0.0)),
            _ => None,
        }
    };

    if pos_args.len() >= 2 {
        match (to_ri(&pos_args[0]), to_ri(&pos_args[1])) {
            (Some((ar, ai)), Some((br, bi))) => {
                let a_c = matches!(&pos_args[0].payload, PyObjectPayload::Complex { .. });
                let b_c = matches!(&pos_args[1].payload, PyObjectPayload::Complex { .. });
                let r = if b_c { ar - bi } else { ar };
                let i = if a_c { ai + br } else { br };
                Some(PyObject::complex(r, i))
            }
            _ => None,
        }
    } else {
        match &pos_args[0].payload {
            PyObjectPayload::Complex { .. } => Some(pos_args[0].clone()),
            PyObjectPayload::Int(n) => Some(PyObject::complex(n.to_f64(), 0.0)),
            PyObjectPayload::Float(f) => Some(PyObject::complex(*f, 0.0)),
            PyObjectPayload::Bool(b) => Some(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0)),
            _ => None,
        }
    }
}

fn set_builtin_value(vm: &mut VirtualMachine, arg: &PyObjectRef) -> PyObjectRef {
    if let PyObjectPayload::Dict(items) = &arg.payload {
        let read = items.read();
        let mut map = new_fx_hashkey_flatmap();
        map.reserve(read.len());
        for key in read.keys() {
            map.insert(key.clone(), key.to_object());
        }
        return PyObject::set_from_flatmap(map);
    }

    let mut map = new_fx_hashkey_flatmap();
    for item in vm.collect_iterable(arg).unwrap_or_default() {
        if let Ok(key) = item.to_hashable_key() {
            map.insert(key, item);
        }
    }
    PyObject::set_from_flatmap(map)
}

fn frozenset_builtin_value(vm: &mut VirtualMachine, arg: &PyObjectRef) -> PyObjectRef {
    if let PyObjectPayload::Dict(items) = &arg.payload {
        let read = items.read();
        let mut map = new_fx_hashkey_map();
        for key in read.keys() {
            map.insert(key.clone(), key.to_object());
        }
        return PyObject::frozenset(map);
    }

    let mut map = new_fx_hashkey_map();
    for item in vm.collect_iterable(arg).unwrap_or_default() {
        if let Ok(key) = item.to_hashable_key() {
            map.insert(key, item);
        }
    }
    PyObject::frozenset(map)
}
