"""Define names for built-in types that aren't directly accessible as a builtin."""

import sys


# Iterators
def _f(): pass
FunctionType = type(_f)
LambdaType = type(lambda: None)

def _g():
    yield 1
GeneratorType = type(_g())

class _C:
    def _m(self): pass
MethodType = type(_C()._m)
BuiltinFunctionType = type(len)
BuiltinMethodType = type([].append)

ModuleType = type(sys)

try:
    raise TypeError
except TypeError:
    tb = sys.exc_info()[2]
    if tb is not None:
        TracebackType = type(tb)
        FrameType = type(tb.tb_frame) if hasattr(tb, 'tb_frame') else None
    else:
        TracebackType = None
        FrameType = None
    del tb

NoneType = type(None)


class SimpleNamespace:
    """A simple attribute-based namespace.

    SimpleNamespace(**kwargs)
    """

    def __init__(self, **kwargs):
        self.__dict__.update(kwargs)

    def __repr__(self):
        items = []
        for k, v in self.__dict__.items():
            items.append("%s=%r" % (k, v))
        return "namespace(%s)" % ", ".join(items)

    def __eq__(self, other):
        if isinstance(other, SimpleNamespace):
            return self.__dict__ == other.__dict__
        return NotImplemented

    def __ne__(self, other):
        if isinstance(other, SimpleNamespace):
            return self.__dict__ != other.__dict__
        return NotImplemented


class MappingProxyType:
    """Read-only proxy of a mapping."""

    def __init__(self, mapping):
        self._mapping = mapping

    def __getitem__(self, key):
        return self._mapping[key]

    def __contains__(self, key):
        return key in self._mapping

    def __len__(self):
        return len(self._mapping)

    def __iter__(self):
        return iter(self._mapping)

    def get(self, key, default=None):
        return self._mapping.get(key, default)

    def keys(self):
        return self._mapping.keys()

    def values(self):
        return self._mapping.values()

    def items(self):
        return self._mapping.items()

    def __repr__(self):
        return "mappingproxy(%r)" % (self._mapping,)


def coroutine(func):
    """Decorator to mark a generator as a coroutine."""
    func._is_coroutine = True
    return func


def new_class(name, bases=(), kwds=None, exec_body=None):
    """Create a class object dynamically using the appropriate metaclass."""
    if kwds is None:
        kwds = {}
    metaclass = kwds.pop('metaclass', type)
    ns = {}
    if exec_body is not None:
        exec_body(ns)
    return metaclass(name, bases, ns, **kwds)


def prepare_class(name, bases=(), kwds=None):
    """Call the __prepare__ method of the appropriate metaclass."""
    if kwds is None:
        kwds = {}
    metaclass = kwds.pop('metaclass', type)
    ns = {}
    if hasattr(metaclass, '__prepare__'):
        ns = metaclass.__prepare__(name, bases, **kwds)
    return metaclass, ns, kwds
