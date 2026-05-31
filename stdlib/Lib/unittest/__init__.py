"""unittest — Test framework for Ferrython (CPython-compatible API)."""

__all__ = [
    'TestCase', 'TestResult', 'TestSuite', 'TestLoader', 'TextTestRunner',
    'main', 'skip', 'skipIf', 'skipUnless', 'expectedFailure', 'SkipTest',
    'FunctionTestCase', 'installHandler', 'registerResult', 'removeResult', 'makeSuite',
]


class SkipTest(Exception):
    """Raised to skip a test."""
    pass


class _ExpectedFailure(Exception):
    """Wrapper for expected failures."""
    pass


def _exc_name(exc_type):
    if isinstance(exc_type, tuple):
        return "(" + ", ".join(_exc_name(item) for item in exc_type) + ")"
    return getattr(exc_type, "__name__", str(exc_type))


def _safe_repr(obj):
    try:
        return repr(obj)
    except Exception:
        return object.__repr__(obj)


class TestResult:
    """Holder for test result information."""

    def __init__(self):
        self.failures = []
        self.errors = []
        self.skipped = []
        self.expectedFailures = []
        self.unexpectedSuccesses = []
        self.testsRun = 0
        self.shouldStop = False
        self.buffer = False
        self.tb_locals = False

    def wasSuccessful(self):
        return (len(self.failures) == 0 and len(self.errors) == 0 and
                len(self.unexpectedSuccesses) == 0)

    def startTest(self, test):
        self.testsRun += 1

    def stopTest(self, test):
        pass

    def startTestRun(self):
        pass

    def stopTestRun(self):
        pass

    def stop(self):
        self.shouldStop = True

    def addSuccess(self, test):
        pass

    def addFailure(self, test, err):
        self.failures.append((test, err))

    def addError(self, test, err):
        self.errors.append((test, err))

    def addSkip(self, test, reason):
        self.skipped.append((test, reason))

    def addExpectedFailure(self, test, err):
        self.expectedFailures.append((test, err))

    def addUnexpectedSuccess(self, test):
        self.unexpectedSuccesses.append(test)

    def addSubTest(self, test, subtest, err):
        if err is None:
            return
        if isinstance(err, test.failureException):
            self.addFailure(subtest, err)
        else:
            self.addError(subtest, err)

    def printErrors(self):
        pass

    def __repr__(self):
        return ("<TestResult run=%d failures=%d errors=%d>" %
                (self.testsRun, len(self.failures), len(self.errors)))


