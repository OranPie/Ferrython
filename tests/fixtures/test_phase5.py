# ═══════════════════════════════════════════
# Phase 5 Tests — yield from, filesystem imports, REPL features,
# unary dunders, with-files, walrus, format, class closures,
# super chains, MRO, set/dict equality, advanced comprehensions
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

# ── yield from ──

def test_yield_from():
    def inner():
        yield 1
        yield 2
        yield 3

    def outer():
        yield from inner()
        yield 4

    test("yield_from_basic", list(outer()) == [1, 2, 3, 4])

    # yield from range
    def gen_range():
        yield from range(5)
    test("yield_from_range", list(gen_range()) == [0, 1, 2, 3, 4])

    # chained yield from
    def a():
        yield 10
        yield 20
    def b():
        yield from a()
        yield 30
    def c():
        yield from b()
        yield 40
    test("yield_from_chain", list(c()) == [10, 20, 30, 40])

    # yield from with list iterator
    def from_list():
        yield from [100, 200, 300]
    test("yield_from_list", list(from_list()) == [100, 200, 300])

    # yield from with string
    def from_str():
        yield from "abc"
    test("yield_from_str", list(from_str()) == ["a", "b", "c"])

    # yield from + local work
    def mixed():
        yield 0
        yield from range(1, 4)
        yield 4
    test("yield_from_mixed", list(mixed()) == [0, 1, 2, 3, 4])

    # nested yield from
    def leaf():
        yield "leaf"
    def mid():
        yield "mid_start"
        yield from leaf()
        yield "mid_end"
    def top():
        yield "top_start"
        yield from mid()
        yield "top_end"
    test("yield_from_nested", list(top()) == ["top_start", "mid_start", "leaf", "mid_end", "top_end"])

test_yield_from()

# ── Unary dunders ──

def test_unary_dunders():
    class Vec:
        def __init__(self, x, y):
            self.x = x
            self.y = y
        def __neg__(self):
            return Vec(-self.x, -self.y)
        def __pos__(self):
            return Vec(abs(self.x), abs(self.y))
        def __abs__(self):
            return (self.x**2 + self.y**2)**0.5
        def __invert__(self):
            return Vec(self.y, self.x)

    v = Vec(3, -4)
    nv = -v
    test("dunder_neg", nv.x == -3 and nv.y == 4)

    pv = +v
    test("dunder_pos", pv.x == 3 and pv.y == 4)

    test("dunder_abs", abs(v) == 5.0)

    iv = ~v
    test("dunder_invert", iv.x == -4 and iv.y == 3)

test_unary_dunders()

# ── with statement on files ──

def test_with_files():
    # Write with 'with'
    with open("/tmp/ferrython_with_test.txt", "w") as f:
        f.write("line1\nline2\nline3")

    # Read with 'with'
    with open("/tmp/ferrython_with_test.txt", "r") as f:
        content = f.read()
    test("with_file_readwrite", content == "line1\nline2\nline3")

    # Read lines with 'with'
    with open("/tmp/ferrython_with_test.txt", "r") as f:
        lines = f.readlines()
    test("with_file_readlines", len(lines) == 3)
    test("with_file_first_line", lines[0] == "line1\n")

test_with_files()

# ── Walrus operator ──

def test_walrus():
    # Basic walrus
    if (n := 10) > 5:
        test("walrus_basic", n == 10)
    else:
        test("walrus_basic", False)

    # Walrus in while
    data = [1, 2, 3, 4, 5]
    idx = 0
    total = 0
    while idx < len(data):
        total = total + data[idx]
        idx = idx + 1
    test("walrus_while_prep", total == 15)

    # Walrus in list comprehension
    result = [y for x in range(10) if (y := x * 3) > 15]
    test("walrus_listcomp", result == [18, 21, 24, 27])

    # Walrus nested
    x = 0
    if (x := (y := 5) + 3) > 7:
        test("walrus_nested", x == 8 and y == 5)

test_walrus()

# ── str.format() ──

def test_format():
    test("format_positional", "Hello, {}!".format("world") == "Hello, world!")
    test("format_multiple", "{} + {} = {}".format(1, 2, 3) == "1 + 2 = 3")
    test("format_indexed", "{0} and {1} and {0}".format("a", "b") == "a and b and a")
    test("format_escaped_brace", "{{{}}}".format(42) == "{42}")
    test("format_empty", "no placeholders".format() == "no placeholders")

