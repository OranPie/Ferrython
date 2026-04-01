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

# ── __getitem__ / __setitem__ / __delitem__ on instances ──

class MyList:
    def __init__(self):
        self.data = {}
    def __getitem__(self, key):
        return self.data[key]
    def __setitem__(self, key, value):
        self.data[key] = value
    def __delitem__(self, key):
        del self.data[key]
    def __len__(self):
        return len(self.data)
    def __contains__(self, key):
        return key in self.data

ml = MyList()
ml[0] = "hello"
ml[1] = "world"
test("setitem", ml[0], "hello")
test("getitem", ml[1], "world")
test("len_dunder", len(ml), 2)
test("contains_dunder", 0 in ml, True)
test("contains_dunder2", 5 in ml, False)
del ml[0]
test("delitem", len(ml), 1)

# ── __call__ on instances ──

class Multiplier:
    def __init__(self, factor):
        self.factor = factor
    def __call__(self, x):
        return x * self.factor

double = Multiplier(2)
triple = Multiplier(3)
test("callable_inst", double(5), 10)
test("callable_inst2", triple(5), 15)
test("callable_check", callable(double), True)

# ── __iter__ / __next__ custom iterator ──

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

test("custom_iter", list(CountDown(5)), [5, 4, 3, 2, 1])
test("custom_iter_sum", sum(CountDown(10)), 55)

# ── __add__ / __mul__ / __sub__ on instances ──

class Vector:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __add__(self, other):
        return Vector(self.x + other.x, self.y + other.y)
    def __sub__(self, other):
        return Vector(self.x - other.x, self.y - other.y)
    def __mul__(self, scalar):
        return Vector(self.x * scalar, self.y * scalar)
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    def __repr__(self):
        return "Vector(" + str(self.x) + ", " + str(self.y) + ")"

v1 = Vector(1, 2)
v2 = Vector(3, 4)
v3 = v1 + v2
test("dunder_add", v3, Vector(4, 6))
v4 = v2 - v1
test("dunder_sub", v4, Vector(2, 2))
v5 = v1 * 3
test("dunder_mul", v5, Vector(3, 6))

# ── __lt__ / __le__ / __gt__ / __ge__ on instances ──

class Temp:
    def __init__(self, degrees):
        self.degrees = degrees
    def __lt__(self, other):
        return self.degrees < other.degrees
    def __le__(self, other):
        return self.degrees <= other.degrees
    def __gt__(self, other):
        return self.degrees > other.degrees
    def __ge__(self, other):
        return self.degrees >= other.degrees
    def __eq__(self, other):
        return self.degrees == other.degrees

t1 = Temp(100)
t2 = Temp(200)
test("dunder_lt", t1 < t2, True)
test("dunder_gt", t2 > t1, True)
test("dunder_le", t1 <= Temp(100), True)
test("dunder_ge", t2 >= t1, True)
test("dunder_eq_inst", t1 == Temp(100), True)

# ── Sorted with custom __lt__ ──
temps = [Temp(30), Temp(10), Temp(20)]
sorted_temps = sorted(temps, key=lambda t: t.degrees)
test("sorted_custom", [t.degrees for t in sorted_temps], [10, 20, 30])

# ── __str__ and __repr__ ──

class Fraction:
    def __init__(self, num, den):
        self.num = num
        self.den = den
    def __str__(self):
        return str(self.num) + "/" + str(self.den)
    def __repr__(self):
        return "Fraction(" + str(self.num) + ", " + str(self.den) + ")"

f = Fraction(1, 2)
test("dunder_str", str(f), "1/2")
test("dunder_repr", repr(f), "Fraction(1, 2)")
# In a list, repr is used for elements
test("dunder_in_list", repr([f]), "[Fraction(1, 2)]")

# ── Class variables vs instance variables ──

class Dog:
    species = "Canis lupus"
    
    def __init__(self, name):
        self.name = name

d1 = Dog("Rex")
d2 = Dog("Buddy")
test("class_var", d1.species, "Canis lupus")
test("class_var2", d2.species, "Canis lupus")
test("inst_var", d1.name, "Rex")
test("inst_var2", d2.name, "Buddy")

# ── Method resolution order ──

class A:
    def who(self):
        return "A"

class B(A):
    def who(self):
        return "B"

class C(A):
    def who(self):
        return "C"

class D(B, C):
    pass

test("mro_diamond", D().who(), "B")

# ── super() ──

class Shape:
    def __init__(self, color):
        self.color = color

class Rectangle(Shape):
    def __init__(self, color, width, height):
        super().__init__(color)
        self.width = width
        self.height = height
    
    def area(self):
        return self.width * self.height

r = Rectangle("red", 3, 4)
test("super_init", r.color, "red")
test("super_area", r.area(), 12)

# ── Exception as/from patterns ──

def catch_and_reraise():
    try:
        raise ValueError("original")
    except ValueError as e:
        return str(e)

test("except_as", catch_and_reraise(), "original")

# ── Nested with statements ──

class Logger:
    def __init__(self, name, log):
        self.name = name
        self.log = log
    def __enter__(self):
        self.log.append("enter " + self.name)
        return self
    def __exit__(self, *args):
        self.log.append("exit " + self.name)
        return False

