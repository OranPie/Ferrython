passed = 0
failed = 0
def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name, "| got:", repr(got), "| expected:", repr(expected))

# ── setattr / getattr / delattr / hasattr ──
class Obj:
    def __init__(self):
        self.x = 10

o = Obj()
test("getattr_basic", getattr(o, "x"), 10)
test("getattr_default", getattr(o, "y", 42), 42)
test("hasattr_true", hasattr(o, "x"), True)
test("hasattr_false", hasattr(o, "y"), False)

setattr(o, "z", 99)
test("setattr_basic", o.z, 99)

delattr(o, "z")
test("delattr_basic", hasattr(o, "z"), False)

# ── vars() on instance ──
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
p = Point(3, 4)
v = vars(p)
test("vars_x", v["x"], 3)
test("vars_y", v["y"], 4)

# ── callable() ──
test("callable_func", callable(len), True)
test("callable_int", callable(42), False)
test("callable_class", callable(Point), True)

def my_fn():
    return 0
test("callable_def", callable(my_fn), True)

# ── isinstance with tuple ──
test("isinstance_int", isinstance(42, int), True)
test("isinstance_str", isinstance("hi", str), True)
test("isinstance_tuple", isinstance(42, (str, int)), True)
test("isinstance_tuple_false", isinstance(42, (str, list)), False)

# ── issubclass ──
class Animal:
    kind = "animal"
class Dog(Animal):
    kind = "dog"
class Cat(Animal):
    kind = "cat"
test("issubclass_true", issubclass(Dog, Animal), True)
test("issubclass_false", issubclass(Dog, Cat), False)
test("issubclass_self", issubclass(Dog, Dog), True)

# ── id() returns unique values ──
a = [1, 2]
b = [1, 2]
test("id_same", id(a) == id(a), True)
test("id_diff", id(a) != id(b), True)

# ── divmod ──
test("divmod_basic", divmod(17, 5), (3, 2))
test("divmod_neg", divmod(-17, 5), (-4, 3))

# ── pow with modulus ──
test("pow_mod", pow(2, 10, 1000), 24)
test("pow_basic", pow(2, 10), 1024)

# ── round ──
test("round_int", round(3.7), 4)
test("round_ndigits", round(3.14159, 2), 3.14)
test("round_neg", round(-2.6), -3)

# ── sum with start ──
test("sum_start", sum([1, 2, 3], 10), 16)

# ── min/max with key ──
test("min_basic", min(3, 1, 2), 1)
test("max_basic", max(3, 1, 2), 3)
test("min_list", min([3, 1, 2]), 1)
test("max_list", max([3, 1, 2]), 3)

# ── all / any ──
test("all_true", all([1, 2, 3]), True)
test("all_false", all([1, 0, 3]), False)
test("all_empty", all([]), True)
test("any_true", any([0, 0, 1]), True)
test("any_false", any([0, 0, 0]), False)
test("any_empty", any([]), False)

# ── bin / hex / oct ──
test("bin_10", bin(10), "0b1010")
test("hex_255", hex(255), "0xff")
test("oct_8", oct(8), "0o10")

# ── string methods: more coverage ──
test("str_title", "hello world".title(), "Hello World")
test("str_capitalize", "hello world".capitalize(), "Hello world")
test("str_swapcase", "Hello World".swapcase(), "hELLO wORLD")
test("str_center", "hi".center(8), "   hi   ")
test("str_center_char", "hi".center(8, "*"), "***hi***")
test("str_ljust", "hi".ljust(5), "hi   ")
test("str_rjust", "hi".rjust(5), "   hi")
test("str_zfill", "42".zfill(5), "00042")
test("str_count", "banana".count("an"), 2)
test("str_index", "hello".index("ll"), 2)
test("str_expandtabs", "a\tb\tc".expandtabs(4), "a   b   c")

# ── string isXxx methods ──
test("str_isalpha", "abc".isalpha(), True)
test("str_isalpha2", "abc1".isalpha(), False)
test("str_isdigit", "123".isdigit(), True)
test("str_isdigit2", "12a".isdigit(), False)
test("str_isalnum", "abc123".isalnum(), True)
test("str_isupper", "ABC".isupper(), True)
test("str_islower", "abc".islower(), True)
test("str_isspace", "  \t\n".isspace(), True)

# ── list methods: more coverage ──
lst = [3, 1, 4, 1, 5]
test("list_count", lst.count(1), 2)
test("list_index", lst.index(4), 2)

lst2 = [1, 2, 3]
lst2.insert(1, 99)
test("list_insert", lst2, [1, 99, 2, 3])

lst3 = [1, 2, 3]
lst3.extend([4, 5])
test("list_extend", lst3, [1, 2, 3, 4, 5])

