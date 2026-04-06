# Phase 35: datetime, enum, __format__, weakref, textwrap, decimal, abc
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── datetime module ──
import datetime

now = datetime.datetime.now()
test("datetime.now year", now.year >= 2024)
test("datetime.now month", 1 <= now.month <= 12)
test("datetime.now day", 1 <= now.day <= 31)
test("datetime.now hour", 0 <= now.hour <= 23)

today = datetime.date.today()
test("date.today year", today.year >= 2024)
test("date.today month", 1 <= today.month <= 12)

td = datetime.timedelta(1, 3600)
test("timedelta days", td.days == 1)
test("timedelta seconds", td.seconds == 3600)

dt = datetime.datetime.fromisoformat("2023-06-15T10:30:00")
test("fromisoformat year", dt.year == 2023)
test("fromisoformat month", dt.month == 6)
test("fromisoformat day", dt.day == 15)
test("fromisoformat hour", dt.hour == 10)
test("fromisoformat minute", dt.minute == 30)

# ── enum module ──
from enum import Enum, auto

class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3

test("enum RED value", Color.RED.value == 1)
test("enum GREEN value", Color.GREEN.value == 2)
test("enum BLUE value", Color.BLUE.value == 3)

class Direction(Enum):
    NORTH = auto()
    SOUTH = auto()
    EAST = auto()
    WEST = auto()

# auto() should give incrementing values
test("auto north", Direction.NORTH.value >= 1)
test("auto south", Direction.SOUTH.value >= 2)

# ── __format__ protocol ──
class Currency:
    def __init__(self, amount, symbol="$"):
        self.amount = amount
        self.symbol = symbol
    
    def __format__(self, spec):
        if spec == "":
            return f"{self.symbol}{self.amount:.2f}"
        return f"{self.symbol}{self.amount:{spec}}"

c = Currency(42.5)
test("__format__ no spec", f"{c}" == "$42.50")
test("__format__ with spec", f"{c:.1f}" == "$42.5")

# ── textwrap ──
import textwrap

wrapped = textwrap.wrap("The quick brown fox jumps over the lazy dog", 20)
test("textwrap.wrap", len(wrapped) > 1)
test("textwrap.wrap lines short", all(len(l) <= 20 for l in wrapped))

filled = textwrap.fill("The quick brown fox jumps over the lazy dog", 20)
test("textwrap.fill", "\n" in filled)

dedented = textwrap.dedent("    hello\n    world")
test("textwrap.dedent", dedented == "hello\nworld")

indented = textwrap.indent("hello\nworld", "  ")
test("textwrap.indent", indented == "  hello\n  world")

# ── abc module ──
import abc

test("abc.abstractmethod exists", callable(abc.abstractmethod))

@abc.abstractmethod
def my_abstract():
    pass
test("abstractmethod returns marker", my_abstract is not None)

# ── decimal module ──
import decimal

d = decimal.Decimal(42)
test("decimal from int", d == 42.0)

d2 = decimal.Decimal("3.14")
test("decimal from str", abs(d2 - 3.14) < 0.001)

# ── weakref module ──
import weakref

class Obj:
    def __init__(self, val):
        self.val = val

o = Obj(42)
r = weakref.ref(o)
# Calling a weakref returns the referent (or None if dead)
obj = r()
test("weakref.ref callable", obj is not None)
test("weakref.ref value", obj.val == 42)
test("weakref.ref same object", obj is o)

# ── bytes operations ──
b = b"Hello, World!"
test("bytes decode", b.decode() == "Hello, World!")
test("bytes upper", b.upper() == b"HELLO, WORLD!")
test("bytes lower", b.lower() == b"hello, world!")
test("bytes split", b.split(b", ") == [b"Hello", b"World!"])
test("bytes startswith", b.startswith(b"Hello"))
test("bytes endswith", b.endswith(b"World!"))
test("bytes find", b.find(b"World") == 7)
test("bytes replace", b.replace(b"World", b"Python") == b"Hello, Python!")
test("bytes join", b", ".join([b"a", b"b", b"c"]) == b"a, b, c")
test("bytes hex", b"\xde\xad".hex() == "dead")

