# Phase 36: Advanced class features, exception handling, closures
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── isinstance with tuple ──
test("isinstance int", isinstance(42, int))
test("isinstance str", isinstance("hello", str))
test("isinstance tuple of types", isinstance(42, (int, str)))
test("isinstance tuple of types str", isinstance("hi", (int, str)))
test("isinstance tuple of types false", not isinstance(42, (str, list)))

# ── issubclass ──
class Animal:
    pass
class Dog(Animal):
    pass
class Cat(Animal):
    pass

test("issubclass Dog Animal", issubclass(Dog, Animal))
test("issubclass Cat Animal", issubclass(Cat, Animal))
test("issubclass Dog Dog", issubclass(Dog, Dog))
test("issubclass Animal Dog false", not issubclass(Animal, Dog))

# ── Multiple except with tuple ──
def safe_div(a, b):
    try:
        return a / b
    except (ZeroDivisionError, TypeError):
        return None

test("except tuple catch ZDE", safe_div(1, 0) is None)
test("except tuple normal", safe_div(10, 2) == 5.0)

# ── Exception attributes ──
try:
    raise ValueError("test error")
except ValueError as e:
    test("exception str", str(e) == "test error")
    test("exception args", e.args == ("test error",))

# ── Nested exceptions ──
def outer():
    try:
        inner()
    except RuntimeError:
        return "caught runtime"
    return "no error"

def inner():
    raise RuntimeError("inner error")

test("nested exception", outer() == "caught runtime")

# ── Generator advanced ──
def fibonacci():
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b

gen = fibonacci()
fibs = [next(gen) for _ in range(10)]
test("generator fibonacci", fibs == [0, 1, 1, 2, 3, 5, 8, 13, 21, 34])

# ── Generator send ──
def accumulator():
    total = 0
    while True:
        value = yield total
        if value is None:
            break
        total += value

gen = accumulator()
next(gen)  # prime
test("gen.send 10", gen.send(10) == 10)
test("gen.send 20", gen.send(20) == 30)
test("gen.send 5", gen.send(5) == 35)

# ── Closure over mutable ──
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
test("closure counter 1", inc() == 1)
test("closure counter 2", inc() == 2)
test("closure counter 3", inc() == 3)
test("closure counter get", get() == 3)

# ── Decorator with args ──
def repeat(n):
    def decorator(func):
        def wrapper(*args, **kwargs):
            result = []
            for _ in range(n):
                result.append(func(*args, **kwargs))
            return result
        return wrapper
    return decorator

@repeat(3)
def greet(name):
    return f"Hi {name}"

test("decorator with args", greet("Alice") == ["Hi Alice", "Hi Alice", "Hi Alice"])

# ── Class with __eq__ and __hash__ ──
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    
    def __eq__(self, other):
        if not isinstance(other, Point):
            return False
        return self.x == other.x and self.y == other.y
    
    def __hash__(self):
        return hash((self.x, self.y))
    
    def __repr__(self):
        return f"Point({self.x}, {self.y})"

p1 = Point(1, 2)
p2 = Point(1, 2)
p3 = Point(3, 4)
test("custom __eq__ true", p1 == p2)
test("custom __eq__ false", p1 != p3)

# ── Dict comprehension ──
squares = {x: x**2 for x in range(6)}
test("dict comp", squares == {0: 0, 1: 1, 2: 4, 3: 9, 4: 16, 5: 25})

# ── Set comprehension ──
evens = {x for x in range(10) if x % 2 == 0}
test("set comp", evens == {0, 2, 4, 6, 8})

# ── Nested comprehension ──
matrix = [[i*3 + j for j in range(3)] for i in range(3)]
test("nested list comp", matrix == [[0, 1, 2], [3, 4, 5], [6, 7, 8]])
flat = [x for row in matrix for x in row]
test("flatten comp", flat == [0, 1, 2, 3, 4, 5, 6, 7, 8])

# ── Lambda in higher-order functions ──
nums = [5, 2, 8, 1, 9]
test("sorted with key", sorted(nums, key=lambda x: -x) == [9, 8, 5, 2, 1])
test("filter lambda", list(filter(lambda x: x > 3, nums)) == [5, 8, 9])
test("map lambda", list(map(lambda x: x * 2, nums)) == [10, 4, 16, 2, 18])

# ── String formatting edge cases ──
test("format int", f"{42:05d}" == "00042")
test("format float", f"{3.14159:.2f}" == "3.14")
test("format string", f"{'hello':>10}" == "     hello")
test("format string left", f"{'hello':<10}" == "hello     ")
test("format string center", f"{'hello':^10}" == "  hello   ")
test("f-string nested expr", f"{'ab' + 'cd'}" == "abcd")

# ── Unpacking in function calls ──
def add(a, b, c):
    return a + b + c

args = [1, 2, 3]
test("unpack in call", add(*args) == 6)

d = {"a": 1, "b": 2, "c": 3}
test("unpack kwargs in call", add(**d) == 6)

# ── Global and nonlocal ──
global_var = 10

def modify_global():
    global global_var
    global_var = 20

modify_global()
test("global modified", global_var == 20)

def outer_func():
    x = 10
    def inner_func():
        nonlocal x
        x = 20
    inner_func()
    return x

test("nonlocal modified", outer_func() == 20)

# ── try/except/else/finally ──
def try_test(val):
    result = []
    try:
        result.append("try")
        if val == 0:
            raise ValueError("zero")
    except ValueError:
        result.append("except")
    else:
        result.append("else")
    finally:
        result.append("finally")
    return result

test("try no exception", try_test(1) == ["try", "else", "finally"])
test("try with exception", try_test(0) == ["try", "except", "finally"])

# ── while/else ──
def find_item(lst, target):
    i = 0
    while i < len(lst):
        if lst[i] == target:
            break
        i += 1
    else:
        return -1
    return i

test("while/else found", find_item([1, 2, 3], 2) == 1)
test("while/else not found", find_item([1, 2, 3], 5) == -1)

# ── for/else ──
def has_prime_factor(n, factor):
    for i in range(2, n):
        if n % i == 0 and i == factor:
            break
    else:
        return False
    return True

test("for/else found", has_prime_factor(12, 3))
test("for/else not found", not has_prime_factor(7, 3))

# ── Chained string methods ──
test("chained str methods", "  Hello, World!  ".strip().lower().replace(",", "").split() == ["hello", "world!"])

# ── any() and all() ──
test("any true", any([0, 0, 1, 0]))
test("any false", not any([0, 0, 0]))
test("all true", all([1, 2, 3]))
test("all false", not all([1, 0, 3]))
test("any generator", any(x > 5 for x in range(10)))
test("all generator", all(x < 10 for x in range(10)))

# ── zip with different lengths ──
test("zip short", list(zip([1, 2, 3], ['a', 'b'])) == [(1, 'a'), (2, 'b')])
test("zip three", list(zip([1, 2], ['a', 'b'], [True, False])) == [(1, 'a', True), (2, 'b', False)])

# ── enumerate ──
test("enumerate", list(enumerate(['a', 'b', 'c'])) == [(0, 'a'), (1, 'b'), (2, 'c')])
test("enumerate start", list(enumerate(['a', 'b'], start=1)) == [(1, 'a'), (2, 'b')])

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
assert failed == 0, f"{failed} tests failed!"
print("ALL PHASE 36 TESTS PASSED")
