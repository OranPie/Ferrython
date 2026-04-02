passed = 0
failed = 0
def test(name, cond):
    global passed, failed
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# === f-string !r and !s conversions ===
class Obj:
    def __repr__(self): return "Obj()"
    def __str__(self): return "an obj"

o = Obj()
test("fstr !r", f"{o!r}" == "Obj()")
test("fstr !s", f"{o!s}" == "an obj")
test("fstr default", f"{o}" == "an obj")

# === issubclass with object ===
test("issubclass int obj", issubclass(int, object))
test("issubclass str obj", issubclass(str, object))
test("issubclass bool int", issubclass(bool, int))
test("issubclass bool obj", issubclass(bool, object))

# === KeyError str representation ===
try:
    raise KeyError("test")
except KeyError as e:
    test("keyerror str", str(e) == "'test'")

# === setattr with descriptor ===
class Cached:
    def __init__(self):
        self._val = None
    @property
    def val(self):
        return self._val
    @val.setter
    def val(self, v):
        self._val = v * 2

c = Cached()
setattr(c, 'val', 5)
test("setattr prop", c.val == 10)

# === dict.update kwargs ===
d = {}
d.update(x=1, y=2, z=3)
test("dict update kw", d == {"x": 1, "y": 2, "z": 3})

# === str.format_map ===
template = "{greeting}, {name}!"
test("format_map", template.format_map({"greeting": "Hello", "name": "World"}) == "Hello, World!")

# === min/max with key ===
test("min key", min(["cc", "a", "bbb"], key=len) == "a")
test("max key", max(["cc", "a", "bbb"], key=len) == "bbb")

# === sorted with key ===
test("sorted key", sorted(["bb", "a", "ccc"], key=len) == ["a", "bb", "ccc"])

# === radd / rmul ===
class Vec2:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __radd__(self, other):
        if other == 0:
            return self
        return NotImplemented
    def __rmul__(self, other):
        if isinstance(other, int):
            return Vec2(self.x * other, self.y * other)
        return NotImplemented

v = Vec2(1, 2)
test("radd", (0 + v).x == 1)
test("rmul", (3 * v).x == 3)

# === Closure in nested class body ===
def make_factory(base_val):
    class Product:
        base = base_val
        def total(self, qty):
            return self.base * qty
    return Product

P = make_factory(10)
test("cls closure factory", P().total(5) == 50)

# === Complex decorator ===
def validate_args(*types):
    def decorator(func):
        def wrapper(*args):
            for arg, expected_type in zip(args, types):
                if not isinstance(arg, expected_type):
                    raise TypeError(f"Expected {expected_type.__name__}, got {type(arg).__name__}")
            return func(*args)
        return wrapper
    return decorator

@validate_args(int, int)
def add(a, b):
    return a + b

test("decorator validate", add(1, 2) == 3)
try:
    add("1", 2)
    test("decorator reject", False)
except TypeError:
    test("decorator reject", True)

# === Generator with exception ===
def gen_exc():
    try:
        yield 1
        yield 2
    except GeneratorExit:
        pass

g = gen_exc()
test("gen 1", next(g) == 1)

# === Property without setter raises ===
class ReadOnly:
    @property
    def val(self):
        return 42

ro = ReadOnly()
test("readonly prop", ro.val == 42)
try:
    ro.val = 99
    test("readonly raises", False)
except AttributeError:
    test("readonly raises", True)

# === Dict comprehension with conditional ===
d = {k: v for k, v in [("a", 1), ("b", 2), ("c", 3)] if v > 1}
test("dict comp cond", d == {"b": 2, "c": 3})

# === Nested dict update ===
config = {"db": {"host": "localhost"}}
config["db"]["port"] = 5432
test("nested dict set", config["db"]["port"] == 5432)

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 56 TESTS PASSED")
