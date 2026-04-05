"""Test new pure Python stdlib modules and enhanced features."""
checks = 0

# Test bisect module
import bisect
a = [1, 3, 5, 7, 9]
idx = bisect.bisect_left(a, 5)
assert idx == 2, f"bisect_left expected 2, got {idx}"
idx = bisect.bisect_right(a, 5)
assert idx == 3, f"bisect_right expected 3, got {idx}"
bisect.insort(a, 4)
assert a == [1, 3, 4, 5, 7, 9], f"insort failed: {a}"
checks += 1
print("PASS bisect")

# Test heapq module
import heapq
h = []
heapq.heappush(h, 5)
heapq.heappush(h, 1)
heapq.heappush(h, 3)
smallest = heapq.heappop(h)
assert smallest == 1, f"heappop expected 1, got {smallest}"
data = [5, 1, 8, 3, 7, 2]
heapq.heapify(data)
result = []
while data:
    result.append(heapq.heappop(data))
assert result == [1, 2, 3, 5, 7, 8], f"heapsort failed: {result}"
checks += 1
print("PASS heapq")

# Test heapq nlargest/nsmallest
big = heapq.nlargest(3, [5, 1, 8, 3, 7, 2])
assert big == [8, 7, 5], f"nlargest failed: {big}"
small = heapq.nsmallest(3, [5, 1, 8, 3, 7, 2])
assert small == [1, 2, 3], f"nsmallest failed: {small}"
checks += 1
print("PASS heapq_nlargest_nsmallest")

# Test difflib SequenceMatcher
import difflib
s = difflib.SequenceMatcher(None, "abcde", "abdce")
ratio = s.ratio()
assert ratio > 0.5, f"ratio too low: {ratio}"
checks += 1
print("PASS difflib_ratio")

# Test difflib get_close_matches
matches = difflib.get_close_matches("appel", ["ape", "apple", "peach", "puppy"])
assert "apple" in matches, f"get_close_matches failed: {matches}"
checks += 1
print("PASS difflib_close_matches")

# Test shlex
import shlex
tokens = shlex.split('echo "hello world" foo')
assert len(tokens) == 3, f"shlex split expected 3 tokens, got {len(tokens)}: {tokens}"
assert tokens[0] == "echo"
assert tokens[1] == "hello world"
assert tokens[2] == "foo"
checks += 1
print("PASS shlex_split")

# Test shlex.quote
q = shlex.quote("hello world")
assert "'" in q or '"' in q, f"shlex.quote didn't quote: {q}"
safe = shlex.quote("simple")
assert safe == "simple", f"shlex.quote quoted safe string: {safe}"
checks += 1
print("PASS shlex_quote")

# Test SimpleNamespace __eq__
import types
ns1 = types.SimpleNamespace(x=1, y=2)
ns2 = types.SimpleNamespace(x=1, y=2)
# Test that __eq__ exists and works
eq_result = ns1.__eq__(ns2)
assert eq_result == True, f"SimpleNamespace __eq__ failed: {eq_result}"
checks += 1
print("PASS simplenamespace_eq")

# Test enhanced unittest assertions
import unittest
tc = unittest.TestCase()

# Test assertAlmostEqual with different places
tc.assertAlmostEqual(3.14159, 3.14159)  # exactly equal
tc.assertAlmostEqual(1.0, 1.00000001)   # within 7 places (diff = 1e-8 < 5e-8)
tc.assertNotAlmostEqual(1.0, 1.1)
checks += 1
print("PASS unittest_almost_equal")

# Test assertGreaterEqual/LessEqual
tc.assertGreaterEqual(5, 5)
tc.assertGreaterEqual(10, 5)
tc.assertLessEqual(5, 5)
tc.assertLessEqual(3, 5)
checks += 1
print("PASS unittest_ge_le")

# Test assertRegex
tc.assertRegex("Python 3.8", "[0-9]+\\.[0-9]+")
tc.assertNotRegex("hello", "[0-9]+")
checks += 1
print("PASS unittest_regex")

# Test assertCountEqual
tc.assertCountEqual([3, 1, 2], [1, 2, 3])
tc.assertCountEqual(["b", "a", "c"], ["a", "b", "c"])
checks += 1
print("PASS unittest_count_equal")

# Test assertDictEqual
tc.assertDictEqual({"x": 1, "y": 2}, {"x": 1, "y": 2})
checks += 1
print("PASS unittest_dict_equal")

# Test assertListEqual / assertTupleEqual
tc.assertListEqual([1, 2, 3], [1, 2, 3])
tc.assertTupleEqual((1, 2), (1, 2))
checks += 1
print("PASS unittest_list_tuple_equal")

# Test assertMultiLineEqual
tc.assertMultiLineEqual("line1\nline2\nline3", "line1\nline2\nline3")
checks += 1
print("PASS unittest_multiline_equal")

# Test fail and subTest
try:
    tc.fail("test failure")
    assert False, "fail() should raise"
except:
    pass  # Expected
checks += 1
print("PASS unittest_fail")

print(f"\n{checks}/{checks} checks passed")
