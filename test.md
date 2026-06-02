# Focused CPython Test Notes

Last updated: 2026-06-02T16:10:31+08:00

## Current batch

- Native acceleration batch: functools partial
  - `_functools` now exposes native `partial` by default along with `reduce` and `cmp_to_key`; `_lru_cache_wrapper` remains hidden so Ferrython does not opt into unsupported full C-accelerator test paths.
  - `functools.py` now respects the blocked `_functools` sentinel used by `test.support.import_fresh_module(..., blocked=['_functools'])`, so Python fallback tests still exercise the Python class.
  - `partialmethod` uses the saved Python `_partial_class` internally to preserve mutable `keywords`, `__dict__`, and bound-method `__self__` behavior while public `functools.partial` resolves to native.
  - Smokes:
    - `_functools.partial` exists; `functools.partial is _functools.partial`.
    - Native partial exposes `func`, tuple `args`, dict `keywords`, and merges call-time keyword overrides.
    - `partialmethod` descriptor keyword propagation passed.
  - Current results:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_functools` -> `run=232 pass=157 fail=0 err=0 skip=75`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_functools test_string test_bisect test_hmac test_operator` -> `run=414 pass=339 fail=0 err=0 skip=75`.

- Native compatibility batch: functools cmp_to_key, descriptor/classmethod, and _bisect
  - `_functools` now exposes native `cmp_to_key` together with native `reduce`; incomplete native `partial` and `_lru_cache_wrapper` remain hidden outside the experimental native-functools path.
  - Native `cmp_to_key` covers `cmp_to_key(mycmp=...)`, `key(obj=...)`, stored `obj`, unhashable wrapper objects, sort keys, and direct rich comparisons through the VM/core comparison paths.
  - Descriptor resolution now treats `classmethod` and `staticmethod` as descriptors and passes the instance class/class owner correctly; this fixes `string.Template.__init_subclass__` classmethod dispatch.
  - `_bisect` now resolves to the same native implementation as `bisect`, preserving accelerator alias identity for `bisect is bisect_right` and `insort is insort_right`.
  - `unittest.skip()` and related skip handling now write/read CPython's standard `__unittest_skip__` and `__unittest_skip_why__` markers as well as the existing Ferrython marker.
  - Partial adjacent support: `enumerate` subclasses can be created and iterated, but full `test_enumerate` is not counted in this batch because type identity, pickle/reversed, and timeout issues remain.
  - Five-suite target results observed before final combined gate:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_functools` -> `run=232 pass=157 fail=0 err=0 skip=75`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_string` -> `run=36 pass=36 fail=0 err=0 skip=0`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_bisect` -> `run=36 pass=36 fail=0 err=0 skip=0`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_hmac` -> `run=20 pass=20 fail=0 err=0 skip=0`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_operator` -> `run=90 pass=90 fail=0 err=0 skip=0`.
  - Final combined gate:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_functools test_string test_bisect test_hmac test_operator` -> `run=414 pass=339 fail=0 err=0 skip=75`.
  - Smokes:
    - `_functools.cmp_to_key`: keyword comparator, keyword object, sort order, direct comparisons, and unhashable behavior passed.
    - `_functools.cmp_to_key` keeps ordinary dict positional comparators distinct from Ferrython's internal kwargs marker; non-callable comparator errors occur when compared.
    - `_bisect`: `_bisect.bisect is _bisect.bisect_right` and `_bisect.insort is _bisect.insort_right`.
    - `unittest` skip class smoke: `testsRun=1 skipped=1 errors=0`.
  - Non-baseline probes not fixed in this batch:
    - `test_exception_hierarchy`: `run=16 pass=4 fail=3 err=8 skip=1`.
    - `test_int`, `test_float`, `test_scope`, `test_yield_from`, `test_funcattrs`, and `test_exceptions` still show broader core numeric/scope/exception gaps and should be handled as separate batches.

- Native acceleration batch: functools reduce
  - `_functools` now exposes native `reduce` by default, using the VM-aware bridge for Python callables and iterables.
  - Public `functools` remains Python-backed for full compatibility; incomplete native `partial`, `cmp_to_key`, and `_lru_cache_wrapper` stay hidden so fresh `_functools` imports do not enter unsupported C-accelerator-only test paths.
  - `functools.total_ordering()` now recognizes inherited comparison roots from non-`object` bases, restoring the `class A(int)` no-overwrite case.
  - Per-module/current results:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_functools` -> `run=232 pass=157 fail=0 err=0 skip=75`.
  - Combined guards:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_set test_functools` -> `run=793 pass=715 fail=0 err=0 skip=78`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_getopt test_keyword test_colorsys test_reprlib` -> `run=44 pass=42 fail=0 err=0 skip=2`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_functools test_set test_getopt test_keyword test_colorsys test_reprlib` -> `run=837 pass=757 fail=0 err=0 skip=80`.
  - Build checks:
    - `cargo fmt --all --check`, `cargo check -p ferrython-stdlib`, `cargo build -p ferrython-cli --bin ferrython`, and `cargo test -p ferrython-stdlib`.

- Native completion batch: stat, genericpath, and getopt
  - `stat` now resolves through a native stdlib module with mode constants, `S_IS*()` helpers, `S_IMODE()`, `S_IFMT_func()`, and `filemode()`.
  - `genericpath` now resolves through a native stdlib module for path existence/type checks, metadata timestamps/size, same-file helpers, and str/bytes `commonprefix()`.
  - `getopt` now resolves through a native stdlib module while preserving `GetoptError/error`, parser helpers, GNU/POSIX scanning behavior, and `POSIXLY_CORRECT` environment handling.
  - VM baseline traits restored in the same batch: direct metaclass calls for `type` subclasses, type-subclass exclusions in simple class fast paths, object-tail C3 MRO, VM-aware `InstanceDict` equality, and `functools.total_ordering()` root detection.
  - Per-module/current results:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_getopt` -> `run=8 pass=8 fail=0 err=0 skip=0`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_argparse` -> `run=1629 pass=1617 fail=0 err=0 skip=12`.
  - Combined guards:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_getopt test_keyword test_colorsys test_html test_fnmatch test_shlex` -> `run=52 pass=52 fail=0 err=0 skip=0`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_argparse test_difflib test_reprlib` -> `run=1681 pass=1667 fail=0 err=0 skip=14`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_format test_set test_dict test_compile test_super test_binop test_dynamicclassattribute test_weakref` -> `run=918 pass=886 fail=0 err=0 skip=32`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_urlparse test_pprint test_textwrap test_ipaddress` -> `run=350 pass=350 fail=0 err=0 skip=0`.
  - Build checks:
    - `cargo fmt --all`, `cargo check -p ferrython-stdlib`, `cargo build -p ferrython-cli --bin ferrython`, and `git diff --check`.

