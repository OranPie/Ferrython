# Ferrython: Simplifications, Layout Weaknesses & CPython 3.8 Gaps

Comprehensive analysis of where Ferrython diverges from, simplifies, or is missing features compared to CPython 3.8, plus structural/layout issues in the codebase.

---

## 1. Grammar & Parser Gaps

### 1.1 F-String Limitations

| Gap | Detail |
|-----|--------|
| **No nested f-strings** | `f"{f'{x}'}"` — lexer treats f-string content as raw text then does a simplistic character walk (`parser.rs:1395–1428`), no recursive tokenisation |
| **Complex expressions limited** | Lambdas, walrus operators, and multi-line expressions inside `{}` may fail because the manual brace-depth counter lacks context awareness |
| **Format spec not parsed** | The format spec after `:` is stored as a raw string, never parsed into an AST node (`parser.rs:1436–1442`) |

### 1.2 Type Comments — Not Implemented

Every `type_comment` field on AST nodes is hardcoded to `None` (`parser.rs:143, 255, 284, 494`). PEP 484 `# type: int` annotations are completely ignored.

### 1.3 Unicode Name Escapes — Stubbed

`\N{SNOWMAN}` replaces with U+FFFD (replacement character). No Unicode name database lookup (`string_parser.rs:97–98`, marked `// TODO`).

### 1.4 Encoding Declarations — Not Implemented

No PEP 263 support (`# -*- coding: utf-8 -*-`). Files are assumed UTF-8.

### 1.5 Lambda Positional-Only Parameters

`parse_lambda_params()` handles `*` and `**` but has no `/` separator case — lambdas cannot declare positional-only parameters (`parser.rs:1497–1547`).

### 1.6 Starred Assignment Validation

`a, *b, *c = items` should be a compile-time error (only one starred target allowed). The parser accepts it without validation (`parser.rs:1830–1857`).

### 1.7 Bytes String Limitation

Bytes literals reject all non-ASCII characters outright (`string_parser.rs:187–192`) rather than allowing escaped non-ASCII like CPython.

---

## 2. AST Differences

The AST is **feature-complete** — all 53 CPython 3.8 node types are present and all fields match. The differences are design choices, not gaps:

| Aspect | CPython | Ferrython |
|--------|---------|-----------|
| Async statements | Separate `AsyncFunctionDef`, `AsyncFor`, `AsyncWith` classes | Merged into `FunctionDef`, `For`, `With` with `is_async: bool` flag |
| End locations | Not standard (added in 3.8 as optional) | Always present: `end_line`, `end_column` on every node |
| `VisitorMut` | Full tree-walk support | Minimal — only `visit_statement()` and `visit_expression()`, no default recursion |

---

## 3. Compiler & Bytecode Gaps

### 3.1 Missing / Problematic Opcodes

| Opcode | Status | Impact |
|--------|--------|--------|
| `SETUP_ASYNC_WITH` | **Missing** | Async context managers (`async with`) cannot be compiled |
| `JumpIfTrueOrPop` vs `SetupFinally` | **Numbering collision** — both assigned value 122 | Undefined runtime behaviour |

### 3.2 No Optimisation Passes

| Pass | Status |
|------|--------|
| Constant folding | ❌ — `x = 1 + 2` emits `LOAD_CONST 1`, `LOAD_CONST 2`, `BINARY_ADD` instead of `LOAD_CONST 3` |
| Peephole optimisation | ❌ — no jump folding, no dead store removal |
| Dead code elimination | ❌ — code after unconditional `return`/`raise` is still compiled |

### 3.3 No `__class__` Cell for `super()`

CPython automatically creates a `__class__` cell variable in every method so `super()` with no arguments works. Ferrython does not — zero references to `__class__` in the compiler. **`super()` without explicit arguments will fail in methods.**

### 3.4 No Exception Table

`CodeObject` has no `exception_table` field. Exception handlers rely on `SETUP_EXCEPT`/`SETUP_FINALLY` jump opcodes rather than a mapping from bytecode ranges to handler offsets, as CPython does.

### 3.5 Exception Handler Variable Cleanup

CPython deletes the exception variable (`as e`) at the end of the `except` block to avoid reference cycles. Ferrython does not emit the cleanup bytecode.

---

## 4. VM & Runtime Gaps

### 4.1 Async/Await — Syntax Only

Async syntax is parsed and compiled, but at runtime every async opcode (`GET_AITER`, `GET_ANEXT`, `BEFORE_ASYNC_WITH`, `GET_AWAITABLE`, `END_ASYNC_FOR`) raises `"async/await is not yet supported"` (`opcodes.rs:2103–2106`).

### 4.2 Comparison Dunders Not Called on Instances

