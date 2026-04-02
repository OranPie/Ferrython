"""Phase 40: Advanced patterns — decorators with args, class methods as callables,
   multiple inheritance, method resolution, __repr__/__str__ dispatch, 
   custom exceptions, context managers, generator.send, yield from"""

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

# 1. Decorator with arguments (closure-based)
def repeat(n):
    def decorator(func):
        def wrapper(*args, **kwargs):
            results = []
            for _ in range(n):
                results.append(func(*args, **kwargs))
            return results
        return wrapper
    return decorator

@repeat(3)
def greet(name):
    return f"Hello, {name}!"

result = greet("World")
test("decorator with args", result == ["Hello, World!", "Hello, World!", "Hello, World!"])

# 2. Multiple inheritance with MRO
class A:
    def method(self):
        return "A"

class B(A):
    def method(self):
        return "B"

class C(A):
    def method(self):
        return "C"

class D(B, C):
    pass

d = D()
test("MRO dispatch", d.method() == "B")

# 3. super() in chain
class Base:
    def greet(self):
        return "Base"

class Middle(Base):
    def greet(self):
        return "Middle+" + super().greet()

class Child(Middle):
    def greet(self):
        return "Child+" + super().greet()

c = Child()
test("super chain", c.greet() == "Child+Middle+Base")

# 4. Custom __repr__ and __str__
class MyClass:
    def __init__(self, val):
        self.val = val
    def __repr__(self):
        return f"MyClass({self.val})"
    def __str__(self):
        return f"value={self.val}"

obj = MyClass(42)
test("custom __repr__", repr(obj) == "MyClass(42)")
test("custom __str__", str(obj) == "value=42")

# 5. Custom exception with attributes
class AppError(Exception):
    def __init__(self, code, msg):
        super().__init__(msg)
        self.code = code
        self.msg = msg

try:
    raise AppError(404, "Not Found")
except AppError as e:
    test("custom exc code", e.code == 404)
    test("custom exc msg", e.msg == "Not Found")

# 6. Context manager protocol
class Managed:
    def __init__(self):
        self.entered = False
        self.exited = False
    def __enter__(self):
        self.entered = True
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.exited = True
        return False

m = Managed()
with m as ctx:
    test("context enter", ctx.entered)
test("context exit", m.exited)

# 7. Context manager suppressing exception
class Suppressor:
    def __enter__(self): return self
    def __exit__(self, *args): return True  # suppress

try:
    with Suppressor():
        raise ValueError("suppressed")
    test("context suppress exc", True)
except:
    test("context suppress exc", False)

# 8. Generator send
def accumulator():
    total = 0
    while True:
        value = yield total
        if value is None:
            break
        total += value

gen = accumulator()
next(gen)  # prime
test("gen send 10", gen.send(10) == 10)
test("gen send 20", gen.send(20) == 30)
test("gen send 5", gen.send(5) == 35)

# 9. yield from
def inner_gen():
    yield 1
    yield 2
    yield 3

def outer_gen():
    yield 0
    yield from inner_gen()
    yield 4

result = list(outer_gen())
test("yield from", result == [0, 1, 2, 3, 4])

# 10. Property with setter
class Temperature:
    def __init__(self, celsius=0):
        self._celsius = celsius
    
    @property
    def celsius(self):
        return self._celsius
    
    @celsius.setter
    def celsius(self, value):
        self._celsius = value
    
    @property
    def fahrenheit(self):
        return self._celsius * 9 / 5 + 32

t = Temperature(100)
test("property getter", t.celsius == 100)
test("property computed", t.fahrenheit == 212.0)
t.celsius = 0
test("property setter", t.celsius == 0)
test("property recomputed", t.fahrenheit == 32.0)

# 11. Static method and class method
class Counter:
    count = 0
    
    def __init__(self):
        Counter.count += 1
    
    @classmethod
    def get_count(cls):
        return cls.count
    
    @staticmethod
    def reset():
        Counter.count = 0

Counter.reset()
c1 = Counter()
c2 = Counter()
test("classmethod", Counter.get_count() == 2)
Counter.reset()
test("staticmethod", Counter.get_count() == 0)

# 12. String methods as first-class citizens
words = ["hello", "WORLD", "Python"]
upper_words = list(map(str.upper, words))
test("map str.upper", upper_words == ["HELLO", "WORLD", "PYTHON"])

lower_words = list(map(str.lower, words))
test("map str.lower", lower_words == ["hello", "world", "python"])

# 13. Dict.get with default
d = {"a": 1, "b": 2}
test("dict.get existing", d.get("a") == 1)
test("dict.get missing", d.get("c") is None)
test("dict.get default", d.get("c", 42) == 42)

# 14. Nested try/except
def nested_try():
    try:
        try:
            raise ValueError("inner")
        except ValueError:
            return "caught inner"
    except:
        return "caught outer"

test("nested try", nested_try() == "caught inner")

# 15. Exception re-raise
def reraise_test():
    try:
        try:
            raise ValueError("original")
        except ValueError:
            raise  # re-raise
    except ValueError as e:
        return str(e)

try:
    result = reraise_test()
    test("reraise", result == "original")
except:
    test("reraise", False)

# 16. Star args and kwargs
def varargs(*args, **kwargs):
    return (args, kwargs)

result = varargs(1, 2, 3, name="Alice", age=30)
test("varargs positional", result[0] == (1, 2, 3))
test("varargs keyword", result[1] == {"name": "Alice", "age": 30})

# 17. Unpacking in function call
def add3(a, b, c):
    return a + b + c

nums = [10, 20, 30]
test("unpack call", add3(*nums) == 60)

kw = {"a": 1, "b": 2, "c": 3}
test("unpack kwargs", add3(**kw) == 6)

# 18. Chained methods
test("chained str methods", "  Hello World  ".strip().lower().split() == ["hello", "world"])

# 19. List slicing
lst = [0, 1, 2, 3, 4, 5]
test("slice basic", lst[1:4] == [1, 2, 3])
test("slice step", lst[::2] == [0, 2, 4])
test("slice negative", lst[-2:] == [4, 5])
test("slice reverse", lst[::-1] == [5, 4, 3, 2, 1, 0])

# 20. Dict unpacking merge
def merge_dicts(**kwargs):
    return kwargs

merged = merge_dicts(**{"a": 1}, **{"b": 2})
test("dict unpack merge", merged == {"a": 1, "b": 2})

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 40 TESTS PASSED")