lst4 = [1, 2, 3]
lst4.clear()
test("list_clear", lst4, [])

lst5 = [1, 2, 3]
c = lst5.copy()
c.append(4)
test("list_copy", lst5, [1, 2, 3])  # original unchanged
test("list_copy2", c, [1, 2, 3, 4])

# ── dict methods: more coverage ──
d = {"a": 1, "b": 2, "c": 3}
test("dict_get_default", d.get("x", 42), 42)
test("dict_get_exists", d.get("a"), 1)
test("dict_pop", d.pop("b"), 2)
test("dict_pop_after", "b" in d, False)

d2 = {"x": 1}
d2.setdefault("x", 99)
d2.setdefault("y", 42)
test("dict_setdefault_exists", d2["x"], 1)
test("dict_setdefault_new", d2["y"], 42)

d3 = {"a": 1, "b": 2}
d3.update({"b": 3, "c": 4})
test("dict_update", d3, {"a": 1, "b": 3, "c": 4})

d4 = {"a": 1, "b": 2}
d4.clear()
test("dict_clear", d4, {})

# ── set methods ──
s = {1, 2, 3}
test("set_len", len(s), 3)
test("set_in", 2 in s, True)
test("set_not_in", 5 in s, False)

s2 = {1, 2, 3}
s2.add(4)
test("set_add", 4 in s2, True)

s3 = {1, 2, 3}
s3.discard(2)
test("set_discard", 2 in s3, False)

s3.discard(99)  # no error for missing
test("set_discard_missing", len(s3), 2)

# ── tuple methods ──
t = (1, 2, 3, 2, 1)
test("tuple_count", t.count(2), 2)
test("tuple_index", t.index(3), 2)

# ── chained comparisons ──
test("chain_cmp1", 1 < 2 < 3, True)
test("chain_cmp2", 1 < 2 > 0, True)
test("chain_cmp3", 1 < 2 < 1, False)

# ── augmented assignment ──
x = 10
x += 5
test("iadd", x, 15)
x -= 3
test("isub", x, 12)
x *= 2
test("imul", x, 24)
x //= 5
test("ifloordiv", x, 4)
x **= 3
test("ipow", x, 64)
x %= 10
test("imod", x, 4)

# ── string multiplication ──
test("str_mul", "ab" * 3, "ababab")
test("str_mul_r", 3 * "ab", "ababab")

# ── list multiplication ──
test("list_mul", [1, 2] * 3, [1, 2, 1, 2, 1, 2])
test("list_mul_r", 2 * [1, 2], [1, 2, 1, 2])

# ── multiple assignment ──
a = b = c = 5
test("multi_assign", (a, b, c), (5, 5, 5))

# ── nested tuple unpacking ──
a, (b, c) = 1, (2, 3)
test("nested_unpack", (a, b, c), (1, 2, 3))

# ── star unpacking in assignment ──
first, *rest = [1, 2, 3, 4, 5]
test("star_unpack_first", first, 1)
test("star_unpack_rest", rest, [2, 3, 4, 5])

*init, last = [1, 2, 3, 4, 5]
test("star_unpack_init", init, [1, 2, 3, 4])
test("star_unpack_last", last, 5)

a, *mid, z = [1, 2, 3, 4, 5]
test("star_unpack_mid", mid, [2, 3, 4])

# ── decorator ──
def my_decorator(func):
    def wrapper(*args):
        return func(*args) * 2
    return wrapper

@my_decorator
def add(a, b):
    return a + b

test("decorator_basic", add(3, 4), 14)

# ── class decorator ──
def add_greeting(cls):
    cls.greet = lambda self: "Hello!"
    return cls

@add_greeting
class Person:
    name = "person"

test("class_decorator", Person().greet(), "Hello!")

# ── property ──
class Circle:
    def __init__(self, radius):
        self._radius = radius

    @property
    def radius(self):
        return self._radius

    @property
    def area(self):
        return 3.14159 * self._radius ** 2

c = Circle(5)
test("property_get", c.radius, 5)
test("property_computed", round(c.area, 2), 78.54)

# ── __repr__ and __str__ ──
class Color:
    def __init__(self, name):
        self.name = name
    def __repr__(self):
        return "Color('" + self.name + "')"
    def __str__(self):
        return self.name

c = Color("red")
test("repr_custom", repr(c), "Color('red')")
test("str_custom", str(c), "red")

# ── __eq__ and __ne__ ──
class Vec:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    def __ne__(self, other):
        return not (self == other)

test("eq_custom", Vec(1, 2) == Vec(1, 2), True)
test("ne_custom", Vec(1, 2) != Vec(1, 3), True)

