"""High-performance container datatypes - pure Python implementations."""

import copy
import keyword
import sys
import reprlib


class OrderedDict(dict):
    """Dictionary that remembers insertion order.

    In Python 3.7+ regular dicts are ordered, so this is a thin wrapper.
    """

    def __repr__(self):
        if not self:
            return 'OrderedDict()'
        items = ', '.join('%r: %r' % (k, v) for k, v in self.items())
        return 'OrderedDict({%s})' % items

    def __eq__(self, other):
        if isinstance(other, OrderedDict):
            return list(self.items()) == list(other.items())
        return dict.__eq__(self, other)

    def move_to_end(self, key, last=True):
        """Move an existing key to either end of an ordered dictionary."""
        if key not in self:
            raise KeyError(key)
        value = self[key]
        del self[key]
        self[key] = value


class Counter(dict):
    """Dict subclass for counting hashable items."""

    def __init__(self, iterable=None, **kwds):
        super().__init__()
        self.update(iterable, **kwds)

    def update(self, iterable=None, **kwds):
        if iterable is not None:
            if isinstance(iterable, dict):
                for elem, count in iterable.items():
                    self[elem] = self.get(elem, 0) + count
            else:
                for elem in iterable:
                    self[elem] = self.get(elem, 0) + 1
        for elem, count in kwds.items():
            self[elem] = self.get(elem, 0) + count

    def __missing__(self, key):
        return 0

    def most_common(self, n=None):
        """List the n most common elements and their counts."""
        items = sorted(self.items(), key=lambda x: x[1], reverse=True)
        if n is not None:
            return items[:n]
        return items

    def elements(self):
        """Iterator over elements repeating each as many times as its count."""
        for elem, count in self.items():
            for _ in range(count):
                yield elem

    def subtract(self, iterable=None, **kwds):
        """Subtract count (not replace)."""
        if iterable is not None:
            if isinstance(iterable, dict):
                for elem, count in iterable.items():
                    self[elem] = self.get(elem, 0) - count
            else:
                for elem in iterable:
                    self[elem] = self.get(elem, 0) - 1
        for elem, count in kwds.items():
            self[elem] = self.get(elem, 0) - count

    def __add__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            newcount = count + other.get(elem, 0)
            if newcount > 0:
                result[elem] = newcount
        for elem, count in other.items():
            if elem not in self and count > 0:
                result[elem] = count
        return result

    def __sub__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            newcount = count - other.get(elem, 0)
            if newcount > 0:
                result[elem] = newcount
        return result

    def __and__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem, count in self.items():
            other_count = other.get(elem, 0)
            newcount = min(count, other_count)
            if newcount > 0:
                result[elem] = newcount
        return result

    def __or__(self, other):
        if not isinstance(other, Counter):
            return NotImplemented
        result = Counter()
        for elem in set(self) | set(other):
            newcount = max(self.get(elem, 0), other.get(elem, 0))
            if newcount > 0:
                result[elem] = newcount
        return result

    def __repr__(self):
        if not self:
            return 'Counter()'
        items = ', '.join('%r: %r' % (k, v) for k, v in self.most_common())
        return 'Counter({%s})' % items

    def total(self):
        """Sum of all counts."""
        return sum(self.values())

    def copy(self):
        return Counter(self)


def most_common(counter, n=None):
    return counter.most_common(n)


def counter_elements(counter):
    return list(counter.elements())


def counter_update(counter, iterable=None, **kwds):
    counter.update(iterable, **kwds)
    return counter


def counter_subtract(counter, iterable=None, **kwds):
    counter.subtract(iterable, **kwds)
    return counter


def counter_total(counter):
    return counter.total()


def counter_copy(counter):
    return counter.copy()


def counter_clear(counter):
    counter.clear()
    return None


def _count_elements(mapping, iterable):
    mapping.update(iterable)


class defaultdict(dict):
    """Dict subclass that calls a factory function to supply missing values."""

    def __init__(self, default_factory=None, *args, **kwargs):
        self.default_factory = default_factory
        super().__init__(*args, **kwargs)

    def __missing__(self, key):
        if self.default_factory is None:
            raise KeyError(key)
        self[key] = value = self.default_factory()
        return value

    def __repr__(self):
        return 'defaultdict(%s, %s)' % (self.default_factory, dict.__repr__(self))

    def copy(self):
        return defaultdict(self.default_factory, self)


