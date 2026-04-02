_pass = 0
_fail = 0
def test(name, cond):
    global _pass, _fail
    if cond:
        _pass += 1
    else:
        _fail += 1
        print(f"  FAIL: {name}")

# ── Walrus operator (not inside exec) ──
data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
filtered = [y for x in data if (y := x * 2) > 10]
test("walrus listcomp", filtered == [12, 14, 16, 18, 20])

# ── Dict merge via update ──
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
d3 = {**d1, **d2}
test("dict merge", d3 == {"a": 1, "b": 3, "c": 4})

# ── Nested with statement ──
class Ctx:
    def __init__(self, name):
        self.name = name
        self.log = []
    def __enter__(self):
        self.log.append(f"enter-{self.name}")
        return self
    def __exit__(self, *args):
        self.log.append(f"exit-{self.name}")
        return False

log = []
c1 = Ctx("a")
c2 = Ctx("b")
with c1 as a:
    with c2 as b:
        log = c1.log + c2.log
test("nested with", log == ["enter-a", "enter-b"])
test("nested with exit", c1.log == ["enter-a", "exit-a"] and c2.log == ["enter-b", "exit-b"])

# ── Exception hierarchy ──
test("exc hierarchy", issubclass(ValueError, Exception))
test("exc hierarchy2", issubclass(TypeError, Exception))
test("exc hierarchy3", issubclass(KeyError, LookupError))
test("exc hierarchy4", issubclass(IndexError, LookupError))
test("exc hierarchy5", issubclass(Exception, BaseException))

# ── Generator protocol advanced ──
def gen_protocol():
    val = yield 1
    yield val
    yield 3

g = gen_protocol()
test("gen next", next(g) == 1)
test("gen send", g.send(42) == 42)
test("gen next2", next(g) == 3)

# ── Generator as context ── (manual)
class GenCtx:
    def __init__(self, gen):
        self.gen = gen
    def __enter__(self):
        return next(self.gen)
    def __exit__(self, *args):
        try:
            next(self.gen)
        except StopIteration:
            pass
        return False

def managed():
    yield "resource"

with GenCtx(managed()) as r:
    test("gen context", r == "resource")

# ── Recursive data structures ──
class Node:
    def __init__(self, val, children=None):
        self.val = val
        self.children = children or []
    def sum_tree(self):
        total = self.val
        for c in self.children:
            total += c.sum_tree()
        return total

tree = Node(1, [Node(2, [Node(4), Node(5)]), Node(3)])
test("tree recursion", tree.sum_tree() == 15)

# ── Class with __new__ ──
class Singleton:
    _instance = None
    def __new__(cls):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance

s1 = Singleton()
s2 = Singleton()
test("singleton", s1 is s2)

# ── Chained assignment ──
a = b = c = []
a.append(1)
test("chained assign ref", b == [1] and c == [1])

# ── Unpacking in return ──
def swap(a, b):
    return b, a
x, y = swap(1, 2)
test("swap unpack", x == 2 and y == 1)

# ── Default mutable arg (known Python gotcha) ──
def append_to(elem, target=[]):
    target.append(elem)
    return target

r1 = append_to(1)
r2 = append_to(2)
test("default mutable", r2 == [1, 2])

# ── Nested dict access ──
data = {"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}
test("nested dict", data["users"][0]["name"] == "Alice")
test("nested dict2", data["users"][1]["age"] == 25)

# ── Complex number ──
c1 = 3 + 4j
test("complex real", c1.real == 3.0)
test("complex imag", c1.imag == 4.0)
test("complex add", (1+2j) + (3+4j) == (4+6j))
test("complex mul", (1+2j) * (3+4j) == (-5+10j))

# ── Bitwise operations ──
test("bitand", 0xFF & 0x0F == 0x0F)
test("bitor", 0xF0 | 0x0F == 0xFF)
test("bitxor", 0xFF ^ 0x0F == 0xF0)
test("bitnot", ~0 == -1)
test("lshift", 1 << 10 == 1024)
test("rshift", 1024 >> 10 == 1)

# ── Large integer arithmetic ──
test("bigint pow", 2 ** 100 == 1267650600228229401496703205376)
test("bigint add", 10**50 + 10**50 == 2 * 10**50)

# ── String raw and escape ──
test("raw string", r"\n" == "\\n")
test("escape tab", "\t" == "\t")
test("multiline str", """hello
world""" == "hello\nworld")

# ── Boolean arithmetic ──
test("bool add", True + True == 2)
test("bool mul", True * 5 == 5)
test("bool int", int(True) == 1 and int(False) == 0)

# ── Chained comparison ──
x = 5
test("chained cmp", 1 < x < 10)
test("chained cmp2", 1 < x <= 5)
test("chained cmp3", 10 > x > 1)

# ── Empty containers ──
test("empty list bool", not [])
test("empty dict bool", not {})
test("empty str bool", not "")
test("empty tuple bool", not ())
test("empty set bool", not set())

# ── Slice assignment ──
lst = [1, 2, 3, 4, 5]
lst[1:3] = [20, 30]
test("slice assign", lst == [1, 20, 30, 4, 5])

# ── Delete from dict ──
d = {"a": 1, "b": 2, "c": 3}
del d["b"]
test("dict del", d == {"a": 1, "c": 3})

# ── String formatting ──
test("format int", "{:05d}".format(42) == "00042")
test("format float", "{:.2f}".format(3.14159) == "3.14")
test("format str", "{:>10}".format("hi") == "        hi")
test("format str left", "{:<10}".format("hi") == "hi        ")
test("format str center", "{:^10}".format("hi") == "    hi    ")

# ── Range features ──
r = range(0, 10, 2)
test("range list", list(r) == [0, 2, 4, 6, 8])
test("range len", len(r) == 5)
test("range in", 4 in r)
test("range not in", 3 not in r)

# ── Multiple decorators ──
def d1(f):
    def w(*a):
        return f(*a) + 1
    return w
def d2(f):
    def w(*a):
        return f(*a) * 2
    return w

@d1
@d2
def compute(x):
    return x

test("multi decorator", compute(5) == 11)  # d2(5) = 10, d1(10) = 11

# ── Class inheritance with super ──
class Base:
    def __init__(self):
        self.base_init = True
    def greet(self):
        return "Base"

class Child(Base):
    def __init__(self):
        super().__init__()
        self.child_init = True
    def greet(self):
        return "Child+" + super().greet()

c = Child()
test("super init", c.base_init and c.child_init)
test("super method", c.greet() == "Child+Base")

# ── divmod / pow with mod ──
test("divmod", divmod(17, 5) == (3, 2))
test("pow mod", pow(2, 10, 1000) == 24)

# ── chr / ord ──
test("chr", chr(65) == "A")
test("ord", ord("A") == 65)

# ── hex / oct / bin ──
test("hex", hex(255) == "0xff")
test("oct", oct(8) == "0o10")
test("bin", bin(10) == "0b1010")

print(f"\nTests: {_pass + _fail} | Passed: {_pass} | Failed: {_fail}")
