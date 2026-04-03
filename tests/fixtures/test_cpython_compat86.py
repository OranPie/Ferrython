# Test 86: Advanced iteration patterns
import itertools

passed86 = 0
total86 = 0

def check86(desc, got, expected):
    global passed86, total86
    total86 += 1
    if got == expected:
        passed86 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- iter() with sentinel ---
class Counter86_1:
    def __init__(self):
        self.n = 0
    def __call__(self):
        self.n += 1
        return self.n

res86_1 = list(iter(Counter86_1(), 5))
check86("iter with sentinel", res86_1, [1, 2, 3, 4])

# --- next() with default ---
it86_2 = iter([10, 20])
check86("next() first", next(it86_2), 10)
check86("next() second", next(it86_2), 20)
check86("next() with default", next(it86_2, "done"), "done")

it86_2b = iter([])
check86("next() empty with default", next(it86_2b, None), None)

# --- StopIteration from exhausted iterator ---
it86_3 = iter([1])
next(it86_3)
try:
    next(it86_3)
    check86("StopIteration raised", False, True)
except StopIteration:
    check86("StopIteration raised", True, True)

# --- reversed() on list ---
res86_4 = list(reversed([1, 2, 3, 4]))
check86("reversed list", res86_4, [4, 3, 2, 1])
check86("reversed empty", list(reversed([])), [])

# --- reversed() on range ---
res86_5 = list(reversed(range(5)))
check86("reversed range(5)", res86_5, [4, 3, 2, 1, 0])
res86_5b = list(reversed(range(2, 8)))
check86("reversed range(2,8)", res86_5b, [7, 6, 5, 4, 3, 2])

# --- enumerate() with start ---
res86_6 = list(enumerate(["a", "b", "c"]))
check86("enumerate default start", res86_6, [(0, "a"), (1, "b"), (2, "c")])
res86_6b = list(enumerate(["x", "y"], start=5))
check86("enumerate start=5", res86_6b, [(5, "x"), (6, "y")])
res86_6c = list(enumerate([]))
check86("enumerate empty", res86_6c, [])

# --- zip() with unequal lengths ---
res86_7 = list(zip([1, 2, 3], ["a", "b"]))
check86("zip unequal lengths", res86_7, [(1, "a"), (2, "b")])
res86_7b = list(zip([], [1, 2]))
check86("zip with empty", res86_7b, [])
res86_7c = list(zip([1, 2], [3, 4], [5, 6]))
check86("zip three iterables", res86_7c, [(1, 3, 5), (2, 4, 6)])

# --- itertools.chain ---
res86_8 = list(itertools.chain([1, 2], [3, 4], [5]))
check86("itertools.chain", res86_8, [1, 2, 3, 4, 5])
res86_8b = list(itertools.chain([], [1], []))
check86("itertools.chain with empties", res86_8b, [1])
res86_8c = list(itertools.chain.from_iterable([[1, 2], [3, 4]]))
check86("chain.from_iterable", res86_8c, [1, 2, 3, 4])

# --- itertools.repeat with times ---
res86_9 = list(itertools.repeat("x", 4))
check86("itertools.repeat", res86_9, ["x", "x", "x", "x"])
res86_9b = list(itertools.repeat(0, 0))
check86("itertools.repeat zero times", res86_9b, [])

# --- itertools.islice ---
res86_10 = list(itertools.islice(range(100), 5))
check86("islice first 5", res86_10, [0, 1, 2, 3, 4])
res86_10b = list(itertools.islice(range(100), 2, 6))
check86("islice start=2 stop=6", res86_10b, [2, 3, 4, 5])
res86_10c = list(itertools.islice(range(100), 0, 10, 3))
check86("islice with step", res86_10c, [0, 3, 6, 9])

# --- itertools.takewhile ---
res86_11 = list(itertools.takewhile(lambda x: x < 5, [1, 3, 5, 2, 4]))
check86("takewhile < 5", res86_11, [1, 3])
res86_11b = list(itertools.takewhile(lambda x: x > 0, [3, 2, 1, 0, 4]))
check86("takewhile > 0", res86_11b, [3, 2, 1])

# --- itertools.dropwhile ---
res86_12 = list(itertools.dropwhile(lambda x: x < 5, [1, 3, 5, 2, 4]))
check86("dropwhile < 5", res86_12, [5, 2, 4])
res86_12b = list(itertools.dropwhile(lambda x: x > 0, [3, 2, 0, 1]))
check86("dropwhile > 0", res86_12b, [0, 1])

# --- itertools.accumulate ---
res86_13 = list(itertools.accumulate([1, 2, 3, 4, 5]))
check86("accumulate sum", res86_13, [1, 3, 6, 10, 15])
res86_13b = list(itertools.accumulate([1, 2, 3, 4], lambda a, b: a * b))
check86("accumulate product", res86_13b, [1, 2, 6, 24])
res86_13c = list(itertools.accumulate([]))
check86("accumulate empty", res86_13c, [])

# --- itertools.groupby ---
data86_14 = [("a", 1), ("a", 2), ("b", 3), ("b", 4), ("a", 5)]
groups86_14 = []
for k, g in itertools.groupby(data86_14, key=lambda x: x[0]):
    groups86_14.append((k, list(g)))
check86("groupby keys", [g[0] for g in groups86_14], ["a", "b", "a"])
check86("groupby first group", groups86_14[0][1], [("a", 1), ("a", 2)])
check86("groupby second group", groups86_14[1][1], [("b", 3), ("b", 4)])

sorted86_14 = sorted([3, 1, 1, 2, 2, 2, 3])
count86_14 = []
for k, g in itertools.groupby(sorted86_14):
    count86_14.append((k, len(list(g))))
check86("groupby counting", count86_14, [(1, 2), (2, 3), (3, 2)])

# --- Generator expression vs list comprehension ---
gen86_15 = (x * 2 for x in range(5))
lst86_15 = [x * 2 for x in range(5)]
check86("list comp type", type(lst86_15), list)
check86("gen expr is generator", hasattr(gen86_15, "__next__"), True)
check86("gen expr to list", list(gen86_15), [0, 2, 4, 6, 8])
check86("list comp value", lst86_15, [0, 2, 4, 6, 8])

# --- Multiple iterations over list vs generator ---
lst86_16 = [1, 2, 3]
first86_16 = list(lst86_16)
second86_16 = list(lst86_16)
check86("list iterates multiple times", first86_16 == second86_16, True)

gen86_16 = (x for x in [1, 2, 3])
first86_16g = list(gen86_16)
second86_16g = list(gen86_16)
check86("generator first iteration", first86_16g, [1, 2, 3])
check86("generator exhausted second", second86_16g, [])

# --- Unpacking iterators ---
a86_17, b86_17, c86_17 = iter([10, 20, 30])
check86("unpack iter a", a86_17, 10)
check86("unpack iter b", b86_17, 20)
check86("unpack iter c", c86_17, 30)

first86_17, *rest86_17 = iter([1, 2, 3, 4, 5])
check86("unpack star first", first86_17, 1)
check86("unpack star rest", rest86_17, [2, 3, 4, 5])

print(f"Tests: {total86} | Passed: {passed86} | Failed: {total86 - passed86}")
