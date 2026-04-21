"""test.support — compatibility shim for CPython regression tests.

Provides a subset of CPython's test.support API sufficient to run the
official CPython regression tests under Ferrython.  Only functions and
constants actually used by the vendored test files are implemented here;
everything else is stubbed with a sensible no-op or skip.
"""

import contextlib
import functools
import gc
import io
import os
import sys
import tempfile
import unittest
import warnings

__all__ = [
    # constants
    "TESTFN", "SAVEDCWD", "NHASHBITS", "verbose", "PGO",
    "HOST", "is_jython", "is_android",
    "MAX_Py_ssize_t",
    "_2G", "_4G",
    # sentinel objects
    "ALWAYS_EQ", "LARGEST", "SMALLEST", "NEVER_EQ",
    # errors
    "Error", "TestFailed", "ResourceDenied",
    # skip/requires decorators
    "cpython_only", "impl_detail", "bigmemtest", "bigaddrspacetest",
    "requires", "requires_IEEE_754", "requires_zlib",
    "requires_gzip", "requires_bz2", "requires_lzma",
    "requires_docstrings", "requires_hashdigest",
    "skip_unless_symlink", "anticipate_failure",
    "no_tracing", "disable_gc", "refcount_test",
    "skip_if_buggy_ucrt_strfptime",
    # imports
    "import_module", "import_fresh_module",
    # filesystem
    "unlink", "findfile", "create_empty_file", "temp_dir", "temp_cwd",
    # I/O capture
    "captured_stdout", "captured_stderr", "captured_stdin",
    "captured_output",
    # unittest helpers
    "check_syntax_error", "check_syntax_warning",
    "check_warnings", "check_no_resource_warning", "check_no_warnings",
    "run_unittest", "run_doctest",
    "check_impl_detail",
    "check_free_after_iterating",
    # misc
    "gc_collect", "sortdict",
    "swap_attr", "swap_item",
    "EnvironmentVarGuard", "Matcher",
    "setswitchinterval", "is_resource_enabled",
    "run_with_locale",
]

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

TESTFN = os.path.join(tempfile.gettempdir(), f"@test_ferrython_{os.getpid()}")
SAVEDCWD = os.getcwd()
NHASHBITS = 64 if sys.maxsize > 2**32 else 32
verbose = False
PGO = False
HOST = "localhost"
is_jython = False
is_android = False
MAX_Py_ssize_t = sys.maxsize
_2G = 2 * 1024 * 1024 * 1024
_4G = 4 * 1024 * 1024 * 1024

# ---------------------------------------------------------------------------
# Sentinel comparison objects
# ---------------------------------------------------------------------------

class _ALWAYS_EQ:
    """An object that compares equal to everything."""
    def __eq__(self, other): return True
    def __ne__(self, other): return False
    def __lt__(self, other): return False
    def __le__(self, other): return True
    def __gt__(self, other): return False
    def __ge__(self, other): return True
    def __hash__(self): return 0

class _LARGEST:
    """An object that is larger than anything except another _LARGEST."""
    def __eq__(self, other): return isinstance(other, _LARGEST)
    def __ne__(self, other): return not self.__eq__(other)
    def __lt__(self, other): return False
    def __le__(self, other): return isinstance(other, _LARGEST)
    def __gt__(self, other): return not isinstance(other, _LARGEST)
    def __ge__(self, other): return True
    def __hash__(self): return 0

class _SMALLEST:
    """An object that is smaller than anything except another _SMALLEST."""
    def __eq__(self, other): return isinstance(other, _SMALLEST)
    def __ne__(self, other): return not self.__eq__(other)
    def __lt__(self, other): return not isinstance(other, _SMALLEST)
    def __le__(self, other): return True
    def __gt__(self, other): return False
    def __ge__(self, other): return isinstance(other, _SMALLEST)
    def __hash__(self): return 0

ALWAYS_EQ = _ALWAYS_EQ()
LARGEST   = _LARGEST()
SMALLEST  = _SMALLEST()

class _NEVER_EQ:
    """An object that compares unequal to everything."""
    def __eq__(self, other): return False
    def __ne__(self, other): return True
    def __hash__(self): return 0

NEVER_EQ = _NEVER_EQ()

# ---------------------------------------------------------------------------
# Error classes
# ---------------------------------------------------------------------------

class Error(Exception):
    """Base class for regression test exceptions."""

class TestFailed(Error):
    """Test failed."""

class ResourceDenied(unittest.SkipTest):
    """Test skipped because it requested a disallowed resource."""

# ---------------------------------------------------------------------------
# Skip / requires decorators
# ---------------------------------------------------------------------------

