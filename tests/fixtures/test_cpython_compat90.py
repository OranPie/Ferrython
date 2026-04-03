## test_cpython_compat90.py - More itertools and iteration (~40 tests)
import itertools

passed90 = 0
total90 = 0

def check90(desc, got, expected):
    global passed90, total90
    total90 += 1
    if got == expected:
        passed90 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- itertools.chain.from_iterable ---
r1 = list(itertools.chain.from_iterable([[1, 2], [3, 4], [5]]))
check90("chain.from_iterable lists", r1, [1, 2, 3, 4, 5])

r2 = list(itertools.chain.from_iterable(["ab", "cd", "ef"]))
check90("chain.from_iterable strings", r2, ["a", "b", "c", "d", "e", "f"])

r3 = list(itertools.chain.from_iterable([[], [1], [], [2, 3]]))
check90("chain.from_iterable with empty", r3, [1, 2, 3])

r4 = list(itertools.chain.from_iterable([]))
check90("chain.from_iterable empty outer", r4, [])

# --- itertools.chain ---
r5 = list(itertools.chain([1, 2], [3], [4, 5, 6]))
check90("chain basic", r5, [1, 2, 3, 4, 5, 6])

r6 = list(itertools.chain("ab", "cd"))
check90("chain strings", r6, ["a", "b", "c", "d"])

# --- zip_longest ---
r7 = list(itertools.zip_longest([1, 2, 3], [4, 5]))
check90("zip_longest default fill", r7, [(1, 4), (2, 5), (3, None)])

r8 = list(itertools.zip_longest([1, 2], [3, 4, 5], fillvalue=0))
check90("zip_longest custom fill", r8, [(1, 3), (2, 4), (0, 5)])

r9 = list(itertools.zip_longest([], [1, 2], fillvalue=-1))
check90("zip_longest one empty", r9, [(-1, 1), (-1, 2)])

r10 = list(itertools.zip_longest([1], [2], [3]))
check90("zip_longest three same len", r10, [(1, 2, 3)])

r11 = list(itertools.zip_longest())
check90("zip_longest no args", r11, [])

# --- enumerate with start ---
r12 = list(enumerate(["a", "b", "c"]))
check90("enumerate default start", r12, [(0, "a"), (1, "b"), (2, "c")])

r13 = list(enumerate(["a", "b", "c"], start=1))
check90("enumerate start=1", r13, [(1, "a"), (2, "b"), (3, "c")])

r14 = list(enumerate(["x"], start=10))
check90("enumerate start=10", r14, [(10, "x")])

r15 = list(enumerate([]))
check90("enumerate empty", r15, [])

r16 = list(enumerate("ab", 5))
check90("enumerate string start=5", r16, [(5, "a"), (6, "b")])

# --- reversed on list ---
r17 = list(reversed([1, 2, 3, 4]))
check90("reversed list", r17, [4, 3, 2, 1])

r18 = list(reversed([]))
check90("reversed empty list", r18, [])

r19 = list(reversed([42]))
check90("reversed single element", r19, [42])

# --- reversed on tuple ---
r20 = list(reversed((1, 2, 3)))
check90("reversed tuple", r20, [3, 2, 1])

# --- reversed on range ---
r21 = list(reversed(range(5)))
check90("reversed range(5)", r21, [4, 3, 2, 1, 0])

r22 = list(reversed(range(1, 4)))
check90("reversed range(1,4)", r22, [3, 2, 1])

r23 = list(reversed(range(0, 10, 2)))
check90("reversed range step 2", r23, [8, 6, 4, 2, 0])

# --- iter with sentinel ---
class Counter90:
    def __init__(self, limit):
        self.n = 0
        self.limit = limit
    def __call__(self):
        self.n += 1
        return self.n

r24 = list(iter(Counter90(10), 5))
check90("iter sentinel stops at 5", r24, [1, 2, 3, 4])

r25 = list(iter(Counter90(10), 1))
check90("iter sentinel stops immediately", r25, [])

# --- map basic ---
r26 = list(map(lambda x: x * 2, [1, 2, 3]))
check90("map double", r26, [2, 4, 6])

r27 = list(map(str, [1, 2, 3]))
check90("map str", r27, ["1", "2", "3"])

r28 = list(map(lambda a, b: a + b, [1, 2, 3], [10, 20, 30]))
check90("map two iterables", r28, [11, 22, 33])

r29 = list(map(lambda x: x.upper(), ["a", "b", "c"]))
check90("map upper", r29, ["A", "B", "C"])

# --- filter basic ---
r30 = list(filter(lambda x: x > 2, [1, 2, 3, 4, 5]))
check90("filter gt 2", r30, [3, 4, 5])

r31 = list(filter(lambda x: x % 2 == 0, range(10)))
check90("filter even", r31, [0, 2, 4, 6, 8])

r32 = list(filter(None, [0, 1, "", "a", None, True, False]))
check90("filter None removes falsy", r32, [1, "a", True])

r33 = list(filter(lambda x: x, []))
check90("filter empty", r33, [])

# --- itertools.islice ---
r34 = list(itertools.islice(range(100), 5))
check90("islice first 5", r34, [0, 1, 2, 3, 4])

r35 = list(itertools.islice(range(100), 2, 6))
check90("islice 2 to 6", r35, [2, 3, 4, 5])

r36 = list(itertools.islice(range(100), 0, 10, 3))
check90("islice step 3", r36, [0, 3, 6, 9])

# --- itertools.repeat ---
r37 = list(itertools.repeat("x", 4))
check90("repeat x 4 times", r37, ["x", "x", "x", "x"])

r38 = list(itertools.repeat(0, 0))
check90("repeat 0 times", r38, [])

# --- itertools.count ---
r39 = list(itertools.islice(itertools.count(10), 5))
check90("count from 10", r39, [10, 11, 12, 13, 14])

r40 = list(itertools.islice(itertools.count(0, 3), 4))
check90("count step 3", r40, [0, 3, 6, 9])

# --- itertools.cycle ---
r41 = list(itertools.islice(itertools.cycle([1, 2, 3]), 7))
check90("cycle 7 elements", r41, [1, 2, 3, 1, 2, 3, 1])

print(f"Tests: {total90} | Passed: {passed90} | Failed: {total90 - passed90}")
