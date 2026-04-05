# Ferrython — Known Limitations

> Comprehensive inventory of gaps between Ferrython and CPython 3.8.
> Updated after builtin subclass inheritance fix session (124/124 tests pass).

---

## 1. Parser Limitations

| Feature | Status | Notes |
|---------|--------|-------|
| F-string nested same-quote | ⚠️ | `f"{"y" if c else "n"}"` — matches CPython 3.8 (rejected); Python 3.12+ allows it |
| Type comments (PEP 484) | ❌ | All `type_comment` fields are `None` |
| Encoding declarations (PEP 263) | ❌ | Ignored; UTF-8 assumed |
| Non-ASCII in bytes literals | ❌ | Rejected at parse time |
| Lambda positional-only `/` | ❌ | `/` not parsed in lambda signatures |
| Error location accuracy | ⚠️ | Some parse errors report wrong column |

## 2. Compiler Limitations

| Feature | Status | Notes |
|---------|--------|-------|
| Constant folding | ✅ | Multi-pass: `2*3+4` → `10`, `"a"+"b"+"c"` → `"abc"` |
| Peephole optimization | ✅ | Jump chain collapse, dead store elimination, NOP removal |
| Dead code elimination | ✅ | Unreachable code after return/jump/raise NOP'd out |
| `SETUP_ASYNC_WITH` opcode | ❌ | Missing; `async with` supported via fallback |
| Exception tables (3.11+) | ❌ | Uses legacy jump-opcode exception style |

## 3. Runtime / VM Limitations

### 3.1 Builtin Type Subclassing
| Feature | Status | Notes |
|---------|--------|-------|
| `class MyList(list)` | ✅ | `repr`, `str`, `len`, `iter`, `bool`, `getitem`, `append` all work |
| `class MyDict(dict)` | ✅ | dict_storage delegation + custom `__missing__` |
| `class MyTuple(tuple)` | ✅ | Via `__builtin_value__` delegation |
| `class MyInt(int)` / `MyStr(str)` | ✅ | Arithmetic/string operations delegated |
| `class MySet(set)` | ✅ | Via `__builtin_value__` delegation |
| HTMLParser subclassing | ✅ | Proper Class with MRO inheritance; callbacks via deferred calls |
| ConfigParser subclassing | ✅ | Proper Class with per-instance state |
| Thread subclassing | ✅ | Proper Class with start/join/is_alive |

### 3.2 Descriptor Protocol
| Feature | Status | Notes |
|---------|--------|-------|
| `__get__`, `__set__`, `__delete__` | ✅ | Full descriptor protocol |
| Data vs non-data descriptor priority | ✅ | data > instance dict > non-data |
| `__getattribute__` override | ✅ | |
| `__set_name__` | ✅ | |

### 3.3 Exception Handling
| Feature | Status | Notes |
|---------|--------|-------|
| `sys.exc_info()` | ✅ | Thread-local tracking, set on handler entry |
| `__traceback__` attribute | ✅ | Proper linked traceback objects with source lines |
| `finally` return override | ✅ | `return` in `finally` correctly overrides `return` in `try` |
| Exception chaining (`from`) | ✅ | `__cause__`, `__context__`, `__suppress_context__` |
| Source-line tracebacks | ✅ | Shows actual Python source line (like CPython) |

### 3.4 I/O Redirection
| Feature | Status | Notes |
|---------|--------|-------|
| `print(..., file=buf)` | ✅ | Dispatches to file object's `.write()` method |
| `sys.stdout = buf` | ✅ | VM resolves `sys.stdout` for each print call |
| `contextlib.redirect_stdout` | ✅ | Uses override stack in stdlib |

### 3.5 Introspection
| Feature | Status | Notes |
|---------|--------|-------|
| `__closure__` with `cell_contents` | ✅ | Proper cell objects |
| `__code__`, `__globals__`, `__kwdefaults__` | ✅ | |
| `type.__subclasses__()` | ✅ | Tracked via weak references |
| `operator.length_hint()` | ✅ | Dispatches `__length_hint__` dunder |
| `inspect` module (17 functions) | ✅ | is*, getmembers, signature, getfullargspec |
| Dict views (`.keys()`, `.values()`, `.items()`) | ✅ | Live view objects |

