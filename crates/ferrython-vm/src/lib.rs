//! Ferrython VM — stack-based bytecode virtual machine.

pub mod builtins;
pub mod frame;
pub(crate) mod opcodes;
#[macro_use]
mod vm_dispatch;
pub mod vm;
mod vm_call;
mod vm_class;
pub(crate) mod vm_dataclass_utils;
mod vm_entry;
mod vm_exception;
pub(crate) mod vm_exec_compile;
mod vm_execute_one;
mod vm_fast_binary;
mod vm_fast_paths;
mod vm_helpers;
mod vm_import;
mod vm_init;
mod vm_iter_fast;
mod vm_itertools_bridge;
mod vm_method_cache;
mod vm_rawio;
mod vm_regex_bridge;
mod vm_trace;
pub(crate) mod vm_truth;

pub use frame::Frame;
pub use vm::VirtualMachine;
