# ═══════════════════════════════════════════
# Phase 67 Tests — __slots__, __class_getitem__,
# and common builtins (CPython alignment)
# ═══════════════════════════════════════════

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL: " + name)

# ── __slots__ basic ──

class Point:
    __slots__ = ("x", "y")
    def __init__(self, x, y):
        self.x = x
        self.y = y

p = Point(1, 2)
test("slots_basic_x", p.x == 1)
test("slots_basic_y", p.y == 2)

# Setting a valid slot attribute
p.x = 10
test("slots_set_valid", p.x == 10)

# Setting an invalid attribute should raise AttributeError
try:
    p.z = 99
    test("slots_reject_invalid", False)
except AttributeError:
    test("slots_reject_invalid", True)

# __dict__ should not be available on slotted instances
try:
    d = p.__dict__
    test("slots_no_dict", False)
except AttributeError:
    test("slots_no_dict", True)

# ── __slots__ with __dict__ included ──

class Flexible:
    __slots__ = ("x", "__dict__")
    def __init__(self, x):
        self.x = x

f = Flexible(5)
f.x = 5
f.extra = 99  # allowed because __dict__ is in __slots__
test("slots_with_dict_allowed", f.extra == 99)

# ── __slots__ inheritance ──

class Point3D(Point):
    __slots__ = ("z",)
    def __init__(self, x, y, z):
        self.x = x
        self.y = y
        self.z = z

p3 = Point3D(1, 2, 3)
test("slots_inherit_x", p3.x == 1)
test("slots_inherit_y", p3.y == 2)
test("slots_inherit_z", p3.z == 3)

# Inherited slots should restrict too
try:
    p3.w = 4
    test("slots_inherit_reject", False)
except AttributeError:
    test("slots_inherit_reject", True)

# ── __slots__ as list ──

class ListSlots:
    __slots__ = ["a", "b"]

ls = ListSlots()
ls.a = 1
ls.b = 2
test("slots_list_form", ls.a == 1 and ls.b == 2)

try:
    ls.c = 3
    test("slots_list_reject", False)
except AttributeError:
    test("slots_list_reject", True)


# ── __class_getitem__ ──

class MyList:
    def __class_getitem__(cls, item):
        return cls

result = MyList[int]
test("class_getitem_basic", result is MyList)

class GenericBox:
    def __class_getitem__(cls, item):
        return "GenericBox[" + item.__name__ + "]"

# Note: int.__name__ may not be available, so test with the class itself
result2 = GenericBox[int]
# The result should be a string containing "GenericBox"
test("class_getitem_custom", "GenericBox" in str(result2))


# ── builtin: breakpoint() ──

# breakpoint() should not crash — it either prints or is a no-op
try:
    breakpoint()
    test("breakpoint_no_crash", True)
except Exception:
    test("breakpoint_no_crash", False)


# ── builtin: __import__() ──

try:
    # __import__ should be callable
    test("import_callable", callable(__import__))
except Exception:
    test("import_callable", False)


# ── builtin: memoryview() ──

try:
    mv = memoryview(b"hello")
    test("memoryview_basic", True)
except Exception:
    test("memoryview_basic", False)


# ── builtin: bytearray() ──

ba = bytearray(b"hello")
test("bytearray_from_bytes", len(ba) == 5)

ba2 = bytearray(3)
test("bytearray_from_int", len(ba2) == 3)

ba3 = bytearray()
test("bytearray_empty", len(ba3) == 0)


# ── builtin: frozenset operations ──

fs1 = frozenset([1, 2, 3])
fs2 = frozenset([2, 3, 4])

# union
u = fs1.union(fs2)
test("frozenset_union", 1 in u and 4 in u)

# intersection
i = fs1.intersection(fs2)
test("frozenset_intersection", 2 in i and 3 in i and 1 not in i)

# difference
d = fs1.difference(fs2)
test("frozenset_difference", 1 in d and 2 not in d)

# issubset / issuperset
test("frozenset_issubset", frozenset([1, 2]).issubset(fs1))
test("frozenset_issuperset", fs1.issuperset(frozenset([1, 2])))

# isdisjoint
test("frozenset_isdisjoint", fs1.isdisjoint(frozenset([5, 6])))
test("frozenset_not_disjoint", not fs1.isdisjoint(fs2))


# ── Summary ──

print("Phase 67: " + str(passed) + " passed, " + str(failed) + " failed")
if failed > 0:
    raise Exception("Phase 67 had " + str(failed) + " failure(s)")
