# Phase 129: Ferryip improvements, bytes.hex(sep), comprehensive edge cases
import sys

results = []

def check(name, result):
    results.append((name, result))

# 1. bytes.hex with separator
b = bytes([0xDE, 0xAD, 0xBE, 0xEF])
check("bytes.hex", b.hex() == "deadbeef")
check("bytes.hex sep", b.hex(":") == "de:ad:be:ef")
check("bytes.hex sep2", b.hex(".", 2) == "dead.beef")

# 2. bytearray.hex with separator
ba = bytearray([0xAB, 0xCD, 0xEF])
check("bytearray.hex sep", ba.hex("-") == "ab-cd-ef")

# 3. Property setter
class Circle:
    def __init__(self, r):
        self._r = r
    @property
    def radius(self):
        return self._r
    @radius.setter
    def radius(self, value):
        if value < 0:
            raise ValueError("negative")
        self._r = value

c = Circle(5)
c.radius = 10
check("property setter", c.radius == 10)
try:
    c.radius = -1
    check("property validate", False)
except ValueError:
    check("property validate", True)

# 4. Exception chaining
try:
    try:
        raise ValueError("original")
    except ValueError:
        raise RuntimeError("chained") from ValueError("cause")
except RuntimeError as e:
    check("exception chain", str(e) == "chained" and e.__cause__ is not None)

# 5. Context manager suppress
class Suppressor:
    def __enter__(self):
        return self
    def __exit__(self, *args):
        return True
with Suppressor():
    raise ValueError("suppressed")
check("exception suppress", True)

# 6. Multiple decorators
def bold(fn):
    def wrapper():
        return "<b>" + fn() + "</b>"
    return wrapper
def italic(fn):
    def wrapper():
        return "<i>" + fn() + "</i>"
    return wrapper

@bold
@italic
def greet():
    return "hello"
check("multi decorator", greet() == "<b><i>hello</i></b>")

# 7. Generator finally on close
log = []
def gen():
    try:
        yield 1
        yield 2
    finally:
        log.append("finally")
g = gen()
next(g)
g.close()
check("gen finally close", log == ["finally"])

# 8. Nonlocal counter
def make_counter():
    count = 0
    def increment():
        nonlocal count
        count += 1
        return count
    return increment
counter = make_counter()
check("nonlocal", [counter(), counter(), counter()] == [1, 2, 3])

# 9. Complex slice assignment
a = [0, 1, 2, 3, 4, 5]
a[1:4] = [10, 20]
check("slice assign", a == [0, 10, 20, 4, 5])

# 10. dict merge operators
d1 = {"a": 1}
d2 = {"b": 2}
d3 = d1 | d2
check("dict merge", d3 == {"a": 1, "b": 2})
d1 |= {"c": 3}
check("dict ior", d1 == {"a": 1, "c": 3})

# 11. Walrus operator
if (n := 42) > 0:
    check("walrus", n == 42)

# 12. Extended star unpacking
a, *b, c = [1, 2, 3, 4, 5]
check("star unpack", a == 1 and b == [2, 3, 4] and c == 5)

# 13. zip strict
try:
    list(zip([1, 2], [3, 4, 5], strict=True))
    check("zip strict", False)
except ValueError:
    check("zip strict", True)

# Print results
for name, passed in results:
    status = "PASS" if passed else "FAIL"
    print(f"  {name}: {status}")

failed = [name for name, passed in results if not passed]
if failed:
    print(f"FAILED: {failed}")
    sys.exit(1)
print(f"phase129: All {len(results)} checks passed")
