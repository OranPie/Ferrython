"""Pure Python implementation of the unittest.mock module.

Provides Mock and MagicMock for testing.
This complements the Rust-registered unittest.mock module with pure Python fallbacks.
"""

__all__ = [
    'Mock', 'MagicMock', 'patch', 'call', 'sentinel',
    'ANY', 'DEFAULT', 'create_autospec',
]


_missing = object()
_all_magics = frozenset([
    '__lt__', '__gt__', '__le__', '__ge__', '__eq__', '__ne__',
    '__add__', '__sub__', '__mul__', '__truediv__', '__floordiv__',
    '__mod__', '__pow__', '__and__', '__or__', '__xor__',
    '__lshift__', '__rshift__', '__neg__', '__pos__', '__abs__',
    '__invert__', '__contains__', '__len__', '__iter__', '__next__',
    '__getitem__', '__setitem__', '__delitem__',
    '__enter__', '__exit__', '__call__',
    '__str__', '__repr__', '__int__', '__float__', '__bool__',
    '__hash__',
])


class _ANY:
    """Object that compares equal to everything."""
    def __eq__(self, other):
        return True
    def __ne__(self, other):
        return False
    def __repr__(self):
        return 'ANY'

ANY = _ANY()


class _DEFAULT:
    def __repr__(self):
        return 'sentinel.DEFAULT'

DEFAULT = _DEFAULT()


class _SentinelObject:
    def __init__(self, name):
        self.name = name
    def __repr__(self):
        return 'sentinel.%s' % self.name


class _Sentinel:
    def __init__(self):
        self._sentinels = {}
    
    def __getattr__(self, name):
        if name not in self._sentinels:
            self._sentinels[name] = _SentinelObject(name)
        return self._sentinels[name]

sentinel = _Sentinel()


class _Call(tuple):
    """Represent a call to a mock."""
    
    def __new__(cls, value=(), name=None, parent=None, two=False, from_kall=True):
        args = ()
        kwargs = {}
        _len = len(value)
        if _len == 3:
            name, args, kwargs = value
        elif _len == 2:
            first, second = value
            if isinstance(first, str):
                name = first
                if isinstance(second, tuple):
                    args = second
                else:
                    kwargs = second
            else:
                args, kwargs = first, second
        elif _len == 1:
            value, = value
            if isinstance(value, str):
                name = value
            elif isinstance(value, tuple):
                args = value
            else:
                name = value
        
        obj = tuple.__new__(cls, (args, kwargs))
        obj._mock_name = name
        obj._mock_parent = parent
        obj._mock_from_kall = from_kall
        return obj
    
    def __repr__(self):
        if self._mock_name:
            return 'call.%s%s' % (self._mock_name, tuple.__repr__(self))
        return 'call%s' % (tuple.__repr__(self),)
    
    def __call__(self, *args, **kwargs):
        if self._mock_name is None:
            return _Call(('', args, kwargs), name='', parent=self,
                         from_kall=False)
        name = self._mock_name + '()'
        return _Call((name, args, kwargs), name=name, parent=self,
                     from_kall=False)
    
    def __getattr__(self, attr):
        return _Call(name=attr, parent=self, from_kall=False)

call = _Call(from_kall=False)


class Mock:
    """A flexible mock object."""
    
    def __init__(self, spec=None, wraps=None, name=None, spec_set=None,
                 side_effect=None, return_value=_missing, **kwargs):
        self._mock_name = name
        self._mock_children = {}
        self._mock_return_value = return_value
        self._mock_side_effect = side_effect
        self._mock_wraps = wraps
        self._spec_class = spec or spec_set
        self._spec_set = spec_set
        self.called = False
        self.call_count = 0
        self.call_args = None
        self.call_args_list = []
        self.method_calls = []
        self.mock_calls = []
        
        if kwargs:
            for key, value in kwargs.items():
                setattr(self, key, value)
    
    @property
    def return_value(self):
        if self._mock_return_value is _missing:
            self._mock_return_value = Mock()
        return self._mock_return_value
    
    @return_value.setter
    def return_value(self, value):
        self._mock_return_value = value
    
    @property
    def side_effect(self):
        return self._mock_side_effect
    
    @side_effect.setter
    def side_effect(self, value):
        self._mock_side_effect = value
    
    def __call__(self, *args, **kwargs):
        self.called = True
        self.call_count += 1
        self.call_args = _Call((args, kwargs))
        self.call_args_list.append(self.call_args)
        self.mock_calls.append(_Call(('', args, kwargs)))
        
        effect = self._mock_side_effect
        if effect is not None:
            if callable(effect):
                result = effect(*args, **kwargs)
                if result is not DEFAULT:
                    return result
            elif isinstance(effect, BaseException):
                raise effect
            elif isinstance(effect, type) and issubclass(effect, BaseException):
                raise effect()
            elif hasattr(effect, '__iter__'):
                try:
                    result = next(iter(effect))
                    return result
                except StopIteration:
                    pass
        
        if self._mock_wraps is not None:
            return self._mock_wraps(*args, **kwargs)
        
        return self.return_value
    
    def __getattr__(self, name):
        if name.startswith('_'):
            raise AttributeError(name)
        if name not in self._mock_children:
            self._mock_children[name] = Mock(name=name)
        return self._mock_children[name]
    
    def __repr__(self):
        name = self._mock_name or 'id=%d' % id(self)
        return "<Mock name='%s'>" % name
    
    def assert_called(self):
        if not self.called:
            raise AssertionError("Expected call not made")
    
    def assert_called_once(self):
        if self.call_count != 1:
            raise AssertionError(
                "Expected to be called once. Called %d times." % self.call_count)
    
    def assert_called_with(self, *args, **kwargs):
        if self.call_args is None:
            raise AssertionError("Expected call not made")
        expected = _Call((args, kwargs))
        if self.call_args != expected:
            raise AssertionError(
                "expected call not found.\nExpected: %s\nActual: %s" %
                (expected, self.call_args))
    
    def assert_called_once_with(self, *args, **kwargs):
        self.assert_called_once()
        self.assert_called_with(*args, **kwargs)
    
    def assert_any_call(self, *args, **kwargs):
        expected = _Call((args, kwargs))
        for call_item in self.call_args_list:
            if call_item == expected:
                return
        raise AssertionError(
            "%s call not found" % expected)
    
    def assert_not_called(self):
        if self.called:
            raise AssertionError(
                "Expected not to be called. Called %d times." % self.call_count)
    
    def reset_mock(self, visited=None, return_value=False, side_effect=False):
        self.called = False
        self.call_count = 0
        self.call_args = None
        self.call_args_list = []
        self.method_calls = []
        self.mock_calls = []
        if return_value:
            self._mock_return_value = _missing
        if side_effect:
            self._mock_side_effect = None
        for child in self._mock_children.values():
            if isinstance(child, Mock):
                child.reset_mock(visited)
    
    def configure_mock(self, **kwargs):
        for attr, value in kwargs.items():
            parts = attr.split('.')
            obj = self
            for part in parts[:-1]:
                obj = getattr(obj, part)
            setattr(obj, parts[-1], value)


