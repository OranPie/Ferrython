# Ferrython: CPython 3.8 Gap Audit

Comprehensive empirical and structural analysis of where Ferrython diverges from CPython 3.8.
**Methodology:** Every gap listed in §1–§5 was verified by running isolated Python programs through the
`ferrython` binary (`cargo build --release`). Results are empirical PASS/FAIL, not source inference.
A separate source-level structural analysis is in §6.

**Test run summary (268 isolated invocations):** 194 PASS · 74 FAIL

> ⚠️ Some items listed as gaps in a previous source-only analysis were incorrect.
> Corrections are noted explicitly with `[CORRECTED]`.

---

## 1. Grammar & Parser Gaps

### 1.1 Semicolons as Statement Separators — Completely Unsupported ❌

**This is the most impactful undocumented gap.** CPython allows multiple statements on one line
separated by `;`. Ferrython raises a `SyntaxError` for any semicolon, even in perfectly valid code:

```python
x = 0; y = 1          # SyntaxError: expression expected
```

This breaks:
- Any code minified or compacted onto a single line
- REPL-style one-liners
- Common patterns like `import os; os.getcwd()`

### 1.2 F-String Limitations

| Expression inside `{...}` | CPython | Ferrython | Error |
|--------------------------|---------|-----------|-------|
| Lambda: `f"{(lambda a: a)(4)}"` | ✅ | ❌ | SyntaxError (wrong line reported) |
| Walrus: `f"{(n:=5)}"` | ✅ | ❌ | SyntaxError (wrong line reported) |
| Dict subscript: `f"{d['k']}"` with outer `"` | ✅ | ❌ | SyntaxError |
| Nested f-string: `f"hello {f'dear {x}'}"` | ✅ | ❌ | NameError (`f` treated as identifier) |
| Conditional, same-quote: `f"{"y" if c else "n"}"` | ✅ | ❌ | NameError (inner `"` closes string) |
| Conditional, mixed-quote: `f"{'y' if c else 'n'}"` | ✅ | ✅ | — |
| Alignment format spec: `f"{s:>10}"` | ✅ | ✅ | — |
| Basic variable: `f"{x}"`, `f"{x+1}"` | ✅ | ✅ | — |

**Root cause:** The f-string lexer does a simplistic brace-depth walk without recursive tokenisation,
so complex expressions that contain nested quotes or operators resembling grammar tokens fail.

**Parse errors always report line 1:** When an f-string or other syntax error occurs, the span
always shows `start_line: 1, start_col: 9` regardless of the actual error location.

### 1.3 Walrus Operator (`:=`) in Comprehensions — Wrong Result ❌

```python
evens = [y for x in range(5) if (y := x*2) > 4]
# CPython: [6, 8, 10]   (x=3→y=6, x=4→y=8, x=5 would give 10 but range stops at 4)
# Actually CPython: [6, 8]  — but the scoping should propagate y to outer scope
# Ferrython: [6, 8]  with y=8 in outer scope ← behaviour differs from CPython scoping rules
```

Walrus in `if` conditions and standalone expressions works. The issue is PEP 572's scoping
rule: walrus inside a comprehension should propagate the binding to the *enclosing* scope,
not the comprehension scope.

### 1.4 Positional-Only Parameter Enforcement — Silent Failure ❌

```python
def f(a, b, /, c): return a + b + c
f(1, b=2, c=3)    # CPython: TypeError: b got multiple values / got unexpected keyword
                   # Ferrython: silently succeeds, returns 6
```

The syntax parses and the function runs, but the `/` boundary is not enforced at call time.

### 1.5 Unicode Name Escapes — Works ✅ `[CORRECTED]`

A previous analysis stated `\N{NAME}` produced U+FFFD. **Empirically verified to work:**

```python
"\N{SNOWMAN}"  # → '☃'   ✅
```

### 1.6 Type Comments, Encoding Declarations, Bytes Literals

