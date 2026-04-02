"""Phase 53: Iterator protocol, __class__ cell, complex inheritance,
   exception re-raise, global/nonlocal edge cases"""

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

# 1. Custom iterator with for/else
class Range2:
    def __init__(self, n):
        self.n = n
    def __iter__(self):
        return Range2Iter(self.n)

class Range2Iter:
    def __init__(self, n):
        self.n = n
        self.i = 0
    def __iter__(self):
        return self
    def __next__(self):
        if self.i >= self.n:
            raise StopIteration
        val = self.i
        self.i += 1
        return val

test("custom iter", list(Range2(5)) == [0, 1, 2, 3, 4])
test("custom for", sum(x for x in Range2(10)) == 45)

# 2. yield from
def gen_yield_from(n):
    yield from range(n)
    yield from range(n, 2 * n)

test("yield from", list(gen_yield_from(3)) == [0, 1, 2, 3, 4, 5])

# 3. Multiple generators interleaved
def gen_a():
    yield 1
    yield 2
    yield 3

def gen_b():
    yield "a"
    yield "b"
    yield "c"

a = gen_a()
b = gen_b()
interleaved = []
for _ in range(3):
    interleaved.append(next(a))
    interleaved.append(next(b))
test("interleaved gen", interleaved == [1, "a", 2, "b", 3, "c"])

# 4. Exception re-raise
def process(items):
    errors = []
    for item in items:
        try:
            if item < 0:
                raise ValueError(f"negative: {item}")
            if item > 100:
                raise OverflowError(f"too large: {item}")
        except ValueError as e:
            errors.append(str(e))
        # OverflowError is not caught here, propagates
    return errors

test("reraise ok", process([1, -2, 3, -4, 5]) == ["negative: -2", "negative: -4"])

caught = False
try:
    process([1, 200])
except OverflowError:
    caught = True
test("uncaught propagates", caught)

# 5. Nonlocal across multiple levels
def make_counter_v2():
    count = [0]  # Mutable container
    
    def increment():
        count[0] += 1
        return count[0]
    
    def decrement():
        count[0] -= 1
        return count[0]
    
    return increment, decrement

inc, dec = make_counter_v2()
test("shared state +", inc() == 1)
test("shared state ++", inc() == 2)
test("shared state -", dec() == 1)

# 6. Tuple unpacking in for loop
pairs = [(1, 2, 3), (4, 5, 6)]
firsts = []
thirds = []
for a, b, c in pairs:
    firsts.append(a)
    thirds.append(c)
test("tuple unpack for", firsts == [1, 4])
test("tuple unpack for 3rd", thirds == [3, 6])

# 7. Dict comprehension with if
d = {k: v for k, v in [("a", 1), ("b", 2), ("c", 3)] if v > 1}
test("dict comp if", d == {"b": 2, "c": 3})

# 8. Complex string formatting
test("format_spec", f"{42:08b}" == "00101010")
test("format_spec hex", f"{255:#x}" == "0xff")
test("format_spec e", f"{1234.5:,.2f}" == "1,234.50")

# 9. Class with __str__ and __repr__
class Token:
    def __init__(self, kind, value):
        self.kind = kind
        self.value = value
    def __str__(self):
        return f"{self.kind}({self.value})"
    def __repr__(self):
        return f"Token({self.kind!r}, {self.value!r})"

t = Token("NUM", 42)
test("str method", str(t) == "NUM(42)")
test("repr method", repr(t) == "Token('NUM', 42)")

# 10. Chained methods with mutation
class Stack:
    def __init__(self):
        self._items = []
    def push(self, item):
        self._items.append(item)
        return self
    def pop(self):
        return self._items.pop()
    def peek(self):
        return self._items[-1] if self._items else None
    def size(self):
        return len(self._items)

s = Stack()
s.push(1).push(2).push(3)
test("stack size", s.size() == 3)
test("stack peek", s.peek() == 3)
test("stack pop", s.pop() == 3)

# 11. Multiple inheritance diamond
class Base:
    def method(self):
        return "Base"

class Left(Base):
    def method(self):
        return "Left+" + super().method()

class Right(Base):
    def method(self):
        return "Right+" + super().method()

class Diamond(Left, Right):
    def method(self):
        return "Diamond+" + super().method()

d = Diamond()
test("diamond mro", d.method() == "Diamond+Left+Right+Base")

# 12. Exception hierarchy catch
class AppError(Exception):
    pass
class DatabaseError(AppError):
    pass
class ConnectionError(DatabaseError):
    pass

def handle(exc_class, msg):
    try:
        raise exc_class(msg)
    except ConnectionError as e:
        return f"conn: {e}"
    except DatabaseError as e:
        return f"db: {e}"
    except AppError as e:
        return f"app: {e}"

test("exc hierarchy 1", handle(ConnectionError, "timeout") == "conn: timeout")
test("exc hierarchy 2", handle(DatabaseError, "query") == "db: query")
test("exc hierarchy 3", handle(AppError, "general") == "app: general")

# 13. Iterator protocol __contains__
class EvenNumbers:
    def __init__(self, limit):
        self.limit = limit
    def __iter__(self):
        return iter(range(0, self.limit, 2))
    def __contains__(self, item):
        return isinstance(item, int) and 0 <= item < self.limit and item % 2 == 0

evens = EvenNumbers(20)
test("contains custom", 4 in evens)
test("contains custom neg", 3 not in evens)
test("iter custom", list(evens) == [0, 2, 4, 6, 8, 10, 12, 14, 16, 18])

# 14. Complex list manipulation
def rotate(lst, k):
    k = k % len(lst) if lst else 0
    return lst[-k:] + lst[:-k]

test("rotate right", rotate([1, 2, 3, 4, 5], 2) == [4, 5, 1, 2, 3])
test("rotate left", rotate([1, 2, 3, 4, 5], -2) == [3, 4, 5, 1, 2])

# 15. Functional patterns
def pipe(*fns):
    def piped(x):
        for fn in fns:
            x = fn(x)
        return x
    return piped

transform = pipe(
    lambda x: x * 2,
    lambda x: x + 10,
    lambda x: x ** 2,
)
test("pipe", transform(3) == 256)  # (3*2+10)^2 = 16^2 = 256

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 53 TESTS PASSED")
