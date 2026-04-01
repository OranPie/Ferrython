# test_phase26.py — FrozenSet keys, import *, bytearray, bytes(), property setter, more edge cases

passed = 0
failed = 0

def assert_test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name)

# ── frozenset as dict key ──
d = {frozenset([1, 2]): "hello", frozenset([3, 4]): "world"}
assert_test("frozenset key lookup", d[frozenset([1, 2])] == "hello")
assert_test("frozenset key lookup 2", d[frozenset([3, 4])] == "world")

# frozenset in set
s = {frozenset([1, 2]), frozenset([3, 4])}
assert_test("frozenset in set", frozenset([1, 2]) in s)

# frozenset as tuple element
t = (frozenset([1]), "test")
assert_test("frozenset in tuple", t[0] == frozenset([1]))

# ── bytes() constructor ──
b1 = bytes(5)
assert_test("bytes int ctor", len(b1) == 5)

b2 = bytes([65, 66, 67])
assert_test("bytes list ctor", len(b2) == 3)

b3 = bytes(b"hello")
assert_test("bytes bytes ctor", len(b3) == 5)

b4 = bytes("hello", "utf-8")
assert_test("bytes str ctor", len(b4) == 5)

# ── bytearray() constructor ──
ba1 = bytearray(5)
assert_test("bytearray int ctor", len(ba1) == 5)

ba2 = bytearray([65, 66, 67])
assert_test("bytearray list ctor", len(ba2) == 3)

ba3 = bytearray(b"hello")
assert_test("bytearray bytes ctor", len(ba3) == 5)

# ── import * ──
# Test with os module
from os.path import *  # noqa — should import join, exists, etc.
# os.path is not a real module in ferrython, but we can test with math
from math import *
assert_test("import star math", sqrt(25) == 5.0)
assert_test("import star pi", abs(pi - 3.14159) < 0.001)

# ── property setter ──
class Temperature:
    def __init__(self):
        self._celsius = 0

    @property
    def celsius(self):
        return self._celsius

    @celsius.setter
    def celsius(self, value):
        self._celsius = value

    @property
    def fahrenheit(self):
        return self._celsius * 9 / 5 + 32

t = Temperature()
assert_test("property getter init", t.celsius == 0)
t.celsius = 100
assert_test("property setter", t.celsius == 100)
assert_test("property computed", t.fahrenheit == 212.0)

# Multiple property setters
class Box:
    def __init__(self, w, h):
        self._w = w
        self._h = h

    @property
    def width(self):
        return self._w

    @width.setter
    def width(self, val):
        if val < 0:
            self._w = 0
        else:
            self._w = val

    @property
    def height(self):
        return self._h

    @height.setter
    def height(self, val):
        if val < 0:
            self._h = 0
        else:
            self._h = val

    def area(self):
        return self._w * self._h

b = Box(10, 20)
assert_test("box area", b.area() == 200)
b.width = 5
assert_test("box set width", b.width == 5)
assert_test("box area after", b.area() == 100)
b.height = -5
assert_test("box neg height", b.height == 0)

# ── __hash__ custom ──
class Hashable:
    def __init__(self, x):
        self.x = x
    def __hash__(self):
        return self.x * 31
    def __eq__(self, other):
        return isinstance(other, Hashable) and self.x == other.x

assert_test("custom hash", hash(Hashable(3)) == 93)
assert_test("custom hash 2", hash(Hashable(0)) == 0)

# ── __bool__ custom ──
class NonEmpty:
    def __init__(self, items):
        self.items = items
    def __bool__(self):
        return len(self.items) > 0

assert_test("bool true", bool(NonEmpty([1])))
assert_test("bool false", not bool(NonEmpty([])))

# ── __contains__ custom ──
class Range10:
    def __contains__(self, x):
        return 0 <= x < 10

r10 = Range10()
assert_test("contains in", 5 in r10)
assert_test("contains not in", 15 not in r10)

# ── Slice assignment ──
x = [1, 2, 3, 4, 5]
x[1:3] = [20, 30, 40]
assert_test("slice assign expand", x == [1, 20, 30, 40, 4, 5])

y = [1, 2, 3, 4, 5]
y[1:4] = [99]
assert_test("slice assign shrink", y == [1, 99, 5])

z = [1, 2, 3]
z[0:0] = [10, 20]
assert_test("slice assign insert", z == [10, 20, 1, 2, 3])

# ── Multiple assignment ──
a = b = c = d = 42
assert_test("multi assign 4", a == 42 and b == 42 and c == 42 and d == 42)

# ── Chained comparisons ──
x = 5
assert_test("chained lt lt", 1 < x < 10)
assert_test("chained le le", 1 <= x <= 10)
assert_test("chained lt gt fail", not (10 < x < 1))
assert_test("chained eq", 5 == x == 5)

