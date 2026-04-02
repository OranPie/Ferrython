"""Phase 44: Advanced Python patterns — complex inheritance, property,
   abstract-like patterns, class methods interplay, multiple return,
   tuple unpacking, *args/**kwargs forwarding, __repr__/__str__,
   isinstance chains, hasattr/getattr/setattr"""

passed = 0
failed = 0
total = 0
def test(name, cond):
    global passed, failed, total
    total += 1
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# 1. Complex class hierarchy with super
class Animal:
    def __init__(self, name):
        self.name = name
    def speak(self):
        return f"{self.name} makes a sound"
    def __repr__(self):
        return f"Animal({self.name})"

class Dog(Animal):
    def __init__(self, name, breed):
        super().__init__(name)
        self.breed = breed
    def speak(self):
        return f"{self.name} barks"
    def __repr__(self):
        return f"Dog({self.name}, {self.breed})"

class GuideDog(Dog):
    def __init__(self, name, breed, handler):
        super().__init__(name, breed)
        self.handler = handler
    def speak(self):
        return f"{self.name} guides {self.handler}"

d = Dog("Rex", "Labrador")
test("dog speak", d.speak() == "Rex barks")
test("dog repr", repr(d) == "Dog(Rex, Labrador)")
test("dog name", d.name == "Rex")

g = GuideDog("Buddy", "Golden", "Alice")
test("guide speak", g.speak() == "Buddy guides Alice")
test("guide breed", g.breed == "Golden")
test("isinstance chain", isinstance(g, Dog) and isinstance(g, Animal))

# 2. Property with getter/setter/deleter
class Temperature:
    def __init__(self, celsius=0):
        self._celsius = celsius
    
    @property
    def celsius(self):
        return self._celsius
    
    @celsius.setter
    def celsius(self, value):
        if value < -273.15:
            raise ValueError("Below absolute zero")
        self._celsius = value
    
    @property
    def fahrenheit(self):
        return self._celsius * 9/5 + 32

t = Temperature(100)
test("property get", t.celsius == 100)
test("property computed", t.fahrenheit == 212.0)
t.celsius = 0
test("property set", t.celsius == 0)
test("property recompute", t.fahrenheit == 32.0)

caught = False
try:
    t.celsius = -300
except ValueError:
    caught = True
test("property validation", caught)

# 3. hasattr/getattr/setattr
class Config:
    def __init__(self):
        self.debug = False
        self.verbose = True

c = Config()
test("hasattr true", hasattr(c, "debug"))
test("hasattr false", hasattr(c, "missing") == False)
test("getattr", getattr(c, "debug") == False)
test("getattr default", getattr(c, "missing", 42) == 42)
setattr(c, "level", 3)
test("setattr", c.level == 3)

# 4. *args/**kwargs forwarding
def log(level, *args, sep=" ", end="\n"):
    msg = sep.join(str(a) for a in args)
    return f"[{level}] {msg}"

test("args forward", log("INFO", "hello", "world") == "[INFO] hello world")
test("args sep", log("DEBUG", "a", "b", "c", sep=", ") == "[DEBUG] a, b, c")

def wrap(*args, **kwargs):
    return args, kwargs

result = wrap(1, 2, 3, x=4, y=5)
test("wrap args", result[0] == (1, 2, 3))
test("wrap kwargs", result[1] == {"x": 4, "y": 5})

# 5. Multiple assignment and tuple unpacking
a, b, c = 1, 2, 3
test("tuple unpack", a == 1 and b == 2 and c == 3)

first, *rest = [1, 2, 3, 4, 5]
test("star unpack first", first == 1)
test("star unpack rest", rest == [2, 3, 4, 5])

*init, last = [1, 2, 3, 4, 5]
test("star unpack init", init == [1, 2, 3, 4])
test("star unpack last", last == 5)

a, *mid, c = [1, 2, 3, 4, 5]
test("star unpack mid", mid == [2, 3, 4])

# 6. Chained comparisons
x = 5
test("chain cmp", 1 < x < 10)
test("chain cmp 2", 0 <= x <= 10)
test("chain cmp false", not (10 < x < 20))

# 7. Conditional expression (ternary)
a = "even" if 4 % 2 == 0 else "odd"
test("ternary", a == "even")
b = "even" if 3 % 2 == 0 else "odd"
test("ternary 2", b == "odd")