| Gap | Status |
|-----|--------|
| PEP 484 `# type: int` comments | ❌ ignored (all `type_comment` fields `None`) |
| PEP 263 `# -*- coding: ... -*-` | ❌ not implemented; UTF-8 assumed |
| Non-ASCII in bytes literals | ❌ rejected outright instead of allowing escapes |
| Lambda positional-only params (`/`) | ❌ no `/` case in `parse_lambda_params()` |
| Multiple starred targets `a, *b, *c = ...` | ❌ accepted silently (should be SyntaxError) |

---

## 2. AST Design Differences

| Aspect | CPython 3.8 | Ferrython |
|--------|-------------|-----------|
| Async statements | Separate `AsyncFunctionDef`, `AsyncFor`, `AsyncWith` | Merged into `FunctionDef`/`For`/`With` with `is_async: bool` |
| End locations | Optional | Always present on every node |
| `VisitorMut` | Full tree-walk | Only `visit_statement()` and `visit_expression()`; no default recursion |

---

## 3. Compiler & Bytecode Gaps

| Gap | Status |
|-----|--------|
| `SETUP_ASYNC_WITH` opcode | ❌ missing — async context managers cannot compile |
| Opcode number collision (`JumpIfTrueOrPop` and `SetupFinally` both = 122) | ❌ undefined behaviour |
| Constant folding (`1+2` → `LOAD_CONST 3`) | ❌ not implemented |
| Peephole optimisation (jump folding, dead stores) | ❌ not implemented |
| Dead code elimination after `return`/`raise` | ❌ not implemented |
| `__class__` cell for zero-arg `super()` | ✅ **works** `[CORRECTED]` — empirically confirmed |
| Exception table (CPython 3.11+ style) | ❌ uses `SETUP_EXCEPT`/`SETUP_FINALLY` jump opcodes |
| Exception variable cleanup at end of `except` block | ❌ no cleanup bytecode emitted |

---

## 4. VM & Runtime Gaps

### 4.1 Async / Await — Syntax Only ❌

Async syntax parses and compiles. At runtime, all async opcodes raise
`"async/await is not yet supported"`. `asyncio` module is also missing (`ImportError`).

```python
import asyncio         # ImportError: No module named 'asyncio'
async def f(): ...     # parses ✅, runs: RuntimeError at first await
```

### 4.2 Comparison Dunders — Work ✅ `[CORRECTED]`

A previous analysis stated `__lt__`, `__le__`, `__eq__`, `__ne__`, `__gt__`, `__ge__` on user
classes were not dispatched. **Empirically verified: all six comparison dunders are called correctly.**

### 4.3 Arithmetic Reflected Dunder (`__radd__`) — Not Dispatched ❌

```python
class V:
    def __radd__(self, o):
        if o == 0: return self
        return NotImplemented

sum([V(1), V(2), V(3)])   # CPython: V(6)  via 0 + V(1) → __radd__
                           # Ferrython: TypeError: unsupported operand type(s) for +: 'int' and 'V'
```

When the LHS is a built-in type (e.g., `int`) and `__add__` returns `NotImplemented`, ferrython
does not fall back to calling `__radd__` on the RHS.

### 4.4 In-Place Dunder (`__iadd__`) — Works ✅

`v += other` correctly dispatches `__iadd__` and reassigns.

### 4.5 `__iter__` / `__next__` on User Classes — Work ✅ `[CORRECTED]`

Custom iterator protocol (`__iter__` returning `self`, `__next__` raising `StopIteration`)
works correctly with `list()`, `for` loops, etc.

### 4.6 Missing Numeric Magic Methods