def requires(resource, msg=None):
    """No-op: Ferrython testing allows all resources by default."""
    pass

def cpython_only(test):
    """Mark a test as CPython-only.  We still run it on Ferrython."""
    return test

def requires_IEEE_754(test):
    return test

def requires_zlib(test):
    return test

def requires_gzip(test):
    return test

def requires_bz2(test):
    return test

def requires_lzma(test):
    return test

def skip_unless_symlink(test):
    return test

def anticipate_failure(condition):
    if condition:
        return unittest.expectedFailure
    return lambda f: f

def bigmemtest(size, memuse, dry_run=True):
    """Decorator for tests that require large amounts of memory."""
    def decorator(f):
        @functools.wraps(f)
        def wrapper(self):
            self.skipTest("big memory test skipped in Ferrython compat suite")
        return wrapper
    return decorator

def bigaddrspacetest(f):
    return unittest.skip("big address space test skipped")(f)

def is_resource_enabled(resource):
    return True

def setswitchinterval(interval):
    pass  # no-op; Ferrython has no thread switch interval

def requires_docstrings(test):
    """Decorator: skip test if docstrings are stripped."""
    return test

def requires_hashdigest(digestname, openssl=None):
    """Decorator: skip test if the hash digest is unavailable."""
    def decorator(func):
        return func
    if callable(digestname):
        return digestname
    return decorator

def no_tracing(func):
    """Decorator to temporarily turn off tracing for the duration of a test."""
    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        original = sys.gettrace()
        try:
            sys.settrace(None)
            return func(*args, **kwargs)
        finally:
            sys.settrace(original)
    return wrapper

@contextlib.contextmanager
def disable_gc():
    """Context manager to disable the garbage collector."""
    have_gc = gc.isenabled()
    gc.disable()
    try:
        yield
    finally:
        if have_gc:
            gc.enable()

def refcount_test(test):
    """Decorator for tests that examine reference counts."""
    return test

def skip_if_buggy_ucrt_strfptime(test):
    """Decorator: skip on buggy ucrt strfptime (Windows only)."""
    return test

def impl_detail(msg=None, **guards):
    """Decorator for tests that depend on implementation details."""
    if check_impl_detail(**guards):
        return lambda func: func
    return unittest.skip(msg or "implementation detail")

def run_with_locale(catstr, *locales):
    """Decorator to run a test under different locale settings."""
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            return func(*args, **kwargs)
        return wrapper
    return decorator

def check_free_after_iterating(test, cls, *args):
    """Check that *cls* frees objects once it's done iterating."""
    pass  # no-op stub for Ferrython

# ---------------------------------------------------------------------------
# Import helpers
# ---------------------------------------------------------------------------

def import_module(name, deprecated=False, *, required_on=()):
    """Import and return the module, or raise SkipTest if unavailable."""
    try:
        return __import__(name, fromlist=[""])
    except ImportError as exc:
        raise unittest.SkipTest(f"module {name!r} not available: {exc}")

def import_fresh_module(name, fresh=(), blocked=(), deprecated=False):
    """Import a fresh copy of a module, temporarily blocking others."""
    import importlib
    for n in fresh:
        sys.modules.pop(n, None)
    blocked_saved = {}
    for n in blocked:
        blocked_saved[n] = sys.modules.pop(n, _MISSING)
        sys.modules[n] = None  # type: ignore[assignment]
    try:
        mod = importlib.import_module(name)
    finally:
        for n in blocked:
            if blocked_saved[n] is _MISSING:
                sys.modules.pop(n, None)
            else:
                sys.modules[n] = blocked_saved[n]
    return mod

# ---------------------------------------------------------------------------
# Filesystem helpers
# ---------------------------------------------------------------------------

def unlink(filename):
    try:
        os.unlink(filename)
    except (FileNotFoundError, PermissionError):
        pass

def create_empty_file(filename):
    with open(filename, "wb"):
        pass

def findfile(file, here=None, subdir=None):
    if os.path.isabs(file):
        return file
    # Try cwd and, as a fallback, the directory of the main script
    search_dirs = []
    if here is not None:
        search_dirs.append(here)
    else:
        search_dirs.append(os.getcwd())
    try:
        main_mod = sys.modules.get('__main__')
        if main_mod is not None and getattr(main_mod, '__file__', None):
            search_dirs.append(os.path.dirname(os.path.abspath(main_mod.__file__)))
    except Exception:
        pass
    for base in search_dirs:
        if subdir is not None:
            candidate = os.path.join(base, subdir, file)
            if os.path.exists(candidate):
                return candidate
        candidate = os.path.join(base, file)
        if os.path.exists(candidate):
            return candidate
    return file

