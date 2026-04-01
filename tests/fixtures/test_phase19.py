passed = 0
failed = 0
def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name, "| got:", repr(got), "| expected:", repr(expected))

# ── exec basic ──
exec("exec_var = 42")
test("exec_basic", exec_var, 42)

exec("exec_list = [1, 2, 3]")
test("exec_list", exec_list, [1, 2, 3])

# exec with function def
exec("def exec_fn(x):\n    return x * 2")
test("exec_fn", exec_fn(5), 10)

# ── eval basic ──
test("eval_expr", eval("1 + 2 + 3"), 6)
test("eval_string", eval("'hello' + ' ' + 'world'"), "hello world")
test("eval_list", eval("[1, 2, 3]"), [1, 2, 3])

# eval with variable from outer scope
z = 100
test("eval_scope", eval("z + 1"), 101)

# ── globals() ──
glob_test_var = 42
g = globals()
test("globals_has_var", "glob_test_var" in g, True)
test("globals_value", g["glob_test_var"], 42)

# ── locals() in function ──
def test_locals(a, b):
    c = a + b
    loc = locals()
    return loc

loc = test_locals(10, 20)
test("locals_a", loc["a"], 10)
test("locals_b", loc["b"], 20)
test("locals_c", loc["c"], 30)

# ── __delattr__ ──
class WithDelattr:
    def __init__(self):
        self.log = []
        self.x = 10
    def __delattr__(self, name):
        self.log.append("del:" + name)

obj = WithDelattr()
del obj.x
test("delattr_dispatch", obj.log, ["del:x"])

# ── class with __setattr__ ──
class Logged:
    def __init__(self):
        # bypass __setattr__ for initial setup
        object.__setattr__ = None  # not real, just test below
    def __setattr__(self, name, value):
        # Custom setattr should be called
        if not hasattr(self, "_log"):
            super().__setattr__("_log", [])
        self._log.append(name)
        super().__setattr__(name, value)

# ── more string methods ──
test("str_partition", "hello world".partition(" "), ("hello", " ", "world"))
test("str_rpartition", "hello world again".rpartition(" "), ("hello world", " ", "again"))
test("str_casefold", "HELLO".casefold(), "hello")
test("str_removeprefix", "TestCase".removeprefix("Test"), "Case")
test("str_removesuffix", "TestCase".removesuffix("Case"), "Test")
test("str_splitlines", "a\nb\nc".splitlines(), ["a", "b", "c"])
test("str_isidentifier", "hello_world".isidentifier(), True)
test("str_isidentifier2", "123abc".isidentifier(), False)
test("str_encode", "hello".encode(), b"hello")
test("str_isnumeric", "123".isnumeric(), True)
test("str_isdecimal", "123".isdecimal(), True)
test("str_isascii", "hello".isascii(), True)

# ── dict views ──
d = {"a": 1, "b": 2, "c": 3}
ks = list(d.keys())
vs = list(d.values())
items = list(d.items())
test("dict_keys", sorted(ks), ["a", "b", "c"])
test("dict_values", sorted(vs), [1, 2, 3])
test("dict_items_len", len(items), 3)

# ── dict popitem ──
d2 = {"a": 1, "b": 2}
item = d2.popitem()
test("dict_popitem_len", len(d2), 1)

# ── set operations ──
s1 = {1, 2, 3, 4}
s2 = {3, 4, 5, 6}
test("set_union", s1 | s2, {1, 2, 3, 4, 5, 6})
test("set_intersection", s1 & s2, {3, 4})
test("set_difference", s1 - s2, {1, 2})
test("set_symmetric_diff", s1 ^ s2, {1, 2, 5, 6})
test("set_subset", {1, 2} <= {1, 2, 3}, True)
test("set_superset", {1, 2, 3} >= {1, 2}, True)

# set methods
s3 = {1, 2, 3}
s3.add(4)
test("set_add", s3, {1, 2, 3, 4})
s3.discard(2)
test("set_discard", s3, {1, 3, 4})

# ── frozenset ──
fs = frozenset([1, 2, 3])
test("frozenset_in", 2 in fs, True)
test("frozenset_len", len(fs), 3)

# ── multiple inheritance ──
class A:
    def greet(self):
        return "A"

class B(A):
    def greet(self):
        return "B"

class C(A):
    def greet(self):
        return "C"

class D(B, C):
    def greet(self):
        return "D"

test("mro_D", D().greet(), "D")

# Super traversal
class Base:
    def method(self):
        return "Base"

class Middle(Base):
    def method(self):
        return "Middle+" + super().method()

class Child(Middle):
    def method(self):
        return "Child+" + super().method()