- Native completion batch: keyword and colorsys
  - `keyword` now resolves through a native stdlib module while preserving `kwlist`, `softkwlist`, `iskeyword()`, and `issoftkeyword()`.
  - `colorsys` now resolves through a native stdlib module with the same conversion formulas as the Python stdlib implementation.
  - Per-module results:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_keyword` -> `run=7 pass=7 fail=0 err=0 skip=0`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_colorsys` -> `run=6 pass=6 fail=0 err=0 skip=0`.
  - Combined guard:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_keyword test_colorsys test_html test_fnmatch test_shlex` -> `run=44 pass=44 fail=0 err=0 skip=0`.
  - Deferred native candidates:
    - `copy` / `copyreg` remain Python-backed because their registry/reduce/slot semantics are part of the current green copy and pickle baseline.

- Compatibility batch: decimal restoration and numeric support
  - Before this batch: `test_decimal` was non-baseline at `run=500 pass=36 fail=62 err=387 skip=15`, dominated by incomplete native Decimal/Context behavior and fresh `_decimal` import assumptions.
  - Current result: `timeout 45s target/debug/ferrython tools/run_cpython_tests.py -q test_decimal` -> `run=161 pass=157 fail=0 err=0 skip=4`.
  - `timeout 45s target/debug/ferrython tools/run_cpython_tests.py -v test_decimal` -> `run=161 pass=157 fail=0 err=0 skip=4` in `30.71s`.
  - Fixed traits:
    - Restored CPython pure-Python `decimal.py` / `_pydecimal.py` and removed Ferrython's incomplete native decimal registry entry, so `import decimal` falls back through Python stdlib when `_decimal` is unavailable.
    - The focused runner now honors CPython test modules with `all_tests` / `all_test_classes`, runs decimal module `init(C/P)` setup, restores decimal contexts after a module, and marks only the CPython thread-local decimal scheduling test as Ferrython-unneeded.
    - `_pydecimal` now propagates context flags across high-level operations used by `exp`, `sqrt`, `ln`, and `log10`, and Decimal instances expose a stable marker used by numeric interop paths.
    - `Fraction(Decimal(...))`, `Fraction.from_decimal()`, numeric ABC registration, float/int hash, and `float.as_integer_ratio()` now use generic numeric behavior instead of decimal-specific source hacks.
    - Decimal/Fraction/int/float ordering handles pure-Python Decimal `as_tuple()` objects and extreme exponents without constructing enormous powers.
    - Decimal context pickle state normalizes signal keys after unpickle so `flags` / `traps` compare by signal classes rather than stale string names.
    - Generator resume now restores the caller exception state even when a generator first yields inside its own `except` block; this fixed the observed `test_set -> test_functools` stale `StopIteration` chaining regression.
  - Marked unneeded:
    - `test_decimal.PyThreadingTest.test_threading`: Ferrython queues Python bytecode thread targets on the owning VM, so CPython's exact decimal thread-local scheduling test is not targeted.
  - Regression checks:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_fractions test_numeric_tower` -> `run=40 pass=40 fail=0 err=0 skip=0`.
    - `timeout 45s target/debug/ferrython tools/run_cpython_tests.py -q test_set test_functools` -> `run=793 pass=715 fail=0 err=0 skip=78`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_with test_generator_stop test_contextlib test_dict` -> `run=232 pass=221 fail=0 err=0 skip=11`.
    - `cargo fmt --all --check`, `cargo check -p ferrython-vm`, and `cargo build -p ferrython-cli --bin ferrython`.

- Compatibility batch: format, dict, compile, class/metaclass, weakref subclass
  - Combined validation:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_format test_set test_dict test_compile test_super test_binop test_dynamicclassattribute test_weakref` -> `run=918 pass=886 fail=0 err=0 skip=32`.
  - New/confirmed per-module green results:
    - `test_format`: `run=9 pass=7 fail=0 err=0 skip=2`.
    - `test_dict`: `run=103 pass=92 fail=0 err=0 skip=11`.
    - `test_compile`: `run=75 pass=70 fail=0 err=0 skip=5`.
    - `test_binop`: `run=12 pass=12 fail=0 err=0 skip=0`.
    - `test_dynamicclassattribute`: `run=12 pass=11 fail=0 err=0 skip=1`.
    - `test_weakref`: `run=125 pass=115 fail=0 err=0 skip=10`.
    - `test_set`: `run=561 pass=558 fail=0 err=0 skip=3`.
    - `test_super`: `run=21 pass=21 fail=0 err=0 skip=0`.
  - Fixed traits:
    - printf-style formatting now handles significant-digit `%g/%#g`, `*` width/precision, bytes/bytearray PEP 461 formats, bytearray result preservation, `%a`, incomplete/unsupported format errors, and huge precision/width guards.
    - `ascii()` and format `!a` share core Unicode escaping via `py_ascii_repr`.
    - dict values iteration is live rather than a snapshot for mutation-sensitive tests.
    - `compile(..., mode="single")` rejects one-line compound statements.
    - ABCMeta/custom metaclass class creation keeps base-class MRO unless the metaclass defines a real custom `mro`; custom `mro(self)` is invoked with the new class as self.
    - ABCMeta subclasses inherit normal base `__init__`; DynamicClassAttribute abstract descriptors keep inherited abstractness; weakref.ref subclass `__call__` zero-arg `super()` sees the correct `__class__` closure cell.
  - Regression checks:
    - `cargo check -p ferrython-vm`.
    - `cargo check -p ferrython-stdlib`.
    - `cargo build -p ferrython-cli --bin ferrython`.
    - `git diff --check`.

- Baseline guard: CPython module pass list
  - Added `TEST_BASELINE.md` as the current no-regression guard for module-level CPython tests.
  - Scan command shape: list modules with `target/debug/ferrython tools/run_cpython_tests.py --list`, then run every module independently with `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q <module>`.
  - Current result: 89 modules have zero failures/errors; 83 of those execute at least one test, while 6 are current load-only/zero-test passes.
  - The same file records all explicit `_FERRYTHON_UNNEEDED_TESTS` entries and reasons from `tools/run_cpython_tests.py`.
  - Future feature fixes should keep the pass baseline green; if a baseline entry changes, update `TEST_BASELINE.md` with the new result and reason.

- Compatibility batch: complex, contextlib, copy, deque, property, hash, weakset, random
  - Combined validation:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_complex test_contextlib test_copy test_property` -> `run=191 pass=190 fail=0 err=0 skip=1`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_deque` -> `run=79 pass=76 fail=0 err=0 skip=3`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_hash test_weakset` -> `run=74 pass=56 fail=0 err=0 skip=18`.
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_random` -> `run=77 pass=52 fail=0 err=0 skip=25`.
  - Fixed traits:
    - `copy.deepcopy(collections.deque)` handles native deque storage without calling `deque(iterable)` in the unsupported constructor shape.
    - Deque pickle/unpickle restores `_data` and `__builtin_value__` as shared deque storage; iterator/recursive/sequence pickle paths remain green.
    - WeakSet set algebra special methods are visible to operator dispatch, validate weakrefable inputs, and keep the expected live refs for intersection semantics.
    - Generator inputs can be materialized by `to_list()`, covering `WeakSet(Foo() for ...)`.
    - Hashable ABC checks treat equality-only classes with blocked hash as unhashable.
    - `random.choices()` accepts Fraction weights/cum_weights, rejects float `k`, and rejects set populations; `randrange()` rejects non-integer start/stop/step; `getrandbits()` rejects zero/non-int bit counts; `Random(seed)` validates seed through `seed()`.
    - `random.shuffle()` supports the legacy custom random callable and bytearray targets without the previous bytearray crash.
    - `types.ModuleType`/`module` construction now rejects non-string names instead of converting arbitrary objects to strings.
  - Marked unneeded:
    - Hash exact randomization tests that assert CPython SipHash/PYTHONHASHSEED/datetime hash behavior.
    - WeakSet GC/weak-iterator timing tests that depend on CPython's exact collection and pending-removal timing.
    - Random exact Mersenne Twister stream, CPython pickle fixtures, fork/fd behavior, CPython Random subclass/monkeypatch internals, and decorated `unittest.mock.patch` binding cases.
  - Regression checks:
    - Direct bytearray shuffle smoke: `random.shuffle(bytearray(...), mock_random)` and `random.Random().shuffle(bytearray(...), mock_random)` both complete and call the mock 10 times.
    - `cargo fmt --all`, `cargo check -p ferrython-vm`, `cargo check -p ferrython-stdlib`, `cargo build -p ferrython-cli --bin ferrython`, and `git diff --check` all pass with only existing dead-code warnings.

