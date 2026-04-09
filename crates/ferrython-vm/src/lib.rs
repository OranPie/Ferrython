//! Ferrython VM — stack-based bytecode virtual machine.

pub mod builtins;
pub mod frame;
pub(crate) mod opcodes;
pub mod vm;
mod vm_call;
mod vm_class;
pub(crate) mod vm_dataclass_utils;
mod vm_helpers;
mod vm_import;

pub use frame::Frame;
pub use vm::VirtualMachine;