# ── __lt__, __le__, __gt__, __ge__ ──
class Temp:
    def __init__(self, val):
        self.val = val
    def __lt__(self, other):
        return self.val < other.val
    def __le__(self, other):
        return self.val <= other.val
    def __gt__(self, other):
        return self.val > other.val
    def __ge__(self, other):
        return self.val >= other.val

test("lt_custom", Temp(10) < Temp(20), True)
test("gt_custom", Temp(20) > Temp(10), True)
test("le_custom", Temp(10) <= Temp(10), True)
test("ge_custom", Temp(10) >= Temp(10), True)

# ── context manager (__enter__ / __exit__) ──
class ManagedResource:
    log = []
    def __enter__(self):
        ManagedResource.log.append("enter")
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        ManagedResource.log.append("exit")
        return False

with ManagedResource() as r:
    ManagedResource.log.append("body")

test("ctx_mgr", ManagedResource.log, ["enter", "body", "exit"])

# ── generator send ──
def accumulator():
    total = 0
    while True:
        val = yield total
        if val is None:
            break
        total += val

g = accumulator()
next(g)  # prime
test("gen_send1", g.send(10), 10)
test("gen_send2", g.send(20), 30)
test("gen_send3", g.send(5), 35)

# ── multiple except clauses ──
def catch_multi(x):
    try:
        if x == 1:
            raise ValueError("val")
        elif x == 2:
            raise TypeError("typ")
        elif x == 3:
            raise KeyError("key")
        return "ok"
    except ValueError:
        return "ValueError"
    except TypeError:
        return "TypeError"
    except KeyError:
        return "KeyError"

test("multi_except1", catch_multi(1), "ValueError")
test("multi_except2", catch_multi(2), "TypeError")
test("multi_except3", catch_multi(3), "KeyError")
test("multi_except4", catch_multi(0), "ok")

# ── exception message / args ──
try:
    raise ValueError("test message")
except ValueError as e:
    test("exc_args", str(e), "test message")

# ── nested exceptions ──
def nested_exc():
    try:
        try:
            raise ValueError("inner")
        except ValueError:
            raise TypeError("outer")
    except TypeError as e:
        return str(e)

test("nested_exc", nested_exc(), "outer")

# ── finally always runs ──
results = []
try:
    results.append("try")
    raise ValueError("x")
except ValueError:
    results.append("except")
finally:
    results.append("finally")
test("finally_runs", results, ["try", "except", "finally"])

# ── class with __contains__ ──
class EvenNumbers:
    def __contains__(self, item):
        return item % 2 == 0

test("contains_custom", 4 in EvenNumbers(), True)
test("contains_custom2", 3 in EvenNumbers(), False)

# ── class with __len__ and __bool__ ──
class Container:
    def __init__(self, items):
        self.items = items
    def __len__(self):
        return len(self.items)
    def __bool__(self):
        return len(self.items) > 0

test("len_custom", len(Container([1, 2, 3])), 3)
test("bool_custom_true", bool(Container([1])), True)
test("bool_custom_false", bool(Container([])), False)

# ── class __getitem__ / __setitem__ ──
class MyList:
    def __init__(self):
        self.data = {}
    def __getitem__(self, key):
        return self.data.get(key, 0)
    def __setitem__(self, key, val):
        self.data[key] = val

ml = MyList()
ml[5] = 42
test("getitem_custom", ml[5], 42)
test("getitem_default", ml[99], 0)

# ── map / filter as iterators ──
test("map_list", list(map(lambda x: x * 2, [1, 2, 3])), [2, 4, 6])
test("filter_list", list(filter(lambda x: x > 2, [1, 2, 3, 4])), [3, 4])

# ── sorted with key ──
words = ["banana", "apple", "cherry"]
test("sorted_key", sorted(words), ["apple", "banana", "cherry"])

# ── enumerate start ──
test("enumerate_start", list(enumerate(["a", "b"], 1)), [(1, "a"), (2, "b")])

# ── dict comprehension ──
test("dictcomp", {k: k**2 for k in range(4)}, {0: 0, 1: 1, 2: 4, 3: 9})

# ── set comprehension ──
test("setcomp", {x % 3 for x in range(9)}, {0, 1, 2})

# ── generator expression ──
test("genexpr_sum", sum(x*x for x in range(5)), 30)

# ── string format method ──
test("str_format1", "{} + {} = {}".format(1, 2, 3), "1 + 2 = 3")
test("str_format2", "{0} {1} {0}".format("a", "b"), "a b a")
test("str_format_named", "{name} is {age}".format(name="Alice", age=30), "Alice is 30")

# ── string join/split with edge cases ──
test("join_empty", ",".join([]), "")
test("split_limit", "a,b,c,d".split(",", 2), ["a", "b", "c,d"])

print("=" * 40)
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
print("=" * 40)
