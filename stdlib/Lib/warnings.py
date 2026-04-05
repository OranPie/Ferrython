"""Pure-Python warnings module."""

import sys

# ── Warning category hierarchy ──

class Warning(Exception):
    pass

class UserWarning(Warning):
    pass

class DeprecationWarning(Warning):
    pass

class RuntimeWarning(Warning):
    pass

class SyntaxWarning(Warning):
    pass

class FutureWarning(Warning):
    pass

class PendingDeprecationWarning(Warning):
    pass

class ImportWarning(Warning):
    pass

class UnicodeWarning(Warning):
    pass

class BytesWarning(Warning):
    pass

class ResourceWarning(Warning):
    pass

# ── Filter list ──

_filters = []

# ── Core functions ──

def formatwarning(message, category, filename, lineno, line=None):
    cat_name = category.__name__ if hasattr(category, '__name__') else str(category)
    s = f"{filename}:{lineno}: {cat_name}: {message}\n"
    if line is not None:
        s += f"  {line.strip()}\n"
    return s

def showwarning(message, category, filename, lineno, file=None, line=None):
    if file is None:
        file = sys.stderr
    text = formatwarning(message, category, filename, lineno, line)
    try:
        file.write(text)
    except Exception:
        pass

def warn(message, category=None, stacklevel=1):
    if category is None:
        category = UserWarning
    cat_name = category.__name__ if hasattr(category, '__name__') else str(category)
    print(f"<stdin>:1: {cat_name}: {message}", file=sys.stderr)

def filterwarnings(action, message='', category=None, module='', lineno=0, append=False):
    if category is None:
        category = Warning
    entry = (action, message, category, module, lineno)
    if append:
        _filters.append(entry)
    else:
        _filters.insert(0, entry)

def simplefilter(action, category=None, append=False):
    if category is None:
        category = Warning
    filterwarnings(action, category=category, append=append)

def resetwarnings():
    _filters.clear()

# ── catch_warnings context manager ──

class catch_warnings:
    def __init__(self, record=False):
        self._record = record
        self._saved_filters = None
        self._log = None

    def __enter__(self):
        self._saved_filters = _filters[:]
        if self._record:
            self._log = []
            return self._log
        return None

    def __exit__(self, *exc_info):
        _filters.clear()
        _filters.extend(self._saved_filters)
        return False
