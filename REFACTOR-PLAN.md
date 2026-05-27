# Ferrython Code Quality Refactor Plan

Last updated: 2026-05-27

## Progress Snapshot

- Phase 1 guardrails are committed: `tools/code_health.py` and
  `CODE_HEALTH_BASELINE.md`.
- Phase 2 text module mechanical splits are mostly complete. The top-level
  `text_modules.rs` is now a small module/re-export shell; `regex_impl.rs`
  has been split internally into match objects, compiled/scanner methods,
  classes/flags, pattern conversion/validation helpers, `_sre` helpers, and
  module-level `re.*` functions. The regex pattern helper hotspot has been
  split into object/subject/pattern extraction, debug dump/output, regex
  conversion, error/group metadata, validation, and engine/flag helper files
  under `regex_impl/pattern/`; the root pattern file is now a small aggregation
  shell.
- Phase 2 introspection module mechanical splits are complete. The top-level
  `introspection_modules.rs` is now a small module/re-export shell.
- `_ast` implementation is being split internally into node/class helpers,
  Rust-AST-to-PyObject conversion, PyObject-to-Rust AST conversion helpers,
  utility APIs, and unparse support. The `ast_convert` path now separates
  context/argument validation from constant/operator/argument conversion
  helpers.
- Phase 2 serial module mechanical splits have retired the large
  `serial_modules/other.rs` bucket. Serialization modules now have named files
  for `base64`, `binascii`, `codecs`, `csv`, `dbm`, `json`, `marshal`,
  `pickle`, `shelve`, and `struct`. The remaining pickle hotspot has also
  been split internally into API, protocol reader, protocol writer, and shared
  helper files under `serial_modules/pickle_module/`.
- Phase 2 misc module mechanical splits are now complete at the top-level bucket.
  Low/medium-coupling
  `__future__`, `readline`, `runpy`, `compileall`, `pstats`, `quopri`,
  `stringprep`, `mimetypes`, `cmd`, `plistlib`, `curses`, `contextvars`, and
  `ctypes` implementations now live under `misc_modules/`, along with the
  remaining `contextlib`, `dataclasses`, `copy`, and `builtins` implementations.
- Phase 2 testing/debug module mechanical splits are complete at the top-level
  bucket. `logging`, `unittest`, `unittest.mock`, `doctest`, `pdb`, `profile`,
  `cProfile`, `timeit`, `faulthandler`, `tracemalloc`, `pydoc`,
  `logging.handlers`, `logging.config`, `pickletools`, and `_testcapi` now
  live under `testing_modules/`. The logging module has started internal
  layering with formatting/time helpers, small class/function factories, and
  handler/formatter class factories split under `testing_modules/logging/`;
  the root logging file has dropped out of the current longest-file list.
- Phase 2 system module mechanical splits have started. The low-coupling back
  half of `sys_modules.rs` now lives under `sys_modules/`: `platform`, `locale`,
  `getpass`, `errno`, `atexit`, `site`, `sched`, `mmap`, `resource`, `fcntl`,
  `sysconfig`, `grp`, `pwd`, `os.path`, sys stdio objects, and the `os` module
  body. The extracted `os` module has started internal layering with
  `terminal_size`, `stat_result`, `scandir`, `environ`, `PathLike` / `fspath`,
  and `walk` helpers split under `sys_modules/os/`. The remaining top-level
  `sys_modules.rs` owns `sys` state, sys module assembly, and
  exception/traceback display helpers.
- Phase 2 network module mechanical splits are now complete at the top-level
  bucket. The earlier low-coupling back half of
  `network_modules/http_module.rs` already lived under `http_module/`:
  `http.cookiejar`, `http.cookies`, `ssl`, `smtplib`, `ftplib`, `imaplib`,
  `poplib`, `cgi`, `xmlrpc`, and `socketserver`. `urllib.parse`,
  `urllib.request`, `http.client`, and `http.server` have now also been split
  into focused files under `http_module/`. The remaining root file is a small
  shell that keeps shared URL parsing/encoding helpers plus `http` /
  `HTTPStatus` assembly. The `socket` module has also started internal
  layering with socket object state/method closures and module-level DNS /
  connection helpers split under `network_modules/socket_module/`; the root
  socket file has dropped out of the current longest-file list.
- Phase 2 math module mechanical splits are complete at the top-level bucket.
  `statistics`, `numbers`, `decimal`, `random`, `heapq`, `bisect`,
  `fractions`, and `cmath` now live under `math_modules/`; the root file keeps
  the real `math` module and numeric conversion helpers.
