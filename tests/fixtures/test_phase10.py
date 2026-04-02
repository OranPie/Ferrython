passed = 0
failed = 0

def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name)
        print("  expected:", expected)
        print("  got:", got)

# ── __bool__ dispatch in if/while/and/or/not ──

class AlwaysFalse:
    def __bool__(self):
        return False

class AlwaysTrue:
    def __bool__(self):
        return True

af = AlwaysFalse()
at = AlwaysTrue()

# if statement
test("bool_if_false", "yes" if af else "no", "no")
test("bool_if_true", "yes" if at else "no", "yes")

# while with __bool__
count = 0
class Counter:
    def __init__(self):
        self.n = 3
    def __bool__(self):
        return self.n > 0
    def tick(self):
        self.n = self.n - 1

c = Counter()
while c:
    count = count + 1
    c.tick()
test("bool_while", count, 3)

# not operator
test("bool_not_false", not af, True)
test("bool_not_true", not at, False)

# and/or with __bool__
test("bool_and_ff", (af and "yes") is af, True)
test("bool_and_tf", (at and "yes"), "yes")
test("bool_or_ff", (af or "no"), "no")
test("bool_or_tf", (at or "no") is at, True)

# bool() builtin
test("bool_call_false", bool(af), False)
test("bool_call_true", bool(at), True)

# __len__-based truthiness
class EmptyContainer:
    def __len__(self):
        return 0

class NonEmptyContainer:
    def __len__(self):
        return 5

ec = EmptyContainer()
nc = NonEmptyContainer()
test("len_if_empty", "yes" if ec else "no", "no")
test("len_if_nonempty", "yes" if nc else "no", "yes")
test("len_not_empty", not ec, True)
test("len_not_nonempty", not nc, False)
test("len_bool_empty", bool(ec), False)
test("len_bool_nonempty", bool(nc), True)

# __bool__ takes priority over __len__
class BoolOverLen:
    def __bool__(self):
        return False
    def __len__(self):
        return 100

bol = BoolOverLen()
test("bool_over_len", bool(bol), False)
test("bool_over_len_if", "yes" if bol else "no", "no")

# filter() with __bool__-returning objects
class Wrap:
    def __init__(self, val):
        self.val = val
    def __bool__(self):
        return self.val

items = [Wrap(True), Wrap(False), Wrap(True), Wrap(False)]
filtered = list(filter(None, items))
test("filter_bool", len(filtered), 2)

# ── dict unpacking with ** in function calls ──

def show(a, b, c):
    return a + b + c

d = {"a": 1, "b": 2, "c": 3}
test("dict_unpack_call", show(**d), 6)

d2 = {"b": 20, "c": 30}
test("dict_unpack_call2", show(1, **d2), 51)

# ── Multiple inheritance ──

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
    pass

d_obj = D()
test("mro_diamond", d_obj.greet(), "B")
test("isinstance_diamond", isinstance(d_obj, A), True)

# ── String methods ──

test("str_startswith", "hello world".startswith("hello"), True)
test("str_endswith", "hello world".endswith("world"), True)
test("str_find", "hello world".find("world"), 6)
test("str_find_missing", "hello world".find("xyz"), -1)
test("str_count", "banana".count("a"), 3)
test("str_center", "hi".center(10, "-"), "----hi----")
test("str_ljust", "hi".ljust(6, "."), "hi....")
test("str_rjust", "hi".rjust(6, "."), "....hi")
test("str_zfill", "42".zfill(5), "00042")
test("str_title", "hello world".title(), "Hello World")
test("str_swapcase", "Hello World".swapcase(), "hELLO wORLD")
test("str_capitalize", "hello WORLD".capitalize(), "Hello world")
test("str_expandtabs", "a\tb".expandtabs(4), "a   b")

# ── Nested comprehensions ──

matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat = [x for row in matrix for x in row]
test("nested_comp_flat", flat, [1, 2, 3, 4, 5, 6, 7, 8, 9])

pairs = [(x, y) for x in range(3) for y in range(3) if x != y]
test("nested_comp_filter", len(pairs), 6)

# ── Chained comparisons ──

test("chain_cmp_1", 1 < 2 < 3, True)
test("chain_cmp_2", 1 < 2 > 1, True)
test("chain_cmp_3", 1 < 2 < 2, False)

x = 5
test("chain_cmp_range", 0 <= x <= 10, True)
test("chain_cmp_range2", 0 <= x <= 4, False)

# ── Augmented assignment ──

x = 10
x += 5
test("aug_add", x, 15)
x -= 3
test("aug_sub", x, 12)
x *= 2
test("aug_mul", x, 24)
x //= 5
test("aug_floordiv", x, 4)
x **= 3
test("aug_power", x, 64)
x %= 10
test("aug_mod", x, 4)

# ── Global / nonlocal ──

g = 100
def modify_global():
    global g
    g = 200

modify_global()
test("global_modify", g, 200)

def outer_nonlocal():
    x = 1
    def inner():
        nonlocal x
        x = 2
    inner()
    return x

test("nonlocal_modify", outer_nonlocal(), 2)

# ── Unpacking assignments ──

a, *b, c = [1, 2, 3, 4, 5]
test("star_unpack_a", a, 1)
test("star_unpack_b", b, [2, 3, 4])
test("star_unpack_c", c, 5)

first, *rest = "hello"
test("star_unpack_str_first", first, "h")
test("star_unpack_str_rest", rest, ["e", "l", "l", "o"])

# ── Class __str__ and __repr__ ──

class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __repr__(self):
        return "Point(" + str(self.x) + ", " + str(self.y) + ")"
    def __str__(self):
        return "(" + str(self.x) + ", " + str(self.y) + ")"