# ── dict methods ──
d = {"a": 1, "b": 2, "c": 3}
test("dict.get", d.get("a") == 1)
test("dict.get default", d.get("z", 42) == 42)
test("dict.keys", sorted(d.keys()) == ["a", "b", "c"])
test("dict.values", sorted(d.values()) == [1, 2, 3])
test("dict.items", sorted(d.items()) == [("a", 1), ("b", 2), ("c", 3)])
d2 = d.copy()
test("dict.copy", d2 == d)
test("dict.copy independent", d2 is not d)
d.update({"d": 4})
test("dict.update", d["d"] == 4)
test("dict.pop", d.pop("d") == 4)
test("dict.pop gone", "d" not in d)
test("dict.setdefault", d.setdefault("e", 5) == 5)
test("dict.setdefault existing", d.setdefault("a", 99) == 1)

# ── set operations ──
s1 = {1, 2, 3, 4}
s2 = {3, 4, 5, 6}
test("set union", s1 | s2 == {1, 2, 3, 4, 5, 6})
test("set intersection", s1 & s2 == {3, 4})
test("set difference", s1 - s2 == {1, 2})
test("set symmetric_difference", s1 ^ s2 == {1, 2, 5, 6})
test("set issubset", {1, 2}.issubset(s1))
test("set issuperset", s1.issuperset({1, 2}))
test("set isdisjoint", s1.isdisjoint({7, 8}))

# ── list methods ──
lst = [3, 1, 4, 1, 5, 9, 2, 6]
test("list.count", lst.count(1) == 2)
test("list.index", lst.index(4) == 2)
lst2 = lst.copy()
lst2.sort()
test("list.sort", lst2 == [1, 1, 2, 3, 4, 5, 6, 9])
lst2.reverse()
test("list.reverse", lst2 == [9, 6, 5, 4, 3, 2, 1, 1])
lst3 = [1, 2, 3]
lst3.extend([4, 5])
test("list.extend", lst3 == [1, 2, 3, 4, 5])
lst3.insert(0, 0)
test("list.insert", lst3 == [0, 1, 2, 3, 4, 5])
lst3.remove(0)
test("list.remove", lst3 == [1, 2, 3, 4, 5])

# ── string methods ──
s = "Hello, World!"
test("str.upper", s.upper() == "HELLO, WORLD!")
test("str.lower", s.lower() == "hello, world!")
test("str.strip", "  hello  ".strip() == "hello")
test("str.lstrip", "  hello  ".lstrip() == "hello  ")
test("str.rstrip", "  hello  ".rstrip() == "  hello")
test("str.split", "a,b,c".split(",") == ["a", "b", "c"])
test("str.join", ",".join(["a", "b", "c"]) == "a,b,c")
test("str.replace", s.replace("World", "Python") == "Hello, Python!")
test("str.startswith", s.startswith("Hello"))
test("str.endswith", s.endswith("World!"))
test("str.find", s.find("World") == 7)
test("str.count", "aabaa".count("a") == 4)
test("str.isdigit", "123".isdigit())
test("str.isalpha", "abc".isalpha())
test("str.center", "hi".center(10, "-") == "----hi----")
test("str.ljust", "hi".ljust(5, "-") == "hi---")
test("str.rjust", "hi".rjust(5, "-") == "---hi")
test("str.zfill", "42".zfill(5) == "00042")
test("str.title", "hello world".title() == "Hello World")
test("str.swapcase", "Hello".swapcase() == "hELLO")
test("str.capitalize", "hello world".capitalize() == "Hello world")

# ── tuple operations ──
t = (1, 2, 3, 2, 1)
test("tuple.count", t.count(2) == 2)
test("tuple.index", t.index(3) == 2)
test("tuple + tuple", (1, 2) + (3, 4) == (1, 2, 3, 4))
test("tuple * int", (1, 2) * 3 == (1, 2, 1, 2, 1, 2))

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
assert failed == 0, f"{failed} tests failed!"
print("ALL PHASE 35 TESTS PASSED")
