//! Ferrython bytecode — Python 3.8 opcode definitions and code objects.

pub mod code;
pub mod opcode;

pub use code::{CodeFlags, CodeObject, ConstantValue};
pub use opcode::{Instruction, Opcode};
