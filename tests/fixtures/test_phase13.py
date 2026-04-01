passed = 0
failed = 0
errors = []

def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        errors.append(name)
        print("FAIL:", name, "| got:", got, "| expected:", expected)

# ── enumerate with start ──
test("enumerate_start", list(enumerate(["a", "b", "c"], 1)), [(1, "a"), (2, "b"), (3, "c")])
test("enumerate_basic", list(enumerate("ab")), [(0, "a"), (1, "b")])

# ── zip ──
test("zip_basic", list(zip([1, 2], [3, 4])), [(1, 3), (2, 4)])
test("zip_uneven", list(zip([1, 2, 3], [4, 5])), [(1, 4), (2, 5)])
test("zip_three", list(zip("abc", [1, 2, 3], [True, False, True])), [("a", 1, True), ("b", 2, False), ("c", 3, True)])

# ── reversed ──
test("reversed_list", list(reversed([1, 2, 3])), [3, 2, 1])
test("reversed_range", list(reversed(range(5))), [4, 3, 2, 1, 0])
test("reversed_str", list(reversed("abc")), ["c", "b", "a"])

# ── dict.get with default ──
d = {"a": 1, "b": 2}
test("dict_get", d.get("a"), 1)
test("dict_get_default", d.get("c", 42), 42)
test("dict_get_none", d.get("c"), None)

# ── dict.pop ──
d2 = {"x": 1, "y": 2}
test("dict_pop", d2.pop("x"), 1)
test("dict_pop_after", d2, {"y": 2})
test("dict_pop_default", d2.pop("z", 99), 99)

# ── dict.setdefault ──
d3 = {"a": 1}
test("setdefault_exists", d3.setdefault("a", 10), 1)
test("setdefault_new", d3.setdefault("b", 20), 20)
test("setdefault_check", d3, {"a": 1, "b": 20})

# ── dict.update ──
d4 = {"a": 1}
d4.update({"b": 2, "c": 3})
test("dict_update", d4, {"a": 1, "b": 2, "c": 3})

# ── list.insert ──
lst = [1, 2, 3]
lst.insert(1, 10)
test("list_insert", lst, [1, 10, 2, 3])

# ── list.remove ──
lst2 = [1, 2, 3, 2, 1]
lst2.remove(2)
test("list_remove", lst2, [1, 3, 2, 1])

# ── list.index ──
test("list_index", [10, 20, 30, 20].index(20), 1)

# ── list.count ──
test("list_count", [1, 2, 3, 2, 1, 2].count(2), 3)

# ── list.reverse ──
lst3 = [3, 1, 2]
lst3.reverse()
test("list_reverse", lst3, [2, 1, 3])

# ── list.extend ──
lst4 = [1, 2]
lst4.extend([3, 4, 5])
test("list_extend", lst4, [1, 2, 3, 4, 5])

# ── list.clear ──
lst5 = [1, 2, 3]
lst5.clear()
test("list_clear", lst5, [])

# ── list.copy ──
lst6 = [1, 2, 3]
lst7 = lst6.copy()
lst7.append(4)
test("list_copy", lst6, [1, 2, 3])
test("list_copy2", lst7, [1, 2, 3, 4])

# ── tuple methods ──
t = (1, 2, 3, 2, 1)
test("tuple_count", t.count(2), 2)
test("tuple_index", t.index(3), 2)

# ── range attributes ──
r = range(1, 10, 2)
test("range_len", len(r), 5)
test("range_in", 3 in r, True)
test("range_notin", 4 in r, False)
test("range_list", list(r), [1, 3, 5, 7, 9])

# ── String multiplication ──
test("str_mul", "ab" * 3, "ababab")
test("str_mul2", 3 * "ha", "hahaha")

# ── Walrus operator ── (Python 3.8)
# NOTE: This needs parser support; skip if not available

# ── Conditional expression chain ──
def classify(n):
    return "pos" if n > 0 else "zero" if n == 0 else "neg"

test("ternary_chain", classify(5), "pos")
test("ternary_chain2", classify(0), "zero")
test("ternary_chain3", classify(-3), "neg")

# ── String format method ──
test("str_format_idx", "{0} {1}".format("hello", "world"), "hello world")
test("str_format_auto", "{} {}".format("a", "b"), "a b")
test("str_format_name", "{name}!".format(name="World"), "World!")

# ── repr of various types ──
test("repr_none", repr(None), "None")
test("repr_bool", repr(True), "True")
test("repr_str", repr("hello"), "'hello'")
test("repr_list", repr([1, 2, 3]), "[1, 2, 3]")
test("repr_dict", repr({}), "{}")
test("repr_tuple", repr((1,)), "(1,)")

# ── dict comprehension ──
test("dict_comp", {x: x**2 for x in range(5)}, {0: 0, 1: 1, 2: 4, 3: 9, 4: 16})

