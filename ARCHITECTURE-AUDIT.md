# Ferrython Architecture Audit — Simplified & Non-Unified Patterns

> Generated 2026-04-03. Tracks every place where the implementation uses
> ad-hoc shortcuts instead of CPython's logical, protocol-driven design.

---

## Legend

| Tag | Meaning |
|-----|---------|
| 🔴 CRITICAL | Blocks correctness of major features |
| 🟠 HIGH | Causes silent wrong behaviour or prevents extensibility |
| 🟡 MEDIUM | Works but fragile / duplicated / hard to maintain |
| 🟢 OK | Intentional simplification that is logically sound |

---

## 1. Object System — `payload.rs`, `methods_attr.rs`

### 1.1 BuiltinBoundMethod stores method **name** not method object 🟠

**Where:** `PyObjectPayload::BuiltinBoundMethod { receiver, method_name: CompactString }`

**Problem:** Method resolution is deferred to call-time by matching a raw string in
`vm_call.rs` (600+ lines of hand-rolled dispatch). CPython resolves the actual
callable at attribute-access time via the descriptor protocol.

**Consequence:** Every new builtin method with keyword args requires editing
*two files* (`methods_attr.rs` to expose it, `vm_call.rs` to dispatch it).

### 1.2 Instance type detection via magic marker attributes 🟠

**Where:** `methods_attr.rs` `instance_builtin_method()` lines 72–207

**Problem:** Deque is identified by `__deque__`, StringIO by `__stringio__`,
pathlib.Path by `__pathlib_path__`, etc. — all sentinel attributes manually
inserted at construction. CPython uses `isinstance()` against the actual class.

**Consequence:** Every new stdlib "native instance" type needs a new marker string
and a new hardcoded branch. Breaks normal introspection.

### 1.3 Attribute lookup bypasses descriptor protocol 🟠

**Where:** `py_get_attr()` in `methods_attr.rs`

**CPython order:**
1. `type.__getattribute__` → data descriptor → instance `__dict__` → non-data descriptor
2. `__getattr__` fallback

**Ferrython:** Flat match-on-payload, only handles `Property` as a data descriptor.
Custom `__get__`/`__set__`/`__delete__` descriptors are not discovered. `__getattr__`
is checked separately in `opcodes.rs LoadAttr`, not inside `py_get_attr`.

### 1.4 InstanceDict wrapper variant 🟡

**Where:** `PyObjectPayload::InstanceDict(...)`

A wrapper that exists only so `instance.__dict__` returns a live view.
CPython exposes the same underlying mapping; no extra enum variant needed.

---

## 2. Generator / Coroutine / AsyncGenerator — `payload.rs`, `methods_attr.rs`, `vm_call.rs`

### 2.1 All three types share an identical `GeneratorState` struct 🟡

**Where:** `GeneratorState { name, frame: Box<dyn Any>, started, finished }`

This is *intentionally* unified at the frame level (good), but the type has
no per-kind metadata: no `cr_await` (what a coroutine is currently awaiting),
no `ag_await`, no `gi_yieldfrom`.

### 2.2 AsyncGenerator missing `__aiter__` / `__anext__` ~~🔴~~ 🟢 FIXED

**Where:** `vm_call.rs` — `__aiter__` returns self, `__anext__` resumes generator

AsyncGenerator now supports `async for` loops correctly via `__aiter__` and
`__anext__` dispatch in the bound-method call handler.

### 2.3 AsyncGenerator missing `asend` / `athrow` / `aclose` 🟡

**Where:** `vm_call.rs:1631-1662`

CPython's async generator protocol has three async-aware methods that each
return an awaitable coroutine. Ferrython exposes sync `send/throw/close`
plus `__aiter__`/`__anext__`. The sync variants work for sequential async.

### 2.4 `close()` does not raise `GeneratorExit` into frame ~~🔴~~ 🟢 FIXED

**Where:** `vm_call.rs:1765-1795`

Generator `close()` now properly throws `GeneratorExit` via `gen_throw()`,
executing `finally` blocks. Verified with try/finally cleanup tests.

### 2.5 Generator/Coroutine frame attributes are stubs 🟡

**Where:** `methods_attr.rs:839-841`

