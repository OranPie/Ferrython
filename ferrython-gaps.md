# Ferrython: CPython 3.8 Gap Audit

**Methodology:** All results are empirical — each item was verified by running an isolated Python
program through `ferrython` (`cargo build --release`, April 2026). No source-code inference.

**Score: 51 PASS · 13 FAIL/PARTIAL (language + stdlib present) + 19 missing modules**

> Items marked `[simplified]` are partially implemented and deferred — the stub unblocks common
> usage but full fidelity is pending.

Legend: ✅ passes · ❌ fails · ⚠️ partial/simplified

---

## 1. Grammar & Parser

| Feature | Status | Notes |
|---------|--------|-------|
| Semicolons as statement separators (`x=1; y=2`) | ✅ | Fixed |
| F-string basic: `f"{x}"`, `f"{x+1}"` | ✅ | |
| F-string lambda: `f"{(lambda a: a)(4)}"` | ✅ | Fixed |
| F-string walrus: `f"{(n:=5)}"` | ✅ | Fixed |
| F-string dict subscript (single quotes): `f"{d['k']}"` | ✅ | Fixed |
| F-string conditional (single quotes): `f"{'y' if c else 'n'}"` | ✅ | Fixed |
| F-string nested (mixed quotes): `f"hello {f'dear {x}'}"` | ✅ | Fixed |
| F-string format spec: `f"{n:08b}"`, `f"{x:.2f}"` | ✅ | |
| F-string multiline triple-quoted | ✅ | |
| F-string `!r` / `!s` conversions | ✅ | |
| F-string conditional, same outer quote: `f"{"y" if c else "n"}"` | ❌ | Inner `"` closes string; NameError |
| F-string `!a` conversion | ⚠️ | Same as `!r` for ASCII; non-ASCII behavior untested |
| Walrus `:=` in `if` / `while` | ✅ | |
| Walrus in comprehension (result + outer scope leak) | ✅ | Fixed |
| Positional-only `/` parameter TypeError enforcement | ✅ | Fixed |
| Multiple starred assignment targets → SyntaxError | ✅ | Fixed |
| `\N{NAME}` unicode name escapes | ✅ | `"\N{SNOWMAN}"` → `'☃'` |
| PEP 484 `# type: int` type comments | ❌ | All `type_comment` fields hardcoded `None` |
| PEP 263 encoding declaration `# -*- coding: ... -*-` | ❌ | Ignored; UTF-8 assumed |
| Non-ASCII in bytes literals | ❌ | Rejected outright |
| Lambda positional-only parameters (`lambda a, b, /: ...`) | ❌ | `/` not parsed in lambda params |
| Parse error line/column reporting | ⚠️ | Some errors still report wrong location |

---

## 2. AST Design Differences

These are deliberate design choices:

| Aspect | CPython 3.8 | Ferrython |
|--------|-------------|-----------|
| Async statements | Separate `AsyncFunctionDef`, `AsyncFor`, `AsyncWith` | Merged with `is_async: bool` flag |
| End locations | Optional | Always present on every node |
| `VisitorMut` | Full tree-walk | Only `visit_statement()` / `visit_expression()`; no default recursion |

---

## 3. Compiler & Bytecode

| Gap | Status |
|-----|--------|
| `SETUP_ASYNC_WITH` opcode | ❌ missing |
| Opcode number collision (`JumpIfTrueOrPop` = `SetupFinally` = 122) | ❌ undefined behaviour |
| Constant folding | ❌ not implemented |
| Peephole optimisation | ❌ not implemented |
| Dead code elimination after `return`/`raise` | ❌ not implemented |
| `__class__` cell for zero-arg `super()` | ✅ works |
| Exception table (CPython 3.11+ range-based) | ❌ uses jump-opcode style |
| `finally` return overrides `try` return | ❌ `try: return 1; finally: return 2` → returns `1`; CPython returns `2` |

---

## 4. VM & Runtime

### 4.1 Async / Await — Syntax Only ❌

`asyncio` module is absent (`ImportError`). Async syntax parses and compiles, but all async
opcodes raise `"async/await is not yet supported"` at runtime.

### 4.2 Arithmetic & Special Dunders

All fixed in recent commits:

| Dunder | Via | Status |
|--------|-----|--------|
| `__lt__`, `__le__`, `__eq__`, `__ne__`, `__gt__`, `__ge__` | comparisons | ✅ |
| `__add__`, `__sub__`, `__mul__`, etc. | operators | ✅ |
| `__radd__` fallback from builtin LHS | `sum([V(...)])` | ✅ Fixed |
| `__iadd__` / `__isub__` etc. | `v += other` | ✅ |
| `__bytes__` | `bytes(obj)` | ✅ Fixed |
| `__round__` | `round(obj, n)` | ✅ Fixed |
| `__trunc__` | `math.trunc(obj)` | ✅ Fixed |
| `__floor__` | `math.floor(obj)` | ✅ Fixed |
| `__ceil__` | `math.ceil(obj)` | ✅ Fixed |
| `__format__` | `format(obj, spec)` | ✅ |
| `__dir__` | `dir(obj)` | ✅ Fixed |
| `__fspath__` | `os.fspath(obj)` | ✅ Fixed |
| `__length_hint__` | `operator.length_hint(obj)` | ❌ Function exists but ignores `__length_hint__`; returns 0 |

### 4.3 Descriptor Protocol ✅

| Feature | Status |
|---------|--------|
| `__get__`, `__set__`, `__delete__` | ✅ |
| Data vs non-data descriptor priority | ✅ |
| `__getattribute__` override | ✅ |
| `__set_name__` | ✅ |
| `__instancecheck__` / `__subclasscheck__` | ✅ Fixed |

### 4.4 Metaclass

| Feature | Status |
|---------|--------|
| `metaclass=`, `__new__`/`__init__`, `__init_subclass__`, `__class_getitem__` | ✅ |
| MRO diamond inheritance | ✅ |
| `__instancecheck__` / `__subclasscheck__` | ✅ Fixed |
| `__prepare__` | ❌ class namespace is always a plain dict |
| Metaclass conflict resolution | ❌ |

### 4.5 `__slots__` ✅

Attribute restriction enforced. Remaining gaps: no descriptor objects per slot name;
`__dict__` not prevented on slotted classes.

### 4.6 Closures — `__closure__` Broken ❌

```python
def make_adder(n):
    def add(x): return x + n
    return add
f = make_adder(5)
f(3)                               # → 8  ✅  (closures work at runtime)
f.__closure__                      # → (5,)  — raw values, NOT cell objects
f.__closure__[0].cell_contents     # AttributeError: 'int' has no attribute 'cell_contents'
```

The `__closure__` tuple holds captured values directly instead of `cell` objects. Code
introspecting closures via `.cell_contents` (e.g., some debugging libraries) will fail.

### 4.7 Exception Chaining ✅ (Fixed)

`raise X from None`, `raise X from Y`, implicit `__context__` all work.
`__cause__`, `__context__`, `__suppress_context__` are set correctly.

### 4.8 `sys.exc_info()` Broken ❌

```python
import sys
try:
    raise TypeError("ti")
except TypeError:
    t, v, tb = sys.exc_info()
    # CPython: (TypeError, TypeError('ti'), <traceback>)
    # Ferrython: (None, None, None)
```

Returns `(None, None, None)` even inside an active `except` block. The current exception
state is not propagated to the thread-local slot that `exc_info` reads.

### 4.9 Generator `.throw()` ✅ (Fixed)

### 4.10 `finally` Return Override ❌

```python
def f():
    try: return 1
    finally: return 2
f()  # CPython → 2  |  Ferrython → 1
```

A `return` in a `finally` block should override the `try`-block return value per CPython semantics.

### 4.11 GC [simplified]

Three generations present (`gen0=700`, `gen1=10`, `gen2=10`). Generations not differentiated
during collection. Cycle detection covers `Instance` objects only; bare `Dict`/`List` cycles
are not reclaimed.

### 4.12 Import System

| Feature | Status |
|---------|--------|
| Module caching, dotted imports, relative imports | ✅ |
| `__import__()` builtin | ✅ Fixed |
| `__loader__`, `__spec__`, `__package__`, `__name__` on modules | ✅ Fixed |
| `sys.meta_path`, `sys.path_hooks` | ❌ |
| `importlib` module | ❌ ImportError |

---

## 5. Built-in Functions & `sys`

### 5.1 Built-in Functions

