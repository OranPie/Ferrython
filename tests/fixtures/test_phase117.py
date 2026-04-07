# Test pathlib rglob, replace, is_relative_to + threading improvements

import pathlib
import os
import tempfile
import shutil

# --- pathlib.rglob ---
tmpdir = tempfile.mkdtemp()
# Clean stale content from previous runs
for item in os.listdir(tmpdir):
    full = os.path.join(tmpdir, item)
    if os.path.isdir(full):
        shutil.rmtree(full)
    else:
        os.remove(full)
subdir = os.path.join(tmpdir, "sub")
os.makedirs(subdir, exist_ok=True)

# Create files at various levels
with open(os.path.join(tmpdir, "a.txt"), "w") as f:
    f.write("root")
with open(os.path.join(subdir, "b.txt"), "w") as f:
    f.write("sub")
with open(os.path.join(subdir, "c.py"), "w") as f:
    f.write("code")

p = pathlib.Path(tmpdir)
txt_files = p.rglob("*.txt")
txt_names = sorted([str(f) for f in txt_files])
assert len(txt_names) == 2, f"Expected 2 .txt files, got {len(txt_names)}: {txt_names}"
print("pathlib.rglob: OK")

# --- pathlib.replace ---
src = pathlib.Path(os.path.join(tmpdir, "a.txt"))
dst_path = os.path.join(tmpdir, "replaced.txt")
result = src.replace(dst_path)
assert not os.path.exists(os.path.join(tmpdir, "a.txt")), "source should be gone"
assert os.path.exists(dst_path), "destination should exist"
print("pathlib.replace: OK")

# --- pathlib.is_relative_to ---
p1 = pathlib.Path("/home/user/docs/file.txt")
assert p1.is_relative_to("/home/user"), "should be relative to /home/user"
assert p1.is_relative_to("/home"), "should be relative to /home"
assert not p1.is_relative_to("/var"), "should not be relative to /var"
print("pathlib.is_relative_to: OK")

# Cleanup
os.remove(dst_path)
os.remove(os.path.join(subdir, "b.txt"))
os.remove(os.path.join(subdir, "c.py"))
os.rmdir(subdir)
os.rmdir(tmpdir)

# --- threading improvements ---
import threading

# Test Thread with target and args
results = []
def worker(x, y):
    results.append(x + y)

t = threading.Thread(target=worker, args=(3, 4))
t.start()
t.join()
assert results == [7], f"Expected [7], got {results}"
print("threading.Thread: OK")

# Test Lock
lock = threading.Lock()
assert lock.acquire(), "Lock should be acquirable"
lock.release()
print("threading.Lock: OK")

# Test Event
event = threading.Event()
assert not event.is_set(), "Event should not be set initially"
event.set()
assert event.is_set(), "Event should be set after set()"
event.clear()
assert not event.is_set(), "Event should not be set after clear()"
print("threading.Event: OK")

# Test current_thread
ct = threading.current_thread()
assert ct is not None
print("threading.current_thread: OK")

# Test Barrier (if exists)
if hasattr(threading, 'Barrier'):
    print("threading.Barrier: exists")

# Test Semaphore
sem = threading.Semaphore(2)
assert sem.acquire(), "Semaphore should be acquirable"
assert sem.acquire(), "Semaphore should be acquirable (count 2)"
sem.release()
sem.release()
print("threading.Semaphore: OK")

# --- multiprocessing check ---
import multiprocessing
q = multiprocessing.Queue()
q.put(42)
assert q.get() == 42
q.put("hello")
q.put("world")
assert q.qsize() == 2
print("multiprocessing.Queue: OK")

ev = multiprocessing.Event()
assert not ev.is_set()
ev.set()
assert ev.is_set()
ev.clear()
assert not ev.is_set()
print("multiprocessing.Event: OK")

print("All phase 117 tests passed!")
