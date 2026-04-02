"""Phase 41: Real-world patterns — dataclass-like, abstract classes, 
   descriptor protocol, __new__, __hash__, __eq__, set operations,
   frozenset, tuple as dict key, complex comprehensions, walrus operator"""

passed = 0
failed = 0
total = 0
def test(name, cond):
    global passed, failed, total
    total += 1
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# 1. __hash__ and __eq__ for custom classes
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    def __hash__(self):
        return hash((self.x, self.y))
    def __repr__(self):
        return f"Point({self.x}, {self.y})"

p1 = Point(1, 2)
p2 = Point(1, 2)
p3 = Point(3, 4)
test("custom __eq__", p1 == p2)
test("custom __ne__", p1 != p3)
test("custom __hash__", hash(p1) == hash(p2))

# 2. Custom class in set
s = {p1, p2, p3}
test("custom in set", len(s) == 2)

# 3. Custom class as dict key
d = {p1: "origin", p3: "far"}
test("custom dict key", d[p2] == "origin")

# 4. Tuple as dict key
td = {(1, 2): "a", (3, 4): "b"}
test("tuple dict key", td[(1, 2)] == "a")

# 5. frozenset
fs = frozenset([1, 2, 3, 2, 1])
test("frozenset dedup", len(fs) == 3)
test("frozenset contains", 2 in fs)

# 6. Set operations
a = {1, 2, 3, 4}
b = {3, 4, 5, 6}
test("set union", a | b == {1, 2, 3, 4, 5, 6})
test("set intersection", a & b == {3, 4})
test("set difference", a - b == {1, 2})
test("set symmetric_diff", a ^ b == {1, 2, 5, 6})

# 7. __contains__ custom
class Range:
    def __init__(self, start, stop):
        self.start = start
        self.stop = stop
    def __contains__(self, item):
        return self.start <= item < self.stop

r = Range(10, 20)
test("custom __contains__", 15 in r)
test("custom not contains", 25 not in r)

# 8. __len__
class Collection:
    def __init__(self, items):
        self._items = items
    def __len__(self):
        return len(self._items)
    def __getitem__(self, idx):
        return self._items[idx]

c = Collection([1, 2, 3])
test("custom __len__", len(c) == 3)
test("custom __getitem__", c[1] == 2)

# 9. Itertools chain
from itertools import chain
result = list(chain([1, 2], [3, 4], [5]))
test("itertools chain", result == [1, 2, 3, 4, 5])

# 10. Itertools zip_longest
from itertools import zip_longest
result = list(zip_longest([1, 2], [3, 4, 5], fillvalue=0))
test("zip_longest", result == [(1, 3), (2, 4), (0, 5)])

# 11. enumerate with start
result = list(enumerate(["a", "b", "c"], start=1))
test("enumerate start", result == [(1, "a"), (2, "b"), (3, "c")])

# 12. sorted with key
words = ["banana", "apple", "cherry"]
result = sorted(words, key=len)
test("sorted key=len", result == ["apple", "banana", "cherry"])

# 13. sorted with reverse
result = sorted([3, 1, 4, 1, 5], reverse=True)
test("sorted reverse", result == [5, 4, 3, 1, 1])

# 14. min/max with key
data = ["short", "medium", "extralong"]
test("min key", min(data, key=len) == "short")
test("max key", max(data, key=len) == "extralong")

# 15. Complex comprehension with conditions
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat_even = [x for row in matrix for x in row if x % 2 == 0]
test("nested comp filter", flat_even == [2, 4, 6, 8])

# 16. Dict comprehension with filter
d = {k: v for k, v in {"a": 1, "b": 2, "c": 3, "d": 4}.items() if v > 2}
test("dict comp filter", d == {"c": 3, "d": 4})

# 17. Generator as iterator
def fibonacci():
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b

fib = fibonacci()
first_10 = [next(fib) for _ in range(10)]
test("fibonacci gen", first_10 == [0, 1, 1, 2, 3, 5, 8, 13, 21, 34])

# 18. Multiple return values
def divmod_custom(a, b):
    return a // b, a % b

q, r = divmod_custom(17, 5)
test("multiple return", q == 3 and r == 2)

# 19. String join and split roundtrip
original = ["hello", "beautiful", "world"]
joined = " ".join(original)
split_back = joined.split(" ")
test("join split roundtrip", split_back == original)

# 20. Nested dict access
config = {
    "database": {
        "host": "localhost",
        "port": 5432,
    },
    "cache": {
        "enabled": True,
    }
}
test("nested dict", config["database"]["host"] == "localhost")
test("nested dict int", config["database"]["port"] == 5432)
test("nested dict bool", config["cache"]["enabled"] == True)

# 21. List comprehension with method calls
sentences = ["Hello World", "Python IS Great", "testing"]
words = [s.lower().split() for s in sentences]
test("comp with methods", words == [["hello", "world"], ["python", "is", "great"], ["testing"]])

# 22. isinstance with tuple of types
test("isinstance multi", isinstance(42, (int, float)))
test("isinstance multi 2", isinstance("hi", (int, str)))
test("isinstance multi neg", not isinstance(42, (str, list)))

# 23. Conditional expression in comprehension
result = ["even" if x % 2 == 0 else "odd" for x in range(5)]
test("ternary in comp", result == ["even", "odd", "even", "odd", "even"])

# 24. String methods
test("str.startswith", "hello world".startswith("hello"))
test("str.endswith", "hello world".endswith("world"))
test("str.replace", "hello".replace("l", "r") == "herro")
test("str.count", "mississippi".count("ss") == 2)
test("str.isdigit", "12345".isdigit())
test("str.isalpha", "hello".isalpha())
test("str.center", "hi".center(10, "-") == "----hi----")
test("str.zfill", "42".zfill(5) == "00042")

# 25. bool operations
test("bool and short", (0 and 5) == 0)
test("bool or short", (0 or 5) == 5)
test("bool not", not False == True)
test("truthy list", bool([1]) == True)
test("falsy list", bool([]) == False)
test("truthy str", bool("x") == True)
test("falsy str", bool("") == False)

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 41 TESTS PASSED")