`gi_frame`, `cr_frame`, `ag_frame` → always `None`.
`gi_running`, `cr_running`, `ag_running` → always `False`.
Makes debugging and introspection of running coroutines impossible.

### 2.6 ASYNC_GENERATOR flag logic uses COROUTINE+GENERATOR combo 🟡

**Where:** `compiler/statements.rs:437-448`, `vm_call.rs:128-145`

CPython sets `CO_ASYNC_GENERATOR` directly (not a combination).
Ferrython infers async-generator by checking both flags simultaneously.
Works today but is fragile.

---

## 3. Async Opcodes — `opcodes.rs`

### 3.1 `GetAwaitable` accepts raw Generators 🟡

**Where:** `opcodes.rs:2624-2625`

CPython only accepts generators wrapped with `@types.coroutine`.
Ferrython accepts any `Generator` variant, which means accidental `await gen()`
on a regular generator won't raise `TypeError`.

### 3.2 `BeforeAsyncWith` / `SetupAsyncWith` do not await `__aexit__` on cleanup 🔴

**Where:** `opcodes.rs:2670-2692` (BeforeAsyncWith), `opcodes.rs:2296-2318` (SetupAsyncWith)

`__aenter__` is properly awaited (via GetAwaitable + YieldFrom after
BeforeAsyncWith). But when `WithCleanupStart` calls `__aexit__`, it does a
synchronous `call_object` — if `__aexit__` is async (returns a coroutine),
the coroutine is never driven.

### 3.3 `EndAsyncFor` does not distinguish `StopAsyncIteration` from other exceptions 🟡

**Where:** `opcodes.rs:2694-2701`

Just pops two stack items regardless. Should re-raise if the exception is not
`StopAsyncIteration`.

### 3.4 `SetupAsyncWith` reuses `BlockKind::With` instead of `BlockKind::AsyncWith` 🟡

**Where:** `opcodes.rs:2314`

Means unwind logic cannot distinguish sync-with from async-with cleanup.

---

## 4. Function Calling — `vm_call.rs`

### 4.1 `call_object` and `call_object_kw` are not unified 🟠

**Where:** `vm_call.rs` — `call_object` (884 lines) vs `call_object_kw` (390 lines)

Two near-duplicate mega-functions. `call_object` has its own inline kwarg
special-cases (e.g. `functools.partial`, `re.sub`, `itertools.groupby`).
CPython has a single `_PyObject_FastCallDict`.

### 4.2 NativeFunction kwargs passed as trailing dict 🟡

**Where:** `vm_call.rs:926-934`

```rust
all_args.push(PyObject::dict(kw_map)); // trailing dict = kwargs
```

A convention, not a protocol. Every NativeFunction must know to pop the last
arg and treat it as a dict. Fragile and undocumented.

### 4.3 BuiltinFunction dispatch is a 120-line match on name string 🟠

**Where:** `vm_call.rs:728-850`

`sorted`, `enumerate`, `zip`, `filter`, `map`, `__build_class__`, etc.
are dispatched by name. Should be resolved to actual function objects at
module-load time.

---

## 5. Exception Handling — `opcodes.rs`

### 5.1 `SetupWith` has a separate code path for Generator context managers 🟡

**Where:** `opcodes.rs:2249-2293`

If the context manager is a `Generator`, it resumes it directly instead of
calling `__enter__`. CPython's `contextlib.contextmanager` creates objects
with real `__enter__`/`__exit__`.

### 5.2 `WithCleanupStart` does not handle Coroutine/AsyncGenerator exit fns 🔴

**Where:** `opcodes.rs:2327-2376`

Only handles the Generator case (resume) and sync call_object. If the
`__exit__` or `__aexit__` function is itself a coroutine, result is wrong.

### 5.3 `EndFinally` extracts exception via stack layout, not exception state 🟡

**Where:** `opcodes.rs:2121-2156`

Manually peeks at stack for `ExceptionType` vs `Class` variants. CPython uses
`sys.exc_info()` / frame exception state, not ad-hoc stack inspection.

---

## 6. Import System — `ferrython-import/lib.rs`, `opcodes.rs`

### 6.1 `sys.modules` is disconnected from VM module cache ~~🔴~~ 🟡

**Where:** `sys_modules.rs` (separate dict) vs `VirtualMachine.modules` (IndexMap)

