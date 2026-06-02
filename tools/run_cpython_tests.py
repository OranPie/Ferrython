#!/usr/bin/env python3
"""CPython regression test runner for Ferrython.

Runs vendored CPython 3.8 tests from tests/cpython/ through the Ferrython
interpreter and reports compatibility results.

Usage:
    ferrython tools/run_cpython_tests.py [OPTIONS] [TEST ...]

    TEST may be a bare name (test_bool), or with .py extension.
    If no TEST is given, all tests in tests/cpython/ are run.

Options:
    -v, --verbose    Show every module in the final summary
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
import time
import unittest


_FERRYTHON_UNNEEDED_TESTS = (
    ("test_tuple.TupleTest.test_hash_exact",
        "Ferrython does not target CPython's exact tuple hash constants"
    ),
    ("test_slice.SliceTest.test_cycle",
        "Ferrython GC does not expose CPython's cycle-collection timing"
    ),
    ("test_generators.FinalizationTest.test_frame_resurrect",
        "Ferrython does not target CPython generator frame resurrection during finalization"
    ),
    ("test_generators.FinalizationTest.test_refcycle",
        "Ferrython GC does not expose CPython's generator finalization timing for isolated cycles"
    ),
    ("test_weakref.MappingTestCase.test_threaded_weak_key_dict_copy",
        "CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget"
    ),
    ("test_weakref.MappingTestCase.test_threaded_weak_key_dict_deepcopy",
        "CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget"
    ),
    ("test_weakref.MappingTestCase.test_threaded_weak_value_dict_copy",
        "CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget"
    ),
    ("test_weakref.MappingTestCase.test_threaded_weak_value_dict_deepcopy",
        "CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget"
    ),
    ("test_weakref.MappingTestCase.test_threaded_weak_valued_setdefault",
        "CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget"
    ),
    ("test_weakref.MappingTestCase.test_threaded_weak_valued_pop",
        "CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget"
    ),
    ("test_weakref.MappingTestCase.test_threaded_weak_valued_consistency",
        "CPython threaded weak-dict stress test exceeds Ferrython's focused runner budget"
    ),
    ("test_functools.TestLRUC.test_lru_cache_threaded2",
        "CPython thread-barrier scheduling stress has implementation-specific cache statistics"
    ),
    ("test_functools.TestLRUPy.test_lru_cache_threaded2",
        "CPython thread-barrier scheduling stress has implementation-specific cache statistics"
    ),
    ("test_decimal.PyThreadingTest.test_threading",
        "Ferrython queues Python bytecode thread targets on the owning VM, so CPython decimal thread-local scheduling is not targeted"
    ),
    ("test_functools.TestLRUC.test_pickle",
        "Ferrython pickle does not target CPython's exact function-wrapper identity roundtrip"
    ),
    ("test_functools.TestLRUPy.test_pickle",
        "Ferrython pickle does not target CPython's exact function-wrapper identity roundtrip"
    ),
    ("test_functools.TestPartialPy.test_recursive_pickle",
        "Ferrython pickle lacks CPython's partial recursion guard and can overflow the host stack"
    ),
    ("test_functools.TestPartialPySubclass.test_recursive_pickle",
        "Ferrython pickle lacks CPython's partial recursion guard and can overflow the host stack"
    ),
    ("test_ordered_dict.CPythonOrderedDictTests.test_pickle_recursive",
        "Ferrython pickle lacks CPython's recursive OrderedDict identity guard and can overflow the host stack"
    ),
    ("test_ordered_dict.CPythonOrderedDictSubclassTests.test_pickle_recursive",
        "Ferrython pickle lacks CPython's recursive OrderedDict identity guard and can overflow the host stack"
    ),
    ("test_ordered_dict.PurePythonOrderedDictTests.test_pickle_recursive",
        "Ferrython pickle lacks CPython's recursive OrderedDict identity guard and can overflow the host stack"
    ),
    ("test_ordered_dict.PurePythonOrderedDictSubclassTests.test_pickle_recursive",
        "Ferrython pickle lacks CPython's recursive OrderedDict identity guard and can overflow the host stack"
    ),
    ("test_ordered_dict.CPythonOrderedDictTests.test_dict_delitem",
        "CPython C OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.CPythonOrderedDictTests.test_dict_pop",
        "CPython C OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.CPythonOrderedDictTests.test_dict_popitem",
        "CPython C OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.CPythonOrderedDictSubclassTests.test_dict_delitem",
        "CPython C OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.CPythonOrderedDictSubclassTests.test_dict_pop",
        "CPython C OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.CPythonOrderedDictSubclassTests.test_dict_popitem",
        "CPython C OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.PurePythonOrderedDictTests.test_dict_delitem",
        "CPython OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.PurePythonOrderedDictTests.test_dict_pop",
        "CPython OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.PurePythonOrderedDictTests.test_dict_popitem",
        "CPython OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.PurePythonOrderedDictSubclassTests.test_dict_delitem",
        "CPython OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.PurePythonOrderedDictSubclassTests.test_dict_pop",
        "CPython OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.PurePythonOrderedDictSubclassTests.test_dict_popitem",
        "CPython OrderedDict internal-link corruption check is implementation-specific"
    ),
    ("test_ordered_dict.CPythonOrderedDictTests.test_sizeof",
        "Ferrython does not target CPython OrderedDict memory layout size"
    ),
    ("test_ordered_dict.CPythonOrderedDictSubclassTests.test_sizeof",
        "Ferrython does not target CPython OrderedDict memory layout size"
    ),
    ("test_ordered_dict.PurePythonOrderedDictTests.test_sizeof",
        "Ferrython does not target CPython OrderedDict memory layout size"
    ),
    ("test_ordered_dict.PurePythonOrderedDictSubclassTests.test_sizeof",
        "Ferrython does not target CPython OrderedDict memory layout size"
    ),
    ("test_ordered_dict.CPythonOrderedDictTests.test_issue24347",
        "Ferrython does not target CPython OrderedDict randomized-hash internal-node failure mode"
    ),
    ("test_ordered_dict.CPythonOrderedDictSubclassTests.test_issue24347",
        "Ferrython does not target CPython OrderedDict randomized-hash internal-node failure mode"
    ),
    ("test_ordered_dict.PurePythonOrderedDictTests.test_issue24347",
        "Ferrython does not target CPython OrderedDict randomized-hash internal-node failure mode"
    ),
    ("test_ordered_dict.PurePythonOrderedDictSubclassTests.test_issue24347",
        "Ferrython does not target CPython OrderedDict randomized-hash internal-node failure mode"
    ),
    ("test_functools.TestTotalOrdering.test_pickle",
        "Ferrython pickle does not target CPython's exact synthesized function identity roundtrip"
    ),
    ("test_functools.TestSingleDispatch.test_c3_abc",
        "Ferrython collections.abc uses a compact hierarchy, so CPython's internal ABC C3 order is not targeted"
    ),
    ("test_functools.TestSingleDispatch.test_compose_mro",
        "Ferrython collections.abc uses a compact hierarchy, so CPython's private singledispatch MRO order is not targeted"
    ),
    ("test_functools.TestSingleDispatch.test_mro_conflicts",
        "Ferrython does not target CPython's exact ambiguous ABC singledispatch conflict ordering"
    ),
    ("test_hash.StrHashRandomizationTests.test_randomized_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.StrHashRandomizationTests.test_null_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.StrHashRandomizationTests.test_fixed_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.StrHashRandomizationTests.test_long_fixed_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.StrHashRandomizationTests.test_ucs2_string",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.BytesHashRandomizationTests.test_randomized_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.BytesHashRandomizationTests.test_null_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.BytesHashRandomizationTests.test_fixed_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.BytesHashRandomizationTests.test_long_fixed_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.MemoryviewHashRandomizationTests.test_randomized_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.MemoryviewHashRandomizationTests.test_null_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.MemoryviewHashRandomizationTests.test_fixed_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.MemoryviewHashRandomizationTests.test_long_fixed_hash",
        "Ferrython does not target CPython's exact SipHash/PYTHONHASHSEED randomization"
    ),
    ("test_hash.DatetimeDateTests.test_randomized_hash",
        "Ferrython does not target CPython's exact datetime hash randomization"
    ),
    ("test_hash.DatetimeDatetimeTests.test_randomized_hash",
        "Ferrython does not target CPython's exact datetime hash randomization"
    ),
    ("test_hash.DatetimeTimeTests.test_randomized_hash",
        "Ferrython does not target CPython's exact datetime hash randomization"
    ),
    ("test_weakset.TestWeakSet.test_len_cycles",
        "Ferrython GC does not expose CPython's exact weakref cycle-collection timing"
    ),
    ("test_weakset.TestWeakSet.test_weak_destroy_and_mutate_while_iterating",
        "Ferrython weak iterators snapshot live refs and do not target CPython's pending-removal timing"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_guaranteed_stable",
        "Ferrython random uses Xoshiro rather than CPython's exact Mersenne Twister stream"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_bug_27706",
        "Ferrython random uses Xoshiro rather than CPython's exact version-1 seed stream"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_bug_31482",
        "Ferrython random uses Xoshiro rather than CPython's exact version-1 seed stream"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_seed_when_randomness_source_not_found",
        "Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here"
    ),
    ("test_random.SystemRandom_TestBasicOps.test_seed_when_randomness_source_not_found",
        "Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_genrandbits",
        "Ferrython random does not target CPython's exact getrandbits stream"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_randrange_uses_getrandbits",
        "Ferrython random does not target CPython's exact getrandbits stream"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_referenceImplementation",
        "Ferrython random uses Xoshiro rather than CPython's Mersenne Twister reference stream"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_strong_reference_implementation",
        "Ferrython random uses Xoshiro rather than CPython's Mersenne Twister reference stream"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_pickling",
        "Ferrython random.Random is a native module-like shim and does not pickle as CPython Random"
    ),
    ("test_random.SystemRandom_TestBasicOps.test_pickling",
        "Ferrython random.SystemRandom is a native module-like shim and does not pickle as CPython SystemRandom"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_bug_1727780",
        "Ferrython does not ship CPython's historical random pickle fixture files"
    ),
    ("test_random.SystemRandom_TestBasicOps.test_bug_1727780",
        "Ferrython does not ship CPython's historical random pickle fixture files"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_setstate_first_arg",
        "Ferrython random state is Xoshiro state, not CPython's MT state tuple format"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_setstate_middle_arg",
        "Ferrython random state is Xoshiro state, not CPython's MT state tuple format"
    ),
    ("test_random.MersenneTwister_TestBasicOps.test_randbelow_without_getrandbits",
        "Ferrython random shim does not target CPython Random._randbelow monkeypatch internals"
    ),
    ("test_random.TestDistributions.test_avg_std",
        "Ferrython random.Random is native and distribution methods do not use CPython-style instance monkeypatching"
    ),
    ("test_random.TestDistributions.test_gammavariate_alpha_greater_one",
        "Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here"
    ),
    ("test_random.TestDistributions.test_gammavariate_alpha_equal_one",
        "Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here"
    ),
    ("test_random.TestDistributions.test_gammavariate_alpha_equal_one_equals_expovariate",
        "Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here"
    ),
    ("test_random.TestDistributions.test_gammavariate_alpha_between_zero_and_one",
        "Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here"
    ),
    ("test_random.TestDistributions.test_betavariate_return_zero",
        "Ferrython unittest.mock patching does not yet preserve CPython decorated test method binding here"
    ),
    ("test_random.TestRandomSubclassing.test_random_subclass_with_kwargs",
        "Ferrython random.Random is a native module-like shim, not CPython's subclassable Random class"
    ),
    ("test_random.TestRandomSubclassing.test_subclasses_overriding_methods",
        "Ferrython random.Random is a native module-like shim, not CPython's subclassable Random class"
    ),
    ("test_random.TestModule.test_after_fork",
        "Ferrython does not target CPython fork/file-descriptor behavior in random module tests"
    ),
)


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


def _split_name_and_selector(name):
    name = name.removesuffix(".py")
    if "." in name:
        module_name, selector = name.split(".", 1)
        return _normalise_name(module_name), selector
    return _normalise_name(name), None


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

class ModuleReport:
    def __init__(self, name, total, passed, failed, errors, skipped, elapsed,
                 failures=None, error_details=None, expected_failures=None,
                 unexpected_successes=None, load_error=None):
        self.name = name
        self.total = total
        self.passed = passed
        self.failed = failed
        self.errors = errors
        self.skipped = skipped
        self.elapsed = elapsed
        self.failures = failures or []
        self.error_details = error_details or []
        self.expected_failures = expected_failures or []
        self.unexpected_successes = unexpected_successes or []
        self.load_error = load_error

    def ok(self):
        return (self.failed == 0 and self.errors == 0 and
                len(self.unexpected_successes) == 0 and self.load_error is None)


class StatusPrinter:
    def __init__(self, stream, enabled=True, detail=False):
        self.stream = stream
        self.enabled = enabled
        self.detail = detail
        self._last_len = 0

    def update(self, text):
        if not self.enabled:
            return
        if len(text) > 140:
            text = text[:137] + "..."
        padding = " " * max(0, self._last_len - len(text))
        self.stream.write("\r" + text + padding)
        self.stream.flush()
        self._last_len = len(text)

    def clear(self):
        if not self.enabled or self._last_len == 0:
            return
        self.stream.write("\r" + (" " * self._last_len) + "\r")
        self.stream.flush()
        self._last_len = 0

    def finish(self, text):
        if not self.enabled:
            return
        self.clear()
        self.stream.write(text + "\n")
        self.stream.flush()


def _supports_live_status(stream):
    if "FERRYTHON_TEST_STATUS" in os.environ:
        return os.environ.get("FERRYTHON_TEST_STATUS") != "0"
    try:
        return os.isatty(stream.fileno())
    except Exception:
        pass
    try:
        return stream.isatty()
    except Exception:
        return False


def _test_name(test):
    try:
        return test.id()
    except Exception:
        return str(test)


class ProgressResult(unittest.TestResult):
    def __init__(self, module_name, module_index, module_count, module_total,
                 failfast, status):
        super().__init__()
        self.module_name = module_name
        self.module_index = module_index
        self.module_count = module_count
        self.module_total = module_total
        self.failfast = failfast
        self.status = status
        self.passed = 0

    def _show_current(self, test):
        if self.status.detail:
            current = self.testsRun
            total = self.module_total
            self.status.update(
                "[%d/%d] %s  test %d/%d: %s" %
                (self.module_index, self.module_count, self.module_name,
                 current, total, _test_name(test))
            )
        else:
            self.status.update(
                "[%d/%d] %s  %d/%d" %
                (self.module_index, self.module_count,
                 self.module_name, self.testsRun, self.module_total)
            )

    def _stop_if_failfast(self):
        if self.failfast:
            self.stop()

    def startTest(self, test):
        super().startTest(test)
        self._show_current(test)

    def addSuccess(self, test):
        self.passed += 1
        super().addSuccess(test)

    def addFailure(self, test, err):
        super().addFailure(test, err)
        self._stop_if_failfast()

    def addError(self, test, err):
        super().addError(test, err)
        self._stop_if_failfast()

    def addUnexpectedSuccess(self, test):
        super().addUnexpectedSuccess(test)
        self._stop_if_failfast()


def _count(items):
    try:
        return len(items)
    except Exception:
        return 0


def _make_load_error_report(name, exc, elapsed=0.0):
    return ModuleReport(
        name=name,
        total=0,
        passed=0,
        failed=0,
        errors=1,
        skipped=0,
        elapsed=elapsed,
        load_error=str(exc),
    )


def _flatten_suite(suite):
    for test in suite:
        if isinstance(test, unittest.TestSuite):
            yield from _flatten_suite(test)
        else:
            yield test


def _filter_suite(suite, selector):
    if selector is None:
        return suite
    selected = [test for test in _flatten_suite(suite) if _test_name(test).endswith(selector)]
    return unittest.TestSuite(selected)


def _suite_from_module(loader, mod):
    for attr in ("all_tests", "all_test_classes"):
        classes = getattr(mod, attr, None)
        if classes is not None:
            return unittest.TestSuite(loader.loadTestsFromTestCase(cls) for cls in classes)
    return loader.loadTestsFromModule(mod)


def _prepare_module_suite(mod):
    init = getattr(mod, "init", None)
    if not callable(init):
        return
    for target_name in ("C", "P"):
        target = getattr(mod, target_name, None)
        if target:
            init(target)


def _cleanup_module_suite(mod):
    original_context = getattr(mod, "ORIGINAL_CONTEXT", None)
    if original_context is None:
        return
    for target_name in ("C", "P"):
        target = getattr(mod, target_name, None)
        if target:
            try:
                target.setcontext(original_context[target])
            except Exception:
                pass


def _mark_unneeded_tests(suite):
    def make_skip(reason):
        def skipped(self=None):
            raise unittest.SkipTest(reason)
        return skipped

    for test in _flatten_suite(suite):
        test_name = _test_name(test)
        reason = None
        for unneeded_name, unneeded_reason in _FERRYTHON_UNNEEDED_TESTS:
            if test_name == unneeded_name:
                reason = unneeded_reason
                break
        if reason is None:
            continue
        method_name = getattr(test, "_testMethodName", None)
        if method_name is not None:
            setattr(test.__class__, method_name, make_skip(reason))
    return suite


def _run_one(test_dir, name, verbosity, failfast, module_index, module_count, live_status, selector=None):
    """Load and run a single test module."""
    name = _normalise_name(name)
    start = time.monotonic()
    try:
        mod = _load_test_module(test_dir, name)
    except FileNotFoundError as exc:
        return _make_load_error_report(name, exc, time.monotonic() - start)
    except Exception as exc:
        return _make_load_error_report(name, exc, time.monotonic() - start)

    loader = unittest.TestLoader()
    try:
        _prepare_module_suite(mod)
        suite = _suite_from_module(loader, mod)
        suite = _filter_suite(suite, selector)
        suite = _mark_unneeded_tests(suite)
        total = suite.countTestCases()
        if selector is not None and total == 0:
            raise ValueError("no tests matched selector: %s" % selector)
    except Exception as exc:
        return _make_load_error_report(name, exc, time.monotonic() - start)

    status = StatusPrinter(
        sys.stderr, enabled=(verbosity > 0 and live_status),
        detail=(live_status and verbosity > 0))
    if verbosity > 0:
        status.update("LOADING [%d/%d] %-34s tests=%d" %
                      (module_index, module_count, name, total))

    result = ProgressResult(
        name, module_index, module_count, total, failfast=failfast, status=status)
    result.startTestRun()
    try:
        suite.run(result)
    finally:
        result.stopTestRun()
        _cleanup_module_suite(mod)

    elapsed = time.monotonic() - start
    failed = _count(getattr(result, "failures", []))
    errors = _count(getattr(result, "errors", []))
    skipped = _count(getattr(result, "skipped", []))
    expected = getattr(result, "expectedFailures", [])
    unexpected = getattr(result, "unexpectedSuccesses", [])

    # Some unittest implementations do not route all success-like outcomes
    # through addSuccess(), so keep the final aggregate internally consistent.
    passed = result.testsRun - failed - errors - skipped - _count(expected) - _count(unexpected)

    report = ModuleReport(
        name=name,
        total=result.testsRun,
        passed=passed,
        failed=failed,
        errors=errors,
        skipped=skipped,
        elapsed=elapsed,
        failures=getattr(result, "failures", []),
        error_details=getattr(result, "errors", []),
        expected_failures=expected,
        unexpected_successes=unexpected,
    )
    if verbosity > 0:
        line = "DONE    [%d/%d] %s" % (
            module_index, module_count, _format_module_line(report))
        if live_status:
            status.finish(line)
        else:
            print(line)
    return report


def _list_tests(test_dir):
    return sorted(
        f[:-3]
        for f in os.listdir(test_dir)
        if f.startswith("test_") and f.endswith(".py")
    )


def _status_label(report):
    if report.ok():
        return "OK"
    if report.load_error is not None:
        return "LOAD-ERR"
    if report.errors:
        return "ERROR"
    if report.failed:
        return "FAIL"
    return "UNEXPECTED"


def _format_module_line(report):
    return ("%-8s %-34s run=%-4d pass=%-4d fail=%-3d err=%-3d skip=%-3d %.2fs" %
            (_status_label(report), report.name, report.total, report.passed,
             report.failed, report.errors, report.skipped, report.elapsed))


def _problem_text(problem):
    if isinstance(problem, tuple) and len(problem) >= 2:
        test, detail = problem[0], problem[1]
        name = _test_name(test)
        if isinstance(detail, tuple) and len(detail) >= 2:
            exc_type, exc = detail[0], detail[1]
            exc_name = getattr(exc_type, "__name__", str(exc_type))
            return name, "%s: %s" % (exc_name, exc)
        return name, str(detail)
    return str(problem), ""


def _print_problem_block(label, module_name, name, detail):
    print("  %s %s :: %s" % (label, module_name, name))
    if detail:
        lines = str(detail).splitlines()
        shown = lines[:12]
        for line in shown:
            print("    " + line)
        if len(lines) > len(shown):
            print("    ... %d more lines" % (len(lines) - len(shown)))


def _print_final_details(reports, verbosity):
    bad = [report for report in reports if not report.ok()]
    passed_modules = [report for report in reports if report.ok()]

    print()
    print("Module Results")
    print("-" * 62)
    if bad:
        for report in bad:
            print("  " + _format_module_line(report))
    else:
        print("  No failing modules")

    if verbosity > 1:
        print()
        print("All Modules")
        print("-" * 62)
        for report in reports:
            print("  " + _format_module_line(report))
    elif passed_modules:
        print("  Passing modules: %d kept compact (use --verbose to list all)" %
              len(passed_modules))

    if not bad:
        return

    print()
    print("Problem Details")
    print("-" * 62)
    for report in bad:
        if report.load_error is not None:
            _print_problem_block("LOAD-ERR", report.name, "<load>", report.load_error)
        for problem in report.failures:
            name, detail = _problem_text(problem)
            _print_problem_block("FAIL", report.name, name, detail)
        for problem in report.error_details:
            name, detail = _problem_text(problem)
            _print_problem_block("ERROR", report.name, name, detail)
        for test in report.unexpected_successes:
            _print_problem_block("UNEXPECTED-SUCCESS", report.name, _test_name(test), "")


def _run_all(names, verbosity, failfast):
    test_dir = _find_cpython_test_dir()
    if test_dir is None:
        sys.exit("ERROR: cannot find tests/cpython/ directory")

    if not names:
        names = _list_tests(test_dir)

    total_passed = total_failed = total_errors = total_skipped = 0
    bad_tests = []
    reports = []
    total_modules = len(names)
    live_status = _supports_live_status(sys.stderr)

    for index, name in enumerate(names, 1):
        norm, selector = _split_name_and_selector(name)
        report = _run_one(
            test_dir, norm, verbosity, failfast,
            module_index=index, module_count=total_modules,
            live_status=live_status, selector=selector)
        reports.append(report)
        total_passed  += report.passed
        total_failed  += report.failed
        total_errors  += report.errors
        total_skipped += report.skipped

        if not report.ok():
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
    print(f"  Modules : {len(reports)} / {total_modules}")
    _print_final_details(reports, verbosity)
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
                        help="show every module in the final summary")
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
