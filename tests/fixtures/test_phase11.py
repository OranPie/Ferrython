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

# ── Multiple assignment targets: a = b = c = 1 ──
a = b = c = 42
test("multi_assign", (a, b, c), (42, 42, 42))

# ── Chained string methods ──
test("chain_str", "  Hello World  ".strip().lower().replace("world", "python"), "hello python")

# ── Dict methods ──
d = {"a": 1, "b": 2, "c": 3}
test("dict_keys", sorted(d.keys()), ["a", "b", "c"])
test("dict_values", sorted(d.values()), [1, 2, 3])
test("dict_items", sorted(d.items()), [("a", 1), ("b", 2), ("c", 3)])
test("dict_get", d.get("a"), 1)
test("dict_get_default", d.get("z", 99), 99)
test("dict_pop", d.pop("c"), 3)
test("dict_len_after_pop", len(d), 2)
d.update({"x": 10, "y": 20})
test("dict_update", d["x"], 10)
d2 = d.copy()
test("dict_copy", d2["a"], 1)
test("dict_setdefault", d.setdefault("z", 100), 100)
test("dict_setdefault_existing", d.setdefault("a", 999), 1)

# ── List methods ──
lst = [3, 1, 4, 1, 5, 9]
test("list_index", lst.index(4), 2)
test("list_count", lst.count(1), 2)
lst2 = lst.copy()
lst2.reverse()
test("list_reverse", lst2, [9, 5, 1, 4, 1, 3])
lst3 = [1, 2]
lst3.extend([3, 4])
test("list_extend", lst3, [1, 2, 3, 4])
lst3.insert(0, 0)
test("list_insert", lst3, [0, 1, 2, 3, 4])
lst3.remove(2)
test("list_remove", lst3, [0, 1, 3, 4])
lst3.clear()
test("list_clear", lst3, [])

# ── Tuple methods ──
t = (1, 2, 3, 2, 1)
test("tuple_count", t.count(2), 2)
test("tuple_index", t.index(3), 2)

# ── Complex numbers full ──
c1 = 3 + 4j
c2 = 1 - 2j
test("complex_mul", c1 * c2, (11 - 2j))
test("complex_div", (4 + 2j) / (1 + 1j), (3 - 1j))
test("complex_neg", -c1, (-3 - 4j))
test("complex_abs", abs(c1), 5.0)

# ── String multiplication ──
test("str_mul", "ab" * 3, "ababab")
test("str_rmul", 3 * "ab", "ababab")

# ── Bytes basics ──
b = b"hello"
test("bytes_iter", list(b), [104, 101, 108, 108, 111])
test("bytes_in", 104 in b, True)
test("bytes_decode", b.decode(), "hello")
test("bytes_upper", b.upper(), b"HELLO")

# ── String methods extended ──
test("str_partition", "hello-world".partition("-"), ("hello", "-", "world"))
test("str_rpartition", "a-b-c".rpartition("-"), ("a-b", "-", "c"))
test("str_casefold", "HELLO".casefold(), "hello")
test("str_removeprefix", "TestCase".removeprefix("Test"), "Case")
test("str_removesuffix", "TestCase".removesuffix("Case"), "Test")
test("str_splitlines", "a\nb\nc".splitlines(), ["a", "b", "c"])
test("str_isidentifier", "hello".isidentifier(), True)
test("str_isidentifier2", "123".isidentifier(), False)
test("str_isascii", "hello".isascii(), True)

# ── Map with multiple iterables ──
test("map_multi", list(map(lambda x, y: x + y, [1, 2, 3], [10, 20, 30])), [11, 22, 33])

# ── Classmethod / staticmethod ──
class MyClass:
    count = 0
    
    @classmethod
    def increment(cls):
        cls.count = cls.count + 1
        return cls.count
    
    @staticmethod
    def greet(name):
        return "Hello, " + name

test("classmethod", MyClass.increment(), 1)
test("classmethod2", MyClass.increment(), 2)
test("staticmethod", MyClass.greet("World"), "Hello, World")

