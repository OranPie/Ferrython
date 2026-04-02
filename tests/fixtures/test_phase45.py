"""Phase 45: Walrus operator, bytes, nested classes, class decorators,
   complex exception patterns, chained methods, multiple inheritance,
   abstract-like patterns"""

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

# 1. Walrus operator (:=) basics
if (n := 10) > 5:
    test("walrus basic", n == 10)

# 2. Walrus in while
data = [1, 2, 3, 4, 5]
idx = 0
total_sum = 0
while idx < len(data) and (val := data[idx]) > 0:
    total_sum += val
    idx += 1
test("walrus while", total_sum == 15)

# 3. Walrus in comprehension filter
numbers = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
evens = [y for x in numbers if (y := x * 2) > 6]
test("walrus comp", evens == [8, 10, 12, 14, 16, 18, 20])

# 4. Bytes basics
b = b"hello"
test("bytes len", len(b) == 5)
test("bytes index", b[0] == 104)  # 'h' = 104
test("bytes slice", b[1:3] == b"el")
test("bytes in", 104 in b)

# 5. Bytes methods
test("bytes upper", b"hello".upper() == b"HELLO")
test("bytes lower", b"HELLO".lower() == b"hello")
test("bytes strip", b"  hello  ".strip() == b"hello")
test("bytes split", b"a,b,c".split(b",") == [b"a", b"b", b"c"])
test("bytes join", b",".join([b"a", b"b", b"c"]) == b"a,b,c")
test("bytes hex", b"AB".hex() == "4142")
test("bytes decode", b"hello".decode("utf-8") == "hello")

# 6. Encode/decode
s = "hello"
b2 = s.encode("utf-8")
test("str encode", b2 == b"hello")
test("bytes decode rt", b2.decode("utf-8") == "hello")

# 7. Nested classes
class Outer:
    class Inner:
        value = 42
    
    def get_inner(self):
        return self.Inner()

o = Outer()
test("nested class", Outer.Inner.value == 42)
inner = o.get_inner()
test("nested instance", inner.value == 42)

# 8. Class decorator
def add_greeting(cls):
    cls.greet = lambda self: f"Hello, {self.name}!"
    return cls

@add_greeting
class Person:
    def __init__(self, name):
        self.name = name

p = Person("Alice")
test("class decorator", p.greet() == "Hello, Alice!")

# 9. Multiple inheritance diamond
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

d = D()
test("diamond MRO", d.method() == "DBCA")

# 10. Exception chaining
try:
    try:
        raise ValueError("original")
    except ValueError as e:
        raise TypeError("converted") from e
except TypeError as te:
    test("exception from", str(te) == "converted")
    test("exception cause", te.__cause__ is not None)

# 11. Exception groups (basic)
try:
    raise RuntimeError("test error")
except RuntimeError as e:
    test("runtime error", str(e) == "test error")

# 12. Re-raise
caught_outer = False
try:
    try:
        raise ValueError("test")
    except ValueError:
        raise  # re-raise
except ValueError as e:
    caught_outer = True
    test("re-raise msg", str(e) == "test")
test("re-raise caught", caught_outer)

# 13. Dict merge operator (|)
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
# Note: |= and | for dicts is Python 3.9+, using .update instead
d3 = {**d1, **d2}
test("dict merge spread", d3 == {"a": 1, "b": 3, "c": 4})

# 14. Multiple return values
def min_max(lst):
    return min(lst), max(lst)

lo, hi = min_max([3, 1, 4, 1, 5, 9])
test("multi return", lo == 1 and hi == 9)

# 15. Complex comprehension with method chains
words = ["Hello", "World", "Python"]
result = [w.lower().replace("o", "0") for w in words]
test("method chain comp", result == ["hell0", "w0rld", "pyth0n"])

# 16. Nested dict access with defaults
config = {"database": {"host": "localhost", "port": 5432}}
host = config.get("database", {}).get("host", "unknown")
test("nested dict get", host == "localhost")
missing = config.get("cache", {}).get("host", "none")
test("nested dict default", missing == "none")

