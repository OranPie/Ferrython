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

# ── enumerate returns iterator ──
e = enumerate(["a", "b", "c"])
test("enum_iter1", next(e), (0, "a"))
test("enum_iter2", next(e), (1, "b"))
test("enum_iter3", next(e), (2, "c"))

# enumerate in for loop
result = []
for i, v in enumerate(["x", "y", "z"], 1):
    result.append((i, v))
test("enum_for", result, [(1, "x"), (2, "y"), (3, "z")])

# enumerate with list()
test("enum_list", list(enumerate("abc")), [(0, "a"), (1, "b"), (2, "c")])

# ── zip returns iterator ──
z = zip([1, 2, 3], ["a", "b", "c"])
test("zip_iter1", next(z), (1, "a"))
test("zip_iter2", next(z), (2, "b"))

# zip in for loop
result = []
for a, b in zip([1, 2], [10, 20]):
    result.append(a + b)
test("zip_for", result, [11, 22])

# zip with list()
test("zip_list", list(zip([1, 2], [3, 4])), [(1, 3), (2, 4)])

# zip different lengths
test("zip_short", list(zip([1, 2, 3], [10, 20])), [(1, 10), (2, 20)])

# ── reversed returns iterator ──
r = reversed([1, 2, 3, 4, 5])
test("rev_iter1", next(r), 5)
test("rev_iter2", next(r), 4)
test("rev_list", list(reversed([1, 2, 3])), [3, 2, 1])

# reversed string
test("rev_str", list(reversed("abc")), ["c", "b", "a"])

# ── dict unpacking {**d1, **d2} ──
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
test("dict_unpack", {**d1, **d2}, {"a": 1, "b": 3, "c": 4})
test("dict_unpack_mixed", {**d1, "x": 10}, {"a": 1, "b": 2, "x": 10})

# ── list unpacking [*a, *b] ──
a = [1, 2]
b = [3, 4]
test("list_unpack", [*a, *b], [1, 2, 3, 4])
test("list_unpack_mixed", [0, *a, 99, *b], [0, 1, 2, 99, 3, 4])

# ── tuple unpacking (*a, *b) ──
test("tuple_unpack", (*a, *b), (1, 2, 3, 4))
test("tuple_unpack_mixed", (0, *a, *b, 5), (0, 1, 2, 3, 4, 5))

# ── __int__ and __float__ dispatch ──
class Temperature:
    def __init__(self, celsius):
        self.celsius = celsius
    def __int__(self):
        return int(self.celsius)
    def __float__(self):
        return float(self.celsius)
    def __bool__(self):
        return self.celsius != 0

t = Temperature(36.6)
test("int_dispatch", int(t), 36)
test("float_dispatch", float(t), 36.6)
test("bool_dispatch", bool(t), True)
test("bool_dispatch_zero", bool(Temperature(0)), False)

# ── __len__ dispatch ──
class FixedSize:
    def __init__(self, n):
        self.n = n
    def __len__(self):
        return self.n

test("len_dispatch", len(FixedSize(42)), 42)

# ── Custom iterable with iter() and next() ──
class Fibonacci:
    def __init__(self, limit):
        self.limit = limit
        self.a = 0
        self.b = 1
    def __iter__(self):
        return self
    def __next__(self):
        if self.a >= self.limit:
            raise StopIteration
        val = self.a
        self.a, self.b = self.b, self.a + self.b
        return val

fibs = list(Fibonacci(20))
test("custom_fib", fibs, [0, 1, 1, 2, 3, 5, 8, 13])

# iter() builtin with custom __iter__
it = iter(Fibonacci(10))
test("iter_builtin", next(it), 0)
test("iter_builtin2", next(it), 1)

# ── Reflected operations ──
class Vector:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __add__(self, other):
        if isinstance(other, Vector):
            return Vector(self.x + other.x, self.y + other.y)
        return Vector(self.x + other, self.y + other)
    def __radd__(self, other):
        return Vector(other + self.x, other + self.y)
    def __mul__(self, other):
        return Vector(self.x * other, self.y * other)
    def __rmul__(self, other):
        return Vector(other * self.x, other * self.y)
    def __neg__(self):
        return Vector(-self.x, -self.y)
    def __repr__(self):
        return "Vector({}, {})".format(self.x, self.y)

v = Vector(1, 2)
test("radd_vec", repr(10 + v), "Vector(11, 12)")
test("rmul_vec", repr(3 * v), "Vector(3, 6)")
test("neg_vec", repr(-v), "Vector(-1, -2)")

# ── Bitwise on instances ──
class BitField:
    def __init__(self, val):
        self.val = val
    def __and__(self, other):
        return BitField(self.val & other.val)
    def __or__(self, other):
        return BitField(self.val | other.val)
    def __xor__(self, other):
        return BitField(self.val ^ other.val)
    def __lshift__(self, n):
        return BitField(self.val << n)
    def __rshift__(self, n):
        return BitField(self.val >> n)

