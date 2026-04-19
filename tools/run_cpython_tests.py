#!/usr/bin/env python3
"""CPython regression test runner for Ferrython.

Runs vendored CPython 3.8 tests from tests/cpython/ through the Ferrython
interpreter and reports compatibility results.

Usage:
    ferrython tools/run_cpython_tests.py [OPTIONS] [TEST ...]

    TEST may be a bare name (test_bool), or with .py extension.
    If no TEST is given, all tests in tests/cpython/ are run.

Options:
    -v, --verbose    Verbose test output (pass 2 to unittest runner)
    -q, --quiet      Minimal output
    -f, --failfast   Stop on first failure
    --list           List available tests and exit

Examples:
    ferrython tools/run_cpython_tests.py
    ferrython tools/run_cpython_tests.py test_bool test_dict
    ferrython tools/run_cpython_tests.py -v test_generators
"""

import argparse
import importlib.util
import os
import sys
import unittest


# ---------------------------------------------------------------------------
# Locate the tests/cpython directory
# ---------------------------------------------------------------------------

def _find_cpython_test_dir():
    """Return the absolute path to tests/cpython/, searching upward."""
    # This script lives in tools/; the workspace root is one level up.
    here = os.path.dirname(os.path.abspath(__file__))
    for base in (os.path.dirname(here), here, os.getcwd()):
        candidate = os.path.join(base, "tests", "cpython")
        if os.path.isdir(candidate):
            return candidate
    return None


# ---------------------------------------------------------------------------
# Module loading
# ---------------------------------------------------------------------------

def _normalise_name(name):
    """Strip .py extension and ensure the name starts with 'test_'."""
    name = name.removesuffix(".py")
    if not name.startswith("test_"):
        name = "test_" + name
    return name


def _load_test_module(test_dir, name):
    """Load *name* as a Python module from *test_dir*."""
    name = _normalise_name(name)
    path = os.path.join(test_dir, name + ".py")
    if not os.path.exists(path):
        raise FileNotFoundError(f"no such test file: {path}")

    # Make the test directory importable so that relative imports work.
    if test_dir not in sys.path:
        sys.path.insert(0, test_dir)

    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


# ---------------------------------------------------------------------------
# Running tests
# ---------------------------------------------------------------------------

def _run_one(test_dir, name, verbosity, failfast):
    """Load and run a single test module.  Returns (passed, failed, errors, skipped)."""
    name = _normalise_name(name)
    try:
        mod = _load_test_module(test_dir, name)
    except FileNotFoundError as exc:
        print(f"  MISSING  {name}: {exc}")
        return 0, 0, 1, 0
    except Exception as exc:
        print(f"  LOAD-ERR {name}: {exc}")
        return 0, 0, 1, 0

    loader = unittest.TestLoader()
    suite = loader.loadTestsFromModule(mod)
    runner = unittest.TextTestRunner(verbosity=verbosity, failfast=failfast)
    result = runner.run(suite)
    passed = result.testsRun - len(result.failures) - len(result.errors) - len(result.skipped)
    return passed, len(result.failures), len(result.errors), len(result.skipped)


def _list_tests(test_dir):
    return sorted(
        f[:-3]
        for f in os.listdir(test_dir)
        if f.startswith("test_") and f.endswith(".py")
    )


def _run_all(names, verbosity, failfast):
    test_dir = _find_cpython_test_dir()
    if test_dir is None:
        sys.exit("ERROR: cannot find tests/cpython/ directory")

    if not names:
        names = _list_tests(test_dir)

    total_passed = total_failed = total_errors = total_skipped = 0
    bad_tests = []

    for name in names:
        norm = _normalise_name(name)
        if verbosity > 0:
            sep = "=" * 62
            print(f"\n{sep}\nRunning: {norm}\n{sep}")

        p, f, e, s = _run_one(test_dir, norm, verbosity, failfast)
        total_passed  += p
        total_failed  += f
        total_errors  += e
        total_skipped += s

        if f or e:
            bad_tests.append(norm)
            if failfast:
                break

    print()
    print("=" * 62)
    print("CPython Compatibility Summary")
    print("=" * 62)
    print(f"  Passed  : {total_passed}")
    print(f"  Failed  : {total_failed}")
    print(f"  Errors  : {total_errors}")
    print(f"  Skipped : {total_skipped}")
    print(f"  Total   : {total_passed + total_failed + total_errors + total_skipped}")
    if bad_tests:
        print()
        print("Failed modules:")
        for t in bad_tests:
            print(f"  - {t}")
    print()
    return 1 if bad_tests else 0


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Run CPython regression tests through Ferrython",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument("tests", nargs="*", metavar="TEST",
                        help="test name(s) to run (default: all)")
    parser.add_argument("-v", "--verbose", action="store_true",
                        help="verbose test output")
    parser.add_argument("-q", "--quiet", action="store_true",
                        help="minimal output")
    parser.add_argument("-f", "--failfast", action="store_true",
                        help="stop on first failure")
    parser.add_argument("--list", action="store_true",
                        help="list available tests and exit")
    args = parser.parse_args()

    if args.list:
        test_dir = _find_cpython_test_dir()
        if test_dir is None:
            sys.exit("ERROR: cannot find tests/cpython/ directory")
        for t in _list_tests(test_dir):
            print(t)
        return

    verbosity = 0 if args.quiet else (2 if args.verbose else 1)
    sys.exit(_run_all(args.tests, verbosity=verbosity, failfast=args.failfast))


if __name__ == "__main__":
    main()
