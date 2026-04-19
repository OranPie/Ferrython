"""unittest — Test framework for Ferrython (CPython-compatible API)."""

__all__ = [
    'TestCase', 'TestResult', 'TestSuite', 'TestLoader', 'TextTestRunner',
    'main', 'skip', 'skipIf', 'skipUnless', 'expectedFailure', 'SkipTest',
    'installHandler', 'registerResult', 'removeResult',
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
        self.expectedFailures = []
        self.unexpectedSuccesses = []
        self.testsRun = 0

    def wasSuccessful(self):
        return len(self.failures) == 0 and len(self.errors) == 0

    def addSuccess(self, test):
        self.testsRun += 1

    def addFailure(self, test, err):
        self.testsRun += 1
        self.failures.append((test, err))

    def addError(self, test, err):
        self.testsRun += 1
        self.errors.append((test, err))

    def addSkip(self, test, reason):
        self.testsRun += 1
        self.skipped.append((test, reason))

    def addExpectedFailure(self, test, err):
        self.testsRun += 1
        self.expectedFailures.append((test, err))

    def addUnexpectedSuccess(self, test):
        self.testsRun += 1
        self.unexpectedSuccesses.append(test)

    def __repr__(self):
        return ("<TestResult run=%d failures=%d errors=%d>" %
                (self.testsRun, len(self.failures), len(self.errors)))


class TestCase:
    """Base class for test cases."""

    maxDiff = 640

    def __init__(self, methodName='runTest'):
        self._testMethodName = methodName
        self._skip_reason = None
        self._cleanups = []
        self._outcome = None

    def setUp(self):
        pass

    def tearDown(self):
        pass

    def setUpClass(cls):
        pass

    def tearDownClass(cls):
        pass

    def id(self):
        return "%s.%s" % (type(self).__name__, self._testMethodName)

    def __str__(self):
        return "%s (%s)" % (self._testMethodName, type(self).__name__)

    def __repr__(self):
        return "<%s testMethod=%s>" % (type(self).__name__, self._testMethodName)

    def shortDescription(self):
        """Returns first line of test method's docstring, or None."""
        method = getattr(self, self._testMethodName, None)
        if method is not None:
            doc = getattr(method, '__doc__', None)
            if doc:
                return doc.strip().split('\n')[0].strip()
        return None

    def addCleanup(self, function, *args, **kwargs):
        """Register a cleanup function to be called after tearDown."""
        self._cleanups.append((function, args, kwargs))

    def doCleanups(self):
        """Execute all cleanup functions registered via addCleanup."""
        result = True
        while self._cleanups:
            function, args, kwargs = self._cleanups.pop()
            try:
                function(*args, **kwargs)
            except Exception:
                result = False
        return result

    def debug(self):
        """Run the test without collecting errors in a TestResult."""
        self.setUp()
        getattr(self, self._testMethodName)()
        self.tearDown()
        self.doCleanups()

    def run(self, result=None):
        if result is None:
            result = TestResult()
        method = getattr(self, self._testMethodName)

        try:
            self.setUp()
        except SkipTest as e:
            result.addSkip(self, str(e))
            return result
        except Exception as e:
            result.addError(self, e)
            return result

        ok = False
        try:
            method()
            ok = True
        except SkipTest as e:
            result.addSkip(self, str(e))
        except AssertionError as e:
            result.addFailure(self, e)
        except Exception as e:
            result.addError(self, e)

        if ok:
            result.addSuccess(self)

        try:
            self.tearDown()
        except Exception as e:
            result.addError(self, e)

        self.doCleanups()

        return result

    def countTestCases(self):
        return 1

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

    def assertGreaterEqual(self, a, b, msg=None):
        if not (a >= b):
            m = msg or ("%r not greater than or equal to %r" % (a, b))
            raise AssertionError(m)

    def assertLess(self, a, b, msg=None):
        if not (a < b):
            m = msg or ("%r not less than %r" % (a, b))
            raise AssertionError(m)

    def assertLessEqual(self, a, b, msg=None):
        if not (a <= b):
            m = msg or ("%r not less than or equal to %r" % (a, b))
            raise AssertionError(m)

    def assertAlmostEqual(self, first, second, places=7, msg=None):
        if round(abs(second - first), places) != 0:
            m = msg or ("%r != %r within %d places" % (first, second, places))
            raise AssertionError(m)

    def assertNotAlmostEqual(self, first, second, places=7, msg=None):
        if round(abs(second - first), places) == 0:
            m = msg or ("%r == %r within %d places" % (first, second, places))
            raise AssertionError(m)

    def assertIsInstance(self, obj, cls, msg=None):
        if not isinstance(obj, cls):
            m = msg or ("%r is not an instance of %r" % (obj, cls))
            raise AssertionError(m)

    def assertNotIsInstance(self, obj, cls, msg=None):
        if isinstance(obj, cls):
            m = msg or ("%r is an instance of %r" % (obj, cls))
            raise AssertionError(m)

    def assertCountEqual(self, first, second, msg=None):
        if sorted(first) != sorted(second):
            m = msg or ("Element counts differ: %r vs %r" % (first, second))
            raise AssertionError(m)

    def assertRegex(self, text, regex, msg=None):
        import re
        if not re.search(regex, text):
            m = msg or ("Regex %r didn't match %r" % (regex, text))
            raise AssertionError(m)

    def assertNotRegex(self, text, regex, msg=None):
        import re
        if re.search(regex, text):
            m = msg or ("Regex %r unexpectedly matched %r" % (regex, text))
            raise AssertionError(m)

    def assertDictEqual(self, d1, d2, msg=None):
        if d1 != d2:
            m = msg or ("%r != %r" % (d1, d2))
            raise AssertionError(m)

    def assertListEqual(self, list1, list2, msg=None):
        if list1 != list2:
            m = msg or ("%r != %r" % (list1, list2))
            raise AssertionError(m)

    def assertTupleEqual(self, tuple1, tuple2, msg=None):
        if tuple1 != tuple2:
            m = msg or ("%r != %r" % (tuple1, tuple2))
            raise AssertionError(m)

    def assertSetEqual(self, set1, set2, msg=None):
        if set1 != set2:
            m = msg or ("%r != %r" % (set1, set2))
            raise AssertionError(m)

    def assertSequenceEqual(self, seq1, seq2, msg=None):
        if list(seq1) != list(seq2):
            m = msg or ("%r != %r" % (seq1, seq2))
            raise AssertionError(m)

    def assertMultiLineEqual(self, first, second, msg=None):
        if first != second:
            m = msg or ("%r != %r" % (first, second))
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

    def assertRaisesRegex(self, exc_type, expected_regex, callable_obj=None, *args, **kwargs):
        """Assert that a regex matches the string representation of the raised exception."""
        if callable_obj is not None:
            import re
            try:
                callable_obj(*args, **kwargs)
            except exc_type as e:
                if not re.search(expected_regex, str(e)):
                    raise AssertionError(
                        '"%s" does not match "%s"' % (expected_regex, str(e)))
                return
            except Exception as e:
                raise AssertionError(
                    "%s raised instead of %s" % (type(e).__name__, exc_type.__name__))
            raise AssertionError("%s not raised" % exc_type.__name__)
        return _AssertRaisesRegexContext(self, exc_type, expected_regex)

    def assertWarns(self, warning_type, callable_obj=None, *args, **kwargs):
        # Simplified: just run the callable (Ferrython doesn't track warnings yet)
        if callable_obj is not None:
            callable_obj(*args, **kwargs)
        return _NullContext()

    def assertWarnsRegex(self, warning_type, expected_regex, callable_obj=None, *args, **kwargs):
        """Simplified assertWarnsRegex — Ferrython doesn't track warnings yet."""
        if callable_obj is not None:
            callable_obj(*args, **kwargs)
        return _NullContext()

    def assertLogs(self, logger=None, level=None):
        """Context manager to assert that log messages are emitted."""
        return _AssertLogsContext(self, logger, level)

    def assertNoLogs(self, logger=None, level=None):
        """Context manager to assert that no log messages are emitted."""
        return _AssertNoLogsContext(self, logger, level)

    def fail(self, msg=None):
        raise AssertionError(msg or "Test failed")

    def skipTest(self, reason):
        raise SkipTest(reason)

    def subTest(self, msg=None, **params):
        return _SubTest(self, msg, params)


class _NullContext:
    """No-op context manager for stubs."""
    def __enter__(self):
        return self
    def __exit__(self, *args):
        return False


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


class _AssertRaisesRegexContext:
    """Context manager for assertRaisesRegex."""

    def __init__(self, test_case, exc_type, expected_regex):
        self._test_case = test_case
        self._exc_type = exc_type
        self._expected_regex = expected_regex

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            raise AssertionError("%s not raised" % self._exc_type.__name__)
        if not issubclass(exc_type, self._exc_type):
            return False
        import re
        if not re.search(self._expected_regex, str(exc_val)):
            raise AssertionError(
                '"%s" does not match "%s"' % (self._expected_regex, str(exc_val)))
        self.exception = exc_val
        return True


class _SubTest:
    """Context manager for subTest — runs a block with parameters."""

    def __init__(self, test_case, msg, params):
        self._test_case = test_case
        self._msg = msg
        self._params = params

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is not None and exc_type is not AssertionError:
            return False
        if exc_type is AssertionError:
            # Record the subtest failure but continue
            params_str = ""
            if self._msg is not None:
                params_str = str(self._msg)
            if self._params:
                p = ", ".join("%s=%r" % (k, v) for k, v in self._params.items())
                if params_str:
                    params_str = "%s [%s]" % (params_str, p)
                else:
                    params_str = p
            return False
        return False


class _AssertLogsContext:
    """Context manager for assertLogs."""

    def __init__(self, test_case, logger=None, level=None):
        self._test_case = test_case
        self._logger_name = logger
        self._level = level or 'INFO'
        self.records = []
        self.output = []

    def __enter__(self):
        import logging
        if self._logger_name is None:
            self._logger = logging.getLogger()
        elif isinstance(self._logger_name, str):
            self._logger = logging.getLogger(self._logger_name)
        else:
            self._logger = self._logger_name
        # Install a capturing handler
        self._handler = _CapturingHandler()
        self._logger.addHandler(self._handler)
        level_map = {'DEBUG': 10, 'INFO': 20, 'WARNING': 30, 'ERROR': 40, 'CRITICAL': 50}
        if isinstance(self._level, str):
            self._level_num = level_map.get(self._level, 20)
        else:
            self._level_num = self._level
        self._old_level = getattr(self._logger, 'level', None)
        self._logger.setLevel(self._level_num)
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self._logger.removeHandler(self._handler)
        if self._old_level is not None:
            self._logger.setLevel(self._old_level)
        if exc_type is not None:
            return False
        self.records = self._handler.records
        self.output = self._handler.output
        if not self.records:
            raise AssertionError("no logs of level %s or above triggered on %s"
                               % (self._level, self._logger_name or 'root'))
        return False


class _CapturingHandler:
    """A logging handler that captures records for assertLogs."""

    def __init__(self):
        self.records = []
        self.output = []
        self.level = 0

    def emit(self, record):
        self.records.append(record)
        msg = getattr(record, 'message', '') or getattr(record, 'msg', '')
        levelname = getattr(record, 'levelname', 'INFO')
        name = getattr(record, 'name', 'root')
        self.output.append("%s:%s:%s" % (levelname, name, msg))

    def setLevel(self, level):
        self.level = level

    def setFormatter(self, fmt):
        pass


class _AssertNoLogsContext:
    """Context manager for assertNoLogs — asserts no log messages are emitted."""

    def __init__(self, test_case, logger=None, level=None):
        self._test_case = test_case
        self._logger_name = logger
        self._level = level or 'INFO'
        self.records = []
        self.output = []

    def __enter__(self):
        import logging
        if self._logger_name is None:
            self._logger = logging.getLogger()
        elif isinstance(self._logger_name, str):
            self._logger = logging.getLogger(self._logger_name)
        else:
            self._logger = self._logger_name
        self._handler = _CapturingHandler()
        self._logger.addHandler(self._handler)
        level_map = {'DEBUG': 10, 'INFO': 20, 'WARNING': 30, 'ERROR': 40, 'CRITICAL': 50}
        if isinstance(self._level, str):
            self._level_num = level_map.get(self._level, 20)
        else:
            self._level_num = self._level
        self._old_level = getattr(self._logger, 'level', None)
        self._logger.setLevel(self._level_num)
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self._logger.removeHandler(self._handler)
        if self._old_level is not None:
            self._logger.setLevel(self._old_level)
        if exc_type is not None:
            return False
        if self._handler.records:
            raise AssertionError(
                "Unexpected logs found: %s" %
                self._handler.output)
        return False


class TestSuite:
    """A collection of test cases."""

    def __init__(self, tests=None):
        self._tests = list(tests) if tests else []

    def addTest(self, test):
        self._tests.append(test)

    def addTests(self, tests):
        for t in tests:
            self.addTest(t)

    def run(self, result=None):
        if result is None:
            result = TestResult()
        # Group tests by class for setUpClass/tearDownClass
        classes_seen = []
        current_class = None
        for test in self._tests:
            test_class = type(test)
            if test_class is not current_class:
                # tearDownClass for previous class
                if current_class is not None:
                    try:
                        tdc = getattr(current_class, 'tearDownClass', None)
                        if tdc is not None:
                            tdc()
                    except Exception:
                        pass
                # setUpClass for new class
                current_class = test_class
                classes_seen.append(current_class)
                try:
                    suc = getattr(current_class, 'setUpClass', None)
                    if suc is not None:
                        suc()
                except Exception as e:
                    result.addError(test, e)
                    continue
            test.run(result)
        # tearDownClass for last class
        if current_class is not None:
            try:
                tdc = getattr(current_class, 'tearDownClass', None)
                if tdc is not None:
                    tdc()
            except Exception:
                pass
        return result

    def countTestCases(self):
        total = 0
        for test in self._tests:
            if hasattr(test, 'countTestCases'):
                total += test.countTestCases()
            else:
                total += 1
        return total

    def __iter__(self):
        return iter(self._tests)

    def __len__(self):
        return len(self._tests)


class TestLoader:
    """Load tests from TestCase classes."""

    def loadTestsFromTestCase(self, testCaseClass):
        test_names = self.getTestCaseNames(testCaseClass)
        suite = TestSuite()
        for name in test_names:
            suite.addTest(testCaseClass(name))
        return suite

    def loadTestsFromModule(self, module):
        suite = TestSuite()
        for name in dir(module):
            obj = getattr(module, name)
            if isinstance(obj, type) and issubclass(obj, TestCase) and obj is not TestCase:
                suite.addTest(self.loadTestsFromTestCase(obj))
        return suite

    def getTestCaseNames(self, testCaseClass):
        names = []
        for name in dir(testCaseClass):
            if name.startswith('test'):
                obj = getattr(testCaseClass, name)
                if callable(obj):
                    names.append(name)
        names.sort()
        return names

    def loadTestsFromName(self, name, module=None):
        """Load tests by name (dotted module.Class.method)."""
        import sys
        parts = name.split('.')
        if module is None:
            # Try to import the module
            mod_name = parts[0]
            __import__(mod_name)
            module = sys.modules[mod_name]
            parts = parts[1:]
        obj = module
        for part in parts:
            obj = getattr(obj, part)
        if isinstance(obj, type) and issubclass(obj, TestCase):
            return self.loadTestsFromTestCase(obj)
        elif callable(obj):
            suite = TestSuite()
            suite.addTest(obj())
            return suite
        return TestSuite()

    def loadTestsFromNames(self, names, module=None):
        suites = [self.loadTestsFromName(name, module) for name in names]
        suite = TestSuite()
        for s in suites:
            suite.addTests(s)
        return suite

    def discover(self, start_dir, pattern='test*.py', top_level_dir=None):
        return TestSuite()


class TextTestRunner:
    """A test runner that outputs results to the console."""

    def __init__(self, stream=None, descriptions=True, verbosity=1,
                 failfast=False, resultclass=None):
        self.verbosity = verbosity
        self.failfast = failfast
        self.resultclass = resultclass or TestResult

    def run(self, test):
        result = self.resultclass()
        test.run(result)
        if self.verbosity > 0:
            if result.wasSuccessful():
                print("OK (%d tests)" % result.testsRun)
            else:
                print("FAILED (failures=%d, errors=%d)" % (
                    len(result.failures), len(result.errors)))
        return result


# --- Decorators ---

def skip(reason):
    """Unconditionally skip a test."""
    def decorator(func):
        def wrapper(*args, **kwargs):
            raise SkipTest(reason)
        wrapper.__name__ = func.__name__
        wrapper.__skip_reason__ = reason
        return wrapper
    return decorator


def skipIf(condition, reason):
    """Skip a test if condition is true."""
    if condition:
        return skip(reason)
    return lambda func: func


def skipUnless(condition, reason):
    """Skip a test unless condition is true."""
    if not condition:
        return skip(reason)
    return lambda func: func


def expectedFailure(func):
    """Mark a test as an expected failure."""
    def wrapper(*args, **kwargs):
        try:
            func(*args, **kwargs)
        except Exception:
            return
        raise AssertionError("test unexpectedly succeeded")
    wrapper.__name__ = func.__name__
    return wrapper


def main(module='__main__', exit=True, verbosity=2, testRunner=None, testLoader=None):
    """Simple test runner entry point.
    
    Discovers TestCase subclasses in the calling module and runs them.
    """
    import sys
    if testLoader is None:
        testLoader = TestLoader()
    if testRunner is None:
        testRunner = TextTestRunner(verbosity=verbosity)

    # Try to discover tests from the calling module
    if module == '__main__':
        # Get the __main__ module from sys.modules
        mod = sys.modules.get('__main__', None)
    elif isinstance(module, str):
        mod = sys.modules.get(module, None)
    else:
        mod = module

    if mod is not None:
        suite = TestSuite()
        for name in dir(mod):
            obj = getattr(mod, name, None)
            if obj is None:
                continue
            if isinstance(obj, type) and issubclass(obj, TestCase) and obj is not TestCase:
                loaded = testLoader.loadTestsFromTestCase(obj)
                suite.addTests(loaded)
        result = testRunner.run(suite)
        if exit and not result.wasSuccessful():
            sys.exit(1)
        return result
    else:
        print("unittest.main() — no module found for test discovery")


# --- Compatibility stubs ---

def installHandler():
    """Install a signal handler to catch Ctrl-C during test runs. (stub)"""
    pass

def registerResult(result):
    """Register a TestResult for cleanup. (stub)"""
    pass

def removeResult(result):
    """Remove a registered TestResult. (stub)"""
    pass
