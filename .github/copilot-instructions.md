# Copilot Instructions for Ferrython

Ferrython is a Python 3.8 interpreter written in Rust, organized as a Cargo workspace
of 17 crates (~67K lines of Rust). It targets CPython 3.8 compatibility with equal or
better performance on CPU-bound workloads.

## Build & Test Commands

```bash
# Build (always use --release for meaningful performance)
cargo build --release

# Run the interpreter
cargo run --release --bin ferrython -- script.py
cargo run --release --bin ferrython -- -c "print('hello')"
cargo run --release --bin ferrython                         # REPL

# CLI flags
cargo run --release --bin ferrython -- --dis script.py      # disassemble bytecode
cargo run --release --bin ferrython -- --profile script.py  # execution profiling
cargo run --release --bin ferrython -- --stats script.py    # bytecode statistics
cargo run --release --bin ferrython -- --compat script.py   # disable superinstructions

# Tests
cargo test                                    # all Rust unit tests
cargo test -p ferrython-parser                # single crate
cargo test -p ferrython-cli --test fixtures   # all 185 Python fixture tests
cargo test test_name                          # single test by name

# Run a single fixture test manually
cargo run --release --bin ferrython -- tests/fixtures/test_phase5.py

# PGO build (profile-guided optimization)
./build_pgo.sh
```

Code formatting: `max_width = 100` (see `rustfmt.toml`).
Build flags: `-C target-cpu=native` (see `.cargo/config.toml`).
Release profile: `opt-level = 3`, `lto = "fat"`, `codegen-units = 1`.

## Architecture

The pipeline is: **Source → Lexer → Parser → AST → Compiler → Bytecode → VM**

```
ferrython-cli        CLI entry point, argument parsing, --compat flag
ferrython-repl       Interactive REPL with readline + syntax highlighting
ferrython-parser     Lexer + recursive-descent parser → tokens/AST
ferrython-ast        AST node definitions, Visitor trait
ferrython-compiler   Two-pass compiler: symbol table → bytecode generation
                     Peephole optimizer: constant folding, dead code elimination,
                     jump chain collapse, superinstruction fusion (gated by --compat)
ferrython-bytecode   CPython 3.8-compatible opcode definitions, CodeObject
ferrython-vm         Stack-based bytecode executor (~6500 lines in vm.rs)
                     Hot-path macros, inline caches, look-ahead optimizations
ferrython-core       Object model: PyObject, PyObjectRef (Rc), PyCell, pool allocator
                     47 payload variants, FxHash maps, freelists, immortal singletons
ferrython-gc         Reference counting + trial-deletion cycle collector
ferrython-ffi        Rust-native extension module API (ModuleDef builder)
ferrython-import     Module resolution, sys.path, bytecode caching
ferrython-stdlib     155+ stdlib modules reimplemented in Rust
ferrython-debug      Disassembly, traceback formatting, line-number resolution
ferrython-traceback  Exception traceback formatting
ferrython-async      Asyncio event loop and coroutine support
ferrython-pip        Package installer
ferrython-toolchain  Build tooling
```

## Key Conventions

### Object Model
- `PyObjectRef` is a newtype wrapping `Rc<PyObject>` — single-threaded, no atomic overhead.
- `PyObjectPayload` is a 47-variant enum (≤32 bytes). Cold variants are boxed to keep it compact.
- `PyInt` dual representation: `Small(i64)` for common values, `Big(Box<BigInt>)` for arbitrary precision.
- `PyCell<T>` replaces `RwLock` for all mutable collection interiors — zero-cost `UnsafeCell`
  wrapper with `.read()` / `.write()` API, safe under GIL guarantee.
- Mutable collections: `List(Rc<PyCell<Vec<...>>>)`, `Dict(Rc<PyCell<FxHashKeyMap>>)`, etc.

