"""Phase 50: Advanced Python features — descriptors, metaclasses, 
   __init_subclass__, class variables, multiple dispatch, slots simulation"""

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

# 1. Descriptor protocol
class Validator:
    def __init__(self, min_val, max_val):
        self.min_val = min_val
        self.max_val = max_val
        self.name = None
    
    def __set_name__(self, owner, name):
        self.name = name
    
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return obj.__dict__.get(f"_{self.name}", 0)
    
    def __set__(self, obj, value):
        if value < self.min_val or value > self.max_val:
            raise ValueError(f"{self.name} must be between {self.min_val} and {self.max_val}")
        obj.__dict__[f"_{self.name}"] = value

class Person:
    age = Validator(0, 150)
    
    def __init__(self, name, age):
        self.name = name
        self.age = age

p = Person("Alice", 30)
test("descriptor get", p.age == 30)
p.age = 40
test("descriptor set", p.age == 40)

try:
    p.age = 200
    test("descriptor validate", False)
except ValueError:
    test("descriptor validate", True)

# 2. __init_subclass__
class Plugin:
    registry = []
    
    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        Plugin.registry.append(cls.__name__)

class PluginA(Plugin):
    pass

class PluginB(Plugin):
    pass

test("init_subclass", "PluginA" in Plugin.registry and "PluginB" in Plugin.registry)

# 3. Class method and static method interaction
class MathHelper:
    precision = 2
    
    @staticmethod
    def add(a, b):
        return a + b
    
    @classmethod
    def set_precision(cls, p):
        cls.precision = p

test("static method", MathHelper.add(3, 4) == 7)
MathHelper.set_precision(4)
test("class method", MathHelper.precision == 4)

# 4. Property with delete
class Temperature:
    def __init__(self, celsius):
        self._celsius = celsius
    
    @property
    def celsius(self):
        return self._celsius
    
    @celsius.setter
    def celsius(self, value):
        if value < -273.15:
            raise ValueError("Below absolute zero!")
        self._celsius = value
    
    @property
    def fahrenheit(self):
        return self._celsius * 9/5 + 32

t = Temperature(100)
test("property get", t.celsius == 100)
test("property computed", abs(t.fahrenheit - 212.0) < 0.001)
t.celsius = 0
test("property set", t.celsius == 0)

# 5. Abstract-like base class (manual)
class Shape:
    def area(self):
        raise NotImplementedError("Subclasses must implement area()")
    
    def describe(self):
        return f"{type(self).__name__} with area {self.area()}"

class Rectangle(Shape):
    def __init__(self, w, h):
        self.w = w
        self.h = h
    def area(self):
        return self.w * self.h

class TriangleShape(Shape):
    def __init__(self, b, h):
        self.b = b
        self.h = h
    def area(self):
        return 0.5 * self.b * self.h

r = Rectangle(3, 4)
test("abstract impl rect", r.area() == 12)
tr = TriangleShape(6, 4)
test("abstract impl tri", tr.area() == 12.0)

# 6. Enum-like class
class Color:
    RED = 1
    GREEN = 2
    BLUE = 3
    
    @classmethod
    def from_name(cls, name):
        mapping = {"red": cls.RED, "green": cls.GREEN, "blue": cls.BLUE}
        return mapping.get(name.lower())

test("enum red", Color.RED == 1)
test("enum from name", Color.from_name("green") == 2)

# 7. Composition over inheritance
class Engine:
    def __init__(self, hp):
        self.hp = hp
    def start(self):
        return f"Engine {self.hp}hp started"

class Car:
    def __init__(self, make, engine):
        self.make = make
        self.engine = engine
    def start(self):
        return f"{self.make}: {self.engine.start()}"

car = Car("Tesla", Engine(300))
test("composition", car.start() == "Tesla: Engine 300hp started")

# 8. Iterator chaining
def take(n, iterable):
    count = 0
    for item in iterable:
        if count >= n:
            break
        yield item
        count += 1

def skip(n, iterable):
    count = 0
    for item in iterable:
        if count >= n:
            yield item
        count += 1

data = list(range(20))
test("take 5", list(take(5, data)) == [0, 1, 2, 3, 4])
test("skip 15", list(skip(15, data)) == [15, 16, 17, 18, 19])
test("skip take", list(take(3, skip(5, data))) == [5, 6, 7])

# 9. Recursive descent expression parser
class Parser:
    def __init__(self, tokens):
        self.tokens = tokens
        self.pos = 0
    
    def parse(self):
        return self.expr()
    
    def expr(self):
        result = self.term()
        while self.pos < len(self.tokens) and self.tokens[self.pos] in ('+', '-'):
            op = self.tokens[self.pos]
            self.pos += 1
            right = self.term()
            if op == '+':
                result += right
            else:
                result -= right
        return result
    
    def term(self):
        result = self.factor()
        while self.pos < len(self.tokens) and self.tokens[self.pos] in ('*', '/'):
            op = self.tokens[self.pos]
            self.pos += 1
            right = self.factor()
            if op == '*':
                result *= right
            else:
                result /= right
        return result
    
    def factor(self):
        if self.tokens[self.pos] == '(':
            self.pos += 1  # skip (
            result = self.expr()
            self.pos += 1  # skip )
            return result
        else:
            val = self.tokens[self.pos]
            self.pos += 1
            return val

test("parser simple", Parser([3, '+', 4]).parse() == 7)
test("parser complex", Parser([2, '+', 3, '*', 4]).parse() == 14)
test("parser parens", Parser(['(', 2, '+', 3, ')', '*', 4]).parse() == 20)

# 10. Coroutine simulation with generators
def coroutine_sim():
    results = []
    
    def producer(items):
        for item in items:
            yield item
    
    def consumer(source):
        for item in source:
            results.append(item * 2)
    
    consumer(producer([1, 2, 3, 4, 5]))
    return results

test("coroutine sim", coroutine_sim() == [2, 4, 6, 8, 10])

# 11. Functional programming patterns
from functools import reduce

test("reduce sum", reduce(lambda a, b: a + b, [1, 2, 3, 4, 5]) == 15)
test("reduce max", reduce(lambda a, b: a if a > b else b, [3, 1, 4, 1, 5, 9]) == 9)

# Compose functions
def compose(*fns):
    def composed(x):
        result = x
        for fn in reversed(fns):
            result = fn(result)
        return result
    return composed

double = lambda x: x * 2
inc = lambda x: x + 1
transform = compose(double, inc)
test("compose", transform(3) == 8)  # double(inc(3)) = double(4) = 8

# 12. Flatten nested structures
def flatten(lst):
    result = []
    for item in lst:
        if isinstance(item, list):
            result.extend(flatten(item))
        else:
            result.append(item)
    return result

test("flatten", flatten([1, [2, [3, 4]], [5, [6, [7]]]]) == [1, 2, 3, 4, 5, 6, 7])

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 50 TESTS PASSED")
