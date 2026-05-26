use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    call_callable, call_callable_kw, make_module, PyObject, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

#[derive(Clone)]
struct AtexitCallback {
    func: PyObjectRef,
    args: Vec<PyObjectRef>,
    kwargs: Vec<(CompactString, PyObjectRef)>,
}

thread_local! {
    static ATEXIT_CALLBACKS: std::cell::RefCell<Vec<AtexitCallback>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub fn register_atexit_callback(
    func: PyObjectRef,
    args: Vec<PyObjectRef>,
    kwargs: Vec<(CompactString, PyObjectRef)>,
) {
    ATEXIT_CALLBACKS.with(|callbacks| {
        callbacks
            .borrow_mut()
            .push(AtexitCallback { func, args, kwargs });
    });
}

pub fn unregister_atexit_callback(func: &PyObjectRef) {
    ATEXIT_CALLBACKS.with(|callbacks| {
        callbacks
            .borrow_mut()
            .retain(|callback| !PyObjectRef::ptr_eq(&callback.func, func));
    });
}

// ── atexit module ──

pub fn create_atexit_module() -> PyObjectRef {
    fn split_atexit_args(
        args: &[PyObjectRef],
    ) -> PyResult<(
        PyObjectRef,
        Vec<PyObjectRef>,
        Vec<(CompactString, PyObjectRef)>,
    )> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "atexit.register requires a callable",
            ));
        }
        let mut pos_args = args[1..].to_vec();
        let mut kwargs = Vec::new();
        if let Some(last) = pos_args.last() {
            if let PyObjectPayload::Dict(map) = &last.payload {
                let read = map.read();
                let mut is_kwargs = false;
                for key in read.keys() {
                    if matches!(key, HashableKey::Str(s) if s.as_str() == "__atexit_kwargs__") {
                        is_kwargs = true;
                        break;
                    }
                }
                if is_kwargs {
                    let dict = pos_args.pop().unwrap();
                    if let PyObjectPayload::Dict(map) = &dict.payload {
                        for (key, value) in map.read().iter() {
                            if let HashableKey::Str(name) = key {
                                if name.as_str() != "__atexit_kwargs__" {
                                    kwargs.push((name.to_compact_string(), value.clone()));
                                }
                            } else {
                                return Err(PyException::type_error("keywords must be strings"));
                            }
                        }
                    }
                }
            }
        }
        Ok((args[0].clone(), pos_args, kwargs))
    }

    let register_fn = PyObject::native_closure("atexit.register", move |args: &[PyObjectRef]| {
        let (func, call_args, kwargs) = split_atexit_args(args)?;
        register_atexit_callback(func, call_args, kwargs);
        Ok(args[0].clone())
    });
    let unregister_fn =
        PyObject::native_closure("atexit.unregister", move |args: &[PyObjectRef]| {
            if let Some(func) = args.first() {
                unregister_atexit_callback(func);
            }
            Ok(PyObject::none())
        });
    let run_exitfuncs = PyObject::native_closure("atexit._run_exitfuncs", move |_| {
        loop {
            let callback = ATEXIT_CALLBACKS.with(|callbacks| callbacks.borrow_mut().pop());
            let Some(callback) = callback else {
                break;
            };
            let result = if callback.kwargs.is_empty() {
                call_callable(&callback.func, &callback.args)
            } else {
                call_callable_kw(&callback.func, &callback.args, callback.kwargs)
            };
            if let Err(err) = result {
                eprintln!("Exception ignored in atexit callback: {}", err);
            }
        }
        Ok(PyObject::none())
    });
    let clear_fn = PyObject::native_closure("atexit._clear", move |_| {
        ATEXIT_CALLBACKS.with(|callbacks| callbacks.borrow_mut().clear());
        Ok(PyObject::none())
    });
    let _ncallbacks =
        PyObject::native_closure("atexit._ncallbacks", move |_args: &[PyObjectRef]| {
            let len = ATEXIT_CALLBACKS.with(|callbacks| callbacks.borrow().len());
            Ok(PyObject::int(len as i64))
        });
    make_module(
        "atexit",
        vec![
            ("register", register_fn),
            ("unregister", unregister_fn),
            ("_run_exitfuncs", run_exitfuncs),
            ("_clear", clear_fn),
            ("_ncallbacks", _ncallbacks),
        ],
    )
}

// ── site module ──