test("super_chain", Child().method(), "Child+Middle+Base")

# ── lambda ──
square = lambda x: x ** 2
test("lambda_basic", square(5), 25)
test("lambda_inline", (lambda x, y: x + y)(3, 4), 7)

# ── list comprehension with condition ──
test("listcomp_if", [x for x in range(10) if x % 2 == 0], [0, 2, 4, 6, 8])

# ── nested comprehension ──
test("nested_comp", [x * y for x in [1, 2, 3] for y in [10, 20]], [10, 20, 20, 40, 30, 60])

# ── dict comprehension ──
test("dictcomp_sq", {x: x**2 for x in range(5)}, {0: 0, 1: 1, 2: 4, 3: 9, 4: 16})

# ── set comprehension ──
test("setcomp_mod", {x % 4 for x in range(12)}, {0, 1, 2, 3})

# ── generator expression ──
test("genexpr_list", list(x * 2 for x in range(5)), [0, 2, 4, 6, 8])

# ── walrus operator ──
results = []
nums = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
for n in nums:
    if (sq := n * n) > 25:
        results.append(sq)
test("walrus_loop", results, [36, 49, 64, 81, 100])

# ── ternary / conditional expression ──
test("ternary_true", "yes" if True else "no", "yes")
test("ternary_false", "yes" if False else "no", "no")
test("ternary_expr", 10 if 5 > 3 else 20, 10)

# ── chained comparison ──
test("chain_3way", 1 < 2 < 3 < 4, True)
test("chain_fail", 1 < 2 < 2, False)

# ── global keyword ──
counter = 0
def inc():
    global counter
    counter += 1

inc()
inc()
inc()
test("global_var", counter, 3)

# ── nonlocal keyword ──
def make_counter():
    count = 0
    def increment():
        nonlocal count
        count += 1
        return count
    return increment

ctr = make_counter()
test("nonlocal_1", ctr(), 1)
test("nonlocal_2", ctr(), 2)
test("nonlocal_3", ctr(), 3)

# ── nested functions / closures ──
def adder(x):
    def add(y):
        return x + y
    return add

add5 = adder(5)
test("closure_basic", add5(3), 8)
test("closure_basic2", add5(10), 15)

# ── default arguments ──
def greet(name, greeting="Hello"):
    return greeting + " " + name

test("default_arg", greet("World"), "Hello World")
test("default_override", greet("World", "Hi"), "Hi World")

# ── *args and **kwargs ──
def flex(*args, **kwargs):
    return (list(args), kwargs)

test("args_only", flex(1, 2, 3), ([1, 2, 3], {}))

# ── while loop with else ──
def while_else(n):
    i = 0
    while i < n:
        if i == 5:
            return "break"
        i += 1
    else:
        return "else"

test("while_else_normal", while_else(3), "else")
test("while_else_break", while_else(10), "break")

# ── for loop with else ──
def for_else(lst, target):
    for x in lst:
        if x == target:
            return "found"
    else:
        return "not found"

test("for_else_found", for_else([1, 2, 3], 2), "found")
test("for_else_notfound", for_else([1, 2, 3], 5), "not found")

# ── try/except/else/finally ──
def try_else(x):
    result = []
    try:
        result.append("try")
        if x < 0:
            raise ValueError("negative")
    except ValueError:
        result.append("except")
    else:
        result.append("else")
    finally:
        result.append("finally")
    return result

test("try_else_ok", try_else(1), ["try", "else", "finally"])
test("try_else_err", try_else(-1), ["try", "except", "finally"])

# ── assert ──
assert True
assert 1 == 1
try:
    assert False, "assertion message"
    test("assert_fail", True, False)  # shouldn't reach
except AssertionError as e:
    test("assert_msg", str(e), "assertion message")

# ── string formatting ──
test("fstr_basic", f"hello {'world'}", "hello world")
test("fstr_expr", f"{1 + 2}", "3")
test("fstr_var", f"x={42}", "x=42")

# ── bytes ──
test("bytes_len", len(b"hello"), 5)
test("bytes_index", b"hello"[0], 104)

# ── None checks ──
test("none_is_none", None is None, True)
test("none_eq_none", None == None, True)
test("none_bool", bool(None), False)

# ── isinstance with built-in types ──
test("isinstance_list", isinstance([], list), True)
test("isinstance_dict", isinstance({}, dict), True)
test("isinstance_tuple", isinstance((), tuple), True)
test("isinstance_bool_int", isinstance(True, int), True)

# ── type() ──
test("type_int", type(42).__name__, "int")
test("type_str", type("hi").__name__, "str")
test("type_list", type([]).__name__, "list")

print("=" * 40)
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
print("=" * 40)