bf1 = BitField(0xFF)
bf2 = BitField(0x0F)
test("bitand_inst", (bf1 & bf2).val, 0x0F)
test("bitor_inst", (bf1 | bf2).val, 0xFF)
test("bitxor_inst", (bf1 ^ bf2).val, 0xF0)
test("lshift_inst", (bf2 << 4).val, 0xF0)
test("rshift_inst", (bf1 >> 4).val, 0x0F)

# ── Functional programming patterns ──
# sorted with key
data = [(3, "c"), (1, "a"), (2, "b")]
test("sorted_key_tuple", sorted(data, key=lambda x: x[0]), [(1, "a"), (2, "b"), (3, "c")])

# map as iterator
m = map(lambda x: x * 2, [1, 2, 3])
test("map_next", next(m), 2)
test("map_list", list(map(str, [1, 2, 3])), ["1", "2", "3"])

# filter as iterator  
f = filter(lambda x: x > 0, [-2, -1, 0, 1, 2])
test("filter_next", next(f), 1)

# ── Complex class hierarchy ──
class Drawable:
    def draw(self):
        return "drawable"

class Colorable:
    def __init__(self):
        self.color = "red"
    def get_color(self):
        return self.color

class Shape(Drawable, Colorable):
    def __init__(self, name):
        Colorable.__init__(self)
        self.name = name
    def draw(self):
        return "shape:" + self.name

class Circle(Shape):
    def __init__(self, radius):
        super().__init__("circle")
        self.radius = radius
    def area(self):
        return 3.14159 * self.radius * self.radius

c = Circle(5)
test("multi_inherit_draw", c.draw(), "shape:circle")
test("multi_inherit_color", c.get_color(), "red")
test("circle_area", round(c.area(), 2), 78.54)

# ── Decorator patterns ──
def memoize(func):
    cache = {}
    def wrapper(n):
        if n not in cache:
            cache[n] = func(n)
        return cache[n]
    return wrapper

@memoize
def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

test("memoize_fib", fib(10), 55)
test("memoize_fib20", fib(20), 6765)

# ── String operations ──
test("str_split_join", "-".join("hello world".split()), "hello-world")
test("str_replace", "aabbbcc".replace("b", "x"), "aaxxxcc")
test("str_count", "abcabcabc".count("abc"), 3)
test("str_find", "hello world".find("world"), 6)
test("str_upper", "hello".upper(), "HELLO")
test("str_lower", "HELLO".lower(), "hello")
test("str_strip", "  hello  ".strip(), "hello")

# ── Numeric operations ──
test("divmod_op", divmod(17, 5), (3, 2))
test("pow_mod", pow(2, 10, 1000), 24)
test("round_float", round(3.14159, 2), 3.14)
test("abs_neg", abs(-42), 42)
test("min_max", (min(3, 1, 4), max(3, 1, 4)), (1, 4))

# ── Dict operations ──
d = {"a": 1, "b": 2, "c": 3}
test("dict_keys", sorted(d.keys()), ["a", "b", "c"])
test("dict_values", sorted(d.values()), [1, 2, 3])
test("dict_items", sorted(d.items()), [("a", 1), ("b", 2), ("c", 3)])
test("dict_get_def", d.get("x", 99), 99)
test("dict_pop", d.pop("b"), 2)
test("dict_after_pop", len(d), 2)

# ── Exception patterns ──
def safe_divide(a, b):
    try:
        return a / b
    except ZeroDivisionError:
        return None

test("safe_div_ok", safe_divide(10, 3), 10 / 3)
test("safe_div_zero", safe_divide(10, 0), None)

# Nested exception
def nested_exc():
    try:
        try:
            x = 1 / 0
        except ZeroDivisionError:
            raise ValueError("converted")
    except ValueError as e:
        return str(e)

test("nested_exc", nested_exc(), "converted")

# ── Generator patterns ──
def chunks(lst, n):
    for i in range(0, len(lst), n):
        yield lst[i:i + n]

test("chunks", list(chunks([1, 2, 3, 4, 5], 2)), [[1, 2], [3, 4], [5]])

# Generator with yield from
def flatten(nested):
    for item in nested:
        if isinstance(item, list):
            yield from flatten(item)
        else:
            yield item

test("flatten", list(flatten([1, [2, 3], [4, [5, 6]]])), [1, 2, 3, 4, 5, 6])

# ── Context manager ──
class Ctx:
    def __init__(self):
        self.entered = False
        self.exited = False
    def __enter__(self):
        self.entered = True
        return self
    def __exit__(self, *args):
        self.exited = True
        return False

ctx = Ctx()
with ctx as c:
    test("ctx_entered", c.entered, True)
    test("ctx_not_exited", c.exited, False)
test("ctx_exited", ctx.exited, True)

print("========================================")
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("Failed tests:", ", ".join(errors))
print("========================================")
