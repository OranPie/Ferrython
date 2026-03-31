//! Code object — the compiled form of a Python function or module.

use crate::opcode::Instruction;
use bitflags::bitflags;
use compact_str::CompactString;

/// A constant value stored in the code object's constant pool.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstantValue {
    None,
    Bool(bool),
    Integer(i64),
    BigInteger(Box<num_bigint::BigInt>),
    Float(f64),
    Complex { real: f64, imag: f64 },
    Str(CompactString),
    Bytes(Vec<u8>),
    Ellipsis,
    /// A nested code object (for nested functions, classes, comprehensions).
    Code(Box<CodeObject>),
    /// A tuple of constants (used for default args, annotations, etc.).
    Tuple(Vec<ConstantValue>),
    /// frozenset of constants
    FrozenSet(Vec<ConstantValue>),
}

bitflags! {
    /// Code object flags (matches CPython's co_flags).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CodeFlags: u32 {
        const OPTIMIZED          = 0x0001;
        const NEWLOCALS          = 0x0002;
        const VARARGS            = 0x0004;
        const VARKEYWORDS        = 0x0008;
        const NESTED             = 0x0010;
        const GENERATOR          = 0x0020;
        const NOFREE             = 0x0040;
        const COROUTINE          = 0x0100;
        const ITERABLE_COROUTINE = 0x0200;
        const ASYNC_GENERATOR    = 0x0400;
    }
}

/// Compiled Python code object (equivalent to CPython's `PyCodeObject`).
#[derive(Debug, Clone, PartialEq)]
pub struct CodeObject {
    /// Bytecode instructions.
    pub instructions: Vec<Instruction>,
    /// Constant pool.
    pub constants: Vec<ConstantValue>,
    /// Names used by LOAD_NAME, STORE_NAME, etc.
    pub names: Vec<CompactString>,
    /// Local variable names (fast locals).
    pub varnames: Vec<CompactString>,
    /// Free variable names (from enclosing scopes).
    pub freevars: Vec<CompactString>,
    /// Cell variable names (referenced by nested scopes).
    pub cellvars: Vec<CompactString>,
    /// Source file name.
    pub filename: CompactString,
    /// Function/module name.
    pub name: CompactString,
    /// Qualified name (e.g., `Outer.Inner.method`).
    pub qualname: CompactString,
    /// First line number in source.
    pub first_line_number: u32,
    /// Line number table (instruction index → source line).
    pub line_number_table: Vec<(u32, u32)>,
    /// Code flags.
    pub flags: CodeFlags,
    /// Number of positional arguments (not including * or ** args).
    pub arg_count: u32,
    /// Number of positional-only arguments (Python 3.8: before `/`).
    pub posonlyarg_count: u32,
    /// Number of keyword-only arguments.
    pub kwonlyarg_count: u32,
    /// Number of local variables.
    pub num_locals: u32,
    /// Maximum stack depth.
    pub max_stack_size: u32,
}

impl CodeObject {
    /// Create a new empty code object.
    pub fn new(name: impl Into<CompactString>, filename: impl Into<CompactString>) -> Self {
        let name = name.into();
        Self {
            instructions: Vec::new(),
            constants: Vec::new(),
            names: Vec::new(),
            varnames: Vec::new(),
            freevars: Vec::new(),
            cellvars: Vec::new(),
            filename: filename.into(),
            qualname: name.clone(),
            name,
            first_line_number: 1,
            line_number_table: Vec::new(),
            flags: CodeFlags::OPTIMIZED | CodeFlags::NEWLOCALS,
            arg_count: 0,
            posonlyarg_count: 0,
            kwonlyarg_count: 0,
            num_locals: 0,
            max_stack_size: 0,
        }
    }

    /// Add a constant and return its index.
    pub fn add_const(&mut self, value: ConstantValue) -> u32 {
        // Check if constant already exists
        for (i, c) in self.constants.iter().enumerate() {
            if c == &value {
                return i as u32;
            }
        }
        let idx = self.constants.len() as u32;
        self.constants.push(value);
        idx
    }

    /// Add a name and return its index.
    pub fn add_name(&mut self, name: impl Into<CompactString>) -> u32 {
        let name = name.into();
        for (i, n) in self.names.iter().enumerate() {
            if n == &name {
                return i as u32;
            }
        }
        let idx = self.names.len() as u32;
        self.names.push(name);
        idx
    }

    /// Add an instruction and return its index.
    pub fn emit(&mut self, instruction: Instruction) -> u32 {
        let idx = self.instructions.len() as u32;
        self.instructions.push(instruction);
        idx
    }

    /// Current instruction offset.
    pub fn current_offset(&self) -> u32 {
        self.instructions.len() as u32
    }
}
