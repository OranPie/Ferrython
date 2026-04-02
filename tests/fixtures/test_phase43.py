"""Phase 43: Real-world stdlib usage — json, re, functools, collections,
   string formatting edge cases, exception handling patterns, closures,
   nested functions, recursive algorithms"""

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

# 1. JSON round-trip
import json

data = {"name": "Alice", "age": 30, "scores": [95, 87, 92]}
json_str = json.dumps(data)
parsed = json.loads(json_str)
test("json round-trip name", parsed["name"] == "Alice")
test("json round-trip age", parsed["age"] == 30)
test("json round-trip list", parsed["scores"] == [95, 87, 92])

# 2. JSON with nested dicts
nested = {"config": {"debug": True, "level": 3}, "tags": ["a", "b"]}
s = json.dumps(nested)
p = json.loads(s)
test("json nested", p["config"]["debug"] == True)
test("json nested list", p["tags"] == ["a", "b"])

# 3. re module
import re

m = re.search(r"\d+", "abc123def")
test("re search", m is not None)
test("re group", m.group(0) == "123")

m2 = re.match(r"hello", "hello world")
test("re match", m2 is not None)

m3 = re.match(r"world", "hello world")
test("re match fail", m3 is None)

parts = re.split(r"\s+", "hello   world   python")
test("re split", parts == ["hello", "world", "python"])

replaced = re.sub(r"\d+", "NUM", "abc123def456")
test("re sub", replaced == "abcNUMdefNUM")

all_nums = re.findall(r"\d+", "abc12def34ghi56")
test("re findall", all_nums == ["12", "34", "56"])

# 4. Collections usage
from collections import OrderedDict, Counter, defaultdict

# Counter operations
c = Counter("abracadabra")
test("counter most_common", c.most_common(1) == [("a", 5)])
test("counter element", c["b"] == 2)
test("counter missing", c["z"] == 0)

# defaultdict
dd = defaultdict(list)
for word in ["apple", "banana", "avocado", "blueberry", "cherry"]:
    dd[word[0]].append(word)
test("defaultdict", sorted(dd["a"]) == ["apple", "avocado"])
test("defaultdict b", sorted(dd["b"]) == ["banana", "blueberry"])

# OrderedDict
od = OrderedDict()
od["z"] = 1
od["a"] = 2
od["m"] = 3
test("ordered keys", list(od.keys()) == ["z", "a", "m"])

# 5. functools.partial
from functools import partial

def power(base, exp):
    return base ** exp

square = partial(power, exp=2)
cube = partial(power, exp=3)
test("partial square", square(5) == 25)
test("partial cube", cube(3) == 27)

# 6. Recursive algorithms
def quicksort(lst):
    if len(lst) <= 1:
        return lst
    pivot = lst[0]
    less = [x for x in lst[1:] if x <= pivot]
    greater = [x for x in lst[1:] if x > pivot]
    return quicksort(less) + [pivot] + quicksort(greater)

test("quicksort", quicksort([3, 6, 8, 10, 1, 2, 1]) == [1, 1, 2, 3, 6, 8, 10])
test("quicksort empty", quicksort([]) == [])
test("quicksort single", quicksort([5]) == [5])

def fibonacci(n):
    if n <= 1:
        return n
    return fibonacci(n - 1) + fibonacci(n - 2)

test("fibonacci", fibonacci(10) == 55)

# 7. Closure patterns
def make_counter():
    count = 0
    def increment():
        nonlocal count
        count += 1
        return count
    def get():
        return count
    return increment, get

inc, get = make_counter()
inc()
inc()
inc()
test("closure counter", get() == 3)

def make_accumulator(initial=0):
    total = initial
    def add(value):
        nonlocal total
        total += value
        return total
    return add

acc = make_accumulator(10)
test("accumulator", acc(5) == 15)
test("accumulator 2", acc(3) == 18)

# 8. Decorator patterns
def memoize(func):
    cache = {}
    def wrapper(*args):
        if args not in cache:
            cache[args] = func(*args)
        return cache[args]
    return wrapper

@memoize
def fib_memo(n):
    if n <= 1:
        return n
    return fib_memo(n - 1) + fib_memo(n - 2)

test("memoized fib", fib_memo(20) == 6765)

