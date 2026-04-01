# Phase 21: Advanced features - format specs, isinstance with custom classes,
# __repr__/__str__ on custom classes, __eq__/__hash__, __getitem__/__setitem__,
# __len__, property decorators, multiple inheritance, chained decorators

passed = 0
failed = 0

def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name, "| got:", repr(got), "| expected:", repr(expected))

# Format Specs
test("fmt_d", f"{42:d}", "42")
test("fmt_05d", f"{42:05d}", "00042")
test("fmt_f", f"{3.14159:.2f}", "3.14")
test("fmt_b", f"{10:b}", "1010")
test("fmt_o", f"{8:o}", "10")
test("fmt_x", f"{255:x}", "ff")
test("fmt_X", f"{255:X}", "FF")
test("fmt_left", f"{'hi':<10}", "hi        ")
test("fmt_right", f"{'hi':>10}", "        hi")
test("fmt_center", f"{'hi':^10}", "    hi    ")
test("fmt_fill", f"{'hi':*^10}", "****hi****")
test("fmt_str_format_d", "{:05d}".format(42), "00042")
test("fmt_str_format_f", "{:.2f}".format(3.14159), "3.14")
test("fmt_str_format_left", "{:<10}".format("hi"), "hi        ")

# str.format() with mixed args
test("format_pos", "{0} {1} {0}".format("a", "b"), "a b a")
test("format_kw", "{name} is {age}".format(name="Alice", age=30), "Alice is 30")

# isinstance with custom classes
class Animal:
    def __init__(self, name):
        self.name = name

class Dog(Animal):
    def __init__(self, name, breed):
        self.name = name
        self.breed = breed

class Cat(Animal):
    pass

d = Dog("Rex", "Lab")
c = Cat("Whiskers")
test("isinstance_direct", isinstance(d, Dog), True)
test("isinstance_parent", isinstance(d, Animal), True)
test("isinstance_sibling", isinstance(d, Cat), False)
test("isinstance_tuple", isinstance(d, (Cat, Dog)), True)
test("isinstance_base_int", isinstance(42, int), True)
test("isinstance_base_str", isinstance("hi", str), True)
test("isinstance_base_list", isinstance([], list), True)

# __repr__ and __str__ on custom classes
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __repr__(self):
        return "Point(" + str(self.x) + ", " + str(self.y) + ")"
    def __str__(self):
        return "(" + str(self.x) + ", " + str(self.y) + ")"

p = Point(3, 4)
test("custom_str", str(p), "(3, 4)")
test("custom_repr", repr(p), "Point(3, 4)")

# __eq__ on custom classes
class Vec2:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    def __add__(self, other):
        return Vec2(self.x + other.x, self.y + other.y)
    def __mul__(self, scalar):
        return Vec2(self.x * scalar, self.y * scalar)

v1 = Vec2(1, 2)
v2 = Vec2(1, 2)
v3 = Vec2(3, 4)
test("custom_eq_true", v1 == v2, True)
test("custom_eq_false", v1 == v3, False)
v4 = v1 + v3
test("custom_add", v4 == Vec2(4, 6), True)
v5 = v1 * 3
test("custom_mul", v5 == Vec2(3, 6), True)

# __getitem__ / __setitem__ / __len__
class MyList:
    def __init__(self):
        self.data = []
    def __getitem__(self, idx):
        return self.data[idx]
    def __setitem__(self, idx, val):
        self.data[idx] = val
    def __len__(self):
        return len(self.data)
    def append(self, val):
        self.data.append(val)

ml = MyList()
ml.append(10)
ml.append(20)
ml.append(30)
test("custom_getitem", ml[1], 20)
ml[1] = 25
test("custom_setitem", ml[1], 25)
test("custom_len", len(ml), 3)

# __contains__
class MyRange:
    def __init__(self, start, stop):
        self.start = start
        self.stop = stop
    def __contains__(self, item):
        return self.start <= item < self.stop

r = MyRange(1, 10)
test("custom_contains_true", 5 in r, True)
test("custom_contains_false", 10 in r, False)

# __iter__ / __next__
class CountDown:
    def __init__(self, start):
        self.current = start
    def __iter__(self):
        return self
    def __next__(self):
        if self.current <= 0:
            raise StopIteration
        self.current = self.current - 1
        return self.current + 1

cd = CountDown(3)
result = list(cd)
test("custom_iter", result, [3, 2, 1])

# Multiple inheritance
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

dd = D()
test("mro_diamond", dd.greet(), "B")

# Property decorator
class Circle:
    def __init__(self, radius):
        self._radius = radius

    @property
    def radius(self):
        return self._radius

    @property
    def area(self):
        return 3.14159 * self._radius * self._radius

ci = Circle(5)
test("property_get", ci.radius, 5)
test("property_computed", ci.area, 3.14159 * 25)

# Static methods
class MathUtils:
    @staticmethod
    def add(a, b):
        return a + b

test("staticmethod", MathUtils.add(3, 4), 7)

# Chained string methods
test("chain_strip_upper", "  hello  ".strip().upper(), "HELLO")
test("chain_replace_split", "a-b-c".replace("-", " ").split(), ["a", "b", "c"])
test("chain_lower_startswith", "HELLO".lower().startswith("he"), True)

# Dict comprehension with condition
d = {k: v for k, v in {"a": 1, "b": 2, "c": 3}.items() if v > 1}
test("dictcomp_filter", d, {"b": 2, "c": 3})

# Nested list comprehension with conditions
flat = [x*y for x in range(1, 4) for y in range(1, 4) if x != y]
test("nested_comp_filter", flat, [2, 3, 2, 6, 3, 6])

