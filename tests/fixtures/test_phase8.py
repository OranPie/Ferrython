"""Phase 8 tests: type() objects, __repr__ dispatch, exception .args,
str.split maxsplit, BuiltinType, and VM-level repr in containers."""

passed = 0
failed = 0
failures = []

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        failures.append(name)
        print(f"  FAIL: {name} got: {got!r} expected: {expected!r}")

# ── type() returns proper <class> objects ──
check("type_int", str(type(42)), "<class 'int'>")
check("type_str", str(type("hi")), "<class 'str'>")
check("type_float", str(type(3.14)), "<class 'float'>")
check("type_bool", str(type(True)), "<class 'bool'>")
check("type_list", str(type([])), "<class 'list'>")
check("type_dict", str(type({})), "<class 'dict'>")
check("type_tuple", str(type((1,))), "<class 'tuple'>")
check("type_set", str(type({1})), "<class 'set'>")
check("type_none", str(type(None)), "<class 'NoneType'>")

# ── type(x) == builtin_type ──
check("type_eq_int", type(42) == int, True)
check("type_eq_str", type("hi") == str, True)
check("type_eq_float", type(3.14) == float, True)
check("type_eq_bool", type(True) == bool, True)
check("type_eq_list", type([]) == list, True)
check("type_eq_dict", type({}) == dict, True)
check("type_ne", type(42) == str, False)

# ── type(x).__name__ ──
check("type_name_int", type(42).__name__, "int")
check("type_name_str", type("hi").__name__, "str")
check("type_name_float", type(3.14).__name__, "float")
check("type_name_list", type([]).__name__, "list")
check("type_name_bool", type(True).__name__, "bool")

# ── type() on custom instances ──
class Animal:
    pass
a = Animal()
check("type_custom", type(a).__name__, "Animal")
check("type_custom_is_class", type(a) == Animal, True)

# ── isinstance still works with BuiltinType ──
check("isinstance_int", isinstance(42, int), True)
check("isinstance_str", isinstance("hello", str), True)
check("isinstance_bool_int", isinstance(True, int), True)
check("isinstance_list", isinstance([], list), True)
check("isinstance_tuple_types", isinstance(42, (str, int)), True)
check("isinstance_neg", isinstance(42, str), False)

# ── str.split with maxsplit ──
check("split_max_1", "a.b.c.d".split(".", 2), ["a", "b", "c.d"])
check("split_max_2", "a b c d".split(" ", 1), ["a", "b c d"])
check("split_max_3", "one::two::three::four".split("::", 2), ["one", "two", "three::four"])
check("split_no_max", "a.b.c".split("."), ["a", "b", "c"])
check("split_none_sep", "a b  c".split(None, 1), ["a", "b  c"])
check("split_ws_default", "  hello  world  ".split(), ["hello", "world"])

# ── exception .args ──
try:
    raise ValueError("bad value")
except ValueError as e:
    check("exc_args", e.args, ("bad value",))
    check("exc_args_0", e.args[0], "bad value")
    check("exc_str", str(e), "bad value")

try:
    raise TypeError("type err")
except TypeError as e:
    check("exc_type_args", e.args, ("type err",))

try:
    1 / 0
except ZeroDivisionError as e:
    check("exc_zdiv_args_type", type(e.args), tuple)

# ── __repr__ dispatch in containers ──
class Pt:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __repr__(self):
        return f"Pt({self.x}, {self.y})"

p1 = Pt(1, 2)
p2 = Pt(3, 4)

check("repr_instance", repr(p1), "Pt(1, 2)")
check("repr_list", repr([p1, p2]), "[Pt(1, 2), Pt(3, 4)]")
check("repr_tuple", repr((p1,)), "(Pt(1, 2),)")
check("repr_tuple2", repr((p1, p2)), "(Pt(1, 2), Pt(3, 4))")

# print uses str() which for containers uses repr()
# Verify via repr comparison

# ── __repr__ with nested containers ──
check("repr_nested_list", repr([[1, 2], [3, 4]]), "[[1, 2], [3, 4]]")
check("repr_list_str", repr(["hello", "world"]), "['hello', 'world']")
check("repr_list_mixed", repr([1, "a", True, None]), "[1, 'a', True, None]")

# ── __str__ dispatch ──
class Greeter:
    def __init__(self, name):
        self.name = name
    def __str__(self):
        return f"Hello, {self.name}!"
    def __repr__(self):
        return f"Greeter({self.name!r})"

g = Greeter("World")
check("str_dispatch", str(g), "Hello, World!")
check("repr_dispatch", repr(g), "Greeter('World')")

# When inside a list, repr is used
check("str_list_uses_repr", repr([g]), "[Greeter('World')]")

# ── type() for various types ──
check("type_bytes", str(type(b"hi")), "<class 'bytes'>")
# range() returns a range type
check("type_range", str(type(range(5))), "<class 'range'>")

# ── int/str/float/bool as BuiltinType still work as constructors ──
check("int_constructor", int("42"), 42)
check("float_constructor", float("3.14"), 3.14)
check("str_constructor", str(42), "42")
check("bool_constructor", bool(1), True)
check("bool_constructor_0", bool(0), False)
check("list_constructor", list((1, 2, 3)), [1, 2, 3])
check("tuple_constructor", tuple([1, 2, 3]), (1, 2, 3))

# ── callable() with BuiltinType ──
check("callable_int", callable(int), True)
check("callable_str", callable(str), True)
check("callable_print", callable(print), True)
check("callable_none", callable(None), False)

# ── dict repr with custom objects ──
class Named:
    def __init__(self, n):
        self.n = n
    def __repr__(self):
        return f"Named({self.n})"

d = {1: Named("a"), 2: Named("b")}
check("dict_repr_custom", repr(d), "{1: Named(a), 2: Named(b)}")

# ── empty containers ──
check("repr_empty_list", repr([]), "[]")
check("repr_empty_tuple", repr(()), "()")
check("repr_empty_dict", repr({}), "{}")

# ── tuple of 1 element ──
check("repr_single_tuple", repr((42,)), "(42,)")

# ── BuiltinType __name__ ──
check("int_name", int.__name__, "int")
check("str_name", str.__name__, "str")
check("list_name", list.__name__, "list")
check("dict_name", dict.__name__, "dict")
check("float_name", float.__name__, "float")
check("bool_name", bool.__name__, "bool")

# ── issubclass with BuiltinType ──
# This tests basic issubclass behavior
class MyError(ValueError):
    pass

check("issubclass_custom", issubclass(MyError, ValueError), True)

# ── str.split edge cases ──
check("split_empty_sep_result", "abc".split("b"), ["a", "c"])
check("split_at_start", ".abc".split(".", 1), ["", "abc"])
check("split_at_end", "abc.".split("."), ["abc", ""])
check("split_max_0", "a.b.c".split(".", 0), ["a.b.c"])

# ── Summary ──
print("=" * 40)
print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failures:
    print(f"FAILURES: {len(failures)}")
    for f in failures:
        print(f"  - {f}")
else:
    print("ALL TESTS PASSED!")
print("=" * 40)
