# Phase 29: contextmanager, defaultdict, lru_cache, more real-world patterns
tests_passed = 0
tests_failed = 0
def test(name, got, expected):
    global tests_passed, tests_failed
    if got == expected:
        tests_passed += 1
    else:
        tests_failed += 1
        print(f"FAIL: {name}: got {got!r}, expected {expected!r}")

# ── contextmanager ──
from contextlib import contextmanager

@contextmanager
def temp_val():
    yield 42

with temp_val() as v:
    test("cm_basic_yield", v, 42)

# contextmanager with setup/teardown
log = []
@contextmanager
def tracked():
    log.append("enter")
    yield "resource"
    log.append("exit")

with tracked() as r:
    log.append("body")
    test("cm_resource", r, "resource")
test("cm_log", log, ["enter", "body", "exit"])

# ── defaultdict ──
from collections import defaultdict

dd = defaultdict(list)
dd["a"].append(1)
dd["a"].append(2)
dd["b"].append(3)
test("dd_a", dd["a"], [1, 2])
test("dd_b", dd["b"], [3])
test("dd_len", len(dd), 2)
test("dd_keys", sorted(dd.keys()), ["a", "b"])

dd2 = defaultdict(int)
dd2["x"] += 10
dd2["y"] += 20
test("dd_int_x", dd2["x"], 10)
test("dd_int_y", dd2["y"], 20)

# ── lru_cache ──
from functools import lru_cache

@lru_cache(maxsize=128)
def fib(n):
    if n < 2: return n
    return fib(n-1) + fib(n-2)

test("lru_fib10", fib(10), 55)
test("lru_fib20", fib(20), 6765)

# ── typing module ──
from typing import List, Dict, Optional, Tuple, Set, Any
test("typing_imported", True, True)

# ── eval/exec ──
test("eval_basic", eval("2 + 3"), 5)
test("eval_expr", eval("'hello' + ' world'"), "hello world")

exec("_exec_var = 42")
test("exec_basic", _exec_var, 42)

# ── Multiple inheritance MRO ──
class A:
    def who(self): return "A"
class B(A):
    def who(self): return "B+" + super().who()
class C(A):
    def who(self): return "C+" + super().who()
class D(B, C):
    def who(self): return "D+" + super().who()

test("mro_diamond", D().who(), "D+B+C+A")

# ── Star unpacking ──
first, *rest = [1, 2, 3, 4, 5]
test("star_first", first, 1)
test("star_rest", rest, [2, 3, 4, 5])

*init, last = [1, 2, 3, 4, 5]
test("star_init", init, [1, 2, 3, 4])
test("star_last", last, 5)

# ── Dict merge ──
d1 = {"a": 1, "b": 2}
d2 = {"b": 3, "c": 4}
merged = {**d1, **d2}
test("dict_merge", merged, {"a": 1, "b": 3, "c": 4})

# ── Ternary in comprehension ──
result = [x if x > 0 else -x for x in [-3, -1, 0, 1, 3]]
test("ternary_comp", result, [3, 1, 0, 1, 3])

# ── Walrus operator ──
if (n := 10) > 5:
    test("walrus_value", n, 10)

# ── Multiple except clauses ──
caught = None
try:
    raise ValueError("test")
except TypeError:
    caught = "type"
except ValueError as e:
    caught = str(e)
except Exception:
    caught = "other"
test("multi_except", caught, "test")

# ── Global and nonlocal ──
counter = 0
def increment():
    global counter
    counter += 1

increment()
increment()
increment()
test("global_counter", counter, 3)

def make_counter():
    count = 0
    def inc():
        nonlocal count
        count += 1
        return count
    return inc

c = make_counter()
test("closure_1", c(), 1)
test("closure_2", c(), 2)
test("closure_3", c(), 3)

# ── Chain from itertools ──
from itertools import chain
test("chain", list(chain([1, 2], [3], [4, 5])), [1, 2, 3, 4, 5])

# ── functools.partial ──
from functools import partial
def add(a, b):
    return a + b

add5 = partial(add, 5)
test("partial", add5(3), 8)

# ── functools.reduce ──
from functools import reduce
test("reduce_sum", reduce(lambda a, b: a + b, [1, 2, 3, 4]), 10)
test("reduce_init", reduce(lambda a, b: a + b, [1, 2, 3], 100), 106)

# ── enumerate with start ──
pairs = list(enumerate(["a", "b", "c"], start=1))
test("enum_start", pairs, [(1, "a"), (2, "b"), (3, "c")])

# ── zip ──
test("zip_basic", list(zip([1, 2, 3], ["a", "b", "c"])), [(1, "a"), (2, "b"), (3, "c")])

# ── sorted with key ──
words = ["banana", "apple", "cherry"]
test("sorted_key", sorted(words, key=len), ["apple", "banana", "cherry"])

# ── any/all ──
test("all_true", all([True, True, True]), True)
test("all_false", all([True, False, True]), False)
test("any_true", any([False, True, False]), True)
test("any_false", any([False, False, False]), False)

# ── map/filter ──
test("map_sq", list(map(lambda x: x**2, [1, 2, 3])), [1, 4, 9])
test("filter_even", list(filter(lambda x: x % 2 == 0, [1, 2, 3, 4])), [2, 4])

print(f"Tests: {tests_passed + tests_failed} | Passed: {tests_passed} | Failed: {tests_failed}")
if tests_failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(f"{tests_failed} TESTS FAILED!")
