"""Data Classes - pure Python implementation of field-based class generation."""


__all__ = [
    'dataclass', 'field', 'fields', 'asdict', 'astuple',
    'make_dataclass', 'replace', 'Field', 'FrozenInstanceError',
    'InitVar', 'MISSING',
]


class _MISSING_TYPE:
    pass

MISSING = _MISSING_TYPE()

_FIELD = 0
_FIELD_INITVAR = 1
_FIELD_CLASSVAR = 2

class FrozenInstanceError(AttributeError):
    pass


class InitVar:
    __slots__ = ('type',)
    def __init__(self, tp):
        self.type = tp
    def __class_getitem__(cls, tp):
        return InitVar(tp)


class Field:
    __slots__ = ('name', 'type', 'default', 'default_factory', 'repr',
                 'hash', 'init', 'compare', 'metadata', 'kw_only',
                 '_field_type')

    def __init__(self, default=MISSING, default_factory=MISSING,
                 init=True, repr=True, hash=None, compare=True,
                 metadata=None, kw_only=False):
        self.name = None
        self.type = None
        self.default = default
        self.default_factory = default_factory
        self.init = init
        self.repr = repr
        self.hash = hash
        self.compare = compare
        self.metadata = metadata or {}
        self.kw_only = kw_only
        self._field_type = _FIELD

    def __repr__(self):
        return ('Field(name=%r,type=%r,default=%r,default_factory=%r,'
                'init=%r,repr=%r,hash=%r,compare=%r,metadata=%r,kw_only=%r)'
                % (self.name, self.type, self.default, self.default_factory,
                   self.init, self.repr, self.hash, self.compare,
                   self.metadata, self.kw_only))


def field(default=MISSING, default_factory=MISSING, init=True, repr=True,
          hash=None, compare=True, metadata=None, kw_only=False):
    """Return an object to identify dataclass fields."""
    if default is not MISSING and default_factory is not MISSING:
        raise ValueError('cannot specify both default and default_factory')
    return Field(default, default_factory, init, repr, hash, compare,
                 metadata, kw_only)


def _process_class(cls, init, repr, eq, order, unsafe_hash, frozen):
    """Process a class to make it a dataclass."""
    # Collect fields from annotations
    cls_fields = []
    annotations = getattr(cls, '__annotations__', {})
    for name, tp in annotations.items():
        # Check if it's already a Field
        if isinstance(getattr(cls, name, MISSING), Field):
            f = getattr(cls, name)
        elif hasattr(cls, name):
            f = Field(default=getattr(cls, name))
        else:
            f = Field()
        f.name = name
        f.type = tp
        cls_fields.append(f)
        # Remove the class attribute (it's now managed by the dataclass)
        if hasattr(cls, name) and not isinstance(getattr(cls, name), Field):
            pass  # Keep default values accessible

    cls.__dataclass_fields__ = {f.name: f for f in cls_fields}

    if init:
        _set_init(cls, cls_fields, frozen)
    if repr:
        _set_repr(cls, cls_fields)
    if eq:
        _set_eq(cls, cls_fields)
    if order:
        _set_order(cls, cls_fields)
    if frozen:
        _set_frozen(cls)

    return cls


def _set_init(cls, fields, frozen):
    """Generate __init__ for the dataclass."""
    def __init__(self, *args, **kwargs):
        init_fields = [f for f in fields if f.init]
        # Process positional args
        for i, val in enumerate(args):
            if i < len(init_fields):
                if frozen:
                    object.__setattr__(self, init_fields[i].name, val)
                else:
                    setattr(self, init_fields[i].name, val)
        # Process keyword args
        for f in init_fields[len(args):]:
            if f.name in kwargs:
                if frozen:
                    object.__setattr__(self, f.name, kwargs[f.name])
                else:
                    setattr(self, f.name, kwargs[f.name])
            elif not isinstance(f.default, _MISSING_TYPE):
                if frozen:
                    object.__setattr__(self, f.name, f.default)
                else:
                    setattr(self, f.name, f.default)
            elif not isinstance(f.default_factory, _MISSING_TYPE):
                if frozen:
                    object.__setattr__(self, f.name, f.default_factory())
                else:
                    setattr(self, f.name, f.default_factory())
            else:
                raise TypeError("__init__() missing required argument: '%s'" % f.name)
        # Set non-init fields with defaults
        for f in fields:
            if not f.init:
                if not isinstance(f.default, _MISSING_TYPE):
                    if frozen:
                        object.__setattr__(self, f.name, f.default)
                    else:
                        setattr(self, f.name, f.default)
                elif not isinstance(f.default_factory, _MISSING_TYPE):
                    if frozen:
                        object.__setattr__(self, f.name, f.default_factory())
                    else:
                        setattr(self, f.name, f.default_factory())
        # Call __post_init__ if defined
        if hasattr(self, '__post_init__'):
            self.__post_init__()
    cls.__init__ = __init__


def _set_repr(cls, fields):
    """Generate __repr__ for the dataclass."""
    def __repr__(self):
        parts = []
        for f in fields:
            if f.repr:
                parts.append('%s=%r' % (f.name, getattr(self, f.name)))
        return '%s(%s)' % (type(self).__name__, ', '.join(parts))
    cls.__repr__ = __repr__


def _set_eq(cls, fields):
    """Generate __eq__ for the dataclass."""
    def __eq__(self, other):
        if type(other) is not type(self):
            return NotImplemented
        for f in fields:
            if f.compare:
                if getattr(self, f.name) != getattr(other, f.name):
                    return False
        return True
    cls.__eq__ = __eq__


