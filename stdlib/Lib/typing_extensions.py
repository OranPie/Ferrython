"""Minimal typing_extensions — re-exports from typing with extras."""

try:
    from typing import (
        Any, Union, Optional, List, Dict, Set, Tuple, FrozenSet,
        Type, Callable, Iterator, Generator, Sequence, Mapping,
        TypeVar, Generic, ClassVar, Final, get_type_hints,
        cast, overload, no_type_check, runtime_checkable, Protocol,
        TYPE_CHECKING,
    )
except ImportError:
    pass

try:
    from typing import Literal
except ImportError:
    Literal = type('Literal', (), {'__class_getitem__': classmethod(lambda cls, x: None)})

# Extra constructs not in typing 3.8
TypeAlias = type('TypeAlias', (), {})
ParamSpec = type('ParamSpec', (), {'__init__': lambda self, name: setattr(self, '__name__', name)})
TypeGuard = type('TypeGuard', (), {'__class_getitem__': classmethod(lambda cls, x: None)})
TypeVarTuple = type('TypeVarTuple', (), {'__init__': lambda self, name: setattr(self, '__name__', name)})
Unpack = type('Unpack', (), {'__class_getitem__': classmethod(lambda cls, x: None)})
Annotated = type('Annotated', (), {'__class_getitem__': classmethod(lambda cls, x: None)})

def runtime_checkable(cls):
    """Decorator — marks a Protocol as runtime-checkable (no-op stub)."""
    return cls
