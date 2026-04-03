# test_cpython_compat100.py - Advanced class features
passed100 = 0
total100 = 0

def check100(desc, got, expected):
    global passed100, total100
    total100 += 1
    if got == expected:
        passed100 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- Multiple inheritance MRO ---
class A100:
    def who(self):
        return "A"

class B100(A100):
    def who(self):
        return "B"

class C100(A100):
    def who(self):
        return "C"

class D100(B100, C100):
    pass

d100 = D100()
check100("MRO picks B over C", d100.who(), "B")
check100("MRO order", [cls.__name__ for cls in D100.__mro__], ["D100", "B100", "C100", "A100", "object"])

# --- super() in multiple inheritance ---
class Base100:
    def greet(self):
        return "Base"

class Left100(Base100):
    def greet(self):
        return "Left+" + super().greet()

class Right100(Base100):
    def greet(self):
        return "Right+" + super().greet()

class Child100(Left100, Right100):
    def greet(self):
        return "Child+" + super().greet()

check100("super chain", Child100().greet(), "Child+Left+Right+Base")

# --- classmethod and staticmethod ---
class MyClass100:
    class_var = 10

    @classmethod
    def get_class_var(cls):
        return cls.class_var

    @staticmethod
    def add(a, b):
        return a + b

check100("classmethod", MyClass100.get_class_var(), 10)
check100("classmethod from instance", MyClass100().get_class_var(), 10)
check100("staticmethod", MyClass100.add(3, 4), 7)
check100("staticmethod from instance", MyClass100().add(3, 4), 7)

# --- __repr__ and __str__ ---
class Repr100:
    def __init__(self, val):
        self.val = val
    def __repr__(self):
        return f"Repr100({self.val!r})"
    def __str__(self):
        return f"Value: {self.val}"

r100 = Repr100(42)
check100("__repr__", repr(r100), "Repr100(42)")
check100("__str__", str(r100), "Value: 42")

# --- Comparison operators ---
class Num100:
    def __init__(self, val):
        self.val = val
    def __eq__(self, other):
        return self.val == other.val
    def __ne__(self, other):
        return self.val != other.val
    def __lt__(self, other):
        return self.val < other.val
    def __gt__(self, other):
        return self.val > other.val
    def __le__(self, other):
        return self.val <= other.val
    def __ge__(self, other):
        return self.val >= other.val

a100 = Num100(5)
b100 = Num100(10)
c100 = Num100(5)

check100("__eq__ true", a100 == c100, True)
check100("__eq__ false", a100 == b100, False)
check100("__ne__ true", a100 != b100, True)
check100("__ne__ false", a100 != c100, False)
check100("__lt__ true", a100 < b100, True)
check100("__lt__ false", b100 < a100, False)
check100("__gt__ true", b100 > a100, True)
check100("__gt__ false", a100 > b100, False)
check100("__le__ true equal", a100 <= c100, True)
check100("__le__ true less", a100 <= b100, True)
check100("__ge__ true equal", a100 >= c100, True)
check100("__ge__ true greater", b100 >= a100, True)

# --- __hash__ ---
class Hashable100:
    def __init__(self, val):
        self.val = val
    def __hash__(self):
        return hash(self.val)
    def __eq__(self, other):
        return self.val == other.val

check100("__hash__", hash(Hashable100(42)), hash(42))
s100 = {Hashable100(1), Hashable100(2), Hashable100(1)}
check100("__hash__ in set dedup", len(s100), 2)

# --- __bool__ ---
class Truthy100:
    def __bool__(self):
        return True

class Falsy100:
    def __bool__(self):
        return False

check100("__bool__ truthy", bool(Truthy100()), True)
check100("__bool__ falsy", bool(Falsy100()), False)
check100("__bool__ truthy in if", "yes" if Truthy100() else "no", "yes")
check100("__bool__ falsy in if", "yes" if Falsy100() else "no", "no")

# --- __len__ ---
class Sized100:
    def __init__(self, n):
        self.n = n
    def __len__(self):
        return self.n

check100("__len__", len(Sized100(5)), 5)
check100("__len__ zero is falsy", bool(Sized100(0)), False)
check100("__len__ nonzero is truthy", bool(Sized100(3)), True)

# --- __contains__ ---
class Container100:
    def __init__(self, items):
        self.items = items
    def __contains__(self, item):
        return item in self.items

c100_cont = Container100([1, 2, 3])
check100("__contains__ true", 2 in c100_cont, True)
check100("__contains__ false", 5 in c100_cont, False)
check100("__contains__ not in", 5 not in c100_cont, True)

# --- __getitem__ and __setitem__ ---
class MyList100:
    def __init__(self):
        self.data = {}
    def __getitem__(self, key):
        return self.data[key]
    def __setitem__(self, key, val):
        self.data[key] = val

ml100 = MyList100()
ml100[0] = "hello"
ml100[1] = "world"
check100("__setitem__/__getitem__", ml100[0], "hello")
check100("__setitem__/__getitem__ 2", ml100[1], "world")

# --- __iter__ and __next__ ---
class Counter100:
    def __init__(self, low, high):
        self.current = low
        self.high = high
    def __iter__(self):
        return self
    def __next__(self):
        if self.current >= self.high:
            raise StopIteration
        val = self.current
        self.current += 1
        return val

check100("__iter__/__next__", list(Counter100(1, 5)), [1, 2, 3, 4])
check100("__iter__ in for loop", sum(x for x in Counter100(1, 4)), 6)

# --- __call__ ---
class Adder100:
    def __init__(self, n):
        self.n = n
    def __call__(self, x):
        return self.n + x

add5_100 = Adder100(5)
check100("__call__", add5_100(3), 8)
check100("__call__ again", add5_100(10), 15)
check100("__call__ callable", callable(add5_100), True)

# --- __add__, __radd__, __iadd__ ---
class Vec100:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __add__(self, other):
        return Vec100(self.x + other.x, self.y + other.y)
    def __radd__(self, other):
        if other == 0:
            return self
        return NotImplemented
    def __iadd__(self, other):
        self.x += other.x
        self.y += other.y
        return self
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    def __repr__(self):
        return f"Vec100({self.x}, {self.y})"

v1_100 = Vec100(1, 2)
v2_100 = Vec100(3, 4)
v3_100 = v1_100 + v2_100
check100("__add__", v3_100, Vec100(4, 6))

v4_100 = Vec100(1, 1)
v4_100 += Vec100(2, 3)
check100("__iadd__", v4_100, Vec100(3, 4))

# __radd__ with sum()
check100("__radd__ with sum", sum([Vec100(1, 2), Vec100(3, 4)]), Vec100(4, 6))

print(f"Tests: {total100} | Passed: {passed100} | Failed: {total100 - passed100}")