class _DequeIterator:
    def __init__(self, deq, reverse=False):
        self._deque = deq
        self._state = deq._state
        self._reverse = reverse
        self._index = len(deq._data) - 1 if reverse else 0

    def __iter__(self):
        return self

    def __next__(self):
        if self._state != self._deque._state:
            raise RuntimeError('deque mutated during iteration')
        data = self._deque._data
        if self._reverse:
            if self._index < 0:
                raise StopIteration
            item = data[self._index]
            self._index -= 1
            return item
        if self._index >= len(data):
            raise StopIteration
        item = data[self._index]
        self._index += 1
        return item

    def __reduce__(self):
        return (type(self), (self._deque.copy(), self._reverse),
                (self._index, self._state))

    def __setstate__(self, state):
        self._index, self._state = state


class _DequeReverseIterator(_DequeIterator):
    def __init__(self, deq):
        super().__init__(deq, True)

    def __reduce__(self):
        return (type(self), (self._deque.copy(),), (self._index, self._state))


class deque:
    """Double-ended queue implemented with a list (simplified)."""

    __hash__ = None

    def __init__(self, iterable=None, maxlen=None):
        self._data = []
        self._maxlen = maxlen
        self._state = 0
        if iterable is not None:
            self.extend(iterable)

    @property
    def maxlen(self):
        return self._maxlen

    def _bump(self):
        self._state += 1

    def _trim_left(self):
        if self._maxlen is not None:
            excess = len(self._data) - self._maxlen
            if excess > 0:
                del self._data[:excess]

    def _trim_right(self):
        if self._maxlen is not None:
            excess = len(self._data) - self._maxlen
            if excess > 0:
                del self._data[self._maxlen:]

    def append(self, x):
        if self._maxlen == 0:
            self._bump()
            return
        self._data.append(x)
        self._trim_left()
        self._bump()

    def appendleft(self, x):
        if self._maxlen == 0:
            self._bump()
            return
        if self._maxlen is not None and len(self._data) >= self._maxlen:
            self._data.pop()
        self._data.insert(0, x)
        self._bump()

    def pop(self):
        if not self._data:
            raise IndexError('pop from an empty deque')
        self._bump()
        return self._data.pop()

    def popleft(self):
        if not self._data:
            raise IndexError('pop from an empty deque')
        self._bump()
        return self._data.pop(0)

    def extend(self, iterable):
        items = list(iterable)
        if self._maxlen == 0:
            if items:
                self._bump()
            return
        if self._maxlen is not None and len(items) >= self._maxlen:
            self._data[:] = items[-self._maxlen:]
            self._bump()
            return
        self._data.extend(items)
        self._trim_left()
        self._bump()

    def extendleft(self, iterable):
        items = list(iterable)
        if self._maxlen == 0:
            if items:
                self._bump()
            return
        if self._maxlen is not None and len(items) >= self._maxlen:
            self._data[:] = list(reversed(items))[:self._maxlen]
            self._bump()
            return
        if items:
            self._data[:0] = reversed(items)
        self._trim_right()
        self._bump()

    def rotate(self, n=1):
        if not self._data:
            return
        n = n % len(self._data)
        if n:
            self._data = self._data[-n:] + self._data[:-n]
            self._bump()

    def clear(self):
        if self._data:
            self._data.clear()
            self._bump()

    def _compare(self, a, b):
        try:
            return a == b
        except Exception:
            raise RuntimeError

    def count(self, x):
        state = self._state
        count = 0
        for item in self._data:
            if self._state != state:
                raise RuntimeError
            if self._compare(item, x):
                count += 1
            if self._state != state:
                raise RuntimeError
        return count

    def index(self, x, start=0, stop=None):
        if stop is None:
            stop = len(self._data)
        data = self._data
        state = self._state
        n = len(data)
        if start < 0:
            start += n
            if start < 0:
                start = 0
        elif start > n:
            start = n
        if stop < 0:
            stop += n
            if stop < 0:
                stop = 0
        elif stop > n:
            stop = n
        for i in range(start, stop):
            if self._state != state:
                raise RuntimeError
            if self._compare(data[i], x):
                if self._state != state:
                    raise RuntimeError
                return i
            if self._state != state:
                raise RuntimeError
        raise ValueError('%r is not in deque' % x)

    def insert(self, i, x):
        if self._maxlen is not None and len(self._data) >= self._maxlen:
            raise IndexError('deque already at its maximum size')
        self._data.insert(i, x)
        self._trim_left()
        self._bump()

    def remove(self, value):
        state = self._state
        for i, item in enumerate(self._data):
            if self._state != state:
                raise RuntimeError
            if self._compare(item, value):
                del self._data[i]
                self._bump()
                return
            if self._state != state:
                raise RuntimeError
        raise ValueError('deque.remove(x): x not in deque')

    def reverse(self):
        self._data.reverse()
        self._bump()

    def copy(self):
        return type(self)(self._data, self._maxlen)

    def __copy__(self):
        return self.copy()

    def __deepcopy__(self, memo):
        return type(self)(copy.deepcopy(self._data, memo), self._maxlen)

    def __reduce__(self):
        return (type(self), (list(self._data), self._maxlen))

    def __len__(self):
        return len(self._data)

    def __bool__(self):
        return bool(self._data)

    def __contains__(self, item):
        state = self._state
        for elem in self._data:
            if self._state != state:
                raise RuntimeError
            if self._compare(elem, item):
                if self._state != state:
                    raise RuntimeError
                return True
            if self._state != state:
                raise RuntimeError
        return False

    def __getitem__(self, index):
        return self._data[index]

    def __setitem__(self, index, value):
        self._data[index] = value
        self._bump()

    def __delitem__(self, index):
        del self._data[index]
        self._bump()

    def __iter__(self):
        return _DequeIterator(self)

    def __reversed__(self):
        return _DequeReverseIterator(self)

    def __add__(self, other):
        try:
            items = list(other)
        except TypeError:
            return NotImplemented
        return type(self)(list(self._data) + items, self._maxlen)

    def __iadd__(self, other):
        self.extend(other)
        return self

    def __mul__(self, n):
        if not isinstance(n, int):
            return NotImplemented
        return type(self)(self._data * n, self._maxlen)

    def __rmul__(self, n):
        return self.__mul__(n)

    def __imul__(self, n):
        if not isinstance(n, int):
            return NotImplemented
        self._data *= n
        self._trim_left()
        self._bump()
        return self

    def __eq__(self, other):
        if isinstance(other, deque):
            return self._data == other._data
        return NotImplemented

    @reprlib.recursive_repr()
    def __repr__(self):
        items = ', '.join(repr(x) for x in self._data)
        if self._maxlen is not None:
            return 'deque([%s], maxlen=%d)' % (items, self._maxlen)
        return 'deque([%s])' % items


