use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

use super::super::extract_bytes;
use super::read::pickle_loads_stack;
use super::write::{pickle_serialize, pickle_serialize_p0, pickle_serialize_p2, PickleWriteMemo};

// ── Public API ──

pub fn create_pickle_module() -> PyObjectRef {
    let pickler_cls = {
        PyObject::native_closure("Pickler", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Pickler requires a file argument"));
            }
            let file = args[0].clone();
            let protocol = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
            let buf: Rc<PyCell<Vec<u8>>> = Rc::new(PyCell::new(Vec::new()));

            let cls_inner =
                PyObject::class(CompactString::from("Pickler"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls_inner);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_file"), file.clone());
                w.insert(CompactString::from("protocol"), PyObject::int(protocol));
                let b = buf.clone();
                let f = file.clone();
                w.insert(
                    CompactString::from("dump"),
                    PyObject::native_closure("dump", move |dargs| {
                        if dargs.is_empty() {
                            return Err(PyException::type_error("dump requires an object"));
                        }
                        let obj = &dargs[dargs.len() - 1];
                        let mut data = b.write();
                        data.clear();
                        pickle_serialize(obj, &mut data)?;
                        if let Some(write_fn) = f.get_attr("write") {
                            let bytes_obj = PyObject::bytes(data.clone());
                            ferrython_core::error::request_vm_call(write_fn, vec![bytes_obj]);
                        }
                        Ok(PyObject::none())
                    }),
                );
                w.insert(
                    CompactString::from("clear_memo"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
            }
            Ok(inst)
        })
    };

    let unpickler_cls = {
        PyObject::native_closure("Unpickler", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "Unpickler requires a file argument",
                ));
            }
            let file = args[0].clone();
            let cls_inner =
                PyObject::class(CompactString::from("Unpickler"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls_inner);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_file"), file.clone());
                let f = file.clone();
                w.insert(
                    CompactString::from("load"),
                    PyObject::native_closure("load", move |_largs| {
                        if let Some(read_fn) = f.get_attr("read") {
                            ferrython_core::error::request_vm_call(read_fn, vec![]);
                        }
                        Ok(PyObject::none())
                    }),
                );
            }
            Ok(inst)
        })
    };

    let pickling_error = PyObject::class(
        CompactString::from("PicklingError"),
        vec![],
        IndexMap::new(),
    );
    let unpickling_error = PyObject::class(
        CompactString::from("UnpicklingError"),
        vec![],
        IndexMap::new(),
    );

    make_module(
        "pickle",
        vec![
            ("dumps", make_builtin(pickle_dumps)),
            ("loads", make_builtin(pickle_loads)),
            ("dump", make_builtin(pickle_dump)),
            ("load", make_builtin(pickle_load)),
            ("_dumps", make_builtin(pickle_dumps)),
            ("_loads", make_builtin(pickle_loads)),
            ("_dump", make_builtin(pickle_dump)),
            ("Pickler", pickler_cls),
            ("Unpickler", unpickler_cls),
            ("HIGHEST_PROTOCOL", PyObject::int(5)),
            ("DEFAULT_PROTOCOL", PyObject::int(4)),
            ("PicklingError", pickling_error),
            ("UnpicklingError", unpickling_error),
            (
                "PickleError",
                PyObject::class(CompactString::from("PickleError"), vec![], IndexMap::new()),
            ),
            (
                "bytes_types",
                PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from("bytes")),
                    PyObject::str_val(CompactString::from("bytearray")),
                ]),
            ),
        ],
    )
}

