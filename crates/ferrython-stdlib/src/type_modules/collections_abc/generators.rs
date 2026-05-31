use super::helpers::{add_method, drop_abstract};
use super::*;

fn raise_from_arg(args: &[PyObjectRef], offset: usize) -> PyException {
    let typ = args
        .get(offset)
        .cloned()
        .unwrap_or_else(|| PyObject::exception_type(ExceptionKind::ValueError));
    let value = args.get(offset + 1);
    let kind = match &typ.payload {
        PyObjectPayload::ExceptionType(kind) => *kind,
        PyObjectPayload::Class(_) => ExceptionKind::ValueError,
        PyObjectPayload::Instance(inst) => {
            if let Some(kind) = inst.class.get_attr("__builtin_exception_kind__") {
                ExceptionKind::from_name(&kind.py_to_string()).unwrap_or(ExceptionKind::Exception)
            } else {
                ExceptionKind::Exception
            }
        }
        PyObjectPayload::ExceptionInstance(ei) => ei.kind,
        _ => ExceptionKind::ValueError,
    };
    let message = value
        .map(|obj| obj.py_to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| typ.py_to_string());
    PyException::new(kind, message)
}

pub(super) fn add_generator_methods(
    generator_cls: &PyObjectRef,
    coroutine_cls: &PyObjectRef,
    async_iterator_cls: &PyObjectRef,
    async_generator_cls: &PyObjectRef,
) {
    add_method(
        &generator_cls,
        "__iter__",
        PyObject::native_closure("Generator.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Generator.__iter__ requires self"));
            }
            Ok(args[0].clone())
        }),
    );
    drop_abstract(&generator_cls, &["__iter__", "__next__", "close", "send"]);
    add_method(
        &generator_cls,
        "send",
        PyObject::native_closure("Generator.send", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("send() requires value"));
            }
            if matches!(&args[1].payload, PyObjectPayload::None) {
                return Err(PyException::new(
                    ExceptionKind::StopIteration,
                    String::new(),
                ));
            }
            Ok(args[1].clone())
        }),
    );
    add_method(
        &generator_cls,
        "throw",
        PyObject::native_closure("Generator.throw", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("throw() requires an exception"));
            }
            Err(raise_from_arg(args, 1))
        }),
    );
    add_method(
        &generator_cls,
        "__next__",
        PyObject::native_closure("Generator.__next__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Generator.__next__ requires self"));
            }
            let send = args[0]
                .get_attr("send")
                .ok_or_else(|| PyException::attribute_error("send"))?;
            ferrython_core::object::helpers::call_callable(&send, &[PyObject::none()])
        }),
    );
    add_method(
        &generator_cls,
        "close",
        PyObject::native_closure("Generator.close", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Generator.close requires self"));
            }
            let throw = args[0]
                .get_attr("throw")
                .ok_or_else(|| PyException::attribute_error("throw"))?;
            let gen_exit = PyObject::exception_type(ExceptionKind::GeneratorExit);
            match ferrython_core::object::helpers::call_callable(&throw, &[gen_exit]) {
                Ok(_) => Err(PyException::runtime_error(
                    "generator ignored GeneratorExit",
                )),
                Err(err)
                    if matches!(
                        err.kind,
                        ExceptionKind::GeneratorExit | ExceptionKind::StopIteration
                    ) =>
                {
                    Ok(PyObject::none())
                }
                Err(err) => Err(err),
            }
        }),
    );
    drop_abstract(&coroutine_cls, &["close"]);
    add_method(
        &coroutine_cls,
        "throw",
        PyObject::native_closure("Coroutine.throw", move |_args: &[PyObjectRef]| {
            Err(PyException::new(
                ExceptionKind::StopIteration,
                String::new(),
            ))
        }),
    );
    add_method(
        &coroutine_cls,
        "close",
        PyObject::native_closure("Coroutine.close", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Coroutine.close requires self"));
            }
            let throw = args[0]
                .get_attr("throw")
                .ok_or_else(|| PyException::attribute_error("throw"))?;
            let gen_exit = PyObject::exception_type(ExceptionKind::GeneratorExit);
            match ferrython_core::object::helpers::call_callable(&throw, &[gen_exit]) {
                Ok(_) => Err(PyException::runtime_error(
                    "coroutine ignored GeneratorExit",
                )),
                Err(err)
                    if matches!(
                        err.kind,
                        ExceptionKind::GeneratorExit | ExceptionKind::StopIteration
                    ) =>
                {
                    Ok(PyObject::none())
                }
                Err(err) => Err(err),
            }
        }),
    );
    add_method(
        &async_iterator_cls,
        "__aiter__",
        PyObject::native_closure("AsyncIterator.__aiter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "AsyncIterator.__aiter__ requires self",
                ));
            }
            Ok(args[0].clone())
        }),
    );
    add_method(
        &async_generator_cls,
        "__aiter__",
        PyObject::native_closure("AsyncGenerator.__aiter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "AsyncGenerator.__aiter__ requires self",
                ));
            }
            Ok(args[0].clone())
        }),
    );
    drop_abstract(&async_generator_cls, &["__aiter__", "__anext__", "aclose"]);
    add_method(
        &async_generator_cls,
        "__anext__",
        PyObject::native_closure("AsyncGenerator.__anext__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "AsyncGenerator.__anext__ requires self",
                ));
            }
            let asend = args[0]
                .get_attr("asend")
                .ok_or_else(|| PyException::attribute_error("asend"))?;
            ferrython_core::object::helpers::call_callable(&asend, &[PyObject::none()])
        }),
    );
    add_method(
        &async_generator_cls,
        "asend",
        PyObject::native_closure("AsyncGenerator.asend", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("asend() requires value"));
            }
            Ok(PyObject::builtin_awaitable(args[1].clone()))
        }),
    );
    add_method(
        &async_generator_cls,
        "athrow",
        PyObject::native_closure("AsyncGenerator.athrow", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("athrow() requires an exception"));
            }
            Err(raise_from_arg(args, 1))
        }),
    );
    add_method(
        &async_generator_cls,
        "aclose",
        PyObject::native_closure("AsyncGenerator.aclose", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "AsyncGenerator.aclose requires self",
                ));
            }
            let athrow = args[0]
                .get_attr("athrow")
                .ok_or_else(|| PyException::attribute_error("athrow"))?;
            let gen_exit = PyObject::exception_type(ExceptionKind::GeneratorExit);
            match ferrython_core::object::helpers::call_callable(&athrow, &[gen_exit]) {
                Ok(_) => Err(PyException::runtime_error(
                    "async generator ignored GeneratorExit",
                )),
                Err(err)
                    if matches!(
                        err.kind,
                        ExceptionKind::GeneratorExit | ExceptionKind::StopIteration
                    ) =>
                {
                    Ok(PyObject::builtin_awaitable(PyObject::none()))
                }
                Err(err) => Err(err),
            }
        }),
    );
}
