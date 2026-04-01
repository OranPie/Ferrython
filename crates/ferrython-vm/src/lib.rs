//! Ferrython VM — stack-based bytecode virtual machine.

pub mod builtins;
pub mod frame;
mod opcodes;
pub mod vm;

pub use frame::Frame;
pub use vm::VirtualMachine;
