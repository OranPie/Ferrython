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
| `super().__getattribute__` | ✅ | Proxies to MRO-based lookup via NativeClosure |
| Data descriptor priority edge cases | ✅ | Full CPython descriptor protocol: data > instance dict > non-data |

### 3.2 Exception Handling
| Feature | Status | Notes |
|---------|--------|-------|
| `sys.exc_info()` | ✅ | Thread-local tracking, set on handler entry, cleared on PopExcept |
| `__traceback__` attribute | ✅ | Proper linked traceback objects with tb_lineno, tb_filename, tb_name, tb_next |
| `finally` return override | ✅ | `return` in `finally` correctly overrides `return` in `try` |
| Unified error types | ✅ | `From<ParseError>` and `From<CompileError>` for `PyException`; enables `?` across error boundaries |

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
| Tuple-value auto-unpacking | ✅ | `EARTH = (mass, radius)` correctly unpacks into `__init__` |
| `IntEnum` / `IntFlag` | ✅ | Supports int comparisons and arithmetic |

### 3.7 ABC Enforcement
| Feature | Status | Notes |
|---------|--------|-------|
| Abstract method enforcement | ✅ | Abstract methods enforced at instantiation time |

## 4. Standard Library Limitations

### 4.1 Stub Modules (import works, most functions are no-ops or simplified)

| Module | What works | What doesn't |
|--------|-----------|--------------|
| `asyncio` | `run()`, `gather()`, `sleep()`, `Queue` basic | Real scheduling, timeouts, cancellation |
| `signal` | `signal.signal()` accepts handler | Handler never invoked; returns `SIG_DFL` |
| `decimal` | Constructor, arithmetic (+, -, *, /), comparisons, quantize, repr | Context/precision control, advanced math (ln, sqrt, exp) |
| `warnings` | `warn()` prints, `catch_warnings(record=True)` captures | Context/filter management |
| `dis` | Full bytecode disassembly with line numbers, args, jump targets | Output uses Rust stdout (not capturable via sys.stdout) |
| `numbers` | `Number` ABC exists | `Complex`, `Real`, `Rational` are stubs |
| `locale` | `getlocale()` returns C locale | No real locale support |
| `inspect` | 17 functions: is*, getmembers, signature, getfullargspec, getdoc, getfile | Parameter/Signature classes are stubs |
| `typing` | `TypeVar`, `Generic`, `Protocol` exist | All are no-op placeholders; `get_type_hints()` returns `{}` |
| `pathlib` | `Path` with 16 methods: exists, is_dir, is_file, mkdir, read_text, write_text, etc. | Advanced path operations |
| `functools` | `lru_cache` fully implemented with maxsize, cache_info, cache_clear | `singledispatch`, `total_ordering` are stubs |

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
| GC cycle detection | ✅ | Covers `Instance`, `Dict`, and `List` objects; trial deletion algorithm |
| String interning | ✅ | `intern.rs` covers dunder names + `intern_or_new()` in hot paths |
| Small-int caching | ✅ | Pre-allocated int pool for -5..=256 (matches CPython) |
| Pre-boxed constant cache | ✅ | Built once per function, shared across all frames via Arc |
| Binary op fast paths | ✅ | int+int, float+float, str+str skip dunder dispatch |
| Shared builtins | ✅ | Arc<IndexMap> — zero clone overhead per frame |
| Attribute lookup | ✅ | Per-class method resolution cache; invalidated on namespace mutation |

## 6. Structural / Code Quality Issues

| Issue | Location | Status | Notes |
|-------|----------|--------|-------|
| God files (>2,000 lines) | opcodes.rs, vm_call.rs | ✅ | opcodes.rs split into focused handlers; vm_call.rs helpers extracted; parser.rs split into mod.rs, statements.rs, expressions.rs, arguments.rs |
| Error type unification | parser/compiler/VM | ✅ | `From<ParseError>` and `From<CompileError>` for `PyException` in ferrython-core |
| `cargo test` for Python fixtures | tests/fixtures/ | ✅ | 91/91 wired via `cargo test -p ferrython-cli --test fixtures` |
| Import system scattered | opcodes.rs + vm_helpers.rs + vm_call.rs | ✅ | Consolidated into `vm_import.rs` |
| Dead code | db_modules.rs | ⚠️ | 2 `#[allow(dead_code)]` on incomplete sqlite3 structs |
| Module organization | stdlib crate | ✅ | network_modules split (socket, http); serial_modules split (json, csv, other); collection_modules split (collections, functools, itertools, operator, other) |
| Debug tooling | ferrython-debug crate | ✅ | Profiler, breakpoints, disassembler (`--dis` disassembles to stderr then executes), bytecode stats |

## 7. Test Results Summary

- **CPython alignment tests**: ~1,343/1,350 pass (~99.5%)
- **Fixture tests**: 91/91 pass (100%)
- **Known test failures by category**:
  - `asyncio.wait_for` timeout (Test 1230)
  - `asyncio.Queue` blocking get (Test 1340)

---

*Last updated after error unification + GC extension session. See `ferrython-gaps.md` for the original feature-by-feature gap audit.*