# 8. Complex dict operations
d = {"a": 1, "b": 2, "c": 3}
test("dict update", d.get("a") == 1)
d.update({"b": 20, "d": 4})
test("dict update merge", d["b"] == 20 and d["d"] == 4)

test("dict pop", d.pop("a") == 1)
test("dict after pop", "a" not in d)
test("dict pop default", d.pop("missing", 99) == 99)

test("dict setdefault", d.setdefault("e", 5) == 5)
test("dict setdefault exists", d.setdefault("b", 99) == 20)

# 9. Set operations
s1 = {1, 2, 3, 4}
s2 = {3, 4, 5, 6}
test("set union", s1 | s2 == {1, 2, 3, 4, 5, 6})
test("set intersect", s1 & s2 == {3, 4})
test("set diff", s1 - s2 == {1, 2})
test("set symmetric", s1 ^ s2 == {1, 2, 5, 6})
test("set subset", {1, 2} <= s1)
test("set superset", s1 >= {1, 2})

# 10. String methods
s = "Hello, World!"
test("str count", s.count("l") == 3)
test("str startswith", s.startswith("Hello"))
test("str endswith", s.endswith("!"))
test("str isdigit", "123".isdigit())
test("str isalpha", "abc".isalpha())
test("str isalnum", "abc123".isalnum())
test("str title", "hello world".title() == "Hello World")
test("str swapcase", "Hello".swapcase() == "hELLO")
test("str zfill", "42".zfill(5) == "00042")
test("str center", "hi".center(10) == "    hi    ")
test("str ljust", "hi".ljust(10) == "hi        ")
test("str rjust", "hi".rjust(10) == "        hi")

# 11. isinstance with multiple types
test("isinstance multi", isinstance(42, (int, float)))
test("isinstance multi 2", isinstance(3.14, (int, float)))
test("isinstance multi false", not isinstance("hi", (int, float)))

# 12. Class with __eq__ and __hash__
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    def __hash__(self):
        return hash((self.x, self.y))
    def __repr__(self):
        return f"Point({self.x}, {self.y})"
    def __add__(self, other):
        return Point(self.x + other.x, self.y + other.y)
    def __mul__(self, scalar):
        return Point(self.x * scalar, self.y * scalar)

p1 = Point(1, 2)
p2 = Point(3, 4)
p3 = p1 + p2
test("point add", p3.x == 4 and p3.y == 6)
p4 = p1 * 3
test("point mul", p4.x == 3 and p4.y == 6)

# Points as dict keys / set members
d = {p1: "origin", p2: "target"}
test("point dict key", d[Point(1, 2)] == "origin")

s = {Point(1, 2), Point(3, 4), Point(1, 2)}
test("point set dedup", len(s) == 2)

# 13. Generator pipeline
def gen_range(n):
    for i in range(n):
        yield i

def gen_filter(it, pred):
    for x in it:
        if pred(x):
            yield x

def gen_map(it, func):
    for x in it:
        yield func(x)

pipeline = list(gen_map(gen_filter(gen_range(10), lambda x: x % 2 == 0), lambda x: x * x))
test("gen pipeline", pipeline == [0, 4, 16, 36, 64])

# 14. Context manager
class Timer:
    def __init__(self):
        self.entered = False
        self.exited = False
    def __enter__(self):
        self.entered = True
        return self
    def __exit__(self, *args):
        self.exited = True
        return False

timer = Timer()
with timer as t:
    test("ctx entered", t.entered)
test("ctx exited", timer.exited)

# 15. Multiple exception handling
def convert(val):
    try:
        return int(val)
    except (ValueError, TypeError):
        return None

test("convert int", convert("42") == 42)
test("convert fail", convert("abc") is None)

# 16. Dict/list in complex patterns
users = [
    {"name": "Alice", "age": 30, "hobbies": ["reading", "coding"]},
    {"name": "Bob", "age": 25, "hobbies": ["gaming", "cooking"]},
    {"name": "Charlie", "age": 35, "hobbies": ["hiking", "coding"]},
]

coders = [u["name"] for u in users if "coding" in u["hobbies"]]
test("nested filter", coders == ["Alice", "Charlie"])

ages = {u["name"]: u["age"] for u in users}
test("dict from list", ages == {"Alice": 30, "Bob": 25, "Charlie": 35})

oldest = max(users, key=lambda u: u["age"])
test("max with key", oldest["name"] == "Charlie")

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 44 TESTS PASSED")