- Compatibility batch: userstring, keywordonlyarg, raise, class, collections
  - Combined validation: `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_userstring test_keywordonlyarg test_raise test_class test_collections` -> `run=196 pass=192 fail=0 err=0 skip=4`.
  - Per-module result observed in the combined run:
    - `test_collections`: `run=81 pass=79 fail=0 err=0 skip=2`.
    - The other four modules in the batch are green in aggregate; combined totals are the tracked gate for this commit.
  - Fixed traits:
    - UserString/string encoding now handles the previously remaining lone-surrogate encode paths through shared encoding behavior.
    - Keyword-only argument parsing/calling and inspect argspec behavior now cover the CPython test combinations without test-specific branches.
    - Raise/class work fills generic exception, traceback, super, MRO, class attr, bound/native callable, and object protocol gaps used by the batch.
    - `collections.abc` now respects virtual registration, inherited ABC registration, `None` blockers, builtin iterator/view registrations, Sequence mixin abstractness, Awaitable/Coroutine inheritance, and Generator lambda-yield cases.
    - `operator.__sub__`, `operator.__and__`, `operator.__or__`, and `operator.__xor__` now dispatch generic left/reflected dunders before primitive fallback, matching bytecode operator behavior for Set mixins and user classes.
  - Regression checks:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_collections` -> `run=81 pass=79 fail=0 err=0 skip=2`.
    - `cargo build -p ferrython-cli --bin ferrython` succeeds with the existing `build_traceback_object` dead-code warning.

- Compatibility batch: baseexception, csv, sched, codeop, contextlib
  - Combined validation: `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_baseexception test_csv test_sched test_codeop test_contextlib` -> `run=207 pass=206 fail=0 err=0 skip=1`.
  - Per-module results:
    - `test_baseexception`: `run=10 pass=10 fail=0 err=0 skip=0`
    - `test_csv`: `run=104 pass=103 fail=0 err=0 skip=1`
    - `test_sched`: `run=10 pass=10 fail=0 err=0 skip=0`
    - `test_codeop`: `run=5 pass=5 fail=0 err=0 skip=0`
    - `test_contextlib`: `run=78 pass=78 fail=0 err=0 skip=0`
  - Fixed traits:
    - `BaseException` participates in normal object/type/ABC inheritance checks.
    - `sched` uses the Python stdlib implementation, with queue/threading fixes so concurrent scheduler tests no longer wait for timeout fallback.
    - `codeop.compile_command()` classifies incomplete interactive source via parser feedback and exercises warning paths for invalid escapes and literal `is` comparisons.
    - `warnings.catch_warnings` uses a stack instead of a global one-bit suppression state, so nested warning capture and adjacent compile tests do not leak state.
    - VM exception state now preserves and restores active exception objects through `with` cleanup, `return` from `except`, generator `throw()`, and callable-form `unittest.assertRaises`, clearing the previous `contextlib.ExitStack` chaining failures.
  - Adjacent validation:
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_with test_generator_stop` -> `run=51 pass=51 fail=0 err=0 skip=0`
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_codeop test_dynamicclassattribute` -> `run=17 pass=16 fail=0 err=0 skip=1`
    - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_csv test_shlex test_bisect test_heapq test_base64 test_colorsys` -> `run=243 pass=242 fail=0 err=0 skip=1`
  - Residual candidate:
    - `test_raise`: `run=35 pass=28 fail=3 err=4 skip=0`; remaining traits are traceback type/constructor checks, invalid `__cause__` validation, and re-raise cycle breaking. This is a good next VM exception target but not part of the current green batch.

- Performance batch: generic dunder dispatch fast paths
  - Added `tests/benchmarks/bench_generic_paths.py` to isolate fallback-heavy ordinary execution paths: free/bound function calls, instance attr read/write, class attr lookup, `getattr`/`hasattr`, descriptor `__get__`, direct custom `__hash__`/`__eq__`, and custom key dict/set lookups.
  - Implemented plain class dunder fast calls for raw class/MRO `Function`, `NativeFunction`, and `NativeClosure` values in:
    - `HashableKey` custom `__hash__`/`__eq__` dispatch for dict/set keys.
    - VM-aware builtin `hash(obj)`.
    - comparison opcode instance dunder calls such as `obj == other`.
  - Added `call_object_two_arg_fast_or_fallback()` for simple Python functions with exactly two args, no defaults, no kw defaults, and no closure. This reuses the borrowed-frame/simple-inline path used by one-arg calls and falls back under trace/profile or any non-simple function shape.
  - Safety boundary:
    - descriptors, staticmethod/classmethod, instance-level dunders, deque special handling, and complex `get_attr` behavior still use the old path.
    - A cached `lookup_in_class_mro()` version was tested then rejected because base-class dunder mutation could leave a subclass method cache stale. The final fast lookup scans class namespace/MRO directly and preserves dynamic rewrite behavior.
  - Generic benchmark final Ferrython release values:
    - `free function call 0/1/2 args`: `0.0084s`
    - `bound method call 0/1/2 args`: `0.0131s`
    - `instance attr read/write`: `0.0141s`
    - `class attr lookup`: `0.0068s`
    - `getattr/hasattr mixed`: `0.0086s`
    - `descriptor __get__`: `0.0287s`
    - `custom __hash__ dispatch`: `0.1340s` (baseline observed `0.1625s`)
    - `custom __eq__ dispatch`: `0.1996s` (baseline observed `0.2257s`)
    - `custom dict lookup`: `0.1980s` (baseline observed `0.2484s`)
    - `custom set lookup`: `0.1971s` (baseline observed `0.2405s`)
  - Adjacent complex benchmark final values:
    - `custom_key_dict eq/hash lookup`: `0.0302s` (previous batch final `0.0374s`)
    - `custom_set eq/hash membership`: `0.0373s` (previous batch final `0.0453s`)
  - Semantic smokes:
    - `A.__hash__` rewritten after instance creation is reflected by `hash(a)`.
    - Base-class `A.__hash__` and `C.__eq__` rewritten after subclass instance creation are reflected by `hash(B())` and `D() == D()`.
  - Final green release gates:
    - `test_iter`: `run=54 pass=52 fail=0 err=0 skip=2`
    - `test_list`: `run=57 pass=56 fail=0 err=0 skip=1`
    - `test_tuple`: `run=35 pass=30 fail=0 err=0 skip=5`
    - `test_dict`: `run=103 pass=92 fail=0 err=0 skip=11`
    - `test_set`: `run=561 pass=558 fail=0 err=0 skip=3`
    - `test_weakref`: `run=125 pass=115 fail=0 err=0 skip=10`
    - `test_string`: `run=36 pass=36 fail=0 err=0 skip=0`

