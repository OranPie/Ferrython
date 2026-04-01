passed = 0
failed = 0
errors = []

def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        errors.append(name)
        print("FAIL:", name, "| got:", got, "| expected:", expected)

# ── Reflected operations ──
class MyNum:
    def __init__(self, val):
        self.val = val
    def __add__(self, other):
        if isinstance(other, MyNum):
            return MyNum(self.val + other.val)
        return MyNum(self.val + other)
    def __radd__(self, other):
        return MyNum(other + self.val)
    def __sub__(self, other):
        if isinstance(other, MyNum):
            return MyNum(self.val - other.val)
        return MyNum(self.val - other)
    def __rsub__(self, other):
        return MyNum(other - self.val)
    def __mul__(self, other):
        if isinstance(other, MyNum):
            return MyNum(self.val * other.val)
        return MyNum(self.val * other)
    def __rmul__(self, other):
        return MyNum(other * self.val)
    def __truediv__(self, other):
        if isinstance(other, MyNum):
            return MyNum(self.val / other.val)
        return MyNum(self.val / other)
    def __rtruediv__(self, other):
        return MyNum(other / self.val)

n = MyNum(10)

# Forward ops
test("fwd_add", (n + 5).val, 15)
test("fwd_sub", (n - 3).val, 7)
test("fwd_mul", (n * 2).val, 20)
test("fwd_div", (n / 4).val, 2.5)

# Reflected ops
test("radd", (5 + n).val, 15)
test("rsub", (20 - n).val, 10)
test("rmul", (3 * n).val, 30)
test("rtruediv", (100 / n).val, 10.0)

# Both instances
a = MyNum(7)
b = MyNum(3)
test("inst_add", (a + b).val, 10)
test("inst_sub", (a - b).val, 4)
test("inst_mul", (a * b).val, 21)

# ── Bitwise operations on instances ──
class Flags:
    def __init__(self, val):
        self.val = val
    def __and__(self, other):
        return Flags(self.val & other.val)
    def __or__(self, other):
        return Flags(self.val | other.val)
    def __xor__(self, other):
        return Flags(self.val ^ other.val)
    def __lshift__(self, other):
        return Flags(self.val << other)
    def __rshift__(self, other):
        return Flags(self.val >> other)

f1 = Flags(0b1100)
f2 = Flags(0b1010)
test("bit_and", (f1 & f2).val, 0b1000)
test("bit_or", (f1 | f2).val, 0b1110)
test("bit_xor", (f1 ^ f2).val, 0b0110)
test("bit_lshift", (Flags(1) << 3).val, 8)
test("bit_rshift", (Flags(16) >> 2).val, 4)

# ── Inplace operations ──
class Accum:
    def __init__(self, val):
        self.val = val
    def __iadd__(self, other):
        self.val += other
        return self
    def __isub__(self, other):
        self.val -= other
        return self
    def __imul__(self, other):
        self.val *= other
        return self

acc = Accum(10)
acc += 5
test("iadd", acc.val, 15)
acc -= 3
test("isub", acc.val, 12)
acc *= 2
test("imul", acc.val, 24)

# ── __contains__ on instance ──
class EvenNumbers:
    def __contains__(self, item):
        return item % 2 == 0

evens = EvenNumbers()
test("contains_true", 4 in evens, True)
test("contains_false", 3 in evens, False)
test("not_contains", 5 not in evens, True)

# ── Custom iterator protocol ──
class CountDown:
    def __init__(self, start):
        self.current = start
    def __iter__(self):
        return self
    def __next__(self):
        if self.current <= 0:
            raise StopIteration
        self.current -= 1
        return self.current + 1

# Use in for loop
result = []
for x in CountDown(5):
    result.append(x)
test("custom_iter_for", result, [5, 4, 3, 2, 1])

# Use next() builtin
cd = CountDown(3)
test("custom_next1", next(cd), 3)
test("custom_next2", next(cd), 2)
test("custom_next3", next(cd), 1)

# Use iter() builtin
cd2 = CountDown(2)
it = iter(cd2)
test("custom_iter_builtin", next(it), 2)

# ── Comparison dunders ──
class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __eq__(self, other):
        if isinstance(other, Point):
            return self.x == other.x and self.y == other.y
        return False
    def __ne__(self, other):
        return not self.__eq__(other)
    def __lt__(self, other):
        return (self.x ** 2 + self.y ** 2) < (other.x ** 2 + other.y ** 2)
    def __le__(self, other):
        return self.__lt__(other) or self.__eq__(other)
    def __gt__(self, other):
        return not self.__le__(other)
    def __ge__(self, other):
        return not self.__lt__(other)

p1 = Point(1, 2)
p2 = Point(1, 2)
p3 = Point(3, 4)

test("eq_true", p1 == p2, True)
test("eq_false", p1 == p3, False)
test("ne_true", p1 != p3, True)
test("lt_true", p1 < p3, True)
test("lt_false", p3 < p1, False)
test("le_true", p1 <= p2, True)
test("gt_true", p3 > p1, True)
test("ge_true", p3 >= p1, True)

# ── __str__ and __repr__ ──
class Color:
    def __init__(self, r, g, b):
        self.r = r
        self.g = g
        self.b = b
    def __str__(self):
        return "Color({}, {}, {})".format(self.r, self.g, self.b)
    def __repr__(self):
        return "Color(r={}, g={}, b={})".format(self.r, self.g, self.b)

c = Color(255, 128, 0)
test("str_dispatch", str(c), "Color(255, 128, 0)")
test("repr_dispatch", repr(c), "Color(r=255, g=128, b=0)")

