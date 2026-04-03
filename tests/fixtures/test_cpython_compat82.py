# Test 82: Closures, decorators, and functional patterns
import functools

passed82 = 0
total82 = 0

def check82(desc, got, expected):
    global passed82, total82
    total82 += 1
    if got == expected:
        passed82 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- Closure captures variable by reference (late binding) ---
funcs82_1 = []
for i82_1 in range(3):
    funcs82_1.append(lambda: i82_1)
check82("closure late binding 0", funcs82_1[0](), 2)
check82("closure late binding 1", funcs82_1[1](), 2)
check82("closure late binding 2", funcs82_1[2](), 2)

# --- Closure with default arg captures value (early binding) ---
funcs82_2 = []
for i82_2 in range(3):
    funcs82_2.append(lambda x=i82_2: x)
check82("closure early binding 0", funcs82_2[0](), 0)
check82("closure early binding 1", funcs82_2[1](), 1)
check82("closure early binding 2", funcs82_2[2](), 2)

# --- Nested closures (3 levels deep) ---
def outer82_3(x82_3):
    def middle82_3(y82_3):
        def inner82_3(z82_3):
            return x82_3 + y82_3 + z82_3
        return inner82_3
    return middle82_3

check82("nested closure 3 levels", outer82_3(1)(2)(3), 6)
check82("nested closure 3 levels different args", outer82_3(10)(20)(30), 60)

# --- Decorator that wraps a function ---
def double_result82_4(fn):
    def wrapper82_4(*args, **kwargs):
        return fn(*args, **kwargs) * 2
    return wrapper82_4

@double_result82_4
def add_one82_4(x):
    return x + 1

check82("decorator wraps function", add_one82_4(5), 12)

# --- Decorator with arguments ---
def multiply_by82_5(factor):
    def decorator82_5(fn):
        def wrapper82_5(*args, **kwargs):
            return fn(*args, **kwargs) * factor
        return wrapper82_5
    return decorator82_5

@multiply_by82_5(3)
def inc82_5(x):
    return x + 1

check82("decorator with arguments", inc82_5(4), 15)

# --- Decorator preserving function name (functools.wraps) ---
def my_deco82_6(fn):
    @functools.wraps(fn)
    def wrapper82_6(*args, **kwargs):
        return fn(*args, **kwargs)
    return wrapper82_6

@my_deco82_6
def hello82_6():
    return "hi"

check82("functools.wraps preserves __name__", hello82_6.__name__, "hello82_6")
check82("functools.wraps return value", hello82_6(), "hi")

# --- functools.reduce ---
val82_7 = functools.reduce(lambda a, b: a + b, [1, 2, 3, 4, 5])
check82("functools.reduce sum", val82_7, 15)

val82_7b = functools.reduce(lambda a, b: a * b, [1, 2, 3, 4], 1)
check82("functools.reduce product with initial", val82_7b, 24)

val82_7c = functools.reduce(lambda a, b: a + b, ["a", "b", "c"])
check82("functools.reduce string concat", val82_7c, "abc")

# --- functools.partial with kwargs ---
def greet82_8(greeting, name):
    return greeting + " " + name

hi82_8 = functools.partial(greet82_8, "Hello")
check82("functools.partial positional", hi82_8("World"), "Hello World")

hi82_8b = functools.partial(greet82_8, name="Alice")
check82("functools.partial with kwargs", hi82_8b("Hey"), "Hey Alice")

# --- Lambda in list comprehension ---
fns82_9 = [lambda x, i=i: x + i for i in range(4)]
check82("lambda in listcomp 0", fns82_9[0](10), 10)
check82("lambda in listcomp 1", fns82_9[1](10), 11)
check82("lambda in listcomp 3", fns82_9[3](10), 13)

# --- Lambda as sort key ---
data82_10 = [("b", 2), ("a", 3), ("c", 1)]
sorted82_10 = sorted(data82_10, key=lambda t: t[1])
check82("lambda sort key", sorted82_10, [("c", 1), ("b", 2), ("a", 3)])

sorted82_10b = sorted(data82_10, key=lambda t: t[0])
check82("lambda sort key by first elem", sorted82_10b, [("a", 3), ("b", 2), ("c", 1)])

# --- map/filter/reduce chains ---
res82_11 = list(map(lambda x: x * 2, [1, 2, 3, 4]))
check82("map doubles", res82_11, [2, 4, 6, 8])

res82_11b = list(filter(lambda x: x > 2, [1, 2, 3, 4]))
check82("filter > 2", res82_11b, [3, 4])

res82_11c = functools.reduce(lambda a, b: a + b, filter(lambda x: x % 2 == 0, map(lambda x: x * 3, [1, 2, 3, 4])))
check82("map/filter/reduce chain", res82_11c, 18)

res82_11d = list(map(str, [1, 2, 3]))
check82("map with builtin str", res82_11d, ["1", "2", "3"])

res82_11e = list(filter(None, [0, 1, "", "a", None, True]))
check82("filter with None as func", res82_11e, [1, "a", True])

# --- Multiple decorators stacking ---
def add_exclaim82_12(fn):
    @functools.wraps(fn)
    def wrapper(*a, **kw):
        return fn(*a, **kw) + "!"
    return wrapper

def add_greeting82_12(fn):
    @functools.wraps(fn)
    def wrapper(*a, **kw):
        return "Hello " + fn(*a, **kw)
    return wrapper

@add_greeting82_12
@add_exclaim82_12
def name82_12():
    return "World"

check82("multiple decorators stacking", name82_12(), "Hello World!")

def upper_result82_12b(fn):
    @functools.wraps(fn)
    def wrapper(*a, **kw):
        return fn(*a, **kw).upper()
    return wrapper

@upper_result82_12b
@add_exclaim82_12
def say82_12b():
    return "hi"

check82("stacked decorators upper+exclaim", say82_12b(), "HI!")

# --- Closure over loop variable ---
def make_adders82_13():
    adders = []
    for i in range(5):
        def adder(x, _i=i):
            return x + _i
        adders.append(adder)
    return adders

adders82_13 = make_adders82_13()
check82("closure over loop var 0", adders82_13[0](100), 100)
check82("closure over loop var 3", adders82_13[3](100), 103)
check82("closure over loop var 4", adders82_13[4](100), 104)

# --- Additional closure tests ---
def counter82_14():
    count = [0]
    def inc():
        count[0] += 1
        return count[0]
    return inc

c82_14 = counter82_14()
check82("closure counter first call", c82_14(), 1)
check82("closure counter second call", c82_14(), 2)
check82("closure counter third call", c82_14(), 3)

# --- Decorator that caches (memoize) ---
def memoize82_15(fn):
    cache = {}
    @functools.wraps(fn)
    def wrapper(*args):
        if args not in cache:
            cache[args] = fn(*args)
        return cache[args]
    return wrapper

@memoize82_15
def fib82_15(n):
    if n < 2:
        return n
    return fib82_15(n - 1) + fib82_15(n - 2)

check82("memoized fib(10)", fib82_15(10), 55)
check82("memoized fib(0)", fib82_15(0), 0)
check82("memoized fib(1)", fib82_15(1), 1)

print(f"Tests: {total82} | Passed: {passed82} | Failed: {total82 - passed82}")
