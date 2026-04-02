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

# ── ABC ──
from abc import ABC, abstractmethod

class Shape(ABC):
    @abstractmethod
    def area(self):
        pass

class Circle(Shape):
    def __init__(self, r):
        self.r = r
    def area(self):
        return 3.14159 * self.r ** 2

test("abc concrete", abs(Circle(5).area() - 78.54) < 0.1)
try:
    Shape()
    test("abc instantiate err", False)
except TypeError:
    test("abc instantiate err", True)

# ── yield from return value ──
def sub_gen():
    yield 1
    yield 2
    return "done"

def main_gen():
    result = yield from sub_gen()
    yield result

test("yield from return", list(main_gen()) == [1, 2, "done"])

# ── JSON __dict__ ──
import json
class User:
    def __init__(self, name, age):
        self.name = name
        self.age = age

j = json.dumps(User("Alice", 30).__dict__)
test("json __dict__", json.loads(j)["name"] == "Alice")

# ── Property deleter ──
class Person:
    def __init__(self, name):
        self._name = name
    @property
    def name(self):
        return self._name
    @name.setter
    def name(self, value):
        self._name = value
    @name.deleter
    def name(self):
        self._name = None

p = Person("Alice")
p.name = "Bob"
del p.name
test("property deleter", p.name is None)

# ── int(str, base) ──
test("int base 2", int("1010", 2) == 10)
test("int base 16", int("ff", 16) == 255)
test("int base 8", int("77", 8) == 63)
test("int base prefix", int("0xff", 16) == 255)

# ── Recursive repr ──
class Node:
    def __init__(self, val, children=None):
        self.val = val
        self.children = children or []
    def __repr__(self):
        if self.children:
            return f"Node({self.val}, {self.children})"
        return f"Node({self.val})"

tree = Node(1, [Node(2), Node(3, [Node(4)])])
test("recursive repr", repr(tree) == "Node(1, [Node(2), Node(3, [Node(4)])])")

# ── Identity eq for objects ──
class Obj:
    pass
a = Obj()
b = Obj()
test("identity eq same", a == a)
test("identity eq diff", a != b)

# ── BuiltinType subclass ──
class MyDict(dict):
    pass
d = MyDict()
test("dict subclass", type(d).__name__ == "MyDict")

# ── lru_cache ──
from functools import lru_cache

call_count = 0
@lru_cache(maxsize=128)
def fibonacci(n):
    global call_count
    call_count += 1
    if n < 2:
        return n
    return fibonacci(n - 1) + fibonacci(n - 2)

test("lru_cache fib", fibonacci(10) == 55)
test("lru_cache cached", call_count == 11)

# ── Enum ──
from enum import Enum, auto

class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3

test("enum value", Color.RED.value == 1)
test("enum name", Color.RED.name == "RED")
test("enum identity", Color.RED is Color.RED)

print(f"\nTests: {_pass + _fail} | Passed: {_pass} | Failed: {_fail}")
if _fail > 0:
    sys.exit(1)