class MagicMock(Mock):
    """A Mock with default implementations of magic methods."""
    
    def __init__(self, *args, **kwargs):
        Mock.__init__(self, *args, **kwargs)
    
    def __lt__(self, other):
        return NotImplemented
    def __gt__(self, other):
        return NotImplemented
    def __le__(self, other):
        return NotImplemented
    def __ge__(self, other):
        return NotImplemented
    def __int__(self):
        return 1
    def __float__(self):
        return 1.0
    def __len__(self):
        return 0
    def __contains__(self, item):
        return False
    def __iter__(self):
        return iter([])
    def __enter__(self):
        return self
    def __exit__(self, *args):
        return False
    def __bool__(self):
        return True


class _patch:
    """Helper for patch()."""
    
    def __init__(self, getter, attribute, new, spec, create,
                 spec_set, autospec, new_callable, kwargs):
        self.getter = getter
        self.attribute = attribute
        self.new = new
        self.spec = spec
        self.create = create
        self.spec_set = spec_set
        self.autospec = autospec
        self.new_callable = new_callable
        self.kwargs = kwargs
        self._original = _missing
    
    def __enter__(self):
        new = self.new
        if new is _missing:
            if self.new_callable is not None:
                new = self.new_callable(**self.kwargs)
            else:
                new = MagicMock(**self.kwargs)
        
        target = self.getter()
        self._original = getattr(target, self.attribute, _missing)
        setattr(target, self.attribute, new)
        return new
    
    def __exit__(self, *args):
        target = self.getter()
        if self._original is _missing:
            if hasattr(target, self.attribute):
                delattr(target, self.attribute)
        else:
            setattr(target, self.attribute, self._original)
        return False
    
    def __call__(self, func):
        if isinstance(func, type):
            return self.decorate_class(func)
        return self.decorate_callable(func)
    
    def decorate_callable(self, func):
        patcher = self
        def patched(*args, **keywargs):
            extra_args = []
            with patcher as new_attr:
                if new_attr is not _missing:
                    extra_args.append(new_attr)
                args += tuple(extra_args)
                return func(*args, **keywargs)
        patched.__name__ = func.__name__ if hasattr(func, '__name__') else str(func)
        return patched
    
    def decorate_class(self, klass):
        for attr in dir(klass):
            if attr.startswith('test'):
                method = getattr(klass, attr)
                if callable(method):
                    wrapped = self(method)
                    setattr(klass, attr, wrapped)
        return klass


def _get_target(target):
    """Parse 'module.Class.attr' into (getter, attribute)."""
    parts = target.rsplit('.', 1)
    if len(parts) != 2:
        raise TypeError("Need a valid target to patch. Got: %r" % target)
    module_path, attribute = parts
    
    def getter():
        import importlib
        return importlib.import_module(module_path)
    
    return getter, attribute


def patch(target, new=_missing, spec=None, create=False, spec_set=None,
          autospec=None, new_callable=None, **kwargs):
    """Decorator/context manager to patch the named target."""
    getter, attribute = _get_target(target)
    return _patch(getter, attribute, new, spec, create, spec_set,
                  autospec, new_callable, kwargs)


def patch_object(target, attribute, new=_missing, spec=None, create=False,
                 spec_set=None, autospec=None, new_callable=None, **kwargs):
    """Patch an attribute of an object."""
    getter = lambda: target
    return _patch(getter, attribute, new, spec, create, spec_set,
                  autospec, new_callable, kwargs)


patch.object = patch_object


def create_autospec(spec, spec_set=False, instance=False, _parent=None, **kwargs):
    """Create a mock object using another object as a spec."""
    return MagicMock(spec=spec, **kwargs)
