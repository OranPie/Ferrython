use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};
use indexmap::IndexMap;

/// xmlrpc module — minimal stub for client/server XML-RPC
pub fn create_xmlrpc_module() -> PyObjectRef {
    let server_proxy = PyObject::native_closure("ServerProxy", move |args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "ServerProxy requires a URL argument",
            ));
        }
        let url = args[0].py_to_string();
        let cls = PyObject::class(CompactString::from("ServerProxy"), vec![], IndexMap::new());
        let mut iattrs = IndexMap::new();
        iattrs.insert(
            CompactString::from("_url"),
            PyObject::str_val(CompactString::from(url.as_str())),
        );
        Ok(PyObject::instance_with_attrs(cls, iattrs))
    });
    make_module(
        "xmlrpc",
        vec![
            ("client", {
                make_module(
                    "xmlrpc.client",
                    vec![
                        ("ServerProxy", server_proxy),
                        (
                            "Fault",
                            make_builtin(|args: &[PyObjectRef]| {
                                let msg = if !args.is_empty() {
                                    args[0].py_to_string()
                                } else {
                                    "XML-RPC Fault".to_string()
                                };
                                Err(PyException::runtime_error(msg))
                            }),
                        ),
                        (
                            "ProtocolError",
                            make_builtin(|args: &[PyObjectRef]| {
                                let msg = if !args.is_empty() {
                                    args[0].py_to_string()
                                } else {
                                    "Protocol Error".to_string()
                                };
                                Err(PyException::runtime_error(msg))
                            }),
                        ),
                    ],
                )
            }),
            ("server", make_module("xmlrpc.server", vec![])),
        ],
    )
}
