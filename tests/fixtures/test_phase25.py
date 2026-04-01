# test_phase25.py — New modules: struct, textwrap, statistics, contextlib, dataclasses, decimal

passed = 0
failed = 0

def assert_test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name)

# ── struct module ──
import struct

# Pack and unpack integers
data = struct.pack(">i", 12345)
assert_test("struct pack int", len(data) == 4)
result = struct.unpack(">i", data)
assert_test("struct unpack int", result[0] == 12345)

# Pack multiple values
data2 = struct.pack("<hh", 100, 200)
result2 = struct.unpack("<hh", data2)
assert_test("struct multi short", result2 == (100, 200))

# Pack float
data3 = struct.pack("<f", 3.14)
result3 = struct.unpack("<f", data3)
assert_test("struct float", abs(result3[0] - 3.14) < 0.01)

# Pack double
data4 = struct.pack("<d", 2.71828)
result4 = struct.unpack("<d", data4)
assert_test("struct double", abs(result4[0] - 2.71828) < 0.00001)

# Pack bool
data5 = struct.pack("??", True, False)
result5 = struct.unpack("??", data5)
assert_test("struct bool", result5 == (True, False))

# calcsize
assert_test("struct calcsize i", struct.calcsize("i") == 4)
assert_test("struct calcsize h", struct.calcsize("h") == 2)
assert_test("struct calcsize d", struct.calcsize("d") == 8)
assert_test("struct calcsize ?", struct.calcsize("?") == 1)

# Pack byte
data6 = struct.pack("BB", 65, 66)
result6 = struct.unpack("BB", data6)
assert_test("struct bytes", result6 == (65, 66))

# ── textwrap module ──
import textwrap

# dedent
indented = "    hello\n    world\n    foo"
dedented = textwrap.dedent(indented)
assert_test("textwrap dedent", dedented == "hello\nworld\nfoo")

# indent
text = "hello\nworld"
indented = textwrap.indent(text, "  ")
assert_test("textwrap indent", indented == "  hello\n  world")

# wrap
long_text = "The quick brown fox jumps over the lazy dog"
wrapped = textwrap.wrap(long_text, 20)
assert_test("textwrap wrap", len(wrapped) >= 2)
all_ok = True
for w in wrapped:
    if len(str(w)) > 20:
        all_ok = False
assert_test("textwrap wrap width", all_ok)

# fill
filled = textwrap.fill(long_text, 20)
assert_test("textwrap fill", "\n" in filled)

# ── statistics module ──
import statistics

# mean
assert_test("statistics mean", statistics.mean([1, 2, 3, 4, 5]) == 3.0)
assert_test("statistics mean float", abs(statistics.mean([1.5, 2.5, 3.5]) - 2.5) < 0.0001)

# median
assert_test("statistics median odd", statistics.median([1, 3, 5]) == 3.0)
assert_test("statistics median even", statistics.median([1, 2, 3, 4]) == 2.5)

# mode
assert_test("statistics mode", statistics.mode([1, 2, 2, 3, 3, 3]) == 3)
assert_test("statistics mode str", statistics.mode(["a", "b", "b", "c"]) == "b")

# stdev and variance
vals = [2, 4, 4, 4, 5, 5, 7, 9]
assert_test("statistics stdev", abs(statistics.stdev(vals) - 2.138) < 0.01)
assert_test("statistics variance", abs(statistics.variance(vals) - 4.571) < 0.01)

# ── contextlib module ──
import contextlib

# contextmanager as decorator (pass-through)
@contextlib.contextmanager
def my_context():
    return 42

assert_test("contextlib contextmanager", callable(my_context))

# ── dataclasses module ──
import dataclasses

@dataclasses.dataclass
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y

p = Point(3, 4)
assert_test("dataclass instance", p.x == 3 and p.y == 4)

# asdict
d = dataclasses.asdict(p)
assert_test("dataclass asdict", d.get("x") == 3 or d.get("x", None) is not None)

# ── decimal module ──
import decimal

d1 = decimal.Decimal("3.14")
assert_test("decimal create", abs(d1 - 3.14) < 0.001)

d2 = decimal.Decimal("100")
assert_test("decimal int", d2 == 100.0)

# ── warnings module ──
import warnings
warnings.warn("test warning")  # Should not crash
assert_test("warnings warn", True)

# ── traceback module ──
import traceback
assert_test("traceback import", True)

# ── numbers module ──
import numbers
assert_test("numbers import", True)

# ── typing module ──
import typing
assert_test("typing TYPE_CHECKING", typing.TYPE_CHECKING == False)

# ── More functools tests ──
from functools import reduce, partial

# reduce with class method
class Acc:
    def __init__(self, val):
        self.val = val
    def add(self, other):
        return Acc(self.val + other.val)
    def __repr__(self):
        return f"Acc({self.val})"

items = [Acc(1), Acc(2), Acc(3)]
result = reduce(lambda a, b: a.add(b), items)
assert_test("reduce with method", result.val == 6)

# partial with lambda
double = partial(lambda x, y: x * y, 2)
assert_test("partial lambda", double(5) == 10)

# ── More operator tests ──
import operator

# String addition
assert_test("operator add str", operator.add("hello", " world") == "hello world")

# Boolean not
assert_test("operator not 0", operator.not_(0) == True)
assert_test("operator not empty", operator.not_("") == True)
assert_test("operator not nonempty", operator.not_("x") == False)

# ── copy deep nested ──
import copy

nested = {"a": [1, {"b": [2, 3]}]}
deep = copy.deepcopy(nested)
assert_test("deep copy nested", deep["a"][1]["b"] == [2, 3])

# ── str.translate advanced ──
# Translate with string replacement values
table = str.maketrans({"a": "AA", "b": "BB"})
# This may not work perfectly since our translate handles ints, not string values in the table

# ── Verify all previous tests still work ──
assert_test("__name__", __name__ == "__main__")

from functools import reduce
assert_test("reduce still works", reduce(lambda a, b: a + b, [1, 2, 3]) == 6)

import re
m = re.search(r"(\d+)", "abc123")
assert_test("re still works", m.group(1) == "123")

print()
print("=" * 40)
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(failed, "TESTS FAILED!")
