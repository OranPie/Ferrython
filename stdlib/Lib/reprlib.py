"""Recursive repr with length limits."""
import builtins

class Repr:
    maxlevel = 6
    maxtuple = 6
    maxlist = 6
    maxarray = 5
    maxdict = 4
    maxset = 6
    maxfrozenset = 6
    maxdeque = 6
    maxstring = 30
    maxlong = 40
    maxother = 30

    def repr(self, obj):
        return self.repr1(obj, self.maxlevel)
    
    def repr1(self, obj, level):
        if level <= 0:
            return '...'
        typename = type(obj).__name__
        method = getattr(self, 'repr_' + typename, None)
        if method:
            return method(obj, level)
        return builtins.repr(obj)
    
    def _repr_iterable(self, obj, level, left, right, maxiter):
        if level <= 0:
            return left + '...' + right
        n = len(obj)
        items = []
        for i, item in enumerate(obj):
            if i >= maxiter:
                items.append('...')
                break
            items.append(self.repr1(item, level - 1))
        return left + ', '.join(items) + right
    
    def repr_tuple(self, obj, level):
        if len(obj) == 1:
            return '(' + self.repr1(obj[0], level - 1) + ',)'
        return self._repr_iterable(obj, level, '(', ')', self.maxtuple)
    
    def repr_list(self, obj, level):
        return self._repr_iterable(obj, level, '[', ']', self.maxlist)
    
    def repr_set(self, obj, level):
        if not obj:
            return 'set()'
        return self._repr_iterable(sorted(obj), level, '{', '}', self.maxset)
    
    def repr_frozenset(self, obj, level):
        if not obj:
            return 'frozenset()'
        return 'frozenset(' + self._repr_iterable(sorted(obj), level, '{', '}', self.maxfrozenset) + ')'
    
    def repr_dict(self, obj, level):
        if level <= 0:
            return '{...}'
        items = []
        count = 0
        for k in obj:
            if count >= self.maxdict:
                items.append('...')
                break
            v = obj[k]
            items.append(self.repr1(k, level-1) + ': ' + self.repr1(v, level-1))
            count = count + 1
        return '{' + ', '.join(items) + '}'
    
    def repr_str(self, obj, level):
        s = builtins.repr(obj)
        if len(s) > self.maxstring:
            i = max(0, (self.maxstring - 3) // 2)
            j = max(0, self.maxstring - 3 - i)
            s = builtins.repr(obj[:i] + obj[len(obj)-j:])
            s = s[:i] + '...' + s[len(s)-j:]
        return s
    
    def repr_int(self, obj, level):
        s = builtins.repr(obj)
        if len(s) > self.maxlong:
            i = max(0, self.maxlong - 3) // 2
            j = max(0, self.maxlong - 3 - i)
            s = s[:i] + '...' + s[len(s)-j:]
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
                result = user_function(self)
            finally:
                running.discard(key)
            return result
        wrapper.__wrapped__ = user_function
        return wrapper
    return decorator