# ── Abstract-like patterns ──
class Base:
    def method(self):
        raise NotImplementedError("subclass must implement")

class Child(Base):
    def method(self):
        return "implemented"

ch = Child()
test("override", ch.method(), "implemented")

try:
    Base().method()
    test("not_impl_error", False, True)
except NotImplementedError as e:
    test("not_impl_error", str(e), "subclass must implement")

# ── Multiple return values ──
def divmod_custom(a, b):
    return a // b, a % b

q, r = divmod_custom(17, 5)
test("multi_return", (q, r), (3, 2))

# ── Nested functions and closures ──
def make_adder(n):
    def adder(x):
        return x + n
    return adder

add5 = make_adder(5)
add10 = make_adder(10)
test("closure_adder", add5(3), 8)
test("closure_adder2", add10(3), 13)

# ── Context manager ──
class ManagedResource:
    def __init__(self):
        self.log = []
    def __enter__(self):
        self.log.append("enter")
        return self
    def __exit__(self, *args):
        self.log.append("exit")
        return False

res = ManagedResource()
with res as r:
    r.log.append("body")
test("context_manager", res.log, ["enter", "body", "exit"])

# ── Generator with send ──
def accumulator():
    total = 0
    while True:
        value = yield total
        if value is None:
            break
        total = total + value

gen = accumulator()
next(gen)  # prime
test("gen_send1", gen.send(10), 10)
test("gen_send2", gen.send(20), 30)
test("gen_send3", gen.send(5), 35)

# ── Nested list comprehension ──
test("nested_listcomp", [[j for j in range(3)] for i in range(2)], [[0, 1, 2], [0, 1, 2]])

# ── Dict unpacking in function call ──
def func(a, b, c):
    return a * 100 + b * 10 + c

test("dict_unpack_call", func(**{"a": 1, "b": 2, "c": 3}), 123)
test("dict_unpack_call2", func(1, **{"b": 2, "c": 3}), 123)

# ── Ternary in different contexts ──
x = 5
test("ternary_assign", "big" if x > 3 else "small", "big")
test("ternary_list", [i if i % 2 == 0 else -i for i in range(5)], [0, -1, 2, -3, 4])

# ── String formatting ──
test("fstring_expr", f"{2 + 3}", "5")
test("fstring_method", f"{'hello'.upper()}", "HELLO")
test("str_format_pos", "{} and {}".format("a", "b"), "a and b")
test("str_format_idx", "{0} and {1}".format("a", "b"), "a and b")

# ── Exception hierarchy ──
test("exc_hier1", issubclass(ValueError, Exception), True)
test("exc_hier2", issubclass(KeyError, LookupError), True)
test("exc_hier3", issubclass(TypeError, Exception), True)
test("exc_hier4", issubclass(Exception, BaseException), True)

# ── isinstance with tuples ──
test("isinstance_tuple1", isinstance(42, (str, int)), True)
test("isinstance_tuple2", isinstance("hi", (str, int)), True)
test("isinstance_tuple3", isinstance([], (str, int)), False)

# ── Unpacking in for ──
pairs = [(1, "a"), (2, "b"), (3, "c")]
keys = []
vals = []
for k, v in pairs:
    keys.append(k)
    vals.append(v)
test("for_unpack_keys", keys, [1, 2, 3])
test("for_unpack_vals", vals, ["a", "b", "c"])

# ── Recursive class ──
class TreeNode:
    def __init__(self, val, left=None, right=None):
        self.val = val
        self.left = left
        self.right = right
    
    def sum(self):
        total = self.val
        if self.left:
            total = total + self.left.sum()
        if self.right:
            total = total + self.right.sum()
        return total

tree = TreeNode(1, TreeNode(2, TreeNode(4), TreeNode(5)), TreeNode(3))
test("tree_sum", tree.sum(), 15)

# ── String escape sequences ──
test("escape_tab", len("\t"), 1)
test("escape_newline", len("\n"), 1)
test("escape_backslash", len("\\"), 1)

