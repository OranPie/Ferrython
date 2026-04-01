# test_phase24.py — functools.partial, __repr__ fallback, more stdlib, patterns

passed = 0
failed = 0

def assert_test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name)

# ── functools.partial ──
from functools import partial

def add(a, b):
    return a + b

add5 = partial(add, 5)
assert_test("partial basic", add5(3) == 8)
assert_test("partial basic 2", add5(10) == 15)

def power(base, exp):
    return base ** exp

square = partial(power, 2)
assert_test("partial power", square(10) == 1024)

# Partial with multiple args
def make_greeting(greeting, name, punctuation):
    return greeting + " " + name + punctuation

hello = partial(make_greeting, "Hello")
assert_test("partial multi", hello("World", "!") == "Hello World!")

friendly = partial(make_greeting, "Hi", "Friend")
assert_test("partial multi 2", friendly("!") == "Hi Friend!")

# Partial with builtin
from functools import reduce
assert_test("partial with reduce", reduce(add, [1, 2, 3, 4]) == 10)

# Partial of partial
add10 = partial(add5, 5)  # add(5, 5) = 10
assert_test("partial of partial", add10() == 10)

# ── __repr__ fallback in str() / print() ──
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __repr__(self):
        return f"Point({self.x}, {self.y})"

p = Point(3, 4)
assert_test("repr fallback str", str(p) == "Point(3, 4)")

# __str__ overrides __repr__ for str()
class Named:
    def __init__(self, name):
        self.name = name
    def __repr__(self):
        return f"Named({self.name!r})"
    def __str__(self):
        return self.name

n = Named("Alice")
assert_test("str overrides repr", str(n) == "Alice")
assert_test("repr still works", repr(n) == "Named('Alice')")

# ── functools.wraps (stub) ──
from functools import wraps
def my_decorator(f):
    @wraps(f)
    def wrapper(*args, **kwargs):
        return f(*args, **kwargs)
    return wrapper

@my_decorator
def greet(name):
    return "hello " + name

assert_test("wraps decorator", greet("world") == "hello world")

# ── functools.total_ordering (stub) ──
from functools import total_ordering

@total_ordering
class Num:
    def __init__(self, val):
        self.val = val
    def __eq__(self, other):
        return self.val == other.val
    def __lt__(self, other):
        return self.val < other.val

assert_test("total_ordering", Num(1) < Num(2))

# ── operator module more tests ──
import operator

# Chain operators
result = operator.add(operator.mul(3, 4), 5)
assert_test("operator chain", result == 17)

# Negative floordiv
assert_test("operator floordiv neg", operator.floordiv(-7, 2) == -4)

# ── copy with class instances ──
import copy

class Container:
    def __init__(self, items):
        self.items = items

c1 = Container([1, 2, 3])
c2 = copy.copy(c1)
# Shallow copy — same items list reference (for Instance, copy returns clone)
assert_test("copy instance", c2 is not c1)

# ── dict.fromkeys advanced ──
keys = "hello"
d = dict.fromkeys(keys, 0)
assert_test("fromkeys from string", len(d) == 4)  # h, e, l, o (l deduplicated)
assert_test("fromkeys dedup", d["l"] == 0)

# ── str.translate edge cases ──
# Translate with int keys directly
table = {ord('a'): ord('A'), ord('e'): ord('E')}
result = "apple".translate(table)
assert_test("translate dict ordinals", result == "ApplE")

# Translate preserving non-mapped chars
table2 = str.maketrans("x", "y")
assert_test("translate no change", "hello".translate(table2) == "hello")

# ── Type dynamic creation ──
MyClass = type("MyClass", (), {"x": 42})
obj = MyClass()
assert_test("type() 3-arg", obj.x == 42)
assert_test("type() name", type(obj).__name__ == "MyClass")

# ── Context managers ──
class TrackingCM:
    def __init__(self):
        self.entered = False
        self.exited = False
    def __enter__(self):
        self.entered = True
        return self
    def __exit__(self, *args):
        self.exited = True
        return False

cm = TrackingCM()
with cm:
    assert_test("cm entered", cm.entered == True)
    assert_test("cm not exited yet", cm.exited == False)
assert_test("cm exited", cm.exited == True)

# ── Multiple inheritance with MRO ──
class A:
    def who(self):
        return "A"

class B(A):
    pass

class C(A):
    def who(self):
        return "C"

class D(B, C):
    pass

d = D()
assert_test("MRO diamond", d.who() == "C")

# ── Decorators ──
def double_result(f):
    def wrapper(*args):
        return f(*args) * 2
    return wrapper

