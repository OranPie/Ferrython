//! VM construction and shared runtime callback registration.

use crate::builtins;
use crate::frame::{FramePool, SharedBuiltins};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::object::PyObjectRef;
use ferrython_debug::{BreakpointManager, ExecutionProfiler};
use indexmap::IndexMap;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::OnceLock;

/// Shared builtins for spawning thread VMs without re-initializing.
static SHARED_BUILTINS: OnceLock<SharedBuiltins> = OnceLock::new();

/// Callback registered with ferrython-core to spawn Python functions on real OS threads.
fn spawn_python_thread_impl(
    func: PyObjectRef,
    args: Vec<PyObjectRef>,
) -> std::thread::JoinHandle<()> {
    let builtins = SHARED_BUILTINS
        .get()
        .expect("SHARED_BUILTINS not initialized")
        .clone();
    std::thread::spawn(move || {
        let mut vm = VirtualMachine::new_for_thread(builtins);
        let _ = vm.call_function_standalone(func, args);
    })
}

pub(crate) fn register_shared_vm_callbacks(builtins: &SharedBuiltins) {
    SHARED_BUILTINS.get_or_init(|| builtins.clone());
    ferrython_core::error::register_thread_spawn(spawn_python_thread_impl);
    ferrython_core::object::register_global_lookup_invalidate(crate::frame::bump_globals_version);
}

impl VirtualMachine {
    pub fn new() -> Self {
        // Initialize search paths (stdlib/Lib) BEFORE sys module is created,
        // so sys.path is populated with the correct paths on first access.
        ferrython_import::init();
        let builtins = SharedBuiltins(Rc::new(builtins::init_builtins()));
        // Register the thread spawn callback so the stdlib can spawn real OS
        // threads for Python function targets.  Uses the shared builtins.
        {
            register_shared_vm_callbacks(&builtins);
        }
        // Register generator frame drop callback (core crate can't know Frame type)
        ferrython_core::object::register_gen_frame_drop(crate::vm_generator::drop_generator_frame);
        let mut modules = IndexMap::new();
        if let Some(builtins_mod) = ferrython_stdlib::load_module("builtins") {
            modules.insert(CompactString::from("builtins"), builtins_mod);
        }
        Self {
            call_stack: Vec::with_capacity(64),
            builtins,
            modules,
            active_exception: None,
            exception_state_stack: Vec::new(),
            sys_modules_dict: None,
            profiler: ExecutionProfiler::new(),
            breakpoints: BreakpointManager::new(),
            frame_pool: FramePool::new(),
            recursion_limit: ferrython_stdlib::get_recursion_limit() as usize,
            call_object_depth: Rc::new(Cell::new(0)),
        }
    }

    /// Create a lightweight VM for use in a spawned thread.
    /// Shares the same builtins map (Arc) so builtin lookup is free.
    pub fn new_for_thread(builtins: SharedBuiltins) -> Self {
        let mut modules = IndexMap::new();
        if let Some(builtins_mod) = ferrython_stdlib::load_module("builtins") {
            modules.insert(CompactString::from("builtins"), builtins_mod);
        }
        Self {
            call_stack: Vec::with_capacity(64),
            builtins,
            modules,
            active_exception: None,
            exception_state_stack: Vec::new(),
            sys_modules_dict: None,
            profiler: ExecutionProfiler::new(),
            breakpoints: BreakpointManager::new(),
            frame_pool: FramePool::new(),
            recursion_limit: ferrython_stdlib::get_recursion_limit() as usize,
            call_object_depth: Rc::new(Cell::new(0)),
        }
    }

    /// Get a clone of the builtins Arc for passing to thread VMs.
    pub fn shared_builtins(&self) -> SharedBuiltins {
        self.builtins.clone()
    }
}
