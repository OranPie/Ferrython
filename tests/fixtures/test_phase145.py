# Phase 145: Metaclass inheritance, repr dispatch, logging propagation
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"FAIL: {name}")

# ── Metaclass inheritance ──
class Meta(type):
    created = []
    def __new__(mcs, name, bases, ns):
        Meta.created.append(name)
        return super().__new__(mcs, name, bases, ns)

class Base(metaclass=Meta):
    pass

class Child(Base):
    pass

class GrandChild(Child):
    pass

test("metaclass inherited to child", type(Child).__name__ == "Meta")
test("metaclass inherited to grandchild", type(GrandChild).__name__ == "Meta")
test("metaclass __new__ called for all", Meta.created == ["Base", "Child", "GrandChild"])

# Factory registry pattern via metaclass
class FactoryMeta(type):
    _registry = {}
    def __new__(mcs, name, bases, ns):
        cls = super().__new__(mcs, name, bases, ns)
        if name != 'Animal':
            mcs._registry[name.lower()] = cls
        return cls

class Animal(metaclass=FactoryMeta):
    def speak(self): return '...'

class Dog(Animal):
    def speak(self): return 'woof'

class Cat(Animal):
    def speak(self): return 'meow'

test("factory registry dog", FactoryMeta._registry['dog']().speak() == 'woof')
test("factory registry cat", FactoryMeta._registry['cat']().speak() == 'meow')

# Singleton metaclass
class SingletonMeta(type):
    _instances = {}
    def __call__(cls, *args, **kw):
        if cls not in cls._instances:
            cls._instances[cls] = super().__call__(*args, **kw)
        return cls._instances[cls]

class SingleBase(metaclass=SingletonMeta):
    pass

class MySingleton(SingleBase):
    def __init__(self):
        self.value = 42

a = MySingleton()
b = MySingleton()
test("singleton via inherited metaclass", a is b)
test("singleton data correct", a.value == 42)

# ── repr() dispatches native closures ──
from dataclasses import dataclass

@dataclass
class Point:
    x: float
    y: float

@dataclass
class Line:
    start: Point
    end: Point

line = Line(Point(1, 2), Point(3, 4))
test("nested dataclass repr", repr(line) == "Line(start=Point(x=1, y=2), end=Point(x=3, y=4))")
test("dataclass point repr", repr(Point(5, 6)) == "Point(x=5, y=6)")

# ── Logging propagation ──
import logging
import io

parent = logging.getLogger('test145')
parent.setLevel(logging.DEBUG)
stream = io.StringIO()
handler = logging.StreamHandler(stream)
handler.setFormatter(logging.Formatter('%(levelname)s:%(name)s:%(message)s'))
parent.addHandler(handler)

child = logging.getLogger('test145.sub')
child.warning('child msg')

output = stream.getvalue()
test("child log propagated to parent", 'child msg' in output)
test("child name in log", 'test145.sub' in output)

# Grandchild inherits effective level from parent
gc = logging.getLogger('test145.sub.deep')
gc.debug('gc debug')  # should propagate up because parent is DEBUG
output2 = stream.getvalue()
test("grandchild inherited DEBUG level", 'gc debug' in output2)

# Same logger returned on repeat call
test("logger caching", logging.getLogger('test145') is parent)
test("child logger caching", logging.getLogger('test145.sub') is child)

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed:
    raise SystemExit(1)
print("ALL PHASE 145 TESTS PASSED")
