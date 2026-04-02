"""Test suite 24: Type system, introspection, advanced OOP"""
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── vars() ──
class Simple:
    def __init__(self, x, y):
        self.x = x
        self.y = y

s = Simple(1, 2)
v = vars(s)
test("vars keys", sorted(v.keys()) == ["x", "y"])
test("vars values", v["x"] == 1 and v["y"] == 2)

# ── __class__ attribute ──
test("int class", (42).__class__.__name__ == "int")
test("str class", "hello".__class__.__name__ == "str")
test("list class", [].__class__.__name__ == "list")

# ── isinstance/issubclass patterns ──
class Animal:
    pass
class Dog(Animal):
    pass
class Cat(Animal):
    pass

d = Dog()
test("isinstance sub", isinstance(d, Animal))
test("isinstance direct", isinstance(d, Dog))
test("not isinstance", not isinstance(d, Cat))
test("issubclass true", issubclass(Dog, Animal))
test("issubclass false", not issubclass(Dog, Cat))
test("issubclass self", issubclass(Dog, Dog))

# ── Property without setter ──
class ReadOnly:
    def __init__(self, val):
        self._val = val
    @property
    def val(self):
        return self._val

ro = ReadOnly(42)
test("readonly get", ro.val == 42)
try:
    ro.val = 100
    test("readonly set", False)
except AttributeError:
    test("readonly set", True)

# ── __repr__ for debugging ──
class Debug:
    def __init__(self, name, value):
        self.name = name
        self.value = value
    def __repr__(self):
        return f"Debug(name={self.name!r}, value={self.value!r})"

d = Debug("test", 42)
test("repr debug", repr(d) == "Debug(name='test', value=42)")

# ── __eq__ and __hash__ for sets/dicts ──
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __eq__(self, other):
        return isinstance(other, Point) and self.x == other.x and self.y == other.y
    def __hash__(self):
        return hash((self.x, self.y))
    def __repr__(self):
        return f"P({self.x},{self.y})"

points = {Point(1,2), Point(3,4), Point(1,2)}  # dup should be removed
test("point set dedup", len(points) == 2)
d = {Point(0,0): "origin", Point(1,1): "one"}
test("point dict", d[Point(0,0)] == "origin")

# ── Abstract method pattern ──
class Serializable:
    def serialize(self):
        raise NotImplementedError
    def __repr__(self):
        return f"{type(self).__name__}()"

class JsonObj(Serializable):
    def __init__(self, data):
        self.data = data
    def serialize(self):
        import json
        return json.dumps(self.data)

test("serialize", JsonObj({"x": 1}).serialize() == '{"x": 1}')

# ── Method chaining ──
class Builder:
    def __init__(self):
        self._items = []
    def add(self, item):
        self._items.append(item)
        return self
    def build(self):
        return self._items[:]

result = Builder().add(1).add(2).add(3).build()
test("method chain", result == [1, 2, 3])

# ── Operator overloading ──
class Vector:
    def __init__(self, *args):
        self.data = list(args)
    def __add__(self, other):
        return Vector(*[a + b for a, b in zip(self.data, other.data)])
    def __sub__(self, other):
        return Vector(*[a - b for a, b in zip(self.data, other.data)])
    def __mul__(self, scalar):
        return Vector(*[a * scalar for a in self.data])
    def __eq__(self, other):
        return self.data == other.data
    def __repr__(self):
        return f"Vector({', '.join(map(str, self.data))})"
    def __len__(self):
        return len(self.data)
    def dot(self, other):
        return sum(a * b for a, b in zip(self.data, other.data))

v1 = Vector(1, 2, 3)
v2 = Vector(4, 5, 6)
test("vec add", v1 + v2 == Vector(5, 7, 9))
test("vec sub", v2 - v1 == Vector(3, 3, 3))
test("vec mul", v1 * 2 == Vector(2, 4, 6))
test("vec dot", v1.dot(v2) == 32)
test("vec len", len(v1) == 3)

# ── Iteration protocol ──
class Range2D:
    def __init__(self, rows, cols):
        self.rows = rows
        self.cols = cols
    def __iter__(self):
        for r in range(self.rows):
            for c in range(self.cols):
                yield (r, c)

