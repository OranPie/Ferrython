"""Regression tests for the Python standard library (Ferrython).

This package mirrors the CPython `test` package and provides the
`test.support` compatibility shim needed to run official CPython
regression tests under Ferrython.
"""


def __getattr__(name):
    """Lazy attribute lookup for CPython test modules.

    CPython test files often do ``from test import support, test_xxx``
    where test_xxx is the module itself (already imported as __main__).
    Return that module from sys.modules if present so the import works.
    """
    import sys
    if name in sys.modules:
        return sys.modules[name]
    if "__main__" in sys.modules:
        main = sys.modules["__main__"]
        if getattr(main, "__name__", None) == "__main__" and getattr(main, "__file__", "").rsplit("/", 1)[-1].rsplit(".", 1)[0] == name:
            return main
    raise AttributeError(f"module 'test' has no attribute {name!r}")
