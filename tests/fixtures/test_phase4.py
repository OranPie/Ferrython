# ═══════════════════════════════════════════
# Phase 4 Tests — Dunder Methods, super(), Nested Comprehensions,
# Callable Objects, Custom Iterators, Multiple Except Types
# ═══════════════════════════════════════════

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL: " + name)

# ── super() ──

def test_super_basic():
    class Animal:
        def __init__(self, name):
            self.name = name
        def speak(self):
            return "..."

    class Dog(Animal):
        def __init__(self, name, breed):
            super().__init__(name)
            self.breed = breed
        def speak(self):
            return "Woof!"

    d = Dog("Rex", "Labrador")
    test("super_init", d.name == "Rex")
    test("super_child_attr", d.breed == "Labrador")
    test("super_override", d.speak() == "Woof!")

def test_super_chain():
    class A:
        def greet(self):
            return "A"
    class B(A):
        def greet(self):
            return "B+" + super().greet()
    class C(B):
        def greet(self):
            return "C+" + super().greet()
    
    c = C()
    test("super_chain", c.greet() == "C+B+A")

# ── Dunder Arithmetic ──

def test_dunder_add():
    class Vector:
        def __init__(self, x, y):
            self.x = x
            self.y = y
        def __add__(self, other):
            return Vector(self.x + other.x, self.y + other.y)
        def __mul__(self, scalar):
            return Vector(self.x * scalar, self.y * scalar)
        def __eq__(self, other):
            return self.x == other.x and self.y == other.y
        def __str__(self):
            return f"Vector({self.x}, {self.y})"
    
    v1 = Vector(1, 2)
    v2 = Vector(3, 4)
    v3 = v1 + v2
    test("dunder_add", v3.x == 4 and v3.y == 6)
    
    v4 = v1 * 3
    test("dunder_mul", v4.x == 3 and v4.y == 6)
    
    test("dunder_eq_true", Vector(1, 2) == Vector(1, 2))
    test("dunder_eq_false", not (Vector(1, 2) == Vector(3, 4)))
    test("dunder_str", str(v3) == "Vector(4, 6)")

def test_dunder_sub():
    class Money:
        def __init__(self, amount):
            self.amount = amount
        def __sub__(self, other):
            return Money(self.amount - other.amount)
        def __lt__(self, other):
            return self.amount < other.amount
        def __le__(self, other):
            return self.amount <= other.amount
    
    m1 = Money(100)
    m2 = Money(30)
    m3 = m1 - m2
    test("dunder_sub", m3.amount == 70)
    test("dunder_lt", m2 < m1)
    test("dunder_le", Money(50) <= Money(50))

# ── Dunder __getitem__ / __setitem__ ──

def test_dunder_getitem():
    class Matrix:
        def __init__(self, rows):
            self.rows = rows
        def __getitem__(self, idx):
            return self.rows[idx]
        def __setitem__(self, idx, val):
            self.rows[idx] = val
        def __len__(self):
            return len(self.rows)
    
    m = Matrix([[1, 2], [3, 4], [5, 6]])
    test("dunder_getitem", m[0] == [1, 2])
    test("dunder_getitem_1", m[1] == [3, 4])
    test("dunder_len", len(m) == 3)
    
    m[1] = [7, 8]
    test("dunder_setitem", m[1] == [7, 8])

# ── Dunder __contains__ ──

def test_dunder_contains():
    class Bag:
        def __init__(self, items):
            self.items = items
        def __contains__(self, item):
            return item in self.items
    
    b = Bag([1, 2, 3, 4, 5])
    test("dunder_in", 3 in b)
    test("dunder_not_in", 10 not in b)

# ── Callable Objects (__call__) ──

def test_callable():
    class Adder:
        def __init__(self, n):
            self.n = n
        def __call__(self, x):
            return self.n + x
    
    add5 = Adder(5)
    test("callable_obj", add5(10) == 15)
    test("callable_obj2", add5(0) == 5)

def test_callable_counter():
    class Counter:
        def __init__(self):
            self.count = 0
        def __call__(self):
            self.count = self.count + 1
            return self.count
    
    c = Counter()
    test("callable_counter_1", c() == 1)
    test("callable_counter_2", c() == 2)
    test("callable_counter_3", c() == 3)

# ── Custom Iterator (__iter__ / __next__) ──

def test_custom_iterator():
    class Countdown:
        def __init__(self, start):
            self.current = start
        def __iter__(self):
            return self
        def __next__(self):
            if self.current <= 0:
                raise StopIteration()
            val = self.current
            self.current = self.current - 1
            return val
    
    result = []
    for x in Countdown(5):
        result.append(x)
    test("custom_iter", result == [5, 4, 3, 2, 1])

