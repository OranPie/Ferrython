# Ferrython Code Quality Refactor Plan

Last updated: 2026-05-26

## Progress Snapshot

- Phase 1 guardrails are committed: `tools/code_health.py` and
  `CODE_HEALTH_BASELINE.md`.
- Phase 2 text module mechanical splits are mostly complete. The top-level
  `text_modules.rs` is now a small module/re-export shell; `regex_impl.rs`
  has been split internally into match objects, compiled/scanner methods,
  classes/flags, pattern conversion/validation helpers, `_sre` helpers, and
  module-level `re.*` functions.
- Phase 2 introspection module mechanical splits are complete. The top-level
  `introspection_modules.rs` is now a small module/re-export shell.
- `_ast` implementation is being split internally into node/class helpers,
  Rust-AST-to-PyObject conversion, utility APIs, and unparse support.
- Phase 2 serial module mechanical splits have retired the large
  `serial_modules/other.rs` bucket. Serialization modules now have named files
  for `base64`, `binascii`, `codecs`, `csv`, `dbm`, `json`, `marshal`,
  `pickle`, `shelve`, and `struct`.
- Phase 2 misc module mechanical splits are now complete at the top-level bucket.
  Low/medium-coupling
  `__future__`, `readline`, `runpy`, `compileall`, `pstats`, `quopri`,
  `stringprep`, `mimetypes`, `cmd`, `plistlib`, `curses`, `contextvars`, and
  `ctypes` implementations now live under `misc_modules/`, along with the
  remaining `contextlib`, `dataclasses`, `copy`, and `builtins` implementations.
- Phase 2 testing/debug module mechanical splits have started. Tail modules
  such as `doctest`, `pdb`, `profile`, `cProfile`, `timeit`, `faulthandler`,
  `tracemalloc`, `pydoc`, `logging.handlers`, `logging.config`,
  `pickletools`, `_testcapi`, and `unittest.mock` now live under
  `testing_modules/`.
- Latest focused validation for these mechanical Rust moves:
  `cargo check -p ferrython-stdlib`.

## Goal

Improve Ferrython's code quality and architecture while preserving CPython
compatibility and keeping performance regressions visible. The refactor should
reduce oversized files, clarify module responsibilities, and separate VM/core
architecture concerns from compatibility fixes.

## Current Hotspots

- `crates/ferrython-vm/src/vm.rs` is over 10k lines and mixes dispatch macros,
  the hot opcode loop, superinstruction implementations, call shortcuts,
  exception unwinding, and fallback opcode handling.
- `crates/ferrython-vm/src/vm_call.rs` is over 7k lines and combines function
  calls, native calls, class instantiation, descriptors, `super()`, and
  frameless call optimizations.
- `crates/ferrython-stdlib/src/text_modules.rs` is nearly 10k lines and hosts
  many unrelated modules such as `string`, `re`, `_sre`, `textwrap`, `fnmatch`,
  `html`, `shlex`, `unicodedata`, `pprint`, and `encodings`.
- `crates/ferrython-stdlib/src/introspection_modules.rs` is nearly 8k lines and
  combines `warnings`, `traceback`, `inspect`, `dis`, `_ast`, `linecache`,
  `token`, `tokenize`, and `symtable`.
- `crates/ferrython-core/src/object/payload.rs` mixes low-level allocation,
  weakref registries, compact string representation, object references, class
  and instance data, iterator data, and the payload enum.
- `crates/ferrython-core/src/object/methods_attr.rs` centralizes class lookup,
  descriptor rules, builtin method dispatch, weakdict special cases, and AST
  compatibility attributes.

## Refactor Principles

- Keep behavior stable. Prefer mechanical file movement before semantic changes.
- Keep hot VM dispatch as a direct `match` unless a measured alternative is
  clearly better.
- Split code around ownership boundaries: VM dispatch, call protocol, object
  model, stdlib module creation, and test/debug tooling.
- Use focused checks during iteration. Use release/LTO builds only when the
  change is explicitly performance-sensitive or a phase is ready for validation.
- Commit each coherent phase separately with the validation performed.
- Do not commit `.codex-work/`.

## Phase 1: Guardrails

1. Add a code-health inspection tool that reports:
   - Longest Rust/Python files.
   - `match` density by file.
   - Function/item density by file.
   - Candidate oversized modules.
2. Add or update a generated report artifact that can be reviewed before and
   after each refactor phase.
3. Use `cargo check` for mechanical movement and focused runtime tests for VM or
   core architecture changes.