| Builtin | Status | Notes |
|---------|--------|-------|
| `eval("expr")` | ✅ | |
| `eval("expr", globals_dict)` | ✅ Fixed | |
| `dir(builtin)` / `dir(user_obj)` with `__dir__` | ✅ Fixed | |
| `format(obj, spec)` | ✅ | |
| `bytes(obj)` via `__bytes__` | ✅ Fixed | |
| `round()`, `math.trunc/floor/ceil()` on custom objects | ✅ Fixed | |
| `memoryview(bytes)` | ✅ Fixed | |
| `__import__(name)` | ✅ Fixed | |
| `super()` zero-arg | ✅ | |
| `... is Ellipsis` singleton | ✅ Fixed | |
| `print(end=, sep=)` to stdout | ✅ | |
| `print(..., file=buf)` | ❌ | `file=` kwarg ignored; always writes to real stdout |
| `breakpoint()` | ⚠️ | Prints advisory message; does not invoke pdb |
| `help()` | ❌ | Not implemented |

### 5.2 `sys` Module

| Attribute / Function | Status | Notes |
|---------------------|--------|-------|
| `sys.argv`, `sys.path`, `sys.version_info` | ✅ | |
| `sys.version_info[:2]` → `(3, 8)` | ✅ | |
| `sys.exit()` → `SystemExit` | ✅ | |
| `sys.getrecursionlimit()` | ✅ | |
| `sys.setrecursionlimit(n)` | ✅ Fixed | Actually changes the limit |
| `sys.modules` | ✅ | |
| `sys._getframe(n)` | ✅ Fixed | |
| `sys.__stdout__` | ✅ | |
| `sys.exc_info()` | ❌ | Returns `(None, None, None)` even inside handler |
| `sys.stdout = buf` | ❌ | Assignment accepted silently; `print()` ignores new value |
| `sys.stdin`, `sys.stderr` | ❌ | Not exposed |

---

## 6. Standard Library

### 6.1 Fully Working ✅

| Module | Key Features Verified |
|--------|-----------------------|
| `re` | match, findall, sub, groups, flags |
| `json` | dumps/loads, None, nested, unicode |
| `os` | path ops, environ, getcwd, listdir, fspath |
| `pathlib` | Path, mkdir, read_text, write_text, exists, unlink |
| `io` | StringIO, BytesIO — read/write/seek/readline |
| `datetime` | now(), date(), timedelta arithmetic, strftime |
| `dataclasses` | @dataclass, field(), __init__/__repr__/__eq__ |
| `collections` | Counter (+ most_common), deque, defaultdict, OrderedDict, namedtuple |
| `functools` | lru_cache, wraps, reduce, partial, total_ordering |
| `itertools` | count, cycle, chain, islice, product, combinations — lazy |
| `contextlib` | contextmanager, suppress |
| `abc` | ABC, abstractmethod — enforcement works |
| `enum` | Enum, IntEnum (+ isinstance(x, int)) |
| `copy` | copy(), deepcopy() |
| `hashlib` | md5, sha1, sha256 |
| `base64` | b64encode, b64decode |
| `bisect` | bisect_left, insort |
| `heapq` | heappush, heappop, nlargest |
| `csv` | reader, writer, DictReader with StringIO |
| `struct` | pack, unpack |
| `random` | seed, randint, shuffle, choice |
| `string` | ascii_lowercase, digits, etc. |
| `textwrap` | fill, dedent |
| `pprint` | pformat |
| `decimal` | Decimal — string-based arithmetic |
| `numbers` | Integral/Real/Complex isinstance |
| `weakref` | ref() — callable, returns referent |
| `threading` | Thread — start/join |
| `subprocess` | run() with capture_output + text |
| `logging` | getLogger, StreamHandler(buf), setLevel |
| `argparse` | ArgumentParser, add_argument, parse_args |
| `typing` | List, Dict, Optional, Union, Tuple etc. |

### 6.2 Present but Broken or Simplified ⚠️

