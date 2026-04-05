import sys
_pass = 0
_fail = 0
def test(name, cond):
    global _pass, _fail
    if cond:
        _pass += 1
    else:
        _fail += 1
        print(f"  FAIL: {name}")

# ═══ ABC enforcement ═══

from abc import ABC, abstractmethod

# ── Single abstract method ──

class Animal(ABC):
    @abstractmethod
    def speak(self):
        pass

# Direct abstract class cannot be instantiated
try:
    Animal()
    test("abc_direct_instantiate", False)
except TypeError as e:
    msg = str(e)
    test("abc_direct_instantiate", "Can't instantiate abstract class Animal" in msg)
    test("abc_error_mentions_method", "speak" in msg)

# Concrete subclass CAN be instantiated
class Dog(Animal):
    def speak(self):
        return "Woof"

d = Dog()
test("abc_concrete_ok", d.speak() == "Woof")

# ── Multiple abstract methods ──

class Shape(ABC):
    @abstractmethod
    def area(self):
        pass

    @abstractmethod
    def perimeter(self):
        pass

try:
    Shape()
    test("abc_multi_abstract", False)
except TypeError as e:
    msg = str(e)
    test("abc_multi_abstract", "Can't instantiate abstract class Shape" in msg)
    test("abc_multi_methods_mentioned", "area" in msg and "perimeter" in msg)
    test("abc_multi_plural", "methods" in msg)

# Partial implementation still fails
class PartialShape(Shape):
    def area(self):
        return 0

try:
    PartialShape()
    test("abc_partial_impl", False)
except TypeError as e:
    msg = str(e)
    test("abc_partial_impl", "perimeter" in msg)
    test("abc_partial_no_area", "area" not in msg)

# Full implementation succeeds
class Square(Shape):
    def __init__(self, side):
        self.side = side
    def area(self):
        return self.side ** 2
    def perimeter(self):
        return 4 * self.side

sq = Square(5)
test("abc_full_impl_area", sq.area() == 25)
test("abc_full_impl_perimeter", sq.perimeter() == 20)

# ── Multi-level inheritance ──

class Base(ABC):
    @abstractmethod
    def do_thing(self):
        pass

class Middle(Base):
    pass  # still abstract, doesn't implement do_thing

try:
    Middle()
    test("abc_multilevel_middle", False)
except TypeError as e:
    test("abc_multilevel_middle", "do_thing" in str(e))

class Concrete(Middle):
    def do_thing(self):
        return 42

c = Concrete()
test("abc_multilevel_concrete", c.do_thing() == 42)

# ── Summary ──
print("========================================")
print(f"phase63: {_pass + _fail} tests | Passed: {_pass} | Failed: {_fail}")
if _fail > 0:
    sys.exit(1)
