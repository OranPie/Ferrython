# test_phase111.py — io.open, re flags, module completeness

# ── io.open works ──
import io
import os
import tempfile

tmpdir = tempfile.gettempdir()
test_file = os.path.join(tmpdir, "_ferrython_io_test.txt")

# Write via io.open
f = io.open(test_file, "w")
f.write("hello io.open\nline 2\n")
f.close()

# Read via io.open
f2 = io.open(test_file, "r")
content = f2.read()
assert "hello io.open" in content
f2.close()

# Context manager
with io.open(test_file, "r") as f3:
    first_line = f3.readline()
    assert "hello" in first_line

# seek/tell
f4 = io.open(test_file, "r")
assert f4.tell() == 0
f4.seek(6)
assert f4.tell() == 6
rest = f4.read()
assert rest.startswith("io.open")
f4.close()

# Cleanup
os.remove(test_file)

# ── re flags completeness ──
import re

assert re.UNICODE == 32
assert re.U == 32
assert re.ASCII == 256
assert re.A == 256
assert re.LOCALE == 4
assert re.L == 4
assert re.TEMPLATE == 1
assert re.T == 1
assert hasattr(re, 'purge')
assert hasattr(re, 'error')

# re.purge doesn't crash
re.purge()

# Compile with flags
p = re.compile(r"hello", re.IGNORECASE)
m = p.match("HELLO world")
assert m is not None

# ── Verify key module attributes ──
import collections
assert hasattr(collections, 'OrderedDict')
assert hasattr(collections, 'defaultdict')
assert hasattr(collections, 'deque')
assert hasattr(collections, 'Counter')
assert hasattr(collections, 'namedtuple')
assert hasattr(collections, 'ChainMap')

import functools
assert hasattr(functools, 'reduce')
assert hasattr(functools, 'partial')
assert hasattr(functools, 'lru_cache')
assert hasattr(functools, 'wraps')
assert hasattr(functools, 'total_ordering')
assert hasattr(functools, 'singledispatch')
assert hasattr(functools, 'cached_property')

import itertools
assert hasattr(itertools, 'chain')
assert hasattr(itertools, 'count')
assert hasattr(itertools, 'combinations')
assert hasattr(itertools, 'permutations')
assert hasattr(itertools, 'product')
assert hasattr(itertools, 'groupby')
assert hasattr(itertools, 'starmap')
assert hasattr(itertools, 'pairwise')
assert hasattr(itertools, 'batched')

print("phase111: all tests passed")
