"""importlib — The implementation of import."""

import sys as _sys

def import_module(name, package=None):
    """Import a module by name."""
    if name.startswith('.'):
        if package is None:
            raise TypeError("relative import requires 'package' argument")
        level = 0
        for ch in name:
            if ch == '.':
                level += 1
            else:
                break
        name = name[level:]
        if name:
            name = package + '.' + name
        else:
            name = package
    __import__(name)
    return _sys.modules[name]

def reload(module):
    """Reload a previously imported module."""
    name = module.__name__ if hasattr(module, '__name__') else str(module)
    if name in _sys.modules:
        del _sys.modules[name]
    __import__(name)
    return _sys.modules[name]

def invalidate_caches():
    """Call invalidate_caches() on all finders in sys.meta_path."""
    pass
