passed = 0
failed = 0
def test(name, cond):
    global passed, failed
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# === Closure in class body ===
def make_cls():
    val = 42
    class C:
        x = val
    return C

test("cls closure basic", make_cls().x == 42)

def make_cls2():
    items = [1, 2, 3]
    class D:
        data = items
        total = sum(items)
    return D

test("cls closure list", make_cls2().data == [1, 2, 3])
test("cls closure sum", make_cls2().total == 6)

# Triple nested: function -> function -> class using outer var
def outer_fn():
    val = 100
    def inner_fn():
        class E:
            x = val
        return E
    return inner_fn()

test("nested fn cls closure", outer_fn().x == 100)

# === setattr with property ===
class Temperature:
    def __init__(self):
        self._c = 0
    @property
    def celsius(self):
        return self._c
    @celsius.setter
    def celsius(self, val):
        self._c = val

t = Temperature()
setattr(t, 'celsius', 37)
test("setattr property", t.celsius == 37)
test("setattr property _c", t._c == 37)

# setattr with custom __setattr__
class Validated:
    def __setattr__(self, name, value):
        if name == "age" and value < 0:
            raise ValueError("age must be >= 0")
        object.__setattr__(self, name, value)

v = Validated()
setattr(v, 'age', 25)
test("setattr custom", v.age == 25)
try:
    setattr(v, 'age', -1)
    test("setattr validates", False)
except ValueError:
    test("setattr validates", True)

# === dict.update with kwargs ===
d = {"a": 1}
d.update(b=2, c=3)
test("dict update kwargs", d == {"a": 1, "b": 2, "c": 3})

d2 = {"x": 10}
d2.update({"y": 20}, z=30)
test("dict update both", d2 == {"x": 10, "y": 20, "z": 30})

d3 = {}
d3.update(first=1, second=2, third=3)
test("dict update empty", d3 == {"first": 1, "second": 2, "third": 3})

# === More closure patterns ===
# Class inheriting from a closure variable
def make_subclass():
    class Base:
        def greet(self):
            return "hello"
    class Child(Base):
        def greet(self):
            return super().greet() + " world"
    return Child

test("cls inherit closure", make_subclass()().greet() == "hello world")

# Closure variable used in method body (not class body)
def factory(multiplier):
    class Mul:
        def calc(self, x):
            return x * multiplier
    return Mul

test("closure in method", factory(3)().calc(7) == 21)

# === Property advanced ===
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
test("prop area", abs(c.area - 78.53975) < 0.001)
c.radius = 10
test("prop setter", c.radius == 10)
test("prop area update", abs(c.area - 314.159) < 0.001)

try:
    c.radius = -1
    test("prop validation", False)
except ValueError:
    test("prop validation", True)

# === Generator send with closure ===
def accumulator(initial):
    total = initial
    def gen():
        nonlocal total
        while True:
            value = yield total
            if value is not None:
                total += value
    return gen()

acc = accumulator(0)
next(acc)
test("gen send 1", acc.send(10) == 10)
test("gen send 2", acc.send(20) == 30)
test("gen send 3", acc.send(5) == 35)

# === Complex decorator patterns ===
def retry(max_attempts):
    def decorator(func):
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
@retry(3)
def flaky():
    global call_count
    call_count += 1
    if call_count < 3:
        raise RuntimeError("not yet")
    return "success"

test("retry decorator", flaky() == "success")
test("retry called 3x", call_count == 3)

# === Dict comprehension from class ===
class Config:
    host = "localhost"
    port = 8080
    debug = True

attrs = {k: v for k, v in Config.__dict__.items() if not k.startswith("_")}
test("dict comp class", attrs["host"] == "localhost")
test("dict comp class2", attrs["port"] == 8080)

# === Nested with statements ===
class CM:
    def __init__(self, name, log):
        self.name = name
        self.log = log
    def __enter__(self):
        self.log.append(f"enter:{self.name}")
        return self
    def __exit__(self, *args):
        self.log.append(f"exit:{self.name}")
        return False

log = []
with CM("a", log) as a:
    with CM("b", log) as b:
        log.append("body")

test("nested with", log == ["enter:a", "enter:b", "body", "exit:b", "exit:a"])

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 55 TESTS PASSED")
