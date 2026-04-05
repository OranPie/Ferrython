"""High-performance container datatypes - pure Python implementations."""

import sys


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


class deque:
    """Double-ended queue implemented with a list (simplified)."""

    def __init__(self, iterable=None, maxlen=None):
        self._data = []
        self.maxlen = maxlen
        if iterable is not None:
            for item in iterable:
                self.append(item)

    def append(self, x):
        self._data.append(x)
        if self.maxlen is not None and len(self._data) > self.maxlen:
            self._data.pop(0)

    def appendleft(self, x):
        self._data.insert(0, x)
        if self.maxlen is not None and len(self._data) > self.maxlen:
            self._data.pop()

    def pop(self):
        if not self._data:
            raise IndexError('pop from an empty deque')
        return self._data.pop()

    def popleft(self):
        if not self._data:
            raise IndexError('pop from an empty deque')
        return self._data.pop(0)

    def extend(self, iterable):
        for item in iterable:
            self.append(item)

    def extendleft(self, iterable):
        for item in iterable:
            self.appendleft(item)

    def rotate(self, n=1):
        if not self._data:
            return
        n = n % len(self._data)
        self._data = self._data[-n:] + self._data[:-n]

    def clear(self):
        self._data.clear()

    def count(self, x):
        return self._data.count(x)

    def index(self, x, start=0, stop=None):
        if stop is None:
            stop = len(self._data)
        for i in range(start, stop):
            if self._data[i] == x:
                return i
        raise ValueError('%r is not in deque' % x)

    def insert(self, i, x):
        if self.maxlen is not None and len(self._data) >= self.maxlen:
            raise IndexError('deque already at its maximum size')
        self._data.insert(i, x)

    def remove(self, value):
        self._data.remove(value)

    def reverse(self):
        self._data.reverse()

    def copy(self):
        return deque(self._data, self.maxlen)

    def __len__(self):
        return len(self._data)

    def __bool__(self):
        return bool(self._data)

    def __contains__(self, item):
        return item in self._data

    def __getitem__(self, index):
        return self._data[index]

    def __setitem__(self, index, value):
        self._data[index] = value

    def __delitem__(self, index):
        del self._data[index]

    def __iter__(self):
        return iter(self._data)

    def __reversed__(self):
        return reversed(self._data)

    def __eq__(self, other):
        if isinstance(other, deque):
            return self._data == other._data
        return NotImplemented

    def __repr__(self):
        items = ', '.join(repr(x) for x in self._data)
        if self.maxlen is not None:
            return 'deque([%s], maxlen=%d)' % (items, self.maxlen)
        return 'deque([%s])' % items


def namedtuple(typename, field_names, rename=False, defaults=None, module=None):
    """Returns a new subclass of tuple with named fields.

    >>> Point = namedtuple('Point', ['x', 'y'])
    >>> p = Point(11, y=22)
    >>> p.x + p.y
    33
    """
    if isinstance(field_names, str):
        field_names = field_names.replace(',', ' ').split()
    field_names = list(field_names)

    if rename:
        seen = set()
        for index, name in enumerate(field_names):
            if (not name.isidentifier() or name.startswith('_') or
                    name in seen):
                field_names[index] = '_%d' % index
            seen.add(field_names[index])

    for name in [typename] + field_names:
        if not isinstance(name, str):
            raise TypeError('Type names and field names must be strings')

    seen = set()
    for name in field_names:
        if name.startswith('_') and not rename:
            raise ValueError('Field names cannot start with an underscore: %r' % name)
        if name in seen:
            raise ValueError('Encountered duplicate field name: %r' % name)
        seen.add(name)

    num_fields = len(field_names)

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
        return '%s(%s)' % (typename, ', '.join(parts))

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
    }

    for index, name in enumerate(field_names):
        def _make_property(idx):
            return property(lambda self: self[idx],
                            doc='Alias for field number %d' % idx)
        namespace[name] = _make_property(index)

    result = type(typename, (tuple,), namespace)

    if defaults is not None:
        result._field_defaults = dict(zip(field_names[-len(defaults):], defaults))
    else:
        result._field_defaults = {}

    if module is not None:
        result.__module__ = module

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
