use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};

mod module_fns;
mod sync_primitives;
mod thread_class;

pub fn create_threading_module() -> PyObjectRef {
    let thread_class = thread_class::create_thread_class();
    let (
        lock_fn,
        rlock_fn,
        event_fn,
        semaphore_fn,
        bounded_semaphore_fn,
        condition_fn,
        barrier_fn,
        timer_fn,
    ) = sync_primitives::create_sync_primitives();
    let (
        current_thread_fn,
        active_count_fn,
        enumerate_fn,
        main_thread_fn,
        local_fn,
        get_ident_fn,
        get_native_id_fn,
        stack_size_fn,
    ) = module_fns::create_module_functions();

    make_module(
        "threading",
        vec![
            ("Thread", thread_class),
            ("Lock", lock_fn),
            ("RLock", rlock_fn),
            ("Event", event_fn),
            ("Semaphore", semaphore_fn.clone()),
            ("BoundedSemaphore", bounded_semaphore_fn),
            ("Condition", condition_fn),
            ("Barrier", barrier_fn),
            ("Timer", timer_fn),
            ("current_thread", current_thread_fn),
            ("active_count", active_count_fn),
            ("enumerate", enumerate_fn),
            ("main_thread", main_thread_fn),
            ("local", local_fn),
            ("get_ident", get_ident_fn),
            ("get_native_id", get_native_id_fn),
            ("stack_size", stack_size_fn),
            ("settrace", make_builtin(|_| Ok(PyObject::none()))),
            ("setprofile", make_builtin(|_| Ok(PyObject::none()))),
            ("excepthook", make_builtin(|_| Ok(PyObject::none()))),
            ("TIMEOUT_MAX", PyObject::float(f64::MAX)),
        ],
    )
}
