"""Proxy module for the vendored CPython weakref regression tests."""

import importlib.util
import os


def _workspace_root():
    here = os.path.abspath(__file__)
    for _ in range(5):
        here = os.path.dirname(here)
        candidate = os.path.join(here, "tests", "cpython", "test_weakref.py")
        if os.path.exists(candidate):
            return here
    return os.getcwd()


_path = os.path.join(_workspace_root(), "tests", "cpython", "test_weakref.py")
_spec = importlib.util.spec_from_file_location(__name__, _path)
_module = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_module)

globals().update(_module.__dict__)
