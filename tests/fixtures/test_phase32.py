# Phase 32: Language feature completeness tests
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── Custom __str__ and __repr__ ──
class Foo:
    def __init__(self, x):
        self.x = x
    def __str__(self):
        return f"Foo({self.x})"
    def __repr__(self):
        return f"Foo(x={self.x!r})"

f = Foo(42)
test("custom __str__", str(f) == "Foo(42)")
test("custom __repr__", repr(f) == "Foo(x=42)")

# ── Custom __len__ and __bool__ ──
class MyList:
    def __init__(self, items):
        self.items = items
    def __len__(self):
        return len(self.items)
    def __bool__(self):
        return len(self.items) > 0

ml = MyList([1, 2, 3])
test("custom __len__", len(ml) == 3)
test("custom __bool__ true", bool(ml) == True)

ml_empty = MyList([])
test("custom __bool__ false", bool(ml_empty) == False)

# ── Custom __contains__ ──
class Bag:
    def __init__(self, items):
        self.items = items
    def __contains__(self, item):
        return item in self.items

b = Bag([1, 2, 3])
test("custom __contains__ true", 2 in b)
test("custom __contains__ false", 5 not in b)

# ── Custom __add__ and __mul__ ──
class Vec:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __add__(self, other):
        return Vec(self.x + other.x, self.y + other.y)
    def __mul__(self, scalar):
        return Vec(self.x * scalar, self.y * scalar)
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    def __repr__(self):
        return f"Vec({self.x}, {self.y})"

v1 = Vec(1, 2)
v2 = Vec(3, 4)
v3 = v1 + v2
test("custom __add__", v3.x == 4 and v3.y == 6)

v4 = v1 * 3
test("custom __mul__", v4.x == 3 and v4.y == 6)

test("custom __eq__ true", Vec(1, 2) == Vec(1, 2))
test("custom __eq__ false", not (Vec(1, 2) == Vec(3, 4)))

# ── Custom __lt__, __le__, __gt__, __ge__ ──
class Rating:
    def __init__(self, value):
        self.value = value
    def __lt__(self, other):
        return self.value < other.value
    def __le__(self, other):
        return self.value <= other.value
    def __gt__(self, other):
        return self.value > other.value
    def __ge__(self, other):
        return self.value >= other.value

test("custom __lt__", Rating(3) < Rating(5))
test("custom __gt__", Rating(5) > Rating(3))
test("custom __le__", Rating(3) <= Rating(3))
test("custom __ge__", Rating(5) >= Rating(3))

# ── Custom __hash__ ──
class Key:
    def __init__(self, v):
        self.v = v
    def __hash__(self):
        return hash(self.v)

test("custom __hash__", hash(Key(42)) == hash(42))

# ── Custom __getitem__ ──
class Matrix:
    def __init__(self, data):
        self.data = data
    def __getitem__(self, key):
        return self.data[key]

m = Matrix([10, 20, 30])
test("custom __getitem__", m[0] == 10 and m[2] == 30)

# ── Multiple inheritance ──
class A:
    def greet(self):
        return "A"

class B(A):
    def greet(self):
        return "B"

class C(A):
    def greet(self):
        return "C"

class D(B, C):
    pass

d = D()
test("MRO method resolution", d.greet() == "B")

# ── super() ──
class Base:
    def __init__(self):
        self.base_init = True
    def method(self):
        return "base"

class Child(Base):
    def __init__(self):
        super().__init__()
        self.child_init = True
    def method(self):
        return "child+" + super().method()

c = Child()
test("super().__init__", c.base_init == True and c.child_init == True)
test("super().method()", c.method() == "child+base")

# ── Class with __init__ and defaults ──
class Config:
    def __init__(self, name, value=10):
        self.name = name
        self.value = value

cfg1 = Config("test")
test("init default", cfg1.name == "test" and cfg1.value == 10)
cfg2 = Config("test", 42)
test("init override", cfg2.name == "test" and cfg2.value == 42)

# ── Static and class methods ──
class Math:
    @staticmethod
    def add(x, y):
        return x + y
    
    @classmethod
    def name(cls):
        return "Math"

test("staticmethod", Math.add(3, 4) == 7)
test("classmethod", Math.name() == "Math")

