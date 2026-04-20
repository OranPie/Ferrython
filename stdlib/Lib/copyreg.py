"""copyreg — Register pickle support functions.

This module provides the infrastructure for registering
reduce functions and constructors for pickling objects.
"""

__all__ = [
    "pickle", "constructor",
    "add_extension", "remove_extension", "clear_extension_cache",
]

dispatch_table = {}

_extension_registry = {}
_inverted_registry = {}
_extension_cache = {}


def pickle(ob_type, pickle_function, constructor_ob=None):
    """Register a reduce function for a type."""
    if not callable(pickle_function):
        raise TypeError("reduction functions must be callable")
    if constructor_ob is not None:
        if not callable(constructor_ob):
            raise TypeError("constructors must be callable")
    dispatch_table[ob_type] = pickle_function


def constructor(ob):
    """Register a constructor for use with reduce functions."""
    if not callable(ob):
        raise TypeError("constructors must be callable")


def _reconstructor(cls, base, state):
    """Helper to reconstruct an object."""
    if base is object:
        obj = object.__new__(cls)
    else:
        obj = base.__new__(cls, state)
    if base.__init__ != object.__init__:
        base.__init__(obj, state)
    return obj


def __newobj__(cls, *args):
    return cls.__new__(cls, *args)


def __newobj_ex__(cls, args, kwargs):
    return cls.__new__(cls, *args, **kwargs)


_HEAPTYPE = 1 << 9


def _slotnames(cls):
    """Return a list of slot names for a given class."""
    names = cls.__dict__.get("__slots__")
    if names is None:
        return []
    if isinstance(names, str):
        names = [names]
    result = []
    for name in names:
        if name == "__dict__" or name == "__weakref__":
            continue
        result.append(name)
    return result


def _reduce_ex(self, protocol=0):
    """Helper for __reduce_ex__ default implementation."""
    cls = type(self)
    return (_reconstructor, (cls, object, None))


def _new_type(cls, *args):
    return cls(*args)


def add_extension(module, name, code):
    """Register an extension code for (module, name)."""
    key = (module, name)
    if key in _extension_registry and _extension_registry[key] != code:
        raise ValueError(
            "key {} is already registered with code {}".format(
                key, _extension_registry[key]
            )
        )
    if code in _inverted_registry and _inverted_registry[code] != key:
        raise ValueError(
            "code {} is already in use for key {}".format(
                code, _inverted_registry[code]
            )
        )
    _extension_registry[key] = code
    _inverted_registry[code] = key


def remove_extension(module, name, code):
    """Unregister an extension code."""
    key = (module, name)
    if _extension_registry.get(key) != code or _inverted_registry.get(code) != key:
        raise ValueError(
            "key {} is not registered with code {}".format(key, code)
        )
    del _extension_registry[key]
    del _inverted_registry[code]
    _extension_cache.clear()


def clear_extension_cache():
    _extension_cache.clear()


# Register complex pickling
def pickle_complex(c):
    return complex, (c.real, c.imag)

try:
    pickle(complex, pickle_complex)
except Exception:
    pass
