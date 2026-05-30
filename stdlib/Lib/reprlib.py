"""Recursive repr with length limits."""

import builtins
from itertools import islice


def _possibly_sorted(x):
    try:
        return sorted(x)
    except Exception:
        return list(x)


class Repr:
    def __init__(self):
        self.fillvalue = '...'
        self.maxlevel = 6
        self.maxtuple = 6
        self.maxlist = 6
        self.maxarray = 5
        self.maxdict = 4
        self.maxset = 6
        self.maxfrozenset = 6
        self.maxdeque = 6
        self.maxstring = 30
        self.maxlong = 40
        self.maxother = 30

    def repr(self, x):
        return self.repr1(x, self.maxlevel)

    def repr1(self, x, level):
        typename = type(x).__name__
        if ' ' in typename:
            typename = '_'.join(typename.split())
        method = getattr(self, 'repr_' + typename, None)
        if method is not None:
            return method(x, level)
        return self.repr_instance(x, level)

    def _repr_iterable(self, x, level, left, right, maxiter, trail=''):
        n = len(x)
        if level <= 0 and n:
            s = self.fillvalue
        else:
            newlevel = level - 1
            pieces = [self.repr1(elem, newlevel) for elem in islice(x, maxiter)]
            if n > maxiter:
                pieces.append(self.fillvalue)
            s = ', '.join(pieces)
            if n == 1 and trail:
                right = trail + right
        return left + s + right

    def repr_tuple(self, x, level):
        return self._repr_iterable(x, level, '(', ')', self.maxtuple, ',')

    def repr_list(self, x, level):
        return self._repr_iterable(x, level, '[', ']', self.maxlist)

    def repr_array(self, x, level):
        if not x:
            return "array('%s')" % x.typecode
        return self._repr_iterable(
            x, level, "array('%s', [" % x.typecode, '])', self.maxarray)

    def repr_set(self, x, level):
        if not x:
            return 'set()'
        return self._repr_iterable(_possibly_sorted(x), level, '{', '}', self.maxset)

    def repr_frozenset(self, x, level):
        if not x:
            return 'frozenset()'
        return self._repr_iterable(
            _possibly_sorted(x), level, 'frozenset({', '})', self.maxfrozenset)

    def repr_deque(self, x, level):
        return self._repr_iterable(x, level, 'deque([', '])', self.maxdeque)

    def repr_dict(self, x, level):
        n = len(x)
        if n == 0:
            return '{}'
        if level <= 0:
            return '{' + self.fillvalue + '}'
        newlevel = level - 1
        pieces = []
        for key in islice(_possibly_sorted(x), self.maxdict):
            pieces.append('%s: %s' % (self.repr1(key, newlevel),
                                      self.repr1(x[key], newlevel)))
        if n > self.maxdict:
            pieces.append(self.fillvalue)
        return '{' + ', '.join(pieces) + '}'

    def repr_str(self, x, level):
        s = builtins.repr(x[:self.maxstring])
        if len(s) > self.maxstring:
            i = max(0, (self.maxstring - 3) // 2)
            j = max(0, self.maxstring - 3 - i)
            s = builtins.repr(x[:i] + x[len(x) - j:])
            s = s[:i] + self.fillvalue + s[len(s) - j:]
        return s

    def repr_int(self, x, level):
        s = builtins.repr(x)
        if len(s) > self.maxlong:
            i = max(0, (self.maxlong - 3) // 2)
            j = max(0, self.maxlong - 3 - i)
            s = s[:i] + self.fillvalue + s[len(s) - j:]
        return s

    def repr_instance(self, x, level):
        try:
            s = builtins.repr(x)
        except Exception:
            return '<%s instance at %#x>' % (x.__class__.__name__, id(x))
        if len(s) > self.maxother:
            i = max(0, (self.maxother - 3) // 2)
            j = max(0, self.maxother - 3 - i)
            s = s[:i] + self.fillvalue + s[len(s) - j:]
        return s


aRepr = Repr()
repr = aRepr.repr


def recursive_repr(fillvalue='...'):
    """Decorator to make a repr function handle recursive calls."""
    def decorator(user_function):
        running = set()

        def wrapper(self):
            key = id(self)
            if key in running:
                return fillvalue
            running.add(key)
            try:
                return user_function(self)
            finally:
                running.discard(key)

        wrapper.__module__ = getattr(user_function, '__module__')
        wrapper.__doc__ = getattr(user_function, '__doc__')
        wrapper.__name__ = getattr(user_function, '__name__')
        wrapper.__qualname__ = getattr(user_function, '__qualname__')
        wrapper.__annotations__ = getattr(user_function, '__annotations__', {})
        return wrapper
    return decorator