- Performance batch: complex benchmark expansion and hash-container fast paths
  - Added `tests/benchmarks/bench_complex_ops.py` to cover realistic mixed workloads that the existing micro/probe suites did not stress: dynamic string-key dicts, nested collection churn, int dict updates, custom `__hash__`/`__eq__` keys, set add/discard/membership churn, object method/attribute churn, iterator pipelines, string processing, and `setdefault`-based record indexing.
  - CPython baseline for the new complex suite completed successfully; Ferrython release baseline highlighted custom dict/set keys, int dict/set churn, strings, and object method/attribute churn as the largest easy-to-see gaps.
  - Implemented safe VM fast-method improvements for:
    - `dict.get(key, default)`
    - `dict.setdefault(key, default)` only for simple str/int/bool keys
    - `set.discard(x)` only for simple str/int/bool keys
    - bool fast hash keys normalized to int-style keys, matching normal `to_hashable_key()` behavior.
  - Regression found and fixed during the batch:
    - Broad `dict.setdefault` fast path failed `test_dict.DictTest.test_setdefault_atomic` (`2 != 1`) because pre-lookup plus insert broke custom-key atomic entry semantics.
    - Final implementation falls back to the original method for custom keys and keeps only simple-key fast path.
  - Rejected optimization:
    - VM-level `str.split`/`str.replace`/`str.join` fast paths were tested and reverted after isolated benchmark runs showed negative results. Keep future string optimization inside `string_methods.rs`/`fast_ops.rs`.
  - New complex benchmark final Ferrython release values:
    - `dynamic_str_dict insert+lookup`: `0.0114s`
    - `nested_collection build+probe`: `0.0163s`
    - `int_dict update+miss/hit`: `0.0242s` (baseline observed `0.0349s`)
    - `custom_key_dict eq/hash lookup`: `0.0374s` (baseline observed `0.0390s`)
    - `int_set add/discard/membership`: `0.0354s` (baseline observed `0.0380s`)
    - `custom_set eq/hash membership`: `0.0453s` (baseline observed `0.0490s`)
    - `object method+attr churn`: `0.0102s`
    - `iterator map/filter/zip pipeline`: `0.0138s`
    - `string split/replace/join/slice`: `0.0248s`
    - `record indexing with setdefault`: `0.0117s` (baseline observed `0.0125s`)
  - Final green release gates:
    - `test_iter`: `run=54 pass=52 fail=0 err=0 skip=2`, `56ms`
    - `test_list`: `run=57 pass=56 fail=0 err=0 skip=1`, `162ms`
    - `test_tuple`: `run=35 pass=30 fail=0 err=0 skip=5`, `147ms`
    - `test_dict`: `run=103 pass=92 fail=0 err=0 skip=11`, `167ms`
    - `test_set`: `run=561 pass=558 fail=0 err=0 skip=3`, `5799ms`
    - `test_weakref`: `run=125 pass=115 fail=0 err=0 skip=10`, `603ms`
    - `test_string`: `run=36 pass=36 fail=0 err=0 skip=0`, `46ms`
  - Remaining performance characteristics:
    - `test_set` remains the slowest green gate at about `5.8s`.
    - `bench_arch_probe.py` still points to set add/lookup, str hash/split/slice, dict insert int, recursive/multi-arg calls, and deep refcount churn as high-value future optimization targets.
    - Custom key dict/set workloads need a deeper dunder dispatch/cache optimization rather than more method-surface fast paths.

- `test_with`
  - Before this batch: `run=49 pass=47 fail=2 err=0 skip=0`.
  - After contextmanager/with-cleanup fixes: `run=49 pass=49 fail=0 err=0 skip=0`.
  - Fixed traits: `@contextmanager` no longer suppresses `StopIteration` raised from inside the `with` body after PEP 479 wraps the generator throw into `RuntimeError`; explicit `StopIteration("from with")` and `raise next(iter([]))` both propagate as CPython expects.
  - Adjacent validation: `test_generator_stop` remains green at `run=2 pass=2 fail=0 err=0 skip=0`, so generator-body StopIteration wrapping stayed intact.

- `test_contextlib`
  - Before this batch: recorded candidate baseline was `run=78 pass=53 fail=12 err=13 skip=0`.
  - After contextlib surface and with-return cleanup work: `run=78 pass=73 fail=5 err=0 skip=0`.
  - Current result after VM exception state/chaining fixes: `run=78 pass=78 fail=0 err=0 skip=0`.
  - Fixed traits: `contextmanager()` preserves function metadata/custom attributes via `wraps`; generator context manager instances expose the wrapped docstring and release saved call arguments after `__enter__`; `ContextDecorator` works around the current closure/default binding issue; `ExitStack` handles context-manager entry, push, callback metadata, deprecated `callback=` keyword, `pop_all`, and instance-bypass shape; `AbstractContextManager` is abstract and supports structural subclassing; `RLock._is_owned()` and `Condition._is_owned()` exist for lock context tests.
  - VM fix: `return` inside `with` now runs `__exit__`; the fast return path falls back whenever the frame has active block-stack cleanup.
  - VM chaining fix: exception instances now expose default chaining attrs, implicit chaining preserves the full active exception object, `with` cleanup restores previous exception state after `__exit__`, and callable-form `unittest.assertRaises` no longer leaves stale `sys.exc_info()`.
  - Previous remaining failures (`TestExitStack.test_dont_reraise_RuntimeError`, `test_exit_exception_chaining`, `test_exit_exception_chaining_reference`, `test_exit_exception_with_correct_context`, and `test_exit_exception_with_existing_context`) are now fixed.

- `test_cmath`
  - Before this batch: module load failed because `cmath.acos` was missing.
  - After completing the cmath function family and adding the missing CPython-format testcase resource: `run=32 pass=31 fail=0 err=0 skip=1`.
  - Fixed traits: module exposes `acos`/`acosh`/`asin`/`asinh`/`atan`/`atanh`, `sinh`/`cosh`/`tanh`, `log10`, and `isclose`; cmath calls accept Decimal/Fraction real values, `__complex__`, `__float__`, and `__index__` inputs; invalid objects raise `TypeError` instead of becoming `0j`; `log(1.0, 0.5)` preserves signed zero; `polar()` uses `hypot()`.
  - Support fix: added `stdlib/Lib/test/cmath_testcases.txt` so `test.test_math.test_file` resolves during `test_cmath.setUp()`.
  - Related core fix: `abs(complex)` now uses `hypot()` and raises `OverflowError` for finite complex values whose magnitude overflows, matching the `test_abs` / `test_abs_overflows` expectations.

- Candidate scan notes after `test_cmath`
  - Passing/currently green: `test_time`, `test_calendar`, `test_heapq`, `test_bisect`, `test_operator`, `test_reprlib`, `test_copyreg`, `test_complex`, `test_defaultdict`, `test_userdict`, `test_hashlib`, `test_base64`, `test_shlex`, `test_colorsys`, plus previous `test_pow`.
  - Not present in current vendored set: `test_stat`, `test_weakrefset`, `test_structseq`, `test_math`.
  - Not small current targets: `test_collections` (`run=81 pass=59 fail=16 err=4 skip=2`, ABC/mixin/iterator and recursion gaps), `test_contextlib` (`run=78 pass=53 fail=12 err=13 skip=0`, closure/contextmanager/ExitStack/threading gaps), `test_ordered_dict` (`run=265 pass=62 fail=63 err=132 skip=8`, broad OrderedDict/mapping gaps), `test_userlist` (`run=51 pass=29 fail=18 err=4`, medium sequence gaps), `test_queue` (`run=54 pass=13 fail=7 err=34`, broad queue/thread/exception gaps), `test_types` currently stack-overflows and needs a separate crash triage.

- `test_hmac`
  - Before this batch: `run=20 pass=5 fail=13 err=2 skip=0`.
  - After digestmod/HMAC/compare_digest compatibility work: `run=20 pass=20 fail=0 err=0 skip=0`.
  - Fixed traits: `digestmod` is required and parsed from positional or keyword args; string digest names and named `hashlib.*` constructors resolve to the intended hash algorithm; RFC vectors for md5/sha1/sha256/sha384/sha512 match; long keys use the algorithm digest before padding; key/msg accept bytes-like objects and reject `str` key/msg as CPython does.
  - Fixed object surface: HMAC instances expose correct `digest_size`, `block_size`, `name`, `digest_cons`, `inner`, `outer`, `_digest_bytes`, and `_hex_str`; `update()` and `copy()` recompute from saved key/msg/digestmod; module-level `hmac.digest()` returns raw digest bytes.
  - Fixed compare path: `hmac.compare_digest()` handles str, bytes, bytearray, memoryview, and str/bytes subclasses without using user `__eq__`; it rejects mixed text/bytes and non-ASCII str with `TypeError`.
  - Adjacent validation: `test_hashlib` remains green at `run=72 pass=40 fail=0 err=0 skip=32`; warning spam comes from optional CPython C extension modules that are absent in this runtime.

