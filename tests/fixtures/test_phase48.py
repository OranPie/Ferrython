"""Phase 48: Advanced OOP, error handling, numeric methods, protocol methods"""

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

# 1. __contains__ protocol
class Container:
    def __init__(self, items):
        self.items = items
    def __contains__(self, item):
        return item in self.items

c = Container([1, 2, 3])
test("contains True", 2 in c)
test("contains False", 5 not in c)

# 2. __len__ and __bool__
class MyList:
    def __init__(self):
        self.data = []
    def append(self, item):
        self.data.append(item)
    def __len__(self):
        return len(self.data)
    def __bool__(self):
        return len(self.data) > 0

ml = MyList()
test("bool empty", not ml)
ml.append(1)
test("bool nonempty", bool(ml))
test("len 1", len(ml) == 1)

# 3. __getitem__ and __setitem__
class Matrix:
    def __init__(self, rows, cols):
        self.rows = rows
        self.cols = cols
        self.data = [0] * (rows * cols)
    def __getitem__(self, key):
        r, c = key
        return self.data[r * self.cols + c]
    def __setitem__(self, key, value):
        r, c = key
        self.data[r * self.cols + c] = value

m = Matrix(2, 3)
m[0, 0] = 1
m[1, 2] = 5
test("matrix set/get", m[0, 0] == 1)
test("matrix set/get 2", m[1, 2] == 5)

# 4. __eq__ and __ne__
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    def __ne__(self, other):
        return not self.__eq__(other)
    def __repr__(self):
        return f"Point({self.x}, {self.y})"

p1 = Point(1, 2)
p2 = Point(1, 2)
p3 = Point(3, 4)
test("eq same", p1 == p2)
test("ne diff", p1 != p3)

# 5. __add__ and __mul__
class Vector:
    def __init__(self, *args):
        self.data = list(args)
    def __add__(self, other):
        return Vector(*[a + b for a, b in zip(self.data, other.data)])
    def __mul__(self, scalar):
        return Vector(*[a * scalar for a in self.data])
    def __repr__(self):
        return f"Vector({', '.join(str(x) for x in self.data)})"

v1 = Vector(1, 2, 3)
v2 = Vector(4, 5, 6)
v3 = v1 + v2
test("vector add", v3.data == [5, 7, 9])
v4 = v1 * 3
test("vector mul", v4.data == [3, 6, 9])

# 6. __iter__ and __next__
class FibIterator:
    def __init__(self, n):
        self.n = n
        self.a = 0
        self.b = 1
        self.count = 0
    def __iter__(self):
        return self
    def __next__(self):
        if self.count >= self.n:
            raise StopIteration
        result = self.a
        self.a, self.b = self.b, self.a + self.b
        self.count += 1
        return result

fib = list(FibIterator(8))
test("fib iterator", fib == [0, 1, 1, 2, 3, 5, 8, 13])

# 7. Context manager with __enter__/__exit__
class FileLogger:
    def __init__(self):
        self.log = []
    def __enter__(self):
        self.log.append("enter")
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.log.append("exit")
        return False

logger = FileLogger()
with logger as l:
    l.log.append("inside")
test("context mgr", logger.log == ["enter", "inside", "exit"])

# 8. Exception with args
try:
    raise ValueError("bad value", 42)
except ValueError as e:
    test("exception args", e.args == ("bad value", 42))

# 9. Exception hierarchy
class AppError(Exception):
    pass
class ValidationError(AppError):
    pass

try:
    raise ValidationError("invalid")
except AppError as e:
    test("custom exc hierarchy", str(e) == "invalid")

# 10. Numeric edge cases
test("int division", 7 // 2 == 3)
test("neg floor div", -7 // 2 == -4)
test("modulo neg", -7 % 3 == 2)
test("power", 2 ** 10 == 1024)
test("float div", abs(7 / 2 - 3.5) < 0.001)

# 11. Complex string formatting
test("format int", format(42, "05d") == "00042")
test("format float", format(3.14159, ".2f") == "3.14")
test("format hex", format(255, "x") == "ff")
test("format oct", format(8, "o") == "10")
test("format bin", format(10, "b") == "1010")

# 12. Dict merging (Python 3.9+ but we can test dict update)
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
merged = {**d1, **d2}
test("dict merge", merged == {"a": 1, "b": 3, "c": 4})

# 13. Tuple operations
t = (1, 2, 3, 2, 1)
test("tuple count", t.count(2) == 2)
test("tuple index", t.index(3) == 2)

# 14. String formatting methods
test("str center", "hi".center(10, "-") == "----hi----")
test("str ljust", "hi".ljust(5, ".") == "hi...")
test("str rjust", "hi".rjust(5, "0") == "000hi")
test("str zfill", "42".zfill(5) == "00042")

# 15. List comprehension with multiple conditions
result = [x for x in range(20) if x % 2 == 0 if x % 3 == 0]
test("multi condition comp", result == [0, 6, 12, 18])

# 16. Nested dict/list patterns
data = {
    "users": [
        {"name": "Alice", "age": 30},
        {"name": "Bob", "age": 25},
    ]
}
names = [u["name"] for u in data["users"]]
test("nested data", names == ["Alice", "Bob"])

# 17. String methods
test("str isalpha", "hello".isalpha())
test("str isdigit", "12345".isdigit())
test("str isalnum", "abc123".isalnum())
test("str isupper", "HELLO".isupper())
test("str islower", "hello".islower())

# 18. Callable check pattern
def is_callable(obj):
    return callable(obj)

test("callable func", is_callable(len))
test("callable lambda", is_callable(lambda: None))
test("callable int", not is_callable(42))

# 19. Global/nonlocal
x = 10
def modify_global():
    global x
    x = 20
modify_global()
test("global var", x == 20)

def make_counter():
    count = 0
    def inc():
        nonlocal count
        count += 1
        return count
    return inc

counter = make_counter()
test("nonlocal", counter() == 1)
test("nonlocal 2", counter() == 2)

# 20. Star unpacking in assignment
first, *rest = [1, 2, 3, 4, 5]
test("star unpack first", first == 1)
test("star unpack rest", rest == [2, 3, 4, 5])

*init, last = [1, 2, 3, 4, 5]
test("star unpack init", init == [1, 2, 3, 4])
test("star unpack last", last == 5)

a, *mid, z = [1, 2, 3, 4, 5]
test("star unpack mid", mid == [2, 3, 4])

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 48 TESTS PASSED")