fn pickle_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.dumps() missing 1 required positional argument: 'obj'",
        ));
    }
    // Extract protocol from positional arg 1, or from a trailing kwargs dict (protocol=N)
    let mut protocol: i64 = 0;
    if let Some(a) = args.get(1) {
        if let Some(n) = a.as_int() {
            protocol = n;
        } else if let PyObjectPayload::Dict(m) = &a.payload {
            let r = m.read();
            for (k, v) in r.iter() {
                if let HashableKey::Str(s) = k {
                    if s.as_str() == "protocol" {
                        if let Some(n) = v.as_int() {
                            protocol = n;
                        }
                    }
                }
            }
        }
    }
    // Also check a last-position kwargs dict (e.g., args[2] when args[1] is obj's second positional)
    if let Some(a) = args.last() {
        if args.len() > 1 {
            if let PyObjectPayload::Dict(m) = &a.payload {
                let r = m.read();
                for (k, v) in r.iter() {
                    if let HashableKey::Str(s) = k {
                        if s.as_str() == "protocol" {
                            if let Some(n) = v.as_int() {
                                protocol = n;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut buf = Vec::new();
    let mut memo = PickleWriteMemo::default();
    let serialized = if protocol >= 2 {
        buf.extend_from_slice(b"\x80\x02");
        pickle_serialize_p2(&args[0], &mut buf, &mut memo)
    } else {
        pickle_serialize_p0(&args[0], &mut buf, &mut memo)
    };
    if let Err(err) = serialized {
        if err.message.starts_with("PicklingError:") {
            return Err(PyException::type_error(err.message));
        }
        return Err(err);
    }
    buf.push(b'.');
    Ok(PyObject::bytes(buf))
}

fn pickle_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.loads() missing 1 required positional argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;
    pickle_loads_stack(&data)
}

fn pickle_dump(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "pickle.dump() missing required arguments: 'obj' and 'file'",
        ));
    }
    let protocol = args.get(2).and_then(|a| a.as_int()).unwrap_or(0);
    let data = pickle_dumps(&[args[0].clone(), PyObject::int(protocol)])?;
    let data_bytes = extract_bytes(&data)?;

    // Try file path first (via .name attribute)
    if let Some(name) = args[1].get_attr("name") {
        let path = name.py_to_string();
        if !path.is_empty() {
            std::fs::write(&path, &data_bytes)
                .map_err(|e| PyException::runtime_error(format!("pickle.dump: {}", e)))?;
            return Ok(PyObject::none());
        }
    }
    // Try file-like object with write method (BytesIO, etc.)
    if let Some(write_method) = args[1].get_attr("write") {
        match &write_method.payload {
            PyObjectPayload::NativeFunction(nf) => {
                let _ = (nf.func)(&[PyObject::bytes(data_bytes.clone())]);
                return Ok(PyObject::none());
            }
            PyObjectPayload::NativeClosure(nc) => {
                let _ = (nc.func)(&[PyObject::bytes(data_bytes.clone())]);
                return Ok(PyObject::none());
            }
            _ => {}
        }
    }
    if let PyObjectPayload::Str(path) = &args[1].payload {
        std::fs::write(path.as_str(), &data_bytes)
            .map_err(|e| PyException::runtime_error(format!("pickle.dump: {}", e)))?;
    }
    Ok(PyObject::none())
}

fn pickle_load(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.load() missing 1 required positional argument: 'file'",
        ));
    }
    // Try file path first (via .name attribute)
    if let Some(name) = args[0].get_attr("name") {
        let path = name.py_to_string();
        if !path.is_empty() && std::path::Path::new(&path).exists() {
            let data = std::fs::read(&path)
                .map_err(|e| PyException::runtime_error(format!("pickle.load: {}", e)))?;
            return pickle_loads_stack(&data);
        }
    }
    // Try file-like object with read method (BytesIO, etc.)
    if let Some(read_method) = args[0].get_attr("read") {
        let read_result = match &read_method.payload {
            PyObjectPayload::NativeFunction(nf) => (nf.func)(&[]).ok(),
            PyObjectPayload::NativeClosure(nc) => (nc.func)(&[]).ok(),
            _ => None,
        };
        if let Some(data_obj) = read_result {
            let data = extract_bytes(&data_obj)?;
            if !data.is_empty() {
                return pickle_loads_stack(&data);
            }
        }
    }
    if let PyObjectPayload::Str(path) = &args[0].payload {
        let data = std::fs::read(path.as_str())
            .map_err(|e| PyException::runtime_error(format!("pickle.load: {}", e)))?;
        return pickle_loads_stack(&data);
    }
    Err(PyException::runtime_error(
        "pickle.load: expected a file path or file-like object",
    ))
}