- Phase 2 concurrency module mechanical splits have started. The low-coupling
  tail of `concurrency_modules.rs` now lives under `concurrency_modules/`:
  `gc`, `_thread`, `signal`, `multiprocessing`, `selectors`, and `select`.
  `threading` has now also been extracted into `concurrency_modules/threading.rs`.
  `weakref` has now been split into `concurrency_modules/weakref/` with
  separate `mod.rs`, `reference.rs`, `finalize.rs`, and `mappings.rs`.
  The larger `reference.rs` and `mappings.rs` internals have now also been
  mechanically split into nested helpers under `weakref/reference/` and
  `weakref/mappings/`.
  The remaining root file now keeps only shared deferred-call state plus
  submodule declarations/re-exports.
- Phase 2 collection module mechanical splits are complete at the top-level
  bucket. `UserDict`, `UserList`, `UserString`, `deque`, `ChainMap`,
  `namedtuple`, `defaultdict`, and `Counter` now live in focused
  `collection_modules/` files; `collections.rs` is a small assembly shell plus
  the remaining `OrderedDict` shim.
- Phase 2 filesystem/process module mechanical splits are complete at the
  top-level bucket. `subprocess`, `zlib`, `shutil`, `glob`, `tempfile`, `io`,
  and `pathlib` now live under `fs_modules/`; the root `fs_modules.rs` is a
  small child module declaration/re-export shell. The `io` module has started
  internal layering with StringIO, BytesIO, TextIOWrapper, and buffered wrapper
  helpers split under `fs_modules/io/`; the root `io` file has dropped out of
  the current longest-file list.
- Phase 2 time module mechanical splits have advanced. `zoneinfo`, `_strptime`,
  `datetime`, and shared calendar/format helpers now live under
  `time_modules/`; the root `time_modules.rs` now owns only the `time` module
  implementation and child module declarations/re-exports. The larger
  `time_modules/datetime.rs` file has started internal layering with
  date and timedelta construction/arithmetic/comparison helpers split under
  `time_modules/datetime/`.
- Phase 2 compression module mechanical splits are complete at the top-level
  bucket. `gzip`, `zipfile`, `bz2`, `lzma`, and `tarfile` now live under
  `compression_modules/`; the root `compression_modules.rs` is a small
  declaration/re-export shell plus the shared bytes-like extractor.
- Phase 2 config module mechanical splits are complete at the top-level bucket.
  `argparse` and `configparser` now live under `config_modules/`; the root
  `config_modules.rs` only declares child modules and re-exports the loaded
  `configparser` factory.
- Phase 2 type module mechanical splits have advanced. `typing`, `enum`,
  `types`, and `abc` now live under `type_modules/`; the root
  `type_modules.rs` keeps shared imports plus the remaining `collections.abc`
  implementation and re-exports the public module factories. The root file has
  dropped out of the current longest-file list.
- Phase 2 email module mechanical splits are complete at the top-level bucket.
  `email.message`, `email.mime.*`, `email.policy`, `email.contentmanager`,
  `email.charset`, `email.utils`, and `email.errors` now live under
  `email_modules/`; the root `email_modules.rs` keeps shared Message helpers,
  the top-level `email` package factory, and message parsing entry points. The
  root file has dropped out of the current longest-file list.
- Phase 2 XML module mechanical splits have started. The lower-coupling
  package, DOM/minidom, SAX, and expat helpers now live under `xml_modules/`;
  the root `xml_modules.rs` still owns the ElementTree parser/object
  implementation plus the `xml.etree.ElementTree` factory.
- Phase 2 database module internal splits are complete for sqlite3. The root
  `db_modules.rs` is now a sqlite3 module assembly shell, while storage,
  parser helpers, SQL execution, row objects, cursor objects, and connection
  objects live under `db_modules/`.
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
- `crates/ferrython-stdlib/src/collection_modules/counter.rs`,
  `namedtuple.rs`, and `user_types.rs` are medium-sized focused files after the
  top-level `collections.rs` bucket split; evaluate internal splits only if
  future changes need them.
- `crates/ferrython-stdlib/src/time_modules/datetime.rs` is about 1.7k lines
  after the top-level time split plus focused date/timedelta internal splits;
  evaluate more datetime/time/timezone splits only if further edits in that
  area justify the risk.
- `crates/ferrython-stdlib/src/type_modules.rs` has dropped out of the current
  top 25 after the first type module split; the remaining `collections.abc`
  body is about 1.2k lines and can be split internally later if needed.
- `crates/ferrython-stdlib/src/sys_modules/os.rs` is about 1.5k lines after
  internal helper splits and has dropped out of the current top 25 longest Rust
  files; good next targets are process/fd helpers only if future os edits need
  them.
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