test("2d range", list(Range2D(2, 3)) == [(0,0),(0,1),(0,2),(1,0),(1,1),(1,2)])

# ── Comparison operators ──
class Temperature:
    def __init__(self, value):
        self.value = value
    def __lt__(self, other):
        return self.value < other.value
    def __eq__(self, other):
        return self.value == other.value
    def __repr__(self):
        return f"{self.value}°"

temps = [Temperature(30), Temperature(20), Temperature(25)]
test("custom sort", [t.value for t in sorted(temps)] == [20, 25, 30])

# ── Context manager that suppresses exceptions ──
class Suppress:
    def __init__(self, *exceptions):
        self.exceptions = exceptions
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        if exc_type is not None:
            for e in self.exceptions:
                if issubclass(exc_type, e):
                    return True
        return False

with Suppress(ZeroDivisionError):
    x = 1 / 0  # should not raise
test("suppress", True)

with Suppress(ValueError):
    try:
        x = 1 / 0  # ZeroDivisionError not suppressed
        test("suppress miss", False)
    except ZeroDivisionError:
        test("suppress miss", True)

# ── Recursive data ──
def tree_depth(tree):
    if not isinstance(tree, list):
        return 0
    if not tree:
        return 0
    return 1 + max(tree_depth(child) for child in tree)

test("tree depth", tree_depth([1, [2, [3, [4]]], [5]]) == 4)

# ── Closure with mutation ──
def make_accumulator():
    total = [0]  # mutable container for closure
    def add(x):
        total[0] += x
        return total[0]
    return add

acc = make_accumulator()
test("accum 1", acc(10) == 10)
test("accum 2", acc(20) == 30)
test("accum 3", acc(5) == 35)

# ── Generator with try/finally ──
def careful_gen(items):
    try:
        for item in items:
            yield item
    finally:
        pass  # cleanup

test("gen finally", list(careful_gen([1, 2, 3])) == [1, 2, 3])

# ── Multiple generators ──
def gen_a():
    yield 1
    yield 2

def gen_b():
    yield from gen_a()
    yield 3
    yield 4

test("yield from chain", list(gen_b()) == [1, 2, 3, 4])

# ── Dynamic class creation ──
MyClass = type("MyClass", (), {"x": 42, "greet": lambda self: "hi"})
obj = MyClass()
test("dynamic class", obj.x == 42)
test("dynamic method", obj.greet() == "hi")

# ── Class with __iter__ returning generator ──
class Words:
    def __init__(self, text):
        self.text = text
    def __iter__(self):
        for word in self.text.split():
            yield word

test("iter gen", list(Words("hello world foo")) == ["hello", "world", "foo"])

# ── Functional patterns ──
# compose
def compose(*fns):
    def composed(x):
        for f in reversed(fns):
            x = f(x)
        return x
    return composed

double = lambda x: x * 2
inc = lambda x: x + 1
test("compose", compose(double, inc)(5) == 12)  # inc(5)=6, double(6)=12

# ── Unpacking edge cases ──
a, = [42]  # single element unpack
test("single unpack", a == 42)

a, b = "xy"
test("string unpack", a == "x" and b == "y")

a, *b = "hello"
test("star string unpack", a == "h" and b == ["e", "l", "l", "o"])

# ── Integer operations ──
test("int abs", abs(-42) == 42)
test("divmod", divmod(17, 5) == (3, 2))
test("pow mod", pow(2, 10, 1000) == 24)

# ── Float operations ──
test("round", round(3.14159, 2) == 3.14)
test("round neg", round(2.5) == 2)  # banker's rounding: round half to even

# ── Boolean algebra ──
test("all empty", all([]))
test("any empty", not any([]))
test("bool list", [bool(x) for x in [0, "", [], {}, None, 1, "a", [1]]] == [False, False, False, False, False, True, True, True])

# ── Nested dict update ──
config = {"a": {"b": 1}}
config["a"]["c"] = 2
test("nested update", config == {"a": {"b": 1, "c": 2}})

# ── Error message quality ──
try:
    [][0]
except IndexError as e:
    test("index error msg", "index" in str(e).lower() or "range" in str(e).lower())

try:
    {}["missing"]
except KeyError as e:
    test("key error msg", "missing" in str(e))

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
