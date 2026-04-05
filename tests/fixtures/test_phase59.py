# Phase 59: Advanced OOP, descriptor protocol, metaclass, performance-critical patterns

# --- Decorator with arguments ---
def repeat(n):
    def decorator(func):
        def wrapper(*args, **kwargs):
            return [func(*args, **kwargs) for _ in range(n)]
        return wrapper
    return decorator

@repeat(3)
def greet(name):
    return f'Hi {name}'

assert greet("X") == ['Hi X', 'Hi X', 'Hi X'], "decorator args"

# --- Singleton class decorator ---
def singleton(cls):
    instances = {}
    def get(*a, **kw):
        if cls not in instances:
            instances[cls] = cls(*a, **kw)
        return instances[cls]
    return get

@singleton
class DB:
    def __init__(self):
        self.id = 42

assert DB() is DB(), "singleton"

# --- Property inheritance ---
class PropBase:
    def __init__(self):
        self._v = 10
    @property
    def v(self):
        return self._v
    @v.setter
    def v(self, x):
        self._v = x

class PropChild(PropBase):
    pass

pc = PropChild()
assert pc.v == 10
pc.v = 99
assert pc.v == 99

# --- Dunder add/eq ---
class Vec:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __add__(self, o):
        return Vec(self.x + o.x, self.y + o.y)
    def __eq__(self, o):
        return self.x == o.x and self.y == o.y
    def __repr__(self):
        return f'Vec({self.x},{self.y})'

assert Vec(1,2) + Vec(3,4) == Vec(4,6)

# --- __class_getitem__ ---
class TypedList:
    def __class_getitem__(cls, item):
        return f'{cls.__name__}[{item.__name__}]'

assert TypedList[int] == 'TypedList[int]'

# --- __init_subclass__ ---
class Registry:
    _reg = []
    def __init_subclass__(cls, **kw):
        super().__init_subclass__(**kw)
        Registry._reg.append(cls.__name__)

class PluginA(Registry): pass
class PluginB(Registry): pass
assert Registry._reg == ['PluginA', 'PluginB']

# --- Metaclass ---
class AutoRepr(type):
    def __new__(mcs, name, bases, ns):
        ns['auto_id'] = name.lower()
        return super().__new__(mcs, name, bases, ns)

class Widget(metaclass=AutoRepr):
    pass

assert Widget.auto_id == 'widget'

# --- __missing__ dict subclass ---
class CountDict(dict):
    def __missing__(self, key):
        self[key] = 0
        return 0

cd = CountDict()
cd['a'] += 1
cd['a'] += 1
cd['b'] += 5
assert cd == {'a': 2, 'b': 5}

# --- __getattr__ fallback ---
class Dynamic:
    def __init__(self):
        self.real = 'yes'
    def __getattr__(self, name):
        return f'dyn_{name}'

d = Dynamic()
assert d.real == 'yes'
assert d.missing == 'dyn_missing'

# --- Descriptor protocol with __set_name__ ---
class Bounded:
    def __init__(self, lo, hi):
        self.lo = lo
        self.hi = hi
    def __set_name__(self, owner, name):
        self.name = name
    def __get__(self, obj, objtype=None):
        if obj is None: return self
        return getattr(obj, f'_{self.name}', None)
    def __set__(self, obj, value):
        if not (self.lo <= value <= self.hi):
            raise ValueError(f'{self.name} out of range')
        setattr(obj, f'_{self.name}', value)

class Cfg:
    port = Bounded(1, 65535)

c = Cfg()
c.port = 8080
assert c.port == 8080
try:
    c.port = 0
    assert False, "should raise"
except ValueError:
    pass

# --- Generator with return value ---
def gen_ret():
    yield 1
    yield 2
    return 'end'

g = gen_ret()
assert next(g) == 1
assert next(g) == 2
try:
    next(g)
except StopIteration as e:
    assert e.args[0] == 'end'

# --- Complex comprehension ---
matrix = [[1,2,3],[4,5,6],[7,8,9]]
evens = [x for row in matrix for x in row if x % 2 == 0]
assert evens == [2, 4, 6, 8]

# --- Star unpacking ---
a, *b, c = range(5)
assert a == 0 and b == [1, 2, 3] and c == 4

# --- Chained comparisons ---
x = 5
assert 1 < x < 10
assert not (1 < x < 3)

# --- Dict merge operator ---
d1 = {'a': 1}
d2 = {'b': 2}
d3 = d1 | d2
assert d3 == {'a': 1, 'b': 2}

# --- Nested closures ---
def outer():
    x = 10
    def middle():
        y = 20
        def inner():
            return x + y
        return inner
    return middle

assert outer()()() == 30

# --- Arithmetic fast-path stress test ---
total = 0
for i in range(10000):
    total += i
assert total == 49995000

print("phase59: all tests passed")