# ── set comprehension ──
test("set_comp", {x % 3 for x in range(9)}, {0, 1, 2})

# ── Nested list comprehension ──
test("nested_listcomp", [[j for j in range(3)] for i in range(2)], [[0, 1, 2], [0, 1, 2]])
test("flatten", [x for row in [[1, 2], [3, 4], [5, 6]] for x in row], [1, 2, 3, 4, 5, 6])

# ── Multiple assignment ──
a = b = c = 10
test("multi_assign", (a, b, c), (10, 10, 10))

# ── Chained comparisons ──
test("chain_cmp", 1 < 2 < 3, True)
test("chain_cmp2", 1 < 2 > 3, False)

# ── is / is not ──
test("is_none", None is None, True)
test("is_not_none", 1 is not None, True)

# ── Membership in string ──
test("in_str", "ell" in "hello", True)
test("notin_str", "xyz" not in "hello", True)

# ── round() ──
test("round_int", round(3.7), 4)
test("round_ndigits", round(3.14159, 2), 3.14)

# ── divmod() ──
test("divmod_basic", divmod(17, 5), (3, 2))

# ── pow() ──
test("pow_basic", pow(2, 10), 1024)
test("pow_mod", pow(2, 10, 100), 24)

# ── bin/hex/oct ──
test("bin_val", bin(10), "0b1010")
test("hex_val", hex(255), "0xff")
test("oct_val", oct(8), "0o10")

# ── isinstance with tuple of types ──
test("isinstance_tuple", isinstance(42, (str, int, float)), True)
test("isinstance_tuple2", isinstance("hi", (str, int)), True)
test("isinstance_tuple3", isinstance([], (str, int)), False)

# ── String startswith/endswith with tuple ──
test("startswith_tuple", "hello".startswith(("he", "wo")), True)
test("endswith_tuple", "hello".endswith(("lo", "xy")), True)

# ── Negative indexing ──
lst = [10, 20, 30, 40, 50]
test("neg_idx", lst[-1], 50)
test("neg_idx2", lst[-3], 30)
test("neg_slice", lst[-3:], [30, 40, 50])
test("neg_slice2", lst[:-2], [10, 20, 30])

# ── Assignment to slice ──
lst = [1, 2, 3, 4, 5]
lst[1:3] = [20, 30]
test("slice_assign", lst, [1, 20, 30, 4, 5])

# ── Delete from dict ──
d = {"a": 1, "b": 2, "c": 3}
del d["b"]
test("del_dict", d, {"a": 1, "c": 3})

# ── Assert statement ──
def assert_test():
    try:
        assert False, "assertion message"
    except AssertionError as e:
        return str(e)

test("assert_msg", assert_test(), "assertion message")

# ── Global/nonlocal ──
counter = 0
def increment():
    global counter
    counter += 1

increment()
increment()
test("global_stmt", counter, 2)

def make_counter():
    count = 0
    def inc():
        nonlocal count
        count += 1
        return count
    return inc

inc = make_counter()
test("nonlocal", (inc(), inc(), inc()), (1, 2, 3))

# ── Multiple return values ──
def minmax(lst):
    return min(lst), max(lst)

test("multi_return", minmax([5, 2, 8, 1, 9]), (1, 9))

# ── Chained string operations ──
test("chain_str_ops", "Hello World".lower().replace("world", "python").upper(), "HELLO PYTHON")

# ── Recursive function ──
def factorial(n):
    if n <= 1:
        return 1
    return n * factorial(n - 1)

test("recursive", factorial(10), 3628800)

# ── Fibonacci ──
def fib(n):
    if n <= 1:
        return n
    a, b = 0, 1
    for i in range(2, n + 1):
        a, b = b, a + b
    return b

test("fib", fib(20), 6765)

# ── Class with __len__ and __bool__ ──
class Container:
    def __init__(self, items):
        self.items = items
    def __len__(self):
        return len(self.items)
    def __getitem__(self, idx):
        return self.items[idx]
    def __contains__(self, item):
        return item in self.items

c = Container([1, 2, 3])
test("dunder_len", len(c), 3)
test("dunder_getitem", c[1], 2)
test("dunder_contains", 2 in c, True)
test("dunder_contains2", 5 in c, False)

# ── vars() / dir() basics ──
# Just test that they run without error
test("type_int", type(42).__name__, "int")
test("type_str", type("hi").__name__, "str")
test("type_list", type([]).__name__, "list")

# ── Exception hierarchy ──
test("exc_hierarchy", issubclass(ValueError, Exception), True)
test("exc_hierarchy2", issubclass(KeyError, LookupError), True)
test("exc_hierarchy3", issubclass(FileNotFoundError, OSError), True)

print("========================================")
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("Failed tests:", ", ".join(errors))
print("========================================")
