//! Ferrython VM — stack-based bytecode virtual machine.

pub mod builtins;
pub mod frame;
pub(crate) mod opcodes;
pub mod vm;
mod vm_call;
mod vm_class;
mod vm_helpers;

pub use frame::Frame;
pub use vm::VirtualMachine;
