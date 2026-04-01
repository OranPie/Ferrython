passed = 0
failed = 0
def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name, "| got:", repr(got), "| expected:", repr(expected))

# ── itertools ──
import itertools

# chain
test("chain_basic", list(itertools.chain([1, 2], [3, 4], [5])), [1, 2, 3, 4, 5])
test("chain_empty", list(itertools.chain([], [1, 2])), [1, 2])

# repeat
test("repeat_n", list(itertools.repeat("x", 3)), ["x", "x", "x"])
test("repeat_0", list(itertools.repeat("x", 0)), [])

# islice
test("islice_basic", list(itertools.islice([0, 1, 2, 3, 4, 5], 3)), [0, 1, 2])
test("islice_start_stop", list(itertools.islice([0, 1, 2, 3, 4, 5], 2, 5)), [2, 3, 4])

# product
test("product_basic", list(itertools.product([1, 2], [3, 4])),
     [(1, 3), (1, 4), (2, 3), (2, 4)])

# zip_longest
test("zip_longest", list(itertools.zip_longest([1, 2, 3], [4, 5])),
     [(1, 4), (2, 5), (3, None)])

# ── collections ──
import collections

# Counter
c = collections.Counter([1, 1, 2, 2, 2, 3])
test("counter_basic", c[1], 2)
test("counter_basic2", c[2], 3)
test("counter_basic3", c[3], 1)

# Counter from string
c2 = collections.Counter("abracadabra")
test("counter_str", c2["a"], 5)
test("counter_str2", c2["b"], 2)

# deque
d = collections.deque([1, 2, 3])
test("deque_basic", list(d), [1, 2, 3])

# OrderedDict
od = collections.OrderedDict()
test("ordereddict_empty", len(od), 0)

# ── functools ──
import functools

# reduce is stubbed, skip for now

# ── single-line function bodies ──
def square(x): return x * x
test("inline_fn", square(5), 25)

def identity(x): return x
test("inline_identity", identity("hello"), "hello")

class Simple: x = 42
test("inline_class", Simple.x, 42)

# ── more advanced patterns ──

# Fibonacci generator
def fib():
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b

g = fib()
fibs = []
for i in range(10):
    fibs.append(next(g))
test("fib_gen", fibs, [0, 1, 1, 2, 3, 5, 8, 13, 21, 34])

# Recursive factorial
def factorial(n):
    if n <= 1: return 1
    return n * factorial(n - 1)

test("factorial_5", factorial(5), 120)
test("factorial_10", factorial(10), 3628800)

# Recursive fibonacci (memoized with dict)
cache = {}
def fib_memo(n):
    if n in cache:
        return cache[n]
    if n <= 1:
        return n
    result = fib_memo(n - 1) + fib_memo(n - 2)
    cache[n] = result
    return result

test("fib_memo_10", fib_memo(10), 55)
test("fib_memo_20", fib_memo(20), 6765)

# Class with class variables and instance variables
class Account:
    interest_rate = 0.05

    def __init__(self, owner, balance):
        self.owner = owner
        self.balance = balance

    def deposit(self, amount):
        self.balance += amount
        return self.balance

    def withdraw(self, amount):
        if amount > self.balance:
            raise ValueError("Insufficient funds")
        self.balance -= amount
        return self.balance

    def get_interest(self):
        return self.balance * Account.interest_rate

acc = Account("Alice", 1000)
test("account_deposit", acc.deposit(500), 1500)
test("account_withdraw", acc.withdraw(200), 1300)
test("account_interest", acc.get_interest(), 65.0)
test("account_owner", acc.owner, "Alice")

# Exception hierarchy
class AppError(Exception):
    def __init__(self, code, message):
        self.message = message
        self.code = code
    def __str__(self):
        return self.message

try:
    raise AppError(404, "Not Found")
except AppError as e:
    test("custom_exc_msg", str(e), "Not Found")
    test("custom_exc_code", e.code, 404)

# Nested list operations
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
transposed = [[row[i] for row in matrix] for i in range(3)]
test("transpose", transposed, [[1, 4, 7], [2, 5, 8], [3, 6, 9]])

# String processing
text = "hello world, hello python, hello rust"
words = text.split()
unique = list(set(words))
test("word_count", len(words), 6)
test("unique_has_hello", "hello" in unique, True)

# List sorting with custom key
data = [(3, "c"), (1, "a"), (2, "b")]
data.sort()
test("sort_tuples", data, [(1, "a"), (2, "b"), (3, "c")])

# Dictionary manipulation
inventory = {"apple": 5, "banana": 3, "cherry": 8}
total = sum(inventory.values())
test("dict_sum_values", total, 16)

# Check sorted returns new list
original = [3, 1, 4, 1, 5]
result = sorted(original)
test("sorted_new_list", result, [1, 1, 3, 4, 5])
test("sorted_original", original, [3, 1, 4, 1, 5])

# Multiple unpacking
a, b, *c = range(5)
test("unpack_range_a", a, 0)
test("unpack_range_b", b, 1)
test("unpack_range_c", c, [2, 3, 4])

# ── exec with multiple statements ──
exec("a = 10\nb = 20\nc = a + b")
test("exec_multi", c, 30)

# ── eval with complex expressions ──
test("eval_complex", eval("[x**2 for x in range(5)]"), [0, 1, 4, 9, 16])

# ── more generator patterns ──
def take(n, gen):
    result = []
    for _ in range(n):
        result.append(next(gen))
    return result

def naturals():
    n = 1
    while True:
        yield n
        n += 1

test("gen_take", take(5, naturals()), [1, 2, 3, 4, 5])

# Generator with filter
def evens():
    n = 0
    while True:
        yield n
        n += 2

test("gen_evens", take(5, evens()), [0, 2, 4, 6, 8])

# ── with statement resource tracking ──
class TrackingResource:
    log = []
    def __init__(self, name):
        self.name = name
    def __enter__(self):
        TrackingResource.log.append("enter:" + self.name)
        return self
    def __exit__(self, *args):
        TrackingResource.log.append("exit:" + self.name)
        return False

with TrackingResource("A") as a:
    TrackingResource.log.append("use:A")

test("ctx_tracking", TrackingResource.log, ["enter:A", "use:A", "exit:A"])

# ── boolean short-circuit ──
log = []
def side_effect(val, name):
    log.append(name)
    return val

result = side_effect(True, "a") or side_effect(True, "b")
test("short_circuit_or", log, ["a"])  # b should not be evaluated

log = []
result = side_effect(False, "a") and side_effect(True, "b")
test("short_circuit_and", log, ["a"])  # b should not be evaluated

log = []
result = side_effect(True, "a") and side_effect(True, "b")
test("short_circuit_and2", log, ["a", "b"])

print("=" * 40)
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed == 0:
    print("ALL TESTS PASSED!")
print("=" * 40)