### 3.6 Async Runtime
| Feature | Status | Notes |
|---------|--------|-------|
| `asyncio.run()`, `gather()`, `sleep()` | ✅ | Sequential execution model |
| Real event loop scheduling | ❌ | All coroutines run to completion sequentially |
| `asyncio.wait_for` timeout | ❌ | Runs coroutine immediately, timeout ignored |
| `asyncio.Queue` blocking | ❌ | `await queue.get()` doesn't suspend; raises if empty |
| Task cancellation | ❌ | `task.cancel()` is a no-op |
| `async for` / `async with` | ⚠️ | Basic support; edge cases may fail |

### 3.7 Other Runtime
| Feature | Status | Notes |
|---------|--------|-------|
| Metaclass with `__prepare__` | ⚠️ | Parses but namespace is always a plain dict |
| Metaclass conflict resolution | ❌ | Not implemented |
| `__slots__` `__dict__` prevention | ⚠️ | Restriction enforced but `__dict__` not prevented |
| GC generations | ⚠️ | Three generations present; not differentiated during collection |

## 4. Standard Library Limitations

### 4.1 Simplified Modules (import works, partially functional)

| Module | What works | What doesn't |
|--------|-----------|--------------|
| `asyncio` | `run()`, `gather()`, `sleep()`, `Queue` basic | Real scheduling, timeouts, cancellation |
| `signal` | `signal.signal()` accepts handler | Handler never invoked |
| `decimal` | Arithmetic, comparisons, quantize | Context/precision control, advanced math |
| `warnings` | `warn()` prints | Filter management, `catch_warnings` population |
| `numbers` | `Number`, `Integral`, `Real`, `Complex` ABCs | Full abstract interface compliance |
| `locale` | `getlocale()` returns C locale | No real locale support |
| `typing` | `TypeVar`, `Generic`, `Protocol`, all container types | `get_type_hints()` returns `{}` |
| `ssl` | Module imports | OpenSSL version hardcoded to `"(stub)"` |
| `multiprocessing` | Module imports | `Pool` is a stub |

### 4.2 Incomplete Implementations

| Module | Gap |
|--------|-----|
| `pickle` | Custom simplified format, not CPython wire-compatible |
| `csv.DictWriter` | writeheader/writerow stubs (no output) |
| `socket` | `setsockopt()`, `fileno()` are stubs; no real socket I/O |
| `configparser.write()` | Returns string instead of writing to file-like object |
| `subprocess.Popen` | Streaming/pipe management not implemented |
| `sqlite3` | Basic query execution; missing cursor protocol details |

### 4.3 Missing Modules (ImportError)

| Category | Modules |
|----------|---------|
| C interop | `ctypes`, `cffi` |
| OS / Low-level | `mmap`, `fcntl`, `select`, `resource` |
| Compression | `gzip`, `bz2`, `lzma`, `zipfile`, `tarfile` |
| Dev tools | `pdb`, `pydoc`, `tracemalloc`, `faulthandler` |
| Introspection | `symtable`, `token`, `tokenize` |

## 5. Performance

| Benchmark | CPython 3.8 | Ferrython | Ratio |
|-----------|------------|-----------|-------|
| `fib(25)` | ~0.03 s | ~0.18 s | ~6× slower |
| `fib(30)` | ~0.3 s | ~1.4 s | ~5× slower |
| Function calls/s | — | 1.2M | — |

| Optimization | Status |
|-------------|--------|
| Constant folding (multi-pass) | ✅ |
| Peephole optimizer | ✅ |
| String interning | ✅ |
| Small-int cache (-5..256) | ✅ |
| Arc<CodeObject> sharing | ✅ |
| Binary op fast paths | ✅ |
| Method resolution cache | ✅ |
| Bytecode caching (.pyc) | ❌ |

## 6. Architecture

- **15 crates** in Cargo workspace
- **106 stdlib modules** registered
- **124 fixture tests** (all passing)
- **13 microbenchmarks** in benchmark suite

| Issue | Status | Notes |
|-------|--------|-------|
| God files (>2,000 lines) | ⚠️ | vm_call.rs ~2,295 lines; most others split |
| Error type unification | ✅ | `From<ParseError>` and `From<CompileError>` for `PyException` |
| Test harness | ✅ | All fixtures wired via `cargo test` |
| Import system | ✅ | Consolidated in ferrython-import crate |
| Debug tooling | ✅ | Profiler, breakpoints, disassembler, bytecode stats |

---

*Last updated after builtin subclass inheritance fix + BuiltinBoundMethod delegation refactor.*
