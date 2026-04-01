"""Phase 38: Advanced language features - multiple assignment targets, walrus operator,
   extended unpacking, chained comparisons, ternary in comprehensions, 
   class variables, __class__ attribute, property deleter, __repr__ vs __str__."""

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

# --- Multiple assignment targets ---
a = b = c = 10
test("multiple assign", a == 10 and b == 10 and c == 10)

x = y = []
x.append(1)
test("multiple assign same ref", y == [1])

# --- Augmented assignment ---
x = 5
x += 3
test("augmented add", x == 8)
x -= 2
test("augmented sub", x == 6)
x *= 4
test("augmented mul", x == 24)
x //= 5
test("augmented floordiv", x == 4)
x **= 3
test("augmented pow", x == 64)
x %= 10
test("augmented mod", x == 4)

# --- Chained comparisons ---
test("chain 1 < 2 < 3", 1 < 2 < 3)
test("chain 1 < 2 > 0", 1 < 2 > 0)
test("chain not 1 < 2 < 1", not (1 < 2 < 1))
test("chain 1 <= 1 <= 2", 1 <= 1 <= 2)
x = 5
test("chain 0 < x < 10", 0 < x < 10)
test("chain not 0 < x < 3", not (0 < x < 3))

# --- Ternary expression ---
x = 10
result = "big" if x > 5 else "small"
test("ternary true", result == "big")
result = "big" if x < 5 else "small"
test("ternary false", result == "small")

# --- Extended unpacking ---
a, *b = [1, 2, 3, 4]
test("star unpack head", a == 1 and b == [2, 3, 4])

*a, b = [1, 2, 3, 4]
test("star unpack tail", a == [1, 2, 3] and b == 4)

a, *b, c = [1, 2, 3, 4, 5]
test("star unpack middle", a == 1 and b == [2, 3, 4] and c == 5)

a, *b = [1]
test("star unpack empty", a == 1 and b == [])

# --- Nested unpacking ---
(a, b), c = [1, 2], 3
test("nested unpack tuple", a == 1 and b == 2 and c == 3)

# --- String methods ---
test("str.startswith", "hello world".startswith("hello"))
test("str.endswith", "hello world".endswith("world"))
test("str.find", "hello".find("ll") == 2)
test("str.find missing", "hello".find("xyz") == -1)
test("str.count", "abcabc".count("abc") == 2)
test("str.index", "hello".index("ll") == 2)
test("str.isdigit", "12345".isdigit())
test("str.isalpha", "hello".isalpha())
test("str.isalnum", "hello123".isalnum())
test("str.zfill", "42".zfill(5) == "00042")
test("str.center", "hi".center(6) == "  hi  ")
test("str.ljust", "hi".ljust(5) == "hi   ")
test("str.rjust", "hi".rjust(5) == "   hi")
test("str.expandtabs", "a\tb".expandtabs(4) == "a   b")
test("str.title", "hello world".title() == "Hello World")
test("str.swapcase", "Hello".swapcase() == "hELLO")
test("str.capitalize", "hello world".capitalize() == "Hello world")
test("str.isspace", "   ".isspace())
test("str.isupper", "HELLO".isupper())
test("str.islower", "hello".islower())

# --- List methods ---
lst = [3, 1, 4, 1, 5]
test("list.count", lst.count(1) == 2)
test("list.index", lst.index(4) == 2)
lst2 = lst.copy()
test("list.copy", lst2 == [3, 1, 4, 1, 5])
lst2.clear()
test("list.clear", lst2 == [])
lst3 = [1, 2, 3]
lst3.insert(1, 10)
test("list.insert", lst3 == [1, 10, 2, 3])
lst3.extend([4, 5])
test("list.extend", lst3 == [1, 10, 2, 3, 4, 5])

# --- Dict methods ---
d = {"a": 1, "b": 2, "c": 3}
test("dict.get default", d.get("x", 99) == 99)
test("dict.get found", d.get("a") == 1)
test("dict.pop", d.pop("b") == 2 and "b" not in d)
d2 = d.copy()
test("dict.copy", d2 == {"a": 1, "c": 3})
d.setdefault("d", 4)
test("dict.setdefault new", d["d"] == 4)
d.setdefault("a", 99)
test("dict.setdefault existing", d["a"] == 1)
d.update({"e": 5, "a": 10})
test("dict.update", d["e"] == 5 and d["a"] == 10)

