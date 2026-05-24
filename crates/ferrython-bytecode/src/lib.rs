//! Ferrython bytecode — Python 3.8 opcode definitions and code objects.

pub mod code;
pub mod opcode;

pub use code::{
    get_int_max_str_digits, set_int_max_str_digits, CodeFlags, CodeObject, ConstantValue,
};
pub use opcode::{Instruction, Opcode};
