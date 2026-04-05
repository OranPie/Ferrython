# ═══════════════════════════════════════════
# Phase 62 Tests — functools.lru_cache
# ═══════════════════════════════════════════

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL: " + name)

# ── lru_cache with fibonacci (should be fast due to caching) ──

from functools import lru_cache

@lru_cache(maxsize=128)
def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

test("fib_0", fib(0) == 0)
test("fib_1", fib(1) == 1)
test("fib_10", fib(10) == 55)
test("fib_20", fib(20) == 6765)
test("fib_30", fib(30) == 832040)

# ── cache_info reports hits/misses ──

info = fib.cache_info()
test("cache_info_has_hits", info.hits > 0)
test("cache_info_has_misses", info.misses > 0)
test("cache_info_maxsize", info.maxsize == 128)
test("cache_info_currsize", info.currsize > 0)
# fib(30) requires 31 unique calls (0..30), so misses == 31
test("cache_info_misses_eq_31", info.misses == 31)
# All recursive calls after the first unique call are hits
test("cache_info_hits_gt_0", info.hits > 0)

# ── cache_clear ──

fib.cache_clear()
info_after_clear = fib.cache_info()
test("cache_clear_hits_reset", info_after_clear.hits == 0)
test("cache_clear_misses_reset", info_after_clear.misses == 0)
test("cache_clear_currsize_reset", info_after_clear.currsize == 0)

# Recompute after clear to verify cache still works
test("fib_after_clear", fib(10) == 55)
info2 = fib.cache_info()
test("cache_works_after_clear", info2.misses == 11)  # 0..10 = 11 unique calls

# ── lru_cache(maxsize=None) — unlimited ──

@lru_cache(maxsize=None)
def square(x):
    return x * x

for i in range(200):
    square(i)
# All 200 entries should be cached (no eviction)
info3 = square.cache_info()
test("unlimited_currsize", info3.currsize == 200)
test("unlimited_maxsize_none", info3.maxsize == None)

# Call again — all should be hits
for i in range(200):
    square(i)
info4 = square.cache_info()
test("unlimited_all_hits", info4.hits == 200)
test("unlimited_misses_unchanged", info4.misses == 200)

# ── lru_cache as bare decorator ──

@lru_cache
def cube(x):
    return x * x * x

test("bare_decorator", cube(3) == 27)
test("bare_decorator_cached", cube(3) == 27)
info5 = cube.cache_info()
test("bare_decorator_hits", info5.hits == 1)
test("bare_decorator_misses", info5.misses == 1)

# ── lru_cache() with empty parens ──

@lru_cache()
def double(x):
    return x * 2

test("empty_parens", double(5) == 10)
test("empty_parens_cached", double(5) == 10)
info6 = double.cache_info()
test("empty_parens_hits", info6.hits == 1)
test("empty_parens_misses", info6.misses == 1)

# ── Summary ──
print("========================================")
print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL TESTS PASSED!")
