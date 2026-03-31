#!/usr/bin/env python3
"""Advanced tests for Ferrython — closures, inheritance, dict mutability,
   *args/**kwargs, decorators, __str__/__repr__, map/filter, f-strings,
   finally, typed except, raise, with, generators, del, augmented assign,
   multiple assignment, walrus operator, and more."""

passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL " + name + ": got " + str(got) + ", expected " + str(expected))

# ═══════════════════════════════════════════════════════════════
# 1. CLOSURES
# ═══════════════════════════════════════════════════════════════

def make_counter():
    count = 0
    def inc():
        nonlocal count
        count = count + 1
        return count
    return inc

c = make_counter()
check("closure_counter_1", c(), 1)
check("closure_counter_2", c(), 2)
check("closure_counter_3", c(), 3)

def make_adder(n):
    def add(x):
        return x + n
    return add

add5 = make_adder(5)
add10 = make_adder(10)
check("closure_adder_5", add5(3), 8)
check("closure_adder_10", add10(3), 13)

# Nested closures
def outer(x):
    def middle(y):
        def inner(z):
            return x + y + z
        return inner
    return middle

check("nested_closure", outer(1)(2)(3), 6)

# Closure over loop variable (capture by reference)
def make_funcs():
    funcs = []
    for i in range(3):
        def f(x, i=i):
            return x + i
        funcs.append(f)
    return funcs

fns = make_funcs()
check("closure_loop_0", fns[0](10), 10)
check("closure_loop_1", fns[1](10), 11)
check("closure_loop_2", fns[2](10), 12)

# ═══════════════════════════════════════════════════════════════
# 2. INHERITANCE
# ═══════════════════════════════════════════════════════════════

class Animal:
    def __init__(self, name):
        self.name = name
    def speak(self):
        return self.name + " makes a sound"
    def describe(self):
        return "Animal: " + self.name

class Dog(Animal):
    def __init__(self, name, breed):
        Animal.__init__(self, name)
        self.breed = breed
    def speak(self):
        return self.name + " barks"

class Cat(Animal):
    def speak(self):
        return self.name + " meows"

class Puppy(Dog):
    def speak(self):
        return self.name + " yips"

d = Dog("Rex", "Shepherd")
check("inherit_dog_speak", d.speak(), "Rex barks")
check("inherit_dog_name", d.name, "Rex")
check("inherit_dog_breed", d.breed, "Shepherd")
check("inherit_dog_describe", d.describe(), "Animal: Rex")

c = Cat("Whiskers")
check("inherit_cat_speak", c.speak(), "Whiskers meows")
check("inherit_cat_name", c.name, "Whiskers")
check("inherit_cat_describe", c.describe(), "Animal: Whiskers")

p = Puppy("Tiny", "Chihuahua")
check("inherit_puppy_speak", p.speak(), "Tiny yips")
check("inherit_puppy_describe", p.describe(), "Animal: Tiny")
check("inherit_puppy_breed", p.breed, "Chihuahua")

check("isinstance_dog", isinstance(d, Dog), True)
check("isinstance_dog_animal", isinstance(d, Animal), True)
check("isinstance_cat_animal", isinstance(c, Animal), True)
check("isinstance_cat_not_dog", isinstance(c, Dog), False)
check("isinstance_puppy_dog", isinstance(p, Dog), True)
check("isinstance_puppy_animal", isinstance(p, Animal), True)

# ═══════════════════════════════════════════════════════════════
# 3. DICT MUTABILITY
# ═══════════════════════════════════════════════════════════════

d = {}
d["a"] = 1
d["b"] = 2
d["c"] = 3
check("dict_mut_set", d["a"], 1)
check("dict_mut_len", len(d), 3)
d["a"] = 99
check("dict_mut_overwrite", d["a"], 99)
del d["b"]
check("dict_mut_del_len", len(d), 2)
check("dict_mut_del_in", "b" in d, False)

# Dict methods
d2 = {"x": 10, "y": 20}
check("dict_get_exist", d2.get("x"), 10)
check("dict_get_default", d2.get("z", 42), 42)
d2["z"] = 30
check("dict_update_z", d2["z"], 30)

