# Test 84: Tuple operations and namedtuple
import collections

passed84 = 0
total84 = 0

def check84(desc, got, expected):
    global passed84, total84
    total84 += 1
    if got == expected:
        passed84 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- Tuple creation and indexing ---
tup84_1 = (1, 2, 3, 4, 5)
check84("tuple indexing [0]", tup84_1[0], 1)
check84("tuple indexing [4]", tup84_1[4], 5)
check84("tuple indexing [-1]", tup84_1[-1], 5)
check84("tuple indexing [-2]", tup84_1[-2], 4)

# --- Tuple slicing ---
tup84_2 = (10, 20, 30, 40, 50)
check84("tuple slice [1:3]", tup84_2[1:3], (20, 30))
check84("tuple slice [:2]", tup84_2[:2], (10, 20))
check84("tuple slice [3:]", tup84_2[3:], (40, 50))
check84("tuple slice [::2]", tup84_2[::2], (10, 30, 50))
check84("tuple slice type", type(tup84_2[1:3]), tuple)

# --- Tuple unpacking ---
a84_3, b84_3, c84_3 = (10, 20, 30)
check84("tuple unpack a", a84_3, 10)
check84("tuple unpack b", b84_3, 20)
check84("tuple unpack c", c84_3, 30)

first84_3, *rest84_3 = (1, 2, 3, 4)
check84("tuple unpack star first", first84_3, 1)
check84("tuple unpack star rest", rest84_3, [2, 3, 4])

*init84_3, last84_3 = (1, 2, 3, 4)
check84("tuple unpack star init", init84_3, [1, 2, 3])
check84("tuple unpack star last", last84_3, 4)

# --- Tuple comparison ---
check84("tuple equal", (1, 2, 3) == (1, 2, 3), True)
check84("tuple not equal", (1, 2, 3) == (1, 2, 4), False)
check84("tuple less than", (1, 2) < (1, 3), True)
check84("tuple greater than", (2,) > (1,), True)
check84("tuple lex order", (1, 2, 3) < (1, 2, 4), True)

# --- Tuple methods: count, index ---
tup84_4 = (1, 2, 3, 2, 1, 2)
check84("tuple.count(2)", tup84_4.count(2), 3)
check84("tuple.count(5)", tup84_4.count(5), 0)
check84("tuple.index(3)", tup84_4.index(3), 2)
check84("tuple.index(1)", tup84_4.index(1), 0)

# --- Tuple concatenation ---
tup84_5 = (1, 2) + (3, 4)
check84("tuple concat", tup84_5, (1, 2, 3, 4))
check84("tuple concat empty", () + (1, 2), (1, 2))

# --- Tuple repetition ---
tup84_6 = (1, 2) * 3
check84("tuple repetition", tup84_6, (1, 2, 1, 2, 1, 2))
check84("tuple repetition zero", (1, 2) * 0, ())

# --- Single-element tuple ---
tup84_7 = (1,)
check84("single element tuple", tup84_7, (1,))
check84("single element tuple len", len(tup84_7), 1)
check84("single element tuple type", type(tup84_7), tuple)

# --- Empty tuple ---
tup84_8 = ()
check84("empty tuple", tup84_8, ())
check84("empty tuple len", len(tup84_8), 0)
check84("empty tuple type", type(tup84_8), tuple)
check84("tuple() constructor", tuple(), ())

# --- Tuple as dict key ---
d84_9 = {}
d84_9[(1, 2)] = "a"
d84_9[(3, 4)] = "b"
check84("tuple as dict key", d84_9[(1, 2)], "a")
check84("tuple as dict key 2", d84_9[(3, 4)], "b")

# --- Tuple in set ---
s84_10 = set()
s84_10.add((1, 2))
s84_10.add((3, 4))
s84_10.add((1, 2))
check84("tuple in set len", len(s84_10), 2)
check84("tuple in set membership", (1, 2) in s84_10, True)

# --- collections.namedtuple creation ---
Point84_11 = collections.namedtuple("Point84_11", ["x", "y"])
p84_11 = Point84_11(3, 4)
check84("namedtuple creation", p84_11, Point84_11(3, 4))
check84("namedtuple type", type(p84_11).__name__, "Point84_11")

# --- namedtuple field access ---
check84("namedtuple field x", p84_11.x, 3)
check84("namedtuple field y", p84_11.y, 4)
check84("namedtuple index [0]", p84_11[0], 3)
check84("namedtuple index [1]", p84_11[1], 4)

# --- namedtuple._asdict() ---
d84_12 = p84_11._asdict()
check84("namedtuple._asdict type", type(d84_12), dict)
check84("namedtuple._asdict x", d84_12["x"], 3)
check84("namedtuple._asdict y", d84_12["y"], 4)

# --- namedtuple._replace() ---
p84_13 = p84_11._replace(x=10)
check84("namedtuple._replace x", p84_13.x, 10)
check84("namedtuple._replace y unchanged", p84_13.y, 4)
check84("namedtuple._replace original unchanged", p84_11.x, 3)

# --- namedtuple._fields ---
check84("namedtuple._fields", Point84_11._fields, ("x", "y"))

# --- isinstance(nt, tuple) ---
check84("namedtuple is tuple", isinstance(p84_11, tuple), True)
check84("namedtuple len", len(p84_11), 2)

# --- namedtuple unpacking ---
x84_14, y84_14 = p84_11
check84("namedtuple unpack x", x84_14, 3)
check84("namedtuple unpack y", y84_14, 4)

print(f"Tests: {total84} | Passed: {passed84} | Failed: {total84 - passed84}")
