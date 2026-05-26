use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

// ── pydoc module ──

pub fn create_pydoc_module() -> PyObjectRef {
    fn pydoc_help(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            println!("Welcome to Ferrython help utility!");
            println!("Type help(object) to get help on an object.");
            return Ok(PyObject::none());
        }
        let obj = &args[0];
        match &obj.payload {
            PyObjectPayload::Str(s) => {
                println!("Help on topic '{}':", s);
                println!("  (No detailed help available)");
            }
            PyObjectPayload::BuiltinType(name) => {
                println!("Help on class {}:", name);
                println!("  Built-in type '{}'", name);
            }
            PyObjectPayload::Function(f) => {
                println!("Help on function {}:", f.name);
                if let Some(doc) = obj.get_attr("__doc__") {
                    if let PyObjectPayload::Str(s) = &doc.payload {
                        println!("  {}", s);
                    }
                }
            }
            PyObjectPayload::Class(cd) => {
                println!("Help on class {}:", cd.name);
                let ns = cd.namespace.read();
                if let Some(doc) = ns.get("__doc__") {
                    if let PyObjectPayload::Str(s) = &doc.payload {
                        println!("  {}", s);
                    }
                }
                println!("\n  Methods:");
                for (name, _) in ns.iter() {
                    if !name.starts_with('_') {
                        println!("    {}", name);
                    }
                }
            }
            PyObjectPayload::Module(entries) => {
                println!("Help on module:");
                let rd = entries.attrs.read();
                if let Some(doc) = rd.get("__doc__") {
                    if let PyObjectPayload::Str(s) = &doc.payload {
                        println!("  {}", s);
                    }
                }
                println!("\n  Contents:");
                for (name, _) in rd.iter() {
                    if !name.starts_with('_') {
                        println!("    {}", name);
                    }
                }
            }
            _ => {
                println!("Help on {} object:", obj.type_name());
                println!("  Type: {}", obj.type_name());
            }
        }
        Ok(PyObject::none())
    }

    fn render_doc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Ok(PyObject::str_val(CompactString::from("")));
        }
        let obj = &args[0];
        let type_name = obj.type_name();
        Ok(PyObject::str_val(CompactString::from(format!(
            "Help on {} object",
            type_name
        ))))
    }

    fn getdoc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Ok(PyObject::none());
        }
        if let Some(doc) = args[0].get_attr("__doc__") {
            if !matches!(&doc.payload, PyObjectPayload::None) {
                return Ok(doc);
            }
        }
        Ok(PyObject::none())
    }

    make_module(
        "pydoc",
        vec![
            ("help", make_builtin(pydoc_help)),
            ("render_doc", make_builtin(render_doc)),
            ("getdoc", make_builtin(getdoc)),
            (
                "describe",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::str_val(CompactString::from("")));
                    }
                    let obj = &args[0];
                    let name = obj
                        .get_attr("__name__")
                        .map(|n| n.py_to_string())
                        .unwrap_or_else(|| obj.type_name().to_string());
                    let desc = match &obj.payload {
                        PyObjectPayload::Module(_) => format!("module {}", name),
                        PyObjectPayload::Class(_) => format!("class {}", name),
                        PyObjectPayload::Function(_)
                        | PyObjectPayload::NativeFunction(_)
                        | PyObjectPayload::NativeClosure(_) => format!("function {}", name),
                        PyObjectPayload::BoundMethod { .. } => format!("method {}", name),
                        _ => obj.type_name().to_string(),
                    };
                    Ok(PyObject::str_val(CompactString::from(desc)))
                }),
            ),
            ("Helper", make_builtin(pydoc_help)),
        ],
    )
}
