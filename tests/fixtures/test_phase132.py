# Phase 132: tarfile.getmember, socket.getsockname port-0, deep stdlib verification
import sys

checks = []

# 1. tarfile.getmember
import tarfile, tempfile, os
td = tempfile.mkdtemp()
f1 = os.path.join(td, 'test.txt')
with open(f1, 'w') as f:
    f.write('hello tar')
tar_path = os.path.join(td, 'test.tar.gz')
with tarfile.open(tar_path, 'w:gz') as tar:
    tar.add(f1, arcname='test.txt')
with tarfile.open(tar_path, 'r:gz') as tar:
    m = tar.getmember('test.txt')
    checks.append(("tarfile_getmember", m.name == 'test.txt' and m.size == 9))
    checks.append(("tarfile_isfile", m.isfile() == True))
    checks.append(("tarfile_isdir", m.isdir() == False))
    names = tar.getnames()
    checks.append(("tarfile_getnames", names == ['test.txt']))
import shutil
shutil.rmtree(td)

# 2. socket.getsockname after listen with port 0
import socket
s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.bind(('127.0.0.1', 0))
s.listen(1)
name = s.getsockname()
checks.append(("socket_getsockname_host", name[0] == '127.0.0.1'))
checks.append(("socket_getsockname_port", name[1] > 0))
s.close()

# 3. UDP getsockname
u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
u.bind(('127.0.0.1', 0))
uname = u.getsockname()
checks.append(("udp_getsockname", uname[1] > 0))
u.close()

# 4. struct calcsize
import struct
checks.append(("struct_calcsize_i", struct.calcsize('i') == 4))
checks.append(("struct_calcsize_3h", struct.calcsize('3h') == 6))
packed = struct.pack('>f', 3.14)
(val,) = struct.unpack('>f', packed)
checks.append(("struct_float", abs(val - 3.14) < 0.01))

# 5. array operations
import array
a = array.array('i', [3, 1, 4, 1, 5])
a.reverse()
checks.append(("array_reverse", list(a) == [5, 1, 4, 1, 3]))
checks.append(("array_tobytes", len(a.tobytes()) == 20))
b = array.array('i')
b.frombytes(a.tobytes())
checks.append(("array_frombytes", list(b) == list(a)))

# 6. namedtuple deeper
from collections import namedtuple
Point = namedtuple('Point', ['x', 'y'])
p = Point(1, 2)
checks.append(("namedtuple_asdict", p._asdict() == {'x': 1, 'y': 2}))
checks.append(("namedtuple_replace", p._replace(x=10).x == 10))
checks.append(("namedtuple_fields", p._fields == ('x', 'y')))

# 7. random with seed
import random
random.seed(42)
r1 = random.random()
random.seed(42)
r2 = random.random()
checks.append(("random_seed", r1 == r2))
checks.append(("random_range", 0 <= r1 < 1))
checks.append(("random_choice", random.choice([1, 2, 3]) in [1, 2, 3]))
checks.append(("random_sample", len(random.sample(range(10), 3)) == 3))

# 8. time module
import time
checks.append(("time_monotonic", time.monotonic() > 0))
checks.append(("time_perf_counter", time.perf_counter() > 0))

# 9. ast deeper
import ast
tree = ast.parse('x = 1 + 2')
checks.append(("ast_parse", type(tree).__name__ == 'Module'))
nodes = list(ast.walk(tree))
node_types = {type(n).__name__ for n in nodes}
checks.append(("ast_walk", 'Assign' in node_types and 'BinOp' in node_types))
checks.append(("ast_literal_eval", ast.literal_eval('[1, 2, 3]') == [1, 2, 3]))

# 10. inspect.signature
import inspect
def func(a, b=10, *args, **kwargs): pass
sig = inspect.signature(func)
params = list(sig.parameters.keys())
checks.append(("inspect_sig_params", 'a' in params and 'b' in params))

# 11. gzip round-trip
import gzip
data = b'hello world' * 50
compressed = gzip.compress(data)
checks.append(("gzip_compress", len(compressed) < len(data)))
checks.append(("gzip_decompress", gzip.decompress(compressed) == data))

# 12. bz2 round-trip
import bz2
compressed = bz2.compress(data)
checks.append(("bz2_roundtrip", bz2.decompress(compressed) == data))

# 13. lzma round-trip
import lzma
compressed = lzma.compress(data)
checks.append(("lzma_roundtrip", lzma.decompress(compressed) == data))

# 14. configparser
import configparser
cp = configparser.ConfigParser()
cp.read_string('[s]\nhost=localhost\nport=8080\n')
checks.append(("configparser_get", cp.get('s', 'host') == 'localhost'))
checks.append(("configparser_getint", cp.getint('s', 'port') == 8080))

# 15. shelve round-trip
import shelve
db_path = tempfile.mktemp()
with shelve.open(db_path) as db:
    db['test'] = [1, 2, 3]
with shelve.open(db_path) as db:
    checks.append(("shelve_roundtrip", db['test'] == [1, 2, 3]))
for ext in ['', '.db', '.dir', '.bak', '.dat']:
    try: os.unlink(db_path + ext)
    except: pass

# 16. concurrent.futures
import concurrent.futures
with concurrent.futures.ThreadPoolExecutor(max_workers=2) as ex:
    results = list(ex.map(lambda x: x*x, range(5)))
checks.append(("threadpool_map", results == [0, 1, 4, 9, 16]))

# 17. multiprocessing.Pool
import multiprocessing
pool = multiprocessing.Pool(2)
results = pool.map(lambda x: x*x, [1, 2, 3])
pool.close()
pool.join()
checks.append(("mp_pool_map", results == [1, 4, 9]))

# 18. ExceptionGroup
try:
    eg = ExceptionGroup('test', [ValueError('a'), TypeError('b')])
    checks.append(("exception_group", len(eg.exceptions) == 2))
except NameError:
    checks.append(("exception_group", False))

# 19. dict | merge (Python 3.9+)
d = {'a': 1} | {'b': 2}
checks.append(("dict_merge_op", d == {'a': 1, 'b': 2}))

# 20. str.removeprefix / removesuffix (Python 3.9+)
checks.append(("removeprefix", 'HelloWorld'.removeprefix('Hello') == 'World'))
checks.append(("removesuffix", 'HelloWorld'.removesuffix('World') == 'Hello'))

# 21. f-string = debug
x = 42
checks.append(("fstring_debug", f'{x=}' == 'x=42'))

# 22. os.urandom
checks.append(("os_urandom", len(os.urandom(16)) == 16))

# 23. hashlib.pbkdf2_hmac
import hashlib
dk = hashlib.pbkdf2_hmac('sha256', b'pw', b'salt', 1000)
checks.append(("pbkdf2_hmac", len(dk) == 32))

# 24. hmac
import hmac
h = hmac.new(b'key', b'msg', hashlib.sha256)
checks.append(("hmac_digest", len(h.hexdigest()) == 64))

# 25. secrets
import secrets
checks.append(("secrets_token", len(secrets.token_hex(16)) == 32))

# ── report ──
passed = sum(1 for _, v in checks if v)
failed = [(n, v) for n, v in checks if not v]
print(f"phase132: {passed}/{len(checks)} passed")
for name, _ in failed:
    print(f"  FAIL: {name}")
if failed:
    sys.exit(1)