`__lt__`, `__le__`, `__eq__`, `__ne__`, `__gt__`, `__ge__` defined on user classes are **not dispatched**. Comparisons fall through to type-based logic in `methods.rs:893–928`.

### 4.3 Descriptor Protocol Incomplete

| Feature | Status |
|---------|--------|
| `__get__`, `__set__`, `__delete__` | ✅ Detected and called for Property |
| Data vs non-data descriptor priority | ✅ Correct |
| Descriptors for dunder operations (`__add__` etc.) | ❌ Not walked through MRO — `try_binary_dunder` only checks `Instance.get_attr` |
| `__getattribute__` override | ❌ Only default implementation, no custom override support |
| `__set_name__` | ⚠️ Only called on `Instance` objects, not all descriptor types |

### 4.4 Metaclass Gaps

| Feature | Status |
|---------|--------|
| `metaclass=` keyword | ✅ Stored in ClassData |
| `__new__` / `__init__` on metaclass | ✅ Called |
| `__init_subclass__` | ✅ Called (PEP 487) |
| `__prepare__` | ❌ Not implemented — class namespace is always a plain dict |
| `__instancecheck__` / `__subclasscheck__` | ❌ Not implemented |
| Metaclass conflict resolution | ❌ Not implemented |

### 4.5 Missing Magic Methods

| Method | Impact |
|--------|--------|
| `__bytes__` | `bytes(obj)` won't dispatch to custom objects |
| `__fspath__` | `os.fspath()` won't work on custom path objects |
| `__length_hint__` | Iterator size hints not used |
| `__round__`, `__trunc__`, `__floor__`, `__ceil__` | `round()`, `math.trunc()` etc. won't dispatch |
| `__aenter__`, `__aexit__`, `__await__`, `__aiter__`, `__anext__` | All async protocols non-functional |
| `__context__`, `__suppress_context__` | Implicit exception chaining missing |

### 4.6 `__slots__` — Partial

Slots are detected and enforced for attribute assignment on instances, but:
- No descriptor objects created for slot names
- No prevention of `__dict__` on slotted classes
- Not removed from `dir()` output

### 4.7 Generational GC — Counters Only

The three-generation structure exists and thresholds are tracked (`gen0=700`, `gen1=10`, `gen2=10`), but the actual collection strategy does not differentiate generations — all eligible objects are scanned every time. Cycle detection only covers `Instance` objects (not bare `Dict`/`List` cycles).

### 4.8 Import System Simplifications

| Feature | Status |
|---------|--------|
| Module caching, dotted imports, relative imports | ✅ |
| `__import__` builtin | ❌ Not exposed to Python code |
| `sys.meta_path`, `sys.path_hooks` | ❌ Not implemented |
| `importlib` module | ❌ Not implemented |
| `__loader__`, `__spec__` on modules | ❌ Never set |

### 4.9 Missing Built-in Functions

`breakpoint`, `help`, `memoryview`, `__import__` are registered but not implemented or raise errors.

### 4.10 Frame & Traceback — Internal Only

Frame objects exist internally but are not exposed to Python. `sys._getframe()` is not implemented. `__traceback__` is not set on exception instances.

### 4.11 Threading — Not Supported

No GIL, no thread objects, no actual concurrency. The `threading` stdlib module is a stub.

---

## 5. Standard Library Gaps

### 5.1 Coverage Summary

| Category | Fully Implemented | Partial / Stub | Missing |
|----------|:-:|:-:|:-:|
| Modules in `lib.rs` | ~15 | ~18 stubs | 150+ from CPython |

### 5.2 Partial / Stub Modules

These are registered but have significant missing functionality:

| Module | What Works | What's Missing |
|--------|-----------|----------------|
| `collections` | OrderedDict (alias to dict), basic defaultdict/Counter/deque | `Counter.most_common()`, `deque` rotation/maxlen, `defaultdict.__missing__` hook |
| `functools` | `reduce`, `partial` | `lru_cache` non-functional, `wraps` limited, `singledispatch` absent |
| `itertools` | Most functions present | All use **eager materialisation** instead of lazy generators |
| `datetime` | `datetime.now()`, basic attributes | No arithmetic, incomplete `strftime`, no timezone/`timedelta` operations |
| `dataclasses` | `@dataclass` decorator recognised | No `__init__`, `__repr__`, `__eq__` auto-generation |
| `io` | `StringIO`, `BytesIO` listed | Stubs only — no read/write |
| `pathlib` | `Path()` constructor, `.name/.stem/.suffix/.parent` | No path operations (`.exists()`, `.read_text()`, etc.) |
| `csv` | Basic `reader()` | `writer`, `DictReader`, `DictWriter` are stubs |
| `subprocess` | `run()`, `call()`, `check_output()` | No `Popen` streaming or pipe management |
| `logging` | Basic `getLogger()`, level functions | No handlers, formatters, or file output |
| `typing` | All type aliases (`List`, `Dict`, `Optional`, etc.) | Constants only — no runtime type checking |
| `abc` | `ABC`, `ABCMeta`, `@abstractmethod` | Markers only — no enforcement |
| `enum` | `Enum`, `IntEnum` classes | Stub markers — no enum functionality |
| `weakref` | Functions listed | All stubs — no actual weak references |
| `threading` | Classes listed | All stubs — no actual threading |
| `unittest` | `TestCase`, `main()` | Stubs — no test runner |
| `argparse` | `ArgumentParser` class | Stub — no argument parsing |