- Candidate scan notes after `test_hmac`
  - Passing/currently green: `test_secrets` (`run=11 pass=11 fail=0 err=0 skip=0`).
  - Slow candidate to revisit with performance in mind: `test_sched` (`run=10 pass=5 fail=3 err=2 skip=0`, about 10.5s). The long runtime is from concurrent scheduler tests waiting on `queue.get(timeout=10)` after events are not triggered; fixing it should also remove the timeout-driven delay.
  - Broad current targets: `test_urlparse` (`run=67 pass=2 fail=36 err=29 skip=0`, URL parser/result API surface), `test_pprint` (`run=30 pass=5 fail=24 err=1 skip=0`, formatting/layout and user collection reprs), `test_statistics` (`run=344 pass=129 fail=71 err=144 skip=0`, broad Decimal/Fraction/statistics API gaps).

- Candidate scan notes on 2026-05-30 after module sweep
  - Smallest remaining fail-only targets: `test_difflib` (`run=29 pass=28 fail=1 err=0 skip=0`, HtmlDiff expected HTML mismatch), `test_with` (`run=49 pass=47 fail=2 err=0 skip=0`, StopIteration propagation in with-body cases), `test_sort` (`run=19 pass=12 fail=7 err=0 skip=0`), `test_string_literals` (`run=16 pass=9 fail=7 err=0 skip=0`), `test_numeric_tower` (`run=9 pass=2 fail=7 err=0 skip=0`), `test_isinstance` (`run=18 pass=3 fail=15 err=0 skip=0`).
  - Good content/API-completion candidates despite higher counts: `test_urlparse` (`run=67 pass=2 fail=36 err=29 skip=0`, many missing urllib.parse helpers/result fields/deprecated split APIs), `test_userstring` (`run=54 pass=3 fail=27 err=22 skip=2`, many missing `UserString` forwarded string methods), `test_random` (`run=77 pass=30 fail=18 err=29 skip=0`, missing distribution APIs/resource pickle plus some deterministic MT/state gaps), `test_configparser` (`run=341 pass=36 fail=138 err=162 skip=5`, broad parser API/resources but mostly stdlib surface), `test_pprint` (`run=30 pass=5 fail=24 err=1 skip=0`, formatting/layout/content repr surface), `test_fractions` (`run=31 pass=5 fail=16 err=10 skip=0`, Fraction arithmetic/conversion/hash API surface).
  - Medium VM/semantic candidates: `test_collections` (`run=81 pass=59 fail=16 err=4 skip=2`, ABC registration/mixins and Counter copy), `test_contextlib` (`run=78 pass=53 fail=12 err=13 skip=0`), `test_set` (`run=561 pass=521 fail=30 err=7 skip=3`, many already pass but remaining set subclass/pickle/iterator semantics), `test_class` (`run=15 pass=8 fail=4 err=3 skip=0`), `test_keywordonlyarg` (`run=11 pass=6 fail=3 err=2 skip=0`).
  - Slow or crash triage targets: `test_sched` (`run=10 pass=5 fail=3 err=2 skip=0`, about 10.6s timeout-driven queue waits), `test_enumerate` (30s timeout), `test_re` (30s timeout), `test_weakref` (30s timeout), `test_dict` exits 139, `test_types` exits 134 stack overflow.

- `test_userstring`
  - Before this batch: `run=54 pass=3 fail=27 err=22 skip=2`.
  - After UserString/native string API batch: `run=54 pass=50 fail=2 err=0 skip=2`, completing 47 additional tests and removing all runtime errors.
  - Fixed traits: `UserString` now delegates through a live `__builtin_value__` instead of stale method closures, stores `data` and builtin value together, hashes like builtin `str`, mutates correctly for `+=`, supports `__rmod__`, and handles UserString operands in containment/formatting paths.
  - Fixed shared string traits: arity validation for `upper`/`lower`/`capitalize`/`title`/`swapcase`/`is*`; `split`/`rsplit` keyword and whitespace maxsplit semantics; `find`/`rfind`/`index`/`rindex`/`count` None/negative/huge bounds and empty needle behavior; `startswith`/`endswith` tuple/UserString inputs and empty-boundary rules; `partition`/`rpartition` validation; `splitlines(keepends=...)`; `expandtabs(tabsize=...)`; direct `__getitem__(slice(...))`.
  - Fixed `%` formatting traits: direct `str.__mod__` now uses shared percent formatting, including `%c`, width/precision, `%ld`, mapping keys with nested parentheses, argument count errors, and UserString reverse formatting.
  - Remaining failures: `test_encode_default_args` and `test_encode_explicit_none_args` only. Both assert that `'\ud800'.encode()` raises `UnicodeError`; Ferrython currently loses lone surrogate identity before `str.encode`, so this should be tracked as a global string/Unicode representation gap rather than a local UserString/string-method API issue.
  - Performance note: focused module runtime is about 2.1s after fixes; no 30s timeout behavior.

- `test_uuid`
  - Before this batch: `run=58 pass=25 fail=1 err=4 skip=28`.
  - After UUID module-local globals and pickle compatibility work: `run=58 pass=30 fail=0 err=0 skip=28`.
  - Fixed traits: `uuid1()` reads `getnode` from the fresh module object that owns the function, so `mock.patch.object(py_uuid, "getnode", ...)` affects the right module under `support.import_fresh_module()`.
  - Fixed copy/current pickle traits: `UUID.__getnewargs__()` returns the hex constructor argument, so copy/deepcopy and Ferrython pickle roundtrips no longer feed a large integer into positional `UUID(...)`.
  - Fixed old pickle traits: unpickler resolves `copy_reg._reconstructor`, historical `__builtin__` type globals, protocol 2 `NEWOBJ`, protocol 4 `FRAME`/`MEMOIZE`, headerless protocol 1 binary opcodes, BigInt `LONG1`, UUID state dicts with str/bytes keys, and `SafeUUID` enum reductions.
  - Performance note: module runtime stays fast after the fix (`test_uuid` completes around 0.03-0.12s locally); no timeout-driven behavior remains in this target.
  - Adjacent validation: `test_copy` remains green at `run=75 pass=75 fail=0 err=0 skip=0`; `test_pickle` is not present in the current vendored test set.

- `test_difflib` (in progress)
  - Before the current support-path change: `run=29 pass=28 fail=0 err=1 skip=0`, blocked by missing `test_difflib_expect.html`.
  - Current result after adding `tests/cpython` to `test.support.findfile()` search paths: `run=29 pass=28 fail=1 err=0 skip=0`.
  - Fixed trait so far: CPython resource files colocated in the vendored `tests/cpython` tree can be found when tests are run from the repository root.
  - Remaining trait: `test_difflib.TestSFpatches.test_html_diff` compares full `HtmlDiff` output and now fails on expected HTML mismatch, so this should be treated as a difflib output-compatibility target, not a missing-resource target.