def namedtuple(typename, field_names, *, rename=False, defaults=None, module=None):
    """Returns a new subclass of tuple with named fields.

    >>> Point = namedtuple('Point', ['x', 'y'])
    >>> p = Point(11, y=22)
    >>> p.x + p.y
    33
    """
    if not isinstance(typename, str):
        raise TypeError('Type names and field names must be strings')
    if not typename.isidentifier() or keyword.iskeyword(typename):
        raise ValueError('Type names and field names must be valid identifiers')
    if isinstance(field_names, str):
        field_names = field_names.replace(',', ' ').split()
    field_names = list(field_names)

    if rename:
        seen = set()
        for index, name in enumerate(field_names):
            if (not isinstance(name, str) or not name.isidentifier() or
                    keyword.iskeyword(name) or name.startswith('_') or
                    name in seen):
                field_names[index] = '_%d' % index
            seen.add(field_names[index])

    seen = set()
    for name in field_names:
        if not isinstance(name, str):
            raise TypeError('Type names and field names must be strings')
        if name.startswith('_') and not rename:
            raise ValueError('Field names cannot start with an underscore: %r' % name)
        if not name.isidentifier() or keyword.iskeyword(name):
            raise ValueError('Field names must be valid identifiers: %r' % name)
        if name in seen:
            raise ValueError('Encountered duplicate field name: %r' % name)
        seen.add(name)

    num_fields = len(field_names)
    if defaults is not None:
        defaults = tuple(defaults)
        if len(defaults) > num_fields:
            raise TypeError('Too many default values')
    if module is None:
        try:
            module = sys._getframe(1).f_globals.get('__name__', '__main__')
        except Exception:
            module = '__main__'

    def __new__(cls, *args, **kwargs):
        if defaults:
            n_defaults = len(defaults)
            n_required = num_fields - n_defaults
            all_args = list(args)
            for i, name in enumerate(field_names[len(args):], len(args)):
                if name in kwargs:
                    all_args.append(kwargs[name])
                elif i >= n_required:
                    all_args.append(defaults[i - n_required])
                else:
                    raise TypeError('__new__() missing required argument: %r' % name)
        else:
            all_args = list(args)
            for name in field_names[len(args):]:
                if name in kwargs:
                    all_args.append(kwargs[name])
                else:
                    raise TypeError('__new__() missing required argument: %r' % name)

        if len(all_args) != num_fields:
            raise TypeError('Expected %d arguments, got %d' % (num_fields, len(all_args)))
        result = tuple.__new__(cls, all_args)
        return result

    def __repr__(self):
        parts = []
        for i, name in enumerate(field_names):
            parts.append('%s=%r' % (name, self[i]))
        return '%s(%s)' % (type(self).__name__, ', '.join(parts))

    def __getnewargs__(self):
        return tuple(self)

    def _asdict(self):
        return dict(zip(field_names, self))

    def _replace(self, **kwargs):
        result = self._asdict()
        result.update(kwargs)
        return type(self)(**result)

    @classmethod
    def _make(cls, iterable):
        return cls(*iterable)

    namespace = {
        '__new__': __new__,
        '__repr__': __repr__,
        '_asdict': _asdict,
        '_replace': _replace,
        '_make': _make,
        '_fields': tuple(field_names),
        '__slots__': (),
        '__getnewargs__': __getnewargs__,
    }

    for index, name in enumerate(field_names):
        def _make_property(idx):
            return property(lambda self: self[idx],
                            doc='Alias for field number %d' % idx)
        namespace[name] = _make_property(index)

    result = type(typename, (tuple,), namespace)

    result.__doc__ = '%s(%s)' % (typename, ', '.join(field_names))
    if defaults is not None:
        result._field_defaults = dict(zip(field_names[-len(defaults):], defaults))
    else:
        result._field_defaults = {}
    result.__module__ = module
    __new__.__defaults__ = defaults if defaults is not None else None
    result.__new__.__defaults__ = defaults if defaults is not None else None

    return result


