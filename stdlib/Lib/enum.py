"""Enumeration classes - pure Python layer complementing the Rust enum module."""


class EnumMeta(type):
    """Metaclass for Enum."""

    def __new__(mcls, cls_name, bases, classdict):
        # Collect enum members
        enum_members = {}
        new_classdict = {}
        for key, value in classdict.items():
            if key.startswith('_') or callable(value):
                new_classdict[key] = value
            else:
                enum_members[key] = value

        cls = super().__new__(mcls, cls_name, bases, new_classdict)
        cls._member_map_ = {}
        cls._value2member_map_ = {}

        for name, value in enum_members.items():
            member = object.__new__(cls)
            member._name_ = name
            member._value_ = value
            member.name = name
            member.value = value
            setattr(cls, name, member)
            cls._member_map_[name] = member
            cls._value2member_map_[value] = member

        return cls

    def __iter__(cls):
        return iter(cls._member_map_.values())

    def __len__(cls):
        return len(cls._member_map_)

    def __contains__(cls, member):
        if isinstance(member, cls):
            return member._name_ in cls._member_map_
        return False

    def __getitem__(cls, name):
        return cls._member_map_[name]

    def __call__(cls, value):
        if value in cls._value2member_map_:
            return cls._value2member_map_[value]
        raise ValueError('%r is not a valid %s' % (value, cls.__name__))


class Enum(metaclass=EnumMeta):
    """Generic enumeration base class."""

    def __repr__(self):
        return '<%s.%s: %r>' % (type(self).__name__, self._name_, self._value_)

    def __str__(self):
        return '%s.%s' % (type(self).__name__, self._name_)

    def __eq__(self, other):
        if type(self) is type(other):
            return self._value_ == other._value_
        return NotImplemented

    def __ne__(self, other):
        result = self.__eq__(other)
        if result is NotImplemented:
            return result
        return not result

    def __hash__(self):
        return hash(self._value_)


class IntEnum(int, Enum):
    """Enum where members are also (and must be) ints."""
    pass


class Flag(Enum):
    """Support for bit flags."""

    def __or__(self, other):
        if not isinstance(other, type(self)):
            return NotImplemented
        result = object.__new__(type(self))
        result._value_ = self._value_ | other._value_
        result._name_ = '%s|%s' % (self._name_, other._name_)
        result.name = result._name_
        result.value = result._value_
        return result

    def __and__(self, other):
        if not isinstance(other, type(self)):
            return NotImplemented
        result = object.__new__(type(self))
        result._value_ = self._value_ & other._value_
        result._name_ = '%s&%s' % (self._name_, other._name_)
        result.name = result._name_
        result.value = result._value_
        return result

    def __invert__(self):
        result = object.__new__(type(self))
        result._value_ = ~self._value_
        result._name_ = '~%s' % self._name_
        result.name = result._name_
        result.value = result._value_
        return result

    def __contains__(self, other):
        if not isinstance(other, type(self)):
            return NotImplemented
        return (self._value_ & other._value_) == other._value_

    def __bool__(self):
        return bool(self._value_)


class IntFlag(int, Flag):
    """Support for integer bit flags."""
    pass


def unique(enumeration):
    """Class decorator that ensures only one name is bound to any one value."""
    duplicates = []
    for name, member in enumeration._member_map_.items():
        if member.name != name:
            duplicates.append((name, member.name))
    if duplicates:
        alias_details = ', '.join(
            '%s -> %s' % (alias, name) for alias, name in duplicates)
        raise ValueError('duplicate values found in %r: %s' %
                         (enumeration, alias_details))
    return enumeration


def auto():
    """Generate the next value when not given."""
    # Simplified — returns a sentinel that EnumMeta should handle
    if not hasattr(auto, '_counter'):
        auto._counter = 0
    auto._counter += 1
    return auto._counter
