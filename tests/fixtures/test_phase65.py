# test_phase65.py — Bytecode caching for imports & descriptor protocol edge cases

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name)

# ════════════════════════════════════════════════════
# Part 1: Import caching — import same module twice
# ════════════════════════════════════════════════════

import sys
test("sys import 1", sys is not None)

# Second import of the same module should reuse cached bytecode
import sys as sys2
test("sys import 2 same object", sys is sys2)

# Import another stdlib module twice
import math
val1 = math.pi
import math as math2
val2 = math2.pi
test("math double import same pi", val1 == val2)
test("math double import same object", math is math2)

# collections imported twice
import collections
od1 = collections.OrderedDict
import collections as collections2
od2 = collections2.OrderedDict
test("collections double import", od1 is od2)

# ════════════════════════════════════════════════════
# Part 2: Descriptor protocol — data vs non-data
# ════════════════════════════════════════════════════

# --- 2a: Data descriptor (has __get__ and __set__) beats instance dict ---

class DataDesc:
    """A data descriptor: has both __get__ and __set__."""
    def __init__(self, initial):
        self.value = initial

    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return self.value

    def __set__(self, obj, value):
        self.value = value

class WithData:
    attr = DataDesc(42)

wd = WithData()
test("data desc initial", wd.attr == 42)

# Even after writing to instance dict, data descriptor should win
wd.__dict__["attr"] = 999
test("data desc beats instance dict", wd.attr == 42)

# Setting through descriptor protocol
wd.attr = 100
test("data desc set", wd.attr == 100)
# Instance dict should still have the old value (descriptor intercepts)
test("data desc instance dict unchanged", wd.__dict__.get("attr") == 999)

# --- 2b: Non-data descriptor (only __get__) loses to instance dict ---

class NonDataDesc:
    """A non-data descriptor: has __get__ but NOT __set__."""
    def __init__(self, val):
        self.val = val

    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return self.val

class WithNonData:
    attr = NonDataDesc("from descriptor")

wn = WithNonData()
test("non-data desc initial", wn.attr == "from descriptor")

# Writing to instance dict should shadow the non-data descriptor
wn.__dict__["attr"] = "from instance"
test("non-data desc loses to instance dict", wn.attr == "from instance")

# --- 2c: Data descriptor with __delete__ ---

class DeleteDesc:
    """Data descriptor with __get__ and __delete__ (no __set__)."""
    def __init__(self):
        self.deleted = False

    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        if self.deleted:
            return "deleted"
        return "present"

    def __delete__(self, obj):
        self.deleted = True

class WithDelete:
    attr = DeleteDesc()

wdel = WithDelete()
test("delete desc initial", wdel.attr == "present")

# Has __delete__ → data descriptor → beats instance dict
wdel.__dict__["attr"] = "instance"
test("delete desc beats instance dict", wdel.attr == "present")

# --- 2d: Property is always a data descriptor ---

class WithProp:
    def __init__(self):
        self._val = 10

    @property
    def attr(self):
        return self._val

    @attr.setter
    def attr(self, v):
        self._val = v

wp = WithProp()
test("property initial", wp.attr == 10)
wp.attr = 20
test("property setter", wp.attr == 20)

# --- 2e: Staticmethod/classmethod are non-data descriptors ---
# They should lose to instance dict entries (CPython behavior)

class WithSM:
    @staticmethod
    def foo():
        return "static"

    @classmethod
    def bar(cls):
        return "class"

wsm = WithSM()
test("staticmethod normal", wsm.foo() == "static")
test("classmethod normal", wsm.bar() == "class")

# Override via instance dict — should shadow the staticmethod/classmethod
wsm.__dict__["foo"] = "instance_foo"
test("staticmethod loses to instance dict", wsm.foo == "instance_foo")

wsm.__dict__["bar"] = "instance_bar"
test("classmethod loses to instance dict", wsm.bar == "instance_bar")

# --- 2f: MRO-inherited data descriptor ---

class Base:
    attr = DataDesc("base")

class Child(Base):
    pass

c = Child()
test("inherited data desc", c.attr == "base")
c.__dict__["attr"] = "child instance"
test("inherited data desc beats instance dict", c.attr == "base")

# --- 2g: MRO-inherited non-data descriptor ---

class Base2:
    attr = NonDataDesc("base non-data")

class Child2(Base2):
    pass

c2 = Child2()
test("inherited non-data desc", c2.attr == "base non-data")
c2.__dict__["attr"] = "child2 instance"
test("inherited non-data desc loses to instance dict", c2.attr == "child2 instance")

print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed > 0:
    raise SystemExit(1)
