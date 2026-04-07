# Phase 131: ctypes .value, file.fileno(), mmap slicing, deep stdlib verification
import sys

checks = []

# 1. ctypes.c_int with value
import ctypes
ci = ctypes.c_int(42)
checks.append(("ctypes_c_int_value", ci.value == 42))

# 2. ctypes.c_double with value
cd = ctypes.c_double(3.14)
checks.append(("ctypes_c_double_value", cd.value == 3.14))

# 3. ctypes.c_bool with value
cb = ctypes.c_bool(True)
checks.append(("ctypes_c_bool_value", cb.value == True))

# 4. ctypes.c_int default (no args)
c0 = ctypes.c_int()
checks.append(("ctypes_c_int_default", c0.value == 0))

# 5. file.fileno() returns positive int
import tempfile, os
tf = tempfile.mktemp()
with open(tf, 'wb') as f:
    f.write(b'hello mmap')
with open(tf, 'rb') as f:
    fd = f.fileno()
    checks.append(("file_fileno", isinstance(fd, int) and fd > 0))
os.unlink(tf)

# 6. mmap slicing
import mmap
tf2 = tempfile.mktemp()
with open(tf2, 'wb') as f:
    f.write(b'hello mmap world')
with open(tf2, 'rb') as f:
    fd = f.fileno()
    mm = mmap.mmap(fd, 0, access=mmap.ACCESS_READ)
    sliced = mm[:5]
    checks.append(("mmap_slice", sliced == b'hello'))
    mm.close()
os.unlink(tf2)

# 7. mmap index returns int
tf3 = tempfile.mktemp()
with open(tf3, 'wb') as f:
    f.write(b'ABC')
with open(tf3, 'rb') as f:
    mm = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
    checks.append(("mmap_index", mm[0] == 65))  # 'A' = 65
    mm.close()
os.unlink(tf3)

# 8. decimal operations
import decimal
d = decimal.Decimal('3.14') + decimal.Decimal('1')
checks.append(("decimal_add", str(d) == '4.14'))

# 9. fractions
import fractions
f = fractions.Fraction(1, 3) + fractions.Fraction(1, 6)
checks.append(("fractions_add", str(f) == '1/2'))

# 10. statistics
import statistics
data = [1, 2, 3, 4, 5]
checks.append(("statistics_mean", statistics.mean(data) == 3.0))

# 11. bisect
import bisect
a = [1, 3, 5, 7, 9]
checks.append(("bisect_left", bisect.bisect_left(a, 5) == 2))

# 12. heapq
import heapq
h = [5, 3, 1, 4, 2]
heapq.heapify(h)
checks.append(("heapq_heapify", heapq.heappop(h) == 1))

# 13. difflib
import difflib
d = list(difflib.unified_diff(['a', 'b'], ['a', 'c'], lineterm=''))
checks.append(("difflib_unified", len(d) > 0))

# 14. textwrap
import textwrap
t = textwrap.fill('The quick brown fox jumps', width=15)
checks.append(("textwrap_fill", '\n' in t))

# 15. pprint
import pprint
s = pprint.pformat({'a': [1, 2, 3]})
checks.append(("pprint_format", 'a' in s))

# 16. string.Template
import string
result = string.Template('$name is $adj').substitute(name='X', adj='Y')
checks.append(("string_template", result == 'X is Y'))

# 17. operator module
import operator
checks.append(("operator_add", operator.add(3, 4) == 7))
checks.append(("operator_itemgetter", operator.itemgetter(1)([10, 20, 30]) == 20))

# 19. copy.deepcopy
import copy
lst = [[1, 2], [3]]
d = copy.deepcopy(lst)
lst[0].append(9)
checks.append(("deepcopy_independent", d[0] == [1, 2]))

# 20. abc.abstractmethod enforcement
import abc
class MyABC(abc.ABC):
    @abc.abstractmethod
    def m(self): pass
class Impl(MyABC):
    def m(self): return 42
checks.append(("abc_impl", Impl().m() == 42))
try:
    MyABC()
    checks.append(("abc_abstract", False))
except TypeError:
    checks.append(("abc_abstract", True))

# 22. threading with real work
import threading
results = []
def worker(n):
    results.append(n * n)
threads = [threading.Thread(target=worker, args=(i,)) for i in range(5)]
for t in threads: t.start()
for t in threads: t.join()
checks.append(("threading_results", sorted(results) == [0, 1, 4, 9, 16]))

# 23. queue.PriorityQueue
import queue
pq = queue.PriorityQueue()
pq.put((2, 'two'))
pq.put((1, 'one'))
checks.append(("priority_queue", pq.get() == (1, 'one')))