# 17. zip with multiple iterables
names = ["Alice", "Bob", "Charlie"]
ages = [30, 25, 35]
scores = [95, 87, 92]
combined = list(zip(names, ages, scores))
test("zip three", combined == [("Alice", 30, 95), ("Bob", 25, 87), ("Charlie", 35, 92)])

# 18. Chained string operations
text = "  Hello, World!  "
result = text.strip().lower().replace(",", "").replace("!", "")
test("str chain", result == "hello world")

# 19. Complex sorting
data = [("Alice", 30), ("Bob", 25), ("Charlie", 30), ("Diana", 25)]
# Sort by age ascending, then name ascending
sorted_data = sorted(data, key=lambda x: (x[1], x[0]))
test("multi sort", sorted_data == [("Bob", 25), ("Diana", 25), ("Alice", 30), ("Charlie", 30)])

# 20. Iterator protocol
class Countdown:
    def __init__(self, start):
        self.start = start
    def __iter__(self):
        return self
    def __next__(self):
        if self.start <= 0:
            raise StopIteration
        self.start -= 1
        return self.start + 1

test("custom iter", list(Countdown(5)) == [5, 4, 3, 2, 1])

# 21. Generator with send
def accumulator():
    total = 0
    while True:
        value = yield total
        if value is None:
            break
        total += value

gen = accumulator()
next(gen)  # prime
test("gen send", gen.send(10) == 10)
test("gen send 2", gen.send(20) == 30)
test("gen send 3", gen.send(5) == 35)

# 22. try/except/else/finally
log = []
try:
    log.append("try")
    x = 42
except Exception:
    log.append("except")
else:
    log.append("else")
finally:
    log.append("finally")
test("try else finally", log == ["try", "else", "finally"])

# 23. try with exception
log2 = []
try:
    log2.append("try")
    raise ValueError("test")
except ValueError:
    log2.append("except")
else:
    log2.append("else")
finally:
    log2.append("finally")
test("try except finally", log2 == ["try", "except", "finally"])

# 24. Truthiness
test("truthy empty list", not [])
test("truthy list", bool([1]))
test("truthy empty str", not "")
test("truthy str", bool("x"))
test("truthy zero", not 0)
test("truthy nonzero", bool(1))
test("truthy none", not None)
test("truthy empty dict", not {})

# 25. All/any
test("all true", all([True, True, True]))
test("all false", not all([True, False, True]))
test("any true", any([False, True, False]))
test("any false", not any([False, False, False]))
test("all empty", all([]))
test("any empty", not any([]))

# 26. Complex lambda
transform = lambda x: x ** 2 if x > 0 else -x
test("lambda cond", transform(3) == 9)
test("lambda neg", transform(-5) == 5)

# 27. Dict keys/values/items
d = {"a": 1, "b": 2, "c": 3}
test("dict keys", sorted(d.keys()) == ["a", "b", "c"])
test("dict values", sorted(d.values()) == [1, 2, 3])
test("dict items", sorted(d.items()) == [("a", 1), ("b", 2), ("c", 3)])

# 28. String formatting
test("format spec", format(42, "05d") == "00042")
test("format float spec", format(3.14159, ".2f") == "3.14")
test("f-string expr", f"{2 + 3}" == "5")
test("f-string format", f"{42:08b}" == "00101010")

# 29. Unpacking in assignments
(a, b), c = (1, 2), 3
test("nested unpack", a == 1 and b == 2 and c == 3)

# 30. isinstance with class hierarchy
class Base:
    pass
class Child(Base):
    pass
class GrandChild(Child):
    pass

gc = GrandChild()
test("isinstance grandchild", isinstance(gc, Base))
test("isinstance child", isinstance(gc, Child))
test("isinstance exact", isinstance(gc, GrandChild))
test("not isinstance", not isinstance(Base(), Child))

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 45 TESTS PASSED")
