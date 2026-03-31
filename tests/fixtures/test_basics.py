# Ferrython comprehensive test suite
passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL:", name, "got:", got, "expected:", expected)

# ═══ ARITHMETIC ═══
check("add", 2 + 3, 5)
check("sub", 10 - 4, 6)
check("mul", 3 * 7, 21)
check("truediv", 10 / 4, 2.5)
check("floordiv", 10 // 3, 3)
check("mod", 10 % 3, 1)
check("pow", 2 ** 10, 1024)
check("neg", -5 + 3, -2)
check("complex_expr", (2 + 3) * 4 - 1, 19)

# ═══ COMPARISONS ═══
check("lt", 3 < 5, True)
check("le", 5 <= 5, True)
check("gt", 7 > 3, True)
check("ge", 4 >= 4, True)
check("eq", 42 == 42, True)
check("ne", 42 != 43, True)
check("chain", 1 < 2 < 3, True)

# ═══ BOOLEANS ═══
check("and", True and True, True)
check("and_false", True and False, False)
check("or", False or True, True)
check("not_t", not True, False)
check("not_f", not False, True)
check("and_short", False and (1/0), False)
check("or_short", True or (1/0), True)

# ═══ STRINGS ═══
check("str_concat", "hello" + " " + "world", "hello world")
check("str_mul", "ab" * 3, "ababab")
check("str_len", len("hello"), 5)
check("str_idx", "hello"[1], "e")
check("str_neg_idx", "hello"[-1], "o")
check("str_upper", "hello".upper(), "HELLO")
check("str_lower", "HELLO".lower(), "hello")
check("str_strip", "  hi  ".strip(), "hi")
check("str_split", "a,b,c".split(","), ["a", "b", "c"])
check("str_join", "-".join(["a", "b", "c"]), "a-b-c")
check("str_replace", "hello".replace("l", "r"), "herro")
check("str_find", "hello".find("ll"), 2)
check("str_count", "banana".count("a"), 3)
check("str_startswith", "hello".startswith("hel"), True)
check("str_endswith", "hello".endswith("llo"), True)
check("str_isdigit", "123".isdigit(), True)
check("str_isalpha", "abc".isalpha(), True)
check("str_title", "hello world".title(), "Hello World")
check("str_capitalize", "hello world".capitalize(), "Hello world")
check("str_swapcase", "Hello".swapcase(), "hELLO")
check("str_center", "hi".center(6), "  hi  ")
check("str_zfill", "42".zfill(5), "00042")
check("str_format", "Hello, {}!".format("World"), "Hello, World!")
check("str_format2", "{} + {} = {}".format(1, 2, 3), "1 + 2 = 3")

# ═══ TYPE CONVERSIONS ═══
check("int_str", int("42"), 42)
check("float_str", float("3.14"), 3.14)
check("str_int", str(42), "42")
check("bool_0", bool(0), False)
check("bool_1", bool(1), True)
check("bool_empty", bool(""), False)
check("bool_nonempty", bool("x"), True)

# ═══ LISTS ═══
a = [1, 2, 3]
check("list_len", len(a), 3)
check("list_idx", a[0], 1)
check("list_neg", a[-1], 3)
check("list_concat", [1, 2] + [3, 4], [1, 2, 3, 4])
check("list_mul", [1, 2] * 2, [1, 2, 1, 2])
check("list_in", 2 in [1, 2, 3], True)
check("list_not_in", 5 not in [1, 2, 3], True)

# List mutability
lst = [3, 1, 4, 1, 5]
lst.append(9)
check("list_append", lst[-1], 9)
lst.pop()
check("list_pop", lst[-1], 5)
lst.insert(0, 0)
check("list_insert", lst[0], 0)
lst.remove(4)
check("list_remove", 4 not in lst, True)
lst.sort()
check("list_sort", lst, [0, 1, 1, 3, 5])
lst.reverse()
check("list_reverse", lst, [5, 3, 1, 1, 0])
lst.clear()
check("list_clear", len(lst), 0)

# List item assignment
m = [10, 20, 30]
m[1] = 99
check("list_setitem", m[1], 99)

# ═══ DICTS ═══
d = {"a": 1, "b": 2, "c": 3}
check("dict_get", d["a"], 1)
check("dict_len", len(d), 3)
check("dict_in", "b" in d, True)
check("dict_keys_len", len(d.keys()), 3)
check("dict_values", 2 in d.values(), True)
check("dict_get_default", d.get("x", 0), 0)

# ═══ TUPLES ═══
t = (1, 2, 3)
check("tuple_len", len(t), 3)
check("tuple_idx", t[1], 2)
check("tuple_unpack_basic", True, True)
x, y = 1, 2
check("unpack_x", x, 1)
check("unpack_y", y, 2)
a, b, c = [10, 20, 30]
check("unpack_list", b, 20)
# Swap
x, y = y, x
check("swap", x, 2)

# ═══ CONTROL FLOW ═══
# if/elif/else
x = 15
if x > 20:
    r = "big"
elif x > 10:
    r = "medium"
else:
    r = "small"
check("elif", r, "medium")

# ternary
check("ternary", "yes" if True else "no", "yes")

# while
s = 0
i = 1
while i <= 10:
    s = s + i
    i = i + 1
check("while_sum", s, 55)

# for + range
s = 0
for i in range(5):
    s = s + i
check("for_range", s, 10)

# for over list
words = ["hello", "world"]
result = ""
for w in words:
    result = result + w + " "
check("for_list", result.strip(), "hello world")

# nested for
pairs = []
for i in range(3):
    for j in range(3):
        if i != j:
            pairs.append((i, j))
check("nested_for", len(pairs), 6)

# break
found = -1
for i in range(10):
    if i == 7:
        found = i
        break
check("break", found, 7)

# continue
evens = []
for i in range(10):
    if i % 2 != 0:
        continue
    evens.append(i)
check("continue", evens, [0, 2, 4, 6, 8])

# ═══ FUNCTIONS ═══
def add(a, b):
    return a + b
check("func_basic", add(3, 4), 7)

def factorial(n):
    if n <= 1:
        return 1
    return n * factorial(n - 1)
check("recursion", factorial(10), 3628800)

def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)
check("fibonacci", fib(10), 55)

