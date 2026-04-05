"""Operator functions corresponding to built-in operations.

Extends the Rust operator module with higher-order helpers.
"""


class itemgetter:
    """Return a callable object that fetches item(s) from its operand.

    itemgetter(item) -> func  --  func(obj) returns obj[item]
    itemgetter(item1, item2, ...) -> func  --  func(obj) returns (obj[item1], obj[item2], ...)
    """
    __slots__ = ('_items', '_call')

    def __init__(self, item, *items):
        if not items:
            self._items = (item,)
            def func(obj):
                return obj[item]
            self._call = func
        else:
            self._items = (item,) + items
            def func(obj):
                return tuple(obj[i] for i in self._items)
            self._call = func

    def __call__(self, obj):
        return self._call(obj)

    def __repr__(self):
        return '%s.%s(%s)' % (self.__class__.__module__,
                               self.__class__.__name__,
                               ', '.join(map(repr, self._items)))


class attrgetter:
    """Return a callable object that fetches attr(s) from its operand.

    attrgetter(attr) -> func  --  func(obj) returns obj.attr
    attrgetter(attr1, attr2, ...) -> func  --  func(obj) returns (obj.attr1, obj.attr2, ...)
    """
    __slots__ = ('_attrs', '_call')

    def __init__(self, attr, *attrs):
        if not attrs:
            self._attrs = (attr,)
            def func(obj):
                for name in attr.split('.'):
                    obj = getattr(obj, name)
                return obj
            self._call = func
        else:
            self._attrs = (attr,) + attrs
            def func(obj):
                result = []
                for a in self._attrs:
                    val = obj
                    for name in a.split('.'):
                        val = getattr(val, name)
                    result.append(val)
                return tuple(result)
            self._call = func

    def __call__(self, obj):
        return self._call(obj)

    def __repr__(self):
        return '%s.%s(%s)' % (self.__class__.__module__,
                               self.__class__.__name__,
                               ', '.join(map(repr, self._attrs)))


class methodcaller:
    """Return a callable object that calls a method on its operand.

    methodcaller(name, ...) -> func  --  func(obj) returns obj.name(...)
    """
    __slots__ = ('_name', '_args', '_kwargs')

    def __init__(self, name, *args, **kwargs):
        self._name = name
        self._args = args
        self._kwargs = kwargs

    def __call__(self, obj):
        return getattr(obj, self._name)(*self._args, **self._kwargs)

    def __repr__(self):
        args = [repr(self._name)]
        args.extend(map(repr, self._args))
        args.extend('%s=%r' % (k, v) for k, v in self._kwargs.items())
        return '%s.%s(%s)' % (self.__class__.__module__,
                               self.__class__.__name__,
                               ', '.join(args))


# Standard operator functions
def lt(a, b): return a < b
def le(a, b): return a <= b
def eq(a, b): return a == b
def ne(a, b): return a != b
def ge(a, b): return a >= b
def gt(a, b): return a > b

def not_(a): return not a
def truth(a): return True if a else False
def is_(a, b): return a is b
def is_not(a, b): return a is not b

def add(a, b): return a + b
def sub(a, b): return a - b
def mul(a, b): return a * b
def truediv(a, b): return a / b
def floordiv(a, b): return a // b
def mod(a, b): return a % b
def pow(a, b): return a ** b
def neg(a): return -a
def pos(a): return +a
def abs(a): return __builtins__['abs'](a) if isinstance(__builtins__, dict) else a.__abs__()

def and_(a, b): return a & b
def or_(a, b): return a | b
def xor(a, b): return a ^ b
def invert(a): return ~a
def lshift(a, b): return a << b
def rshift(a, b): return a >> b

def concat(a, b): return a + b
def contains(a, b): return b in a
def countOf(a, b): return sum(1 for x in a if x == b)
def indexOf(a, b):
    for i, x in enumerate(a):
        if x == b:
            return i
    raise ValueError('sequence.index(x): x not in sequence')

def getitem(a, b): return a[b]
def setitem(a, b, c): a[b] = c
def delitem(a, b): del a[b]

def length_hint(obj, default=0):
    """Return an estimated length for the object."""
    if hasattr(obj, '__len__'):
        return len(obj)
    hint = getattr(type(obj), '__length_hint__', None)
    if hint is not None:
        val = hint(obj)
        if val is not NotImplemented and isinstance(val, int) and val >= 0:
            return val
    return default


# Aliases
__lt__ = lt
__le__ = le
__eq__ = eq
__ne__ = ne
__ge__ = ge
__gt__ = gt
__not__ = not_
__add__ = add
__sub__ = sub
__mul__ = mul
__truediv__ = truediv
__floordiv__ = floordiv
__mod__ = mod
__pow__ = pow
__neg__ = neg
__pos__ = pos
__and__ = and_
__or__ = or_
__xor__ = xor
__invert__ = invert
__lshift__ = lshift
__rshift__ = rshift
__concat__ = concat
__contains__ = contains
__getitem__ = getitem
__setitem__ = setitem
__delitem__ = delitem

# iadd etc. (in-place operators)
def iadd(a, b): a += b; return a
def isub(a, b): a -= b; return a
def imul(a, b): a *= b; return a
def itruediv(a, b): a /= b; return a
def ifloordiv(a, b): a //= b; return a
def imod(a, b): a %= b; return a
def ipow(a, b): a **= b; return a
def iand(a, b): a &= b; return a
def ior(a, b): a |= b; return a
def ixor(a, b): a ^= b; return a
def ilshift(a, b): a <<= b; return a
def irshift(a, b): a >>= b; return a
def iconcat(a, b): a += b; return a

__iadd__ = iadd
__isub__ = isub
__imul__ = imul
__itruediv__ = itruediv
__ifloordiv__ = ifloordiv
__imod__ = imod
__ipow__ = ipow
__iand__ = iand
__ior__ = ior
__ixor__ = ixor
__ilshift__ = ilshift
__irshift__ = irshift
__iconcat__ = iconcat
