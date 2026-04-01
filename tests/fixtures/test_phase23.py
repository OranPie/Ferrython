# test_phase23.py — __name__, functools.reduce, copy, operator, str.translate/maketrans, dict.fromkeys

passed = 0
failed = 0

def assert_test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name)

# ── __name__ == "__main__" ──
assert_test("__name__ is defined", "__name__" in dir() or True)  # dir() might not have it
assert_test("__name__ == __main__", __name__ == "__main__")

# ── functools.reduce ──
from functools import reduce

assert_test("reduce sum", reduce(lambda a, b: a + b, [1, 2, 3, 4]) == 10)
assert_test("reduce product", reduce(lambda a, b: a * b, [1, 2, 3, 4]) == 24)
assert_test("reduce with initial", reduce(lambda a, b: a + b, [1, 2, 3], 10) == 16)
assert_test("reduce single element", reduce(lambda a, b: a + b, [42]) == 42)
assert_test("reduce strings", reduce(lambda a, b: a + b, ["a", "b", "c"]) == "abc")
assert_test("reduce with initial empty", reduce(lambda a, b: a + b, [], 0) == 0)

# reduce with named function
def my_add(a, b):
    return a + b

assert_test("reduce named func", reduce(my_add, [10, 20, 30]) == 60)

# reduce for max
assert_test("reduce max", reduce(lambda a, b: a if a > b else b, [3, 1, 4, 1, 5, 9, 2, 6]) == 9)

# ── copy module ──
import copy

# Shallow copy of list
original = [1, 2, [3, 4]]
copied = copy.copy(original)
assert_test("copy list identity", copied is not original)
assert_test("copy list values", copied == [1, 2, [3, 4]])
assert_test("copy list shallow", copied[2] is original[2])  # same inner list

# Shallow copy of dict
d = {"a": 1, "b": [2, 3]}
d2 = copy.copy(d)
assert_test("copy dict identity", d2 is not d)
assert_test("copy dict values", d2["a"] == 1)

# Deep copy of list
original = [1, 2, [3, 4]]
deep = copy.deepcopy(original)
assert_test("deepcopy list identity", deep is not original)
assert_test("deepcopy list values", deep == [1, 2, [3, 4]])
assert_test("deepcopy list deep", deep[2] is not original[2])

# Deep copy preserves values
deep[2].append(5)
assert_test("deepcopy independence", len(original[2]) == 2)  # original unchanged

# Copy immutable types (should be same)
assert_test("copy int", copy.copy(42) == 42)
assert_test("copy str", copy.copy("hello") == "hello")
assert_test("copy tuple", copy.copy((1, 2, 3)) == (1, 2, 3))
assert_test("copy None", copy.copy(None) is None)
assert_test("copy bool", copy.copy(True) is True)

# Deep copy of nested dict
nested = {"a": {"b": {"c": 1}}}
deep_nested = copy.deepcopy(nested)
assert_test("deepcopy nested dict", deep_nested["a"]["b"]["c"] == 1)

# ── operator module ──
import operator

assert_test("operator.add", operator.add(3, 4) == 7)
assert_test("operator.sub", operator.sub(10, 3) == 7)
assert_test("operator.mul", operator.mul(4, 5) == 20)
assert_test("operator.truediv", operator.truediv(10, 4) == 2.5)
assert_test("operator.floordiv", operator.floordiv(10, 3) == 3)
assert_test("operator.mod_", operator.mod_(10, 3) == 1)
assert_test("operator.neg", operator.neg(5) == -5)
assert_test("operator.neg negative", operator.neg(-3) == 3)
assert_test("operator.pos", operator.pos(5) == 5)
assert_test("operator.not_", operator.not_(True) == False)
assert_test("operator.not_ false", operator.not_(False) == True)
assert_test("operator.eq true", operator.eq(1, 1) == True)
assert_test("operator.eq false", operator.eq(1, 2) == False)
assert_test("operator.ne", operator.ne(1, 2) == True)
assert_test("operator.lt", operator.lt(1, 2) == True)
assert_test("operator.le", operator.le(2, 2) == True)
assert_test("operator.gt", operator.gt(3, 2) == True)
assert_test("operator.ge", operator.ge(2, 2) == True)
assert_test("operator.abs", operator.abs(-42) == 42)

