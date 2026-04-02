"""Test suite 26: Hashable instances, exception args, more opcodes, __del__"""
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── Custom hashable objects in sets/dicts ──
class Card:
    def __init__(self, rank, suit):
        self.rank = rank
        self.suit = suit
    def __eq__(self, other):
        return isinstance(other, Card) and self.rank == other.rank and self.suit == other.suit
    def __hash__(self):
        return hash((self.rank, self.suit))
    def __repr__(self):
        return f"{self.rank}{self.suit}"

hand = {Card("A", "♠"), Card("K", "♥"), Card("A", "♠")}
test("card set", len(hand) == 2)

deck_dict = {Card("A", "♠"): 11, Card("K", "♥"): 10}
test("card dict get", deck_dict[Card("A", "♠")] == 11)

# ── Exception args ──
try:
    raise ValueError("bad value", 42)
except ValueError as e:
    test("exc args", e.args == ("bad value", 42))
    test("exc str", "bad value" in str(e))

try:
    raise TypeError("wrong type")
except TypeError as e:
    test("type exc args", e.args == ("wrong type",))

# ── Exception chaining ──
try:
    try:
        1 / 0
    except ZeroDivisionError as e:
        raise ValueError("calc failed") from e
except ValueError as e:
    test("exc chain", str(e) == "calc failed")
    test("exc cause", e.__cause__ is not None)

# ── Nested exception handling ──
result = []
try:
    result.append("outer try")
    try:
        result.append("inner try")
        raise ValueError("inner")
    except ValueError:
        result.append("inner except")
        raise TypeError("rethrow")
except TypeError:
    result.append("outer except")
test("nested exc", result == ["outer try", "inner try", "inner except", "outer except"])

# ── Multiple except clauses ──
def classify_error(exc):
    try:
        raise exc
    except ValueError:
        return "value"
    except TypeError:
        return "type"
    except KeyError:
        return "key"
    except Exception:
        return "other"

test("classify value", classify_error(ValueError("x")) == "value")
test("classify type", classify_error(TypeError("x")) == "type")
test("classify key", classify_error(KeyError("x")) == "key")
test("classify other", classify_error(RuntimeError("x")) == "other")

# ── try/except/else/finally ──
log = []
try:
    log.append("try")
except Exception:
    log.append("except")
else:
    log.append("else")
finally:
    log.append("finally")
test("try else finally", log == ["try", "else", "finally"])

log2 = []
try:
    log2.append("try")
    raise ValueError("x")
except ValueError:
    log2.append("except")
finally:
    log2.append("finally")
test("try except finally", log2 == ["try", "except", "finally"])

# ── Global/nonlocal ──
x = 10
def modify_global():
    global x
    x = 20
modify_global()
test("global", x == 20)

def outer():
    y = 1
    def inner():
        nonlocal y
        y = 2
    inner()
    return y
test("nonlocal", outer() == 2)

# ── Walrus operator-like pattern (assignment in condition - simulated) ──
data = [1, 2, 3, 4, 5]
filtered = [x for x in data if x > 2]
test("filter comp", filtered == [3, 4, 5])

# ── Chained comparisons ──
test("chain lt", 1 < 2 < 3 < 4)
test("chain le", 1 <= 1 <= 2 <= 3)
test("chain mixed", 0 < 1 <= 1 < 2)
test("chain false", not (1 < 2 > 3))

# ── String operations ──
test("str join list", ", ".join(["a", "b", "c"]) == "a, b, c")
test("str split max", "a.b.c.d".split(".", 2) == ["a", "b", "c.d"])
test("str replace count", "aaa".replace("a", "b", 2) == "bba")
test("str zfill", "42".zfill(5) == "00042")
test("str center", "hi".center(10) == "    hi    ")
test("str ljust", "hi".ljust(5) == "hi   ")
test("str rjust", "hi".rjust(5) == "   hi")
test("str count", "hello".count("l") == 2)
test("str expandtabs", "a\tb".expandtabs(4) == "a   b")

# ── List operations ──
lst = [3, 1, 4, 1, 5, 9]
test("list count", lst.count(1) == 2)
test("list index", lst.index(4) == 2)
lst2 = lst[:]
lst2.sort()
test("list sort", lst2 == [1, 1, 3, 4, 5, 9])
lst3 = lst[:]
lst3.reverse()
test("list reverse", lst3 == [9, 5, 1, 4, 1, 3])
lst4 = [1, 2]
lst4.extend([3, 4])
test("list extend", lst4 == [1, 2, 3, 4])
lst5 = [1, 2, 3]
lst5.insert(1, 10)
test("list insert", lst5 == [1, 10, 2, 3])
lst6 = [1, 2, 3, 2]
lst6.remove(2)
test("list remove", lst6 == [1, 3, 2])