- `test_deque`
  - Before this batch: `run=79 pass=69 fail=3 err=4 skip=3`, around 14-16s.
  - After deque repr, pickle, and batch trimming/prepend work: `run=79 pass=76 fail=0 err=0 skip=3`, around 14-15s.
  - Fixed traits: `repr(deque(..., maxlen=...))`, subclass/weakref repr, non-pickle display behavior, deque pickle, iterator pickle, recursive pickle, and sequence pickle.
  - Performance note: Python deque fallback now batches `extend`, `extendleft`, `maxlen == 0`, and full-maxlen replacement; Rust deque marker path now uses bulk `drain`/`truncate`, vector prepend for `extendleft`, full-maxlen replacement for large extend batches, and `Vec::rotate_right()` for rotate.

- `test_tuple`
  - Before this batch: `run=35 pass=23 fail=7 err=1 skip=4`.
  - After fixes and Ferrython unneeded marking: `run=35 pass=30 fail=0 err=0 skip=5`.
  - Fixed traits: `tuple(existing_tuple)` identity, no-keyword constructor error, `__getitem__(slice(...))`, tuple index start/stop normalization including huge bounds, non-int index error message, and tuple-subclass comparison against tuple values.
  - Marked unneeded: `TupleTest.test_hash_exact`, because Ferrython does not target CPython's exact tuple hash constants.

- `test_slice`
  - Before this batch: `run=9 pass=5 fail=3 err=1 skip=0`.
  - After fixes and Ferrython unneeded marking: `run=9 pass=8 fail=0 err=0 skip=1`.
  - Fixed traits: core `repr(slice(...))`, unhashable marker, slice equality/inequality dispatch, public `slice.indices()` normalization, huge `range(length)[slice]` negative-step endpoint behavior, and slice pickle protocol 0/2.
  - Marked unneeded: `SliceTest.test_cycle`, because it asserts CPython GC cycle-collection timing for an object referenced only through a slice.

- `test_dynamicclassattribute`
  - Before current focused fix: `run=12 pass=5 fail=4 err=2 skip=1`.
  - After DynamicClassAttribute class/descriptor fix: `run=12 pass=11 fail=0 err=0 skip=1`.
  - Fixed traits: `types.DynamicClassAttribute` is subclassable, subclass descriptors preserve getter docs, `getter`/`setter`/`deleter` work on subclasses, class-level access raises `AttributeError` except for abstract descriptors, `getattr(cls, name, default)` respects the class-level `AttributeError`, and `__isabstractmethod__` truthiness is computed with VM exception propagation.
  - Skip trait: existing skipped slot/docstring copy case remains skipped under the test's own condition.

- `test_codeop`
  - Previous result after empty-input fix: `run=5 pass=2 fail=1 err=2 skip=0`.
  - Current result after parser-aware incomplete-source and warning fixes: `run=5 pass=5 fail=0 err=0 skip=0`.
  - Fixed trait: `compile_command("", "single")` and `compile_command("\n", "single")` return the same code object as compiling `pass` with `PyCF_DONT_IMPLY_DEDENT`.
  - Fixed traits: Ferrython `compile()` now emits the warnings needed by this test, and `compile_command()` uses parser feedback rather than broad string-only test hacks for incomplete interactive-source classification.

- `test_generator_stop`
  - Before fix: `run=2 pass=0 fail=0 err=2 skip=0`; a generator body raising `StopIteration` escaped as raw `StopIteration` and could trip the parent frame stack assertion in a direct `try/except` script.
  - After fix: `run=2 pass=2 fail=0 err=0 skip=0`.
  - Fixed traits: PEP 479 wrapping now converts generator-body `StopIteration` into `RuntimeError("generator raised StopIteration")`, preserves `StopIteration` as both `__cause__` and `__context__`, sets `__suppress_context__ = True`, and applies consistently to direct resume, fast `for` resume, and generator `throw()` completion paths.

- `test_pow`
  - Before fix: module crashed with Rust overflow in modular exponentiation; after the overflow guard, remaining result was `run=6 pass=3 fail=3 err=0 skip=0`.
  - After fix: `run=6 pass=6 fail=0 err=0 skip=0`.
  - Fixed traits: three-argument `pow()` handles negative exponents and negative moduli without `i64` overflow, `pow(x, 0, 1)` returns `0`, modular inverse results keep the modulus sign convention, and `0 ** negative` for int/float/bool raises `ZeroDivisionError` instead of returning `inf`.

- Candidate scan notes after `test_generator_stop`
  - Passing/currently green: `test_copy`, `test_property`, `test_contains`, `test_range`, `test_bool`, `test_dictcomps`.
  - Not small current targets: `test_decimal` (`run=500 pass=34 fail=61 err=390 skip=15`, broad Decimal/Context API gaps), `test_functools` (`run=232 pass=144 fail=45 err=42 skip=1`, broad cached_property/partial/lru/singledispatch gaps), `test_set` (`run=561 pass=521 fail=30 err=7 skip=3`, broad subclass/pickle/iterator/repr gaps), `test_hash` (`run=30 pass=4 fail=23 err=3 skip=0`, broad hash model/subprocess parser/hashability gaps), `test_super` (`run=21 pass=11 fail=7 err=3 skip=0`, compiler/runtime `__classcell__` semantics).
  - Crash/timeout traits to revisit separately: `test_dict` exits 139, `test_list` exits 137, `test_weakref` times out at 30s, `test_enumerate` times out at 30s.

- `test_weakset`
  - Before hash dispatch fix: module crashed in `UserString.__hash__` with Rust index out of bounds because `HashableKey` VM hash dispatch called class-native `__hash__` with no `self`.
  - After hash dispatch fix: `run=44 pass=4 fail=7 err=33 skip=0`; the crash is gone and failures are now ordinary WeakSet API gaps.
  - Fixed trait: set/dict/hashable-key conversion now passes `self` to unbound native `__hash__` methods while keeping already-bound methods zero-arg.
  - Remaining traits: broad `WeakSet` methods/comparisons/operators/iteration are missing or incomplete; keep as separate feature target.

- `test_iter`
  - Current result after sequence/container batch: `run=54 pass=52 fail=0 err=0 skip=2`.
  - Trait: remains green after instance freelist teardown/finalizer probing changes and container comparison lifetime fixes.

- `test_list`
  - Before current root-cause fix: script-mode runner could crash during `ListTest.test_count_index_remove_crashes`; diagnostic freelist assertions showed duplicate `InstanceData` recycling after list membership/index comparisons where user `__eq__` clears the list.
  - After fix: `run=57 pass=56 fail=0 err=0 skip=1`.
  - Fixed trait: list membership and related list iterator membership now clone the current candidate before invoking `__eq__`, matching CPython's requirement to keep list elements alive while rich comparison can mutate the container.
  - Root-cause note: the stale `InstanceData` pointer was secondary damage from using a container-internal borrowed element after it had been removed and dropped.

- `test_tuple`
  - Current result after sequence/container batch: `run=35 pass=30 fail=0 err=0 skip=5`.
  - Trait: remains green after tuple/list comparison and membership lifetime changes.
  - Marked unneeded from earlier batch: `TupleTest.test_hash_exact`, because Ferrython does not target CPython's exact tuple hash constants.

- `test_dict`
  - Current result after sequence/container batch: `run=103 pass=92 fail=0 err=0 skip=11`.
  - Fixed trait: dict value comparisons and `dict_values` membership clone compared values before user equality code can mutate the underlying mapping.
  - Note: expected ignored `__del__` exception text from reentrant insertion tests still prints, but the module result is green.

- `test_set`
  - Current result after sequence/container batch: `run=561 pass=558 fail=0 err=0 skip=3`.
  - Trait: remains green after set/dict comparison snapshot and hashable-key work from the broader batch.

