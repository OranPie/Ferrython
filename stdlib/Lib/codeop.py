"""Utilities to compile possibly incomplete Python code.

This module provides a way to determine if a piece of Python source code
is a complete statement, useful for building interactive interpreters.
"""

__all__ = ['compile_command', 'Compile', 'CommandCompiler']


def compile_command(source, filename="<input>", symbol="single"):
    """Compile a command and determine whether it is incomplete.

    Returns a code object if the command is complete and valid.
    Returns None if the command is incomplete.
    Raises SyntaxError if the command is a syntax error.
    """
    try:
        code = compile(source, filename, symbol)
        return code
    except SyntaxError:
        # Could be incomplete or truly invalid
        # Try adding a newline - if it still fails, it's a real error
        try:
            compile(source + "\n", filename, symbol)
            return None  # Incomplete
        except SyntaxError:
            raise  # Real syntax error


class Compile:
    """Instances of this class behave much like the built-in compile
    function, but with __future__ statement awareness."""

    def __init__(self):
        self.flags = 0

    def __call__(self, source, filename, symbol):
        return compile(source, filename, symbol)


class CommandCompiler:
    """Instances of this class have __call__ methods identical in
    signature to compile_command; the difference is that if the
    instance compiles program text containing a __future__ statement,
    the instance 'remembers' and compiles all subsequent program texts
    with the statement in force."""

    def __init__(self):
        self.compiler = Compile()

    def __call__(self, source, filename="<input>", symbol="single"):
        return compile_command(source, filename, symbol)
