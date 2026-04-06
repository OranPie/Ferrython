"""Utilities to compile possibly incomplete Python code.

This module provides a way to determine if a piece of Python source code
is a complete statement, useful for building interactive interpreters.
"""

__all__ = ['compile_command', 'Compile', 'CommandCompiler']

PyCF_DONT_IMPLY_DEDENT = 0x200


def _maybe_compile(compiler, source, filename, symbol):
    """Compile source code, distinguishing incomplete from erroneous code.

    Return a code object if complete. Return None if incomplete.
    Raise SyntaxError (or OverflowError/ValueError) if invalid.
    """
    # Try compiling as-is
    err = err1 = err2 = None
    code = code1 = None

    try:
        code = compiler(source, filename, symbol)
    except SyntaxError as e:
        err = e

    try:
        code1 = compiler(source + "\n", filename, symbol)
    except SyntaxError as e:
        err1 = e

    try:
        compiler(source + "\n\n", filename, symbol)
    except SyntaxError as e:
        err2 = e

    if code is not None:
        return code
    if err1 is None and err2 is None:
        return None  # Both extra-newline versions work -> incomplete

    # If adding \n changes the error, the code is incomplete
    if err1 is not None and err2 is not None:
        e1_msg = str(err1)
        e2_msg = str(err2)
        if e1_msg == e2_msg:
            raise err1
    if err is not None:
        raise err
    return None


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