p = Point(3, 4)
test("repr_custom", repr(p), "Point(3, 4)")
test("str_custom", str(p), "(3, 4)")

# list/tuple with custom repr
pts = [Point(1, 2), Point(3, 4)]
test("list_repr", repr(pts), "[Point(1, 2), Point(3, 4)]")

# ── Exception handling ──

def divide(a, b):
    try:
        return a / b
    except ZeroDivisionError:
        return "zero!"
    except TypeError:
        return "type!"

test("exc_zerodiv", divide(10, 0), "zero!")
test("exc_type", divide("a", "b"), "type!")
test("exc_normal", divide(10, 2), 5.0)

# nested try
def nested_try():
    try:
        try:
            raise ValueError("inner")
        except TypeError:
            return "wrong"
    except ValueError as e:
        return str(e)

test("nested_try_exc", nested_try(), "inner")

# finally always runs
results = []
def with_finally():
    try:
        results.append("try")
        return 42
    finally:
        results.append("finally")

r = with_finally()
test("finally_runs", results, ["try", "finally"])
test("finally_return", r, 42)

# ── Generator features ──

def gen_range(n):
    i = 0
    while i < n:
        yield i
        i += 1

test("gen_list", list(gen_range(5)), [0, 1, 2, 3, 4])
test("gen_sum", sum(gen_range(10)), 45)

# Generator expression
test("genexpr_sum", sum(x * x for x in range(5)), 30)

# ── Decorator ──

def double_result(func):
    def wrapper(*args):
        return func(*args) * 2
    return wrapper

@double_result
def add(a, b):
    return a + b

test("decorator", add(3, 4), 14)

# ── Property ──

class Circle:
    def __init__(self, radius):
        self._radius = radius
    
    @property
    def radius(self):
        return self._radius
    
    @radius.setter
    def radius(self, value):
        if value < 0:
            raise ValueError("negative")
        self._radius = value

c = Circle(5)
test("property_get", c.radius, 5)
c.radius = 10
test("property_set", c.radius, 10)

# ── Dict comprehension ──

squares = {x: x**2 for x in range(6)}
test("dict_comp", squares, {0: 0, 1: 1, 2: 4, 3: 9, 4: 16, 5: 25})

# Filtered dict comp
even_sq = {x: x**2 for x in range(10) if x % 2 == 0}
test("dict_comp_filter", even_sq, {0: 0, 2: 4, 4: 16, 6: 36, 8: 64})

# ── Set operations ──

s1 = {1, 2, 3, 4}
s2 = {3, 4, 5, 6}
test("set_union", s1 | s2, {1, 2, 3, 4, 5, 6})
test("set_intersection", s1 & s2, {3, 4})
test("set_difference", s1 - s2, {1, 2})
test("set_symmetric_diff", s1 ^ s2, {1, 2, 5, 6})
test("set_issubset", {1, 2} <= {1, 2, 3}, True)
test("set_issuperset", {1, 2, 3} >= {1, 2}, True)

# ── Walrus operator ──

data = [1, 5, 10, 3, 8, 15, 2]
big = [y for x in data if (y := x * 2) > 10]
test("walrus_comp", big, [20, 16, 30])

# ── Multiple except types ──

def multi_except(x):
    try:
        if x == 0:
            raise ValueError("val")
        elif x == 1:
            raise TypeError("typ")
        else:
            raise KeyError("key")
    except (ValueError, TypeError) as e:
        return "caught: " + str(e)
    except KeyError as e:
        return "key: " + str(e)

test("multi_except_val", multi_except(0), "caught: val")
test("multi_except_type", multi_except(1), "caught: typ")
test("multi_except_key", multi_except(2), "key: 'key'")

# ── Enumerate with start ──

items = ["a", "b", "c"]
result = list(enumerate(items, 1))
test("enumerate_start", result, [(1, "a"), (2, "b"), (3, "c")])

# ── Zip longest simulation with zip ──

test("zip_basic", list(zip([1,2,3], "abc")), [(1, "a"), (2, "b"), (3, "c")])

# ── Recursive generator ──

def flatten(lst):
    for item in lst:
        if isinstance(item, list):
            yield from flatten(item)
        else:
            yield item

nested = [1, [2, 3], [4, [5, 6]], 7]
test("recursive_gen", list(flatten(nested)), [1, 2, 3, 4, 5, 6, 7])

# ── Class with __eq__ and __hash__ ──

class Token:
    def __init__(self, kind, value):
        self.kind = kind
        self.value = value
    def __eq__(self, other):
        if not isinstance(other, Token):
            return False
        return self.kind == other.kind and self.value == other.value
    def __hash__(self):
        return hash(self.kind) + hash(self.value)

t1 = Token("NUM", 42)
t2 = Token("NUM", 42)
t3 = Token("STR", "hi")
test("custom_eq", t1 == t2, True)
test("custom_neq", t1 == t3, False)
# dict key with custom __hash__ requires VM-level dispatch (future)
# d = {t1: "first"}
# test("custom_hash_lookup", d[t2], "first")

# ── String formatting ──

test("format_spec_int", format(42, "05d"), "00042")
test("format_spec_float", format(3.14159, ".2f"), "3.14")
test("fstring_expr", f"{'hello':>10}", "     hello")

# ── Bytes basic ──

b = b"hello"
test("bytes_len", len(b), 5)
test("bytes_index", b[0], 104)
test("bytes_slice", b[1:3], b"el")

# ── Complex numbers ──

c1 = 3 + 4j
c2 = 1 - 2j
test("complex_add", c1 + c2, (4+2j))
test("complex_real", c1.real, 3.0)
test("complex_imag", c1.imag, 4.0)

print("========================================")
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
print("========================================")