def _set_order(cls, fields):
    """Generate comparison methods for the dataclass."""
    def _cmp_key(self):
        return tuple(getattr(self, f.name) for f in fields if f.compare)

    def __lt__(self, other):
        if type(other) is not type(self):
            return NotImplemented
        return _cmp_key(self) < _cmp_key(other)

    def __le__(self, other):
        if type(other) is not type(self):
            return NotImplemented
        return _cmp_key(self) <= _cmp_key(other)

    def __gt__(self, other):
        if type(other) is not type(self):
            return NotImplemented
        return _cmp_key(self) > _cmp_key(other)

    def __ge__(self, other):
        if type(other) is not type(self):
            return NotImplemented
        return _cmp_key(self) >= _cmp_key(other)

    cls.__lt__ = __lt__
    cls.__le__ = __le__
    cls.__gt__ = __gt__
    cls.__ge__ = __ge__


def _set_frozen(cls):
    """Make the dataclass frozen (immutable)."""
    def __setattr__(self, name, value):
        if hasattr(self, '__dataclass_fields__'):
            raise FrozenInstanceError('cannot assign to field %r' % name)
        object.__setattr__(self, name, value)

    def __delattr__(self, name):
        raise FrozenInstanceError('cannot delete field %r' % name)

    cls.__setattr__ = __setattr__
    cls.__delattr__ = __delattr__


def dataclass(cls=None, *, init=True, repr=True, eq=True, order=False,
              unsafe_hash=False, frozen=False):
    """Returns the same class as was passed in, with dunder methods added."""
    def wrap(cls):
        return _process_class(cls, init, repr, eq, order, unsafe_hash, frozen)
    if cls is None:
        return wrap
    return wrap(cls)


def fields(class_or_instance):
    """Return a tuple describing the fields of this dataclass."""
    try:
        field_dict = getattr(class_or_instance, '__dataclass_fields__')
    except AttributeError:
        raise TypeError('has no dataclass fields')
    return tuple(field_dict.values())


def asdict(obj, dict_factory=dict):
    """Return the fields of a dataclass instance as a dict."""
    if not hasattr(type(obj), '__dataclass_fields__'):
        raise TypeError('asdict() should be called on dataclass instances')
    result = []
    for f in fields(obj):
        value = getattr(obj, f.name)
        value = _asdict_inner(value, dict_factory)
        result.append((f.name, value))
    return dict_factory(result)


def _asdict_inner(obj, dict_factory):
    if hasattr(type(obj), '__dataclass_fields__'):
        return asdict(obj, dict_factory)
    elif isinstance(obj, tuple) and hasattr(obj, '_fields'):
        return type(obj)(*[_asdict_inner(v, dict_factory) for v in obj])
    elif isinstance(obj, (list, tuple)):
        return type(obj)(_asdict_inner(v, dict_factory) for v in obj)
    elif isinstance(obj, dict):
        return dict_factory((_asdict_inner(k, dict_factory),
                              _asdict_inner(v, dict_factory)) for k, v in obj.items())
    else:
        return obj


def astuple(obj, tuple_factory=tuple):
    """Return the fields of a dataclass instance as a tuple."""
    if not hasattr(type(obj), '__dataclass_fields__'):
        raise TypeError('astuple() should be called on dataclass instances')
    result = []
    for f in fields(obj):
        value = getattr(obj, f.name)
        result.append(_astuple_inner(value, tuple_factory))
    return tuple_factory(result)


def _astuple_inner(obj, tuple_factory):
    if hasattr(type(obj), '__dataclass_fields__'):
        return astuple(obj, tuple_factory)
    elif isinstance(obj, tuple) and hasattr(obj, '_fields'):
        return type(obj)(*[_astuple_inner(v, tuple_factory) for v in obj])
    elif isinstance(obj, (list, tuple)):
        return type(obj)(_astuple_inner(v, tuple_factory) for v in obj)
    elif isinstance(obj, dict):
        return type(obj)((_astuple_inner(k, tuple_factory),
                           _astuple_inner(v, tuple_factory)) for k, v in obj.items())
    else:
        return obj


def replace(obj, **changes):
    """Return a new object replacing specified fields with new values."""
    if not hasattr(type(obj), '__dataclass_fields__'):
        raise TypeError('replace() should be called on dataclass instances')
    result = {}
    for f in fields(obj):
        if f.name in changes:
            result[f.name] = changes[f.name]
        else:
            result[f.name] = getattr(obj, f.name)
    return type(obj)(**result)


def make_dataclass(cls_name, fields_list, *, bases=(), namespace=None,
                   init=True, repr=True, eq=True, order=False,
                   unsafe_hash=False, frozen=False):
    """Dynamically create a dataclass."""
    if namespace is None:
        namespace = {}
    annotations = {}
    for item in fields_list:
        if isinstance(item, str):
            annotations[item] = 'typing.Any'
        elif isinstance(item, tuple):
            if len(item) == 2:
                annotations[item[0]] = item[1]
            elif len(item) == 3:
                annotations[item[0]] = item[1]
                namespace[item[0]] = item[2]
    namespace['__annotations__'] = annotations
    cls = type(cls_name, bases, namespace)
    return dataclass(cls, init=init, repr=repr, eq=eq, order=order,
                     unsafe_hash=unsafe_hash, frozen=frozen)