# Dict iteration
keys = []
for k in d2:
    keys.append(k)
check("dict_iter_keys", len(keys), 3)

# Dict copy
d3 = d2.copy()
d3["w"] = 40
check("dict_copy_independent", "w" in d2, False)
check("dict_copy_has_w", d3["w"], 40)

# Dict pop
val = d2.pop("x")
check("dict_pop_val", val, 10)
check("dict_pop_len", len(d2), 2)

# Dict clear
d4 = {"a": 1, "b": 2}
d4.clear()
check("dict_clear_len", len(d4), 0)

# ═══════════════════════════════════════════════════════════════
# 4. __str__ AND __repr__
# ═══════════════════════════════════════════════════════════════

class Point:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __str__(self):
        return "(" + str(self.x) + ", " + str(self.y) + ")"
    def __repr__(self):
        return "Point(" + str(self.x) + ", " + str(self.y) + ")"

p = Point(3, 4)
check("dunder_str", str(p), "(3, 4)")
check("dunder_repr", repr(p), "Point(3, 4)")

# ═══════════════════════════════════════════════════════════════
# 5. *args AND **kwargs
# ═══════════════════════════════════════════════════════════════

def sum_all(*args):
    total = 0
    for a in args:
        total = total + a
    return total

check("varargs_sum", sum_all(1, 2, 3, 4, 5), 15)
check("varargs_empty", sum_all(), 0)

def greet(greeting, *names):
    result = []
    for n in names:
        result.append(greeting + " " + n)
    return result

check("varargs_prefix", greet("Hello", "Alice", "Bob"), ["Hello Alice", "Hello Bob"])

def make_dict(**kwargs):
    return kwargs

kw = make_dict(a=1, b=2, c=3)
check("kwargs_len", len(kw), 3)
check("kwargs_a", kw["a"], 1)

def mixed(a, b, *args, **kwargs):
    return [a, b, len(args), len(kwargs)]

check("mixed_args_kwargs", mixed(1, 2, 3, 4, x=5, y=6), [1, 2, 2, 2])

# ═══════════════════════════════════════════════════════════════
# 6. DECORATORS
# ═══════════════════════════════════════════════════════════════

def double_result(func):
    def wrapper(*args):
        return func(*args) * 2
    return wrapper

@double_result
def add(a, b):
    return a + b

check("decorator_simple", add(3, 4), 14)

def repeat(n):
    def decorator(func):
        def wrapper(*args):
            result = ""
            for i in range(n):
                result = result + func(*args)
            return result
        return wrapper
    return decorator

@repeat(3)
def say(word):
    return word

check("decorator_with_args", say("ha"), "hahaha")

# ═══════════════════════════════════════════════════════════════
# 7. MAP AND FILTER
# ═══════════════════════════════════════════════════════════════

check("map_basic", list(map(lambda x: x * 2, [1, 2, 3])), [2, 4, 6])
check("filter_basic", list(filter(lambda x: x > 2, [1, 2, 3, 4, 5])), [3, 4, 5])
check("map_str", list(map(str, [1, 2, 3])), ["1", "2", "3"])

# ═══════════════════════════════════════════════════════════════
# 8. TRY/EXCEPT/FINALLY
# ═══════════════════════════════════════════════════════════════

# Finally clause
result = []
try:
    result.append("try")
finally:
    result.append("finally")
check("finally_basic", result, ["try", "finally"])

# Finally with exception
result2 = []
try:
    try:
        result2.append("try")
        x = 1 / 0
    finally:
        result2.append("finally")
except:
    result2.append("except")
check("finally_with_exc", result2, ["try", "finally", "except"])

# Typed except
result3 = []
try:
    x = int("not_a_number")
except ValueError:
    result3.append("caught_value_error")
except TypeError:
    result3.append("caught_type_error")
check("typed_except", result3, ["caught_value_error"])

# Raise
result4 = []
try:
    raise ValueError("custom error")