# 24. re.sub
import re
result = re.sub(r'(\w+)@(\w+)', r'\2:\1', 'user@host')
checks.append(("re_sub", result == 'host:user'))

# 25. re.split
parts = re.split(r'[,;]+', 'a,b;;c')
checks.append(("re_split", parts == ['a', 'b', 'c']))

# 26. csv.DictWriter / DictReader round-trip
import csv, io
sio = io.StringIO()
w = csv.DictWriter(sio, fieldnames=['name', 'age'])
w.writeheader()
w.writerow({'name': 'Alice', 'age': 30})
rows = list(csv.DictReader(io.StringIO(sio.getvalue())))
checks.append(("csv_dictwriter", rows[0]['name'] == 'Alice'))

# 27. datetime arithmetic
import datetime
d1 = datetime.date(2024, 1, 1)
d2 = datetime.date(2024, 2, 1)
checks.append(("date_diff", (d2 - d1).days == 31))

# 28. datetime.strftime
dt = datetime.datetime(2024, 6, 15, 10, 30)
checks.append(("strftime", dt.strftime('%Y-%m-%d') == '2024-06-15'))

# 29. subprocess.run
import subprocess
r = subprocess.run(['echo', 'hi'], capture_output=True, text=True)
checks.append(("subprocess_run", r.stdout.strip() == 'hi'))

# 30. pathlib operations
from pathlib import Path
p = Path('/tmp') / 'ferrython_phase131_test'
p.mkdir(exist_ok=True)
(p / 'x.txt').write_text('data')
checks.append(("pathlib_write_read", (p / 'x.txt').read_text() == 'data'))
import shutil
shutil.rmtree(str(p))

# 31. os.walk
td = tempfile.mkdtemp()
os.makedirs(os.path.join(td, 'sub'))
with open(os.path.join(td, 'f.txt'), 'w') as f: f.write('x')
walked = list(os.walk(td))
checks.append(("os_walk", len(walked) >= 2))
shutil.rmtree(td)

# 32. inspect.signature
import inspect
def func(a, b=10): pass
sig = inspect.signature(func)
params = list(sig.parameters.keys())
checks.append(("inspect_signature", params == ['a', 'b']))

# 33. asyncio.gather
import asyncio
async def sq(x):
    return x * x
async def amain():
    return await asyncio.gather(sq(2), sq(3), sq(4))
checks.append(("asyncio_gather", asyncio.run(amain()) == [4, 9, 16]))

# 34. async for with async generator
async def agen():
    for i in range(3):
        yield i
async def collect():
    r = []
    async for v in agen():
        r.append(v)
    return r
checks.append(("async_for", asyncio.run(collect()) == [0, 1, 2]))

# 35. sqlite3 with Row factory
import sqlite3
conn = sqlite3.connect(':memory:')
conn.execute('CREATE TABLE t (id INTEGER, name TEXT)')
conn.execute("INSERT INTO t VALUES (1, 'Alice')")
conn.commit()
conn.row_factory = sqlite3.Row
row = conn.cursor().execute('SELECT * FROM t').fetchone()
checks.append(("sqlite3_row", row['name'] == 'Alice'))
conn.close()

# 36. collections.deque maxlen
from collections import deque
d = deque([1, 2, 3], maxlen=3)
d.append(4)
checks.append(("deque_maxlen", list(d) == [2, 3, 4]))

# 37. collections.Counter.most_common
from collections import Counter
c = Counter('abracadabra')
top = c.most_common(1)
checks.append(("counter_most_common", top[0][0] == 'a' and top[0][1] == 5))

# 38. weakref.WeakValueDictionary
import weakref
class Obj: pass
o = Obj()
d = weakref.WeakValueDictionary()
d['k'] = o
checks.append(("weakvalue_dict", d['k'] is o))

# 39. pickle round-trip with class
import pickle
class Pt:
    def __init__(self, x, y):
        self.x = x
        self.y = y
p = Pt(3, 4)
p2 = pickle.loads(pickle.dumps(p))
checks.append(("pickle_class", p2.x == 3 and p2.y == 4))

# 40. hashlib md5
import hashlib
h = hashlib.md5(b'hello').hexdigest()
checks.append(("hashlib_md5", h == '5d41402abc4b2a76b9719d911017c592'))

# ── report ──
passed = sum(1 for _, v in checks if v)
failed = [(n, v) for n, v in checks if not v]
print(f"phase131: {passed}/{len(checks)} passed")
for name, _ in failed:
    print(f"  FAIL: {name}")
if failed:
    sys.exit(1)
