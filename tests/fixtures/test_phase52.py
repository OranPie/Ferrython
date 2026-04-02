"""Phase 52: Edge cases, error recovery, complex patterns"""

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

# 1. Multiple assignment targets
a = b = c = 10
test("multi assign", a == 10 and b == 10 and c == 10)

# 2. Augmented assignment
x = [1, 2, 3]
x += [4, 5]
test("aug assign list", x == [1, 2, 3, 4, 5])

y = "hello"
y += " world"
test("aug assign str", y == "hello world")

# 3. Complex slicing
lst = list(range(10))
test("slice step", lst[::2] == [0, 2, 4, 6, 8])
test("slice rev", lst[::-1] == [9, 8, 7, 6, 5, 4, 3, 2, 1, 0])
test("slice neg step", lst[8:2:-2] == [8, 6, 4])

# 4. Nested function scoping
def outer():
    x = 10
    def middle():
        y = 20
        def inner():
            return x + y
        return inner()
    return middle()

test("nested scope", outer() == 30)

# 5. Closure variable capture
def make_adders():
    adders = []
    for i in range(5):
        def adder(x, i=i):
            return x + i
        adders.append(adder)
    return adders

adders = make_adders()
test("closure capture", [a(10) for a in adders] == [10, 11, 12, 13, 14])

# 6. Ternary expression chains
def classify(x):
    return "positive" if x > 0 else "negative" if x < 0 else "zero"

test("ternary pos", classify(5) == "positive")
test("ternary neg", classify(-3) == "negative")
test("ternary zero", classify(0) == "zero")

# 7. Complex try/except/else/finally
def safe_divide(a, b):
    result = None
    error = None
    executed_else = False
    executed_finally = False
    try:
        result = a / b
    except ZeroDivisionError as e:
        error = str(e)
    else:
        executed_else = True
    finally:
        executed_finally = True
    return result, error, executed_else, executed_finally

r, e, el, f = safe_divide(10, 2)
test("try success", r == 5.0 and e is None and el and f)

r, e, el, f = safe_divide(10, 0)
test("try error", r is None and e is not None and not el and f)

# 8. Dict as switch/case
def calculator(op, a, b):
    ops = {
        "+": lambda: a + b,
        "-": lambda: a - b,
        "*": lambda: a * b,
        "/": lambda: a / b if b != 0 else float("inf"),
    }
    return ops.get(op, lambda: None)()

test("calc add", calculator("+", 3, 4) == 7)
test("calc mul", calculator("*", 3, 4) == 12)
test("calc div", calculator("/", 10, 3) == 10/3)

# 9. Enumerate with start
test("enum start", list(enumerate(["a", "b", "c"], start=1)) == [(1, "a"), (2, "b"), (3, "c")])

# 10. Complex string operations
text = "Hello, World! How are you?"
words = text.split()
test("split default", len(words) == 5)
test("join", "-".join(words) == "Hello,-World!-How-are-you?")

# 11. Comprehension with walrus
data = [1, -2, 3, -4, 5]
positives = [y for x in data if (y := x * x) > 4]
test("walrus comp", positives == [9, 16, 25])

# 12. Multiple exception handling
def risky(val):
    try:
        if val == "type":
            raise TypeError("type error")
        elif val == "value":
            raise ValueError("value error")
        elif val == "key":
            raise KeyError("key error")
        return "ok"
    except (TypeError, ValueError) as e:
        return f"caught: {type(e).__name__}"
    except KeyError:
        return "key not found"

test("multi except 1", risky("type") == "caught: TypeError")
test("multi except 2", risky("value") == "caught: ValueError")
test("multi except 3", risky("key") == "key not found")
test("multi except 4", risky("none") == "ok")

# 13. Generator comprehension  
gen = (x * x for x in range(5))
test("gencomp type", hasattr(gen, '__next__'))
test("gencomp list", list(gen) == [0, 1, 4, 9, 16])

# 14. Lambda with default args
f = lambda x, y=10: x + y
test("lambda default", f(5) == 15)
test("lambda override", f(5, 20) == 25)

# 15. Comparison operators
test("cmp chain", 1 < 2 < 3 < 4)
test("cmp chain false", not (1 < 2 > 3))
test("cmp in", 3 in [1, 2, 3, 4])
test("cmp not in", 5 not in [1, 2, 3, 4])
test("cmp is none", None is None)
test("cmp is not", 1 is not None)

# 16. Complex data transformation
records = [
    {"name": "Alice", "dept": "Engineering", "salary": 100000},
    {"name": "Bob", "dept": "Engineering", "salary": 120000},
    {"name": "Charlie", "dept": "Marketing", "salary": 90000},
    {"name": "Diana", "dept": "Marketing", "salary": 95000},
]

# Group by department and get average salary
dept_salaries = {}
for r in records:
    dept = r["dept"]
    if dept not in dept_salaries:
        dept_salaries[dept] = []
    dept_salaries[dept].append(r["salary"])

avg_salaries = {dept: sum(sals) // len(sals) for dept, sals in dept_salaries.items()}
test("group avg", avg_salaries == {"Engineering": 110000, "Marketing": 92500})

# 17. Recursive power set
def power_set(s):
    if not s:
        return [[]]
    rest = power_set(s[1:])
    return rest + [[s[0]] + subset for subset in rest]

ps = power_set([1, 2, 3])
test("power set len", len(ps) == 8)
test("power set contains", [1, 2] in ps)

# 18. Complex number simulation
class Complex:
    def __init__(self, real, imag=0):
        self.real = real
        self.imag = imag
    
    def __add__(self, other):
        if isinstance(other, Complex):
            return Complex(self.real + other.real, self.imag + other.imag)
        return Complex(self.real + other, self.imag)
    
    def __mul__(self, other):
        if isinstance(other, Complex):
            return Complex(
                self.real * other.real - self.imag * other.imag,
                self.real * other.imag + self.imag * other.real
            )
        return Complex(self.real * other, self.imag * other)
    
    def __eq__(self, other):
        return self.real == other.real and self.imag == other.imag
    
    def __repr__(self):
        if self.imag >= 0:
            return f"({self.real}+{self.imag}j)"
        return f"({self.real}{self.imag}j)"

c1 = Complex(3, 4)
c2 = Complex(1, -2)
c3 = c1 + c2
test("complex add", c3 == Complex(4, 2))
c4 = c1 * c2  # (3+4j)(1-2j) = 3-6j+4j-8j² = 3-2j+8 = 11+(-2)j
test("complex mul", c4 == Complex(11, -2))

# 19. Binary search
def binary_search(arr, target):
    lo, hi = 0, len(arr) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        if arr[mid] == target:
            return mid
        elif arr[mid] < target:
            lo = mid + 1
        else:
            hi = mid - 1
    return -1

arr = list(range(0, 100, 2))  # [0, 2, 4, ..., 98]
test("bsearch found", binary_search(arr, 50) == 25)
test("bsearch not found", binary_search(arr, 51) == -1)

# 20. Dynamic dispatch table
class Calculator:
    def __init__(self):
        self._ops = {}
    
    def register(self, name, func):
        self._ops[name] = func
        return self
    
    def calc(self, name, *args):
        if name in self._ops:
            return self._ops[name](*args)
        raise ValueError(f"Unknown op: {name}")

calc = Calculator()
calc.register("add", lambda a, b: a + b)
calc.register("pow", lambda a, b: a ** b)
test("dyn dispatch add", calc.calc("add", 3, 4) == 7)
test("dyn dispatch pow", calc.calc("pow", 2, 10) == 1024)

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 52 TESTS PASSED")