# 9. String formatting edge cases
test("format int", f"{42:05d}" == "00042")
test("format float", f"{3.14159:.2f}" == "3.14")
test("format hex", f"{255:x}" == "ff")
test("format oct", f"{8:o}" == "10")
test("format bin", f"{10:b}" == "1010")
test("format width", f"{'hello':>10}" == "     hello")
test("format center", f"{'hi':^10}" == "    hi    ")

# 10. Exception handling patterns
class Retry:
    def __init__(self, max_retries=3):
        self.max_retries = max_retries
    
    def execute(self, func):
        for i in range(self.max_retries):
            try:
                return func(i)
            except ValueError:
                if i == self.max_retries - 1:
                    raise
                continue

def flaky_func(attempt):
    if attempt < 2:
        raise ValueError(f"attempt {attempt} failed")
    return "success"

r = Retry()
test("retry pattern", r.execute(flaky_func) == "success")

# 11. List operations
lst = [3, 1, 4, 1, 5, 9, 2, 6]
test("list sorted", sorted(lst) == [1, 1, 2, 3, 4, 5, 6, 9])
test("list min", min(lst) == 1)
test("list max", max(lst) == 9)
test("list sum", sum(lst) == 31)

# Sorting with key and reverse
words = ["banana", "apple", "cherry", "date"]
test("sort key", sorted(words, key=len) == ["date", "apple", "banana", "cherry"])
test("sort reverse", sorted(words, reverse=True) == ["date", "cherry", "banana", "apple"])

# 12. Dictionary comprehension patterns
squares = {x: x*x for x in range(6)}
test("dict comp", squares == {0: 0, 1: 1, 2: 4, 3: 9, 4: 16, 5: 25})

inv = {v: k for k, v in squares.items()}
test("dict invert", inv[9] == 3)

# 13. String methods
s = "Hello, World!"
test("str split join", " ".join(s.split(", ")) == "Hello World!")
test("str strip", "  hello  ".strip() == "hello")
test("str lstrip", "  hello  ".lstrip() == "hello  ")
test("str rstrip", "  hello  ".rstrip() == "  hello")
test("str find", s.find("World") == 7)
test("str rfind", "abcabc".rfind("abc") == 3)
test("str replace", s.replace("World", "Python") == "Hello, Python!")
test("str partition", "key=value".partition("=") == ("key", "=", "value"))

# 14. Map, filter, reduce patterns
numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
evens = list(filter(lambda x: x % 2 == 0, numbers))
test("filter evens", evens == [2, 4, 6, 8, 10])

doubled = list(map(lambda x: x * 2, evens))
test("map doubled", doubled == [4, 8, 12, 16, 20])

# 15. Nested data structures
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
transpose = [[row[i] for row in matrix] for i in range(3)]
test("transpose", transpose == [[1, 4, 7], [2, 5, 8], [3, 6, 9]])

flat = [x for row in matrix for x in row]
test("flatten", flat == [1, 2, 3, 4, 5, 6, 7, 8, 9])

# 16. Advanced string operations
csv_line = "Alice,30,Developer"
fields = csv_line.split(",")
test("csv parse", fields == ["Alice", "30", "Developer"])

# String multiplication in formatting
header = "=" * 20
test("str mul", len(header) == 20 and header == "====================")

# 17. Boolean logic and short-circuit
def side_effect_a():
    return False

def side_effect_b():
    return True

test("and short-circuit", not (False and side_effect_b()))
test("or short-circuit", True or side_effect_a())

# 18. Chained operations
data = [
    {"name": "Alice", "score": 95},
    {"name": "Bob", "score": 87},
    {"name": "Charlie", "score": 92},
    {"name": "Diana", "score": 98},
]

top_students = [d["name"] for d in sorted(data, key=lambda x: x["score"], reverse=True)[:2]]
test("chained ops", top_students == ["Diana", "Alice"])

# 19. Error handling with finally
resources = []

def use_resource():
    resources.append("acquired")
    try:
        resources.append("used")
        return "result"
    finally:
        resources.append("released")

result = use_resource()
test("finally return", result == "result")
test("finally side effect", resources == ["acquired", "used", "released"])

# 20. Enumerate with dict building
items = ["apple", "banana", "cherry"]
indexed = {i: item for i, item in enumerate(items)}
test("enumerate dict", indexed == {0: "apple", 1: "banana", 2: "cherry"})

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 43 TESTS PASSED")