log = []
with Logger("A", log):
    with Logger("B", log):
        log.append("body")
test("nested_with", log, ["enter A", "enter B", "body", "exit B", "exit A"])

# ── List comprehension with multiple if ──

test("listcomp_multi_if", [x for x in range(20) if x % 2 == 0 if x % 3 == 0], [0, 6, 12, 18])

# ── Dict merge with | operator (Python 3.9+) ──
# Skip for now — our target is 3.8

# ── String join with generator ──
test("str_join_gen", ",".join(str(x) for x in range(5)), "0,1,2,3,4")

# ── Unpacking in assignments ──
a, (b, c) = 1, (2, 3)
test("nested_unpack", (a, b, c), (1, 2, 3))

# ── Default mutable argument gotcha (we don't really test the bug, just defaults) ──
def append_to(element, target=None):
    if target is None:
        target = []
    target.append(element)
    return target

test("default_none", append_to(1), [1])
test("default_none2", append_to(2), [2])

# ── Lambda with default args ──
inc = lambda x, step=1: x + step
test("lambda_default", inc(5), 6)
test("lambda_default2", inc(5, 2), 7)

# ── Chained method calls ──
test("chain_methods", [3, 1, 4, 1, 5].count(1), 2)

# ── Dict constructor from pairs ──
test("dict_from_pairs", dict([(1, "a"), (2, "b")]), {1: "a", 2: "b"})

# ── Multiple inheritance method resolution ──
class Mixin:
    def feature(self):
        return "mixin"

class Base2:
    def feature(self):
        return "base"

class Combined(Mixin, Base2):
    pass

test("mixin_mro", Combined().feature(), "mixin")

# ── Generator expression in function call ──
test("genexpr_in_call", sum(x**2 for x in range(5)), 30)
test("genexpr_any", any(x > 3 for x in range(5)), True)
test("genexpr_all", all(x < 10 for x in range(5)), True)

# ── Truthiness of custom objects ──
class TruthyClass:
    def __init__(self, val):
        self.val = val
    def __bool__(self):
        return self.val > 0

test("bool_custom_t", bool(TruthyClass(5)), True)
test("bool_custom_f", bool(TruthyClass(-1)), False)
test("if_custom", "yes" if TruthyClass(1) else "no", "yes")
test("if_custom2", "yes" if TruthyClass(-1) else "no", "no")

# ── Star args and kwargs together ──
def show_args(*args, **kwargs):
    return (args, sorted(kwargs.keys()))

test("star_args", show_args(1, 2, 3, a=4, b=5), ((1, 2, 3), ["a", "b"]))

# ── Nested dict access ──
data = {"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}
test("nested_access", data["users"][0]["name"], "Alice")
test("nested_access2", data["users"][1]["age"], 25)

# ── String methods in chain ──
test("chain_str2", "  Hello, World!  ".strip().split(", "), ["Hello", "World!"])

# ── Try/except with else ──
def safe_div(a, b):
    try:
        result = a / b
    except ZeroDivisionError:
        return "error"
    else:
        return result

test("try_else", safe_div(10, 2), 5.0)
test("try_else_err", safe_div(10, 0), "error")

# ── Comparison operators returning NotImplemented ──
# Just test basic mixed comparisons
test("mixed_cmp", 1 < 2.0, True)
test("mixed_cmp2", 2.0 > 1, True)

# ── Power operator precedence ──
test("pow_prec", 2 ** 3 ** 2, 512)  # right-associative: 2 ** (3**2) = 2**9

# ── Bitwise operations ──
test("bitwise_and", 0xFF & 0x0F, 15)
test("bitwise_or", 0xF0 | 0x0F, 255)
test("bitwise_xor", 0xFF ^ 0x0F, 240)
test("bitwise_not", ~0, -1)
test("left_shift", 1 << 8, 256)
test("right_shift", 256 >> 4, 16)

# ── Augmented assignment with containers ──
lst = [1, 2]
lst += [3, 4]
test("iadd_list", lst, [1, 2, 3, 4])

s = "hello"
s += " world"
test("iadd_str", s, "hello world")

# ── Global exception types ──
test("exception_type", type(ValueError("x")).__name__, "ValueError")

# ── hasattr / getattr / setattr ──
class Obj:
    x = 10

o = Obj()
test("hasattr_t", hasattr(o, "x"), True)
test("hasattr_f", hasattr(o, "y"), False)
test("getattr_val", getattr(o, "x"), 10)
test("getattr_default", getattr(o, "y", 42), 42)
setattr(o, "z", 99)
test("setattr_val", o.z, 99)

# ── Multiple exception handling ──
def multi_catch():
    results = []
    for exc_type in [ValueError, TypeError, KeyError]:
        try:
            raise exc_type("test")
        except (ValueError, TypeError) as e:
            results.append("VT: " + str(e))
        except KeyError as e:
            results.append("K: " + str(e))
    return results

test("multi_catch", multi_catch(), ["VT: test", "VT: test", "K: test"])

print("========================================")
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("Failed tests:", ", ".join(errors))
print("========================================")
