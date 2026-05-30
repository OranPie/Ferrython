# Focused CPython Test Notes

Last updated: 2026-05-30T17:38:16+08:00

## Current batch

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
  - Current result after empty-input fix: `run=5 pass=2 fail=1 err=2 skip=0`.
  - Fixed trait: `compile_command("", "single")` and `compile_command("\n", "single")` return the same code object as compiling `pass` with `PyCF_DONT_IMPLY_DEDENT`.
  - Remaining traits: Ferrython `compile()` does not yet emit CPython `SyntaxWarning`/`DeprecationWarning`, and incomplete interactive-source classification still needs a parser-aware solution. Avoid broad string-only test hacks here.

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