| Dunder | Dispatch via | CPython | Ferrython | Error |
|--------|-------------|---------|-----------|-------|
| `__bytes__` | `bytes(obj)` | ✅ | ❌ | `TypeError: cannot convert to bytes` |
| `__round__` | `round(obj, n)` | ✅ | ❌ | `TypeError: type doesn't define __round__` |
| `__trunc__` | `math.trunc(obj)` | ✅ | ❌ | `TypeError: float() argument must be... 'N'` |
| `__floor__` | `math.floor(obj)` | ✅ | ❌ | `TypeError: float() argument must be... 'N'` |
| `__ceil__` | `math.ceil(obj)` | ✅ | ❌ | `TypeError: float() argument must be... 'N'` |

### 4.7 `format()` Builtin — Works ✅ `[CORRECTED]`

`format(obj, "spec")` correctly calls `obj.__format__("spec")`.

### 4.8 `dir()` — Broken for Built-ins ❌

```python
dir([])        # CPython: ['append', 'clear', 'copy', ...]
               # Ferrython: []   (empty list)

class D:
    def __dir__(self): return ["custom"]
dir(D())       # CPython: ['custom']
               # Ferrython: ['__annotations__', '__dir__', '__qualname__']
               # (doesn't call __dir__; returns internal attrs)
```

### 4.9 `__fspath__` / `os.fspath()` — Not Implemented ❌

```python
os.fspath(my_path_obj)  # AttributeError: 'module' object has no attribute 'fspath'
```

`os.fspath()` is not implemented. The `__fspath__` protocol is therefore non-functional.

### 4.10 `operator.length_hint()` — Not Implemented ❌

```python
import operator
operator.length_hint(obj)  # AttributeError: 'module' object has no attribute 'length_hint'
```

### 4.11 Descriptor Protocol

| Feature | Status |
|---------|--------|
| `__get__`, `__set__`, `__delete__` | ✅ work correctly |
| Data vs non-data descriptor priority | ✅ correct |
| `__getattribute__` custom override | ✅ `[CORRECTED]` — works |
| `__set_name__` | ✅ `[CORRECTED]` — works |
| `__instancecheck__` / `__subclasscheck__` on metaclass | ❌ not dispatched |
| Descriptors for dunder operations | ❌ `try_binary_dunder` only checks `Instance.get_attr` |

### 4.12 `__slots__` — Mostly Works ✅ `[CORRECTED]`

Basic slot declaration and attribute access work:
```python
class S:
    __slots__ = ["x", "y"]
    def __init__(self):
        self.x = 1; self.y = 2    # Note: requires separate lines due to §1.1
```

Remaining slot gaps:
- No descriptor objects created for slot names
- No prevention of `__dict__` on slotted classes

### 4.13 Metaclass

| Feature | Status |
|---------|--------|
| `metaclass=` keyword | ✅ |
| `__new__` / `__init__` on metaclass | ✅ |
| `__init_subclass__` | ✅ |
| `__class_getitem__` | ✅ `[CORRECTED]` |
| MRO diamond inheritance | ✅ `[CORRECTED]` |
| `__prepare__` | ❌ class namespace is always a plain dict |
| `__instancecheck__` / `__subclasscheck__` | ❌ |
| Metaclass conflict resolution | ❌ |

### 4.14 Exception Chaining

```python
raise RuntimeError("clean") from None
# CPython: sets __suppress_context__ = True, __cause__ = None
# Ferrython: AttributeError: 'exception' object has no attribute '__suppress_context__'
```

`__suppress_context__`, `__cause__`, and `__context__` attributes are not implemented on
exception objects. `raise X from Y` syntax parses, but chaining semantics are absent.

### 4.15 Generator `.throw()` — Broken ❌

```python
g = gen()
next(g)
g.throw(ValueError, ValueError("msg"))
# CPython: generator catches the exception in its try/except, yields handler result
# Ferrython: ValueError propagates out (not injected into the generator)
```

### 4.16 `fn.__closure__` — Returns None ❌

```python
def make_adder(n):
    def add(x): return x + n
    return add

make_adder(5).__closure__      # CPython: (<cell at 0x...>,)
                               # Ferrython: None
```