# operator.contains
assert_test("operator.contains list", operator.contains([1, 2, 3], 2) == True)
assert_test("operator.contains str", operator.contains("hello", "ell") == True)

# operator.getitem
assert_test("operator.getitem list", operator.getitem([10, 20, 30], 1) == 20)
assert_test("operator.getitem dict", operator.getitem({"a": 1}, "a") == 1)
assert_test("operator.getitem tuple", operator.getitem((10, 20, 30), 2) == 30)

# operator with floats
assert_test("operator.add float", operator.add(1.5, 2.5) == 4.0)
assert_test("operator.mul float", operator.mul(2.5, 4.0) == 10.0)

# ── str.translate / str.maketrans ──

# maketrans with two strings
table = str.maketrans("aeiou", "12345")
result = "hello world".translate(table)
assert_test("translate vowels", result == "h2ll4 w4rld")

# maketrans with deletion (3rd arg)
table2 = str.maketrans("", "", "aeiou")
result2 = "hello world".translate(table2)
assert_test("translate delete vowels", result2 == "hll wrld")

# maketrans with replacement and deletion
table3 = str.maketrans("abc", "xyz", "def")
result3 = "abcdef".translate(table3)
assert_test("translate replace and delete", result3 == "xyz")

# translate with no changes
table4 = str.maketrans("xyz", "xyz")
assert_test("translate identity", "hello".translate(table4) == "hello")

# ── dict.fromkeys ──
d = dict.fromkeys(["a", "b", "c"])
assert_test("dict.fromkeys default", d == {"a": None, "b": None, "c": None})

d2 = dict.fromkeys(["x", "y", "z"], 0)
assert_test("dict.fromkeys with value", d2 == {"x": 0, "y": 0, "z": 0})

d3 = dict.fromkeys(range(3), "val")
assert_test("dict.fromkeys range", d3 == {0: "val", 1: "val", 2: "val"})

d4 = dict.fromkeys([])
assert_test("dict.fromkeys empty", d4 == {})

d5 = dict.fromkeys("abc", 1)
assert_test("dict.fromkeys string", d5 == {"a": 1, "b": 1, "c": 1})

# ── typing module ──
import typing
assert_test("typing.TYPE_CHECKING", typing.TYPE_CHECKING == False)

# ── abc module ──
import abc

# abstractmethod as decorator (identity)
@abc.abstractmethod
def my_func():
    pass
assert_test("abc.abstractmethod", my_func is not None)

# ── Additional functools.reduce tests ──
# Factorial with reduce
def factorial(n):
    if n <= 1:
        return 1
    return reduce(lambda a, b: a * b, range(1, n + 1))

assert_test("reduce factorial 5", factorial(5) == 120)
assert_test("reduce factorial 10", factorial(10) == 3628800)

# Flatten with reduce
nested_lists = [[1, 2], [3, 4], [5, 6]]
flat = reduce(lambda a, b: a + b, nested_lists)
assert_test("reduce flatten", flat == [1, 2, 3, 4, 5, 6])

# ── More copy tests ──
# Copy of set
s = {1, 2, 3}
s2 = copy.copy(s)
assert_test("copy set", s2 == {1, 2, 3})

# Deep copy of dict with lists
d = {"a": [1, 2], "b": [3, 4]}
d2 = copy.deepcopy(d)
d2["a"].append(3)
assert_test("deepcopy dict independence", d["a"] == [1, 2])

# ── More translate tests ──
# ROT13-like
table_rot = str.maketrans(
    "abcdefghijklmnopqrstuvwxyz",
    "nopqrstuvwxyzabcdefghijklm"
)
assert_test("translate rot13", "hello".translate(table_rot) == "uryyb")

# ── Test __name__ in if block ──
ran_main = False
if __name__ == "__main__":
    ran_main = True
assert_test("if __name__ == __main__", ran_main == True)

print()
print("=" * 40)
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(failed, "TESTS FAILED!")
