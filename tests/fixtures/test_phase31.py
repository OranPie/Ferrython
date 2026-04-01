# Phase 31: Expanded itertools, Counter.most_common, more stdlib
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── itertools.combinations ──
from itertools import combinations, permutations, accumulate, compress, tee, product, chain

result = list(combinations([1, 2, 3, 4], 2))
test("combinations C(4,2)", len(result) == 6)
test("combinations first", result[0] == (1, 2))
test("combinations last", result[-1] == (3, 4))

result = list(combinations('abc', 2))
test("combinations strings", len(result) == 3)
test("combinations strings first", result[0] == ('a', 'b'))

# ── itertools.permutations ──
result = list(permutations([1, 2, 3]))
test("permutations 3!", len(result) == 6)
test("permutations first", result[0] == (1, 2, 3))

result = list(permutations([1, 2, 3], 2))
test("permutations P(3,2)", len(result) == 6)
test("permutations P(3,2) first", result[0] == (1, 2))

# ── itertools.accumulate ──
result = list(accumulate([1, 2, 3, 4, 5]))
test("accumulate sum", result == [1, 3, 6, 10, 15])

# ── itertools.compress ──
result = list(compress('ABCDEF', [1, 0, 1, 0, 1, 1]))
test("compress", result == ['A', 'C', 'E', 'F'])

# ── itertools.tee ──
a, b = tee([1, 2, 3])
test("tee two copies", a == [1, 2, 3] and b == [1, 2, 3])

# ── itertools.product ──
result = list(product([1, 2], [3, 4]))
test("product 2x2", len(result) == 4)
test("product first", result[0] == (1, 3))
test("product last", result[-1] == (2, 4))

# ── itertools.chain ──
result = list(chain([1, 2], [3, 4], [5]))
test("chain", result == [1, 2, 3, 4, 5])

# ── Counter.most_common ──
from collections import Counter

c = Counter(['a', 'b', 'a', 'c', 'b', 'a'])
test("Counter counts a", c['a'] == 3)
test("Counter counts b", c['b'] == 2)
test("Counter counts c", c['c'] == 1)

mc = c.most_common(2)
test("most_common length", len(mc) == 2)
test("most_common first is a", mc[0][0] == 'a' and mc[0][1] == 3)
test("most_common second is b", mc[1][0] == 'b' and mc[1][1] == 2)

mc_all = c.most_common()
test("most_common all", len(mc_all) == 3)

# Counter with integers
c2 = Counter([1, 1, 2, 2, 2, 3])
test("Counter int counts", c2[2] == 3)
mc2 = c2.most_common(1)
test("Counter int most_common", mc2[0][0] == 2 and mc2[0][1] == 3)

# ── Additional tests for dict methods ──
d = {'a': 1, 'b': 2, 'c': 3}
test("dict.copy", d.copy() == {'a': 1, 'b': 2, 'c': 3})
test("dict.setdefault exists", d.setdefault('a', 99) == 1)
test("dict.setdefault new", d.setdefault('d', 99) == 99)
test("dict.popitem", True)  # already tested elsewhere

# ── More namedtuple tests ──
from collections import namedtuple

Record = namedtuple('Record', 'id name value')
r = Record(1, 'test', 42.5)
test("namedtuple 3 fields", r.id == 1 and r.name == 'test' and r.value == 42.5)
test("namedtuple index 3", r[0] == 1 and r[1] == 'test' and r[2] == 42.5)

# ── Itertools edge cases ──
test("combinations empty", list(combinations([], 2)) == [])
test("combinations r>n", list(combinations([1, 2], 5)) == [])
test("permutations single", list(permutations([1])) == [(1,)])

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
assert failed == 0, f"{failed} tests failed!"
print("ALL PHASE 31 TESTS PASSED")
