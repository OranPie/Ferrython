"""Test suite 29: File I/O, advanced iteration, numeric edge cases"""
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── File I/O ──
import os

# Write and read
with open("/tmp/ferrython_test.txt", "w") as f:
    f.write("Hello, World!\n")
    f.write("Line 2\n")

with open("/tmp/ferrython_test.txt", "r") as f:
    content = f.read()

test("file write read", content == "Hello, World!\nLine 2\n")

# Read lines
with open("/tmp/ferrython_test.txt", "r") as f:
    lines = f.readlines()

test("readlines", lines == ["Hello, World!\n", "Line 2\n"])

# Append mode
with open("/tmp/ferrython_test.txt", "a") as f:
    f.write("Line 3\n")

with open("/tmp/ferrython_test.txt", "r") as f:
    content = f.read()

test("append", content == "Hello, World!\nLine 2\nLine 3\n")

# Clean up
os.remove("/tmp/ferrython_test.txt")

# ── Numeric edge cases ──
test("int max", 2**63 - 1 > 0)  # should handle large ints
test("int neg", -2**63 < 0)
test("float inf", float("inf") > 1e308)
test("float neg inf", float("-inf") < -1e308)
test("float nan neq", float("nan") != float("nan"))
test("int from float", int(3.14) == 3)
test("int from str", int("42") == 42)
test("float from int", float(42) == 42.0)
test("float from str", float("3.14") == 3.14)

# ── Bitwise operations ──
test("bit and", 0xFF & 0x0F == 0x0F)
test("bit or", 0xF0 | 0x0F == 0xFF)
test("bit xor", 0xFF ^ 0x0F == 0xF0)
test("bit not", ~0 == -1)
test("bit lshift", 1 << 10 == 1024)
test("bit rshift", 1024 >> 5 == 32)

# ── String formatting edge cases ──
test("fstr expr", f"{'hello':>10}" == "     hello")
test("fstr num", f"{42:05d}" == "00042")
test("fstr float", f"{3.14:.1f}" == "3.1")
test("fstr comma", f"{1000000:,}" == "1,000,000")

# ── Advanced iteration ──
# zip with different lengths
test("zip short", list(zip([1,2,3], [4,5])) == [(1,4), (2,5)])

# enumerate
test("enumerate", list(enumerate("abc")) == [(0, "a"), (1, "b"), (2, "c")])

# reversed
test("reversed list", list(reversed([1, 2, 3])) == [3, 2, 1])
test("reversed str", list(reversed("abc")) == ["c", "b", "a"])
test("reversed range", list(reversed(range(5))) == [4, 3, 2, 1, 0])

# ── min/max with key ──
words = ["hello", "hi", "hey", "howdy"]
test("min key", min(words, key=len) == "hi")
test("max key", len(max(words, key=len)) == 5)  # "hello" or "howdy"

# ── any/all with generators ──
test("any gen", any(x > 3 for x in [1, 2, 3, 4, 5]))
test("all gen", all(x > 0 for x in [1, 2, 3, 4, 5]))
test("not all gen", not all(x > 3 for x in [1, 2, 3, 4, 5]))

# ── sum with start ──
test("sum start", sum([1, 2, 3], 10) == 16)
test("sum empty", sum([], 0) == 0)

# ── sorted with key and reverse ──
data = [("b", 2), ("a", 1), ("c", 3)]
test("sorted key tuple", sorted(data, key=lambda x: x[1]) == [("a", 1), ("b", 2), ("c", 3)])

# ── dict.fromkeys ──
test("fromkeys", dict.fromkeys(["a", "b", "c"], 0) == {"a": 0, "b": 0, "c": 0})
test("fromkeys none", dict.fromkeys(["x", "y"]) == {"x": None, "y": None})

# ── Multiple assignment ──
a = b = c = 10
test("multi assign", a == 10 and b == 10 and c == 10)

# ── Augmented assignment with lists ──
lst = [1, 2]
lst += [3, 4]
test("list iadd", lst == [1, 2, 3, 4])
lst *= 2
test("list imul", lst == [1, 2, 3, 4, 1, 2, 3, 4])

# ── String multiplication ──
test("str mul", "ab" * 3 == "ababab")
test("str mul zero", "hello" * 0 == "")

# ── Boolean context ──
test("empty str false", not "")
test("nonempty str true", bool("x"))
test("zero false", not 0)
test("nonzero true", bool(1))
test("empty list false", not [])
test("nonempty list true", bool([1]))
test("none false", not None)

# ── Complex dict patterns ──
# Merge dicts
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
merged = {**d1, **d2}
test("dict merge", merged == {"a": 1, "b": 3, "c": 4})

# Dict with tuple keys
coords = {(0, 0): "origin", (1, 0): "right", (0, 1): "up"}
test("tuple key", coords[(0, 0)] == "origin")

# ── Nested function scoping ──
def make_counter(start=0):
    count = [start]
    def increment():
        count[0] += 1
        return count[0]
    def get():
        return count[0]
    return increment, get

inc, get = make_counter(10)
test("counter inc", inc() == 11)
test("counter inc2", inc() == 12)
test("counter get", get() == 12)

# ── Generator with yield from ──
def flatten(nested):
    for item in nested:
        if isinstance(item, list):
            yield from flatten(item)
        else:
            yield item

test("flatten", list(flatten([1, [2, 3], [4, [5, 6]], 7])) == [1, 2, 3, 4, 5, 6, 7])

# ── while/else ──
i = 0
while i < 5:
    i += 1
else:
    completed = True
test("while else", completed and i == 5)

# ── for/else ──
for x in range(5):
    if x == 10:  # never true
        break
else:
    for_completed = True
test("for else", for_completed)

found = False
for x in range(5):
    if x == 3:
        found = True
        break
else:
    found = False  # should not reach here
test("for break", found)

# ── assert statement ──
try:
    assert True  # should not raise
    test("assert true", True)
except AssertionError:
    test("assert true", False)

try:
    assert False, "custom message"
    test("assert false", False)
except AssertionError as e:
    test("assert false", "custom message" in str(e))

# ── del statement ──
d = {"a": 1, "b": 2, "c": 3}
del d["b"]
test("del dict", d == {"a": 1, "c": 3})

lst = [1, 2, 3, 4, 5]
del lst[2]
test("del list", lst == [1, 2, 4, 5])

# ── in/not in ──
test("in list", 3 in [1, 2, 3, 4])
test("not in list", 5 not in [1, 2, 3, 4])
test("in str", "ell" in "hello")
test("not in str", "xyz" not in "hello")
test("in dict", "a" in {"a": 1, "b": 2})
test("in set", 3 in {1, 2, 3})
test("in range", 5 in range(10))
test("not in range", 15 not in range(10))

# ── Chained attribute access ──
class A:
    class B:
        class C:
            value = 42

test("chain attr", A.B.C.value == 42)

# ── Lambda patterns ──
test("lambda sort", sorted([(1, "b"), (3, "a"), (2, "c")], key=lambda x: x[0]) == [(1, "b"), (2, "c"), (3, "a")])
test("lambda filter", list(filter(lambda x: x.startswith("a"), ["apple", "banana", "avocado"])) == ["apple", "avocado"])

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
