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

# ── Decorator with arguments ──

def repeat(n):
    def decorator(func):
        def wrapper(*args, **kwargs):
            results = []
            for i in range(n):
                results.append(func(*args, **kwargs))
            return results
        return wrapper
    return decorator

@repeat(3)
def greet(name):
    return "hi " + name

test("deco_args", greet("world"), ["hi world", "hi world", "hi world"])

# ── Stacked decorators ──

def bold(func):
    def wrapper():
        return "<b>" + func() + "</b>"
    return wrapper

def italic(func):
    def wrapper():
        return "<i>" + func() + "</i>"
    return wrapper

@bold
@italic
def say_hello():
    return "hello"

test("stacked_deco", say_hello(), "<b><i>hello</i></b>")

# ── Class decorator ──

def add_repr(cls):
    def new_repr(self):
        attrs = []
        for k in sorted(self.__dict__.keys()):
            attrs.append(k + "=" + repr(self.__dict__[k]))
        return cls.__name__ + "(" + ", ".join(attrs) + ")"
    cls.__repr__ = new_repr
    return cls

# Note: class decorators may not work yet, skip for now

# ── Property with computation ──

class Circle:
    def __init__(self, radius):
        self._radius = radius
    
    @property
    def radius(self):
        return self._radius
    
    @radius.setter
    def radius(self, value):
        if value < 0:
            raise ValueError("Radius cannot be negative")
        self._radius = value
    
    @property
    def area(self):
        return 3.14159 * self._radius * self._radius

c = Circle(5)
test("prop_getter", c.radius, 5)
test("prop_computed", round(c.area, 2), 78.54)
c.radius = 10
test("prop_setter", c.radius, 10)
test("prop_computed2", round(c.area, 2), 314.16)

# ── Property validation ──
try:
    c.radius = -1
    test("prop_validation", "no error", "ValueError")
except ValueError:
    test("prop_validation", "ValueError", "ValueError")

# ── Multiple inheritance with super ──

class Loggable:
    def log(self):
        return type(self).__name__ + " logged"

class Serializable:
    def serialize(self):
        return type(self).__name__ + " serialized"

class Entity(Loggable, Serializable):
    pass

e = Entity()
test("multi_inh_log", e.log(), "Entity logged")
test("multi_inh_ser", e.serialize(), "Entity serialized")

# ── Dunder hash and eq for use as dict keys ──

class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __eq__(self, other):
        return isinstance(other, Point) and self.x == other.x and self.y == other.y
    def __hash__(self):
        return hash((self.x, self.y))
    def __repr__(self):
        return "Point(" + str(self.x) + ", " + str(self.y) + ")"

# We can use these in sets/dicts once __hash__ dispatch works
p1 = Point(1, 2)
p2 = Point(1, 2)
p3 = Point(3, 4)
test("dunder_eq", p1 == p2, True)
test("dunder_eq2", p1 == p3, False)

# ── Iterator protocol with for loop ──

class Squares:
    def __init__(self, n):
        self.n = n
        self.i = 0
    def __iter__(self):
        return self
    def __next__(self):
        if self.i >= self.n:
            raise StopIteration
        result = self.i * self.i
        self.i = self.i + 1
        return result

test("custom_iter_for", [x for x in Squares(5)], [0, 1, 4, 9, 16])
test("custom_iter_sum2", sum(Squares(5)), 30)

# ── Generator with yield ──

def countdown(n):
    while n > 0:
        yield n
        n -= 1

test("gen_list", list(countdown(5)), [5, 4, 3, 2, 1])

# ── Generator expression ──
test("genexpr_sum", sum(x*x for x in range(10)), 285)

# ── Nested generators ──
def flatten(lst):
    for item in lst:
        if isinstance(item, list):
            for sub in flatten(item):
                yield sub
        else:
            yield item

test("nested_gen", list(flatten([1, [2, 3], [4, [5, 6]]])), [1, 2, 3, 4, 5, 6])

# ── Context manager protocol ──

class Timer:
    def __init__(self, log):
        self.log = log
    def __enter__(self):
        self.log.append("start")
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.log.append("end")
        return False

log = []
with Timer(log) as t:
    log.append("work")
test("ctx_mgr", log, ["start", "work", "end"])

# ── Context manager suppressing exception ──

class Suppressor:
    def __enter__(self):
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        return True  # Suppress exception

result = "ok"
with Suppressor():
    raise ValueError("should be suppressed")
    result = "bad"
test("ctx_suppress", result, "ok")

# ── try/except/else/finally ──

def complex_try():
    log = []
    try:
        log.append("try")
        x = 10 / 2
    except ZeroDivisionError:
        log.append("except")
    else:
        log.append("else")
    finally:
        log.append("finally")
    return log

test("try_else_finally", complex_try(), ["try", "else", "finally"])

# ── try/except/finally with exception ──

def complex_try2():
    log = []
    try:
        log.append("try")
        x = 10 / 0
    except ZeroDivisionError:
        log.append("except")
    else:
        log.append("else")
    finally:
        log.append("finally")
    return log

test("try_except_finally", complex_try2(), ["try", "except", "finally"])

# ── Multiple exception types ──

