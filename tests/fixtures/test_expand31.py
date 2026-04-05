"""Test expanded features: frozenset comparison, runtime_checkable Protocol,
re.compile groups, SimpleNamespace, difflib kwargs, fcntl import."""
checks = 0

# frozenset comparison operators
a = frozenset([1, 2])
b = frozenset([1, 2, 3])
assert a < b, "frozenset proper subset"
assert a <= b, "frozenset subset"
assert b > a, "frozenset proper superset"
assert b >= a, "frozenset superset"
assert a == frozenset([1, 2]), "frozenset equality"
assert a != frozenset([3, 4]), "frozenset inequality"
# Mixed frozenset/set comparison
assert frozenset([1, 2]) < {1, 2, 3}, "frozenset < set"
assert {1, 2} < frozenset([1, 2, 3]), "set < frozenset"
checks += 1
print("PASS frozenset_comparison")

# runtime_checkable Protocol
from typing import Protocol, runtime_checkable

@runtime_checkable
class Drawable(Protocol):
    def draw(self) -> str: ...

class Circle:
    def draw(self):
        return "circle"

class Square:
    pass

assert isinstance(Circle(), Drawable), "Circle implements Drawable"
assert not isinstance(Square(), Drawable), "Square does not implement Drawable"
assert not isinstance("hello", Drawable), "str does not implement Drawable"
checks += 1
print("PASS runtime_checkable_protocol")

# re.compile groups and groupindex
import re
p = re.compile(r"(?P<first>\w+)\s+(?P<last>\w+)")
assert p.groups == 2, f"Pattern.groups: expected 2, got {p.groups}"
assert p.groupindex == {"first": 1, "last": 2}, f"Pattern.groupindex: {p.groupindex}"
p2 = re.compile(r"(\d+)-(\d+)")
assert p2.groups == 2
assert p2.groupindex == {}
checks += 1
print("PASS re_compile_groups")

# SimpleNamespace repr and equality
import types
ns = types.SimpleNamespace(x=1, y="hello")
r = repr(ns)
assert "namespace(" in r, f"SimpleNamespace repr: {r}"
assert "x=1" in r, f"SimpleNamespace repr missing x: {r}"
ns2 = types.SimpleNamespace(x=1, y="hello")
assert ns == ns2, "SimpleNamespace equality"
assert not (ns == types.SimpleNamespace(x=2, y="hello")), "SimpleNamespace inequality"
checks += 1
print("PASS simplenamespace")

# difflib with kwargs
import difflib
s1 = ["one", "two"]
s2 = ["one", "TWO", "three"]
d = list(difflib.unified_diff(s1, s2, fromfile="a.txt", tofile="b.txt"))
assert len(d) > 0, "unified_diff should produce output"
assert any("a.txt" in str(line) for line in d), "fromfile kwarg should work"
checks += 1
print("PASS difflib_kwargs")

# fcntl import
import fcntl
assert hasattr(fcntl, "fcntl")
assert hasattr(fcntl, "flock")
assert hasattr(fcntl, "LOCK_EX")
assert fcntl.LOCK_EX == 2
checks += 1
print("PASS fcntl_import")

# Division/modulo fast paths
assert 10 % 3 == 1
assert -10 % 3 == 2
assert 10 / 3 == 10/3
assert 10 // 3 == 3
assert -10 // 3 == -4
assert 7.5 % 2.5 == 0.0
assert 7.5 // 2.5 == 3.0
checks += 1
print("PASS division_modulo")

print(f"\n{'='*40}")
print(f"Tests: {checks} | Passed: {checks} | Failed: 0")
print("ALL TESTS PASSED!")