class TestCase:
    """Base class for test cases."""

    failureException = AssertionError
    longMessage = True
    maxDiff = 640
    _class_cleanups = []

    def __init__(self, methodName='runTest'):
        self._testMethodName = methodName
        self._skip_reason = None
        self._cleanups = []
        self._outcome = None

    def setUp(self):
        pass

    def tearDown(self):
        pass

    @classmethod
    def setUpClass(cls):
        pass

    @classmethod
    def tearDownClass(cls):
        pass

    @classmethod
    def addClassCleanup(cls, function, *args, **kwargs):
        if "_class_cleanups" not in cls.__dict__:
            cls._class_cleanups = []
        cls._class_cleanups.append((function, args, kwargs))

    @classmethod
    def doClassCleanups(cls):
        result = True
        cleanups = getattr(cls, "_class_cleanups", [])
        while cleanups:
            function, args, kwargs = cleanups.pop()
            try:
                function(*args, **kwargs)
            except BaseException:
                result = False
        return result

    @classmethod
    def enterClassContext(cls, cm):
        result = cm.__enter__()
        cls.addClassCleanup(cm.__exit__, None, None, None)
        return result

    def id(self):
        return "%s.%s.%s" % (
            self.__class__.__module__, type(self).__name__, self._testMethodName)

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

    def enterContext(self, cm):
        result = cm.__enter__()
        self.addCleanup(cm.__exit__, None, None, None)
        return result

    def doCleanups(self):
        """Execute all cleanup functions registered via addCleanup."""
        result = True
        while self._cleanups:
            function, args, kwargs = self._cleanups.pop()
            try:
                function(*args, **kwargs)
            except BaseException:
                result = False
        return result

    def debug(self):
        """Run the test without collecting errors in a TestResult."""
        self.setUp()
        try:
            getattr(self, self._testMethodName)()
        finally:
            try:
                self.tearDown()
            finally:
                self.doCleanups()

    def defaultTestResult(self):
        return TestResult()

    def run(self, result=None):
        if result is None:
            result = self.defaultTestResult()
        method = getattr(self, self._testMethodName)
        result.startTest(self)

        try:
            try:
                self.setUp()
            except SkipTest as e:
                result.addSkip(self, str(e))
                return result
            except BaseException as e:
                result.addError(self, e)
                return result

            ok = False
            try:
                method()
                ok = True
            except SkipTest as e:
                result.addSkip(self, str(e))
            except self.failureException as e:
                result.addFailure(self, e)
            except BaseException as e:
                result.addError(self, e)

            try:
                self.tearDown()
            except BaseException as e:
                ok = False
                result.addError(self, e)

            if self.doCleanups() is False:
                ok = False

            if ok:
                result.addSuccess(self)
        finally:
            result.stopTest(self)

        return result

    def countTestCases(self):
        return 1

    def _formatMessage(self, msg, standardMsg):
        if msg is None:
            return standardMsg
        if not self.longMessage:
            return msg
        return "%s : %s" % (standardMsg, msg)

    def _fail(self, msg=None):
        raise self.failureException(msg or "Test failed")

    # --- Assertion methods ---

    def assertEqual(self, first, second, msg=None):
        if first != second:
            self._fail(self._formatMessage(msg, "%r != %r" % (first, second)))

    assertEquals = assertEqual
    failUnlessEqual = assertEqual

    def assertNotEqual(self, first, second, msg=None):
        if first == second:
            self._fail(self._formatMessage(msg, "%r == %r" % (first, second)))

    assertNotEquals = assertNotEqual
    failIfEqual = assertNotEqual

    def assertTrue(self, expr, msg=None):
        if not expr:
            self._fail(self._formatMessage(msg, "%r is not true" % (expr,)))

    assert_ = assertTrue
    failUnless = assertTrue

    def assertFalse(self, expr, msg=None):
        if expr:
            self._fail(self._formatMessage(msg, "%r is not false" % (expr,)))

    failIf = assertFalse

    def assertIs(self, a, b, msg=None):
        if a is not b:
            self._fail(self._formatMessage(msg, "%r is not %r" % (a, b)))

    def assertIsNot(self, a, b, msg=None):
        if a is b:
            self._fail(self._formatMessage(msg, "%r is %r" % (a, b)))

    def assertIsNone(self, obj, msg=None):
        if obj is not None:
            self._fail(self._formatMessage(msg, "%r is not None" % (obj,)))

    def assertIsNotNone(self, obj, msg=None):
        if obj is None:
            self._fail(self._formatMessage(msg, "unexpectedly None"))

    def assertIn(self, member, container, msg=None):
        if member not in container:
            self._fail(self._formatMessage(msg, "%r not found in %r" % (member, container)))

    def assertNotIn(self, member, container, msg=None):
        if member in container:
            self._fail(self._formatMessage(
                msg, "%r unexpectedly found in %r" % (member, container)))

    def assertGreater(self, a, b, msg=None):
        if not (a > b):
            self._fail(self._formatMessage(msg, "%r not greater than %r" % (a, b)))

    def assertGreaterEqual(self, a, b, msg=None):
        if not (a >= b):
            self._fail(self._formatMessage(
                msg, "%r not greater than or equal to %r" % (a, b)))

    def assertLess(self, a, b, msg=None):
        if not (a < b):
            self._fail(self._formatMessage(msg, "%r not less than %r" % (a, b)))

    def assertLessEqual(self, a, b, msg=None):
        if not (a <= b):
            self._fail(self._formatMessage(
                msg, "%r not less than or equal to %r" % (a, b)))

    def assertAlmostEqual(self, first, second, places=None, msg=None, delta=None):
        if first == second:
            return
        if delta is not None:
            if places is not None:
                raise TypeError("specify delta or places not both")
            if abs(first - second) <= delta:
                return
            standard = "%r != %r within %r delta (%r difference)" % (
                first, second, delta, abs(first - second))
            self._fail(self._formatMessage(msg, standard))
        if places is None:
            places = 7
        if round(abs(second - first), places) != 0:
            self._fail(self._formatMessage(
                msg, "%r != %r within %d places" % (first, second, places)))

    assertAlmostEquals = assertAlmostEqual
    failUnlessAlmostEqual = assertAlmostEqual

    def assertNotAlmostEqual(self, first, second, places=None, msg=None, delta=None):
        if delta is not None:
            if places is not None:
                raise TypeError("specify delta or places not both")
            if abs(first - second) <= delta:
                standard = "%r == %r within %r delta (%r difference)" % (
                    first, second, delta, abs(first - second))
                self._fail(self._formatMessage(msg, standard))
            return
        if places is None:
            places = 7
        if round(abs(second - first), places) == 0:
            self._fail(self._formatMessage(
                msg, "%r == %r within %d places" % (first, second, places)))

    assertNotAlmostEquals = assertNotAlmostEqual
    failIfAlmostEqual = assertNotAlmostEqual

    def assertIsInstance(self, obj, cls, msg=None):
        if not isinstance(obj, cls):
            self._fail(self._formatMessage(
                msg, "%r is not an instance of %r" % (obj, cls)))

    def assertNotIsInstance(self, obj, cls, msg=None):
        if isinstance(obj, cls):
            self._fail(self._formatMessage(
                msg, "%r is an instance of %r" % (obj, cls)))

    def assertCountEqual(self, first, second, msg=None):
        try:
            from collections import Counter
            first_count = Counter(first)
            second_count = Counter(second)
            equal = first_count == second_count
        except Exception:
            first_list = list(first)
            second_list = list(second)
            equal = len(first_list) == len(second_list)
            if equal:
                remaining = list(second_list)
                for item in first_list:
                    if item in remaining:
                        remaining.remove(item)
                    else:
                        equal = False
                        break
        if not equal:
            self._fail(self._formatMessage(
                msg, "Element counts differ: %r vs %r" % (first, second)))

    def assertRegex(self, text, regex, msg=None):
        import re
        if not re.search(regex, text):
            self._fail(self._formatMessage(
                msg, "Regex %r didn't match %r" % (regex, text)))

    assertRegexpMatches = assertRegex

    def assertNotRegex(self, text, regex, msg=None):
        import re
        if re.search(regex, text):
            self._fail(self._formatMessage(
                msg, "Regex %r unexpectedly matched %r" % (regex, text)))

    assertNotRegexpMatches = assertNotRegex

    def assertDictEqual(self, d1, d2, msg=None):
        if d1 != d2:
            self._fail(self._formatMessage(msg, "%r != %r" % (d1, d2)))

    def assertListEqual(self, list1, list2, msg=None):
        if list1 != list2:
            self._fail(self._formatMessage(msg, "%r != %r" % (list1, list2)))

    def assertTupleEqual(self, tuple1, tuple2, msg=None):
        if tuple1 != tuple2:
            self._fail(self._formatMessage(msg, "%r != %r" % (tuple1, tuple2)))

    def assertSetEqual(self, set1, set2, msg=None):
        if set1 != set2:
            self._fail(self._formatMessage(msg, "%r != %r" % (set1, set2)))

    def assertSequenceEqual(self, seq1, seq2, msg=None, seq_type=None):
        if seq_type is not None:
            if not isinstance(seq1, seq_type):
                self._fail("First sequence is not a %s: %r" % (seq_type, seq1))
            if not isinstance(seq2, seq_type):
                self._fail("Second sequence is not a %s: %r" % (seq_type, seq2))
        if list(seq1) != list(seq2):
            self._fail(self._formatMessage(msg, "%r != %r" % (seq1, seq2)))

    def assertMultiLineEqual(self, first, second, msg=None):
        if first != second:
            self._fail(self._formatMessage(msg, "%r != %r" % (first, second)))

    def assertRaises(self, exc_type, callable_obj=None, *args, **kwargs):
        if callable_obj is not None:
            with self.assertRaises(exc_type):
                callable_obj(*args, **kwargs)
            return
        return _AssertRaisesContext(self, exc_type)

    def assertRaisesRegex(self, exc_type, expected_regex, callable_obj=None, *args, **kwargs):
        """Assert that a regex matches the string representation of the raised exception."""
        if callable_obj is not None:
            with self.assertRaisesRegex(exc_type, expected_regex):
                callable_obj(*args, **kwargs)
            return
        return _AssertRaisesRegexContext(self, exc_type, expected_regex)

    def assertWarns(self, warning_type, callable_obj=None, *args, **kwargs):
        context = _AssertWarnsContext(self, warning_type)
        if callable_obj is not None:
            with context:
                callable_obj(*args, **kwargs)
        return context

    def assertWarnsRegex(self, warning_type, expected_regex, callable_obj=None, *args, **kwargs):
        context = _AssertWarnsContext(self, warning_type, expected_regex)
        if callable_obj is not None:
            with context:
                callable_obj(*args, **kwargs)
        return context

    def assertLogs(self, logger=None, level=None):
        """Context manager to assert that log messages are emitted."""
        return _AssertLogsContext(self, logger, level)

    def assertNoLogs(self, logger=None, level=None):
        """Context manager to assert that no log messages are emitted."""
        return _AssertNoLogsContext(self, logger, level)

    def fail(self, msg=None):
        self._fail(msg)

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


