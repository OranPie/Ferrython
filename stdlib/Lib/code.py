"""Utilities needed to emulate Python's interactive interpreter.

This module provides classes and functions for building interactive
console-like applications.
"""

__all__ = ['InteractiveInterpreter', 'InteractiveConsole',
           'interact', 'compile_command']


def compile_command(source, filename="<input>", symbol="single"):
    """Compile a command and determine whether it is incomplete.

    Returns a code object if the source is complete and valid.
    Returns None if the source might be incomplete.
    Raises SyntaxError if the source is definitely invalid.
    """
    try:
        code = compile(source, filename, symbol)
        return code
    except SyntaxError:
        try:
            compile(source + "\n", filename, symbol)
            return None
        except SyntaxError:
            raise


class InteractiveInterpreter:
    """Base class for InteractiveConsole.

    An interactive interpreter that provides a simple interface for
    running code interactively.
    """

    def __init__(self, locals=None):
        if locals is None:
            locals = {"__name__": "__console__", "__doc__": None}
        self.locals = locals

    def runsource(self, source, filename="<input>", symbol="single"):
        """Execute source code in the interpreter.

        Returns True if more input is needed (incomplete statement),
        False otherwise.
        """
        try:
            code = compile_command(source, filename, symbol)
        except (OverflowError, SyntaxError, ValueError):
            self.showsyntaxerror(filename)
            return False

        if code is None:
            return True

        self.runcode(code)
        return False

    def runcode(self, code):
        """Execute a code object."""
        try:
            exec(code, self.locals)
        except SystemExit:
            raise
        except Exception as e:
            self.showtraceback()

    def showsyntaxerror(self, filename=None):
        """Display a syntax error."""
        print("SyntaxError")

    def showtraceback(self):
        """Display the exception that just occurred."""
        import sys
        try:
            ei = sys.exc_info()
            if ei[0] is not None:
                print(str(ei[0].__name__) + ": " + str(ei[1]))
        except Exception:
            print("Error displaying traceback")

    def write(self, data):
        """Write a string to the standard error stream."""
        print(data)


class InteractiveConsole(InteractiveInterpreter):
    """Closely emulate the behavior of the interactive Python interpreter.

    Adds readline-like line editing and history capabilities.
    """

    def __init__(self, locals=None, filename="<console>"):
        super().__init__(locals)
        self.filename = filename
        self.resetbuffer()

    def resetbuffer(self):
        """Reset the input buffer."""
        self.buffer = []

    def interact(self, banner=None, exitmsg=None):
        """Emulate the interactive Python console.

        The optional banner argument specifies the banner to print
        before the first interaction.
        """
        if banner is not None:
            print(banner)
        more = False
        while True:
            try:
                if more:
                    prompt = '... '
                else:
                    prompt = '>>> '
                line = input(prompt)
                more = self.push(line)
            except EOFError:
                print()
                break
            except KeyboardInterrupt:
                print("\nKeyboardInterrupt")
                self.resetbuffer()
                more = False
        if exitmsg is not None:
            print(exitmsg)

    def push(self, line):
        """Push a line to the interpreter.

        Returns True if more input is expected, False if the
        statement is complete or has an error.
        """
        self.buffer.append(line)
        source = "\n".join(self.buffer)
        more = self.runsource(source, self.filename)
        if not more:
            self.resetbuffer()
        return more

    def raw_input(self, prompt=""):
        """Write a prompt and read a line."""
        return input(prompt)


def interact(banner=None, readfunc=None, local=None, exitmsg=None):
    """Closely emulate the interactive Python interpreter.

    This is a backwards compatible interface to InteractiveConsole.
    """
    console = InteractiveConsole(local)
    console.interact(banner, exitmsg)
