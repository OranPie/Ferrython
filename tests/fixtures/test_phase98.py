# test_phase98.py — CPython alignment fixes: namedtuple str, walrus scope,
# __class__ cell, __init__ return, iterator type names
checks = 0

# 1. namedtuple str() shows named fields
from collections import namedtuple
Point = namedtuple("Point", ["x", "y"])
p = Point(1, 2)
assert str(p) == "Point(x=1, y=2)", f"got {str(p)}"
assert repr(p) == "Point(x=1, y=2)", f"got {repr(p)}"
checks += 1

# 2. namedtuple with string values uses repr for values
Entry = namedtuple("Entry", ["key", "value"])
e = Entry("name", "Alice")
s = repr(e)
assert "key=" in s and "value=" in s, f"got {s}"
checks += 1

# 3. walrus operator in list comprehension leaks to outer scope
results = [y := x * 2 for x in range(5)]
assert results == [0, 2, 4, 6, 8]
assert y == 8, f"got y={y}"
checks += 1

# 4. walrus in nested comprehension
[z := i for i in range(3)]
assert z == 2
checks += 1

# 5. __class__ cell in method
class A:
    def get_class(self):
        return __class__.__name__
assert A().get_class() == "A"
checks += 1

# 6. __class__ in subclass method refers to defining class
class B(A):
    def get_class(self):
        return __class__.__name__
assert B().get_class() == "B"
checks += 1

# 7. __init__ returning non-None raises TypeError
class Bad:
    def __init__(self):
        return 42
try:
    Bad()
    assert False, "should have raised TypeError"
except TypeError as e:
    assert "__init__" in str(e)
checks += 1

# 8. Normal __init__ works fine
class Good:
    def __init__(self, x):
        self.x = x
assert Good(10).x == 10
checks += 1

# 9. map type name
m = map(str, [1, 2])
assert type(m).__name__ == "map", f"got {type(m).__name__}"
checks += 1

# 10. filter type name
f = filter(None, [1, 0, 2])
assert type(f).__name__ == "filter", f"got {type(f).__name__}"
checks += 1

# 11. zip type name
z = zip([1], [2])
assert type(z).__name__ == "zip", f"got {type(z).__name__}"
checks += 1

# 12. enumerate type name
e = enumerate([1, 2])
assert type(e).__name__ == "enumerate", f"got {type(e).__name__}"
checks += 1

# 13. __class__ with super() chain
class X:
    def who(self):
        return __class__.__name__
class Y(X):
    def who(self):
        return __class__.__name__ + "+" + super().who()
assert Y().who() == "Y+X"
checks += 1

# 14. walrus with pre-existing variable
w = "before"
[w := i * 10 for i in range(3)]
assert w == 20, f"got {w}"
checks += 1

# 15. namedtuple in f-string
P2 = namedtuple("P2", "x y")
p2 = P2(3, 4)
assert f"{p2}" == "P2(x=3, y=4)", f"got {f'{p2}'}"
checks += 1

print(f"test_phase98: {checks}/15 checks passed")
