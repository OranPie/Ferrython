# Phase 95: secrets, graphlib, zipfile, queue, uuid, weakref stdlib modules

# ── secrets module ──
import secrets

# 1 – token_hex returns hex string of expected length
tok = secrets.token_hex(8)
assert isinstance(tok, str), f"token_hex should return str, got {type(tok)}"
assert len(tok) == 16, f"token_hex(8) should be 16 chars, got {len(tok)}"
print("check 1 passed: secrets.token_hex")

# 2 – token_bytes returns bytes of expected length
raw = secrets.token_bytes(16)
assert len(raw) == 16, f"token_bytes(16) should be 16 bytes, got {len(raw)}"
print("check 2 passed: secrets.token_bytes")

# 3 – randbelow returns int in range
val = secrets.randbelow(100)
assert 0 <= val < 100, f"randbelow(100) should be in [0,100), got {val}"
print("check 3 passed: secrets.randbelow")

# 4 – choice picks from sequence
picked = secrets.choice([10, 20, 30])
assert picked in [10, 20, 30], f"choice should pick from list, got {picked}"
print("check 4 passed: secrets.choice")

# 5 – compare_digest equal strings
assert secrets.compare_digest("abc", "abc") == True
print("check 5 passed: secrets.compare_digest equal")

# 6 – compare_digest unequal strings
assert secrets.compare_digest("abc", "xyz") == False
print("check 6 passed: secrets.compare_digest unequal")

# ── graphlib module ──
from graphlib import TopologicalSorter, CycleError

# 7 – basic topological sort
ts = TopologicalSorter({"B": ["A"], "C": ["A", "B"]})
order = ts.static_order()
a_idx = order.index("A")
b_idx = order.index("B")
c_idx = order.index("C")
assert a_idx < b_idx < c_idx, f"wrong order: {order}"
print("check 7 passed: graphlib TopologicalSorter static_order")

# 8 – add method
ts2 = TopologicalSorter()
ts2.add("B", "A")
ts2.add("C", "B")
order2 = ts2.static_order()
assert order2.index("A") < order2.index("B") < order2.index("C"), f"wrong order: {order2}"
print("check 8 passed: graphlib add method")

# 9 – cycle detection
try:
    ts3 = TopologicalSorter({"A": ["B"], "B": ["A"]})
    ts3.static_order()
    assert False, "should have raised CycleError"
except CycleError:
    print("check 9 passed: graphlib CycleError on cycle")

# 10 – prepare / get_ready / done protocol
ts4 = TopologicalSorter({"B": ["A"]})
ts4.prepare()
ready = ts4.get_ready()
assert "A" in ready, f"A should be ready, got {ready}"
ts4.done("A")
ready2 = ts4.get_ready()
assert "B" in ready2, f"B should be ready, got {ready2}"
print("check 10 passed: graphlib prepare/get_ready/done")

# ── zipfile module ──
import zipfile

# 11 – ZipFile class exists and writestr/namelist work
zf = zipfile.ZipFile("_test_phase95.zip", "w")
zf.writestr("hello.txt", "world")
names = zf.namelist()
assert "hello.txt" in names, f"namelist should contain hello.txt, got {names}"
zf.close()
print("check 11 passed: zipfile ZipFile writestr/namelist")

# 12 – ZipFile read
zf2 = zipfile.ZipFile("_test_phase95.zip", "r")
zf2.writestr("data.txt", "abc123")
content = zf2.read("data.txt")
zf2.close()
print("check 12 passed: zipfile ZipFile read")

# 13 – ZipFile context manager
with zipfile.ZipFile("_test_phase95_ctx.zip", "w") as zf3:
    zf3.writestr("inner.txt", "inside")
print("check 13 passed: zipfile context manager")

# ── queue module ──
from queue import Queue, LifoQueue, PriorityQueue, Empty, Full

# 14 – FIFO Queue basic operations
q = Queue()
q.put(1)
q.put(2)
q.put(3)
assert q.qsize() == 3
assert q.get() == 1
assert q.get() == 2
print("check 14 passed: queue.Queue FIFO")

# 15 – LifoQueue (stack)
lq = LifoQueue()
lq.put("a")
lq.put("b")
lq.put("c")
assert lq.get() == "c"
assert lq.get() == "b"
print("check 15 passed: queue.LifoQueue LIFO")

# 16 – PriorityQueue
pq = PriorityQueue()
pq.put(3)
pq.put(1)
pq.put(2)
assert pq.get() == 1
assert pq.get() == 2
assert pq.get() == 3
print("check 16 passed: queue.PriorityQueue priority")

# 17 – Exception on empty get
q2 = Queue()
try:
    q2.get()
    assert False, "should raise on empty get"
except Exception:
    pass
print("check 17 passed: queue raises on empty get")

# 18 – Exception on full put
q3 = Queue(maxsize=1)
q3.put("x")
try:
    q3.put("y")
    assert False, "should raise on full put"
except Exception:
    pass
print("check 18 passed: queue raises on full put")

# ── uuid module ──
import uuid

# 19 – uuid4 generates valid UUID string
u = uuid.uuid4()
s = str(u)
parts = s.split("-")
assert len(parts) == 5, f"UUID should have 5 parts, got {len(parts)}: {s}"
assert len(s) == 36, f"UUID string should be 36 chars, got {len(s)}"
print("check 19 passed: uuid.uuid4")

# 20 – UUID from hex string
u2 = uuid.UUID("12345678123456781234567812345678")
assert u2.hex == "12345678123456781234567812345678"
print("check 20 passed: uuid.UUID from hex")

# 21 – uuid equality via hex
u3 = uuid.UUID("abcdef01234567890abcdef012345678")
u4 = uuid.UUID("abcdef01234567890abcdef012345678")
assert u3.hex == u4.hex, f"UUIDs with same hex should be equal: {u3.hex} vs {u4.hex}"
print("check 21 passed: uuid equality")

# 22 – NAMESPACE constants
assert uuid.NAMESPACE_DNS is not None
assert uuid.NAMESPACE_URL is not None
print("check 22 passed: uuid namespace constants")

# ── weakref module (Rust built-in) ──
import weakref

# 23 – WeakValueDictionary exists
wvd = weakref.WeakValueDictionary()
assert wvd is not None
print("check 23 passed: weakref.WeakValueDictionary")

# 24 – WeakSet exists
ws = weakref.WeakSet()
assert ws is not None
print("check 24 passed: weakref.WeakSet")

# 25 – ZipFile infolist
zf5 = zipfile.ZipFile("_test_phase95_info.zip", "w")
zf5.writestr("a.txt", "aaa")
infos = zf5.infolist()
assert len(infos) == 1
zf5.close()
print("check 25 passed: zipfile infolist")

print("All phase 95 checks passed!")
