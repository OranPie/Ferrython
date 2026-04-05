"""functools - Higher-order functions and operations on callable objects.

This module provides pure Python implementations of functools utilities.
The Rust-implemented lru_cache is available from the built-in functools module.
"""


WRAPPER_ASSIGNMENTS = ('__module__', '__name__', '__qualname__', '__doc__',
                       '__dict__', '__wrapped__')
WRAPPER_UPDATES = ('__dict__',)


def update_wrapper(wrapper, wrapped,
                   assigned=WRAPPER_ASSIGNMENTS,
                   updated=WRAPPER_UPDATES):
    """Update a wrapper function to look like the wrapped function."""
    for attr in assigned:
        try:
            value = getattr(wrapped, attr)
        except AttributeError:
            pass
        else:
            setattr(wrapper, attr, value)
    for attr in updated:
        try:
            getattr(wrapper, attr).update(getattr(wrapped, attr, {}))
        except AttributeError:
            pass
    wrapper.__wrapped__ = wrapped
    return wrapper


def wraps(wrapped, assigned=WRAPPER_ASSIGNMENTS, updated=WRAPPER_UPDATES):
    """Decorator factory to apply update_wrapper() to a wrapper function."""
    def decorator(wrapper):
        return update_wrapper(wrapper, wrapped, assigned, updated)
    return decorator


def reduce(function, iterable, initial=None):
    """Apply a function of two arguments cumulatively to the items of an iterable."""
    it = iter(iterable)
    if initial is None:
        try:
            value = next(it)
        except StopIteration:
            raise TypeError('reduce() of empty iterable with no initial value')
    else:
        value = initial
    for element in it:
        value = function(value, element)
    return value


def partial(func, *args, **kwargs):
    """Create a new function with partial application of the given arguments."""
    def newfunc(*fargs, **fkwargs):
        newkeywords = dict(kwargs)
        newkeywords.update(fkwargs)
        return func(*args, *fargs, **newkeywords)
    newfunc.func = func
    newfunc.args = args
    newfunc.keywords = kwargs
    newfunc.__name__ = getattr(func, '__name__', 'partial')
    newfunc.__doc__ = getattr(func, '__doc__', None)
    return newfunc


def total_ordering(cls):
    """Class decorator that fills in missing ordering methods.

    Given a class that defines one or more rich comparison ordering methods,
    this decorator supplies the rest.
    """
    convert = {
        '__lt__': [
            ('__gt__', lambda self, other: not (self < other) and self != other),
            ('__le__', lambda self, other: self < other or self == other),
            ('__ge__', lambda self, other: not (self < other)),
        ],
        '__le__': [
            ('__ge__', lambda self, other: not (self <= other) or self == other),
            ('__lt__', lambda self, other: self <= other and self != other),
            ('__gt__', lambda self, other: not (self <= other)),
        ],
        '__gt__': [
            ('__lt__', lambda self, other: not (self > other) and self != other),
            ('__ge__', lambda self, other: self > other or self == other),
            ('__le__', lambda self, other: not (self > other)),
        ],
        '__ge__': [
            ('__gt__', lambda self, other: self >= other and self != other),
            ('__le__', lambda self, other: not (self >= other) or self == other),
            ('__lt__', lambda self, other: not (self >= other)),
        ],
    }

    roots = {op for op in convert if getattr(cls, op, None) is not getattr(object, op, None)}
    if not roots:
        raise ValueError('must have at least one ordering operation defined')

    root = max(roots)
    for opname, opfunc in convert[root]:
        if opname not in roots:
            setattr(cls, opname, opfunc)
    return cls


class cached_property:
    """Transform a method into a property cached on the instance.

    Similar to property(), with the addition of caching.
    """

    def __init__(self, func):
        self.func = func
        self.attrname = None
        self.__doc__ = func.__doc__
        self.__name__ = func.__name__

    def __set_name__(self, owner, name):
        if self.attrname is None:
            self.attrname = name
        elif name != self.attrname:
            raise TypeError(
                "Cannot assign the same cached_property to two different names "
                "(%r and %r)." % (self.attrname, name)
            )

    def __get__(self, instance, owner=None):
        if instance is None:
            return self
        if self.attrname is None:
            raise TypeError(
                "Cannot use cached_property instance without calling __set_name__ on it.")
        try:
            val = instance.__dict__[self.attrname]
        except (KeyError, AttributeError):
            val = self.func(instance)
            try:
                instance.__dict__[self.attrname] = val
            except AttributeError:
                pass
        return val


def cmp_to_key(mycmp):
    """Convert a cmp= function into a key= function."""
    class K:
        __slots__ = ['obj']
        def __init__(self, obj):
            self.obj = obj
        def __lt__(self, other):
            return mycmp(self.obj, other.obj) < 0
        def __gt__(self, other):
            return mycmp(self.obj, other.obj) > 0
        def __eq__(self, other):
            return mycmp(self.obj, other.obj) == 0
        def __le__(self, other):
            return mycmp(self.obj, other.obj) <= 0
        def __ge__(self, other):
            return mycmp(self.obj, other.obj) >= 0
        def __hash__(self):
            raise TypeError('unhashable type')
    return K


class singledispatch:
    """Single-dispatch generic function decorator."""

    def __init__(self, func):
        self._default = func
        self._registry = {}
        self.__name__ = getattr(func, '__name__', 'singledispatch')

    def register(self, cls, func=None):
        if func is None:
            # Used as decorator: @f.register(int)
            def decorator(f):
                self._registry[cls] = f
                return f
            return decorator
        self._registry[cls] = func
        return func

    def __call__(self, *args, **kwargs):
        if not args:
            return self._default(*args, **kwargs)
        tp = type(args[0])
        impl = self._registry.get(tp)
        if impl is not None:
            return impl(*args, **kwargs)
        # Check MRO
        for base in type(args[0]).__mro__:
            impl = self._registry.get(base)
            if impl is not None:
                return impl(*args, **kwargs)
        return self._default(*args, **kwargs)

    @property
    def registry(self):
        return dict(self._registry)
