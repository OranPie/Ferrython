# CPython Official Regression Tests

This directory contains test files vendored verbatim from the CPython 3.8
standard library (`Lib/test/`).  They are used to measure Ferrython's
compatibility with CPython 3.8 behaviour.

## Included tests

| File | What it covers |
|------|----------------|
| `test_bool.py` | `bool` semantics (PEP 285) |
| `test_complex.py` | Complex-number arithmetic |
| `test_dict.py` | Built-in `dict` type |
| `test_enumerate.py` | `enumerate()` built-in |
| `test_exception_hierarchy.py` | Exception class hierarchy |
| `test_float.py` | Floating-point arithmetic |
| `test_fstring.py` | f-string syntax (PEP 498) |
| `test_functools.py` | `functools` standard library module |
| `test_generators.py` | Generator functions and expressions |
| `test_int.py` | Built-in `int` type |
| `test_isinstance.py` | `isinstance()` / `issubclass()` |
| `test_iter.py` | Iterator protocol |
| `test_operator.py` | `operator` standard library module |
| `test_set.py` | Built-in `set` and `frozenset` types |
| `test_string.py` | `string` standard library module |

## Source

All files are taken from the CPython **3.8** branch:
<https://github.com/python/cpython/tree/3.8/Lib/test>

They are kept verbatim so that diffs against upstream remain minimal.

## Running

```bash
# Via Make (builds release binary first)
make cpython-test
make cpython-test-verbose

# Via the runner script directly (uses whatever binary is on PATH)
ferrython tools/run_cpython_tests.py
ferrython tools/run_cpython_tests.py test_bool test_dict
ferrython tools/run_cpython_tests.py --verbose test_generators

# Via Cargo integration tests (requires release build)
cargo test -p ferrython-cli --test cpython_suite
cargo test -p ferrython-cli --test cpython_suite -- --ignored   # see failing tests
```

## Compatibility shim

CPython tests depend on `test.support`.  Ferrython provides a compatibility
shim at `stdlib/Lib/test/support/__init__.py` that implements the subset of
the `test.support` API actually exercised by these tests.

## Adding more tests

1. Copy the test file from `cpython/3.8/Lib/test/` verbatim into this directory.
2. Add a `cpython_test!(test_name);` line to `crates/ferrython-cli/tests/cpython_suite.rs`.
3. Run `make cpython-test` to check compatibility.
4. If the test fails, add `#[ignore = "reason"]` to the Rust test and file an issue.
