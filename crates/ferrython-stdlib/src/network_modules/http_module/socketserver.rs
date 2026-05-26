use compact_str::CompactString;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;

/// socketserver — server framework (stub for werkzeug/flask)
pub fn create_socketserver_module() -> PyObjectRef {
    // BaseServer class
    let base_server = PyObject::class(CompactString::from("BaseServer"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = base_server.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("__init__"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 3 {
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs
                            .write()
                            .insert(CompactString::from("server_address"), args[1].clone());
                        inst.attrs
                            .write()
                            .insert(CompactString::from("RequestHandlerClass"), args[2].clone());
                    }
                }
                Ok(PyObject::none())
            }),
        );
        ns.insert(
            CompactString::from("serve_forever"),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("shutdown"),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("server_close"),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
        );
    }

    let tcp_server = PyObject::class(
        CompactString::from("TCPServer"),
        vec![base_server.clone()],
        IndexMap::new(),
    );
    let udp_server = PyObject::class(
        CompactString::from("UDPServer"),
        vec![base_server.clone()],
        IndexMap::new(),
    );
    let threading_tcp = PyObject::class(
        CompactString::from("ThreadingTCPServer"),
        vec![tcp_server.clone()],
        IndexMap::new(),
    );
    let threading_udp = PyObject::class(
        CompactString::from("ThreadingUDPServer"),
        vec![udp_server.clone()],
        IndexMap::new(),
    );
    let forking_tcp = PyObject::class(
        CompactString::from("ForkingTCPServer"),
        vec![tcp_server.clone()],
        IndexMap::new(),
    );
    let forking_udp = PyObject::class(
        CompactString::from("ForkingUDPServer"),
        vec![udp_server.clone()],
        IndexMap::new(),
    );

    // BaseRequestHandler
    let base_handler = PyObject::class(
        CompactString::from("BaseRequestHandler"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = base_handler.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("setup"),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("handle"),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("finish"),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
        );
    }
    let stream_handler = PyObject::class(
        CompactString::from("StreamRequestHandler"),
        vec![base_handler.clone()],
        IndexMap::new(),
    );
    let datagram_handler = PyObject::class(
        CompactString::from("DatagramRequestHandler"),
        vec![base_handler.clone()],
        IndexMap::new(),
    );

    // ThreadingMixIn
    let threading_mixin = PyObject::class(
        CompactString::from("ThreadingMixIn"),
        vec![],
        IndexMap::new(),
    );
    let forking_mixin =
        PyObject::class(CompactString::from("ForkingMixIn"), vec![], IndexMap::new());

    make_module(
        "socketserver",
        vec![
            ("BaseServer", base_server),
            ("TCPServer", tcp_server),
            ("UDPServer", udp_server),
            ("ThreadingTCPServer", threading_tcp),
            ("ThreadingUDPServer", threading_udp),
            ("ForkingTCPServer", forking_tcp),
            ("ForkingUDPServer", forking_udp),
            ("BaseRequestHandler", base_handler),
            ("StreamRequestHandler", stream_handler),
            ("DatagramRequestHandler", datagram_handler),
            ("ThreadingMixIn", threading_mixin),
            ("ForkingMixIn", forking_mixin),
        ],
    )
}