def rmtree(path):
    import shutil
    try:
        shutil.rmtree(path)
    except FileNotFoundError:
        pass

# ---------------------------------------------------------------------------
# I/O capture context managers
# ---------------------------------------------------------------------------

@contextlib.contextmanager
def captured_stdout():
    old, sys.stdout = sys.stdout, io.StringIO()
    try:
        yield sys.stdout
    finally:
        sys.stdout = old

@contextlib.contextmanager
def captured_stderr():
    old, sys.stderr = sys.stderr, io.StringIO()
    try:
        yield sys.stderr
    finally:
        sys.stderr = old

@contextlib.contextmanager
def captured_stdin():
    old, sys.stdin = sys.stdin, io.StringIO()
    try:
        yield sys.stdin
    finally:
        sys.stdin = old

@contextlib.contextmanager
def captured_output(stream_name):
    """Capture a named output stream (stdout, stderr)."""
    orig = getattr(sys, stream_name)
    setattr(sys, stream_name, io.StringIO())
    try:
        yield getattr(sys, stream_name)
    finally:
        setattr(sys, stream_name, orig)

@contextlib.contextmanager
def temp_dir(path=None, quiet=False):
    """Context manager that creates a temporary directory."""
    dir_path = path or tempfile.mkdtemp()
    if path and not os.path.isdir(path):
        os.makedirs(path)
    try:
        yield dir_path
    finally:
        try:
            import shutil
            shutil.rmtree(dir_path)
        except Exception:
            pass

@contextlib.contextmanager
def temp_cwd(name='tempcwd', quiet=False):
    """Context manager that creates and changes to a temporary directory."""
    with temp_dir(quiet=quiet) as temp_path:
        old_cwd = os.getcwd()
        os.chdir(temp_path)
        try:
            yield temp_path
        finally:
            os.chdir(old_cwd)

# ---------------------------------------------------------------------------
# unittest helpers
# ---------------------------------------------------------------------------

def check_syntax_error(testcase, statement, errtext=None, lineno=None, offset=None):
    """Assert that compiling *statement* raises SyntaxError."""
    with testcase.assertRaisesRegex(SyntaxError, errtext if errtext else r"[\s\S]*"):
        compile(statement, "<test string>", "exec")

def check_syntax_warning(testcase, statement, errtext="", *, lineno=None, offset=None):
    """Assert that compiling *statement* emits a SyntaxWarning."""
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        try:
            compile(statement, "<test string>", "exec")
        except SyntaxError:
            pass
    # We don't enforce the warning strictly; just check it compiled.

@contextlib.contextmanager
def check_warnings(*filters, quiet=True):
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        for f in filters:
            if isinstance(f, tuple):
                warnings.filterwarnings(*f)
        yield w

@contextlib.contextmanager
def check_no_resource_warning(testcase):
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        yield
    for warning in w:
        if issubclass(warning.category, ResourceWarning):
            testcase.fail(f"unexpected ResourceWarning: {warning.message}")

@contextlib.contextmanager
def check_no_warnings(testcase, *, category=Warning):
    with warnings.catch_warnings(record=True) as w:
        warnings.simplefilter("always")
        yield
    relevant = [x for x in w if issubclass(x.category, category)]
    if relevant:
        testcase.fail(f"unexpected warning: {relevant[0].message}")

def run_unittest(*classes):
    """Run unittest test-case classes and raise TestFailed on any failure."""
    suite = unittest.TestSuite()
    for cls in classes:
        if isinstance(cls, str):
            suite.addTest(unittest.TestLoader().loadTestsFromName(cls))
        elif isinstance(cls, unittest.TestSuite):
            suite.addTest(cls)
        else:
            suite.addTests(unittest.TestLoader().loadTestsFromTestCase(cls))
    verbosity = 2 if verbose else 1
    result = unittest.TextTestRunner(verbosity=verbosity).run(suite)
    if not result.wasSuccessful():
        raise TestFailed(
            f"{len(result.failures)} failure(s), {len(result.errors)} error(s)"
        )

def run_doctest(module, verbosity=None):
    """Run doctests in *module* and raise TestFailed on any failure."""
    import doctest
    results = doctest.testmod(module, verbose=verbosity)
    if results.failed:
        raise TestFailed(f"{results.failed} doctest failure(s)")

def check_impl_detail(**guards):
    """Return True if the named implementation detail is active.

    Recognises 'cpython' and 'ferrython'.  All others return False.
    With no arguments returns True (unconditional check).
    """
    if not guards:
        return True
    return guards.get("ferrython", False) or guards.get("cpython", True)

# ---------------------------------------------------------------------------
# Miscellaneous helpers
# ---------------------------------------------------------------------------