# ── String methods ──
assert_test("str capitalize", "hello world".capitalize() == "Hello world")
assert_test("str title", "hello world".title() == "Hello World")
assert_test("str swapcase", "Hello World".swapcase() == "hELLO wORLD")
assert_test("str center", "hi".center(10) == "    hi    ")
assert_test("str center fill", "hi".center(10, "*") == "****hi****")
assert_test("str ljust", "hi".ljust(10) == "hi        ")
assert_test("str rjust", "hi".rjust(10) == "        hi")
assert_test("str zfill", "42".zfill(5) == "00042")
assert_test("str zfill neg", "-42".zfill(6) == "-00042")
assert_test("str isdigit", "12345".isdigit())
assert_test("str isdigit false", not "12.3".isdigit())
assert_test("str isalpha", "hello".isalpha())
assert_test("str isalpha false", not "hello1".isalpha())
assert_test("str isalnum", "hello123".isalnum())
assert_test("str partition", "hello-world".partition("-") == ("hello", "-", "world"))
assert_test("str rpartition", "a-b-c".rpartition("-") == ("a-b", "-", "c"))
assert_test("str expandtabs", "a\tb".expandtabs(4) == "a   b")

# ── Dict methods ──
d = {"a": 1, "b": 2}
d.update({"c": 3})
assert_test("dict update", d == {"a": 1, "b": 2, "c": 3})

d2 = {"a": 1, "b": 2}
assert_test("dict setdefault exists", d2.setdefault("a", 99) == 1)
assert_test("dict setdefault new", d2.setdefault("c", 99) == 99)
assert_test("dict setdefault added", d2["c"] == 99)

d3 = {"a": 1, "b": 2, "c": 3}
item = d3.popitem()
assert_test("dict popitem", len(d3) == 2)

d4 = {"a": 1, "b": 2}
d4.clear()
assert_test("dict clear", len(d4) == 0)

# ── Set methods ──
s1 = {1, 2, 3}
s2 = {2, 3, 4}
assert_test("set union", s1 | s2 == {1, 2, 3, 4})
assert_test("set intersection", s1 & s2 == {2, 3})
assert_test("set difference", s1 - s2 == {1})
assert_test("set symmetric diff", s1 ^ s2 == {1, 4})
assert_test("set issubset", {1, 2}.issubset({1, 2, 3}))
assert_test("set issuperset", {1, 2, 3}.issuperset({1, 2}))

# ── enumerate with start ──
result = list(enumerate(["a", "b", "c"], 1))
assert_test("enumerate start 1", result == [(1, "a"), (2, "b"), (3, "c")])

result2 = list(enumerate(["x", "y"], 10))
assert_test("enumerate start 10", result2 == [(10, "x"), (11, "y")])

# ── List methods ──
lst = [3, 1, 4, 1, 5]
assert_test("list count", lst.count(1) == 2)
assert_test("list index", lst.index(4) == 2)

lst2 = [1, 2, 3]
lst2.insert(1, 99)
assert_test("list insert", lst2 == [1, 99, 2, 3])

lst3 = [1, 2, 3]
lst3.extend([4, 5])
assert_test("list extend", lst3 == [1, 2, 3, 4, 5])

lst4 = [1, 2, 3]
lst4 += [4, 5]
assert_test("list iadd", lst4 == [1, 2, 3, 4, 5])

lst5 = [1, 2, 3]
lst5 *= 2
assert_test("list imul", lst5 == [1, 2, 3, 1, 2, 3])

# ── Tuple unpacking advanced ──
a, *b, c = [1, 2, 3, 4, 5]
assert_test("star unpack", a == 1 and b == [2, 3, 4] and c == 5)

first, *rest = [10, 20, 30]
assert_test("star unpack head", first == 10 and rest == [20, 30])

*init, last = [10, 20, 30]
assert_test("star unpack tail", init == [10, 20] and last == 30)

# ── Dict comprehension ──
squares = {x: x**2 for x in range(5)}
assert_test("dict comp", squares == {0: 0, 1: 1, 2: 4, 3: 9, 4: 16})

# ── Set comprehension ──
evens = {x for x in range(10) if x % 2 == 0}
assert_test("set comp", evens == {0, 2, 4, 6, 8})

# ── Nested list comprehension ──
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat = [x for row in matrix for x in row]
assert_test("nested list comp", flat == [1, 2, 3, 4, 5, 6, 7, 8, 9])

# ── from import * test ──
from functools import *
assert_test("import star functools reduce", reduce(lambda a, b: a + b, [1, 2, 3]) == 6)

print()
print("=" * 40)
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(failed, "TESTS FAILED!")
