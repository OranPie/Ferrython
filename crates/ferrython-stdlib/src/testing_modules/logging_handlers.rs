use compact_str::CompactString;
use ferrython_core::object::{make_module, PyObject, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;

// ─── logging.handlers submodule ─────────────────────────────────────────────

pub fn create_logging_handlers_module() -> PyObjectRef {
    let make_handler_class = |name: &str| -> PyObjectRef {
        let class_name = CompactString::from(name);
        let cn = class_name.clone();
        let cls = PyObject::class(class_name, vec![], IndexMap::new());
        let _cls_ret = cls.clone();
        let factory = PyObject::native_closure(name, move |args: &[PyObjectRef]| {
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("level"), PyObject::int(0));
                let cn2 = cn.clone();
                attrs.insert(
                    CompactString::from("setLevel"),
                    PyObject::native_function("setLevel", |_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("setFormatter"),
                    PyObject::native_function("setFormatter", |_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("emit"),
                    PyObject::native_function("emit", |_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("close"),
                    PyObject::native_function("close", |_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("flush"),
                    PyObject::native_function("flush", |_| Ok(PyObject::none())),
                );
                // Store constructor args
                for (i, arg) in args.iter().enumerate() {
                    attrs.insert(CompactString::from(format!("_arg{}", i)), arg.clone());
                }
                let _ = cn2;
            }
            Ok(inst)
        });
        factory
    };

    make_module(
        "logging.handlers",
        vec![
            (
                "RotatingFileHandler",
                make_handler_class("RotatingFileHandler"),
            ),
            (
                "TimedRotatingFileHandler",
                make_handler_class("TimedRotatingFileHandler"),
            ),
            ("SocketHandler", make_handler_class("SocketHandler")),
            ("DatagramHandler", make_handler_class("DatagramHandler")),
            ("SysLogHandler", make_handler_class("SysLogHandler")),
            ("NTEventLogHandler", make_handler_class("NTEventLogHandler")),
            ("SMTPHandler", make_handler_class("SMTPHandler")),
            ("MemoryHandler", make_handler_class("MemoryHandler")),
            ("HTTPHandler", make_handler_class("HTTPHandler")),
            ("QueueHandler", make_handler_class("QueueHandler")),
            ("QueueListener", make_handler_class("QueueListener")),
            (
                "WatchedFileHandler",
                make_handler_class("WatchedFileHandler"),
            ),
            ("BufferingHandler", make_handler_class("BufferingHandler")),
            (
                "BaseRotatingHandler",
                make_handler_class("BaseRotatingHandler"),
            ),
        ],
    )
}
