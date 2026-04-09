"""Backport of new typing features for older Python versions.

Re-exports from typing with additional constructs from Python 3.9-3.12+.
"""

import typing
import sys

__all__ = [
    # Re-exports from typing
    'Any', 'Union', 'Optional', 'List', 'Dict', 'Set', 'Tuple',
    'FrozenSet', 'Type', 'Callable', 'Iterator', 'Generator',
    'Sequence', 'Mapping', 'MutableMapping', 'Iterable',
    'TypeVar', 'Generic', 'ClassVar', 'Final', 'Literal',
    'Protocol', 'get_type_hints', 'cast', 'overload',
    'no_type_check', 'runtime_checkable', 'TYPE_CHECKING',
    # Extensions
    'TypeAlias', 'ParamSpec', 'ParamSpecArgs', 'ParamSpecKwargs',
    'TypeGuard', 'TypeVarTuple', 'Unpack', 'Annotated',
    'Self', 'Never', 'LiteralString', 'TypedDict',
    'Required', 'NotRequired', 'assert_type', 'reveal_type',
    'dataclass_transform', 'override', 'get_overloads',
    'clear_overloads', 'NamedTuple', 'is_typeddict',
    'get_original_bases', 'get_args', 'get_origin',
    'Concatenate', 'ReadOnly', 'Buffer', 'deprecated',
    'Doc', 'NoDefault', 'TypeIs', 'Sentinel',
]

# Re-export everything from typing
for _name in [
    'Any', 'Union', 'Optional', 'List', 'Dict', 'Set', 'Tuple',
    'FrozenSet', 'Type', 'Callable', 'Iterator', 'Generator',
    'Sequence', 'Mapping', 'MutableMapping', 'Iterable',
    'TypeVar', 'Generic', 'ClassVar', 'Final',
    'get_type_hints', 'cast', 'overload', 'no_type_check',
    'TYPE_CHECKING', 'NamedTuple',
]:
    try:
        globals()[_name] = getattr(typing, _name)
    except AttributeError:
        pass

try:
    from typing import Literal
except (ImportError, AttributeError):
    class _LiteralGenericAlias:
        def __repr__(self): return 'typing.Literal'
        def __class_getitem__(cls, params): return cls
    Literal = _LiteralGenericAlias

try:
    from typing import Protocol
except (ImportError, AttributeError):
    class Protocol:
        pass

try:
    from typing import runtime_checkable
except (ImportError, AttributeError):
    def runtime_checkable(cls):
        return cls

try:
    from typing import get_args, get_origin
except (ImportError, AttributeError):
    def get_args(tp): return getattr(tp, '__args__', ())
    def get_origin(tp): return getattr(tp, '__origin__', None)


# ── Python 3.10+ types ──

class _SpecialForm:
    """Base for special typing forms."""
    def __init__(self, name, doc=''):
        self._name = name
        self.__doc__ = doc
    def __repr__(self):
        return f'typing_extensions.{self._name}'
    def __class_getitem__(cls, params):
        return cls
    def __getitem__(self, params):
        return self


TypeAlias = _SpecialForm('TypeAlias', 'Special form for marking type aliases')


class ParamSpec:
    """Parameter specification variable."""
    def __init__(self, name, *, bound=None, covariant=False, contravariant=False):
        self.__name__ = name
        self.__bound__ = bound
        self.__covariant__ = covariant
        self.__contravariant__ = contravariant
        self.args = ParamSpecArgs(self)
        self.kwargs = ParamSpecKwargs(self)

    def __repr__(self):
        return f'~{self.__name__}'


class ParamSpecArgs:
    """The args for a ParamSpec object."""
    def __init__(self, origin):
        self.__origin__ = origin
    def __repr__(self):
        return f'{self.__origin__.__name__}.args'


class ParamSpecKwargs:
    """The kwargs for a ParamSpec object."""
    def __init__(self, origin):
        self.__origin__ = origin
    def __repr__(self):
        return f'{self.__origin__.__name__}.kwargs'


class TypeVarTuple:
    """Type variable tuple (PEP 646)."""
    def __init__(self, name):
        self.__name__ = name
    def __repr__(self):
        return f'*{self.__name__}'
    def __iter__(self):
        yield Unpack[self]


TypeGuard = _SpecialForm('TypeGuard', 'Special form for type narrowing')
Concatenate = _SpecialForm('Concatenate', 'Used with ParamSpec for callable concatenation')
Unpack = _SpecialForm('Unpack', 'Unpack for TypeVarTuple')
Annotated = _SpecialForm('Annotated', 'Add context-specific metadata to a type')


# ── Python 3.11+ types ──

Self = _SpecialForm('Self', 'Use to annotate methods that return self')
Never = _SpecialForm('Never', 'The bottom type, a type that has no values')
LiteralString = _SpecialForm('LiteralString', 'A type that represents literal strings')
Required = _SpecialForm('Required', 'Mark a TypedDict key as required')
NotRequired = _SpecialForm('NotRequired', 'Mark a TypedDict key as not required')
ReadOnly = _SpecialForm('ReadOnly', 'Mark a TypedDict key as read-only')
Buffer = _SpecialForm('Buffer', 'Buffer protocol support')


# ── TypedDict ──

