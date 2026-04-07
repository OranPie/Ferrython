# Phase 154: Dunder methods on builtin types, dict/set/list dunders
import sys

passed = 0
failed = 0

def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print(f"FAIL {name}: got {got!r}, expected {expected!r}")

# --- str dunder methods ---
test("str.__format__", "hello".__format__(""), "hello")
test("str.__format__ spec", "hello".__format__(">10"), "     hello")
test("str.__repr__", "hi".__repr__(), "'hi'")
test("str.__len__", "hello".__len__(), 5)
test("str.__contains__", "hello".__contains__("ell"), True)
test("str.__eq__", "abc".__eq__("abc"), True)
test("str.__ne__", "abc".__ne__("xyz"), True)
test("str.__add__", "hello".__add__(" world"), "hello world")
test("str.__mul__", "ab".__mul__(3), "ababab")

# --- int dunder methods ---
test("int.__format__", (42).__format__(""), "42")
test("int.__format__ d", (42).__format__("d"), "42")
test("int.__str__", (42).__str__(), "42")
test("int.__repr__", (42).__repr__(), "42")
test("int.__eq__", (42).__eq__(42), True)
test("int.__lt__", (10).__lt__(20), True)
test("int.__add__", (40).__add__(2), 42)
test("int.__mul__", (6).__mul__(7), 42)
test("int.__sub__", (50).__sub__(8), 42)
test("int.__bool__", (0).__bool__(), False)
test("int.__abs__", (-5).__abs__(), 5)
test("int.__neg__", (5).__neg__(), -5)

# --- float dunder methods ---
test("float.__format__", (3.14).__format__(".2f"), "3.14")
test("float.__str__", (3.14).__str__(), "3.14")
test("float.__eq__", (3.14).__eq__(3.14), True)
test("float.__add__", (1.5).__add__(2.5), 4.0)
test("float.__abs__", (-2.5).__abs__(), 2.5)
test("float.__round__", (3.14159).__round__(2), 3.14)
test("float.__bool__", (0.0).__bool__(), False)
test("float.__neg__", (5.0).__neg__(), -5.0)

# --- dict dunder methods ---
d = {'a': 1, 'b': 2}
test("dict.__contains__", d.__contains__('a'), True)
test("dict.__contains__ miss", d.__contains__('z'), False)
test("dict.__len__", d.__len__(), 2)
test("dict.__getitem__", d.__getitem__('b'), 2)
test("dict.__bool__", d.__bool__(), True)
test("dict.__bool__ empty", {}.__bool__(), False)

# --- set dunder methods ---
s = {1, 2, 3}
test("set.__contains__", s.__contains__(2), True)
test("set.__contains__ miss", s.__contains__(5), False)
test("set.__len__", s.__len__(), 3)
test("set.__bool__", s.__bool__(), True)

# --- list dunder methods ---
lst = [10, 20, 30]
test("list.__contains__", lst.__contains__(20), True)
test("list.__len__", lst.__len__(), 3)

# --- tuple dunder methods ---
t = (1, 2, 3)
test("tuple.__contains__", t.__contains__(2), True)
test("tuple.__contains__ miss", t.__contains__(5), False)

print(f"\ntest_phase154: {passed} passed, {failed} failed")
if failed:
    sys.exit(1)
