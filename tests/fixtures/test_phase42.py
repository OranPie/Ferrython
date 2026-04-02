"""Phase 42: Advanced class features — __add__/__mul__/__lt__ overloading,
   __iter__/__next__ custom iterators, __getitem__/__setitem__/__delitem__,
   __bool__/__len__ truthiness, abstract patterns, mixin classes,
   class variables vs instance variables, __class_getitem__"""

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

# 1. Operator overloading: __add__, __mul__, __sub__
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
        return f"Vector({self.x}, {self.y})"
    def __abs__(self):
        return (self.x ** 2 + self.y ** 2) ** 0.5
    def __neg__(self):
        return Vector(-self.x, -self.y)
    def __bool__(self):
        return self.x != 0 or self.y != 0

v1 = Vector(1, 2)
v2 = Vector(3, 4)
test("vec add", v1 + v2 == Vector(4, 6))
test("vec sub", v2 - v1 == Vector(2, 2))
test("vec mul", v1 * 3 == Vector(3, 6))
test("vec abs", abs(v2) == 5.0)
test("vec neg", -v1 == Vector(-1, -2))
test("vec bool true", bool(v1) == True)
test("vec bool false", bool(Vector(0, 0)) == False)

# 2. Comparison overloading
class Temperature:
    def __init__(self, value):
        self.value = value
    def __lt__(self, other):
        return self.value < other.value
    def __le__(self, other):
        return self.value <= other.value
    def __gt__(self, other):
        return self.value > other.value
    def __ge__(self, other):
        return self.value >= other.value
    def __eq__(self, other):
        return self.value == other.value

t1 = Temperature(20)
t2 = Temperature(30)
test("temp lt", t1 < t2)
test("temp gt", t2 > t1)
test("temp le", t1 <= t2)
test("temp ge", t2 >= t1)
test("temp eq", t1 == Temperature(20))

# Sorting custom objects
temps = [Temperature(30), Temperature(10), Temperature(20)]
sorted_temps = sorted(temps)
test("sort custom", [t.value for t in sorted_temps] == [10, 20, 30])

# 3. Custom iterator with __iter__ and __next__
class Countdown:
    def __init__(self, start):
        self.start = start
    def __iter__(self):
        self.current = self.start
        return self
    def __next__(self):
        if self.current <= 0:
            raise StopIteration
        val = self.current
        self.current -= 1
        return val

test("custom iter", list(Countdown(5)) == [5, 4, 3, 2, 1])
test("custom iter empty", list(Countdown(0)) == [])

# 4. __getitem__ and __setitem__
class Grid:
    def __init__(self, size):
        self.size = size
        self.data = {}
    def __getitem__(self, key):
        return self.data.get(key, 0)
    def __setitem__(self, key, value):
        self.data[key] = value
    def __contains__(self, key):
        return key in self.data

g = Grid(5)
g[(0, 0)] = 1
g[(1, 2)] = 42
test("grid getitem", g[(0, 0)] == 1)
test("grid getitem default", g[(3, 3)] == 0)
test("grid setitem", g[(1, 2)] == 42)
test("grid contains", (0, 0) in g)
test("grid not contains", (5, 5) not in g)

# 5. Mixin classes
class JsonMixin:
    def to_json(self):
        import json
        return json.dumps(self.__dict__())

class Serializable:
    def __dict__(self):
        return {"type": self.__class__.__name__ if hasattr(self, '__class__') else "unknown"}

class ReprMixin:
    def __repr__(self):
        return f"{type(self).__name__}(...)"

class MyModel(ReprMixin):
    def __init__(self, name):
        self.name = name

m = MyModel("test")
test("repr mixin", repr(m) == "MyModel(...)")

# 6. Class variables vs instance variables
class Config:
    default_timeout = 30
    
    def __init__(self, timeout=None):
        if timeout is not None:
            self.timeout = timeout
        else:
            self.timeout = Config.default_timeout

c1 = Config()
c2 = Config(60)
test("class var default", c1.timeout == 30)
test("instance var override", c2.timeout == 60)
test("class var unchanged", Config.default_timeout == 30)