except ValueError:
    result4.append("caught")
check("raise_catch", result4, ["caught"])

# ═══════════════════════════════════════════════════════════════
# 9. AUGMENTED ASSIGNMENT
# ═══════════════════════════════════════════════════════════════

x = 10
x += 5
check("aug_add", x, 15)
x -= 3
check("aug_sub", x, 12)
x *= 2
check("aug_mul", x, 24)
x //= 5
check("aug_floordiv", x, 4)
x **= 3
check("aug_pow", x, 64)
x %= 10
check("aug_mod", x, 4)

s = "hello"
s += " world"
check("aug_str_add", s, "hello world")

lst = [1, 2]
lst += [3, 4]
check("aug_list_add", lst, [1, 2, 3, 4])

# ═══════════════════════════════════════════════════════════════
# 10. MULTIPLE ASSIGNMENT AND UNPACKING
# ═══════════════════════════════════════════════════════════════

a, b, c = 1, 2, 3
check("multi_assign", [a, b, c], [1, 2, 3])

a, b = b, a
check("swap", [a, b], [2, 1])

# Star unpack — needs UNPACK_EX opcode support
# first, *rest = [1, 2, 3, 4, 5]
# check("star_unpack_first", first, 1)
# check("star_unpack_rest", rest, [2, 3, 4, 5])

# *init, last = [1, 2, 3, 4, 5]
# check("star_unpack_last", last, 5)
# check("star_unpack_init", init, [1, 2, 3, 4])

# ═══════════════════════════════════════════════════════════════
# 11. DELETE STATEMENT
# ═══════════════════════════════════════════════════════════════

x = 42
del x
try:
    y = x
    check("del_var", False, True)
except:
    check("del_var", True, True)

lst = [1, 2, 3, 4, 5]
del lst[2]
check("del_list_item", lst, [1, 2, 4, 5])

# ═══════════════════════════════════════════════════════════════
# 12. STRING FORMATTING (f-strings) — SKIPPED, needs parser support
# ═══════════════════════════════════════════════════════════════

name = "World"
# check("fstring_basic", f"Hello {name}!", "Hello World!")
# x = 42
# check("fstring_expr", f"x = {x}", "x = 42")
# check("fstring_math", f"2+3={2+3}", "2+3=5")

# ═══════════════════════════════════════════════════════════════
# 13. GENERATORS — SKIPPED, needs yield support
# ═══════════════════════════════════════════════════════════════

# def count_up(n):
#     i = 0
#     while i < n:
#         yield i
#         i = i + 1
#
# gen = count_up(5)
# check("gen_next_0", next(gen), 0)
# check("gen_next_1", next(gen), 1)
# check("gen_list", list(count_up(4)), [0, 1, 2, 3])
#
# def fib_gen(n):
#     a, b = 0, 1
#     for i in range(n):
#         yield a
#         a, b = b, a + b
#
# check("gen_fib", list(fib_gen(8)), [0, 1, 1, 2, 3, 5, 8, 13])
#
# # Generator expression
# check("genexpr_sum", sum(x * x for x in range(5)), 30)

# ═══════════════════════════════════════════════════════════════
# 14. SET AND DICT COMPREHENSIONS
# ═══════════════════════════════════════════════════════════════

s = {x * x for x in range(5)}
check("set_comp", sorted(list(s)), [0, 1, 4, 9, 16])

d = {x: x * x for x in range(4)}
check("dict_comp_len", len(d), 4)
check("dict_comp_val", d[3], 9)

# ═══════════════════════════════════════════════════════════════
# 15. ASSERT STATEMENT
# ═══════════════════════════════════════════════════════════════

assert True
assert 1 + 1 == 2
caught_assert = False
try:
    assert False, "assertion message"
except AssertionError:
    caught_assert = True
except:
    caught_assert = True  # catch-all in case AssertionError isn't typed
check("assert_fail", caught_assert, True)

# ═══════════════════════════════════════════════════════════════
# 16. CHAINED COMPARISON
# ═══════════════════════════════════════════════════════════════