def test_custom_iter_list():
    class Repeat:
        def __init__(self, value, times):
            self.value = value
            self.times = times
            self.count = 0
        def __iter__(self):
            return self
        def __next__(self):
            if self.count >= self.times:
                raise StopIteration()
            self.count = self.count + 1
            return self.value
    
    result = list(Repeat("hello", 3))
    test("custom_iter_list", result == ["hello", "hello", "hello"])

# ── Nested Comprehensions ──

def test_nested_listcomp():
    result = [x * y for x in range(1, 4) for y in range(1, 4)]
    test("nested_listcomp", result == [1, 2, 3, 2, 4, 6, 3, 6, 9])

def test_nested_listcomp_filter():
    result = [x * y for x in range(1, 5) for y in range(1, 5) if x != y]
    expected = [2, 3, 4, 2, 6, 8, 3, 6, 12, 4, 8, 12]
    test("nested_listcomp_filter", result == expected)

def test_flatten():
    matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
    flat = [x for row in matrix for x in row]
    test("flatten", flat == [1, 2, 3, 4, 5, 6, 7, 8, 9])

# ── Multiple except types (tuple) ──

def test_except_tuple():
    caught = None
    try:
        x = 1 / 0
    except (ValueError, ZeroDivisionError):
        caught = "div"
    test("except_tuple_zerodiv", caught == "div")

def test_except_tuple_value():
    caught = None
    try:
        x = int("abc")
    except (ValueError, TypeError):
        caught = "val"
    test("except_tuple_value", caught == "val")

def test_except_tuple_fallthrough():
    caught = None
    try:
        x = {}["missing"]
    except (ValueError, TypeError):
        caught = "wrong"
    except KeyError:
        caught = "key"
    test("except_tuple_fallthrough", caught == "key")

# ── Dunder __bool__ for truthiness ──

def test_dunder_bool():
    class Empty:
        def __bool__(self):
            return False
    
    class Full:
        def __bool__(self):
            return True
    
    test("dunder_bool_false", not Empty())
    test("dunder_bool_true", bool(Full()) == True)

# ── Dunder __repr__ ──

def test_dunder_repr():
    class Point:
        def __init__(self, x, y):
            self.x = x
            self.y = y
        def __repr__(self):
            return f"Point({self.x}, {self.y})"
    
    p = Point(1, 2)
    test("dunder_repr", repr(p) == "Point(1, 2)")

# ── Advanced super() with method calls ──

def test_super_method():
    class Shape:
        def area(self):
            return 0
        def describe(self):
            return f"area={self.area()}"
    
    class Circle(Shape):
        def __init__(self, r):
            self.r = r
        def area(self):
            return 3.14159 * self.r * self.r
    
    c = Circle(5)
    test("super_method", abs(c.area() - 78.5397) < 0.1)
    test("super_describe", "area=" in c.describe())

# ── Chained super ──

def test_super_with_attrs():
    class Base:
        def __init__(self):
            self.base_val = 10
    
    class Mid(Base):
        def __init__(self):
            super().__init__()
            self.mid_val = 20
    
    class Top(Mid):
        def __init__(self):
            super().__init__()
            self.top_val = 30
    
    t = Top()
    test("super_chain_base", t.base_val == 10)
    test("super_chain_mid", t.mid_val == 20)
    test("super_chain_top", t.top_val == 30)

# ── Set/Dict comprehension nested ──

def test_set_comp():
    result = {x * x for x in range(5)}
    test("set_comp", result == {0, 1, 4, 9, 16})

def test_dict_comp():
    result = {k: k * k for k in range(5)}
    test("dict_comp", result == {0: 0, 1: 1, 2: 4, 3: 9, 4: 16})

# ── Run all tests ──

test_super_basic()
test_super_chain()
test_dunder_add()
test_dunder_sub()
test_dunder_getitem()
test_dunder_contains()
test_callable()
test_callable_counter()
test_custom_iterator()
test_custom_iter_list()
test_nested_listcomp()
test_nested_listcomp_filter()
test_flatten()
test_except_tuple()
test_except_tuple_value()
test_except_tuple_fallthrough()
test_dunder_bool()
test_dunder_repr()
test_super_method()
test_super_with_attrs()
test_set_comp()
test_dict_comp()

print("========================================")
print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(f"SOME TESTS FAILED: {failed}")