# 7. Inheritance and super() with multiple args
class Animal:
    def __init__(self, name, sound):
        self.name = name
        self.sound = sound
    def speak(self):
        return f"{self.name} says {self.sound}"

class Dog(Animal):
    def __init__(self, name):
        super().__init__(name, "Woof")
    def fetch(self):
        return f"{self.name} fetches!"

d = Dog("Rex")
test("dog speak", d.speak() == "Rex says Woof")
test("dog fetch", d.fetch() == "Rex fetches!")

# 8. Property with validation
class Person:
    def __init__(self, name, age):
        self._name = name
        self.age = age
    
    @property
    def age(self):
        return self._age
    
    @age.setter
    def age(self, value):
        if value < 0:
            raise ValueError("Age cannot be negative")
        self._age = value

p = Person("Alice", 30)
test("person age", p.age == 30)
p.age = 25
test("person age set", p.age == 25)

try:
    p.age = -1
    test("person age validate", False)
except ValueError:
    test("person age validate", True)

# 9. __str__ vs __repr__
class Color:
    def __init__(self, r, g, b):
        self.r = r
        self.g = g
        self.b = b
    def __str__(self):
        return f"rgb({self.r}, {self.g}, {self.b})"
    def __repr__(self):
        return f"Color({self.r}, {self.g}, {self.b})"

c = Color(255, 0, 128)
test("color str", str(c) == "rgb(255, 0, 128)")
test("color repr", repr(c) == "Color(255, 0, 128)")
test("f-string uses str", f"{c}" == "rgb(255, 0, 128)")

# 10. Class with __call__
class Adder:
    def __init__(self, n):
        self.n = n
    def __call__(self, x):
        return self.n + x

add5 = Adder(5)
test("callable instance", add5(10) == 15)
test("callable instance 2", add5(0) == 5)

# Test callable in map
result = list(map(Adder(10), [1, 2, 3]))
test("map with callable", result == [11, 12, 13])

# 11. Exception hierarchy
class AppError(Exception):
    pass

class ValidationError(AppError):
    pass

class DatabaseError(AppError):
    pass

try:
    raise ValidationError("bad input")
except AppError as e:
    test("exception hierarchy", True)
except:
    test("exception hierarchy", False)

try:
    raise DatabaseError("connection failed")
except ValidationError:
    test("exception specificity", False)
except AppError:
    test("exception specificity", True)

# 12. Multiple return paths in try/except/else/finally
def complex_try(x):
    result = []
    try:
        if x == 0:
            raise ValueError("zero")
        result.append("try")
    except ValueError:
        result.append("except")
    else:
        result.append("else")
    finally:
        result.append("finally")
    return result

test("try success", complex_try(1) == ["try", "else", "finally"])
test("try failure", complex_try(0) == ["except", "finally"])

# 13. Generator with complex state
def running_average():
    total = 0
    count = 0
    average = None
    while True:
        value = yield average
        if value is None:
            break
        total += value
        count += 1
        average = total / count

gen = running_average()
next(gen)
test("gen avg 1", gen.send(10) == 10.0)
test("gen avg 2", gen.send(20) == 15.0)
test("gen avg 3", gen.send(30) == 20.0)

# 14. Nested comprehension with multiple variables
pairs = [(x, y) for x in range(3) for y in range(3) if x != y]
test("nested comp pairs", len(pairs) == 6)
test("nested comp content", (0, 1) in pairs and (1, 0) in pairs)

# 15. Dict methods
d = {"a": 1, "b": 2, "c": 3}
test("dict keys", sorted(d.keys()) == ["a", "b", "c"])
test("dict values", sorted(d.values()) == [1, 2, 3])
test("dict items", sorted(d.items()) == [("a", 1), ("b", 2), ("c", 3)])
d.update({"d": 4, "a": 10})
test("dict update", d["a"] == 10 and d["d"] == 4)
test("dict pop", d.pop("d") == 4 and "d" not in d)
test("dict setdefault", d.setdefault("e", 5) == 5 and d["e"] == 5)
test("dict setdefault existing", d.setdefault("a", 99) == 10)

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 42 TESTS PASSED")