class ChainMap:
    """A ChainMap groups multiple dicts together to provide a single view."""

    def __init__(self, *maps):
        self.maps = list(maps) or [{}]

    def __getitem__(self, key):
        for mapping in self.maps:
            try:
                return mapping[key]
            except KeyError:
                pass
        raise KeyError(key)

    def __setitem__(self, key, value):
        self.maps[0][key] = value

    def __delitem__(self, key):
        try:
            del self.maps[0][key]
        except KeyError:
            raise KeyError('Key not found in the first mapping: %r' % key)

    def __contains__(self, key):
        for mapping in self.maps:
            if key in mapping:
                return True
        return False

    def __len__(self):
        seen = set()
        for mapping in self.maps:
            for key in mapping:
                seen.add(key)
        return len(seen)

    def __iter__(self):
        seen = set()
        for mapping in self.maps:
            for key in mapping:
                if key not in seen:
                    seen.add(key)
                    yield key

    def get(self, key, default=None):
        try:
            return self[key]
        except KeyError:
            return default

    def keys(self):
        return list(self)

    def values(self):
        return [self[k] for k in self]

    def items(self):
        return [(k, self[k]) for k in self]

    def new_child(self, m=None):
        if m is None:
            m = {}
        return ChainMap(m, *self.maps)

    @property
    def parents(self):
        return ChainMap(*self.maps[1:])

    def __repr__(self):
        return 'ChainMap(%s)' % ', '.join(repr(m) for m in self.maps)


class UserDict(dict):
    """Wrapper around dictionary objects for easier subclassing."""

    def __init__(self, dict=None, **kwargs):
        self.data = {}
        if dict is not None:
            self.update(dict)
        if kwargs:
            self.update(kwargs)

    def __len__(self):
        return len(self.data)

    def __getitem__(self, key):
        if key in self.data:
            return self.data[key]
        raise KeyError(key)

    def __setitem__(self, key, item):
        self.data[key] = item

    def __delitem__(self, key):
        del self.data[key]

    def __iter__(self):
        return iter(self.data)

    def __contains__(self, key):
        return key in self.data

    def __repr__(self):
        return repr(self.data)

    def __or__(self, other):
        if isinstance(other, UserDict):
            return self.__class__(self.data | other.data)
        if isinstance(other, dict):
            return self.__class__(self.data | other)
        return NotImplemented

    def copy(self):
        c = self.__class__()
        c.data = self.data.copy()
        return c

    def keys(self):
        return self.data.keys()

    def items(self):
        return self.data.items()

    def values(self):
        return self.data.values()

    def get(self, key, default=None):
        return self.data.get(key, default)

    def update(self, other=None, **kwargs):
        if other is not None:
            if hasattr(other, 'items'):
                for k, v in other.items():
                    self.data[k] = v
            else:
                for k, v in other:
                    self.data[k] = v
        for k, v in kwargs.items():
            self.data[k] = v

    def pop(self, key, *args):
        return self.data.pop(key, *args)

    def setdefault(self, key, default=None):
        return self.data.setdefault(key, default)