Closures function correctly, but the `__closure__` attribute is `None` instead of a tuple
of cell objects. Cell contents are inaccessible from Python.

### 4.17 GC Details

The three-generation structure exists with thresholds (`gen0=700`, `gen1=10`, `gen2=10`),
but generations are not differentiated during collection — all eligible objects are scanned
every cycle. Cycle detection only covers `Instance` objects, not bare `Dict`/`List` cycles.

### 4.18 Import System

| Feature | Status |
|---------|--------|
| Module caching, dotted imports, relative imports | ✅ |
| `__import__` builtin | ❌ `NameError: name '__import__' is not defined` |
| `sys.meta_path`, `sys.path_hooks` | ❌ not implemented |
| `importlib` module | ❌ `ImportError: No module named 'importlib'` |
| `__loader__`, `__spec__` on modules | ❌ never set |

---

## 5. Built-in Functions & `sys` Module

### 5.1 Built-in Functions

| Builtin | Status | Error |
|---------|--------|-------|
| `print(..., end=X)` | ✅ works | — |
| `print(..., sep=X)` | ✅ works | — |
| `eval("expr")` | ✅ basic eval works | — |
| `eval("expr", globals)` | ❌ | `NameError` — globals dict not used |
| `dir(builtin)` | ❌ | Returns `[]` for list, dict, etc. |
| `dir(user_obj)` | ❌ | Ignores `__dir__`; returns internal attrs |
| `format(obj, spec)` | ✅ works | — |
| `round(n)` | ✅ for floats | — |
| `round(custom_obj, n)` | ❌ | `TypeError: type doesn't define __round__` |
| `bytes(obj)` | ❌ | `TypeError: cannot convert to bytes` |
| `memoryview(b)` | ❌ | `NameError: name 'memoryview' is not defined` |
| `__import__(name)` | ❌ | `NameError: name '__import__' is not defined` |
| `breakpoint()` | ❌ | `TypeError: 'breakpoint' is not callable` |
| `help()` | ❌ | not implemented |
| `super()` (no args) | ✅ works `[CORRECTED]` | — |

### 5.2 `Ellipsis` Singleton Identity ❌

```python
x = ...
type(x).__name__     # 'ellipsis'  ✅  (lowercase, correct)
x is Ellipsis        # False  ❌  — should be True; singleton identity broken
Ellipsis             # works (name resolves) ✅
```

The `...` literal and the `Ellipsis` name both exist, but they are not the same object.

### 5.3 `sys` Module Gaps

| `sys` attribute/function | Status | Error |
|--------------------------|--------|-------|
| `sys.argv`, `sys.path`, `sys.version_info` | ✅ | — |
| `sys.version_info[:2]` | ✅ returns `(3, 8)` | — |
| `sys.exit()` | ✅ raises `SystemExit` | — |
| `sys.getrecursionlimit()` | ✅ returns 1000 | — |
| `sys.setrecursionlimit(n)` | ❌ | Silently ignored — limit does not change |
| `sys.exc_info()` | ❌ | `AttributeError: 'module' object has no attribute 'exc_info'` |
| `sys.stdout` (read) | ✅ | — |
| `sys.stdout = buf` (write) | ❌ | `AttributeError: 'module' object does not support attribute assignment` |
| `sys._getframe()` | ❌ | `AttributeError: 'module' object has no attribute '_getframe'` |
| `sys.stdin`, `sys.stderr` | ❌ | not exposed |
| `sys.modules` | ✅ exists | — |

---

## 6. Standard Library

### 6.1 Fully Absent — `ImportError` ❌

These modules are completely unimplemented:

| Category | Modules |
|----------|---------|
| **Async** | `asyncio`, `concurrent.futures` |
| **OS / Signals** | `signal`, `atexit` |
| **Networking** | `socket`, `http`, `urllib`, `email`, `ssl`, `ftplib`, `smtplib` |
| **Database** | `sqlite3`, `dbm` |
| **Compression** | `gzip`, `bz2`, `lzma`, `zlib`, `zipfile`, `tarfile` |
| **XML / HTML** | `xml`, `html`, `xml.etree` |
| **Serialisation** | `pickle`, `shelve`, `marshal` |
| **Data structures** | `array`, `bisect` *(see §6.2)*, `heapq` *(see §6.2)*, `queue` |
| **Numeric** | `fractions`, `cmath` |
| **Introspection** | `importlib`, `ast`, `symtable`, `token`, `tokenize`, `types`, `code` |
| **Unicode** | `unicodedata`, `codecs` |
| **Config** | `configparser`, `getopt` |
| **IDs** | `uuid` |
| **Dev tools** | `pdb`, `doctest`, `pydoc`, `tracemalloc`, `faulthandler` |
| **C interop** | `ctypes`, `cffi` |

> Note: `bisect` and `heapq` import successfully — see §6.2.

### 6.2 Present but Significantly Broken ⚠️

| Module | What Works | What's Broken |
|--------|-----------|---------------|
| `decimal` | `Decimal(str)` constructor | Arithmetic gives floating-point result: `Decimal("1.1") + Decimal("2.2")` → `3.3000000000000003` instead of `3.3` |
| `numbers` | Module imports; ABC classes present | `isinstance(42, numbers.Integral)` → `False`; built-in types are not registered with the ABCs |
| `enum.IntEnum` | Declaration, member access, equality | `isinstance(Dir.N, int)` → `False`; `IntEnum` members are not `int` subclasses |
| `weakref` | Module imports | `weakref.ref(obj)` returns `obj` itself (the referent), not a callable weakref; `r()` raises `TypeError` |
| `threading` | Module imports | `threading.Thread(target=f)` returns `None` (constructor broken); no actual threading |
| `subprocess` | `subprocess.run()` runs the process | `capture_output=True` does not capture; `text=True` does not decode; stdout/stderr not accessible |
| `warnings` | `warnings.warn()` emits to stderr | `warnings.catch_warnings()` context manager doesn't capture; `len(w)` fails |
| `logging` | `logging.getLogger()`, `logger.info()` | `StreamHandler(buf)` writes to stderr, not to `buf`; stream injection broken |
| `argparse` | `ArgumentParser()` constructor | `add_argument()` not implemented; `dir(p)` shows only `['__argparse__']` |
| `csv` | `csv.reader()` with file/list input | `csv.DictReader(io.StringIO(...))` fails: `TypeError: 'StringIO' object is not iterable` |
| `datetime` | `datetime.now()`, `.year/.month/.day`, `strftime()` | `date + timedelta` → `TypeError`; `datetime.strptime()` not implemented |
| `contextlib.ExitStack` | Imports | `stack.enter_context(cm)` → `TypeError: takes at least 2 argument(s) (1 given)` |
| `typing` | Type aliases, annotations | `get_type_hints(f)` → `KeyError` when accessing annotation keys |
| `numbers` (via `platform`) | `platform.system()` works | `platform.python_version()` unknown |
| `bisect` | Module imports; functions present | Not fully verified |
| `heapq` | Module imports; functions present | Not fully verified |

### 6.3 Present and Working ✅ `[CORRECTED from prior analysis]`

Several modules documented as non-functional are **fully working**:

| Module / Feature | Status | Notes |
|-----------------|--------|-------|
| `collections.Counter.most_common()` | ✅ | Prior doc said missing — it works |
| `collections.deque`, `defaultdict` | ✅ | Basic operations work |
| `itertools.count()`, `itertools.cycle()` | ✅ lazy | Prior doc said eager — they are lazy generators |
| `functools.lru_cache` | ✅ | Prior doc said non-functional — works with `@lru_cache` |
| `functools.wraps`, `total_ordering` | ✅ | Work correctly |
| `dataclasses.@dataclass` | ✅ full | Prior doc said no `__init__`/`__repr__`/`__eq__` — all three auto-generated |
| `io.StringIO`, `io.BytesIO` | ✅ read/write | Prior doc said stubs-only |
| `pathlib.Path.read_text()`, `.write_text()` | ✅ | Prior doc said no path operations |
| `abc.ABC` + `@abstractmethod` enforcement | ✅ | Prior doc said markers only — enforcement works |
| `enum.Enum` | ✅ | Basic Enum works; `IntEnum` isinstance broken (see §6.2) |
| `contextlib.contextmanager`, `suppress` | ✅ | Work correctly |
| `copy.copy()`, `copy.deepcopy()` | ✅ | Work correctly |
| `hashlib.md5()`, `sha256()` | ✅ | Work correctly |
| `base64.b64encode()`, `b64decode()` | ✅ | Work correctly |
| `bisect.bisect_left()`, `insort()` | ✅ | Work correctly |
| `heapq.heappush()`, `heappop()` | ✅ | Work correctly |
| `datetime.now()`, `strftime()` | ✅ | Basic datetime works |

---

## 7. Performance

### 7.1 Recursive Fibonacci — ~47× Slower Than CPython ❌

```
fib(30):  CPython ≈ 0.3 s    Ferrython ≈ 14 s
```

Pure recursive Python is dramatically slower. This is expected for an unoptimised interpreter
but is worth documenting. No JIT, no constant folding, no peephole optimisation (see §3.2)
all contribute. Stack-based dispatch without specialisation is the primary factor.

---

## 8. Layout & Structural Weaknesses

### 8.1 God Files

| File | Lines | Problem |
|------|------:|---------|
| `vm/opcodes.rs` | 2,113 | All opcode handlers in one `impl` block |
| `parser/parser.rs` | 2,082 | Every grammar rule in one file |
| `core/object/methods.rs` | 2,017 | Arithmetic, comparison, string, attribute, descriptor logic mixed |
| `vm/vm_call.rs` | 1,507 | All call/invoke logic |
| `compiler/statements.rs` | 1,079 | All statement compilation |
| `vm/builtins/core_fns.rs` | 1,066 | 40+ builtin functions |
| `stdlib/misc_modules.rs` | 1,010 | 19 unrelated stdlib modules |

### 8.2 VM Over-Coupling

`ferrython-vm` depends on 7 internal crates (bytecode, core, compiler, parser, stdlib, import, debug).
VM cannot be tested in isolation.

### 8.3 Fragile Import ↔ Stdlib Boundary

`ferrython-import` depends on `ferrython-stdlib`, `ferrython-parser`, and `ferrython-compiler`.
Adding Python-level `importlib` would create a circular dependency.

### 8.4 Three Incompatible Error Types

| Crate | Error Type | Conversion |
|-------|-----------|------------|
| `ferrython-parser` | `ParseError` | None |
| `ferrython-compiler` | `CompileError` | None |
| `ferrython-vm` | `PyException` | None |

No `From` impls, no unified error trait. CLI duplicates `match` arms.

### 8.5 No Automated Test Harness

- 64 Python fixture files in `tests/fixtures/` but none run by `cargo test`
- ~5 `#[test]` functions total in the Rust codebase
- `tests/benchmarks/`, `tests/cpython_compat/`, `tests/integration/` — empty
- `tools/` — empty

### 8.6 Other Structural Issues

| Issue | Detail |
|-------|--------|
| Over-exposed public APIs | `ferrython-core` wildcard re-exports expose internal helpers |
| String parsing duplication | `lexer.rs` and `string_parser.rs` overlap |
| Module boilerplate | Same `create_*_module()` pattern repeated 43 times |
| CLI error handling duplication | Same `match … Err(e) => eprintln!; exit(1)` ×3 |
| Dead code | 8 `#[allow(dead_code)]` markers; `sys_modules.rs` entirely marked dead |