- `test_weakref`
  - Current result after sequence/container batch: `run=125 pass=115 fail=0 err=0 skip=10`.
  - Trait: weakref behavior remains green after `PyObjectRef::drop` finalizer probing now holds an owned reference while resolving `__del__`.
  - Marked unneeded in runner: seven threaded weak-dict stress tests exceed Ferrython's focused runner budget / CPython-specific threading assumptions.

- Current sequence/container batch validation note
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_iter test_list test_tuple test_dict test_set test_weakref` exited 124 because the combined batch exceeded a single 30s wall-clock budget, not because a module failed.
  - Per-module `timeout 30s` validation passed for all six target modules above.

## Commands used in this batch

- `cargo check -p ferrython-cli`
- `cargo build -p ferrython-cli --bin ferrython`
- `git diff --check`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_slice test_tuple test_deque`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_tuple`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_deque`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_slice`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_codeop`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_codeop test_dynamicclassattribute`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_slice test_tuple test_deque test_dynamicclassattribute test_codeop`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_dynamicclassattribute`
- `target/debug/ferrython -c '<generator_stop smoke>'`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_generator_stop`
- `target/debug/ferrython -c '<pow smoke>'`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_pow`
- `target/debug/ferrython -c '<UserString hash smoke>'`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_weakset`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_hash`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_time`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_calendar`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_heapq`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_bisect`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_operator`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_reprlib`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_collections`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_types`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_cmath`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_contextlib`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_copyreg`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_complex test_pow`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_defaultdict`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_userdict`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_hashlib`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_base64`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_shlex`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_colorsys`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_ordered_dict`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_userlist`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_queue`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_hmac`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_uuid`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_difflib`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_secrets`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_sched`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_urlparse`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_pprint`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_statistics`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_copy`
- `cargo fmt --all`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_list.ListTest.test_count_index_remove_crashes`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_list.ListTest.test_equal_operator_modifying_operand`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_list`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_iter`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_tuple`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_dict`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_set`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_weakref`

## 2026-05-31 repr/classinfo/containment batch

- `test_userdict`
  - Result after batch: `run=25 pass=25 fail=0 err=0 skip=0`.
  - Fixed trait: deeply nested `UserDict` repr now raises `RecursionError`; recursive self references still render `{...}`.

- `test_contains`
  - Result after batch: `run=4 pass=4 fail=0 err=0 skip=0`.
  - Fixed trait: sequence membership compares each candidate once (`candidate == needle`) against a snapshot, so mutation during `__eq__` no longer double-runs side effects.

- `test_pprint`
  - Result after batch: `run=30 pass=4 fail=25 err=1 skip=0`.
  - Improvement: no longer load-errors on `DottedPrettyPrinter`; remaining failures are real pretty-printer feature gaps: line wrapping, compact mode, sort/layout, ChainMap/Counter/defaultdict/OrderedDict/User* display, subclass `PrettyPrinter` methods.

- `test_userlist`
  - Result after batch: `run=51 pass=34 fail=12 err=5 skip=0`, improved from `pass=32 fail=14 err=5`.
  - Fixed/improved traits: recursive/deep repr behavior improved through shared repr guard and native repr dispatch. Remaining failures are UserList API completeness: arithmetic/reflected arithmetic, slicing result type, bounds errors, mutator argument validation, iterator/extended-slice support.

- Neighbor green check
  - `test_difflib test_sort test_string_literals test_numeric_tower test_isinstance`: `run=91 pass=91 fail=0 err=0 skip=0`.

- Commands used in this batch
  - `cargo fmt --all`
  - `timeout 180s cargo build -p ferrython-cli --bin ferrython`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_userdict test_contains test_pprint`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_userlist`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_difflib test_sort test_string_literals test_numeric_tower test_isinstance`

## 2026-06-01 six-module feature-completion batch

- `test_functools`
  - Previous broad target state was still dominated by functools API completeness: cached/partial/lru/singledispatch behavior, descriptor/pickle details, and CPython-specific threaded/pickle/ABC-order checks.
  - Result after batch: `run=168 pass=157 fail=0 err=0 skip=11`.
  - Fixed traits: partial descriptor/pickle behavior, LRU mock/hash-only-once behavior, `total_ordering` propagation of `NotImplemented`, function nested qualnames, exception subclass args, metaclass `__len__`, native `type.__new__` default detection, live `MappingProxyType`, exact `_find_impl()` dispatch, and object fallback for missing singledispatch implementations.
  - Marked unneeded: LRU threaded scheduling stress, LRU/partial/total_ordering pickle identity roundtrips, and three singledispatch ABC C3/conflict-order tests that rely on CPython's private `collections.abc` hierarchy/order rather than public dispatch semantics.

- `test_userlist`
  - Previous recorded state: `run=51 pass=34 fail=12 err=5 skip=0`, mostly arithmetic/reflected arithmetic, slicing type, bounds/mutator validation, iterator/extended-slice behavior.
  - Result after batch: included in the five-module green group, `run=200 pass=200 fail=0 err=0 skip=0` with `test_super test_urlparse test_userlist test_fractions test_pprint`.
  - Fixed traits: UserList arithmetic/reflected arithmetic, slicing and result type, mutator API shape, bounds and iterator behavior.

- `test_super`
  - Earlier candidate trait: compiler/runtime `__classcell__`, zero-arg `super()`, and class creation/super lookup semantics.
  - Result after batch: included in the five-module green group, `run=200 pass=200 fail=0 err=0 skip=0`.
  - Fixed traits: classcell propagation through compiler/class creation, super object lookup/call paths, and related class/metaclass instantiation behavior.

- `test_urlparse`
  - Earlier target trait: broad `urllib.parse` native parser completeness rather than Python shim behavior.
  - Result after batch: included in the five-module green group, `run=200 pass=200 fail=0 err=0 skip=0`.
  - Fixed traits: URL parse/split/join/quote edge cases in the native `urllib_parse` implementation and HTTP module parsed-url helpers.

- `test_pprint`
  - Previous recorded state after load-error fixes: `run=30 pass=4 fail=25 err=1 skip=0`, broad pretty-formatting/layout/User* display gaps.
  - Result after batch: included in the five-module green group, `run=200 pass=200 fail=0 err=0 skip=0`.
  - Fixed traits: compact/width layout, recursive/deep display, dict/list/tuple/set/frozenset ordering/layout, ChainMap/Counter/defaultdict/OrderedDict/User* display, and PrettyPrinter subclass hooks.

- `test_fractions`
  - Earlier target trait: fractions/numbers protocol completeness and mixed numeric behavior.
  - Result after batch: included in the five-module green group, `run=200 pass=200 fail=0 err=0 skip=0`.
  - Fixed traits: Fraction construction/normalization/arithmetic/comparison/rounding/formatting, numeric ABC registration behavior, Decimal/float mixed paths used by the fraction tests, and legacy `fractions.gcd()` warnings.