# Multiple assignment
a = b = c = 42
test("multi_assign", (a, b, c), (42, 42, 42))

# Augmented assignment
x = [1, 2, 3]
x += [4, 5]
test("aug_list_add", x, [1, 2, 3, 4, 5])

# Chained comparison
x = 5
test("chained_cmp", 1 < x < 10, True)
test("chained_cmp_false", 1 < x < 3, False)

# Ternary expression
test("ternary_true", "yes" if True else "no", "yes")
test("ternary_false", "yes" if False else "no", "no")

# Walrus operator
result = []
data = [1, 2, 3, 4, 5]
for x in data:
    if (y := x * 2) > 6:
        result.append(y)
test("walrus_filter", result, [8, 10])

# Lambda
double = lambda x: x * 2
test("lambda_basic", double(5), 10)
add = lambda x, y=1: x + y
test("lambda_default", add(5), 6)
test("lambda_override", add(5, 3), 8)

# map/filter with lambda
test("map_lambda", list(map(lambda x: x ** 2, [1, 2, 3, 4])), [1, 4, 9, 16])
test("filter_lambda", list(filter(lambda x: x % 2 == 0, range(10))), [0, 2, 4, 6, 8])

# sorted with key
words = ["banana", "apple", "cherry"]
test("sorted_key", sorted(words, key=len), ["apple", "banana", "cherry"])
test("sorted_reverse", sorted([3, 1, 2], reverse=True), [3, 2, 1])

# min/max with key
test("min_key", min(["banana", "apple", "cherry"], key=len), "apple")
test("max_key", max(["banana", "apple", "cherry"], key=len), "banana")

# any/all
test("any_true", any([False, True, False]), True)
test("any_false", any([False, False, False]), False)
test("all_true", all([True, True, True]), True)
test("all_false", all([True, False, True]), False)

# sum
test("sum_basic", sum([1, 2, 3, 4, 5]), 15)
test("sum_start", sum([1, 2, 3], 10), 16)

# abs
test("abs_neg", abs(-42), 42)
test("abs_float", abs(-3.14), 3.14)

# divmod
test("divmod", divmod(17, 5), (3, 2))

# pow
test("pow_2", pow(2, 10), 1024)
test("pow_mod", pow(2, 10, 100), 24)

# round
test("round_int", round(3.7), 4)
test("round_ndigits", round(3.14159, 2), 3.14)

# chr / ord
test("chr_65", chr(65), "A")
test("ord_A", ord("A"), 65)

# bin / hex / oct
test("bin_10", bin(10), "0b1010")
test("hex_255", hex(255), "0xff")
test("oct_8", oct(8), "0o10")

# String operations
test("str_find", "hello world".find("world"), 6)
test("str_find_none", "hello".find("xyz"), -1)
test("str_count", "abracadabra".count("a"), 5)
test("str_isdigit", "12345".isdigit(), True)
test("str_isalpha", "hello".isalpha(), True)
test("str_partition", "hello-world".partition("-"), ("hello", "-", "world"))

# Boolean operations
test("bool_and", True and False, False)
test("bool_or", True or False, True)
test("bool_not", not True, False)

# None checks
test("is_none", None is None, True)
test("is_not_none", 42 is not None, True)

# Dict operations
d = {"a": 1, "b": 2, "c": 3}
test("dict_get_default", d.get("x", 0), 0)
test("dict_pop", d.pop("c"), 3)
test("dict_keys", sorted(list(d.keys())), ["a", "b"])
test("dict_values_sum", sum(d.values()), 3)
test("dict_setdefault", d.setdefault("d", 4), 4)
test("dict_setdefault_exists", d.setdefault("a", 99), 1)

# String join
test("join", "-".join(["a", "b", "c"]), "a-b-c")
test("join_empty", "".join(["a", "b", "c"]), "abc")

# Multiple exception handling
try:
    x = 1 / 0
except (TypeError, ZeroDivisionError) as e:
    test("multi_except", True, True)

# Nested try/except
def nested_try():
    try:
        try:
            raise ValueError("inner")
        except ValueError:
            return "caught inner"
    except Exception:
        return "caught outer"

test("nested_try", nested_try(), "caught inner")

# Global/nonlocal
def make_counter():
    count = 0
    def increment():
        nonlocal count
        count += 1
        return count
    return increment

counter = make_counter()
test("nonlocal_1", counter(), 1)
test("nonlocal_2", counter(), 2)
test("nonlocal_3", counter(), 3)

# Generator
def fib(n):
    a, b = 0, 1
    for _ in range(n):
        yield a
        a, b = b, a + b

test("generator_fib", list(fib(8)), [0, 1, 1, 2, 3, 5, 8, 13])

# Complex data structures
data = {"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}
names = [u["name"] for u in data["users"]]
test("complex_data", names, ["Alice", "Bob"])

# List of dicts
records = [{"x": 1}, {"x": 2}, {"x": 3}]
total = sum(r["x"] for r in records)
test("sum_generator", total, 6)

# type() as constructor
test("type_name_int", type(42).__name__, "int")
test("type_name_str", type("hi").__name__, "str")
test("type_name_list", type([]).__name__, "list")

# String multiplication
test("str_mul", "ab" * 3, "ababab")
test("str_mul_rev", 3 * "ab", "ababab")

# List multiplication
test("list_mul", [1, 2] * 3, [1, 2, 1, 2, 1, 2])

# Dict merge
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
d3 = {**d1, **d2}
test("dict_merge", d3, {"a": 1, "b": 3, "c": 4})

# Unpacking in function call
def add3(a, b, c):
    return a + b + c

test("unpack_call", add3(*[1, 2, 3]), 6)

print("=" * 40)
print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL TESTS PASSED!")
print("=" * 40)