### Memory & Allocation
- **Pool allocator**: Thread-local slab allocator (128-block slabs) with intrusive freelist
  for PyObject allocation. Eliminates malloc/free for hot paths.
- **Immortal objects**: `IMMORTAL_REFCOUNT = u32::MAX` — Clone/Drop are no-ops.
  Applied to: None, True, False, small ints (-5..65536), float singletons, empty collections.
- **Freelists**: Dict maps (80), attr maps (80), instance objects (80), exceptions (16).
- **FxHash**: All attribute maps and dict/set maps use FxHasher (3-4x faster than SipHash
  for short strings).
- **Global allocator**: MiMalloc.

### VM Dispatch (vm.rs)
The main dispatch loop uses unsafe stack/local access macros for zero-bounds-check performance:
- `spush!` / `spop!` / `speek!` — unchecked stack operations
- `slocal!` / `sset_local!` — unchecked local variable access
- `hot_ok!` — skip `Ok(None)` construction, directly continue dispatch
- `chain_jump!` — if next instruction is `JumpAbsolute`, consume it inline
- `cmp_jump_lookahead!` — fuse CompareOp + PopJumpIf* to skip bool allocation
- `chain_pop_none!` — skip pushing None if followed by POP_TOP

Interned method names (`define_interned!` macro) enable `ptr_eq()` comparisons for hot
builtin method dispatch (append, pop, get, join, split, replace, etc.).

### Compiler (Two-Pass)
1. **Symbol table pass** (`symbol_table.rs`): classifies every variable as
   Local/Global/Nonlocal/Free/Cell before any bytecode is emitted.
2. **Code generation pass** (`expressions.rs`, `statements.rs`): emits correct
   LOAD_FAST/LOAD_GLOBAL/LOAD_DEREF etc.

The peephole optimizer runs fixed-point iterations: constant folding → dead store
elimination → dead code elimination → jump chain collapse → NOP removal →
superinstruction fusion (unless `--compat`).

### Closures & Cells
Captured variables use `PyObjectPayload::Cell(CellRef)` where `CellRef = Rc<PyCell<Option<PyObjectRef>>>`.
The compiler emits `MAKE_CELL`/`LOAD_DEREF`/`STORE_DEREF` for free/cell variables.

### Bytecode Format
CPython 3.8 wordcode: 1-byte opcode + 1-byte arg, with `EXTENDED_ARG` for args > 255.
Superinstructions use opcode numbers above CPython's range (disabled with `--compat`).

### Error Handling
- `PyResult<T>` = `Result<T, PyException>` throughout the VM and compiler.
- `PyException` supports `cause` (explicit `raise … from …`) and `context` (implicit chaining).

### GC
Trial-deletion cycle collector triggered every 700 allocations. Only cycle-capable types
(Instance, Dict, List) are tracked. Call `ferrython_core::object::init_gc()` at startup.

### Thread-Local EQ Dispatch
Dict/set key comparison installs a thread-local callback before hashing to dispatch
to Python-level `__eq__`/`__hash__`, avoiding circular crate dependencies.

## Test Fixtures

185 Python test files in `tests/fixtures/` with a consistent harness:

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

Files are named `test_phase{N}.py` (core language features) and `test_expand{N}.py`
(expanded coverage). The Rust test harness (`ferrython-cli/tests/fixtures.rs`) generates
one `#[test]` per fixture file via the `fixture_test!()` macro.

## Performance Guidelines

- **Do NOT add new opcodes** or superinstructions without the `--compat` gate.
- **Do NOT add fast-path specializations** (e.g., hardcoding more builtin methods).
- Focus on **architecture, algorithms, and data structure** improvements.
- Always benchmark with `--release`. Benchmark variance is high (20-50%) on shared VMs —
  only trust back-to-back A/B comparisons in the same session.
- The `--compat` flag disables superinstructions for fair CPython comparison.
  Use `FERRYTHON_COMPAT=1` env var as an alternative.
