# Focused CPython Test Notes

Last updated: 2026-05-30T14:45:36+08:00

## Current batch

- `test_deque`
  - Before this batch: `run=79 pass=69 fail=3 err=4 skip=3`, around 14-16s.
  - After deque repr and batch trimming/prepend work: `run=79 pass=72 fail=1 err=3 skip=3`, around 14-15s.
  - Fixed traits: `repr(deque(..., maxlen=...))`, subclass/weakref repr, and non-pickle display behavior.
  - Remaining traits: pickle/reduce paths still confuse callable class objects with deque instances; iterator pickle mutates/loads `builtin_function_or_method`.
  - Performance note: Python deque fallback now batches `extend`, `extendleft`, and maxlen trimming; Rust deque marker path now uses bulk `drain`, `truncate`, and vector prepend for `extendleft`.

- `test_tuple`
  - Before this batch: `run=35 pass=23 fail=7 err=1 skip=4`.
  - After fixes: `run=35 pass=30 fail=1 err=0 skip=4`.
  - Fixed traits: `tuple(existing_tuple)` identity, no-keyword constructor error, `__getitem__(slice(...))`, tuple index start/stop normalization including huge bounds, non-int index error message, and tuple-subclass comparison against tuple values.
  - Remaining trait: tuple hash algorithm differs from CPython 3.8 exact constants.

- `test_slice`
  - Before this batch: `run=9 pass=5 fail=3 err=1 skip=0`.
  - After fixes: `run=9 pass=6 fail=2 err=1 skip=0`.
  - Fixed traits: core `repr(slice(...))`, unhashable marker, slice equality/inequality dispatch, and most public `slice.indices()` normalization including negative-step `None` stop.
  - Remaining traits: object held only by slice is still not collected, slice pickling is unsupported, and huge `range` comparison with saturated endpoints still disagrees in one `test_indices` subcase.

- `test_dynamicclassattribute`
  - Before this batch: load error.
  - After `DynamicClassAttribute` property-backed constructor: `run=12 pass=5 fail=4 err=2 skip=1`.
  - Fixed trait: module loads and plain DynamicClassAttribute works as a descriptor-like property payload.
  - Remaining traits: full subclass-of-DynamicClassAttribute behavior is not implemented; doc propagation, abstract flag behavior, class access `AttributeError`, and `getter`/`setter` on subclasses remain open.

- `test_codeop`
  - Current result after reverting an over-broad incomplete-source rewrite: `run=5 pass=2 fail=2 err=1 skip=0`.
  - Remaining traits: code object equality differs, Ferrython `compile()` does not yet emit CPython `SyntaxWarning`/`DeprecationWarning`, and incomplete interactive-source classification still needs a parser-aware solution.

## Commands used in this batch

- `cargo check -p ferrython-cli`
- `cargo build -p ferrython-cli --bin ferrython`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_slice test_tuple test_deque`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_tuple`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_codeop test_dynamicclassattribute`
- `timeout 30s target/debug/ferrython tools/run_cpython_tests.py -q test_slice test_tuple test_deque test_dynamicclassattribute test_codeop`