def TypedDict(typename, fields=None, *, total=True, **kwargs):
    """Create a typed dictionary class."""
    if fields is None:
        fields = kwargs

    ns = {
        '__annotations__': dict(fields),
        '__total__': total,
        '__required_keys__': frozenset(fields.keys()) if total else frozenset(),
        '__optional_keys__': frozenset() if total else frozenset(fields.keys()),
        '__name__': typename,
    }
    return type(typename, (dict,), ns)


def is_typeddict(tp):
    """Check if a type is a TypedDict."""
    return (isinstance(tp, type) and issubclass(tp, dict)
            and hasattr(tp, '__annotations__') and hasattr(tp, '__total__'))


# ── Utility functions ──

def assert_type(val, typ):
    """Assert that the value has the given type (no-op at runtime)."""
    return val


def reveal_type(obj):
    """Reveal the inferred type of an expression (prints at runtime)."""
    print(f"Runtime type is {type(obj).__name__!r}")
    return obj


def dataclass_transform(
    *,
    eq_default=True,
    order_default=False,
    kw_only_default=False,
    frozen_default=False,
    field_specifiers=(),
    **kwargs,
):
    """Decorator for marking a class/function/descriptor as a dataclass transform."""
    def decorator(cls_or_fn):
        cls_or_fn.__dataclass_transform__ = {
            'eq_default': eq_default,
            'order_default': order_default,
            'kw_only_default': kw_only_default,
            'frozen_default': frozen_default,
            'field_specifiers': field_specifiers,
            'kwargs': kwargs,
        }
        return cls_or_fn
    return decorator


def override(method):
    """Mark a method as overriding a parent method (PEP 698)."""
    method.__override__ = True
    return method


_overload_registry = {}

def get_overloads(func):
    """Return all defined overloads for func."""
    return _overload_registry.get(getattr(func, '__qualname__', id(func)), [])


def clear_overloads():
    """Clear all overloads in the registry."""
    _overload_registry.clear()


def get_original_bases(cls):
    """Get the original bases of a class as specified."""
    return getattr(cls, '__orig_bases__', cls.__bases__)


# ── deprecated decorator (PEP 702) ──

import warnings

class deprecated:
    """Indicate that a class, function or overload is deprecated.

    Usage::

        @deprecated("Use B instead")
        class A:
            pass

    """
    def __init__(self, message, /, *, category=DeprecationWarning, stacklevel=1):
        if not isinstance(message, str):
            raise TypeError(
                f"Expected an object of type str for 'message', not "
                f"{type(message).__name__!r}"
            )
        self.message = message
        self.category = category
        self.stacklevel = stacklevel

    def __call__(self, arg):
        msg = self.message
        category = self.category
        stacklevel = self.stacklevel
        if category is None:
            arg.__deprecated__ = msg
            return arg
        elif isinstance(arg, type):
            original_new = arg.__new__
            def __new__(cls, *args, **kwargs):
                if cls is arg:
                    warnings.warn(msg, category=category, stacklevel=stacklevel + 1)
                if original_new is not object.__new__:
                    return original_new(cls, *args, **kwargs)
                else:
                    return original_new(cls)
            arg.__new__ = __new__
            arg.__deprecated__ = msg
            return arg
        elif callable(arg):
            def wrapper(*args, **kwargs):
                warnings.warn(msg, category=category, stacklevel=stacklevel + 1)
                return arg(*args, **kwargs)
            wrapper.__wrapped__ = arg
            wrapper.__deprecated__ = msg
            wrapper.__name__ = getattr(arg, '__name__', '<unknown>')
            wrapper.__qualname__ = getattr(arg, '__qualname__', wrapper.__name__)
            wrapper.__doc__ = getattr(arg, '__doc__', None)
            return wrapper
        else:
            raise TypeError(
                "@deprecated decorator with non-None category must be applied to "
                f"a class or callable, not {arg!r}"
            )

    def __repr__(self):
        return f"@deprecated({self.message!r})"


# ── Doc annotation (PEP 727) ──

class Doc:
    """Add documentation to a parameter or attribute."""
    def __init__(self, documentation, /):
        self.documentation = documentation
    def __repr__(self):
        return f"Doc({self.documentation!r})"
    def __hash__(self):
        return hash(self.documentation)
    def __eq__(self, other):
        if isinstance(other, Doc):
            return self.documentation == other.documentation
        return NotImplemented


# ── NoDefault sentinel ──

class _NoDefaultType:
    __instance = None
    def __new__(cls):
        if cls.__instance is None:
            cls.__instance = super().__new__(cls)
        return cls.__instance
    def __repr__(self):
        return 'typing_extensions.NoDefault'

NoDefault = _NoDefaultType()


# ── TypeIs (PEP 742) ──

class TypeIs:
    """Type narrowing predicate — like TypeGuard but narrows in both branches."""
    def __init__(self, tp):
        self.__type__ = tp
    def __class_getitem__(cls, item):
        return cls(item)
    def __repr__(self):
        return f"TypeIs[{self.__type__!r}]"


# ── Sentinel (PEP 661) ──

class Sentinel:
    """Create a unique sentinel object."""
    def __init__(self, name, repr=None):
        self._name = name
        self._repr = repr if repr is not None else f'<{name}>'
    def __repr__(self):
        return self._repr
    def __call__(self, *args, **kwargs):
        raise TypeError(f"{type(self).__name__!r} object is not callable")
    def __getstate__(self):
        raise TypeError(f"Cannot pickle {type(self).__name__!r} object")
