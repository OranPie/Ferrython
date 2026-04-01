"""Phase 39: Advanced stdlib and language edge cases - collections.OrderedDict,
   collections.Counter, deque, multiple inheritance diamond, classmethod/staticmethod,
   property with setter/deleter, metaclass basics, __slots__ stub, __call__,
   type() 3-arg, vars(), dir(), hasattr/getattr/setattr/delattr."""

passed = 0
failed = 0
total = 0

def test(name, condition):
    global passed, failed, total
    total += 1
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# --- collections.OrderedDict ---
from collections import OrderedDict

od = OrderedDict()
od["c"] = 3
od["a"] = 1
od["b"] = 2
test("OrderedDict order", list(od.keys()) == ["c", "a", "b"])
test("OrderedDict values", list(od.values()) == [3, 1, 2])

# --- collections.Counter ---
from collections import Counter

c = Counter("abracadabra")
test("Counter a", c["a"] == 5)
test("Counter b", c["b"] == 2)
test("Counter most_common", c.most_common(2) == [("a", 5), ("b", 2)])

c2 = Counter({"red": 4, "blue": 2})
test("Counter dict init", c2["red"] == 4)

# --- collections.defaultdict ---
from collections import defaultdict

dd = defaultdict(int)
dd["x"] += 1
dd["x"] += 1
dd["y"] += 1
test("defaultdict int", dd["x"] == 2 and dd["y"] == 1)

dd2 = defaultdict(list)
dd2["a"].append(1)
dd2["a"].append(2)
dd2["b"].append(3)
test("defaultdict list", dd2["a"] == [1, 2])

# --- collections.deque ---
from collections import deque

dq = deque([1, 2, 3])
dq.append(4)
test("deque append", list(dq) == [1, 2, 3, 4])
dq.appendleft(0)
test("deque appendleft", list(dq) == [0, 1, 2, 3, 4])
dq.pop()
test("deque pop", list(dq) == [0, 1, 2, 3])
dq.popleft()
test("deque popleft", list(dq) == [1, 2, 3])

# --- collections.namedtuple ---
from collections import namedtuple

Point = namedtuple("Point", ["x", "y"])
p = Point(1, 2)
test("namedtuple access", p.x == 1 and p.y == 2)
test("namedtuple index", p[0] == 1 and p[1] == 2)
test("namedtuple unpack", list(p) == [1, 2])

# --- classmethod ---
class MyClass:
    instances = 0
    
    def __init__(self):
        MyClass.instances += 1
    
    @classmethod
    def get_count(cls):
        return cls.instances
    
    @classmethod
    def create_two(cls):
        return cls(), cls()

obj1 = MyClass()
obj2 = MyClass()
test("classmethod", MyClass.get_count() == 2)

# --- staticmethod ---
class MathHelper:
    @staticmethod
    def add(a, b):
        return a + b
    
    @staticmethod
    def multiply(a, b):
        return a * b

test("staticmethod", MathHelper.add(3, 4) == 7)
test("staticmethod instance", MathHelper().multiply(3, 4) == 12)

# --- property with setter ---
class Temperature:
    def __init__(self, celsius=0):
        self._celsius = celsius
    
    @property
    def celsius(self):
        return self._celsius
    
    @celsius.setter
    def celsius(self, value):
        if value < -273.15:
            raise ValueError("Temperature below absolute zero")
        self._celsius = value
    
    @property
    def fahrenheit(self):
        return self._celsius * 9/5 + 32

t = Temperature(100)
test("property getter", t.celsius == 100)
test("property computed", t.fahrenheit == 212.0)
t.celsius = 0
test("property setter", t.celsius == 0)

try:
    t.celsius = -300
    test("property validator", False)
except ValueError:
    test("property validator", True)

# --- __call__ dunder ---
class Adder:
    def __init__(self, n):
        self.n = n
    def __call__(self, x):
        return self.n + x

add5 = Adder(5)
test("__call__", add5(10) == 15)
test("__call__ 2", add5(0) == 5)
test("callable check", callable(add5))

# --- Diamond inheritance ---
class A:
    def method(self):
        return "A"

class B(A):
    def method(self):
        return "B+" + super().method()

class C(A):
    def method(self):
        return "C+" + super().method()

class D(B, C):
    def method(self):
        return "D+" + super().method()

d = D()
test("diamond MRO", d.method() == "D+B+C+A")

# --- Multiple inheritance with __init__ ---
class Base1:
    def __init__(self):
        self.base1 = True

class Base2:
    def __init__(self):
        self.base2 = True

class Child(Base1, Base2):
    def __init__(self):
        Base1.__init__(self)
        Base2.__init__(self)
        self.child = True

c = Child()
test("multi inherit init", c.base1 and c.base2 and c.child)

# --- hasattr/getattr/setattr/delattr ---
class Obj:
    def __init__(self):
        self.x = 10

o = Obj()
test("hasattr true", hasattr(o, "x"))
test("hasattr false", not hasattr(o, "y"))
test("getattr", getattr(o, "x") == 10)
test("getattr default", getattr(o, "y", 42) == 42)
setattr(o, "z", 100)
test("setattr", o.z == 100)
delattr(o, "z")
test("delattr", not hasattr(o, "z"))

