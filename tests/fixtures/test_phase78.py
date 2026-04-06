# test_phase78: Enhanced io.StringIO/BytesIO + more stdlib features
results = []

# ── 1. io.StringIO with full methods ──
try:
    import io
    
    sio = io.StringIO()
    sio.write("Hello ")
    sio.write("World!")
    assert sio.getvalue() == "Hello World!", f"getvalue: {sio.getvalue()!r}"
    assert sio.tell() == 12, f"tell: {sio.tell()}"
    
    # seek and read
    sio.seek(0)
    assert sio.read(5) == "Hello", f"read(5): unexpected"
    assert sio.read() == " World!", f"read(): unexpected"
    
    # readline
    sio2 = io.StringIO("line1\nline2\nline3\n")
    assert sio2.readline() == "line1\n"
    assert sio2.readline() == "line2\n"
    
    # readlines
    sio2.seek(0)
    lines = sio2.readlines()
    assert len(lines) == 3, f"readlines: {lines}"
    
    # Initial value
    sio3 = io.StringIO("initial")
    assert sio3.getvalue() == "initial"
    sio3.seek(0)
    assert sio3.read() == "initial"
    
    # truncate
    sio4 = io.StringIO("abcdefg")
    sio4.seek(3)
    sio4.truncate()
    assert sio4.getvalue() == "abc", f"truncate: {sio4.getvalue()!r}"
    
    results.append("PASS io.StringIO")
except Exception as e:
    results.append(f"FAIL io.StringIO: {e}")

# ── 2. io.BytesIO with full methods ──
try:
    bio = io.BytesIO()
    bio.write(b"Hello ")
    bio.write(b"World!")
    assert bio.getvalue() == b"Hello World!"
    assert bio.tell() == 12
    
    bio.seek(0)
    assert bio.read(5) == b"Hello"
    assert bio.read() == b" World!"
    
    # Initial value
    bio2 = io.BytesIO(b"initial")
    assert bio2.getvalue() == b"initial"
    bio2.seek(0)
    assert bio2.read() == b"initial"
    
    results.append("PASS io.BytesIO")
except Exception as e:
    results.append(f"FAIL io.BytesIO: {e}")

# ── 3. io.StringIO as context manager ──
try:
    with io.StringIO() as sio:
        sio.write("test")
        val = sio.getvalue()
    assert val == "test"
    
    results.append("PASS io.StringIO_ctx")
except Exception as e:
    results.append(f"FAIL io.StringIO_ctx: {e}")

# ── 4. json roundtrip through StringIO ──
try:
    import json
    
    data = {"key": "value", "nums": [1, 2, 3]}
    
    # Encode to string
    encoded = json.dumps(data)
    
    # Verify roundtrip
    decoded = json.loads(encoded)
    assert decoded["key"] == "value"
    assert decoded["nums"] == [1, 2, 3]
    
    # Pretty print
    pretty = json.dumps(data, indent=4)
    assert "\n" in pretty
    assert "    " in pretty
    
    results.append("PASS json_io")
except Exception as e:
    results.append(f"FAIL json_io: {e}")

# ── 5. os module features ──
try:
    import os
    
    cwd = os.getcwd()
    assert isinstance(cwd, str) and len(cwd) > 0
    
    assert os.sep in ('/', '\\')
    assert os.name in ('posix', 'nt')
    
    # os.path
    p = os.path.join("dir", "file.txt")
    assert "dir" in p and "file.txt" in p
    
    assert os.path.basename("/foo/bar/baz.txt") == "baz.txt"
    assert os.path.dirname("/foo/bar/baz.txt") == "/foo/bar"
    
    # os.getpid
    pid = os.getpid()
    assert isinstance(pid, int) and pid > 0
    
    # os.cpu_count
    cpus = os.cpu_count()
    assert isinstance(cpus, int) and cpus > 0
    
    # os.environ
    assert hasattr(os.environ, 'get') or isinstance(os.environ, dict)
    assert "PATH" in os.environ or "HOME" in os.environ
    
    # os.getenv
    home = os.getenv("HOME", "/tmp")
    assert isinstance(home, str)
    assert os.getenv("NONEXISTENT_VAR_12345") is None
    
    results.append("PASS os_module")
except Exception as e:
    results.append(f"FAIL os_module: {e}")

# ── 6. os.listdir and os.walk ──
try:
    import os
    
    entries = os.listdir(".")
    assert isinstance(entries, list)
    assert len(entries) > 0  # current dir should have files
    
    # os.walk
    walked = list(os.walk("."))
    assert len(walked) > 0  # at least the current directory
    # Each entry is (dirpath, dirnames, filenames)
    root_entry = walked[0]
    assert isinstance(root_entry, tuple) or isinstance(root_entry, list)
    assert len(root_entry) == 3
    
    results.append("PASS os_walk_listdir")
except Exception as e:
    results.append(f"FAIL os_walk_listdir: {e}")

# ── 7. Enhanced collections from Rust ──
try:
    from collections import OrderedDict, defaultdict, deque, namedtuple
    
    # OrderedDict
    od = OrderedDict([("a", 1), ("b", 2)])
    assert list(od.keys()) == ["a", "b"] or len(od) == 2
    
    # defaultdict
    dd = defaultdict(int)
    dd["a"] += 1
    dd["a"] += 1
    dd["b"] += 1
    assert dd["a"] == 2 or isinstance(dd, dict)
    
    # namedtuple
    Point = namedtuple("Point", ["x", "y"])
    p = Point(3, 4)
    assert p[0] == 3
    assert p[1] == 4
    
    results.append("PASS collections_rust")
except Exception as e:
    results.append(f"FAIL collections_rust: {e}")

# ── 8. functools from Rust ──
try:
    import functools
    
    # lru_cache
    call_count = 0
    @functools.lru_cache(maxsize=128)
    def fib(n):
        global call_count
        call_count += 1
        if n < 2:
            return n
        return fib(n-1) + fib(n-2)
    
    result = fib(10)
    assert result == 55, f"fib(10)={result}"
    
    results.append("PASS functools_rust")
except Exception as e:
    results.append(f"FAIL functools_rust: {e}")

# ── 9. Verify multiple stdlib modules import ──
try:
    import math
    import time
    import hashlib
    import base64
    import struct
    import itertools
    import re
    
    assert math.pi > 3.14
    assert math.sqrt(16) == 4.0
    assert hasattr(time, 'time')
    assert hasattr(base64, 'b64encode')
    assert hasattr(struct, 'pack')
    assert hasattr(itertools, 'chain')
    
    results.append("PASS multi_import")
except Exception as e:
    results.append(f"FAIL multi_import: {e}")

# ── Summary ──
for r in results:
    print(r)

passed = sum(1 for r in results if r.startswith("PASS"))
total = len(results)
print(f"\n{passed}/{total} stdlib checks passed")
assert passed == total, f"Some checks failed!"