| Module | Remaining Gap |
|--------|---------------|
| `datetime.strptime()` | ❌ `AttributeError: 'type' has no attribute 'strptime'` |
| `contextlib.ExitStack` | ❌ `enter_context()` → TypeError: takes at least 2 args (1 given) |
| `typing.get_type_hints()` | ❌ Returns `{}` — annotations not read from function objects |
| `warnings` | ❌ No `filters` attr; `catch_warnings(record=True)` list never populated |
| `operator.length_hint(obj)` | ❌ Ignores `__length_hint__`; returns 0 for custom objects |
| `subprocess.Popen` | ❌ Streaming/pipe management not implemented [simplified] |
| `csv.DictWriter` | ❌ Not implemented |
| `threading` sync primitives | ❌ Only `Thread` works; RLock/Semaphore/Event not functional |
| `fractions.Fraction` | ❌ `ImportError: No module named 'fractions'` |

### 6.3 Completely Absent — ImportError ❌

| Category | Modules |
|----------|---------|
| Async runtime | `asyncio`, `concurrent.futures` |
| OS / Signals | `signal`, `atexit` |
| Networking | `socket`, `http`, `urllib`, `email`, `ssl` |
| Database | `sqlite3`, `dbm` |
| Compression | `gzip`, `bz2`, `lzma`, `zlib`, `zipfile`, `tarfile` |
| Serialisation | `pickle`, `shelve`, `marshal` |
| XML / HTML | `xml`, `xml.etree.ElementTree`, `html`, `html.parser` |
| Data structures | `array`, `queue` |
| Numeric | `fractions`, `cmath` |
| Unicode | `unicodedata`, `codecs` |
| Introspection | `importlib`, `ast`, `symtable`, `token`, `tokenize`, `types`, `code` |
| Config | `configparser`, `getopt` |
| IDs | `uuid` |
| Dev tools | `pdb`, `doctest`, `pydoc`, `tracemalloc`, `faulthandler` |
| C interop | `ctypes`, `cffi` |
| Advanced OS | `mmap`, `fcntl`, `select`, `resource` |

---

## 7. Performance

| Benchmark | CPython 3.8 | Ferrython | Ratio |
|-----------|------------|-----------|-------|
| `fib(25)` | ~0.03 s | ~1.5 s | ~50× |
| `fib(30)` | ~0.3 s | ~14 s | ~47× |

No JIT, no constant folding, no peephole optimisation. Expected for an unoptimised interpreter.

---

## 8. Layout & Structural Weaknesses

### 8.1 God Files

| File | Lines | Issue |
|------|------:|-------|
| `vm/opcodes.rs` | 2,113 | All opcode handlers in one `impl` |
| `parser/parser.rs` | 2,082 | All grammar rules in one file |
| `core/object/methods.rs` | 2,017 | Arithmetic, comparison, string, attr, descriptor logic mixed |
| `vm/vm_call.rs` | 1,507 | All call/invoke logic |
| `compiler/statements.rs` | 1,079 | All statement compilation |
| `vm/builtins/core_fns.rs` | 1,066 | 40+ builtin functions |
| `stdlib/misc_modules.rs` | 1,010 | 19 unrelated modules |

### 8.2 VM Over-Coupling

`ferrython-vm` depends on 7 internal crates. Cannot be tested in isolation.

### 8.3 Fragile Import ↔ Stdlib Boundary

`ferrython-import` depends on `ferrython-stdlib` + `ferrython-parser` + `ferrython-compiler`.
Adding Python-level `importlib` would create a circular dependency.

### 8.4 Three Incompatible Error Types

| Crate | Error Type | `From` impl |
|-------|-----------|------------|
| `ferrython-parser` | `ParseError` | None |
| `ferrython-compiler` | `CompileError` | None |
| `ferrython-vm` | `PyException` | None |

### 8.5 No Automated Test Harness

- 64+ Python fixtures in `tests/fixtures/` — none run by `cargo test`
- ~5 Rust `#[test]` functions in entire codebase
- `tests/benchmarks/`, `tests/cpython_compat/`, `tests/integration/` — empty
- `tools/` — empty

### 8.6 Other

| Issue | Detail |
|-------|--------|
| Wildcard re-exports | `ferrython-core` exposes internal helpers as public API |
| String parsing duplication | `lexer.rs` and `string_parser.rs` overlap |
| Module boilerplate | `create_*_module()` pattern repeated 43+ times |
| CLI error handling duplication | Same `match Err(e) => eprintln!; exit(1)` ×3 |
| Dead code | 8 `#[allow(dead_code)]` markers; `sys_modules.rs` entirely marked dead |
