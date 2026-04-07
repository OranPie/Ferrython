# Test realistic Python patterns that packages commonly use

# --- 1. ABC-based interface pattern ---
from abc import ABC, abstractmethod

class Shape(ABC):
    @abstractmethod
    def area(self):
        pass
    
    @abstractmethod 
    def perimeter(self):
        pass
    
    def describe(self):
        return f"{self.__class__.__name__}: area={self.area():.2f}"

class Rectangle(Shape):
    def __init__(self, w, h):
        self.w = w
        self.h = h
    def area(self):
        return self.w * self.h
    def perimeter(self):
        return 2 * (self.w + self.h)

r = Rectangle(3, 4)
assert r.area() == 12
assert r.perimeter() == 14
assert "Rectangle" in r.describe()
print("ABC pattern: OK")

# --- 2. Decorator pattern ---
import functools

def retry(max_attempts=3):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            for attempt in range(max_attempts):
                try:
                    return func(*args, **kwargs)
                except Exception:
                    if attempt == max_attempts - 1:
                        raise
            return None
        return wrapper
    return decorator

call_count = 0
@retry(max_attempts=3)
def flaky_function():
    global call_count
    call_count += 1
    if call_count < 3:
        raise ValueError("not yet")
    return "success"

result = flaky_function()
assert result == "success"
assert call_count == 3
assert flaky_function.__name__ == "flaky_function"
print("Decorator pattern: OK")

# --- 3. Context manager pattern ---
class Timer:
    def __init__(self, name="timer"):
        self.name = name
        self.elapsed = 0
    def __enter__(self):
        import time
        self.start = time.time()
        return self
    def __exit__(self, *args):
        import time
        self.elapsed = time.time() - self.start
        return False

with Timer("test") as t:
    total = sum(range(10000))
assert t.elapsed >= 0
assert total == 49995000
print("Context manager: OK")

# --- 4. Enum pattern ---
from enum import Enum, IntEnum, auto

class Color(Enum):
    RED = auto()
    GREEN = auto()
    BLUE = auto()

class Permission(IntEnum):
    READ = 4
    WRITE = 2
    EXECUTE = 1

assert Color.RED.value == 1
assert Color.GREEN.value == 2
assert Permission.READ == 4
assert Permission.READ | Permission.WRITE == 6
print("Enum pattern: OK")

# --- 5. Collections pattern ---
from collections import defaultdict, Counter, namedtuple

# Word frequency counter
words = "the quick brown fox jumps over the lazy dog the fox".split()
freq = Counter(words)
assert freq["the"] == 3
assert freq["fox"] == 2
top2 = freq.most_common(2)
assert top2[0][0] == "the"

# Grouping with defaultdict
groups = defaultdict(list)
data = [("a", 1), ("b", 2), ("a", 3), ("b", 4), ("c", 5)]
for key, val in data:
    groups[key].append(val)
assert groups["a"] == [1, 3]
assert groups["b"] == [2, 4]

# Named tuple
Point3D = namedtuple("Point3D", ["x", "y", "z"])
p = Point3D(1, 2, 3)
assert p.x == 1 and p.y == 2 and p.z == 3
assert p[0] == 1
print("Collections patterns: OK")

# --- 6. Chained comparison ---
x = 5
assert 1 < x < 10
assert not (1 < x < 3)
assert 0 <= x <= 10
print("Chained comparison: OK")

# --- 7. Dict comprehension with filtering ---
prices = {"apple": 1.50, "banana": 0.50, "cherry": 3.00, "date": 2.00}
expensive = {k: v for k, v in prices.items() if v > 1.0}
assert len(expensive) == 3
assert "banana" not in expensive
print("Dict comprehension: OK")

# --- 8. Multiple assignment and tuple unpacking ---
a, (b, c), d = 1, (2, 3), 4
assert a == 1 and b == 2 and c == 3 and d == 4

# Swap
x, y = 1, 2
x, y = y, x
assert x == 2 and y == 1
print("Tuple unpacking: OK")

# --- 9. String formatting ---
name = "World"
assert f"Hello, {name}!" == "Hello, World!"
assert "Hello, {}!".format(name) == "Hello, World!"
assert "Hello, %s!" % name == "Hello, World!"
assert "{0} {1} {0}".format("a", "b") == "a b a"
print("String formatting: OK")

# --- 10. Exception chaining ---
try:
    try:
        raise ValueError("original")
    except ValueError as e:
        raise RuntimeError("wrapped") from e
except RuntimeError as e:
    assert str(e) == "wrapped"
    assert e.__cause__ is not None
    print("Exception chaining: OK")

print("All phase 119 tests passed!")