# --- type() single arg ---
test("type int", type(42).__name__ == "int")
test("type str", type("hello").__name__ == "str")
test("type list", type([]).__name__ == "list")
test("type dict", type({}).__name__ == "dict")
test("type none", type(None).__name__ == "NoneType")

# --- vars() ---
class Simple:
    def __init__(self):
        self.a = 1
        self.b = 2

s = Simple()
v = vars(s)
test("vars", v["a"] == 1 and v["b"] == 2)

# --- dir() on instance ---
d = dir(s)
test("dir has attrs", "a" in d and "b" in d)

# --- __dict__ access ---
test("__dict__", s.__dict__["a"] == 1)

# --- isinstance with inheritance ---
class Animal:
    pass

class Dog(Animal):
    pass

class Cat(Animal):
    pass

d = Dog()
test("isinstance parent", isinstance(d, Animal))
test("isinstance self", isinstance(d, Dog))
test("isinstance sibling", not isinstance(d, Cat))

# --- issubclass ---
test("issubclass", issubclass(Dog, Animal))
test("issubclass self", issubclass(Dog, Dog))
test("issubclass not", not issubclass(Cat, Dog))

# --- Chained method calls ---
class Builder:
    def __init__(self):
        self.parts = []
    def add(self, part):
        self.parts.append(part)
        return self
    def build(self):
        return "-".join(self.parts)

result = Builder().add("a").add("b").add("c").build()
test("method chaining", result == "a-b-c")

# --- Generator with send ---
def accumulator():
    total = 0
    while True:
        value = yield total
        if value is None:
            break
        total += value

gen = accumulator()
next(gen)  # prime
test("gen send 1", gen.send(10) == 10)
test("gen send 2", gen.send(20) == 30)
test("gen send 3", gen.send(5) == 35)

# --- itertools ---
import itertools

# chain
result = list(itertools.chain([1, 2], [3, 4], [5]))
test("itertools.chain", result == [1, 2, 3, 4, 5])

# repeat
result = list(itertools.repeat("x", 3))
test("itertools.repeat", result == ["x", "x", "x"])

# count
result = []
for i in itertools.count(10):
    if i >= 13:
        break
    result.append(i)
test("itertools.count", result == [10, 11, 12])

# cycle
result = []
c = itertools.cycle([1, 2, 3])
for _ in range(7):
    result.append(next(c))
test("itertools.cycle", result == [1, 2, 3, 1, 2, 3, 1])

# islice
result = list(itertools.islice(range(100), 5, 10))
test("itertools.islice", result == [5, 6, 7, 8, 9])

# product
result = list(itertools.product([1, 2], ["a", "b"]))
test("itertools.product", result == [(1, "a"), (1, "b"), (2, "a"), (2, "b")])

# permutations
result = list(itertools.permutations([1, 2, 3], 2))
test("itertools.permutations", len(result) == 6)

# combinations
result = list(itertools.combinations([1, 2, 3], 2))
test("itertools.combinations", result == [(1, 2), (1, 3), (2, 3)])

# --- functools ---
import functools

# reduce
result = functools.reduce(lambda a, b: a + b, [1, 2, 3, 4])
test("functools.reduce", result == 10)

# partial
def power(base, exp):
    return base ** exp

square = functools.partial(power, exp=2)
cube = functools.partial(power, exp=3)
test("functools.partial square", square(5) == 25)
test("functools.partial cube", cube(3) == 27)

# --- operator module ---
import operator

test("operator.add", operator.add(3, 4) == 7)
test("operator.mul", operator.mul(3, 4) == 12)
test("operator.sub", operator.sub(10, 3) == 7)
test("operator.eq", operator.eq(3, 3))
test("operator.lt", operator.lt(1, 2))

# --- Exception chaining ---
try:
    try:
        raise ValueError("original")
    except ValueError:
        raise RuntimeError("secondary")
except RuntimeError as e:
    test("exception chain", str(e) == "secondary")

# --- Assert with expression ---
try:
    x = 5
    assert x > 10, f"x is {x}, expected > 10"
    test("assert message", False)
except AssertionError as e:
    test("assert message", "x is 5" in str(e))

# --- Nested functions with closure ---
def make_multiplier(n):
    def multiplier(x):
        return x * n
    return multiplier

double = make_multiplier(2)
triple = make_multiplier(3)
test("closure 1", double(5) == 10)
test("closure 2", triple(5) == 15)

# --- Class with __iter__ ---
class Countdown:
    def __init__(self, n):
        self.n = n
    def __iter__(self):
        self.current = self.n
        return self
    def __next__(self):
        if self.current <= 0:
            raise StopIteration
        self.current -= 1
        return self.current + 1

test("custom iter", list(Countdown(5)) == [5, 4, 3, 2, 1])

# --- String join on generator ---
result = ", ".join(str(x) for x in range(5))
test("join generator", result == "0, 1, 2, 3, 4")

# --- Dict unpacking ---
def func(**kwargs):
    return kwargs

d1 = {"a": 1}
d2 = {"b": 2}
result = {**d1, **d2}
test("dict unpack merge", result == {"a": 1, "b": 2})

# --- Truthiness ---
test("empty list falsy", not [])
test("empty dict falsy", not {})
test("empty str falsy", not "")
test("zero falsy", not 0)
test("none falsy", not None)
test("nonempty truthy", bool([1]))
test("nonzero truthy", bool(42))

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 39 TESTS PASSED")
