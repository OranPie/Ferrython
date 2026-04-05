# Ferrython — Known Limitations

> Comprehensive inventory of gaps between Ferrython and CPython 3.8.
> Updated: 155+ stdlib modules, 125 fixture tests (all passing), 15 crates, ~64K lines Rust.

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
| Exception hierarchy matching | ✅ | `except ArithmeticError` catches `ZeroDivisionError` |
| Multiple except clauses | ✅ | Tuple of exception types, bare except |

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
| Unbound dunder methods | ✅ | `int.__add__(3, 4)`, 35+ dunders on all builtin types |
| `super().__delattr__` / `__eq__` / `__hash__` | ✅ | Full super proxy delegation |
| `dir()` with dunders | ✅ | Includes type-specific dunders (e.g., `__add__`, `__len__` for list) |
| Regex lookahead/lookbehind | ✅ | Via fancy-regex fallback for `(?=`, `(?!`, `(?<=`, `(?<!` |

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
| `__class__` implicit cell (PEP 3135) | ❌ | `__class__` in methods not auto-injected by compiler |
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
| `argparse` | ArgumentParser, add_argument, parse_args, parse_known_args | Subparsers, mutually exclusive groups, FileType |

### 4.2 Incomplete Implementations

| Module | Gap |
|--------|-----|
| `pickle` | Custom simplified format, not CPython wire-compatible; instance methods not restored on unpickle |
| `socket` | `setsockopt()` is stub; UDP recvfrom/sendto not implemented |
| `configparser.write()` | Returns string instead of writing to file-like object |
| `subprocess.Popen` | Streaming/pipe management not implemented |
| `sqlite3` | Basic query execution; missing cursor protocol details |
| `xml.etree` | Namespace support (xmlns) not implemented |
| `decimal` | Context precision works but capped at 36 digits (i128 range); missing copy_decimal |
| `os.environ` | `os.environ[k] = v` does not call `putenv()` — use `os.putenv()` to sync |
| `weakref` | Collections (WeakValueDictionary, etc.) are stubs using regular containers |

### 4.3 Recently Fixed (This Session)

| Module | What was fixed |
|--------|---------------|
| `json.JSONDecodeError` | Proper `ValueError` subclass; `except json.JSONDecodeError` works |
| `json.dumps` `default=` | Python function callbacks via VM interception; pre-processes object tree |
| `json.dump` `default=` | Same as `json.dumps`; file output with custom serialization |
| `super()` in `@classmethod` | Now properly unwraps ClassMethod/StaticMethod descriptors |
| `super()` in `@staticmethod` | Raw function returned correctly through super proxy |
| `json.dump/load` | File dispatch: NativeFunction write/read now receives self arg correctly |
| In-place operators | `list +=`, `list *=`, `set \|=`, `set -=`, `set &=`, `set ^=` preserve identity |
| `namedtuple` | `defaults=` kwarg now parsed and applied (right-aligned to fields) |
| `hashlib` | Now supports `update()`, `digest()`, `hexdigest()`, `copy()` (incremental hashing) |
| `enum` | `auto()` per-class counter, CPython `repr` format `<Class.Name: value>` |
| `argparse` | Subparsers, mutually exclusive groups |
| `struct` | `unpack_from()`, `iter_unpack()`, `struct.error` exception |
| `io` | StringIO/BytesIO protocol methods, BytesIO.readline, StringIO.__iter__ |
| `subprocess` | `.pid`, `send_signal()`, `terminate()` with SIGTERM |

### 4.3 Missing Modules (ImportError)

| Category | Modules |
|----------|---------|
| C interop | `ctypes`, `cffi` |

> All previously listed missing modules (compression, pdb, mmap, fcntl, select, resource, token) have been implemented.

## 5. Performance

| Benchmark | CPython 3.8 | Ferrython (release) | Ratio |
|-----------|------------|-----------|-------|
| `fib(25)` | ~0.03 s | ~0.13 s | ~4× slower |
| `fib(30)` | ~0.3 s | ~1.3 s | ~4× slower |
| Method calls/s | — | 564K | — |
| Dict 100K ops | — | 0.19s | — |
| List 100K append+sum | — | 0.12s | — |
| Genexpr sum 100K | — | 0.11s | — |
| Try/except 100K | — | 0.02s | — |

| Optimization | Status |
|-------------|--------|
| Constant folding (multi-pass) | ✅ |
| Peephole optimizer | ✅ |
| String interning | ✅ |
| Small-int cache (-5..256) | ✅ |
| Arc<CodeObject> sharing | ✅ |
| Binary op fast paths | ✅ |
| Method resolution cache | ✅ |
| Inline CallFunction fast path | ✅ |
| Frame vector pool (16 slots) | ✅ |
| Global cache per frame | ✅ |
| Bytecode caching (.pyc) | ❌ |

## 6. Architecture

- **15 crates** in Cargo workspace (~64K lines Rust)
- **155+ stdlib modules** registered
- **137 tests** (125 fixture + 12 unit, all passing via `cargo test`)
- **13 microbenchmarks** in benchmark suite

| Issue | Status | Notes |
|-------|--------|-------|
| God files (>2,000 lines) | ⚠️ | vm_call.rs ~2,481 lines; most others split |
| Error type unification | ✅ | `From<ParseError>` and `From<CompileError>` for `PyException` |
| Test harness | ✅ | All fixtures wired via `cargo test` |
| Import system | ✅ | Consolidated in ferrython-import crate |
| Debug tooling | ✅ | Profiler, breakpoints, disassembler, bytecode stats |
| XML Element state | ✅ | Unified instance-attr model (no dual-state desync) |

## 7. Recent Improvements (This Session)

| Feature | Details |
|---------|---------|
| XML Element redesign | Eliminated dual-state bug; `child.text = "hello"` now serializes correctly |
| `ET.tostring()` bytes | Returns `bytes` by default (CPython compat); `encoding='unicode'` for str |
| `urljoin()` normalization | Properly resolves `..` and `.` path segments |
| `dir()` dunders | Includes type-specific dunders for all builtin types |
| `frozenset` cross-type ops | `frozenset & set`, `frozenset - set`, `frozenset ^ set` all work |
| `Decimal` precision | Preserves trailing zeros (e.g., `3.14 + 2.86 = 6.00`) |
| Regex lookaround | `(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)` via fancy-regex fallback |
| `csv.DictWriter` | `writeheader()` and `writerow()` now produce correct output |

---

*Last updated after XML redesign, urljoin fix, dir() enhancement, regex lookaround, stdlib expansion.*