# ── Negative indexing ──
lst = [10, 20, 30, 40, 50]
test("neg_idx_1", lst[-1], 50)
test("neg_idx_2", lst[-2], 40)
test("neg_slice", lst[-3:], [30, 40, 50])
test("neg_slice2", lst[:-2], [10, 20, 30])

# ── in operator for dict ──
d = {"x": 1, "y": 2}
test("dict_in", "x" in d, True)
test("dict_not_in", "z" in d, False)

# ── Multiple decorators ──
def bold(fn):
    def wrapper(*args):
        return "<b>" + fn(*args) + "</b>"
    return wrapper

def italic(fn):
    def wrapper(*args):
        return "<i>" + fn(*args) + "</i>"
    return wrapper

@bold
@italic
def greet(name):
    return "Hello " + name

test("multi_decorator", greet("World"), "<b><i>Hello World</i></b>")

# ── Yield from ──
def inner():
    yield 1
    yield 2
    yield 3

def outer():
    yield 0
    yield from inner()
    yield 4

test("yield_from", list(outer()), [0, 1, 2, 3, 4])

# ── Type checking ──
test("type_int", type(42).__name__, "int")
test("type_str", type("hi").__name__, "str")
test("type_list", type([]).__name__, "list")
test("type_dict", type({}).__name__, "dict")
test("type_bool", type(True).__name__, "bool")
test("type_none", type(None).__name__, "NoneType")

# ── Power with negative exponent ──
test("pow_neg", 2 ** -1, 0.5)
test("pow_float", 4 ** 0.5, 2.0)

# ── Large integers ──
test("big_int_mul", 2 ** 64, 18446744073709551616)
test("big_int_add", 10 ** 20 + 1, 100000000000000000001)

# ── all() and any() ──
test("all_true", all([True, True, True]), True)
test("all_false", all([True, False, True]), False)
test("any_true", any([False, True, False]), True)
test("any_false", any([False, False, False]), False)

# ── min/max with key ──
words = ["banana", "apple", "cherry"]
test("min_key", min(words, key=len), "apple")
test("max_key", max(words, key=len), "banana")

# ── sorted with key ──
test("sorted_key", sorted(words, key=len), ["apple", "banana", "cherry"])
test("sorted_rev", sorted([3, 1, 2], reverse=True), [3, 2, 1])

# ── enumerate ──
test("enumerate", list(enumerate("abc")), [(0, "a"), (1, "b"), (2, "c")])

# ── zip ──
test("zip", list(zip([1,2,3], "abc")), [(1, "a"), (2, "b"), (3, "c")])

# ── reversed ──
test("reversed", list(reversed([1, 2, 3])), [3, 2, 1])

# ── sum with start ──
test("sum_start", sum([1, 2, 3], 10), 16)

# ── round ──
test("round_int", round(3.7), 4)
test("round_digits", round(3.14159, 2), 3.14)

# ── abs ──
test("abs_int", abs(-42), 42)
test("abs_float", abs(-3.14), 3.14)

# ── divmod ──
test("divmod", divmod(17, 5), (3, 2))

# ── hash consistency ──
test("hash_str", hash("hello") == hash("hello"), True)
test("hash_int", hash(42) == hash(42), True)

# ── id ──
a = [1, 2, 3]
b = a
test("id_same", id(a) == id(b), True)
c = [1, 2, 3]
test("id_diff", id(a) == id(c), False)

# ── callable ──
test("callable_func", callable(len), True)
test("callable_int", callable(42), False)
test("callable_class", callable(int), True)

# ── Global built-in types ──
test("int_call", int("42"), 42)
test("float_call", float("3.14"), 3.14)
test("str_call", str(42), "42")
test("bool_call", bool(0), False)
test("list_call", list("abc"), ["a", "b", "c"])
test("tuple_call", tuple([1, 2, 3]), (1, 2, 3))
test("set_call", sorted(list(set([1, 2, 2, 3]))), [1, 2, 3])

print("========================================")
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("Failed tests:", ", ".join(errors))
print("========================================")
