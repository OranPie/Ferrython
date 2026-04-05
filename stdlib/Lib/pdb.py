"""Debugger stubs for Ferrython."""

__all__ = ['set_trace', 'run', 'post_mortem', 'pm']


def set_trace():
    """Set a breakpoint (stub — not available in Ferrython)."""
    print("*** Breakpoint (pdb not available in Ferrython)")


def run(statement, globals=None, locals=None):
    """Execute a statement under debugger control (stub)."""
    print("*** pdb.run() is not available in Ferrython")


def post_mortem(traceback=None):
    """Enter post-mortem debugging (stub)."""
    print("*** pdb.post_mortem() is not available in Ferrython")


def pm():
    """Enter post-mortem debugging of the last traceback (stub)."""
    print("*** pdb.pm() is not available in Ferrython")


class Pdb:
    """Stub Pdb class for Ferrython."""

    def __init__(self):
        pass

    def set_trace(self):
        set_trace()

    def run(self, statement, globals=None, locals=None):
        run(statement, globals, locals)
