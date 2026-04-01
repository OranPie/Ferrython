# Phase 34: Property setter, __setitem__, custom iterators, descriptors, more
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── Property getter + setter ──
class Temperature:
    def __init__(self, celsius=0):
        self._celsius = celsius
    
    @property
    def celsius(self):
        return self._celsius
    
    @celsius.setter
    def celsius(self, value):
        if value < -273.15:
            raise ValueError("Temperature below absolute zero!")
        self._celsius = value
    
    @property
    def fahrenheit(self):
        return self._celsius * 9/5 + 32

t = Temperature(100)
test("property getter", t.celsius == 100)
test("property fahrenheit", t.fahrenheit == 212.0)
t.celsius = 0
test("property setter", t.celsius == 0)
test("property setter effect", t.fahrenheit == 32.0)

try:
    t.celsius = -300
    test("property setter validation", False)
except ValueError:
    test("property setter validation", True)

# ── __setitem__ / __getitem__ ──
class Matrix:
    def __init__(self, rows, cols):
        self.rows = rows
        self.cols = cols
        self.data = {}
    
    def __setitem__(self, key, value):
        self.data[key] = value
    
    def __getitem__(self, key):
        return self.data.get(key, 0)
    
    def __contains__(self, key):
        return key in self.data

m = Matrix(3, 3)
m["0_0"] = 1
m["1_1"] = 5
m["2_2"] = 9
test("__setitem__", m["0_0"] == 1)
test("__getitem__", m["1_1"] == 5)
test("__getitem__ default", m["0_1"] == 0)
test("__contains__", "2_2" in m)

# ── Custom iterator ──
class CountDown:
    def __init__(self, start):
        self.current = start
    
    def __iter__(self):
        return self
    
    def __next__(self):
        if self.current <= 0:
            raise StopIteration
        self.current -= 1
        return self.current + 1

result = list(CountDown(5))
test("custom iterator", result == [5, 4, 3, 2, 1])

# for loop with custom iterator
total = 0
for x in CountDown(3):
    total += x
test("custom iterator for loop", total == 6)

# ── Fibonacci iterator ──
class Fibonacci:
    def __init__(self, limit):
        self.limit = limit
        self.a = 0
        self.b = 1
    
    def __iter__(self):
        return self
    
    def __next__(self):
        if self.a > self.limit:
            raise StopIteration
        val = self.a
        self.a, self.b = self.b, self.a + self.b
        return val

fibs = list(Fibonacci(20))
test("fibonacci iterator", fibs == [0, 1, 1, 2, 3, 5, 8, 13])

# ── __repr__ and __str__ ──
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    
    def __repr__(self):
        return f"Point({self.x}, {self.y})"
    
    def __str__(self):
        return f"({self.x}, {self.y})"
    
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    
    def __add__(self, other):
        return Point(self.x + other.x, self.y + other.y)
    
    def __mul__(self, scalar):
        return Point(self.x * scalar, self.y * scalar)

p1 = Point(1, 2)
p2 = Point(3, 4)
test("__repr__", repr(p1) == "Point(1, 2)")
test("__str__", str(p1) == "(1, 2)")
test("__eq__", p1 == Point(1, 2))
test("__add__", p1 + p2 == Point(4, 6))
test("__mul__", p1 * 3 == Point(3, 6))

# ── __len__ and __bool__ ──
class Stack:
    def __init__(self):
        self.items = []
    
    def push(self, item):
        self.items.append(item)
    
    def pop(self):
        return self.items.pop()
    
    def __len__(self):
        return len(self.items)
    
    def __bool__(self):
        return len(self.items) > 0

s = Stack()
test("__bool__ empty", not s)
test("__len__ empty", len(s) == 0)
s.push(1)
s.push(2)
test("__bool__ nonempty", bool(s))
test("__len__ nonempty", len(s) == 2)
test("stack pop", s.pop() == 2)

# ── Multiple decorators ──
def bold(func):
    def wrapper(*args, **kwargs):
        return "<b>" + func(*args, **kwargs) + "</b>"
    return wrapper

def italic(func):
    def wrapper(*args, **kwargs):
        return "<i>" + func(*args, **kwargs) + "</i>"
    return wrapper

@bold
@italic
def greet(name):
    return f"Hello, {name}"

test("stacked decorators", greet("World") == "<b><i>Hello, World</i></b>")

# ── Class methods and static methods ──
class MyClass:
    count = 0
    
    def __init__(self, name):
        self.name = name
        MyClass.count += 1
    
    @classmethod
    def get_count(cls):
        return cls.count
    
    @staticmethod
    def validate(name):
        return len(name) > 0

a = MyClass("a")
b = MyClass("b")
test("classmethod", MyClass.get_count() == 2)
test("staticmethod", MyClass.validate("test"))
test("staticmethod false", not MyClass.validate(""))

# ── Exception chaining ──
try:
    try:
        raise ValueError("original")
    except ValueError as e:
        raise RuntimeError("wrapper") from e
except RuntimeError as e:
    test("exception caught", str(e) == "wrapper")

# ── Context manager ──
class Managed:
    def __init__(self):
        self.entered = False
        self.exited = False
    
    def __enter__(self):
        self.entered = True
        return self
    
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.exited = True
        return False

mgr = Managed()
with mgr as m:
    test("context manager enter", m.entered)
    test("context manager same obj", m is mgr)
test("context manager exit", mgr.exited)

# ── Generator expression in function calls ──
test("sum generator", sum(x*x for x in range(5)) == 30)
test("min generator", min(x*x for x in range(1, 5)) == 1)
test("max generator", max(x*x for x in range(1, 5)) == 16)

# ── String formatting ──
test("format spec", format(42, 'd') == '42')
test("f-string expr", f"{'hello':>10}" == '     hello')
test("f-string number", f"{3.14159:.2f}" == '3.14')

# ── Dict unpacking ──
d1 = {"a": 1, "b": 2}
d2 = {"c": 3, "d": 4}
merged = {**d1, **d2}
test("dict unpacking", merged == {"a": 1, "b": 2, "c": 3, "d": 4})

# ── List/tuple unpacking ──
first, *rest = [1, 2, 3, 4, 5]
test("star unpack first", first == 1)
test("star unpack rest", rest == [2, 3, 4, 5])

*init, last = [1, 2, 3, 4, 5]
test("star unpack last", last == 5)
test("star unpack init", init == [1, 2, 3, 4])

# ── Walrus operator ──
data = [1, 2, 3, 4, 5, 6, 7, 8]
filtered = [y for x in data if (y := x * 2) > 6]
test("walrus in comprehension", filtered == [8, 10, 12, 14, 16])

# ── Ternary expression ──
x = 10
result = "positive" if x > 0 else "non-positive"
test("ternary", result == "positive")

# ── Chained comparisons ──
x = 5
test("chained comparison", 1 < x < 10)
test("chained comparison false", not (1 < x < 3))

# ── Multiple assignment ──
a = b = c = 42
test("multiple assignment", a == b == c == 42)

# ── Augmented assignment ──
x = 10
x += 5
test("augmented +=", x == 15)
x -= 3
test("augmented -=", x == 12)
x *= 2
test("augmented *=", x == 24)
x //= 5
test("augmented //=", x == 4)
x **= 3
test("augmented **=", x == 64)

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
assert failed == 0, f"{failed} tests failed!"
print("ALL PHASE 34 TESTS PASSED")