### 5.3 Completely Absent Module Categories

| Category | Missing Modules |
|----------|----------------|
| **Networking** | `socket`, `http`, `urllib`, `email`, `ssl`, `ftplib`, `smtplib` |
| **Async** | `asyncio`, `concurrent.futures` |
| **Database** | `sqlite3`, `dbm` |
| **Compression** | `gzip`, `bz2`, `lzma`, `zlib`, `zipfile`, `tarfile` |
| **XML / HTML** | `xml`, `html`, `xml.etree` |
| **Serialisation** | `pickle`, `shelve`, `marshal` |
| **C interop** | `ctypes`, `cffi` |
| **Data structures** | `array`, `bisect`, `heapq`, `queue` |
| **Numeric** | `fractions`, `cmath` |
| **Introspection** | `importlib`, `ast`, `symtable`, `token`, `tokenize`, `types`, `code` |
| **OS advanced** | `signal`, `atexit`, `mmap`, `fcntl`, `select`, `resource` |
| **Unicode** | `unicodedata`, `codecs`, `locale` (stub only) |
| **Config** | `configparser`, `getopt` |
| **Dev tools** | `pdb`, `doctest`, `pydoc`, `tracemalloc`, `faulthandler` |

---

## 6. Layout & Structural Weaknesses

### 6.1 God Files

11 files exceed 500 lines; 3 exceed 2,000:

| File | Lines | Problem |
|------|------:|---------|
| `vm/opcodes.rs` | 2,113 | All opcode handlers in one `impl` block — impossible to isolate changes |
| `parser/parser.rs` | 2,082 | Every grammar rule in one file |
| `core/object/methods.rs` | 2,017 | Single trait impl mixing arithmetic, comparison, string, attribute, and descriptor logic |
| `vm/vm_call.rs` | 1,507 | All call/invoke logic |
| `compiler/statements.rs` | 1,079 | All statement compilation |
| `vm/builtins/core_fns.rs` | 1,066 | 40+ builtin functions |
| `stdlib/misc_modules.rs` | 1,010 | 19 unrelated stdlib modules |

### 6.2 VM Over-Coupling

`ferrython-vm` depends on **7 internal crates** (bytecode, core, compiler, parser, stdlib, import, debug). This makes it impossible to test the VM in isolation and means any change in any lower crate can break the VM.

### 6.3 Fragile Import ↔ Stdlib Boundary

`ferrython-import` depends on `ferrython-stdlib`, `ferrython-parser`, and `ferrython-compiler`. If `ferrython-stdlib` ever needs to execute Python code (e.g., for `importlib`), a circular dependency emerges.

### 6.4 Three Incompatible Error Types

| Crate | Error Type | Conversion to Others |
|-------|-----------|---------------------|
| `ferrython-parser` | `ParseError` | None |
| `ferrython-compiler` | `CompileError` | None |
| `ferrython-vm` | `PyException` | None |

The CLI handles each separately with duplicated `match` arms. No `From` impls, no unified error trait.

### 6.5 No Automated Test Harness

- 64 Python fixture files in `tests/fixtures/` but **none are run by `cargo test`**
- Only ~5 `#[test]` functions in the entire Rust codebase
- `tests/benchmarks/`, `tests/cpython_compat/`, `tests/integration/` are **empty directories**
- `tools/` directory is **completely empty**

### 6.6 Over-Exposed Public APIs

`ferrython-core` uses wildcard re-exports (`pub use payload::*`, `pub use methods::*`, `pub use helpers::*`), exposing internal helpers like `is_data_descriptor` and `has_descriptor_get` as public API.

### 6.7 Code Duplication

| Area | Files Involved |
|------|---------------|
| String parsing | `lexer.rs` and `string_parser.rs` overlap |
| Type coercion | `builtins/type_methods.rs` and `vm_helpers.rs` |
| Module creation boilerplate | Same `create_*_module()` pattern repeated 43 times |
| CLI error handling | Same `match … Err(e) => { eprintln!; exit(1) }` pattern ×3 |

### 6.8 Dead Code

8 `#[allow(dead_code)]` markers across the codebase, including:
- `parser.rs:30` — `filename` field declared but never read
- `sys_modules.rs` — entire module marked dead code
