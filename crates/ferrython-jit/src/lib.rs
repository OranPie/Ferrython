//! Ferrython JIT compiler — Cranelift-based tiered compilation.
//!
//! # Current Status
//!
//! Defines the API surface for future JIT compilation. The interpreter currently
//! runs all code through the bytecode VM. This module will eventually provide
//! hot-loop detection and Cranelift-based native code generation.
//!
//! # Design
//!
//! - Monitor execution counts per CodeObject
//! - When a function exceeds the JIT threshold, compile to native code
//! - Native code replaces the bytecode loop for that function

use ferrython_bytecode::code::CodeObject;

/// Check whether a code object should be JIT-compiled based on execution count.
///
/// Currently always returns `false` — JIT is not yet implemented.
pub fn should_jit(_code: &CodeObject, _exec_count: u64) -> bool {
    false
}

/// JIT compilation threshold (number of executions before compiling).
pub const JIT_THRESHOLD: u64 = 1000;

/// Placeholder for compiled native code.
pub struct CompiledCode {
    _private: (),
}

/// Attempt to JIT-compile a code object. Returns `None` until JIT is implemented.
pub fn compile(_code: &CodeObject) -> Option<CompiledCode> {
    None
}