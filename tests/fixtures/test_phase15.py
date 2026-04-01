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

# ── type() with 3 args ──
MyClass = type("MyClass", (), {"x": 10, "y": 20})
obj = MyClass()
test("type3_attr", obj.x, 10)
test("type3_name", type(obj).__name__, "MyClass")

# type() with inheritance
Base = type("Base", (), {"greet": lambda self: "hello"})
Child = type("Child", (Base,), {"name": "child"})
c = Child()
test("type3_inherit", c.greet(), "hello")
test("type3_childattr", c.name, "child")

# ── __class__ ──
class Animal:
    pass

class Dog(Animal):
    pass

d = Dog()
test("class_attr", d.__class__.__name__, "Dog")
test("class_attr2", type(d).__name__, "Dog")

# ── __dict__ on instance ──
class Bag:
    def __init__(self):
        self.x = 1
        self.y = 2

b = Bag()
bd = b.__dict__
test("inst_dict_x", bd["x"], 1)
test("inst_dict_y", bd["y"], 2)

# ── __dict__ on class ──
class Config:
    debug = True
    verbose = False

cd = Config.__dict__
test("class_dict", cd["debug"], True)

# ── Walrus operator ──
data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
filtered = [y for x in data if (y := x * 2) > 10]
test("walrus_comp", filtered, [12, 14, 16, 18, 20])

# Simple walrus
if (n := 10) > 5:
    test("walrus_if", n, 10)

# ── for/else ──
def find_item(lst, target):
    for item in lst:
        if item == target:
            return "found"
    else:
        return "not found"

test("for_else_found", find_item([1, 2, 3], 2), "found")
test("for_else_notfound", find_item([1, 2, 3], 5), "not found")

# ── while/else ──
def while_else_test():
    i = 0
    while i < 5:
        i += 1
    else:
        return "done: " + str(i)

test("while_else", while_else_test(), "done: 5")

# ── Chained comparisons ──
test("chain3", 1 < 2 < 3 < 4, True)
test("chain3f", 1 < 2 < 3 < 2, False)
test("chain_mixed", 0 <= 5 < 10, True)

# ── Multiple assignment ──
a = b = c = []
a.append(1)
test("multi_assign_ref", b, [1])  # Same object

# ── Complex string formatting ──
test("format_percent", "%s has %d items" % ("list", 5), "list has 5 items")
test("format_perc2", "%.2f%%" % 99.5, "99.50%")

# ── Nested function ──
def outer():
    data = []
    def add(x):
        data.append(x)
    def get():
        return data
    return add, get

add, get = outer()
add(1)
add(2)
add(3)
test("nested_fn", get(), [1, 2, 3])

# ── Generator with send ──
def accumulator():
    total = 0
    while True:
        value = yield total
        if value is None:
            break
        total += value

gen = accumulator()
next(gen)  # Prime
gen.send(10)
gen.send(20)
result = gen.send(30)
test("gen_send", result, 60)

# ── Exception handling patterns ──
def divide_safe(a, b):
    try:
        return a / b
    except ZeroDivisionError:
        return float('inf')

test("safe_div", divide_safe(10, 2), 5.0)
test("safe_div_zero", divide_safe(10, 0), float('inf'))

# ── Nested try/except ──
def nested_try():
    try:
        try:
            raise ValueError("inner")
        except ValueError:
            raise TypeError("outer")
    except TypeError as e:
        return str(e)

test("nested_try", nested_try(), "outer")

# ── Default args with mutable ──
def make_list(item, lst=None):
    if lst is None:
        lst = []
    lst.append(item)
    return lst

test("default_mut1", make_list(1), [1])
test("default_mut2", make_list(2), [2])

# ── Unpacking in comprehensions ──
pairs = [(1, "a"), (2, "b"), (3, "c")]
test("unpack_comp", {k: v for k, v in pairs}, {1: "a", 2: "b", 3: "c"})

# ── Multiple return ──
def swap(a, b):
    return b, a

test("swap", swap(1, 2), (2, 1))

# ── Class with class methods ──
class Counter:
    count = 0
    
    def __init__(self):
        Counter.count += 1
    
    @classmethod
    def get_count(cls):
        return cls.count

c1 = Counter()
c2 = Counter()
c3 = Counter()
test("classmethod_count", Counter.get_count(), 3)

# ── Static method ──
class MathUtils:
    @staticmethod
    def add(a, b):
        return a + b

test("staticmethod", MathUtils.add(3, 4), 7)

# ── Large list operations ──
big = list(range(100))
test("big_list_sum", sum(big), 4950)
test("big_list_len", len(big), 100)
test("big_list_slice", big[50:55], [50, 51, 52, 53, 54])

# ── Dict merge ──
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
merged = {}
merged.update(d1)
merged.update(d2)
test("dict_merge", merged, {"a": 1, "b": 3, "c": 4})

# ── String methods chain ──
test("str_chain", " hello world ".strip().title(), "Hello World")

# ── Truthiness of various types ──
test("truthy_list", bool([1]), True)
test("truthy_empty", bool([]), False)
test("truthy_str", bool("x"), True)
test("truthy_empty_str", bool(""), False)
test("truthy_zero", bool(0), False)
test("truthy_nonzero", bool(42), True)
test("truthy_dict", bool({}), False)
test("truthy_dict2", bool({"a": 1}), True)

# ── isinstance with base classes ──
class Shape:
    pass

class Circle(Shape):
    pass

c = Circle()
test("isinstance_base", isinstance(c, Shape), True)
test("isinstance_exact", isinstance(c, Circle), True)

# ── Multiple bases isinstance ──
class Printable:
    pass

class Saveable:
    pass

class Document(Printable, Saveable):
    pass

doc = Document()
test("isinstance_multi", isinstance(doc, Printable), True)
test("isinstance_multi2", isinstance(doc, Saveable), True)

# ── Recursive data structures ──
def tree_sum(node):
    if isinstance(node, int):
        return node
    return sum(tree_sum(child) for child in node)

test("tree_sum", tree_sum([1, [2, 3], [4, [5, 6]]]), 21)

# ── Generator pipeline ──
def evens(n):
    for i in range(n):
        if i % 2 == 0:
            yield i

def squared(gen):
    for x in gen:
        yield x * x

test("gen_pipeline", list(squared(evens(10))), [0, 4, 16, 36, 64])

# ── Complex dict comprehension ──
words = ["hello", "world", "python", "rust"]
test("dictcomp_complex", {w: len(w) for w in words if len(w) > 4}, {"hello": 5, "world": 5, "python": 6})

# ── Mixed types in list ──
mixed = [1, "two", 3.0, True, None, [4, 5]]
test("mixed_types", len(mixed), 6)
test("mixed_access", mixed[5], [4, 5])

# ── Error messages ──
try:
    x = {}["missing"]
except KeyError as e:
    test("keyerror_msg", "missing" in str(e), True)

try:
    x = [][0]
except IndexError as e:
    test("indexerror_msg", "index" in str(e).lower(), True)

# ── Numeric edge cases ──
test("int_div", 7 // 2, 3)
test("neg_div", -7 // 2, -4)
test("mod_neg", -7 % 3, 2)
test("pow_neg", (-2) ** 3, -8)

# ── String escapes ──
test("str_newline", "a\nb", "a\nb")
test("str_tab", "a\tb", "a\tb")
test("str_len_escape", len("a\nb"), 3)

print("========================================")
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("Failed tests:", ", ".join(errors))
print("========================================")