# ── Property ──
class Circle:
    def __init__(self, radius):
        self._radius = radius
    
    @property
    def radius(self):
        return self._radius
    
    @property
    def area(self):
        return 3.14159 * self._radius ** 2

circle = Circle(5)
test("property getter radius", circle.radius == 5)
test("property getter area", abs(circle.area - 78.53975) < 0.001)

# ── Nested functions / closures ──
def make_counter():
    count = [0]
    def increment():
        count[0] += 1
        return count[0]
    return increment

counter = make_counter()
test("closure 1", counter() == 1)
test("closure 2", counter() == 2)
test("closure 3", counter() == 3)

# ── Generator expressions ──
gen = sum(x*x for x in range(5))
test("generator expr sum", gen == 30)  # 0+1+4+9+16

# ── dict/list/set comprehensions ──
squares = {x: x*x for x in range(5)}
test("dict comprehension", squares == {0: 0, 1: 1, 2: 4, 3: 9, 4: 16})

unique = {x % 3 for x in range(10)}
test("set comprehension", unique == {0, 1, 2})

flat = [x for sub in [[1,2],[3,4],[5]] for x in sub]
test("nested list comp", flat == [1, 2, 3, 4, 5])

# ── Ternary / conditional expression ──
test("ternary true", ("yes" if True else "no") == "yes")
test("ternary false", ("yes" if False else "no") == "no")

# ── Walrus operator ──
data = [1, 2, 3, 4, 5]
result = [y for x in data if (y := x * 2) > 4]
test("walrus operator", result == [6, 8, 10])

# ── Unpacking ──
a, *b, c = [1, 2, 3, 4, 5]
test("star unpacking", a == 1 and b == [2, 3, 4] and c == 5)

# ── Multiple assignment ──
x = y = z = 10
test("multi assign", x == 10 and y == 10 and z == 10)

# ── Chained comparison ──
test("chained comparison", 1 < 2 < 3 < 4)
test("chained comparison false", not (1 < 2 > 3))

# ── String methods ──
test("str.upper", "hello".upper() == "HELLO")
test("str.lower", "HELLO".lower() == "hello")
test("str.strip", "  hello  ".strip() == "hello")
test("str.split default", "a b c".split() == ['a', 'b', 'c'])
test("str.startswith", "hello".startswith("hel"))
test("str.endswith", "hello".endswith("llo"))
test("str.replace", "hello world".replace("world", "python") == "hello python")
test("str.count", "banana".count("a") == 3)
test("str.find", "hello".find("ll") == 2)
test("str.isdigit", "123".isdigit())
test("str.isalpha", "abc".isalpha())
test("str.title", "hello world".title() == "Hello World")
test("str.capitalize", "hello world".capitalize() == "Hello world")
test("str.center", "hi".center(6) == "  hi  ")
test("str.zfill", "42".zfill(5) == "00042")

# ── List methods ──
lst = [3, 1, 4, 1, 5]
lst.sort()
test("list.sort", lst == [1, 1, 3, 4, 5])

lst2 = [3, 1, 4]
lst2.reverse()
test("list.reverse", lst2 == [4, 1, 3])

test("list.index", [10, 20, 30].index(20) == 1)
test("list.count", [1, 2, 1, 3, 1].count(1) == 3)

# ── Dict comprehension from zip ──
keys = ['a', 'b', 'c']
vals = [1, 2, 3]
d = dict(zip(keys, vals))
test("dict from zip", d == {'a': 1, 'b': 2, 'c': 3})

# ── Exception handling ──
try:
    1 / 0
except ZeroDivisionError as e:
    test("catch ZeroDivisionError", True)

try:
    raise ValueError("test error")
except ValueError as e:
    test("raise and catch ValueError", str(e) == "test error")

# ── try/except/else/finally ──
result = []
try:
    result.append("try")
except:
    result.append("except")
else:
    result.append("else")
finally:
    result.append("finally")
test("try/else/finally", result == ["try", "else", "finally"])

# ── With statement ──
class CM:
    def __enter__(self):
        return "resource"
    def __exit__(self, *args):
        pass

with CM() as r:
    test("with statement", r == "resource")

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
assert failed == 0, f"{failed} tests failed!"
print("ALL PHASE 32 TESTS PASSED")
