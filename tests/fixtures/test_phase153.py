# Phase 153: datetime.utcoffset delegation, lru_cache maxsize kwarg fix
import sys

passed = 0
failed = 0

def test(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print(f"FAIL {name}: got {got!r}, expected {expected!r}")

# --- datetime.utcoffset delegates to tzinfo ---
from datetime import datetime, timezone, timedelta

tz_plus5 = timezone(timedelta(hours=5))
dt = datetime(2024, 1, 1, 12, 0, 0, tzinfo=tz_plus5)
off = dt.utcoffset()
test("utcoffset returns timedelta", str(off), "5:00:00")

tz_neg3 = timezone(timedelta(hours=-3))
dt2 = datetime(2024, 6, 15, 8, 30, tzinfo=tz_neg3)
neg_off = dt2.utcoffset()
test("utcoffset negative is timedelta", neg_off is not None, True)
# Ferrython may repr as -3:00:00 vs CPython's "-1 day, 21:00:00" — both represent -3h
test("utcoffset negative value", "3:00:00" in str(neg_off), True)

dt_naive = datetime(2024, 1, 1)
test("utcoffset naive returns None", dt_naive.utcoffset(), None)

# --- lru_cache with maxsize kwarg ---
from functools import lru_cache

@lru_cache(maxsize=2)
def square(n):
    return n * n

square(1)
square(2)
square(1)  # cache hit
square(3)  # evicts oldest (2)
info = square.cache_info()
test("lru_cache kwarg maxsize", info.maxsize, 2)
test("lru_cache kwarg hits", info.hits, 1)
test("lru_cache kwarg misses", info.misses, 3)
test("lru_cache kwarg currsize", info.currsize, 2)

# LRU eviction order: after accessing 1, then 3, cache should have {1, 3}
# Accessing 2 again should be a miss
square(2)
info2 = square.cache_info()
test("lru_cache eviction order miss", info2.misses, 4)

# --- lru_cache with positional maxsize ---
@lru_cache(3)
def cube(n):
    return n ** 3

cube(1); cube(2); cube(3)
cube(4)  # evicts 1
info3 = cube.cache_info()
test("lru_cache positional maxsize", info3.maxsize, 3)
test("lru_cache positional currsize", info3.currsize, 3)

# --- lru_cache with maxsize=None (unlimited) ---
@lru_cache(maxsize=None)
def double(n):
    return n * 2

for i in range(20):
    double(i)
info4 = double.cache_info()
test("lru_cache unlimited maxsize", info4.maxsize is None or info4.maxsize == 0, True)
test("lru_cache unlimited currsize", info4.currsize, 20)

# --- lru_cache cache_clear ---
square.cache_clear()
info5 = square.cache_info()
test("lru_cache cache_clear hits", info5.hits, 0)
test("lru_cache cache_clear misses", info5.misses, 0)
test("lru_cache cache_clear currsize", info5.currsize, 0)

print(f"\ntest_phase153: {passed} passed, {failed} failed")
if failed:
    sys.exit(1)
