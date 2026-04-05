"""Abstract Base Classes (ABCs) - pure Python layer."""


class abstractmethod:
    """A decorator indicating abstract methods."""

    def __init__(self, funcobj):
        funcobj.__isabstractmethod__ = True
        self._func = funcobj

    def __call__(self, *args, **kwargs):
        return self._func(*args, **kwargs)

    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return self._func.__get__(obj, objtype)


class abstractclassmethod(classmethod):
    """A decorator indicating abstract classmethods."""
    __isabstractmethod__ = True

    def __init__(self, callable):
        callable.__isabstractmethod__ = True
        super().__init__(callable)


class abstractstaticmethod(staticmethod):
    """A decorator indicating abstract staticmethods."""
    __isabstractmethod__ = True

    def __init__(self, callable):
        callable.__isabstractmethod__ = True
        super().__init__(callable)


class abstractproperty(property):
    """A decorator indicating abstract properties."""
    __isabstractmethod__ = True


class ABCMeta(type):
    """Metaclass for defining Abstract Base Classes (ABCs).

    Use this metaclass to create ABCs. An ABC can be subclassed directly,
    and then acts as a mix-in class.
    """

    _abc_registry = {}
    _abc_cache = {}
    _abc_negative_cache = {}

    def __new__(mcls, name, bases, namespace, **kwargs):
        cls = super().__new__(mcls, name, bases, namespace)
        # Find abstract methods
        abstracts = set()
        for attr_name, value in namespace.items():
            if getattr(value, '__isabstractmethod__', False):
                abstracts.add(attr_name)
        # Inherit abstract methods from bases
        for base in bases:
            for attr_name in getattr(base, '__abstractmethods__', set()):
                value = getattr(cls, attr_name, None)
                if getattr(value, '__isabstractmethod__', False):
                    abstracts.add(attr_name)
        cls.__abstractmethods__ = frozenset(abstracts)
        return cls

    def register(cls, subclass):
        """Register a virtual subclass of an ABC."""
        if not isinstance(cls, ABCMeta):
            raise TypeError("Can only register classes")
        if subclass in getattr(cls, '_abc_registry', {}):
            return subclass
        if not hasattr(cls, '_abc_registry'):
            cls._abc_registry = {}
        cls._abc_registry[subclass] = True
        return subclass

    def __instancecheck__(cls, instance):
        """Override for isinstance(instance, cls)."""
        subclass = type(instance)
        if subclass in getattr(cls, '_abc_cache', {}):
            return True
        subtype = type.__instancecheck__(cls, instance)
        if subtype:
            return True
        # Check registry
        for registered in getattr(cls, '_abc_registry', {}):
            if issubclass(subclass, registered):
                if not hasattr(cls, '_abc_cache'):
                    cls._abc_cache = {}
                cls._abc_cache[subclass] = True
                return True
        return False

    def __subclasscheck__(cls, subclass):
        """Override for issubclass(subclass, cls)."""
        if subclass in getattr(cls, '_abc_cache', {}):
            return True
        result = type.__subclasscheck__(cls, subclass)
        if result:
            return True
        for registered in getattr(cls, '_abc_registry', {}):
            if issubclass(subclass, registered):
                if not hasattr(cls, '_abc_cache'):
                    cls._abc_cache = {}
                cls._abc_cache[subclass] = True
                return True
        return False


class ABC(metaclass=ABCMeta):
    """Helper class that provides a standard way to create an ABC using
    inheritance, if desired.

    A useful mixin class, but not always needed. A class may already
    be based on ABCMeta, and in that case using ABC would just add noise.
    """
    __slots__ = ()