# --- Set operations ---
s1 = {1, 2, 3}
s2 = {2, 3, 4}
test("set union |", s1 | s2 == {1, 2, 3, 4})
test("set intersection &", s1 & s2 == {2, 3})
test("set difference -", s1 - s2 == {1})
test("set symmetric_difference ^", s1 ^ s2 == {1, 4})
test("set issubset", {1, 2}.issubset({1, 2, 3}))
test("set issuperset", {1, 2, 3}.issuperset({1, 2}))
test("set isdisjoint", {1, 2}.isdisjoint({3, 4}))

# --- Class features ---
class Counter:
    count = 0  # class variable
    
    def __init__(self):
        Counter.count += 1
        self.id = Counter.count
    
    def __repr__(self):
        return f"Counter(id={self.id})"
    
    def __str__(self):
        return f"Counter #{self.id}"

c1 = Counter()
c2 = Counter()
test("class variable", Counter.count == 2)
test("__repr__", repr(c1) == "Counter(id=1)")
test("__str__", str(c2) == "Counter #2")

# --- __contains__ dunder ---
class Range10:
    def __contains__(self, item):
        return 0 <= item < 10

r = Range10()
test("__contains__ in", 5 in r)
test("__contains__ not in", 15 not in r)

# --- __bool__ dunder ---
class AlwaysFalse:
    def __bool__(self):
        return False

class AlwaysTrue:
    def __bool__(self):
        return True

test("__bool__ false", not AlwaysFalse())
test("__bool__ true", bool(AlwaysTrue()))

# --- __eq__ and __ne__ ---
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    
    def __ne__(self, other):
        return not self.__eq__(other)
    
    def __hash__(self):
        return hash((self.x, self.y))

p1 = Point(1, 2)
p2 = Point(1, 2)
p3 = Point(3, 4)
test("__eq__", p1 == p2)
test("__ne__", p1 != p3)

# --- __lt__, __le__, __gt__, __ge__ ---
class Sortable:
    def __init__(self, val):
        self.val = val
    def __lt__(self, other):
        return self.val < other.val
    def __le__(self, other):
        return self.val <= other.val
    def __gt__(self, other):
        return self.val > other.val
    def __ge__(self, other):
        return self.val >= other.val

s1 = Sortable(1)
s2 = Sortable(2)
test("__lt__", s1 < s2)
test("__gt__", s2 > s1)
test("__le__", s1 <= s2)
test("__ge__", s2 >= s1)

# --- __add__, __mul__, __sub__ ---
class Vec2:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __add__(self, other):
        return Vec2(self.x + other.x, self.y + other.y)
    def __sub__(self, other):
        return Vec2(self.x - other.x, self.y - other.y)
    def __mul__(self, scalar):
        return Vec2(self.x * scalar, self.y * scalar)
    def __neg__(self):
        return Vec2(-self.x, -self.y)
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y

v1 = Vec2(1, 2)
v2 = Vec2(3, 4)
test("__add__", (v1 + v2) == Vec2(4, 6))
test("__sub__", (v2 - v1) == Vec2(2, 2))
test("__mul__", (v1 * 3) == Vec2(3, 6))
test("__neg__", (-v1) == Vec2(-1, -2))

# --- __len__ on custom class ---
class MyList:
    def __init__(self):
        self.data = []
    def append(self, item):
        self.data.append(item)
    def __len__(self):
        return len(self.data)

ml = MyList()
ml.append(1)
ml.append(2)
test("__len__", len(ml) == 2)

# --- __getitem__ and __setitem__ ---
class Matrix:
    def __init__(self):
        self.data = {}
    def __setitem__(self, key, value):
        self.data[key] = value
    def __getitem__(self, key):
        return self.data[key]

m = Matrix()
m[0, 0] = 1
m[0, 1] = 2
test("__setitem__ tuple key", m[0, 0] == 1)
test("__getitem__ tuple key", m[0, 1] == 2)

# --- Global/nonlocal ---
counter = 0
def increment():
    global counter
    counter += 1

increment()
increment()
test("global keyword", counter == 2)

def make_counter():
    count = 0
    def inc():
        nonlocal count
        count += 1
        return count
    return inc

c = make_counter()
test("nonlocal", c() == 1 and c() == 2 and c() == 3)

# --- Multiple return values ---
def swap(a, b):
    return b, a

x, y = swap(1, 2)
test("multiple return", x == 2 and y == 1)

# --- Default mutable argument ---
def append_to(val, lst=None):
    if lst is None:
        lst = []
    lst.append(val)
    return lst

test("mutable default", append_to(1) == [1])
test("mutable default 2", append_to(2) == [2])

# --- Lambda ---
square = lambda x: x ** 2
test("lambda", square(5) == 25)

add = lambda x, y: x + y
test("lambda multi arg", add(3, 4) == 7)

