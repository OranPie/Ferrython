# test_cpython_compat96.py - Functional programming
import functools
import operator
import itertools

passed96 = 0
total96 = 0

def check96(desc, got, expected):
    global passed96, total96
    total96 += 1
    if got == expected:
        passed96 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# functools.reduce - sum
check96("reduce sum", functools.reduce(lambda a, b: a + b, [1, 2, 3, 4, 5]), 15)

# functools.reduce - product
check96("reduce product", functools.reduce(lambda a, b: a * b, [1, 2, 3, 4, 5]), 120)

# functools.reduce with initial
check96("reduce with initial", functools.reduce(lambda a, b: a + b, [1, 2, 3], 10), 16)

# functools.reduce single element
check96("reduce single element", functools.reduce(lambda a, b: a + b, [42]), 42)

# functools.reduce empty with initial
check96("reduce empty with initial", functools.reduce(lambda a, b: a + b, [], 99), 99)

# functools.reduce string concat
check96("reduce string concat", functools.reduce(lambda a, b: a + b, ["h", "e", "l", "l", "o"]), "hello")

# functools.reduce max
check96("reduce max", functools.reduce(lambda a, b: a if a > b else b, [3, 1, 4, 1, 5, 9, 2, 6]), 9)

# functools.partial
def add96(a, b):
    return a + b

add5_96 = functools.partial(add96, 5)
check96("partial basic", add5_96(3), 8)
check96("partial different arg", add5_96(10), 15)

# functools.partial with kwargs
def greet96(greeting, name):
    return greeting + " " + name

hello96 = functools.partial(greet96, greeting="Hello")
check96("partial with kwargs", hello96(name="World"), "Hello World")

# functools.partial nested
add10_96 = functools.partial(add5_96, 5)
check96("partial nested (partial of partial)", add10_96(), 10)

# functools.partial preserves func
check96("partial func attribute", add5_96.func, add96)
check96("partial args attribute", add5_96.args, (5,))

# functools.lru_cache
call_count_96 = 0

@functools.lru_cache(maxsize=32)
def fib96(n):
    global call_count_96
    call_count_96 += 1
    if n < 2:
        return n
    return fib96(n - 1) + fib96(n - 2)

check96("lru_cache fib(10)", fib96(10), 55)
check96("lru_cache fib(20)", fib96(20), 6765)
first_count_96 = call_count_96
r96_fib10 = fib96(10)
check96("lru_cache cached result", r96_fib10, 55)
check96("lru_cache no extra calls on cached", call_count_96, first_count_96)

# lru_cache cache_info
info96 = fib96.cache_info()
check96("lru_cache cache_info has hits", info96.hits > 0, True)

# operator module functions
check96("operator.add", operator.add(3, 4), 7)
check96("operator.sub", operator.sub(10, 3), 7)
check96("operator.mul", operator.mul(3, 4), 12)
check96("operator.truediv", operator.truediv(10, 4), 2.5)
check96("operator.floordiv", operator.floordiv(10, 3), 3)
check96("operator.mod", operator.mod(10, 3), 1)
check96("operator.pow", operator.pow(2, 10), 1024)
check96("operator.neg", operator.neg(5), -5)
check96("operator.abs", operator.abs(-5), 5)
check96("operator.eq", operator.eq(3, 3), True)
check96("operator.ne", operator.ne(3, 4), True)
check96("operator.lt", operator.lt(3, 4), True)
check96("operator.le", operator.le(3, 3), True)
check96("operator.gt", operator.gt(4, 3), True)
check96("operator.ge", operator.ge(3, 3), True)
check96("operator.not_", operator.not_(False), True)
check96("operator.and_", operator.and_(0b1100, 0b1010), 0b1000)
check96("operator.or_", operator.or_(0b1100, 0b1010), 0b1110)
check96("operator.xor", operator.xor(0b1100, 0b1010), 0b0110)

