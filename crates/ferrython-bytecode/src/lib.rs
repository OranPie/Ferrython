//! Ferrython bytecode — Python 3.8 opcode definitions and code objects.

pub mod opcode;
pub mod code;

pub use opcode::{Opcode, Instruction};
pub use code::{CodeObject, CodeFlags, ConstantValue};