# ── Dict operations ──
d = {"a": 1, "b": 2, "c": 3}
test("dict pop", d.pop("b") == 2 and "b" not in d)
test("dict get default", d.get("z", 99) == 99)
d2 = {"x": 1}
d2.update({"y": 2, "z": 3})
test("dict update", d2 == {"x": 1, "y": 2, "z": 3})

# ── Set operations ──
s1 = {1, 2, 3, 4}
s2 = {3, 4, 5, 6}
test("set union", s1 | s2 == {1, 2, 3, 4, 5, 6})
test("set intersection", s1 & s2 == {3, 4})
test("set difference", s1 - s2 == {1, 2})
test("set symmetric diff", s1 ^ s2 == {1, 2, 5, 6})
test("set issubset", {1, 2}.issubset({1, 2, 3}))
test("set issuperset", {1, 2, 3}.issuperset({1, 2}))

# ── Augmented assignment ──
x = 10
x += 5
test("iadd", x == 15)
x -= 3
test("isub", x == 12)
x *= 2
test("imul", x == 24)
x //= 5
test("ifloordiv", x == 4)
x **= 3
test("ipow", x == 64)
x %= 10
test("imod", x == 4)

# ── Ternary operator ──
test("ternary true", "yes" if True else "no")
test("ternary false", ("yes" if False else "no") == "no")
test("ternary expr", (1 if 5 > 3 else 0) == 1)

# ── Nested comprehensions ──
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat = [x for row in matrix for x in row]
test("nested comp", flat == [1, 2, 3, 4, 5, 6, 7, 8, 9])

transpose = [[row[i] for row in matrix] for i in range(3)]
test("transpose", transpose == [[1, 4, 7], [2, 5, 8], [3, 6, 9]])

# ── Multiple inheritance ──
class A:
    def method(self):
        return "A"

class B(A):
    def method(self):
        return "B" + super().method()

class C(A):
    def method(self):
        return "C" + super().method()

class D(B, C):
    def method(self):
        return "D" + super().method()

test("mro method", D().method() == "DBCA")

# ── Property with setter ──
class Circle:
    def __init__(self, radius):
        self._radius = radius
    
    @property
    def radius(self):
        return self._radius
    
    @radius.setter
    def radius(self, value):
        if value < 0:
            raise ValueError("radius must be non-negative")
        self._radius = value
    
    @property
    def area(self):
        return 3.14159 * self._radius ** 2

c = Circle(5)
test("circle r", c.radius == 5)
test("circle area", abs(c.area - 78.53975) < 0.001)
c.radius = 10
test("circle set r", c.radius == 10)

try:
    c.radius = -1
    test("circle neg", False)
except ValueError:
    test("circle neg", True)

# ── __contains__ ──
class Interval:
    def __init__(self, lo, hi):
        self.lo = lo
        self.hi = hi
    def __contains__(self, item):
        return self.lo <= item <= self.hi

iv = Interval(1, 10)
test("contains yes", 5 in iv)
test("contains no", 11 not in iv)

# ── __bool__ ──
class Bag:
    def __init__(self, items):
        self.items = items
    def __bool__(self):
        return len(self.items) > 0
    def __len__(self):
        return len(self.items)

test("bool true", bool(Bag([1, 2])))
test("bool false", not bool(Bag([])))

# ── map, filter, zip patterns ──
test("map sq", list(map(lambda x: x**2, [1, 2, 3])) == [1, 4, 9])
test("filter even", list(filter(lambda x: x % 2 == 0, range(10))) == [0, 2, 4, 6, 8])
test("zip dict", dict(zip(["a", "b"], [1, 2])) == {"a": 1, "b": 2})

# ── Generator expressions ──
gen_sum = sum(x**2 for x in range(5))
test("genexpr sum", gen_sum == 30)

# ── Callable class ──
class Adder:
    def __init__(self, n):
        self.n = n
    def __call__(self, x):
        return self.n + x

add10 = Adder(10)
test("callable", add10(5) == 15)
test("is callable", callable(add10))

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