@double_result
def add_nums(a, b):
    return a + b

assert_test("decorator", add_nums(3, 4) == 14)

# ── Global keyword ──
counter = 0
def increment():
    global counter
    counter += 1

increment()
increment()
increment()
assert_test("global keyword", counter == 3)

# ── Chained comparisons ──
assert_test("chained lt", 1 < 2 < 3)
assert_test("chained mixed", 1 < 2 > 0)
assert_test("chained eq", 1 <= 1 <= 2)

# ── Tuple unpacking in for ──
data = [(1, "a"), (2, "b"), (3, "c")]
result = []
for num, letter in data:
    result.append(str(num) + letter)
assert_test("tuple unpack for", result == ["1a", "2b", "3c"])

# ── Star unpacking in function calls ──
def triple(a, b, c):
    return a + b + c

args = [10, 20, 30]
assert_test("star unpack call", triple(*args) == 60)

# ── Dict unpacking ──
d1 = {"a": 1, "b": 2}
d2 = {"c": 3, "d": 4}
merged = {**d1, **d2}
assert_test("dict unpack merge", merged == {"a": 1, "b": 2, "c": 3, "d": 4})

# ── Generator expressions ──
gen_sum = sum(x**2 for x in range(5))
assert_test("generator expr sum", gen_sum == 30)

gen_list = list(x * 2 for x in range(4))
assert_test("generator expr list", gen_list == [0, 2, 4, 6])

# ── Set comprehension ──
s = {x % 3 for x in range(10)}
assert_test("set comprehension", s == {0, 1, 2})

# ── Dict comprehension ──
dc = {k: v for k, v in enumerate("abc")}
assert_test("dict comprehension", dc == {0: "a", 1: "b", 2: "c"})

# ── Nested comprehension ──
matrix = [[1, 2], [3, 4], [5, 6]]
flat = [x for row in matrix for x in row]
assert_test("nested comprehension", flat == [1, 2, 3, 4, 5, 6])

# ── List comprehension with condition ──
evens = [x for x in range(10) if x % 2 == 0]
assert_test("list comp filter", evens == [0, 2, 4, 6, 8])

# ── Multiple assignment ──
a = b = c = 99
assert_test("multiple assign", a == 99 and b == 99 and c == 99)

# ── Augmented assignment ──
x = [1, 2]
x += [3, 4]
assert_test("augmented list", x == [1, 2, 3, 4])

y = "hello"
y += " world"
assert_test("augmented str", y == "hello world")

# ── Ternary expression ──
val = "even" if 4 % 2 == 0 else "odd"
assert_test("ternary", val == "even")
val2 = "even" if 3 % 2 == 0 else "odd"
assert_test("ternary 2", val2 == "odd")

# ── Walrus operator simulation ── (if supported)
# items = [1, 2, 3, 4, 5]
# result = [y := x * 2 for x in items]  # walrus in comprehension

# ── String multiplication ──
assert_test("str mul", "ab" * 3 == "ababab")
assert_test("str mul 0", "ab" * 0 == "")
assert_test("list mul", [1, 2] * 3 == [1, 2, 1, 2, 1, 2])

# ── isinstance with tuple ──
assert_test("isinstance tuple", isinstance(42, (str, int, float)))
assert_test("isinstance tuple false", not isinstance(42, (str, list)))

# ── hasattr / getattr / setattr ──
class Obj:
    x = 10

o = Obj()
assert_test("hasattr", hasattr(o, "x"))
assert_test("not hasattr", not hasattr(o, "y"))
assert_test("getattr", getattr(o, "x") == 10)
assert_test("getattr default", getattr(o, "y", 42) == 42)
setattr(o, "z", 100)
assert_test("setattr", o.z == 100)

# ── staticmethod ──
class Math:
    @staticmethod
    def add(a, b):
        return a + b

assert_test("staticmethod", Math.add(3, 4) == 7)

# ── classmethod ──
class Counter2:
    count = 0
    @classmethod
    def inc(cls):
        cls.count += 1
        return cls.count

assert_test("classmethod 1", Counter2.inc() == 1)
assert_test("classmethod 2", Counter2.inc() == 2)

# ── property ──
class Circle:
    def __init__(self, radius):
        self._radius = radius
    
    @property
    def radius(self):
        return self._radius
    
    @property
    def area(self):
        return 3.14159 * self._radius ** 2

c = Circle(5)
assert_test("property get", c.radius == 5)
assert_test("property computed", abs(c.area - 78.53975) < 0.001)

print()
print("=" * 40)
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(failed, "TESTS FAILED!")
