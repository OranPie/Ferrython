//! Ferrython Compiler — translates Python 3.8 AST into bytecode.
//!
//! This crate takes a parsed AST (from `ferrython-ast`) and compiles it into
//! a `CodeObject` (from `ferrython-bytecode`) that can be executed by the VM.

mod compiler;
pub mod error;
pub mod symbol_table;
mod peephole;

pub use compiler::Compiler;
pub use error::CompileError;
pub use peephole::{set_superinstructions_enabled, superinstructions_enabled};

use ferrython_ast::Module;
use ferrython_bytecode::CodeObject;

/// Compile a Python module AST into a bytecode `CodeObject`.
///
/// This is the main entry point for the compiler. It performs symbol-table
/// analysis, then walks the AST emitting bytecode instructions.
///
/// # Errors
/// Returns `CompileError` if the AST contains invalid constructs
/// (e.g., `break` outside a loop, invalid assignment targets).
pub fn compile(module: &Module, filename: &str) -> Result<CodeObject, CompileError> {
    let mut compiler = Compiler::new(filename);
    let mut code = compiler.compile_module(module)?;
    peephole::optimize(&mut code);
    Ok(code)
}

/// Compile in interactive (REPL) mode. Expression statements emit `PrintExpr`
/// and store the result in `_`.
pub fn compile_interactive(module: &Module, filename: &str) -> Result<CodeObject, CompileError> {
    let mut compiler = Compiler::new(filename);
    compiler.set_interactive(true);
    let mut code = compiler.compile_module(module)?;
    peephole::optimize(&mut code);
    Ok(code)
}