`sys.modules` is now populated and readable. Writing to `sys.modules` has
limited effect on future imports (VM uses its own cache first). Reads work
correctly (`"math" in sys.modules` returns True after import).

### 6.2 `__import__` builtin is disabled ~~🔴~~ 🟢 FIXED

**Where:** `core_fns.rs` — uses `ImportRequest` + `post_call_intercept()`

`__import__()` works via a thread-local request that the VM intercepts after
the NativeFunction call returns, resolving via `import_module_simple()`.
Verified: `builtins.__import__("math")` returns the math module correctly.

### 6.3 Module metadata incomplete 🟠

**Where:** `constructors.rs:197-214`

`__file__`, `__spec__`, `__loader__`, `__package__` all set to `None`.
Breaks relative imports, debugging, and module introspection.

### 6.4 Circular import not protected 🟠

**Where:** `opcodes.rs:2006-2028`

Module is cached *after* execution completes. If module A imports B which
imports A, the second `import A` won't find it in cache and re-executes.

### 6.5 Stdlib module registry is a hardcoded match statement 🟡

**Where:** `ferrython-stdlib/src/lib.rs:22-88` — 40+ `match name { "math" => ..., }`

Adding a module requires a code change and recompile.

### 6.6 No `sys.meta_path` / `sys.path_hooks` mechanism 🟡

**Where:** `sys_modules.rs:108-109` — empty lists, never consulted

---

## 7. Stdlib — `misc_modules.rs`

### 7.1 Deferred-call mechanism for Python→Rust→Python calls 🟡

**Where:** `misc_modules.rs:17-29`

Thread-local queue drained by VM after NativeClosure returns.
Works but is a workaround for NativeClosure not having VM access.

### 7.2 Threading.Thread does not spawn OS threads 🟡

**Where:** `misc_modules.rs:1800-1815`

`start()` pushes to the deferred-call queue; VM runs it sequentially.
Correct semantics require actual thread spawning or a GIL-like model.

### 7.3 Instance "methods" use shared-state closures instead of descriptors 🟡

**Where:** `misc_modules.rs` throughout (StreamHandler, Lock, Event, Thread)

Because `LoadAttr` does not inject `self` for `NativeFunction` attrs on
instances, every stateful stdlib object must use `Arc<RwLock<T>>` closures.
CPython uses the descriptor protocol + `self` injection automatically.

---

## Summary: Severity Counts

| Severity | Count | Key Areas |
|----------|-------|-----------|
| 🔴 CRITICAL | 3 | Async-with aexit, WithCleanupStart coroutine, sys.modules write-through |
| 🟠 HIGH | 6 | BuiltinBoundMethod name dispatch, marker attrs, descriptor bypass, call duplication |
| 🟡 MEDIUM | 12+ | Generator metadata stubs, NativeFunction kwargs, deferred calls, threading stubs |
| 🟢 FIXED | 4 | AsyncGen __aiter__/__anext__, close() GeneratorExit, __import__, sys.modules reads |
| 🟠 HIGH | 6 | Descriptor protocol, instance markers, call dispatch, module metadata, circular imports |
| 🟡 MEDIUM | 12 | Stubs, flag logic, deferred calls, generator attrs, hardcoded registries |
| 🟢 OK | — | GeneratorState reuse, basic opcode structure |

---

## Prioritised Fix Order

1. **AsyncGenerator protocol** (`__aiter__`, `__anext__`, `asend`, `athrow`, `aclose`) — unblocks async-for
2. **Generator/Coroutine `close()`** — raise `GeneratorExit`, execute `finally` blocks
3. **Async-with `__aexit__` awaiting** — drive returned coroutine in WithCleanupStart
4. **`EndAsyncFor` exception filtering** — re-raise non-`StopAsyncIteration`
5. **`sys.modules` ↔ VM cache sync** — live view or shared backing store
6. **`__import__` builtin** — route to VM's import machinery
7. **Module metadata** — set `__file__`, `__package__`, compute `__spec__`
8. **Circular import protection** — cache module *before* executing body
9. **asyncio event loop** — `asyncio.run`, `sleep`, `gather`, `create_task`
10. **Import hooks** — `sys.meta_path` finders, `importlib.import_module`
