# Ferrython Basic Test Suite
# Tests all currently working features

passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if str(got) == str(expected):
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL:", name, "got", got, "expected", expected)

# ── Arithmetic ──
check("add", 2 + 3, 5)
check("sub", 10 - 4, 6)
check("mul", 7 * 8, 56)
check("div", 15 / 4, 3.75)
check("floordiv", 17 // 5, 3)
check("mod", 17 % 5, 2)
check("pow", 2 ** 10, 1024)
check("neg", -5 + 3, -2)
check("order", 2 + 3 * 4, 14)

# ── Comparisons ──
check("eq", 1 == 1, True)
check("ne", 1 != 2, True)
check("lt", 1 < 2, True)
check("gt", 3 > 2, True)
check("le", 2 <= 2, True)
check("ge", 3 >= 2, True)

# ── Boolean Logic ──
check("and_tt", True and True, True)
check("and_tf", True and False, False)
check("or_ff", False or False, False)
check("or_tf", True or False, True)
check("not_t", not True, False)
check("not_f", not False, True)

# ── Strings ──
check("str_cat", "hello" + " " + "world", "hello world")
check("str_mul", "ab" * 3, "ababab")
check("str_len", len("hello"), 5)
check("str_bool_empty", bool(""), False)
check("str_bool", bool("x"), True)

# ── Type Conversions ──
check("int_str", int("42"), 42)
check("str_int", str(123), "123")
check("float_int", float(3), 3.0)
check("bool_0", bool(0), False)
check("bool_1", bool(1), True)
check("bool_none", bool(None), False)

# ── Lists ──
a = [1, 2, 3, 4, 5]
check("list_len", len(a), 5)
check("list_idx", a[0], 1)
check("list_idx2", a[2], 3)
check("list_concat", [1, 2] + [3, 4], [1, 2, 3, 4])

# ── Dicts ──
d = {"name": "ferrython", "version": 1}
check("dict_get", d["name"], "ferrython")
check("dict_len", len(d), 2)

# ── Tuples ──
t = (10, 20, 30)
check("tuple_len", len(t), 3)
check("tuple_idx", t[1], 20)

# ── Tuple Unpacking ──
x, y = 1, 2
check("unpack_basic", x, 1)
check("unpack_basic2", y, 2)
x, y = y, x
check("swap_x", x, 2)
check("swap_y", y, 1)
a, b, c = [10, 20, 30]
check("unpack_list", b, 20)

# ── Control Flow ──
result = "yes" if True else "no"
check("ifexpr_t", result, "yes")
result = "yes" if False else "no"
check("ifexpr_f", result, "no")

# ── While Loop ──
n = 1
while n < 100:
    n = n * 2
check("while", n, 128)

# ── For Loop with Range ──
total = 0
for i in range(10):
    total = total + i
check("for_range", total, 45)

# ── Nested Loops ──
count = 0
for i in range(3):
    for j in range(4):
        count = count + 1
check("nested_loops", count, 12)

# ── For Loop with List ──
items = [10, 20, 30]
total = 0
for item in items:
    total = total + item
check("for_list", total, 60)

# ── Break ──
found = -1
for i in range(100):
    if i > 5:
        found = i
        break
check("break", found, 6)

# ── Functions ──
def add(a, b):
    return a + b

check("func_call", add(3, 4), 7)

def factorial(n):
    if n <= 1:
        return 1
    return n * factorial(n - 1)

check("recursion", factorial(10), 3628800)

# ── Default Args ──
def greet(name, greeting="Hello"):
    return greeting + ", " + name + "!"

check("default_arg", greet("World"), "Hello, World!")

# ── Global Variables ──
counter = 0
def inc():
    global counter
    counter = counter + 1

inc()
inc()
inc()
check("global_var", counter, 3)

# ── Fibonacci ──
def fib(n):
    if n <= 1:
        return n
    a, b = 0, 1
    for i in range(2, n + 1):
        a, b = b, a + b
    return b

check("fib_0", fib(0), 0)
check("fib_1", fib(1), 1)
check("fib_10", fib(10), 55)
check("fib_20", fib(20), 6765)

# ── If/Elif/Else ──
def classify(n):
    if n > 0:
        return "positive"
    elif n < 0:
        return "negative"
    else:
        return "zero"

check("elif_pos", classify(5), "positive")
check("elif_neg", classify(-3), "negative")
check("elif_zero", classify(0), "zero")

# ── Multiple Return ──
def divmod_fn(a, b):
    return a // b, a % b

q, r = divmod_fn(17, 5)
check("multi_return_q", q, 3)
check("multi_return_r", r, 2)


# ── Default Arguments ──
def greet(name, greeting="Hello"):
    return greeting + ", " + name + "!"

check("default_arg1", greet("World"), "Hello, World!")
check("default_arg2", greet("World", "Hi"), "Hi, World!")

def add3(a, b=10, c=100):
    return a + b + c

check("default_multi1", add3(1), 111)
check("default_multi2", add3(1, 2), 103)
check("default_multi3", add3(1, 2, 3), 6)

# ── Continue ──
odds = 0
for i in range(10):
    if i % 2 == 0:
        continue
    odds = odds + i
check("continue", odds, 25)

# ── In / Not In ──
check("in_list", 3 in [1, 2, 3, 4], True)
check("not_in_list", 5 in [1, 2, 3, 4], False)
check("in_str", "lo" in "hello", True)
check("not_in_str", "xyz" not in "hello", True)
check("in_dict", "name" in {"name": "test"}, True)

# ── String Methods ──
check("upper", "hello".upper(), "HELLO")
check("lower", "HELLO".lower(), "hello")
check("strip", "  hi  ".strip(), "hi")
check("replace", "hello world".replace("world", "python"), "hello python")
check("find", "hello".find("ll"), 2)
check("find_miss", "hello".find("xyz"), -1)
check("count", "hello".count("l"), 2)
check("startswith", "hello".startswith("hel"), True)
check("endswith", "hello".endswith("llo"), True)
check("isdigit", "123".isdigit(), True)
check("isalpha", "abc".isalpha(), True)
check("join", ", ".join(["a", "b", "c"]), "a, b, c")
check("split", "a,b,c".split(","), ["a", "b", "c"])
check("capitalize", "hello".capitalize(), "Hello")
check("title", "hello world".title(), "Hello World")

# ── str.format ──
check("format1", "{} + {} = {}".format(1, 2, 3), "1 + 2 = 3")
check("format2", "name: {}".format("Alice"), "name: Alice")

# ── Dict Methods ──
d2 = {"a": 1, "b": 2, "c": 3}
check("dict_get", d2.get("a"), 1)
check("dict_get_default", d2.get("z", 99), 99)
check("dict_keys", len(d2.keys()), 3)
check("dict_values", len(d2.values()), 3)

# ── Classes ──
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    
    def distance(self):
        return (self.x ** 2 + self.y ** 2) ** 0.5
    
    def translate(self, dx, dy):
        return Point(self.x + dx, self.y + dy)

p = Point(3, 4)
check("class_attr", p.x, 3)
check("class_method", p.distance(), 5.0)
p2 = p.translate(1, 1)
check("class_translate_x", p2.x, 4)
check("class_translate_y", p2.y, 5)

# ── Class with default args ──
class Counter:
    def __init__(self, start=0):
        self.value = start
    
    def inc(self):
        self.value = self.value + 1
    
    def get(self):
        return self.value

c = Counter()
check("class_default", c.get(), 0)
c.inc()
c.inc()
c.inc()
check("class_inc", c.get(), 3)

c2 = Counter(10)
check("class_start", c2.get(), 10)

# ── Update summary ──
print()
print("=" * 40)
total = passed + failed
print("Tests:", total, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("SOME TESTS FAILED")
