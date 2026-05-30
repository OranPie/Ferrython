# Focused CPython Test Notes

Last updated: 2026-05-30T15:18:21+08:00

## Current batch

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
  - Before this batch: load error.
  - After `DynamicClassAttribute` property-backed constructor: `run=12 pass=5 fail=4 err=2 skip=1`.
  - Fixed trait: module loads and plain DynamicClassAttribute works as a descriptor-like property payload.
  - Remaining traits: full subclass-of-DynamicClassAttribute behavior is not implemented; doc propagation, abstract flag behavior, class access `AttributeError`, and `getter`/`setter` on subclasses remain open.

- `test_codeop`
  - Current result after empty-input fix: `run=5 pass=2 fail=1 err=2 skip=0`.
  - Fixed trait: `compile_command("", "single")` and `compile_command("\n", "single")` return the same code object as compiling `pass` with `PyCF_DONT_IMPLY_DEDENT`.
  - Remaining traits: Ferrython `compile()` does not yet emit CPython `SyntaxWarning`/`DeprecationWarning`, and incomplete interactive-source classification still needs a parser-aware solution. Avoid broad string-only test hacks here.

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
