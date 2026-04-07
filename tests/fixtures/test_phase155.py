# test_phase155.py — gc.get_objects, logging.getMessage, deeper stdlib probes

# gc.get_objects
import gc
assert isinstance(gc.get_objects(), list), "gc.get_objects should return list"
assert isinstance(gc.get_referrers(), list), "gc.get_referrers should return list"
assert isinstance(gc.get_referents(), list), "gc.get_referents should return list"
gc.freeze()
gc.unfreeze()
assert gc.get_freeze_count() == 0
assert isinstance(gc.garbage, list)
assert isinstance(gc.callbacks, list)

# logging getMessage
import logging
class TestHandler(logging.Handler):
    def __init__(self):
        super().__init__()
        self.records = []
    def emit(self, record):
        self.records.append(record)
h = TestHandler()
logger = logging.getLogger("test_gm155")
logger.addHandler(h)
logger.setLevel(logging.DEBUG)
logger.info("hello world")
assert len(h.records) >= 1, "handler should have records"
assert h.records[0].getMessage() == "hello world", f"getMessage failed: {h.records[0].getMessage()}"

# stdlib depth checks
import struct
assert struct.unpack('>I', struct.pack('>I', 1024))[0] == 1024

import base64
assert base64.b64decode(base64.b64encode(b"test")) == b"test"

import io
sio = io.StringIO()
sio.write("hello ")
sio.write("world")
assert sio.getvalue() == "hello world"

from contextlib import suppress
with suppress(ValueError):
    int("abc")

import decimal
assert str(decimal.Decimal("1.1") + decimal.Decimal("2.2")) == "3.3"

from collections import OrderedDict
od = OrderedDict()
od['a'] = 1; od['b'] = 2; od['c'] = 3
od.move_to_end('a')
assert list(od.keys()) == ['b', 'c', 'a']

import itertools
assert list(itertools.chain.from_iterable([[1,2],[3,4]])) == [1,2,3,4]
assert list(itertools.combinations([1,2,3], 2)) == [(1,2),(1,3),(2,3)]
assert list(itertools.combinations_with_replacement([1,2], 2)) == [(1,1),(1,2),(2,2)]
a, b = itertools.tee(range(3), 2)
assert list(a) == [0,1,2] and list(b) == [0,1,2]

from functools import total_ordering
@total_ordering
class Student:
    def __init__(self, grade): self.grade = grade
    def __eq__(self, o): return self.grade == o.grade
    def __lt__(self, o): return self.grade < o.grade
assert Student(90) < Student(95) and Student(90) >= Student(85)

import math
assert math.comb(10,3) == 120 and math.perm(5,2) == 20
assert math.prod([1,2,3,4,5]) == 120
assert math.isclose(0.1 + 0.2, 0.3, rel_tol=1e-9)

from functools import cache
calls = 0
@cache
def fib(n):
    global calls; calls += 1
    if n < 2: return n
    return fib(n-1) + fib(n-2)
assert fib(10) == 55 and calls == 11

print("test_phase155 passed")
