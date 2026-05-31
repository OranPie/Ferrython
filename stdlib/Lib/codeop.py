"""Utilities to compile possibly incomplete Python code.

This module provides a way to determine if a piece of Python source code
is a complete statement, useful for building interactive interpreters.
"""

__all__ = ['compile_command', 'Compile', 'CommandCompiler']

import warnings

PyCF_DONT_IMPLY_DEDENT = 0x200


def _is_comment_or_blank(source):
    for line in source.split("\n"):
        line = line.strip()
        if line and line[0] != '#':
            return False
    return True


def _has_unclosed_bracket(source):
    stack = []
    quote = None
    triple = False
    escape = False
    comment = False
    i = 0
    while i < len(source):
        ch = source[i]
        if comment:
            if ch == "\n":
                comment = False
            i += 1
            continue
        if quote is not None:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif triple and source.startswith(quote * 3, i):
                quote = None
                triple = False
                i += 2
            elif not triple and ch == quote:
                quote = None
            i += 1
            continue
        if ch == "#":
            comment = True
        elif ch in ("'", '"'):
            quote = ch
            triple = source.startswith(ch * 3, i)
            if triple:
                i += 2
        elif ch in "([{":
            stack.append(ch)
        elif ch in ")]}":
            if stack:
                stack.pop()
        i += 1
    return bool(stack)


def _has_unclosed_triple_quote(source):
    quote = None
    escape = False
    i = 0
    while i < len(source):
        ch = source[i]
        if quote is not None:
            if escape:
                escape = False
            elif ch == "\\":
                escape = True
            elif source.startswith(quote * 3, i):
                quote = None
                i += 2
            i += 1
            continue
        if ch in ("'", '"') and source.startswith(ch * 3, i):
            quote = ch
            i += 3
            continue
        i += 1
    return quote is not None


def _last_significant_line(source):
    for line in reversed(source.splitlines()):
        if line.strip() and not line.lstrip().startswith("#"):
            return line.rstrip()
    return ""


def _starts_compound_suite(source):
    for line in source.splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            return stripped.endswith(":")
    return False


def _looks_incomplete_source(source, symbol, exc):
    if symbol == "eval" and not source.strip():
        return True
    msg = str(exc)
    stripped = source.rstrip()
    last = _last_significant_line(source)
    if symbol == "eval" and "=" in source and "==" not in source:
        return False
    if _has_unclosed_bracket(source):
        return True
    if source.endswith("\\") and not source.endswith("\\\n"):
        return True
    if "unterminated triple-quoted string literal" in msg:
        return True
    if "unterminated string literal" in msg and _has_unclosed_triple_quote(source):
        return True
    if "unterminated string literal" in msg and source.endswith("\\"):
        return True
    if "unexpected EOF" in msg:
        return True
    if "expression expected" in msg and source.endswith("\\\n"):
        return False
    if "expected an indented block" in msg and last.endswith(":"):
        return True
    return False


def _forces_invalid_continuation(source):
    return source.endswith("\\\n")


def _single_needs_dedent_marker(source, symbol):
    if symbol != "single":
        return False
    if source.endswith("\n"):
        return False
    return _starts_compound_suite(source)


def _maybe_compile(compiler, source, filename, symbol):
    """Compile source code, distinguishing incomplete from erroneous code.

    Return a code object if complete. Return None if incomplete.
    Raise SyntaxError (or OverflowError/ValueError) if invalid.
    """
    if _is_comment_or_blank(source):
        if symbol != "eval":
            source = "pass"
        else:
            return None

    with warnings.catch_warnings():
        warnings.simplefilter("ignore", (SyntaxWarning, DeprecationWarning))
        try:
            compiler(source, filename, symbol)
        except SyntaxError as err:
            if _forces_invalid_continuation(source):
                raise err
            if _looks_incomplete_source(source, symbol, err):
                return None
            try:
                compiler(source + "\n", filename, symbol)
                return None
            except SyntaxError as err1:
                if _looks_incomplete_source(source + "\n", symbol, err1):
                    return None
                raise err

    if _single_needs_dedent_marker(source, symbol):
        return None
    return compiler(source, filename, symbol)


def compile_command(source, filename="<input>", symbol="single"):
    r"""Compile a command and determine whether it is incomplete.

    Arguments:

    source   -- the source string; may contain \n characters
    filename -- optional filename from which source was read; default
                "<input>"
    symbol   -- optional grammar start symbol; "single" (default), "eval"
                or "exec"

    Return value / exceptions raised:

    - Return a code object if the command is complete and valid
    - Return None if the command is incomplete
    - Raise SyntaxError, ValueError, or OverflowError if the command is a
      syntax error (OverflowError and ValueError can be produced by
      malformed literals).
    """
    if symbol == "single" and source in ("", "\n"):
        return compile("pass", filename, symbol, PyCF_DONT_IMPLY_DEDENT)
    return _maybe_compile(compile, source, filename, symbol)


class Compile:
    """Instances of this class behave much like the built-in compile
    function, but if the instance compiles program text containing a
    __future__ statement, the instance 'remembers' and compiles all
    subsequent program texts with the statement in force."""

    def __init__(self):
        self.flags = PyCF_DONT_IMPLY_DEDENT
        self.compiler_flags = 0

    def __call__(self, source, filename, symbol, **kwargs):
        flags = self.flags | self.compiler_flags
        codeob = compile(source, filename, symbol)
        # In CPython, update flags based on __future__ statements in the code.
        # We don't yet have co_flags, so skip for now.
        return codeob


class CommandCompiler:
    """Instances of this class have __call__ methods identical in
    signature to compile_command; the difference is that if the
    instance compiles program text containing a __future__ statement,
    the instance 'remembers' and compiles all subsequent program texts
    with the statement in force."""

    def __init__(self):
        self.compiler = Compile()

    def __call__(self, source, filename="<input>", symbol="single"):
        return _maybe_compile(self.compiler, source, filename, symbol)
