# test_phase64.py — sys recursion limits and str.maketrans/translate

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name)

# ── sys.getrecursionlimit / sys.setrecursionlimit ──

import sys

# getrecursionlimit returns a positive integer
limit = sys.getrecursionlimit()
test("getrecursionlimit returns int", type(limit) == int)
test("getrecursionlimit > 0", limit > 0)
test("getrecursionlimit default 1000", limit == 1000)

# setrecursionlimit changes the limit
sys.setrecursionlimit(500)
test("setrecursionlimit to 500", sys.getrecursionlimit() == 500)

sys.setrecursionlimit(2000)
test("setrecursionlimit to 2000", sys.getrecursionlimit() == 2000)

# restore default
sys.setrecursionlimit(1000)
test("restore default 1000", sys.getrecursionlimit() == 1000)

# recursion limit is enforced
def recurse(n):
    if n <= 0:
        return 0
    return recurse(n - 1) + 1

sys.setrecursionlimit(50)
caught = False
try:
    recurse(200)
except RecursionError:
    caught = True
test("RecursionError raised", caught)

# restore for remaining tests
sys.setrecursionlimit(1000)

# ── str.maketrans / str.translate ──

# maketrans with two args
table = str.maketrans("abc", "xyz")
test("maketrans type is dict", type(table) == dict)
test("maketrans maps ord('a')->ord('x')", table[ord("a")] == ord("x"))
test("maketrans maps ord('b')->ord('y')", table[ord("b")] == ord("y"))
test("maketrans maps ord('c')->ord('z')", table[ord("c")] == ord("z"))

# translate with two-arg table
result = "abcdef".translate(table)
test("translate two-arg", result == "xyzdef")

# maketrans with three args (third arg = delete chars)
table2 = str.maketrans("aeiou", "AEIOU", "xyz")
result2 = "helloxyworld".translate(table2)
test("translate three-arg", result2 == "hEllOwOrld")

# maketrans with single dict arg
table3 = str.maketrans({ord("h"): "H", ord("w"): "W"})
result3 = "hello world".translate(table3)
test("translate dict-arg maketrans", result3 == "Hello World")

# translate with None values (delete)
table4 = str.maketrans("", "", "aeiou")
result4 = "hello world".translate(table4)
test("translate delete vowels", result4 == "hll wrld")

print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed > 0:
    raise SystemExit(1)