test_format()

# ── Super chains ──

def test_super_chains():
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

    class Puppy(Dog):
        def __init__(self, name, breed, age):
            super().__init__(name, breed)
            self.age = age
        def speak(self):
            return super().speak() + " (puppy)"

    p = Puppy("Rex", "Lab", 1)
    test("super_chain_init_name", p.name == "Rex")
    test("super_chain_init_breed", p.breed == "Lab")
    test("super_chain_init_age", p.age == 1)
    test("super_chain_speak", p.speak() == "Woof! (puppy)")

    # Deep chain
    class A:
        def val(self):
            return "A"
    class B(A):
        def val(self):
            return "B+" + super().val()
    class C(B):
        def val(self):
            return "C+" + super().val()
    class D(C):
        def val(self):
            return "D+" + super().val()

    test("super_deep_chain", D().val() == "D+C+B+A")

test_super_chains()

# ── Class closures (class referencing itself from methods) ──

def test_class_closures():
    def make_class():
        class Node:
            def __init__(self, val, next_node=None):
                self.val = val
                self.next_node = next_node
            def append(self, val):
                if self.next_node is None:
                    self.next_node = Node(val)
                else:
                    self.next_node.append(val)
            def to_list(self):
                result = [self.val]
                if self.next_node is not None:
                    result = result + self.next_node.to_list()
                return result
        return Node

    Node = make_class()
    head = Node(1)
    head.append(2)
    head.append(3)
    test("class_closure_linked_list", head.to_list() == [1, 2, 3])

    # Class method referencing class for factory
    def make_counter():
        class Counter:
            count = 0
            def __init__(self):
                Counter.count = Counter.count + 1
            def get_count(self):
                return Counter.count
        return Counter

    C = make_counter()
    c1 = C()
    c2 = C()
    c3 = C()
    test("class_closure_shared_state", c3.get_count() == 3)

test_class_closures()

# ── Set and dict equality ──

def test_set_dict_eq():
    test("set_eq", {1, 2, 3} == {3, 2, 1})
    test("set_ne", {1, 2} != {1, 2, 3})
    test("dict_eq", {"a": 1, "b": 2} == {"b": 2, "a": 1})
    test("dict_ne", {"a": 1} != {"a": 2})

    # Set comprehension equality
    s1 = {x*x for x in range(5)}
    test("set_comp_eq", s1 == {0, 1, 4, 9, 16})

    # Dict comprehension equality
    d1 = {x: x*x for x in range(4)}
    test("dict_comp_eq", d1 == {0: 0, 1: 1, 2: 4, 3: 9})

test_set_dict_eq()

# ── MRO and isinstance ──

def test_mro():
    class Base:
        def who(self):
            return "Base"

    class Left(Base):
        def who(self):
            return "Left"

    class Right(Base):
        def who(self):
            return "Right"

    class Child(Left, Right):
        pass

    c = Child()
    test("mro_left_wins", c.who() == "Left")
    test("isinstance_child", isinstance(c, Child))
    test("isinstance_left", isinstance(c, Left))
    test("isinstance_base", isinstance(c, Base))

test_mro()

# ── Advanced dunder methods ──

def test_advanced_dunders():
    class Matrix:
        def __init__(self, data):
            self.data = data
        def __getitem__(self, key):
            return self.data[key]
        def __setitem__(self, key, value):
            self.data[key] = value
        def __len__(self):
            return len(self.data)
        def __contains__(self, item):
            return item in self.data
        def __bool__(self):
            return len(self.data) > 0
        def __str__(self):
            return "Matrix(" + str(self.data) + ")"
        def __repr__(self):
            return "Matrix(data=" + repr(self.data) + ")"

    m = Matrix([1, 2, 3, 4, 5])
    test("dunder_getitem", m[0] == 1)
    test("dunder_getitem_neg", m[-1] == 5)
    m[2] = 99
    test("dunder_setitem", m[2] == 99)
    test("dunder_len_instance", len(m) == 5)
    test("dunder_contains", 99 in m)
    test("dunder_not_contains", 100 not in m)
    test("dunder_bool_true", bool(m) == True)
    test("dunder_str_instance", str(m) == "Matrix([1, 2, 99, 4, 5])")

    empty = Matrix([])
    test("dunder_bool_false_not", not empty)