# operator.itemgetter
getter96 = operator.itemgetter(1)
check96("operator.itemgetter single", getter96([10, 20, 30]), 20)

getter96_multi = operator.itemgetter(0, 2)
check96("operator.itemgetter multi", getter96_multi([10, 20, 30]), (10, 30))

# operator.attrgetter
class Obj96:
    def __init__(self, x, y):
        self.x = x
        self.y = y

obj96_1 = Obj96(10, 20)
ag96 = operator.attrgetter("x")
check96("operator.attrgetter", ag96(obj96_1), 10)

# itertools.accumulate
check96("accumulate sum", list(itertools.accumulate([1, 2, 3, 4, 5])), [1, 3, 6, 10, 15])
check96("accumulate mul", list(itertools.accumulate([1, 2, 3, 4, 5], operator.mul)), [1, 2, 6, 24, 120])
check96("accumulate max", list(itertools.accumulate([3, 1, 4, 1, 5], max)), [3, 3, 4, 4, 5])
check96("accumulate initial", list(itertools.accumulate([1, 2, 3], initial=10)), [10, 11, 13, 16])
check96("accumulate empty", list(itertools.accumulate([])), [])

# itertools.chain
check96("chain basic", list(itertools.chain([1, 2], [3, 4], [5])), [1, 2, 3, 4, 5])
check96("chain empty", list(itertools.chain([], [], [])), [])
check96("chain strings", list(itertools.chain("ab", "cd")), ["a", "b", "c", "d"])

# itertools.chain.from_iterable
check96("chain.from_iterable", list(itertools.chain.from_iterable([[1, 2], [3, 4]])), [1, 2, 3, 4])

# itertools.groupby
data96 = [("a", 1), ("a", 2), ("b", 3), ("b", 4), ("a", 5)]
grouped96 = [(k, list(g)) for k, g in itertools.groupby(data96, key=lambda x: x[0])]
check96("groupby basic", grouped96, [("a", [("a", 1), ("a", 2)]), ("b", [("b", 3), ("b", 4)]), ("a", [("a", 5)])])

# groupby with sorted data
data96_s = sorted(["banana", "apple", "cherry", "avocado", "blueberry"], key=lambda x: x[0])
grouped96_s = [(k, list(g)) for k, g in itertools.groupby(data96_s, key=lambda x: x[0])]
check96("groupby sorted first char keys", [k for k, g in grouped96_s], ["a", "b", "c"])

# map with multiple iterables
check96("map two iterables", list(map(operator.add, [1, 2, 3], [10, 20, 30])), [11, 22, 33])
check96("map three iterables", list(map(lambda a, b, c: a + b + c, [1, 2], [10, 20], [100, 200])), [111, 222])
check96("map unequal length", list(map(operator.mul, [1, 2, 3], [10, 20])), [10, 40])

# filter
check96("filter basic", list(filter(lambda x: x > 3, [1, 2, 3, 4, 5])), [4, 5])
check96("filter None removes falsy", list(filter(None, [0, 1, "", "a", None, True, False])), [1, "a", True])
check96("filter empty", list(filter(lambda x: x > 10, [1, 2, 3])), [])

# zip
check96("zip basic", list(zip([1, 2, 3], ["a", "b", "c"])), [(1, "a"), (2, "b"), (3, "c")])
check96("zip unequal", list(zip([1, 2], ["a", "b", "c"])), [(1, "a"), (2, "b")])

# sorted with key
check96("sorted with key", sorted(["banana", "apple", "cherry"], key=len), ["apple", "banana", "cherry"])
check96("sorted with key reverse", sorted([3, 1, 4, 1, 5], reverse=True), [5, 4, 3, 1, 1])

# functools.reduce with operator
check96("reduce with operator.add", functools.reduce(operator.add, range(1, 11)), 55)
check96("reduce with operator.mul", functools.reduce(operator.mul, range(1, 6)), 120)

print(f"Tests: {total96} | Passed: {passed96} | Failed: {total96 - passed96}")
