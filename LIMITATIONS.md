# Ferrython — Known Limitations

> Comprehensive inventory of gaps between Ferrython and CPython 3.8.
> Updated after CPython alignment test batch 1350 (~99.3% pass rate on 1350 tests).

---

## 1. Parser Limitations

| Feature | Status | Notes |
|---------|--------|-------|
| F-string nested same-quote | ❌ | `f"{'y' if c else 'n'}"` fails when inner quotes match outer |
| Type comments (PEP 484) | ❌ | All `type_comment` fields are `None` |
| Encoding declarations (PEP 263) | ❌ | Ignored; UTF-8 assumed |
| Non-ASCII in bytes literals | ❌ | Rejected at parse time |
| Lambda positional-only `/` | ❌ | `/` not parsed in lambda signatures |
| Error location accuracy | ⚠️ | Some parse errors report wrong column |

## 2. Compiler Limitations

| Feature | Status | Notes |
|---------|--------|-------|
| Constant folding | ❌ | `1 + 2` emits LOAD+LOAD+ADD, not LOAD 3 |
| Peephole optimization | ❌ | No jump-chain collapse, no dead-store removal |
| Dead code elimination | ❌ | Code after unconditional `return` still emitted |
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
| `sys.exc_info()` | ❌ | Returns `(None, None, None)` even inside `except` blocks |
| `__traceback__` attribute | ❌ | Exception objects lack `.tb_lineno`, `.tb_frame` etc. |
| `finally` return override | ⚠️ | `return` in `try` not always overridden by `return` in `finally` |

### 3.3 I/O Redirection
| Feature | Status | Notes |
|---------|--------|-------|
| `print(..., file=buf)` | ❌ | `file` kwarg silently ignored; always writes to stdout |
| `sys.stdout = buf` | ❌ | Assignment works but VM ignores replacement |

### 3.4 Introspection
| Feature | Status | Notes |
|---------|--------|-------|
| `__closure__` | ❌ | Returns raw values instead of cell objects |
| `__code__` | ❌ | Returns `None` on function objects |
| `type.__subclasses__()` | ❌ | Not tracked |
| `operator.length_hint()` | ❌ | Ignores `__length_hint__`, returns 0 |
| `dir()` completeness | ⚠️ | Missing imported names in some scopes |
| Dict views (`.keys()`) | ⚠️ | Returns snapshot list, not live view object |

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
| `decimal` | Constructor, basic repr | Arithmetic, context, precision control |
| `warnings` | `warn()` prints | `catch_warnings()` record list never populated |
| `dis` | Basic disassembly | Incomplete instruction set display |
| `numbers` | `Number` ABC exists | `Complex`, `Real`, `Rational` are stubs |
| `locale` | `getlocale()` returns C locale | No real locale support |
| `inspect` | `isfunction()`, `isclass()` | `getmembers()`, `signature()` incomplete |
| `typing` | `TypeVar`, `Generic`, `Protocol` exist | All are no-op placeholders; `get_type_hints()` returns `{}` |

### 4.2 Incomplete Implementations

| Module | Gap |
|--------|-----|
| `pickle` | Custom simplified format, not CPython-compatible |
| `csv` | `DictWriter` not implemented |
| `datetime` | `strptime()` raises `AttributeError` |
| `contextlib` | `ExitStack.enter_context()` raises `TypeError`; `redirect_stdout/stderr` are stubs |
| `multiprocessing` | `Pool` is a stub |
| `socket` | `setsockopt()`, `fileno()` are stubs; no real socket I/O |
| `ssl` | OpenSSL version hardcoded to `"(stub)"` |
| `configparser` | `write()` returns string instead of writing to file |
| `textwrap` | Placeholder suffix handling simplified |
| `bytes.join()` | Only accepts `list`, not arbitrary iterables |

### 4.3 Missing Modules (ImportError)

**Core**: `ctypes`, `cffi`, `mmap`, `fcntl`, `select`, `resource`
**Compression**: `gzip`, `bz2`, `lzma`, `zlib`, `zipfile`, `tarfile`
**Data**: `array`, `fractions`, `cmath`
**Dev tools**: `pdb`, `doctest`, `pydoc`, `tracemalloc`, `faulthandler`
**Unicode**: `unicodedata`, `codecs`
**Introspection**: `symtable`, `token`, `tokenize`, `code`

## 5. Performance Limitations

| Area | Status | Notes |
|------|--------|-------|
| Recursive fibonacci ~50× slower than CPython | ❌ | No constant folding, no JIT, no inline caching |
| No bytecode caching (`.pyc`) | ❌ | Every import re-parses and re-compiles |
| Arc-based refcounting overhead | ⚠️ | Atomic ops on every clone/drop |
| GC cycle detection | ⚠️ | Only covers `Instance` objects; `Dict`/`List` cycles not reclaimed |
| String interning | ❌ | No interning; every string allocation is fresh |
| Small-int caching | ❌ | No pre-allocated int pool for -5..256 |
| Attribute lookup | ⚠️ | Linear MRO scan every time; no method cache |

## 6. Structural / Code Quality Issues

| Issue | Location | Notes |
|-------|----------|-------|
| God files (>2,000 lines) | opcodes.rs, vm_call.rs, parser.rs | Hard to navigate and maintain |
| Three incompatible error types | parser/compiler/VM | No `From` impls between them |
| No `cargo test` for Python fixtures | tests/fixtures/ | 84 fixtures exist but not wired to test harness |
| Empty test directories | benchmarks/, cpython_compat/, integration/ | Infrastructure exists but unused |
| Import system scattered | opcodes.rs + vm_helpers.rs + vm_call.rs | Three code paths, partially duplicated |
| Dead code | db_modules.rs, misc_modules.rs | `#[allow(dead_code)]` markers |

## 7. Test Results Summary

- **CPython alignment tests**: ~1,343/1,350 pass (~99.5%)
- **Fixture tests**: 84/84 pass (100%)
- **Known test failures by category**:
  - Enum tuple-value unpacking (Test 1069)
  - `asyncio.wait_for` timeout (Test 1230)
  - `asyncio.Queue` blocking get (Test 1340)
  - `super().__getattribute__` (Test 1260)
  - Dict views not live (Test 1236, cosmetic)

---

*Last updated after test batch 1350. See `ferrython-gaps.md` for the original feature-by-feature gap audit.*
