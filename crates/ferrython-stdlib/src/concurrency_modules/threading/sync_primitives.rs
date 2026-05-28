use super::*;

mod condition_timer;
mod events;
mod locks;
mod semaphores;

pub(super) fn create_sync_primitives() -> (
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
    PyObjectRef,
) {
    let (lock_fn, rlock_fn) = locks::create_lock_primitives();
    let (semaphore_fn, bounded_semaphore_fn) = semaphores::create_semaphore_primitives();
    let (event_fn, barrier_fn) = events::create_event_primitives();
    let (condition_fn, timer_fn) = condition_timer::create_condition_timer_primitives();

    (
        lock_fn,
        rlock_fn,
        event_fn,
        semaphore_fn,
        bounded_semaphore_fn,
        condition_fn,
        barrier_fn,
        timer_fn,
    )
}
