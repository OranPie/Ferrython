# test_phase27.py — Expanded stdlib: time, random, sys, platform, locale, inspect

passed = 0
failed = 0

def assert_test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name)

# ── time module ──
import time

t1 = time.time()
assert_test("time.time type", isinstance(t1, float))
assert_test("time.time positive", t1 > 1000000000)

t2 = time.monotonic()
assert_test("time.monotonic type", isinstance(t2, float))

t3 = time.perf_counter()
assert_test("time.perf_counter type", isinstance(t3, float))

# strftime
formatted = time.strftime("%Y-%m-%d")
assert_test("time.strftime date", len(formatted) == 10 and "-" in formatted)

formatted2 = time.strftime("%H:%M:%S")
assert_test("time.strftime time", ":" in formatted2)

# localtime
lt = time.localtime()
assert_test("time.localtime tuple", len(lt) == 9)
assert_test("time.localtime year", lt[0] >= 2024)

# ── random module ──
import random

r = random.random()
assert_test("random.random range", 0.0 <= r <= 1.0)

ri = random.randint(1, 10)
assert_test("random.randint range", 1 <= ri <= 10)

c = random.choice([10, 20, 30])
assert_test("random.choice in list", c in [10, 20, 30])

u = random.uniform(1.0, 2.0)
assert_test("random.uniform range", 1.0 <= u <= 2.0)

s = random.sample([1, 2, 3, 4, 5], 3)
assert_test("random.sample len", len(s) == 3)
assert_test("random.sample subset", all(x in [1, 2, 3, 4, 5] for x in s))

rr = random.randrange(10)
assert_test("random.randrange", 0 <= rr < 10)

rr2 = random.randrange(5, 15)
assert_test("random.randrange 2", 5 <= rr2 < 15)

# ── sys module ──
import sys

assert_test("sys.version", "ferrython" in sys.version)
assert_test("sys.version_info", sys.version_info[0] == 3 and sys.version_info[1] == 8)
assert_test("sys.platform", isinstance(sys.platform, str))
assert_test("sys.maxsize", sys.maxsize > 2**30)
assert_test("sys.maxunicode", sys.maxunicode == 0x10FFFF)
assert_test("sys.byteorder", sys.byteorder in ("little", "big"))
assert_test("sys.getdefaultencoding", sys.getdefaultencoding() == "utf-8")
assert_test("sys.getfilesystemencoding", sys.getfilesystemencoding() == "utf-8")
assert_test("sys.getrecursionlimit", sys.getrecursionlimit() == 1000)
assert_test("sys.getsizeof", sys.getsizeof(42) > 0)
assert_test("sys.intern", sys.intern("hello") == "hello")
assert_test("sys.float_info", len(sys.float_info) >= 8)
assert_test("sys.__debug__", sys.__debug__ == True)

# ── platform module ──
import platform

assert_test("platform.system", isinstance(platform.system(), str))
assert_test("platform.machine", isinstance(platform.machine(), str))
assert_test("platform.python_version", platform.python_version() == "3.8.0")
assert_test("platform.python_implementation", platform.python_implementation() == "Ferrython")
assert_test("platform.architecture", len(platform.architecture()) == 2)

# ── locale module ──
import locale

loc = locale.getlocale()
assert_test("locale.getlocale", len(loc) == 2)
assert_test("locale.getpreferredencoding", locale.getpreferredencoding() == "UTF-8")
assert_test("locale.LC_ALL", isinstance(locale.LC_ALL, int))

# ── inspect module ──
import inspect

def my_func():
    pass

class MyClass:
    pass

assert_test("inspect.isfunction", inspect.isfunction(my_func))
assert_test("inspect.isfunction false", not inspect.isfunction(42))
assert_test("inspect.isclass", inspect.isclass(MyClass))
assert_test("inspect.isclass false", not inspect.isclass(my_func))

# ── Advanced: enumerate with different starts ──
r1 = list(enumerate(["a", "b", "c"]))
assert_test("enumerate default", r1 == [(0, "a"), (1, "b"), (2, "c")])

r2 = list(enumerate(["x", "y"], 5))
assert_test("enumerate start=5", r2 == [(5, "x"), (6, "y")])

# ── Complex string formatting ──
assert_test("format int padding", f"{'hello':>10}" == "     hello")
assert_test("format int padding 2", f"{'hi':<10}" == "hi        ")
assert_test("format int padding 3", f"{'hi':^10}" == "    hi    ")
assert_test("format int fill", f"{'hi':*>10}" == "********hi")

# ── Nested dict/list operations ──
data = {"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}
names = [u["name"] for u in data["users"]]
assert_test("nested data access", names == ["Alice", "Bob"])
ages = sorted([u["age"] for u in data["users"]])
assert_test("nested sorted", ages == [25, 30])

# ── Exception handling advanced ──
class CustomError(Exception):
    pass

class SpecificError(CustomError):
    pass

try:
    raise SpecificError("test")
except CustomError as e:
    assert_test("catch subclass", str(e) == "test")

try:
    raise ValueError("bad")
except (TypeError, ValueError) as e:
    assert_test("multi except", str(e) == "bad")

# ── Generator expression ──
gen_sum = sum(x * x for x in range(5))
assert_test("gen expr sum", gen_sum == 30)

# ── Dict merge and update ──
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
merged = {**d1, **d2}
assert_test("dict merge unpack", merged == {"a": 1, "b": 3, "c": 4})

# ── isinstance with builtins ──
assert_test("isinstance int", isinstance(42, int))
assert_test("isinstance str", isinstance("hello", str))
assert_test("isinstance list", isinstance([1, 2], list))
assert_test("isinstance dict", isinstance({}, dict))
assert_test("isinstance float", isinstance(3.14, float))
assert_test("isinstance bool", isinstance(True, bool))
assert_test("isinstance tuple", isinstance((1, 2), tuple))

# ── map/filter with lambda ──
result = list(map(lambda x: x * 2, [1, 2, 3]))
assert_test("map lambda", result == [2, 4, 6])

result2 = list(filter(lambda x: x > 2, [1, 2, 3, 4, 5]))
assert_test("filter lambda", result2 == [3, 4, 5])

print()
print("=" * 40)
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(failed, "TESTS FAILED!")
