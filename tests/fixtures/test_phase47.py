"""Phase 47: Advanced iteration, dataclass-like, abc-like patterns,
   __slots__ simulation, type() dynamic class creation, complex f-strings,
   nested exceptions, while/else, for/else patterns"""

passed = 0
failed = 0
total = 0
def test(name, cond):
    global passed, failed, total
    total += 1
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# 1. for/else pattern
def find_prime(start, end):
    for n in range(start, end):
        for d in range(2, n):
            if n % d == 0:
                break
        else:
            return n
    return None

test("for_else prime", find_prime(10, 20) == 11)

# 2. while/else pattern
def search(lst, target):
    i = 0
    while i < len(lst):
        if lst[i] == target:
            break
        i += 1
    else:
        return -1
    return i

test("while_else found", search([1, 2, 3, 4], 3) == 2)
test("while_else not found", search([1, 2, 3, 4], 5) == -1)

# 3. Dynamic class creation with type()
MyClass = type("MyClass", (), {"x": 10, "greet": lambda self: f"Hello from {self.x}"})
obj = MyClass()
test("type() class", obj.x == 10)
test("type() method", obj.greet() == "Hello from 10")

# 4. Type checking
test("type int", type(42) == int)
test("type str", type("hello") == str)
test("type list", type([]) == list)
test("type dict", type({}) == dict)
test("type bool", type(True) == bool)
test("type float", type(3.14) == float)

# 5. Nested exceptions
log = []
try:
    log.append("outer try")
    try:
        log.append("inner try")
        raise ValueError("inner error")
    except ValueError as e:
        log.append(f"caught: {e}")
        raise TypeError("converted") from e
except TypeError as e:
    log.append(f"outer caught: {e}")
finally:
    log.append("finally")

test("nested except", log == ["outer try", "inner try", "caught: inner error", "outer caught: converted", "finally"])

# 6. Complex f-strings
name = "World"
test("f-string basic", f"Hello, {name}!" == "Hello, World!")
test("f-string expr", f"{2 ** 10}" == "1024")
test("f-string method", f"{'hello'.upper()}" == "HELLO")
test("f-string format", f"{3.14159:.2f}" == "3.14")
test("f-string nested", f"{'yes' if True else 'no'}" == "yes")

# 7. Chained comparisons edge cases
x = 5
test("chain lt lt", 1 < x < 10)
test("chain le le", 5 <= x <= 5)
test("chain eq", x == 5 == 5)

# 8. Advanced unpacking
def unpack_test():
    a, (b, c), d = 1, [2, 3], 4
    return a, b, c, d
test("nested unpack", unpack_test() == (1, 2, 3, 4))

# 9. Reversed iteration
test("reversed list", list(reversed([1, 2, 3])) == [3, 2, 1])
test("reversed range", list(reversed(range(5))) == [4, 3, 2, 1, 0])
test("reversed str", "".join(reversed("hello")) == "olleh")

# 10. Complex list operations
lst = list(range(10))
del lst[5]
test("del item", lst == [0, 1, 2, 3, 4, 6, 7, 8, 9])

lst2 = list(range(10))
del lst2[2:5]
test("del slice", lst2 == [0, 1, 5, 6, 7, 8, 9])

# 11. Multiple except clauses
def safe_convert(val):
    try:
        return int(val)
    except ValueError:
        return "value_error"
    except TypeError:
        return "type_error"

test("except clause 1", safe_convert("42") == 42)
test("except clause 2", safe_convert("abc") == "value_error")
test("except clause 3", safe_convert(None) == "type_error")

# 12. Itertools-like patterns
from itertools import chain, repeat, count

# chain
test("chain", list(chain([1, 2], [3, 4], [5])) == [1, 2, 3, 4, 5])

# repeat with limit
test("repeat", list(repeat("x", 3)) == ["x", "x", "x"])

# 13. Collections patterns
from collections import deque

d = deque([1, 2, 3])
d.append(4)
d.appendleft(0)
test("deque append", list(d) == [0, 1, 2, 3, 4])
test("deque pop", d.pop() == 4)
test("deque popleft", d.popleft() == 0)

# 14. String methods advanced
test("str splitlines", "line1\nline2\nline3".splitlines() == ["line1", "line2", "line3"])
test("str expandtabs", "a\tb\tc".expandtabs(4) == "a   b   c")
test("str maketrans", "hello".translate(str.maketrans("helo", "HELO")) == "HELLO")

# 15. Complex closures
def make_ops():
    ops = []
    for op_name, op_func in [("add", lambda a, b: a + b), 
                               ("mul", lambda a, b: a * b)]:
        ops.append((op_name, op_func))
    return ops

ops = make_ops()
test("closure ops add", ops[0][1](3, 4) == 7)
test("closure ops mul", ops[1][1](3, 4) == 12)

# 16. Generator expression
gen_sum = sum(x * x for x in range(10))
test("genexp sum", gen_sum == 285)

gen_max = max(len(w) for w in ["hello", "world", "hi"])
test("genexp max", gen_max == 5)

# 17. Dict comprehension from other dict
prices = {"apple": 1.0, "banana": 0.5, "cherry": 2.0}
expensive = {k: v for k, v in prices.items() if v >= 1.0}
test("dict comp filter", expensive == {"apple": 1.0, "cherry": 2.0})

# 18. Set comprehension
squares_set = {x * x for x in range(-5, 6)}
test("set comp", len(squares_set) == 6)  # 0, 1, 4, 9, 16, 25

# 19. Conditional import pattern
try:
    import json
    has_json = True
except ImportError:
    has_json = False
test("conditional import", has_json)

# 20. Assert statement
try:
    assert True, "should not fail"
    test("assert pass", True)
except AssertionError:
    test("assert pass", False)

try:
    assert False, "expected failure"
    test("assert fail", False)
except AssertionError as e:
    test("assert fail", str(e) == "expected failure")

# 21. Complex class patterns
class Meta:
    registry = []
    
    @classmethod
    def register(cls, klass):
        cls.registry.append(klass.__name__)
        return klass

@Meta.register
class Plugin1:
    pass

@Meta.register  
class Plugin2:
    pass

test("class registry", Meta.registry == ["Plugin1", "Plugin2"])

# 22. Property as class method
class Circle:
    def __init__(self, radius):
        self.radius = radius
    
    @property
    def area(self):
        return 3.14159 * self.radius ** 2
    
    @property
    def diameter(self):
        return self.radius * 2

c = Circle(5)
test("circle area", abs(c.area - 78.53975) < 0.001)
test("circle diameter", c.diameter == 10)

# 23. Recursive generator
def tree_iter(data):
    if isinstance(data, list):
        for item in data:
            yield from tree_iter(item)
    else:
        yield data

tree = [1, [2, [3, 4]], [5, 6]]
test("recursive gen", list(tree_iter(tree)) == [1, 2, 3, 4, 5, 6])

# 24. Multiple inheritance with super()
class Loggable:
    def log(self):
        return "Loggable"

class Serializable:
    def serialize(self):
        return "Serializable"

class Model(Loggable, Serializable):
    def describe(self):
        return f"{self.log()} and {self.serialize()}"

m = Model()
test("multi inherit", m.describe() == "Loggable and Serializable")

# 25. Callable objects
class Adder:
    def __init__(self, n):
        self.n = n
    def __call__(self, x):
        return self.n + x

add5 = Adder(5)
test("callable obj", add5(10) == 15)
test("callable map", list(map(add5, [1, 2, 3])) == [6, 7, 8])

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 47 TESTS PASSED")
