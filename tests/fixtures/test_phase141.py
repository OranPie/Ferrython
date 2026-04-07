# Phase 141: timezone, frozenset hash, marshal, sqlite3, multiprocessing, contextmanager

# timezone
from datetime import timezone, timedelta
assert str(timezone.utc) == 'UTC'
tz5 = timezone(timedelta(hours=5))
assert str(tz5) == 'UTC+05:00'
assert timezone.utc.tzname(None) == 'UTC'
assert timezone.utc.utcoffset(None) == timedelta(0)
tzneg = timezone(timedelta(hours=-3, minutes=-30))
assert 'UTC-03:30' in str(tzneg)

# frozenset hash
fs = frozenset({1, 2, 3})
assert hash(fs) != 0
assert hash(frozenset({1,2,3})) == hash(frozenset({3,1,2}))
d = {frozenset({1,2}): 'ab'}
assert d[frozenset({2,1})] == 'ab'
# Nested frozenset
outer = frozenset({frozenset({1,2}), frozenset({3,4})})
assert len(outer) == 2

# marshal roundtrip
import marshal
for v in [42, 3.14, 'hello', b'\x00\x01', [1, 2], (3, 4), {5: 6}, True, None]:
    assert marshal.loads(marshal.dumps(v)) == v, f"marshal fail for {v!r}"

# sqlite3 Row
import sqlite3
conn = sqlite3.connect(':memory:')
conn.row_factory = sqlite3.Row
c = conn.cursor()
c.execute('CREATE TABLE t (id INTEGER, name TEXT)')
c.execute('INSERT INTO t VALUES (1, "alice")')
c.execute('SELECT * FROM t')
row = c.fetchone()
assert row['name'] == 'alice'
assert row[0] == 1
assert list(row.keys()) == ['id', 'name']
conn.close()

# multiprocessing primitives
from multiprocessing import Queue, Event, Semaphore
q = Queue()
q.put(42)
q.put('hello')
assert q.get() == 42
assert not q.empty()
assert q.get() == 'hello'
assert q.empty()
e = Event()
assert not e.is_set()
e.set()
assert e.is_set()
e.clear()
assert not e.is_set()
sem = Semaphore(2)
assert sem.acquire()
assert sem.acquire()
sem.release()
assert sem.acquire()

# contextmanager
from contextlib import contextmanager
results = []
@contextmanager
def tracked():
    results.append('enter')
    yield 99
    results.append('exit')
with tracked() as v:
    assert v == 99
assert results == ['enter', 'exit']

# TypedDict
from typing import TypedDict
class Point(TypedDict):
    x: int
    y: int
p = Point(x=1, y=2)
assert p['x'] == 1 and p['y'] == 2

# enum Flag
from enum import Flag, auto
class Perm(Flag):
    R = auto()
    W = auto()
    X = auto()
rw = Perm.R | Perm.W
assert Perm.R in rw
assert not (Perm.X in rw)

# collections.Counter
from collections import Counter
c = Counter('aabbbcc')
assert c['b'] == 3
assert c.most_common(1) == [('b', 3)]

# functools.lru_cache
from functools import lru_cache
call_count = 0
@lru_cache(maxsize=128)
def fib(n):
    global call_count
    call_count += 1
    if n < 2:
        return n
    return fib(n-1) + fib(n-2)
assert fib(10) == 55
first_count = call_count
fib(10)  # should hit cache
assert call_count == first_count  # no new calls

print("phase141: all checks passed")
