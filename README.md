# Ferrython 🐍⚙️

A high-performance Python 3.8 interpreter written in Rust.

## Features

- **Full Python 3.8 compatibility** — targets CPython 3.8 language specification
- **Bytecode VM** — stack-based virtual machine with CPython-compatible opcodes
- **JIT compilation** — Cranelift-based JIT for hot code paths
- **Hybrid GC** — reference counting + cycle-detecting tracing collector
- **Full standard library** — best-effort reimplementation of all stdlib modules
- **Interactive REPL** — readline, history, tab completion, syntax highlighting
- **Developer tools** — pdb-compatible debugger, profiler, coverage
- **Cross-platform** — Linux, macOS, Windows

## Building

```bash
cargo build --release
```

## Running

```bash
# Interactive REPL
cargo run --release --bin ferrython

# Run a script
cargo run --release --bin ferrython -- script.py

# Run a command
cargo run --release --bin ferrython -- -c "print('Hello, World!')"
```

## Project Structure

Ferrython is organized as a Cargo workspace with 14 crates:

| Crate | Purpose |
|---|---|
| `ferrython-ast` | AST node definitions & visitor traits |
| `ferrython-parser` | Lexer + parser → AST |
| `ferrython-compiler` | AST → bytecode compiler |
| `ferrython-bytecode` | Bytecode definitions & serialization |
| `ferrython-vm` | Stack-based bytecode virtual machine |
| `ferrython-core` | Core runtime: object model, memory, GIL |
| `ferrython-gc` | Hybrid garbage collector |
| `ferrython-jit` | Cranelift JIT compiler |
| `ferrython-ffi` | Rust-native FFI framework |
| `ferrython-import` | Import system & module loader |
| `ferrython-repl` | Interactive REPL |
| `ferrython-debug` | Debugger, profiler, coverage |
| `ferrython-stdlib` | Standard library modules |
| `ferrython-cli` | CLI entry point |

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.