def greet(name, greeting="Hello"):
    return greeting + ", " + name + "!"
check("default_arg", greet("World"), "Hello, World!")
check("override_default", greet("World", "Hi"), "Hi, World!")

counter = 0
def inc():
    global counter
    counter = counter + 1
inc()
inc()
inc()
check("global_var", counter, 3)

def multi_return():
    return 1, 2, 3
a, b, c = multi_return()
check("multi_return", a + b + c, 6)

# ═══ LAMBDA ═══
double = lambda x: x * 2
check("lambda", double(5), 10)
check("lambda_inline", (lambda x, y: x + y)(3, 4), 7)
check("lambda_default", (lambda x=3: x ** 2)(), 9)
apply = lambda f, x: f(x)
check("lambda_higher", apply(lambda x: x * 3, 7), 21)

# ═══ LIST COMPREHENSION ═══
check("listcomp", [x ** 2 for x in range(6)], [0, 1, 4, 9, 16, 25])
check("listcomp_filter", [x for x in range(10) if x % 2 == 0], [0, 2, 4, 6, 8])
check("listcomp_method", [s.upper() for s in ["hello", "world"]], ["HELLO", "WORLD"])

# ═══ SLICING ═══
a = [10, 20, 30, 40, 50]
check("slice_basic", a[1:3], [20, 30])
check("slice_from", a[:3], [10, 20, 30])
check("slice_to", a[2:], [30, 40, 50])
check("slice_neg", a[-2:], [40, 50])
check("slice_step", a[::2], [10, 30, 50])
check("slice_rev", a[::-1], [50, 40, 30, 20, 10])
check("str_slice", "Hello"[1:4], "ell")
check("str_rev", "Hello"[::-1], "olleH")
check("tuple_slice", (1,2,3,4,5)[1:4], (2,3,4))

# ═══ TRY/EXCEPT ═══
x = 0
try:
    x = 1 / 0
except:
    x = -1
check("try_except", x, -1)

y = 0
try:
    y = 42
except:
    y = -1
else:
    y = y + 8
check("try_else", y, 50)

result = ""
try:
    try:
        1 / 0
    except:
        result = "inner "
        1 / 0
except:
    result = result + "outer"
check("nested_except", result, "inner outer")

def safe_div(a, b):
    try:
        return a / b
    except:
        return 0
check("func_except_ok", safe_div(10, 2), 5.0)
check("func_except_err", safe_div(10, 0), 0)

count = 0
for i in range(5):
    try:
        x = 10 / (i - 2)
    except:
        count = count + 1
check("loop_except", count, 1)

# ═══ CLASSES ═══
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def magnitude(self):
        return (self.x ** 2 + self.y ** 2) ** 0.5

p = Point(3, 4)
check("class_attr", p.x, 3)
check("class_method", p.magnitude(), 5.0)

p2 = Point(0, 0)
check("class_instance", p2.x, 0)
check("class_independent", p.x, 3)

class Counter:
    def __init__(self):
        self.count = 0
    def inc(self):
        self.count = self.count + 1
    def get(self):
        return self.count

c = Counter()
c.inc()
c.inc()
c.inc()
check("class_mutation", c.get(), 3)

# ═══ BUILTINS ═══
check("abs", abs(-5), 5)
check("min", min(3, 1, 4), 1)
check("max", max(3, 1, 4), 4)
check("sum", sum([1, 2, 3, 4]), 10)
check("round", round(3.7), 4)
check("pow", pow(2, 10), 1024)
check("len_str", len("hello"), 5)
check("len_list", len([1,2,3]), 3)
check("len_dict", len({"a": 1}), 1)
check("sorted", sorted([3,1,4,1,5]), [1,1,3,4,5])
check("reversed", list(reversed([1,2,3])), [3,2,1])
check("enumerate", list(enumerate(["a","b"])), [(0,"a"),(1,"b")])
check("zip", list(zip([1,2],[3,4])), [(1,3),(2,4)])
check("chr", chr(65), "A")
check("ord", ord("A"), 65)
check("hex", hex(255), "0xff")
check("bin", bin(10), "0b1010")
check("isinstance_int", isinstance(42, int), True)
check("isinstance_str", isinstance("hi", str), True)
check("callable_func", callable(abs), True)
check("callable_int", callable(42), False)
check("type_int", type(42), int)
check("type_str", type("hi"), str)
check("type_list", type([]), list)
x_obj = [1, 2, 3]
y_obj = [4, 5, 6]
check("id_diff", id(x_obj) != id(y_obj), True)
check("hash_int", hash(42), hash(42))

# ═══ IN / NOT IN / IS / IS NOT ═══
check("in_list", 3 in [1, 2, 3], True)
check("not_in_list", 4 not in [1, 2, 3], True)
check("in_str", "ell" in "hello", True)
check("in_dict", "a" in {"a": 1}, True)
a = [1, 2]
b = a
check("is_same", a is b, True)
check("is_not_diff", a is not [1, 2], True)
check("none_is", None is None, True)

print("========================================")
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("SOME TESTS FAILED!")
