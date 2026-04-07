# Phase 122: json default=, frozen dataclass, InstanceDict serialization, stdlib depth

# --- 1. json.dumps with default= (Python lambda/function) ---
import json

class MyObj:
    def __init__(self):
        self.x = 42
        self.y = "hello"

# default=lambda calling __dict__
s = json.dumps(MyObj(), default=lambda o: o.__dict__)
parsed = json.loads(s)
assert parsed == {"x": 42, "y": "hello"}, f"Got: {parsed}"
print("json default lambda.__dict__: OK")

# default=vars (builtin)
s2 = json.dumps(MyObj(), default=vars)
assert json.loads(s2) == {"x": 42, "y": "hello"}
print("json default=vars: OK")

# default= with named function
def my_default(o):
    return {"x": o.x, "y": o.y}
s3 = json.dumps(MyObj(), default=my_default)
assert json.loads(s3) == {"x": 42, "y": "hello"}
print("json default named func: OK")

# --- 2. Frozen dataclass ---
from dataclasses import dataclass

@dataclass(frozen=True)
class Point:
    x: int
    y: int

p = Point(3, 4)
assert p.x == 3 and p.y == 4
try:
    p.x = 99
    assert False, "should have raised"
except AttributeError:
    pass
# Frozen + eq → hashable
assert hash(p) == hash(Point(3, 4))
assert p == Point(3, 4)
assert p != Point(1, 2)
print("frozen dataclass: OK")

# --- 3. Frozen dataclass with defaults ---
@dataclass(frozen=True)
class Config:
    name: str
    debug: bool = False

c = Config("test")
assert c.name == "test"
assert c.debug == False
print("frozen defaults: OK")

# --- 4. json with indent + default ---
class Nested:
    def __init__(self):
        self.items = [1, 2, 3]
        self.label = "data"

s4 = json.dumps(Nested(), default=vars, indent=2)
parsed4 = json.loads(s4)
assert parsed4 == {"items": [1, 2, 3], "label": "data"}
print("json indent+default: OK")

# --- 5. statistics module ---
import statistics
assert statistics.mean([1, 2, 3, 4, 5]) == 3.0
assert statistics.median([1, 2, 3, 4, 5]) == 3
print("statistics: OK")

# --- 6. functools.cached_property ---
import functools

class Expensive:
    def __init__(self):
        self.call_count = 0

    @functools.cached_property
    def value(self):
        self.call_count += 1
        return 42

e = Expensive()
assert e.value == 42
assert e.value == 42
print("cached_property: OK")

# --- 7. bisect module ---
import bisect
a = [1, 3, 5, 7, 9]
assert bisect.bisect_left(a, 5) == 2
assert bisect.bisect_right(a, 5) == 3
bisect.insort(a, 4)
assert a == [1, 3, 4, 5, 7, 9]
print("bisect: OK")

# --- 8. heapq nlargest/nsmallest ---
import heapq
data = [3, 1, 4, 1, 5, 9, 2, 6]
assert heapq.nlargest(3, data) == [9, 6, 5]
assert heapq.nsmallest(3, data) == [1, 1, 2]
print("heapq: OK")

# --- 9. enum.auto ---
from enum import Enum, auto

class Color(Enum):
    RED = auto()
    GREEN = auto()
    BLUE = auto()

assert Color.RED.value == 1
assert Color.GREEN.value == 2
assert Color.BLUE.value == 3
print("enum.auto: OK")

# --- 10. typing.Generic ---
from typing import TypeVar, Generic

T = TypeVar('T')

class Stack(Generic[T]):
    def __init__(self):
        self._items = []
    def push(self, item):
        self._items.append(item)
    def pop(self):
        return self._items.pop()
    def __len__(self):
        return len(self._items)

s = Stack()
s.push(1)
s.push(2)
assert len(s) == 2
assert s.pop() == 2
print("typing.Generic: OK")

print("All phase 122 tests passed!")