def multi_except(x):
    try:
        if x == 0:
            raise ValueError("value")
        elif x == 1:
            raise TypeError("type")
        elif x == 2:
            raise KeyError("key")
        return "ok"
    except (ValueError, TypeError) as e:
        return "VT:" + str(e)
    except KeyError as e:
        return "K:" + str(e)

test("multi_exc0", multi_except(0), "VT:value")
test("multi_exc1", multi_except(1), "VT:type")
test("multi_exc2", multi_except(2), "K:key")
test("multi_exc3", multi_except(3), "ok")

# ── Nested function scoping ──

def make_adder(n):
    def adder(x):
        return x + n
    return adder

add5 = make_adder(5)
add10 = make_adder(10)
test("closure_adder", add5(3), 8)
test("closure_adder2", add10(3), 13)

# ── Closure over mutable state ──

def counter():
    count = [0]  # Use list to allow mutation in nested scope
    def inc():
        count[0] += 1
        return count[0]
    def get():
        return count[0]
    return inc, get

inc, get = counter()
inc()
inc()
inc()
test("closure_mutable", get(), 3)

# ── String formatting ──
test("str_format_int", "x = {:d}".format(42), "x = 42")
test("str_format_float", "pi = {:.2f}".format(3.14159), "pi = 3.14")
test("str_format_pad", ">{:>10}<".format("hello"), ">     hello<")
test("str_format_pad2", ">{:<10}<".format("hello"), ">hello     <")
test("str_format_pad3", ">{:^10}<".format("hello"), ">  hello   <")

# ── f-string basics ──
name = "World"
test("fstring", f"Hello, {name}!", "Hello, World!")
x = 42
test("fstring_expr", f"x = {x * 2}", "x = 84")
test("fstring_fmt", f"{3.14159:.2f}", "3.14")

# ── Dict items/keys/values ──
d = {"a": 1, "b": 2, "c": 3}
test("dict_keys", sorted(d.keys()), ["a", "b", "c"])
test("dict_values", sorted(d.values()), [1, 2, 3])
test("dict_items", sorted(d.items()), [("a", 1), ("b", 2), ("c", 3)])

# ── Unpacking in for loop ──
pairs = [(1, "a"), (2, "b"), (3, "c")]
result = []
for num, letter in pairs:
    result.append(str(num) + letter)
test("unpack_for", result, ["1a", "2b", "3c"])

# ── Star unpacking in assignment ──
first, *rest = [1, 2, 3, 4, 5]
test("star_unpack_first", first, 1)
test("star_unpack_rest", rest, [2, 3, 4, 5])

*init, last = [1, 2, 3, 4, 5]
test("star_unpack_init", init, [1, 2, 3, 4])
test("star_unpack_last", last, 5)

a, *b, c = [1, 2, 3, 4, 5]
test("star_unpack_mid", (a, b, c), (1, [2, 3, 4], 5))

# ── Lambda with map ──
test("lambda_map", list(map(lambda x: x**2, [1, 2, 3, 4])), [1, 4, 9, 16])

# ── Conditional list comprehension ──
test("cond_listcomp", [x if x > 0 else -x for x in [-3, -1, 0, 2, 4]], [3, 1, 0, 2, 4])

# ── Nested dict comprehension ──
test("nested_dictcomp", {k: v for k, v in [("a", 1), ("b", 2)]}, {"a": 1, "b": 2})

# ── Exception hierarchy isinstance ──
try:
    d = {}
    d["missing"]
except KeyError:
    test("keyerror_catch", True, True)

try:
    lst = [1, 2, 3]
    lst[10]
except IndexError:
    test("indexerror_catch", True, True)

# ── Chained string methods  ──
test("chain_methods_str", "  HELLO WORLD  ".strip().lower().split(), ["hello", "world"])

# ── List multiplication ──
test("list_mul", [0] * 5, [0, 0, 0, 0, 0])
test("list_mul2", [1, 2] * 3, [1, 2, 1, 2, 1, 2])

# ── Bool arithmetic ──
test("bool_add", True + True, 2)
test("bool_mul", True * 5, 5)
test("bool_sum", sum([True, False, True, True]), 3)

# ── None comparisons ──
test("none_eq", None == None, True)
test("none_ne", None != 0, True)
test("none_ne2", None != False, True)
test("none_ne3", None != "", True)

# ── String slicing ──
s = "Hello, World!"
test("str_slice", s[7:12], "World")
test("str_slice2", s[:5], "Hello")
test("str_slice3", s[-6:], "World!")
test("str_step", s[::2], "Hlo ol!")

# ── Complex dict operations ──
inventory = {"apple": 5, "banana": 3, "cherry": 8}
total = sum(inventory.values())
test("dict_sum_values", total, 16)
expensive = {k: v for k, v in inventory.items() if v > 4}
test("dict_filter", expensive, {"apple": 5, "cherry": 8})

# ── Ternary in comprehension ──
test("ternary_comp", ["even" if x % 2 == 0 else "odd" for x in range(5)],
     ["even", "odd", "even", "odd", "even"])

print("========================================")
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print("Failed tests:", ", ".join(errors))
print("========================================")