class UserList(list):
    """Wrapper around list objects for easier subclassing."""

    def __init__(self, initlist=None):
        self.data = []
        if initlist is not None:
            if isinstance(initlist, UserList):
                self.data = initlist.data[:]
            elif isinstance(initlist, list):
                self.data = initlist[:]
            else:
                self.data = list(initlist)

    def __repr__(self):
        return repr(self.data)

    def __len__(self):
        return len(self.data)

    def __getitem__(self, i):
        if isinstance(i, slice):
            return self.__class__(self.data[i])
        return self.data[i]

    def __setitem__(self, i, item):
        self.data[i] = item

    def __delitem__(self, i):
        del self.data[i]

    def __contains__(self, item):
        return item in self.data

    def __iter__(self):
        return iter(self.data)

    def __add__(self, other):
        if isinstance(other, UserList):
            return self.__class__(self.data + other.data)
        return self.__class__(self.data + list(other))

    def __mul__(self, n):
        return self.__class__(self.data * n)

    def append(self, item):
        self.data.append(item)

    def insert(self, i, item):
        self.data.insert(i, item)

    def pop(self, i=-1):
        return self.data.pop(i)

    def remove(self, item):
        self.data.remove(item)

    def clear(self):
        self.data.clear()

    def copy(self):
        return self.__class__(self.data[:])

    def count(self, item):
        return self.data.count(item)

    def index(self, item, *args):
        return self.data.index(item, *args)

    def reverse(self):
        self.data.reverse()

    def sort(self, *args, **kwargs):
        self.data.sort(*args, **kwargs)

    def extend(self, other):
        if isinstance(other, UserList):
            self.data.extend(other.data)
        else:
            self.data.extend(other)


class UserString(str):
    """Wrapper around string objects for easier subclassing."""

    def __init__(self, seq=''):
        if isinstance(seq, str):
            self.data = seq
        elif isinstance(seq, UserString):
            self.data = seq.data
        else:
            self.data = str(seq)

    def __str__(self):
        return self.data

    def __repr__(self):
        return repr(self.data)

    def __len__(self):
        return len(self.data)

    def __getitem__(self, index):
        return self.__class__(self.data[index])

    def __add__(self, other):
        if isinstance(other, UserString):
            return self.__class__(self.data + other.data)
        return self.__class__(self.data + str(other))

    def __mul__(self, n):
        return self.__class__(self.data * n)

    def __contains__(self, char):
        return char in self.data

    def __eq__(self, other):
        if isinstance(other, UserString):
            return self.data == other.data
        return self.data == other

    def __hash__(self):
        return hash(self.data)

    def __iter__(self):
        return iter(self.data)

    def upper(self):
        return self.__class__(self.data.upper())

    def lower(self):
        return self.__class__(self.data.lower())

    def strip(self, chars=None):
        return self.__class__(self.data.strip(chars))

    def split(self, sep=None, maxsplit=-1):
        return self.data.split(sep, maxsplit)

    def replace(self, old, new, count=-1):
        if count < 0:
            return self.__class__(self.data.replace(old, new))
        return self.__class__(self.data.replace(old, new, count))

    def find(self, sub, start=0, end=None):
        if end is None:
            end = len(self.data)
        return self.data.find(sub, start, end)

    def count(self, sub, start=0, end=None):
        if end is None:
            end = len(self.data)
        return self.data.count(sub, start, end)

    def startswith(self, prefix, start=0, end=None):
        if end is None:
            end = len(self.data)
        return self.data.startswith(prefix, start, end)

    def endswith(self, suffix, start=0, end=None):
        if end is None:
            end = len(self.data)
        return self.data.endswith(suffix, start, end)

    def join(self, seq):
        return self.data.join(seq)

    def title(self):
        return self.__class__(self.data.title())

    def capitalize(self):
        return self.__class__(self.data.capitalize())

    def encode(self, encoding='utf-8', errors='strict'):
        return self.data.encode(encoding, errors)