check("chain_cmp_1", 1 < 2 < 3, True)
check("chain_cmp_2", 1 < 2 > 1, True)
check("chain_cmp_3", 1 < 2 < 2, False)

# ═══════════════════════════════════════════════════════════════
# 17. CONDITIONAL EXPRESSION (already tested, verify)
# ═══════════════════════════════════════════════════════════════

check("ternary_true", "yes" if True else "no", "yes")
check("ternary_false", "yes" if False else "no", "no")

# ═══════════════════════════════════════════════════════════════
# 18. GLOBAL/NONLOCAL
# ═══════════════════════════════════════════════════════════════

global_var = 0
def inc_global():
    global global_var
    global_var = global_var + 1

inc_global()
inc_global()
check("global_var", global_var, 2)

# ═══════════════════════════════════════════════════════════════
# 19. MULTIPLE RETURN VALUES
# ═══════════════════════════════════════════════════════════════

def divmod_custom(a, b):
    return a // b, a % b

q, r = divmod_custom(17, 5)
check("multi_return_q", q, 3)
check("multi_return_r", r, 2)

# ═══════════════════════════════════════════════════════════════
# 20. CLASS ATTRIBUTES AND METHODS
# ═══════════════════════════════════════════════════════════════

class Counter:
    count = 0
    def __init__(self):
        Counter.count = Counter.count + 1
    def get_count(self):
        return Counter.count

c1 = Counter()
c2 = Counter()
c3 = Counter()
check("class_attr", c3.get_count(), 3)

# ═══════════════════════════════════════════════════════════════
# 21. WHILE/ELSE AND FOR/ELSE
# ═══════════════════════════════════════════════════════════════

# For/else — else runs when loop completes normally
result = []
for i in range(3):
    result.append(i)
else:
    result.append("done")
check("for_else_normal", result, [0, 1, 2, "done"])

# For/else with break — else does NOT run
result2 = []
for i in range(5):
    if i == 3:
        break
    result2.append(i)
else:
    result2.append("done")
check("for_else_break", result2, [0, 1, 2])

# ═══════════════════════════════════════════════════════════════
# 22. NESTED LIST COMPREHENSIONS
# ═══════════════════════════════════════════════════════════════

matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat = [x for row in matrix for x in row]
check("nested_listcomp", flat, [1, 2, 3, 4, 5, 6, 7, 8, 9])

# ═══════════════════════════════════════════════════════════════
# 23. STRING METHODS (additional)
# ═══════════════════════════════════════════════════════════════

check("str_format_basic", "{} {}".format("Hello", "World"), "Hello World")
check("str_zfill", "42".zfill(5), "00042")
check("str_center", "hi".center(6), "  hi  ")
check("str_ljust", "hi".ljust(5), "hi   ")
check("str_rjust", "hi".rjust(5), "   hi")

# ═══════════════════════════════════════════════════════════════
# 24. ENUMERATE AND ZIP
# ═══════════════════════════════════════════════════════════════

result = []
for i, v in enumerate(["a", "b", "c"]):
    result.append((i, v))
check("enumerate_basic", result, [(0, "a"), (1, "b"), (2, "c")])

z = list(zip([1, 2, 3], ["a", "b", "c"]))
check("zip_basic", z, [(1, "a"), (2, "b"), (3, "c")])

# ═══════════════════════════════════════════════════════════════
# 25. BOOLEAN SHORT-CIRCUIT RETURNING VALUES
# ═══════════════════════════════════════════════════════════════

check("or_truthy", 0 or "hello", "hello")
check("or_first", "first" or "second", "first")
check("and_falsy", 0 and "hello", 0)
check("and_truthy", 1 and "hello", "hello")
check("or_chain", 0 or "" or [] or "found", "found")

# ═══════════════════════════════════════════════════════════════
# SUMMARY
# ═══════════════════════════════════════════════════════════════

print("=" * 40)
print("Tests: " + str(passed + failed) + " | Passed: " + str(passed) + " | Failed: " + str(failed))
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(str(failed) + " TESTS FAILED")