## Phase 2: Mechanical Stdlib Splits

Split large stdlib implementation buckets without changing public factory APIs.

Target `text_modules` layout:

- `crates/ferrython-stdlib/src/text_modules/mod.rs`
- `string.rs`
- `regex.rs`
- `sre.rs`
- `textwrap.rs`
- `fnmatch.rs`
- `html.rs`
- `shlex.rs`
- `unicodedata.rs`
- `pprint.rs`
- `encodings.rs`
- small shared helpers when required

Target `introspection_modules` layout:

- `crates/ferrython-stdlib/src/introspection_modules/mod.rs`
- `warnings.rs`
- `traceback.rs`
- `inspect.rs`
- `dis.rs`
- `ast.rs`
- `linecache.rs`
- `token.rs`
- `tokenize.rs`
- `symtable.rs`
- small shared helpers when required

Validation:

- `cargo check -p ferrython-stdlib`
- Focused import smoke tests for moved modules through `target/debug/ferrython`.

## Phase 3: Stdlib Module Registry

Replace the central `load_module()` mega-match with a grouped registry while
preserving module names and creation functions.

Initial target:

- `stdlib::registry::{math, system, text, collections, serialization, fs, time,
  types, introspection, concurrency, network, import, misc}`
- Static slices of `ModuleSpec { name, create }`.
- A single top-level resolver that iterates grouped slices.

Do not optimize with `phf` until behavior is stable and profiling indicates a
real import-resolution cost.

## Phase 4: VM Dispatch Slimming

Keep the main opcode dispatch loop direct, but move bulky arm bodies into
specialized helpers:

- `dispatch/stack.rs`
- `dispatch/iter.rs`
- `dispatch/call.rs`
- `dispatch/attrs.rs`
- `dispatch/compare.rs`
- `dispatch/superinstructions.rs`
- `dispatch/macros.rs`

Priority extraction targets:

- `ForIter` and `ForIterStoreFast`.
- `CallFunction`, `LoadGlobalCallFunction`, `CallMethod`, and
  `CallMethodPopTop`.
- `LoadAttr`, `StoreAttr`, `LoadFastLoadAttr`, and `LoadFastLoadMethod`.
- `CompareOp` and compare/jump superinstructions.

Validation:

- `cargo build -p ferrython-cli --bin ferrython -j6`
- Focused CPython tests around iteration, calls, attrs, exceptions, and compare.
- CPython baseline comparisons only for performance-sensitive changes.

## Phase 5: VM Call Architecture

Split `vm_call.rs` into modules with clearer boundaries:

- `call/object.rs`: public `call_object` entry and shared orchestration.
- `call/function.rs`: Python function and frame preparation.
- `call/native.rs`: native functions and closures.
- `call/class.rs`: class construction and `__new__` / `__init__` flow.
- `call/descriptor.rs`: descriptor binding and method resolution.
- `call/frameless.rs`: frameless and inlined Python function shortcuts.
- `call/super_object.rs`: `super()` behavior.

Introduce internal structures only where they reduce repeated branching:

- `PreparedCall`
- `CallTarget`
- `CallArgs`

This phase has higher semantic risk and should be split into several commits.

## Phase 6: Core Object Boundary Cleanup

Split low-level object code by responsibility:

- `object/cell.rs`: `PyCell`, read/write guards.
- `object/str_repr.rs`: compact string representation.
- `object/alloc.rs`: object block allocation, freelists, refcount operations.
- `object/weakref.rs`: weakref object registry and weak reference primitives.
- `object/class.rs`: `ClassData` and class versioning.
- `object/instance.rs`: `InstanceData`.
- `object/iterator.rs`: iterator payload data.
- `object/payload.rs`: the `PyObjectPayload` enum and payload-level drop/debug.

Then split attribute protocol code:

- `object/attrs/lookup.rs`
- `object/attrs/descriptor.rs`
- `object/attrs/builtin_methods.rs`
- `object/attrs/weakdict.rs`
- `object/attrs/ast.rs`

Validation:

- `cargo check -p ferrython-core -p ferrython-vm -p ferrython-stdlib`
- Focused weakref, copy, deque, dict/set iterator, and AST tests.

## Phase 7: Final Validation

After each phase:

- Update this plan with completed items and notes if the actual path changes.
- Commit the coherent change set.

At the end:

- Run broad focused CPython compatibility candidates selected from recent fixes.
- Run a release performance comparison only for VM/core performance-affecting
  changes.
- Record remaining risks and next candidates.