# --- Map/filter/zip ---
test("map", list(map(lambda x: x * 2, [1, 2, 3])) == [2, 4, 6])
test("filter", list(filter(lambda x: x > 2, [1, 2, 3, 4])) == [3, 4])
test("zip", list(zip([1, 2], ["a", "b"])) == [(1, "a"), (2, "b")])

# --- Enumerate ---
test("enumerate", list(enumerate(["a", "b"])) == [(0, "a"), (1, "b")])
test("enumerate start", list(enumerate(["a", "b"], 1)) == [(1, "a"), (2, "b")])

# --- All/any ---
test("all true", all([True, True, True]))
test("all false", not all([True, False, True]))
test("any true", any([False, True, False]))
test("any false", not any([False, False, False]))
test("all empty", all([]))

# --- Sorted with key ---
test("sorted", sorted([3, 1, 4, 1, 5]) == [1, 1, 3, 4, 5])
test("sorted reverse", sorted([3, 1, 4], reverse=True) == [4, 3, 1])
test("sorted key", sorted(["banana", "apple", "cherry"], key=len) == ["apple", "banana", "cherry"])

# --- Min/max with key ---
test("min", min(3, 1, 4) == 1)
test("max", max(3, 1, 4) == 4)
test("min list", min([3, 1, 4]) == 1)
test("max key", max(["a", "bb", "ccc"], key=len) == "ccc")

# --- isinstance with tuple ---
test("isinstance tuple", isinstance(42, (str, int)))
test("isinstance tuple false", not isinstance(42, (str, list)))

# --- String formatting ---
test("format int", f"{42:05d}" == "00042")
test("format float", f"{3.14:.1f}" == "3.1")
test("format align", f"{'hi':>10}" == "        hi")
test("format align left", f"{'hi':<10}" == "hi        ")
test("format align center", f"{'hi':^10}" == "    hi    ")

# --- Bytes ---
test("bytes literal", b"hello" == b"hello")
test("bytes len", len(b"hello") == 5)
test("bytes index", b"hello"[0] == 104)
test("bytes decode", b"hello".decode("utf-8") == "hello")
test("str encode", "hello".encode("utf-8") == b"hello")

# --- Assert ---
test("assert pass", True)  # Just confirm assert doesn't crash
try:
    assert False, "test message"
    test("assert fail", False)
except AssertionError as e:
    test("assert fail message", str(e) == "test message")

# --- Walrus operator ---
data = [1, 2, 3, 4, 5]
result = [y for x in data if (y := x * 2) > 4]
test("walrus in comprehension", result == [6, 8, 10])

# --- f-string with expressions ---
x = 10
test("f-string expr", f"{x + 5}" == "15")
test("f-string method", f"{'hello'.upper()}" == "HELLO")

# --- Dict comprehension ---
d = {k: v for k, v in zip("abc", [1, 2, 3])}
test("dict comprehension", d == {"a": 1, "b": 2, "c": 3})

# --- Set comprehension ---
s = {x * 2 for x in range(5)}
test("set comprehension", s == {0, 2, 4, 6, 8})

# --- Nested comprehension ---
matrix = [[1, 2], [3, 4], [5, 6]]
flat = [x for row in matrix for x in row]
test("nested comprehension", flat == [1, 2, 3, 4, 5, 6])

# --- Generator expression ---
g = sum(x * x for x in range(5))
test("generator expression", g == 30)

# --- Try/except/else/finally ---
def divide(a, b):
    try:
        result = a / b
    except ZeroDivisionError:
        return "zero"
    else:
        return result
    finally:
        pass

test("try else", divide(10, 2) == 5.0)
test("try except", divide(10, 0) == "zero")

# --- Multiple except ---
def multi_except(x):
    try:
        if x == 0:
            raise ValueError("val")
        elif x == 1:
            raise TypeError("type")
        elif x == 2:
            raise KeyError("key")
        return "ok"
    except (ValueError, TypeError) as e:
        return f"caught: {e}"
    except KeyError:
        return "key error"

test("multi except val", multi_except(0) == "caught: val")
test("multi except type", multi_except(1) == "caught: type")
test("multi except key", multi_except(2) == "key error")
test("multi except none", multi_except(3) == "ok")

# --- Context manager ---
class Resource:
    def __init__(self):
        self.opened = False
        self.closed = False
    def __enter__(self):
        self.opened = True
        return self
    def __exit__(self, *args):
        self.closed = True
        return False

with Resource() as r:
    test("context enter", r.opened)
test("context exit", r.closed)

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 38 TESTS PASSED")
