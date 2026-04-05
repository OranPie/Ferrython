# Ferrython — Known Limitations

> Comprehensive inventory of gaps between Ferrython and CPython 3.8.
> Updated after CPython alignment test batch 1350 (~99.3% pass rate on 1350 tests).

---

## 1. Parser Limitations

| Feature | Status | Notes |
|---------|--------|-------|
| F-string nested same-quote | ⚠️ | Matches CPython 3.8 behavior (rejected); Python 3.12+ allows it |
| Type comments (PEP 484) | ❌ | All `type_comment` fields are `None` |
| Encoding declarations (PEP 263) | ❌ | Ignored; UTF-8 assumed |
| Non-ASCII in bytes literals | ❌ | Rejected at parse time |
| Lambda positional-only `/` | ❌ | `/` not parsed in lambda signatures |
| Error location accuracy | ⚠️ | Some parse errors report wrong column |

## 2. Compiler Limitations

| Feature | Status | Notes |
|---------|--------|-------|
| Constant folding | ✅ | Multi-pass: `2*3+4` → `10`, string concat+repeat |
| Peephole optimization | ✅ | Jump chain collapse, dead store elimination, NOP removal |
| Dead code elimination | ✅ | Unreachable code after return/jump/raise NOP'd out |
| `SETUP_ASYNC_WITH` opcode | ❌ | Missing; `async with` partially supported via fallback |
| Exception tables (3.11+) | ❌ | Uses legacy jump-opcode exception style |

## 3. Runtime / VM Limitations

### 3.1 Descriptor Protocol
| Feature | Status | Notes |
|---------|--------|-------|
| `super().__getattribute__` | ❌ | `super()` doesn't proxy `__getattribute__` |
| Data descriptor priority edge cases | ⚠️ | Most cases work; some MRO edge cases may differ |

### 3.2 Exception Handling
| Feature | Status | Notes |
|---------|--------|-------|
| `sys.exc_info()` | ✅ | Thread-local tracking, set on handler entry, cleared on PopExcept |
| `__traceback__` attribute | ✅ | Proper linked traceback objects with tb_lineno, tb_filename, tb_name, tb_next |
| `finally` return override | ✅ | `return` in `finally` correctly overrides `return` in `try` |

### 3.3 I/O Redirection
| Feature | Status | Notes |
|---------|--------|-------|
| `print(..., file=buf)` | ✅ | Dispatches to file object's `.write()` method |
| `sys.stdout = buf` | ✅ | VM resolves `sys.stdout` for each print call |

### 3.4 Introspection
| Feature | Status | Notes |
|---------|--------|-------|
| `__closure__` | ✅ | Returns tuple of cell objects |
| `__code__` | ✅ | Returns actual CodeObject |
| `__kwdefaults__` | ✅ | Returns keyword-only defaults dict |
| `__globals__` | ✅ | Returns function's global namespace |
| `type.__subclasses__()` | ✅ | Tracked via weak references |
| `operator.length_hint()` | ✅ | Handles NativeFunction, NativeClosure, falls back to py_len() |
| `dir()` completeness | ⚠️ | Missing imported names in some scopes |
| Dict views (`.keys()`) | ✅ | Live view objects backed by shared Arc; support len/iter/contains/repr |

### 3.5 Async Runtime
| Feature | Status | Notes |
|---------|--------|-------|
| Real event loop scheduling | ❌ | All coroutines run to completion sequentially |
| `asyncio.wait_for` timeout | ❌ | Runs coroutine immediately, timeout ignored |
| `asyncio.Queue` blocking | ❌ | `await queue.get()` doesn't suspend; raises if empty |
| Task cancellation | ❌ | `task.cancel()` is a no-op |
| `async for` / `async with` | ⚠️ | Basic support; edge cases may fail |

### 3.6 Enum
| Feature | Status | Notes |
|---------|--------|-------|
| Tuple-value auto-unpacking | ❌ | `EARTH = (mass, radius)` doesn't unpack into `__init__(self, mass, radius)` |
| `IntEnum` / `IntFlag` | ⚠️ | Basic support; some operator edge cases |

## 4. Standard Library Limitations

### 4.1 Stub Modules (import works, most functions are no-ops or simplified)

