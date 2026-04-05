"""Pure-Python traceback module.

Provides formatting helpers for exception tracebacks.
Since Ferrython may not expose real traceback objects, these
functions gracefully handle None / string inputs.
"""

import sys


class FrameSummary:
    def __init__(self, filename, lineno, name, line=None):
        self.filename = filename
        self.lineno = lineno
        self.name = name
        self.line = line

    def __repr__(self):
        return f"<FrameSummary file {self.filename}, line {self.lineno} in {self.name}>"


def format_tb(tb, limit=None):
    if tb is None:
        return []
    return ['  File "<unknown>", line 0, in <module>\n']


def print_tb(tb, limit=None, file=None):
    if file is None:
        file = sys.stderr
    for line in format_tb(tb, limit):
        file.write(line)


def extract_tb(tb, limit=None):
    if tb is None:
        return []
    return [FrameSummary('<unknown>', 0, '<module>')]


def format_exception(exc_type, exc_value, exc_tb):
    lines = []
    if exc_tb is not None:
        lines.append('Traceback (most recent call last):\n')
        lines.append('  File "<unknown>", line 0, in <module>\n')
    type_name = exc_type.__name__ if hasattr(exc_type, '__name__') else str(exc_type)
    lines.append(f'{type_name}: {exc_value}\n')
    return lines


def format_exc(limit=None, chain=True):
    ei = sys.exc_info() if hasattr(sys, 'exc_info') else (None, None, None)
    if ei[0] is None:
        return 'NoneType: None\n'
    return ''.join(format_exception(ei[0], ei[1], ei[2]))


def print_exc(limit=None, file=None, chain=True):
    if file is None:
        file = sys.stderr
    file.write(format_exc(limit, chain))


def format_stack(f=None, limit=None):
    return ['  File "<unknown>", line 0, in <module>\n']


def print_stack(f=None, limit=None, file=None):
    if file is None:
        file = sys.stderr
    for line in format_stack(f, limit):
        file.write(line)
