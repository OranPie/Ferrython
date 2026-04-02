"""Test suite 28: Feature expansion - descriptors, exceptions, type methods"""
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── getattr / setattr / delattr builtins ──
class Obj:
    def __init__(self):
        self.x = 10

o = Obj()
test("getattr", getattr(o, "x") == 10)
test("getattr default", getattr(o, "y", 42) == 42)
setattr(o, "z", 99)
test("setattr", o.z == 99)
test("hasattr true", hasattr(o, "x"))
test("hasattr false", not hasattr(o, "missing"))

# ── dir() basics ──
class DirTest:
    class_var = 1
    def __init__(self):
        self.inst_var = 2
    def method(self):
        pass

dt = DirTest()
d = dir(dt)
test("dir has class_var", "class_var" in d)
test("dir has inst_var", "inst_var" in d)
test("dir has method", "method" in d)

# ── vars() ──
class VarsTest:
    def __init__(self, a, b):
        self.a = a
        self.b = b

vt = VarsTest(1, 2)
v = vars(vt)
test("vars keys", "a" in v and "b" in v)
test("vars values", v["a"] == 1)

# ── Exception hierarchy ──
test("exc is base", issubclass(ValueError, Exception))
test("exc is base2", issubclass(TypeError, Exception))
test("exc is base3", issubclass(KeyError, LookupError))
test("exc is base4", issubclass(IndexError, LookupError))
test("exc is base5", issubclass(RuntimeError, Exception))

# ── Exception attributes ──
try:
    raise ValueError("test message")
except ValueError as e:
    test("exc str", str(e) == "test message")
    test("exc args", e.args == ("test message",))

# ── Custom exception ──
class AppError(Exception):
    def __init__(self, code, message):
        super().__init__(message)
        self.code = code

try:
    raise AppError(404, "Not Found")
except AppError as e:
    test("custom exc code", e.code == 404)
    test("custom exc msg", str(e) == "Not Found")

# ── Exception as context manager ──
class Resource:
    opened = False
    def __enter__(self):
        Resource.opened = True
        return self
    def __exit__(self, *args):
        Resource.opened = False
        return False

with Resource() as r:
    test("ctx enter", Resource.opened)
test("ctx exit", not Resource.opened)

# ── Multiple context managers ──
log = []
class Logger:
    def __init__(self, name):
        self.name = name
    def __enter__(self):
        log.append(f"enter {self.name}")
        return self
    def __exit__(self, *args):
        log.append(f"exit {self.name}")
        return False

with Logger("a") as a:
    with Logger("b") as b:
        log.append("body")

test("nested ctx", log == ["enter a", "enter b", "body", "exit b", "exit a"])

# ── Generator close ──
def gen_close_test():
    try:
        yield 1
        yield 2
        yield 3
    finally:
        pass

g = gen_close_test()
test("gen next", next(g) == 1)

# ── String methods batch ──
test("str title", "hello world".title() == "Hello World")
test("str capitalize", "hello WORLD".capitalize() == "Hello world")
test("str swapcase", "Hello".swapcase() == "hELLO")
test("str isdigit", "123".isdigit())
test("str isalpha", "abc".isalpha())
test("str isalnum", "abc123".isalnum())
test("str isupper", "ABC".isupper())
test("str islower", "abc".islower())
test("str isspace", "  \t\n".isspace())
test("str startswith", "hello".startswith("hel"))
test("str endswith", "hello".endswith("llo"))
test("str partition", "hello-world".partition("-") == ("hello", "-", "world"))
test("str rpartition", "a-b-c".rpartition("-") == ("a-b", "-", "c"))

# ── List comprehension edge cases ──
test("nested list comp", [[j for j in range(i)] for i in range(4)] == [[], [0], [0, 1], [0, 1, 2]])
test("conditional comp", [x if x > 0 else -x for x in [-2, -1, 0, 1, 2]] == [2, 1, 0, 1, 2])

# ── Dict operations ──
d = {"a": 1, "b": 2, "c": 3}
test("dict keys sorted", sorted(d.keys()) == ["a", "b", "c"])
test("dict values sorted", sorted(d.values()) == [1, 2, 3])
test("dict items", sorted(d.items()) == [("a", 1), ("b", 2), ("c", 3)])
d2 = dict(d)  # copy
test("dict copy constructor", d2 == d and d2 is not d)

# ── Set operations ──
s = {1, 2, 3}
s.add(4)
test("set add", s == {1, 2, 3, 4})
s.discard(2)
test("set discard", s == {1, 3, 4})
s.discard(99)  # no error
test("set discard missing", s == {1, 3, 4})

# ── Frozen set ──
fs = frozenset([1, 2, 3])
test("frozenset in", 2 in fs)
test("frozenset union", fs | frozenset([3, 4]) == frozenset([1, 2, 3, 4]))
test("frozenset len", len(fs) == 3)

# ── Bytes operations ──
b = b"hello"
test("bytes len", len(b) == 5)
test("bytes index", b[0] == 104)
test("bytes slice", b[1:3] == b"el")
test("bytes upper", b.upper() == b"HELLO")
test("bytes lower", b"HELLO".lower() == b"hello")

# ── Complex type creation patterns ──
class Registry:
    _items = {}
    
    @classmethod
    def register(cls, name):
        def decorator(klass):
            cls._items[name] = klass
            return klass
        return decorator
    
    @classmethod
    def get(cls, name):
        return cls._items.get(name)

@Registry.register("user")
class User:
    def __init__(self, name):
        self.name = name

@Registry.register("admin")
class Admin:
    def __init__(self, name):
        self.name = name

test("registry", Registry.get("user") is User)
test("registry admin", Registry.get("admin") is Admin)

# ── Decorator with arguments ──
def repeat(n):
    def decorator(func):
        def wrapper(*args, **kwargs):
            result = []
            for _ in range(n):
                result.append(func(*args, **kwargs))
            return result
        return wrapper
    return decorator

@repeat(3)
def greet(name):
    return f"hi {name}"

test("decorator args", greet("world") == ["hi world", "hi world", "hi world"])

# ── Callable protocol ──
class Multiplier:
    def __init__(self, factor):
        self.factor = factor
    def __call__(self, x):
        return x * self.factor

double = Multiplier(2)
triple = Multiplier(3)
test("callable obj", double(5) == 10)
test("callable obj2", triple(5) == 15)
test("map callable", list(map(double, [1, 2, 3])) == [2, 4, 6])

# ── Recursive class ──
class TreeNode:
    def __init__(self, value, children=None):
        self.value = value
        self.children = children or []
    
    def count(self):
        total = 1
        for child in self.children:
            total += child.count()
        return total
    
    def depth(self):
        if not self.children:
            return 1
        return 1 + max(c.depth() for c in self.children)

tree = TreeNode(1, [
    TreeNode(2, [TreeNode(4), TreeNode(5)]),
    TreeNode(3, [TreeNode(6)])
])
test("tree count", tree.count() == 6)
test("tree depth", tree.depth() == 3)

# ── Generator pipeline ──
def gen_range(n):
    for i in range(n):
        yield i

def gen_filter(gen, pred):
    for item in gen:
        if pred(item):
            yield item

def gen_map(gen, fn):
    for item in gen:
        yield fn(item)

pipeline = list(gen_map(gen_filter(gen_range(10), lambda x: x % 2 == 0), lambda x: x * x))
test("gen pipeline", pipeline == [0, 4, 16, 36, 64])

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
