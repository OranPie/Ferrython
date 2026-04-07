# Test dis file= kwarg, threading.Lock blocking=, operator.methodcaller args, tempfile uniqueness

# --- dis.dis with file= kwarg ---
import dis, io
buf = io.StringIO()
code = compile('x = 1 + 2', '<test>', 'exec')
dis.dis(code, file=buf)
output = buf.getvalue()
assert len(output) > 0, "dis output should not be empty"
assert 'LoadConst' in output, f"expected LoadConst in output: {output!r}"
print("dis file= kwarg: OK")

# --- threading.Lock blocking=False ---
import threading
lock = threading.Lock()
lock.acquire()
result = lock.acquire(blocking=False)
assert result == False, f"expected False when already locked, got {result}"
lock.release()
result2 = lock.acquire(blocking=False)
assert result2 == True, f"expected True on unlocked, got {result2}"
lock.release()
print("threading.Lock blocking: OK")

# --- operator.methodcaller with args ---
from operator import methodcaller
mc_split = methodcaller('split', ',')
assert mc_split('a,b,c') == ['a', 'b', 'c'], f"split failed: {mc_split('a,b,c')}"
mc_upper = methodcaller('upper')
assert mc_upper('hello') == 'HELLO'
mc_replace = methodcaller('replace', 'world', 'rust')
assert mc_replace('hello world') == 'hello rust'
mc_starts = methodcaller('startswith', 'he')
assert mc_starts('hello') == True
mc_find = methodcaller('find', 'lo')
assert mc_find('hello') == 3
print("operator.methodcaller: OK")

# --- tempfile unique names ---
import tempfile, os
d1 = tempfile.mkdtemp()
d2 = tempfile.mkdtemp()
assert d1 != d2, f"mkdtemp should generate unique names: {d1} == {d2}"
assert os.path.isdir(d1)
assert os.path.isdir(d2)
os.rmdir(d1)
os.rmdir(d2)
print("tempfile unique: OK")

# --- RLock blocking ---
rlock = threading.RLock()
rlock.acquire()
assert rlock.acquire(blocking=False) == True, "RLock should allow reentrant acquire"
rlock.release()
rlock.release()
print("RLock blocking: OK")

print("All phase 149 tests passed!")
