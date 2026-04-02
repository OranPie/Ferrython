# Copilot Instructions for Ferrython

Ferrython is a Python 3.8 interpreter written in Rust, organized as a Cargo workspace of 13 crates.

## Build & Test Commands

```bash
# Build
cargo build
cargo build --release

# Run the interpreter
cargo run --release --bin ferrython -- script.py
cargo run --release --bin ferrython -- -c "print('hello')"
cargo run --release --bin ferrython          # REPL
cargo run --release --bin ferrython -- --dis script.py  # disassemble bytecode

# Tests
cargo test                          # all Rust tests
cargo test -p ferrython-parser      # single crate
cargo test test_name                # single test by name

# Run Python fixture tests manually
cargo run --release --bin ferrython -- tests/fixtures/test_phase5.py
```

Code formatting: `max_width = 100` (see `rustfmt.toml`).

## Architecture

The pipeline is: **Source → Lexer → Parser → AST → Compiler → Bytecode → VM**

```
ferrython-cli        CLI entry point, argument parsing
ferrython-repl       Interactive REPL
ferrython-parser     Lexer + recursive-descent parser → tokens/AST
ferrython-ast        AST node types, Visitor trait
ferrython-compiler   Two-pass: symbol table analysis → bytecode generation
ferrython-bytecode   CPython 3.8-compatible opcode definitions, CodeObject
ferrython-vm         Stack-based bytecode executor (opcodes.rs, vm_call.rs)
ferrython-core       Object model: PyObject, PyObjectRef (Arc), all 47 payload types
ferrython-gc         Hybrid ref-counting + generational cycle collector (3 gens)
ferrython-ffi        Rust-native extension module API (ModuleDef builder)
ferrython-import     Module resolution, sys.path, bytecode caching
ferrython-stdlib     45+ stdlib modules reimplemented in Rust
ferrython-debug      Disassembly, traceback formatting, line-number resolution
```

The largest/most complex files: `vm/opcodes.rs` (2113 lines), `parser/parser.rs` (2082 lines), `core/object/methods.rs` (2017 lines), `vm/vm_call.rs` (1507 lines).

## Key Conventions

### Object Model
- `PyObjectRef` = `Arc<PyObject>`. All Python values are heap-allocated and reference-counted.
- `PyObjectPayload` is a single 47-variant enum covering every Python type (Int, Str, List, Function, Class, Generator, Cell, etc.).
- `PyInt` uses a dual representation: `Small(i64)` for common values, `Big(Box<BigInt>)` for arbitrary precision — arithmetic promotes automatically.
- Mutable collections (List, Dict) wrap interior state in `Arc<RwLock<...>>`.

### Compiler (Two-Pass)
1. **Symbol table pass** (`symbol_table.rs`): walks AST to classify every variable as Local/Global/Nonlocal/Free/Cell before any bytecode is emitted.
2. **Code generation pass** (`expressions.rs`, `statements.rs`): uses symbol table to emit correct LOAD_FAST/LOAD_GLOBAL/LOAD_DEREF etc.

Nested code objects (functions, classes, comprehensions) are stored as `CodeObject` constants in the parent and created at runtime with `LOAD_CONST` + `MAKE_FUNCTION`.

### Closures & Cells
Captured variables use `PyObjectPayload::Cell(Arc<RwLock<Option<PyObjectRef>>>)`. The compiler emits `MAKE_CELL`/`LOAD_DEREF`/`STORE_DEREF` for free/cell variables.

### Bytecode Format
CPython 3.8 wordcode: 1-byte opcode + 1-byte arg, with `EXTENDED_ARG` for args > 255.

### FFI / Native Modules
Stdlib modules written in Rust use the `ModuleDef` builder from `ferrython-ffi`. The import system checks built-ins → FFI → filesystem in that order.

### Error Handling
- `PyResult<T>` = `Result<T, PyException>` throughout the VM and compiler.
- Custom error types use `thiserror`. `PyException` supports both `cause` (explicit `raise … from …`) and `context` (implicit chaining), per PEP 3134.

### GC
The cycle collector is triggered every 700 allocations (gen0 threshold). Call `ferrython_core::object::init_gc()` at startup before any object creation.

### Thread-Local EQ Dispatch
Dict/set key comparison installs a thread-local callback before any hashing operation to dispatch to Python-level `__eq__`/`__hash__`, avoiding circular crate dependencies.

## Test Fixtures

Python test files live in `tests/fixtures/` and follow a consistent harness pattern:

```python
passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL: " + name)
```

Files are named `test_phase{N}.py` (core language phases) and `test_expand{N}.py` (expanded coverage). The CPython compat, integration, and benchmark directories exist but are currently empty.