# ── __call__ on instance ──
class Adder:
    def __init__(self, base):
        self.base = base
    def __call__(self, x):
        return self.base + x

add5 = Adder(5)
test("call_inst", add5(10), 15)
test("call_inst2", add5(20), 25)
test("callable_check", callable(add5), True)

# ── __getitem__/__setitem__/__delitem__ ──
class MyList:
    def __init__(self):
        self.data = {}
    def __getitem__(self, key):
        return self.data.get(key, 0)
    def __setitem__(self, key, value):
        self.data[key] = value
    def __delitem__(self, key):
        if key in self.data:
            del self.data[key]
    def __len__(self):
        return len(self.data)

ml = MyList()
ml["a"] = 10
ml["b"] = 20
test("getitem", ml["a"], 10)
test("getitem_miss", ml["c"], 0)
test("len_custom", len(ml), 2)
del ml["a"]
test("delitem", ml["a"], 0)
test("len_after_del", len(ml), 1)

# ── __bool__ and __len__ truthiness ──
class Empty:
    def __bool__(self):
        return False

class NonEmpty:
    def __bool__(self):
        return True

test("bool_false", bool(Empty()), False)
test("bool_true", bool(NonEmpty()), True)

class SizedEmpty:
    def __len__(self):
        return 0

class SizedNonEmpty:
    def __len__(self):
        return 5

test("len_truthy_0", bool(SizedEmpty()), False)
test("len_truthy_5", bool(SizedNonEmpty()), True)

# ── __class__ and __dict__ ──
class Person:
    def __init__(self, name, age):
        self.name = name
        self.age = age

p = Person("Alice", 30)
test("class_name", p.__class__.__name__, "Person")
test("dict_name", p.__dict__["name"], "Alice")
test("dict_age", p.__dict__["age"], 30)

# ── type() with 3 args dynamic class ──
Animal = type("Animal", (), {"species": "unknown", "legs": 4})
a = Animal()
test("type3_species", a.species, "unknown")
test("type3_legs", a.legs, 4)

# With inheritance
Cat = type("Cat", (Animal,), {"species": "cat", "sound": "meow"})
cat = Cat()
test("type3_child_species", cat.species, "cat")
test("type3_child_sound", cat.sound, "meow")
test("type3_child_legs", cat.legs, 4)  # inherited

# ── Sorted with key ──
words = ["banana", "apple", "cherry", "date"]
test("sorted_key", sorted(words, key=len), ["date", "apple", "banana", "cherry"])
test("sorted_key_rev", sorted(words, key=len, reverse=True), ["cherry", "banana", "apple", "date"])

nums = [3, 1, -4, 1, -5, 9, 2, -6]
test("sorted_key_abs", sorted(nums, key=abs), [1, 1, 2, 3, -4, -5, -6, 9])

# Sort by attribute
class Student:
    def __init__(self, name, grade):
        self.name = name
        self.grade = grade

students = [Student("Alice", 90), Student("Bob", 85), Student("Charlie", 95)]
sorted_students = sorted(students, key=lambda s: s.grade)
test("sorted_key_attr", [s.name for s in sorted_students], ["Bob", "Alice", "Charlie"])

# ── Hash dispatch ──
class HashablePoint:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __hash__(self):
        return self.x * 31 + self.y
    def __eq__(self, other):
        return isinstance(other, HashablePoint) and self.x == other.x and self.y == other.y

hp = HashablePoint(1, 2)
test("hash_dispatch", hash(hp), 1 * 31 + 2)

# ── Chained method calls ──
class Builder:
    def __init__(self):
        self.items = []
    def add(self, item):
        self.items.append(item)
        return self
    def build(self):
        return self.items

result = Builder().add(1).add(2).add(3).build()
test("chained_methods", result, [1, 2, 3])

# ── Multiple inheritance method resolution ──
class A:
    def method(self):
        return "A"

class B(A):
    def method(self):
        return "B"

class C(A):
    def method(self):
        return "C"

class D(B, C):
    pass

d = D()
test("mro_diamond", d.method(), "B")

# ── Generator expression with conditional ──
gen_result = list(x * x for x in range(10) if x % 2 == 0)
test("genexp_cond", gen_result, [0, 4, 16, 36, 64])

# ── Nested comprehensions ──
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat = [x for row in matrix for x in row]
test("nested_comp", flat, [1, 2, 3, 4, 5, 6, 7, 8, 9])

# ── Dict comprehension with computation ──
squares = {x: x*x for x in range(6)}
test("dictcomp", squares, {0: 0, 1: 1, 2: 4, 3: 9, 4: 16, 5: 25})

# ── Complex inheritance ──
class Serializable:
    def serialize(self):
        items = []
        for k, v in self.__dict__.items():
            items.append(k + "=" + repr(v))
        return self.__class__.__name__ + "(" + ", ".join(items) + ")"

class NamedThing(Serializable):
    def __init__(self, name):
        self.name = name

class ColoredThing(NamedThing):
    def __init__(self, name, color):
        super().__init__(name)
        self.color = color

ct = ColoredThing("apple", "red")
s = ct.serialize()
test("serialize_class", "ColoredThing" in s, True)
test("serialize_name", "name" in s, True)
test("serialize_color", "color" in s, True)

# ── Exception hierarchy isinstance ──
try:
    raise ValueError("test")
except Exception as e:
    test("exc_isinstance", isinstance(e, ValueError), True)

# ── Default argument with next ──
cd = CountDown(1)
next(cd)  # gets 1
test("next_default", next(cd, "done"), "done")

print("========================================")
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("Failed tests:", ", ".join(errors))
print("========================================")
