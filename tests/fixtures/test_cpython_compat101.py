# test_cpython_compat101.py - Generator and iterator protocol
import itertools

passed101 = 0
total101 = 0

def check101(desc, got, expected):
    global passed101, total101
    total101 += 1
    if got == expected:
        passed101 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- Generator basic yield ---
def count_up101(n):
    for i in range(n):
        yield i

check101("generator basic", list(count_up101(5)), [0, 1, 2, 3, 4])
check101("generator empty", list(count_up101(0)), [])

def fibonacci101():
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b

fib101 = fibonacci101()
fib_vals101 = [next(fib101) for _ in range(8)]
check101("generator infinite fib", fib_vals101, [0, 1, 1, 2, 3, 5, 8, 13])

# --- Generator with return value (StopIteration.value) ---
def gen_with_return101():
    yield 1
    yield 2
    return "done"

g101_r = gen_with_return101()
check101("gen return next 1", next(g101_r), 1)
check101("gen return next 2", next(g101_r), 2)
try:
    next(g101_r)
    check101("gen return StopIteration", False, True)
except StopIteration as e:
    check101("gen return StopIteration value", e.value, "done")

# --- Generator send() ---
def accumulator101():
    total = 0
    while True:
        val = yield total
        if val is None:
            break
        total += val

g101_s = accumulator101()
check101("send initial next", next(g101_s), 0)
check101("send 10", g101_s.send(10), 10)
check101("send 20", g101_s.send(20), 30)
check101("send 5", g101_s.send(5), 35)

# --- Generator throw() ---
def gen_throw101():
    try:
        yield 1
        yield 2
    except ValueError:
        yield "caught"

g101_t = gen_throw101()
check101("throw next", next(g101_t), 1)
check101("throw ValueError", g101_t.throw(ValueError), "caught")

# --- Generator close() ---
def gen_close101():
    try:
        yield 1
        yield 2
    except GeneratorExit:
        pass

g101_cl = gen_close101()
check101("close first next", next(g101_cl), 1)
g101_cl.close()
closed101 = True
try:
    next(g101_cl)
    closed101 = False
except StopIteration:
    pass
check101("close stops generator", closed101, True)

# --- yield from basic delegation ---
def inner101():
    yield 1
    yield 2

def outer101():
    yield 0
    yield from inner101()
    yield 3

check101("yield from", list(outer101()), [0, 1, 2, 3])

# --- yield from with iterable ---
def yield_from_list101():
    yield from [10, 20, 30]
    yield from range(3)

check101("yield from list+range", list(yield_from_list101()), [10, 20, 30, 0, 1, 2])

# --- yield from with return value ---
def inner_ret101():
    yield 1
    yield 2
    return "result"

def outer_ret101():
    val = yield from inner_ret101()
    yield val

check101("yield from return value", list(outer_ret101()), [1, 2, "result"])

# --- Generator expressions ---
check101("genexpr sum", sum(x * x for x in range(10)), 285)
check101("genexpr list", list(x * 2 for x in range(5)), [0, 2, 4, 6, 8])
check101("genexpr with cond", list(x for x in range(10) if x % 2 == 0), [0, 2, 4, 6, 8])
check101("genexpr min", min(abs(x - 5) for x in range(10)), 0)
check101("genexpr max", max(x * x for x in range(5)), 16)

# --- iter() with sentinel ---
class CallCounter101:
    def __init__(self):
        self.n = 0
    def __call__(self):
        self.n += 1
        return self.n

cc101 = CallCounter101()
check101("iter sentinel", list(iter(cc101, 4)), [1, 2, 3])

# --- next() with default ---
check101("next with default empty", next(iter([]), "default"), "default")
check101("next with default has value", next(iter([42]), "default"), 42)
check101("next no default", next(iter([99])), 99)

# --- zip() exhaustion ---
z101_a = iter([1, 2, 3])
z101_b = iter(["a", "b"])
z101_result = list(zip(z101_a, z101_b))
check101("zip stops at shortest", z101_result, [(1, "a"), (2, "b")])

# zip with generators
check101("zip with generators", list(zip((x for x in range(3)), (x * 10 for x in range(3)))), [(0, 0), (1, 10), (2, 20)])

# --- map/filter with generators ---
def gen_nums101():
    yield 1
    yield 2
    yield 3

check101("map with generator", list(map(lambda x: x * 2, gen_nums101())), [2, 4, 6])
check101("filter with generator", list(filter(lambda x: x > 1, gen_nums101())), [2, 3])

# --- any() and all() with generators (short-circuit) ---
check101("any generator true", any(x > 3 for x in range(10)), True)
check101("any generator false", any(x > 10 for x in range(10)), False)
check101("any empty", any(x for x in []), False)
check101("all generator true", all(x < 10 for x in range(10)), True)
check101("all generator false", all(x < 5 for x in range(10)), False)
check101("all empty", all(x for x in []), True)

# short-circuit verification
sc101_count = 0
def sc_gen101():
    global sc101_count
    for i in range(100):
        sc101_count += 1
        yield i

sc101_count = 0
any(x > 2 for x in sc_gen101())
check101("any short-circuits", sc101_count < 100, True)

sc101_count = 0
all(x < 2 for x in sc_gen101())
check101("all short-circuits", sc101_count < 100, True)

# --- itertools.chain ---
check101("itertools.chain", list(itertools.chain([1, 2], [3, 4], [5])), [1, 2, 3, 4, 5])
check101("itertools.chain empty", list(itertools.chain([], [])), [])
check101("itertools.chain generators", list(itertools.chain(range(3), range(3, 6))), [0, 1, 2, 3, 4, 5])

# --- itertools.islice ---
check101("islice basic", list(itertools.islice(range(100), 5)), [0, 1, 2, 3, 4])
check101("islice start stop", list(itertools.islice(range(100), 2, 7)), [2, 3, 4, 5, 6])
check101("islice with step", list(itertools.islice(range(100), 0, 10, 3)), [0, 3, 6, 9])
check101("islice of generator", list(itertools.islice(count_up101(10), 3)), [0, 1, 2])

# --- itertools.zip_longest ---
check101("zip_longest basic", list(itertools.zip_longest([1, 2, 3], ["a", "b"])), [(1, "a"), (2, "b"), (3, None)])
check101("zip_longest fillvalue", list(itertools.zip_longest([1, 2], [10], fillvalue=0)), [(1, 10), (2, 0)])
check101("zip_longest equal", list(itertools.zip_longest([1, 2], [3, 4])), [(1, 3), (2, 4)])

print(f"Tests: {total101} | Passed: {passed101} | Failed: {total101 - passed101}")
