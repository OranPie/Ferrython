//! Ferrython VM — stack-based bytecode virtual machine.

pub mod builtins;
pub mod frame;
pub mod vm;

pub use frame::Frame;
pub use vm::VirtualMachine;
