"""Basic test framework for Ferrython."""

__all__ = [
    'TestCase', 'TestResult', 'TestSuite', 'main',
    'skip', 'expectedFailure',
]


class SkipTest(Exception):
    """Raised to skip a test."""
    pass


class _ExpectedFailure(Exception):
    """Wrapper for expected failures."""
    pass


class TestResult:
    """Holder for test result information."""

    def __init__(self):
        self.failures = []
        self.errors = []
        self.skipped = []
        self.expected_failures = []
        self.unexpected_successes = []
        self.tests_run = 0

    def wasSuccessful(self):
        return len(self.failures) == 0 and len(self.errors) == 0

    def addSuccess(self, test):
        self.tests_run += 1

    def addFailure(self, test, err):
        self.tests_run += 1
        self.failures.append((test, err))

    def addError(self, test, err):
        self.tests_run += 1
        self.errors.append((test, err))

    def addSkip(self, test, reason):
        self.tests_run += 1
        self.skipped.append((test, reason))

    def __repr__(self):
        return ("<TestResult run=%d failures=%d errors=%d>" %
                (self.tests_run, len(self.failures), len(self.errors)))


class TestCase:
    """Base class for test cases."""

    def __init__(self, methodName='runTest'):
        self._methodName = methodName
        self._skip_reason = None

    def setUp(self):
        pass

    def tearDown(self):
        pass

    def run(self, result=None):
        if result is None:
            result = TestResult()
        method = getattr(self, self._methodName)

        try:
            self.setUp()
        except SkipTest as e:
            result.addSkip(self, str(e))
            return result
        except Exception as e:
            result.addError(self, e)
            return result

        try:
            method()
            result.addSuccess(self)
        except SkipTest as e:
            result.addSkip(self, str(e))
        except AssertionError as e:
            result.addFailure(self, e)
        except Exception as e:
            result.addError(self, e)

        try:
            self.tearDown()
        except Exception as e:
            result.addError(self, e)

        return result

    # --- Assertion methods ---

    def assertEqual(self, first, second, msg=None):
        if first != second:
            m = msg or ("%r != %r" % (first, second))
            raise AssertionError(m)

    def assertNotEqual(self, first, second, msg=None):
        if first == second:
            m = msg or ("%r == %r" % (first, second))
            raise AssertionError(m)

    def assertTrue(self, expr, msg=None):
        if not expr:
            m = msg or ("%r is not true" % (expr,))
            raise AssertionError(m)

    def assertFalse(self, expr, msg=None):
        if expr:
            m = msg or ("%r is not false" % (expr,))
            raise AssertionError(m)

    def assertIs(self, a, b, msg=None):
        if a is not b:
            m = msg or ("%r is not %r" % (a, b))
            raise AssertionError(m)

    def assertIsNot(self, a, b, msg=None):
        if a is b:
            m = msg or ("%r is %r" % (a, b))
            raise AssertionError(m)

    def assertIsNone(self, obj, msg=None):
        if obj is not None:
            m = msg or ("%r is not None" % (obj,))
            raise AssertionError(m)

    def assertIsNotNone(self, obj, msg=None):
        if obj is None:
            m = msg or "unexpectedly None"
            raise AssertionError(m)

    def assertIn(self, member, container, msg=None):
        if member not in container:
            m = msg or ("%r not found in %r" % (member, container))
            raise AssertionError(m)

    def assertNotIn(self, member, container, msg=None):
        if member in container:
            m = msg or ("%r unexpectedly found in %r" % (member, container))
            raise AssertionError(m)

    def assertGreater(self, a, b, msg=None):
        if not (a > b):
            m = msg or ("%r not greater than %r" % (a, b))
            raise AssertionError(m)

    def assertLess(self, a, b, msg=None):
        if not (a < b):
            m = msg or ("%r not less than %r" % (a, b))
            raise AssertionError(m)

    def assertRaises(self, exc_type, callable_obj=None, *args, **kwargs):
        if callable_obj is not None:
            try:
                callable_obj(*args, **kwargs)
            except exc_type:
                return
            except Exception as e:
                raise AssertionError(
                    "%s raised instead of %s" % (type(e).__name__, exc_type.__name__))
            raise AssertionError("%s not raised" % exc_type.__name__)
        return _AssertRaisesContext(self, exc_type)

    def fail(self, msg=None):
        raise AssertionError(msg or "Test failed")

    def skipTest(self, reason):
        raise SkipTest(reason)


class _AssertRaisesContext:
    """Context manager for assertRaises."""

    def __init__(self, test_case, exc_type):
        self._test_case = test_case
        self._exc_type = exc_type

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            raise AssertionError("%s not raised" % self._exc_type.__name__)
        if not issubclass(exc_type, self._exc_type):
            return False
        self.exception = exc_val
        return True


class TestSuite:
    """A collection of test cases."""

    def __init__(self, tests=None):
        self._tests = list(tests) if tests else []

    def addTest(self, test):
        self._tests.append(test)

    def run(self, result=None):
        if result is None:
            result = TestResult()
        for test in self._tests:
            test.run(result)
        return result

    def __iter__(self):
        return iter(self._tests)


def skip(reason):
    """Decorator to unconditionally skip a test."""
    def decorator(func):
        def wrapper(*args, **kwargs):
            raise SkipTest(reason)
        wrapper.__name__ = func.__name__
        return wrapper
    return decorator


def expectedFailure(func):
    """Decorator to mark a test as an expected failure."""
    def wrapper(*args, **kwargs):
        try:
            func(*args, **kwargs)
        except Exception:
            return
        raise AssertionError("test unexpectedly succeeded")
    wrapper.__name__ = func.__name__
    return wrapper


def main():
    """Discover and run TestCase subclasses (stub for Ferrython)."""
    print("unittest.main() stub — use explicit test running in Ferrython")


class AssertionError(Exception):
    """Assertion error for test failures."""
    pass