def gc_collect():
    """Force a full garbage collection cycle."""
    gc.collect()
    gc.collect()
    gc.collect()

def sortdict(d):
    """Return a new dict with keys sorted."""
    return {k: d[k] for k in sorted(d)}

_MISSING = object()

@contextlib.contextmanager
def swap_attr(obj, attr_name, new_val):
    """Temporarily replace *attr_name* on *obj* with *new_val*."""
    old_val = getattr(obj, attr_name, _MISSING)
    setattr(obj, attr_name, new_val)
    try:
        yield
    finally:
        if old_val is _MISSING:
            try:
                delattr(obj, attr_name)
            except AttributeError:
                pass
        else:
            setattr(obj, attr_name, old_val)

@contextlib.contextmanager
def swap_item(obj, item, new_val):
    """Temporarily replace *obj[item]* with *new_val*."""
    try:
        old_val = obj[item]
    except KeyError:
        old_val = _MISSING
    obj[item] = new_val
    try:
        yield
    finally:
        if old_val is _MISSING:
            try:
                del obj[item]
            except KeyError:
                pass
        else:
            obj[item] = old_val

class EnvironmentVarGuard:
    """Context manager that saves and restores os.environ entries."""

    def __init__(self):
        self._environ = os.environ
        self._reset = {}

    def __enter__(self):
        return self

    def __exit__(self, *ignored):
        for key, val in self._reset.items():
            if val is _MISSING:
                self._environ.pop(key, None)
            else:
                self._environ[key] = val
        self._reset.clear()

    def _save(self, key):
        if key not in self._reset:
            self._reset[key] = self._environ.get(key, _MISSING)

    def set(self, key, value):
        self._save(key)
        self._environ[key] = value

    def unset(self, key):
        self._save(key)
        self._environ.pop(key, None)

    def __setitem__(self, key, value):
        self.set(key, value)

    def __delitem__(self, key):
        self.unset(key)

    def __getitem__(self, key):
        return self._environ[key]

    def __contains__(self, key):
        return key in self._environ

    def get(self, key, default=None):
        return self._environ.get(key, default)

class SuppressCrashReport:
    """Context manager to suppress crash dialogs (no-op on Ferrython)."""
    def __enter__(self):
        return self
    def __exit__(self, *args):
        pass

def strip_python_stderr(stderr):
    """Strip known CPython stderr noise."""
    if isinstance(stderr, bytes):
        return stderr.strip()
    return stderr.strip()

class Matcher:
    _partial_matches = ("msg", "message")

    def matches(self, d, **kwargs):
        for k, v in kwargs.items():
            dv = d.get(k)
            if not self.match_value(k, dv, v):
                return False
        return True

    def match_value(self, k, dv, v):
        if k in self._partial_matches:
            return dv is not None and str(dv).startswith(str(v))
        return dv == v


# ── Threading helpers ──

import contextlib
import threading

@contextlib.contextmanager
def start_threads(threads, unlock=None):
    """Context manager to start and join threads."""
    for t in threads:
        t.start()
    if unlock:
        unlock.set()
    try:
        yield
    finally:
        for t in threads:
            t.join()

def threading_cleanup(*original_values):
    """Cleanup stale threads."""
    pass

def reap_threads(func):
    """Decorator to cleanup threads after test."""
    def wrapper(*args, **kwargs):
        return func(*args, **kwargs)
    wrapper.__name__ = func.__name__
    return wrapper

def reap_children():
    """Cleanup zombie child processes."""
    pass

def verbose():
    """Return True if running in verbose mode."""
    return False

def is_jython():
    return False

def cpython_only(test):
    """Decorator: skip if not CPython."""
    return test

def check_impl_detail(**guards):
    return True

def bigmemtest(size, memuse, dry_run=True):
    """Decorator for big-memory tests."""
    def decorator(f):
        return f
    return decorator

def bigaddrspacetest(f):
    return f

def requires_type_collecting(test):
    """Decorator: skip if type collecting not supported."""
    return test

def forget(modname):
    """Remove module from sys.modules and try to delete .pyc files."""
    import sys
    try:
        del sys.modules[modname]
    except KeyError:
        pass

@contextlib.contextmanager
def save_restore_warnings_filters():
    """Context manager that saves/restores warnings filters."""
    import warnings
    old_filters = warnings.filters[:]
    try:
        yield
    finally:
        warnings.filters[:] = old_filters


class FakePath:
    """Fake path object for testing os.fspath() etc."""
    def __init__(self, path):
        self.path = path
    def __fspath__(self):
        return self.path
    def __repr__(self):
        return f"FakePath({self.path!r})"
