"""Test phase 125: hashlib.pbkdf2_hmac, scrypt, attrgetter dotted, make_dataclass, deep coverage."""

# --- hashlib.pbkdf2_hmac ---
import hashlib

dk = hashlib.pbkdf2_hmac("sha256", b"password", b"salt", 1000)
assert len(dk) == 32, f"pbkdf2_hmac default dklen: {len(dk)}"

dk16 = hashlib.pbkdf2_hmac("sha256", b"password", b"salt", 1000, 16)
assert len(dk16) == 16, f"pbkdf2_hmac explicit dklen: {len(dk16)}"

dk_sha1 = hashlib.pbkdf2_hmac("sha1", b"pass", b"salt", 100)
assert len(dk_sha1) == 20, f"pbkdf2_hmac sha1: {len(dk_sha1)}"

# deterministic: same params -> same output
dk_a = hashlib.pbkdf2_hmac("sha256", b"test", b"salt", 500)
dk_b = hashlib.pbkdf2_hmac("sha256", b"test", b"salt", 500)
assert dk_a == dk_b, "pbkdf2_hmac deterministic"

# different passwords -> different output
dk_c = hashlib.pbkdf2_hmac("sha256", b"other", b"salt", 500)
assert dk_a != dk_c, "pbkdf2_hmac different passwords"

# --- hashlib.scrypt ---
dk_s = hashlib.scrypt(b"password", b"salt", 2, 1, 1, 32)
assert len(dk_s) == 32, f"scrypt dklen: {len(dk_s)}"

dk_s2 = hashlib.scrypt(b"password", b"salt", 2, 1, 1, 32)
assert dk_s == dk_s2, "scrypt deterministic"

# --- operator.attrgetter dotted ---
import operator

class Inner:
    val = 42

class Outer:
    inner = Inner()

getter = operator.attrgetter("inner.val")
assert getter(Outer()) == 42, "attrgetter dotted"

getter2 = operator.attrgetter("inner.val", "inner")
result = getter2(Outer())
assert result[0] == 42, "attrgetter multi first"
assert isinstance(result[1], Inner), "attrgetter multi second"

# --- dataclasses.make_dataclass ---
import dataclasses

Pt = dataclasses.make_dataclass("Pt", [("x", int), ("y", int)])
p = Pt(1, 2)
assert p.x == 1 and p.y == 2, f"make_dataclass: {p.x}, {p.y}"
assert repr(p) == "Pt(x=1, y=2)", f"make_dataclass repr: {repr(p)}"

# with string-only field names
Pt2 = dataclasses.make_dataclass("Pt2", ["a", "b"])
p2 = Pt2(10, 20)
assert p2.a == 10 and p2.b == 20, "make_dataclass string fields"

# --- comprehensive stdlib depth ---
# collections.ChainMap
import collections
cm = collections.ChainMap({"x": 1}, {"y": 2, "x": 10})
assert cm["x"] == 1 and cm["y"] == 2, "ChainMap"
assert sorted(cm.keys()) == ["x", "y"], "ChainMap keys"

# itertools product/combinations
import itertools
assert list(itertools.product([1,2], [3,4])) == [(1,3),(1,4),(2,3),(2,4)]
assert list(itertools.combinations([1,2,3], 2)) == [(1,2),(1,3),(2,3)]
assert list(itertools.permutations([1,2,3], 2)) == [(1,2),(1,3),(2,1),(2,3),(3,1),(3,2)]

# functools.singledispatch
import functools

@functools.singledispatch
def process(val):
    return "default"

@process.register(int)
def _(val):
    return "int"

@process.register(str)
def _(val):
    return "str"

assert process(42) == "int"
assert process("hi") == "str"
assert process(3.14) == "default"

# contextlib.ExitStack
import contextlib
resources = []
with contextlib.ExitStack() as stack:
    for i in range(3):
        stack.callback(lambda x=i: resources.append(x))
    resources.append("done")
assert resources == ["done", 2, 1, 0], f"ExitStack: {resources}"

# typing features
import typing
T = typing.TypeVar("T", bound=int)
assert T.__bound__ is int, "TypeVar bound"

hints = typing.get_type_hints(type("X", (), {"__annotations__": {"x": int}}))
assert hints == {"x": int}, f"get_type_hints: {hints}"

# dataclass __post_init__
@dataclasses.dataclass
class Rect:
    w: float
    h: float
    area: float = 0.0
    def __post_init__(self):
        self.area = self.w * self.h

r = Rect(3.0, 4.0)
assert r.area == 12.0, f"post_init: {r.area}"

# enum features
import enum
class Priority(enum.IntEnum):
    LOW = 1
    MED = 2
    HIGH = 3

assert Priority.HIGH == 3
assert Priority.HIGH + 1 == 4
assert sorted([Priority.HIGH, Priority.LOW]) == [Priority.LOW, Priority.HIGH]

# abc abstract enforcement
import abc
class Shape(abc.ABC):
    @abc.abstractmethod
    def area(self): pass

class Circle(Shape):
    def __init__(self, r): self.r = r
    def area(self): return 3.14 * self.r * self.r

assert Circle(5).area() == 78.5
try:
    Shape()
    assert False, "Should have raised TypeError"
except TypeError:
    pass

# weakref
import weakref
class Ref:
    pass
obj = Ref()
ref = weakref.ref(obj)
assert ref() is obj, "weakref"

# json roundtrip
import json
d = {"items": [1, None, True, 3.14]}
assert json.loads(json.dumps(d)) == d, "json roundtrip"

# traceback
import traceback
try:
    1/0
except:
    tb = traceback.format_exc()
    assert "ZeroDivisionError" in tb, f"traceback: {tb}"

print("All test_phase125 tests passed!")