| Module | What works | What doesn't |
|--------|-----------|--------------|
| `asyncio` | `run()`, `gather()`, `sleep()`, `Queue` basic | Real scheduling, timeouts, cancellation |
| `signal` | `signal.signal()` accepts handler | Handler never invoked; returns `SIG_DFL` |
| `decimal` | Constructor, arithmetic (+, -, *, /), comparisons, quantize, repr | Context/precision control, advanced math (ln, sqrt, exp) |
| `warnings` | `warn()` prints | `catch_warnings()` record list never populated |
| `dis` | Basic disassembly | Incomplete instruction set display |
| `numbers` | `Number` ABC exists | `Complex`, `Real`, `Rational` are stubs |
| `locale` | `getlocale()` returns C locale | No real locale support |
| `inspect` | 17 functions: is*, getmembers, signature, getfullargspec, getdoc, getfile | Parameter/Signature classes are stubs |
| `typing` | `TypeVar`, `Generic`, `Protocol` exist | All are no-op placeholders; `get_type_hints()` returns `{}` |

### 4.2 Incomplete Implementations

| Module | Gap |
|--------|-----|
| `pickle` | Custom simplified format, not CPython-compatible |
| `csv` | `DictWriter` writerow/writerows/writeheader implemented |
| `datetime` | `strptime()` works |
| `contextlib` | `ExitStack.enter_context()` works for native CMs; `redirect_stdout/stderr` are stubs |
| `multiprocessing` | `Pool` is a stub |
| `socket` | `setsockopt()`, `fileno()` are stubs; no real socket I/O |
| `ssl` | OpenSSL version hardcoded to `"(stub)"` |
| `configparser` | `write()` returns string instead of writing to file |
| `textwrap` | Placeholder suffix handling simplified |
| `bytes.join()` | ✅ | Accepts list, tuple, frozenset, set |

### 4.3 Missing Modules (ImportError)

**Core**: `ctypes`, `cffi`, `mmap`, `fcntl`, `select`, `resource`
**Compression**: `gzip`, `bz2`, `lzma`, `zipfile`, `tarfile` (zlib exists but simplified)
**Dev tools**: `pdb`, `pydoc`, `tracemalloc`, `faulthandler`
**Introspection**: `symtable`, `token`, `tokenize`, `code`

## 5. Performance Limitations

| Area | Status | Notes |
|------|--------|-------|
| Recursive fibonacci ~7× slower than CPython | ⚠️ | Arc<CodeObject> + shared caches; fib(20) at 48 ops/s |
| Function call overhead | ✅ | 1.2M calls/s (was 220K — Arc<CodeObject> + shared constant cache) |
| No bytecode caching (`.pyc`) | ❌ | Every import re-parses and re-compiles |
| Arc-based refcounting overhead | ⚠️ | Atomic ops on every clone/drop |
| GC cycle detection | ⚠️ | Only covers `Instance` objects; `Dict`/`List` cycles not reclaimed |
| String interning | ❌ | No interning; every string allocation is fresh |
| Small-int caching | ✅ | Pre-allocated int pool for -5..=256 (matches CPython) |
| Pre-boxed constant cache | ✅ | Built once per function, shared across all frames via Arc |
| Binary op fast paths | ✅ | int+int, float+float, str+str skip dunder dispatch |
| Shared builtins | ✅ | Arc<IndexMap> — zero clone overhead per frame |
| Attribute lookup | ✅ | Per-class method resolution cache; invalidated on namespace mutation |

## 6. Structural / Code Quality Issues

| Issue | Location | Status | Notes |
|-------|----------|--------|-------|
| God files (>2,000 lines) | opcodes.rs, vm_call.rs, parser.rs | ⚠️ | opcodes.rs split into focused handlers; vm_call.rs helpers extracted |
| Three incompatible error types | parser/compiler/VM | ❌ | No `From` impls between them |
| `cargo test` for Python fixtures | tests/fixtures/ | ✅ | 84/84 wired via `cargo test -p ferrython-cli --test fixtures` |
| Import system scattered | opcodes.rs + vm_helpers.rs + vm_call.rs | ✅ | Consolidated into `vm_import.rs` |
| Dead code | db_modules.rs | ⚠️ | 2 `#[allow(dead_code)]` on incomplete sqlite3 structs |
| misc_modules.rs catch-all | stdlib crate | ✅ | Split from 1717 → 801 lines; modules redistributed to category files |
| Debug tooling | ferrython-debug crate | ✅ | Profiler, breakpoints, disassembler, bytecode stats |

## 7. Test Results Summary

- **CPython alignment tests**: ~1,343/1,350 pass (~99.5%)
- **Fixture tests**: 86/86 pass (100%)
- **Known test failures by category**:
  - Enum tuple-value unpacking (Test 1069)
  - `asyncio.wait_for` timeout (Test 1230)
  - `asyncio.Queue` blocking get (Test 1340)
  - `super().__getattribute__` (Test 1260)
  - Dict views ✅ (Test 1236 — now live view objects)

---

*Last updated after restructure session. See `ferrython-gaps.md` for the original feature-by-feature gap audit.*