test_advanced_dunders()

# ── Callable objects ──

def test_callable():
    class Adder:
        def __init__(self, n):
            self.n = n
        def __call__(self, x):
            return self.n + x

    add5 = Adder(5)
    test("callable_basic", add5(10) == 15)
    test("callable_another", add5(0) == 5)

    # Use callable as key function
    class Multiplier:
        def __init__(self, factor):
            self.factor = factor
        def __call__(self, x):
            return x * self.factor

    double = Multiplier(2)
    result = list(map(double, [1, 2, 3]))
    test("callable_map", result == [2, 4, 6])

test_callable()

# ── Custom iterators ──

def test_custom_iterators():
    class Countdown:
        def __init__(self, start):
            self.current = start
        def __iter__(self):
            return self
        def __next__(self):
            if self.current <= 0:
                raise StopIteration
            val = self.current
            self.current = self.current - 1
            return val

    test("custom_iter_list", list(Countdown(5)) == [5, 4, 3, 2, 1])
    test("custom_iter_sum", sum(Countdown(10)) == 55)

    # For loop with custom iterator
    result = []
    for x in Countdown(3):
        result.append(x)
    test("custom_iter_for", result == [3, 2, 1])

test_custom_iterators()

# ── Multiple except types ──

def test_multi_except():
    caught = None
    try:
        x = 1 / 0
    except (ValueError, ZeroDivisionError) as e:
        caught = "zero"
    test("multi_except_zero", caught == "zero")

    caught = None
    try:
        int("abc")
    except (ValueError, TypeError) as e:
        caught = "value"
    test("multi_except_value", caught == "value")

test_multi_except()

# ── Nested comprehensions ──

def test_nested_comp():
    # Flatten
    matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
    flat = [x for row in matrix for x in row]
    test("nested_comp_flatten", flat == [1, 2, 3, 4, 5, 6, 7, 8, 9])

    # Multiply table
    table = [i * j for i in range(1, 4) for j in range(1, 4)]
    test("nested_comp_multiply", table == [1, 2, 3, 2, 4, 6, 3, 6, 9])

    # Filtered nested
    pairs = [(i, j) for i in range(4) for j in range(4) if i != j]
    test("nested_comp_filter", len(pairs) == 12)

test_nested_comp()

# ── Decorator patterns ──

def test_decorators():
    def double_result(func):
        def wrapper(*args):
            return func(*args) * 2
        return wrapper

    @double_result
    def add(a, b):
        return a + b

    test("decorator_basic", add(3, 4) == 14)

    # Stacked decorators
    def add_one(func):
        def wrapper(*args):
            return func(*args) + 1
        return wrapper

    @add_one
    @double_result
    def mul(a, b):
        return a * b

    test("decorator_stacked", mul(3, 4) == 25)  # (3*4)*2 + 1

test_decorators()

# ── Exception chaining ──

def test_exceptions():
    # Re-raise in except
    caught_outer = False
    try:
        try:
            raise ValueError("inner")
        except ValueError:
            raise TypeError("outer")
    except TypeError as e:
        caught_outer = True
    test("except_reraise", caught_outer)

    # Finally always runs
    finally_ran = False
    try:
        x = 1
    finally:
        finally_ran = True
    test("finally_normal", finally_ran)

    finally_ran = False
    try:
        raise ValueError("test")
    except ValueError:
        pass
    finally:
        finally_ran = True
    test("finally_after_except", finally_ran)

test_exceptions()

# ── Generator expressions ──

def test_genexpr():
    total = sum(x*x for x in range(10))
    test("genexpr_sum", total == 285)

    any_big = any(x > 50 for x in range(100))
    test("genexpr_any", any_big)

    all_pos = all(x >= 0 for x in range(10))
    test("genexpr_all", all_pos)

test_genexpr()

# ── Print results ──
print("========================================")
print("Tests: " + str(passed + failed) + " | Passed: " + str(passed) + " | Failed: " + str(failed))
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("SOME TESTS FAILED: " + str(failed))
print("========================================")