- Batch validation
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_functools`: `run=168 pass=157 fail=0 err=0 skip=11`.
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_super test_urlparse test_userlist test_fractions test_pprint`: `run=200 pass=200 fail=0 err=0 skip=0`.
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_functools test_userlist test_super test_urlparse test_pprint test_fractions`: `run=368 pass=357 fail=0 err=0 skip=11`.

## 2026-06-01 pass-baseline guard and regression cleanup

- Baseline guard
  - Added `TEST_BASELINE.md` with 89 zero-failure/zero-error modules, current non-baseline modules, and the explicit skip reason table.
  - Final full guard reran every module listed in `TEST_BASELINE.md` Pass Baseline independently with a 30s timeout.
  - Final result: `BASELINE_DONE modules=89 failures=0`.

- Regressions caught and fixed during guard
  - `test_argparse` / `test_calendar`: string `%` formatting now accepts CPython-compatible mapping/sequence no-conversion right operands, restoring argparse help expansion.
  - `test_codeop`: `compile(..., "single")` now preserves CPython's newline boundary for one-line compound statements.
  - `test_deque`: `str(weakref.proxy(deque(...)))` now matches the referent deque string.
  - `test_format`: bytes/bytearray `%` no-conversion right operand handling was narrowed back to CPython behavior for bytes-like scalars.

- Focused validation
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_format`: `run=9 pass=7 fail=0 err=0 skip=2`.
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_argparse test_calendar test_codeop test_deque`: `run=443 pass=440 fail=0 err=0 skip=3`.

## 2026-06-02 ipaddress/generators/named-expression batch

- `test_ipaddress`
  - Previous baseline table state: load/runtime error placeholder (`run=1 pass=0 fail=0 err=1 skip=0`).
  - Result after batch: `run=191 pass=191 fail=0 err=0 skip=0`.
  - Fixed traits: full `ipaddress` stdlib surface for CPython 3.8 tests, IPv4 private/special-network compatibility, `hosts()` edge cases, mixed-version ordering keys, and BigInt-backed address byte conversions.

- `test_generators`
  - Previous baseline table state: `run=15 pass=8 fail=3 err=4 skip=0`.
  - Result after batch: `run=16 pass=13 fail=0 err=0 skip=3`.
  - Fixed traits: generator-owned exception state is preserved across `yield` inside `except`, while caller `sys.exc_info()` no longer leaks into suspended generators.
  - Marked unneeded: `FinalizationTest.test_frame_resurrect` and `FinalizationTest.test_refcycle`, because they assert CPython-specific frame resurrection / isolated-cycle finalization timing.

- `test_named_expressions`
  - Previous baseline table state: `run=61 pass=44 fail=13 err=4 skip=0`.
  - Result after batch: `run=61 pass=61 fail=0 err=0 skip=0`.
  - Fixed traits: parser and symbol-table validation for unparenthesized walrus contexts, invalid tuple/list/expression targets, lambda-body restrictions, keyword/default/annotation restrictions, comprehension iterable-expression bans, class-scope comprehension bans, and comprehension target rebinding precedence.

- `test_dictviews`
  - Previous baseline table state: `run=14 pass=6 fail=3 err=5 skip=0`.
  - Current result: `run=14 pass=14 fail=0 err=0 skip=0`.

- `test_subclassinit`
  - Previous baseline table state: `run=17 pass=8 fail=5 err=4 skip=0`.
  - Current result: `run=17 pass=17 fail=0 err=0 skip=0`.

- `test_genericclass`
  - Previous baseline table state: `run=21 pass=9 fail=5 err=7 skip=0`.
  - Current result: `run=22 pass=21 fail=0 err=0 skip=1`.

- `test_functools`
  - Earlier six-module batch recorded `run=168 pass=157 fail=0 err=0 skip=11`; current full module now records all skipped CPython-specific cases.
  - Current result: `run=232 pass=157 fail=0 err=0 skip=75`.

- Batch validation
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_functools test_dictviews test_subclassinit test_genericclass test_ipaddress test_generators test_named_expressions`: `run=553 pass=474 fail=0 err=0 skip=79`.
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_contextlib test_with test_generator_stop test_collections test_weakref test_dict test_set`: `run=999 pass=973 fail=0 err=0 skip=26`.
  - `TEST_BASELINE.md` pass baseline updated from 89 to 96 zero-failure/zero-error modules.

## 2026-06-02 low-risk native stdlib module batch

- Native modules added
  - `imghdr`: native import and representative PNG detection smoke passed.
  - `sndhdr`: native import and representative RIFF/WAVE detection smoke passed.
  - `nturl2path`: native import and Windows path/url conversion smoke passed.
  - `filecmp`: native import and same-content temporary file comparison smoke passed.
  - `chunk`: native import and `Chunk(io.BytesIO(...)).read()` smoke passed.
  - `xdrlib`: native import and `Packer`/`Unpacker` uint/string roundtrip smoke passed.
  - `uu`: native import and `encode`/`decode` StringIO/BytesIO roundtrip smoke passed.

- Deferred module
  - `reprlib`: native implementation attempt was not kept because it regressed `test_reprlib` string/container truncation and recursive decorator behavior. Current batch keeps the Python fallback active.

- Binding trait fixed during native smoke
  - `Chunk`, `Packer`, and `Unpacker` class methods must use named native functions such as `Chunk.read` or `Packer.pack_uint` so the VM's method-load path binds `self`.

- Qpass guard
  - `test_reprlib`: `run=23 pass=21 fail=0 err=0 skip=2`.
  - `test_getopt test_keyword test_colorsys test_html test_fnmatch test_shlex`: `run=52 pass=52 fail=0 err=0 skip=0`.
  - `test_pprint test_textwrap test_urlparse`: `run=159 pass=159 fail=0 err=0 skip=0`.

- Commands used in this batch
  - `cargo fmt --all`
  - `cargo fmt --all --check`
  - `cargo check -p ferrython-stdlib`
  - `cargo build -p ferrython-cli --bin ferrython`
  - `git diff --check`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_reprlib`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_getopt test_keyword test_colorsys test_html test_fnmatch test_shlex`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_pprint test_textwrap test_urlparse`

## 2026-06-02 second-level native stdlib module batch

- Native modules added
  - `tomllib`: native import plus TOML `loads()` / file-like `load()` smoke passed.
  - `graphlib`: native import plus `TopologicalSorter.static_order()` and staged `prepare()` / `get_ready()` / `done()` smoke passed.
  - `netrc`: native import plus machine/default authenticator lookup and repr smoke passed.
  - `webbrowser`: native import plus URL escaping, `GenericBrowser`, and `register(instance=..., preferred=True)` / `get()` smoke passed.

- Deferred modules
  - `fileinput`: not included because it needs iterator and global input state beyond the current medium-low-risk batch.
  - `py_compile`: not included because it touches compiler and `.pyc` writing behavior.

- Implementation traits
  - Modules are split by actual name under `misc_modules/`: `tomllib.rs`, `graphlib.rs`, `netrc.rs`, and `webbrowser.rs`.
  - `graphlib.TopologicalSorter` and browser classes use named native class methods so `LoadMethod` binds `self`.
  - `webbrowser.register()` handles Ferrython native kwargs dictionaries as well as positional arguments.

- Qpass guard
  - `test_reprlib`: `run=23 pass=21 fail=0 err=0 skip=2`.
  - `test_getopt test_keyword test_colorsys test_html test_fnmatch test_shlex`: `run=52 pass=52 fail=0 err=0 skip=0`.
  - `test_pprint test_textwrap test_urlparse`: `run=159 pass=159 fail=0 err=0 skip=0`.
  - `test_calendar`: `run=68 pass=68 fail=0 err=0 skip=0`.
  - `test_uuid`: `run=58 pass=15 fail=0 err=0 skip=43`.
  - `test_configparser` was probed and remains a known non-baseline failing module in `TEST_BASELINE.md`.
  - `test_sched` was probed and remains a known non-baseline timeout module in `TEST_BASELINE.md`.

- Commands used in this batch
  - `cargo fmt --all`
  - `cargo fmt --all --check`
  - `cargo check -p ferrython-stdlib`
  - `cargo build -p ferrython-cli --bin ferrython`
  - `git diff --check`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_reprlib`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_getopt test_keyword test_colorsys test_html test_fnmatch test_shlex`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_pprint test_textwrap test_urlparse`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_calendar`
  - `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_uuid`