class _AssertWarnsContext:
    def __init__(self, test_case, warning_type, expected_regex=None):
        self._test_case = test_case
        self._warning_type = warning_type
        self._expected_regex = expected_regex
        self.warnings = []
        self.warning = None
        self.filename = None
        self.lineno = None

    def __enter__(self):
        import warnings
        self._cm = warnings.catch_warnings(record=True)
        self.warnings = self._cm.__enter__()
        warnings.simplefilter("always", self._warning_type)
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self._cm.__exit__(exc_type, exc_val, exc_tb)
        if exc_type is not None:
            return False
        if not self.warnings:
            self._test_case._fail("%s not triggered" % _exc_name(self._warning_type))
        self.warning = self.warnings[0].message
        self.filename = getattr(self.warnings[0], "filename", None)
        self.lineno = getattr(self.warnings[0], "lineno", None)
        if self._expected_regex is not None:
            import re
            if not re.search(self._expected_regex, str(self.warning)):
                self._test_case._fail(
                    '"%s" does not match "%s"' % (self._expected_regex, str(self.warning)))
        return False


class _AssertRaisesContext:
    """Context manager for assertRaises."""

    def __init__(self, test_case, exc_type):
        self._test_case = test_case
        self._exc_type = exc_type
        self.exception = None

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            self._test_case._fail("%s not raised" % _exc_name(self._exc_type))
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
        self.exception = None

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is None:
            self._test_case._fail("%s not raised" % _exc_name(self._exc_type))
        if not issubclass(exc_type, self._exc_type):
            return False
        import re
        if not re.search(self._expected_regex, str(exc_val)):
            self._test_case._fail(
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
            return False
        return False

    def __str__(self):
        parts = []
        if self._msg is not None:
            parts.append(str(self._msg))
        if self._params:
            parts.append(", ".join("%s=%r" % (k, v) for k, v in self._params.items()))
        suffix = " [%s]" % "; ".join(parts) if parts else ""
        return "%s%s" % (self._test_case, suffix)


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
            if getattr(result, "shouldStop", False):
                break
            test_class = type(test)
            if test_class is not current_class:
                # tearDownClass for previous class
                if current_class is not None:
                    try:
                        tdc = getattr(current_class, 'tearDownClass', None)
                        if tdc is not None:
                            tdc()
                    except BaseException:
                        pass
                # setUpClass for new class
                current_class = test_class
                classes_seen.append(current_class)
                try:
                    suc = getattr(current_class, 'setUpClass', None)
                    if suc is not None:
                        suc()
                except BaseException as e:
                    result.addError(test, e)
                    continue
            test.run(result)
        # tearDownClass for last class
        if current_class is not None:
            try:
                tdc = getattr(current_class, 'tearDownClass', None)
                if tdc is not None:
                    tdc()
            except BaseException:
                pass
        return result

    def debug(self):
        for test in self._tests:
            if hasattr(test, "debug"):
                test.debug()
            else:
                test()

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

    testMethodPrefix = 'test'
    sortTestMethodsUsing = staticmethod(lambda a, b: (a > b) - (a < b))
    suiteClass = TestSuite
    testNamePatterns = None

    def loadTestsFromTestCase(self, testCaseClass):
        if not hasattr(testCaseClass, 'run'):
            return self.suiteClass()
        test_names = self.getTestCaseNames(testCaseClass)
        suite = self.suiteClass()
        for name in test_names:
            suite.addTest(testCaseClass(name))
        return suite

    def loadTestsFromModule(self, module, pattern=None):
        suite = self.suiteClass()
        for name in dir(module):
            obj = getattr(module, name)
            if (isinstance(obj, type) and issubclass(obj, TestCase) and
                    obj is not TestCase and hasattr(obj, 'run')):
                suite.addTest(self.loadTestsFromTestCase(obj))
        load_tests = getattr(module, 'load_tests', None)
        if load_tests is not None:
            return load_tests(self, suite, pattern)
        return suite

    def getTestCaseNames(self, testCaseClass):
        names = []
        for name in dir(testCaseClass):
            if name.startswith(self.testMethodPrefix):
                obj = getattr(testCaseClass, name)
                if callable(obj):
                    names.append(name)
        names.sort()
        return names

    def loadTestsFromName(self, name, module=None):
        """Load tests by name (dotted module.Class.method)."""
        import sys
        if type(name) is not str:
            if type(name) is type and issubclass(name, TestCase):
                return self.loadTestsFromTestCase(name)
            if callable(name):
                suite = self.suiteClass()
                suite.addTest(name())
                return suite
            return self.suiteClass()
        parts = name.split('.')
        if module is None:
            # Try to import the module
            mod_name = parts[0]
            __import__(mod_name)
            module = sys.modules[mod_name]
            parts = parts[1:]
        obj = module
        parent = None
        attr_name = None
        for part in parts:
            parent = obj
            attr_name = part
            obj = getattr(obj, part)
        if isinstance(obj, type) and issubclass(obj, TestCase):
            return self.loadTestsFromTestCase(obj)
        elif isinstance(obj, TestCase):
            suite = self.suiteClass()
            suite.addTest(obj)
            return suite
        elif (parent is not None and isinstance(parent, type) and
              issubclass(parent, TestCase) and attr_name is not None and
              callable(obj)):
            suite = self.suiteClass()
            suite.addTest(parent(attr_name))
            return suite
        elif hasattr(obj, "_tests") and hasattr(obj, "run"):
            return obj
        elif callable(obj):
            test = obj()
            if hasattr(test, "run"):
                suite = self.suiteClass()
                suite.addTest(test)
                return suite
            if hasattr(test, "_tests"):
                return test
            suite = self.suiteClass()
            return suite
        return self.suiteClass()

    def loadTestsFromNames(self, names, module=None):
        suites = [self.loadTestsFromName(name, module) for name in names]
        suite = self.suiteClass()
        for s in suites:
            suite.addTests(s)
        return suite

    def discover(self, start_dir, pattern='test*.py', top_level_dir=None):
        return self.suiteClass()


def makeSuite(testCaseClass, prefix='test'):
    """Compatibility wrapper for older unittest code paths."""
    loader = TestLoader()
    if prefix == 'test':
        return loader.loadTestsFromTestCase(testCaseClass)
    names = []
    for name in dir(testCaseClass):
        if name.startswith(prefix):
            obj = getattr(testCaseClass, name)
            if callable(obj):
                names.append(name)
    names.sort()
    suite = TestSuite()
    for name in names:
        suite.addTest(testCaseClass(name))
    return suite


class TextTestRunner:
    """A test runner that outputs results to the console."""

    def __init__(self, stream=None, descriptions=True, verbosity=1,
                 failfast=False, resultclass=None):
        import sys
        self.stream = stream or sys.stderr
        self.descriptions = descriptions
        self.verbosity = verbosity
        self.failfast = failfast
        self.resultclass = resultclass or TestResult

    def run(self, test):
        result = self.resultclass()
        result.failfast = self.failfast
        result.startTestRun()
        test.run(result)
        result.stopTestRun()
        if self.verbosity > 0:
            if result.failures or result.errors:
                for test_case, traceback_str in result.failures:
                    print("=" * 70, file=self.stream)
                    print("FAIL:", test_case, file=self.stream)
                    print("-" * 70, file=self.stream)
                    print(traceback_str, file=self.stream)
                for test_case, traceback_str in result.errors:
                    print("=" * 70, file=self.stream)
                    print("ERROR:", test_case, file=self.stream)
                    print("-" * 70, file=self.stream)
                    print(traceback_str, file=self.stream)
            if result.wasSuccessful():
                print("OK (%d tests)" % result.testsRun, file=self.stream)
            else:
                print("FAILED (failures=%d, errors=%d)" % (
                    len(result.failures), len(result.errors)), file=self.stream)
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
        except BaseException:
            return
        raise AssertionError("test unexpectedly succeeded")
    wrapper.__name__ = func.__name__
    return wrapper


class FunctionTestCase(TestCase):
    def __init__(self, testFunc, setUp=None, tearDown=None, description=None):
        super().__init__('runTest')
        self._testFunc = testFunc
        self._setUpFunc = setUp
        self._tearDownFunc = tearDown
        self._description = description

    def setUp(self):
        if self._setUpFunc is not None:
            self._setUpFunc()

    def tearDown(self):
        if self._tearDownFunc is not None:
            self._tearDownFunc()

    def runTest(self):
        self._testFunc()

    def id(self):
        return getattr(self._testFunc, "__name__", repr(self._testFunc))

    def shortDescription(self):
        return self._description


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
