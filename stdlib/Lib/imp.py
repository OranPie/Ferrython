"""Deprecated imp module — minimal compatibility shim."""

import importlib
import os

PY_SOURCE = 1
PY_COMPILED = 2
C_EXTENSION = 3
PKG_DIRECTORY = 5
C_BUILTIN = 6
PY_FROZEN = 7

def reload(module):
    return importlib.reload(module)

def find_module(name, path=None):
    """Find a module, returning (file, pathname, description)."""
    if path is None:
        import sys
        path = sys.path
    for d in path:
        fpath = os.path.join(d, name + '.py')
        if os.path.exists(fpath):
            return (open(fpath, 'r'), fpath, ('.py', 'r', PY_SOURCE))
        pkg_path = os.path.join(d, name, '__init__.py')
        if os.path.exists(pkg_path):
            return (None, os.path.join(d, name), ('', '', PKG_DIRECTORY))
    raise ImportError(f"No module named '{name}'")

def load_module(name, file, pathname, description):
    """Load a module given info from find_module."""
    import importlib.util
    spec = importlib.util.spec_from_file_location(name, pathname)
    if spec is None:
        raise ImportError(f"Cannot load module '{name}'")
    module = importlib.util.module_from_spec(spec)
    import sys
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module

def is_builtin(name):
    return False

def is_frozen(name):
    return False

def get_suffixes():
    return [('.py', 'r', PY_SOURCE)]

def new_module(name):
    import types
    return types.ModuleType(name)
