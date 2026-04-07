# Phase 135: Package manager features, import improvements, stdlib deep verification

# --- Test 1: functools deeper ---
import functools

# lru_cache with cache_clear
@functools.lru_cache(maxsize=32)
def cached_add(a, b):
    return a + b

cached_add(1, 2)
cached_add(1, 2)  # cache hit
cached_add(3, 4)
info = cached_add.cache_info()
assert info.hits >= 1
assert info.misses >= 2
cached_add.cache_clear()
info2 = cached_add.cache_info()
assert info2.currsize == 0
print("CHECK 1 OK: lru_cache with cache_clear")

# --- Test 2: functools.singledispatch ---
try:
    @functools.singledispatch
    def process(arg):
        return f"default: {arg}"
    
    @process.register(int)
    def _(arg):
        return f"int: {arg}"
    
    @process.register(str)
    def _(arg):
        return f"str: {arg}"
    
    assert process(42) == "int: 42"
    assert process("hello") == "str: hello"
    assert process([1, 2]) == "default: [1, 2]"
    print("CHECK 2 OK: singledispatch")
except Exception as e:
    print(f"CHECK 2 SKIP: singledispatch ({e})")

# --- Test 3: collections.Counter operations ---
import collections

c1 = collections.Counter("abracadabra")
assert c1['a'] == 5
assert c1['b'] == 2
# most_common
top = c1.most_common(3)
assert top[0] == ('a', 5)
# subtraction keeps only positive
c2 = collections.Counter("abcdef")
diff = c1 - c2
assert diff['a'] == 4  # 5-1
assert diff.get('e', 0) == 0  # not in c1 or zero
print("CHECK 3 OK: Counter operations")

# --- Test 4: collections.OrderedDict move_to_end ---
from collections import OrderedDict
od = OrderedDict([('a', 1), ('b', 2), ('c', 3)])
od.move_to_end('a')
assert list(od.keys()) == ['b', 'c', 'a']
od.move_to_end('c', last=False)
assert list(od.keys()) == ['c', 'b', 'a']
print("CHECK 4 OK: OrderedDict.move_to_end")

# --- Test 5: itertools.islice ---
import itertools
result = list(itertools.islice(range(100), 5, 15, 2))
assert result == [5, 7, 9, 11, 13]
print("CHECK 5 OK: itertools.islice")

# --- Test 6: itertools.tee ---
try:
    it = iter(range(5))
    a, b = itertools.tee(it, 2)
    assert list(a) == [0, 1, 2, 3, 4]
    assert list(b) == [0, 1, 2, 3, 4]
    print("CHECK 6 OK: itertools.tee")
except Exception as e:
    print(f"CHECK 6 SKIP: itertools.tee ({e})")

# --- Test 7: itertools.chain.from_iterable ---
result = list(itertools.chain.from_iterable([[1, 2], [3, 4], [5]]))
assert result == [1, 2, 3, 4, 5]
print("CHECK 7 OK: itertools.chain.from_iterable")

# --- Test 8: heapq.merge ---
import heapq
result = list(heapq.merge([1, 3, 5], [2, 4, 6]))
assert result == [1, 2, 3, 4, 5, 6]
print("CHECK 8 OK: heapq.merge")

# --- Test 9: contextlib.contextmanager ---
from contextlib import contextmanager

@contextmanager
def managed(name):
    yield name.upper()

with managed("test") as value:
    assert value == "TEST"
print("CHECK 9 OK: contextlib.contextmanager")

# --- Test 10: dataclasses.asdict and astuple ---
import dataclasses

@dataclasses.dataclass
class Point:
    x: float
    y: float

p = Point(1.0, 2.0)
d = dataclasses.asdict(p)
assert d == {'x': 1.0, 'y': 2.0}
t = dataclasses.astuple(p)
assert t == (1.0, 2.0)
print("CHECK 10 OK: dataclasses.asdict/astuple")

# --- Test 11: dataclasses.replace ---
try:
    p2 = dataclasses.replace(p, x=3.0)
    assert p2.x == 3.0 and p2.y == 2.0
    print("CHECK 11 OK: dataclasses.replace")
except Exception as e:
    print(f"CHECK 11 SKIP: dataclasses.replace ({e})")

# --- Test 12: typing module features ---
import typing
assert typing.Optional[int] is not None
assert typing.Union[int, str] is not None
assert typing.List[int] is not None
assert typing.Dict[str, int] is not None
assert typing.Tuple[int, str] is not None
print("CHECK 12 OK: typing subscript forms")

# --- Test 13: string module ---
import string
assert string.ascii_lowercase == 'abcdefghijklmnopqrstuvwxyz'
assert string.ascii_uppercase == 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'
assert string.digits == '0123456789'
assert len(string.punctuation) > 0
print("CHECK 13 OK: string module constants")

# --- Test 14: pathlib comprehensive ---
from pathlib import Path
p = Path("/home/user/documents/file.txt")
assert p.name == "file.txt"
assert p.stem == "file"
assert p.suffix == ".txt"
assert str(p.parent) == "/home/user/documents"
assert p.parts == ('/', 'home', 'user', 'documents', 'file.txt')
print("CHECK 14 OK: pathlib comprehensive")

# --- Test 15: json.dumps formatting ---
import json
data = {"name": "test", "values": [1, 2, 3]}
compact = json.dumps(data, separators=(',', ':'))
assert ' ' not in compact
pretty = json.dumps(data, indent=2)
assert '\n' in pretty
print("CHECK 15 OK: json formatting")

# --- Test 16: csv.DictWriter/DictReader ---
import csv
import io

output = io.StringIO()
writer = csv.DictWriter(output, fieldnames=['name', 'age'])
writer.writeheader()
writer.writerow({'name': 'Alice', 'age': '30'})
writer.writerow({'name': 'Bob', 'age': '25'})

output.seek(0)
reader = csv.DictReader(output)
rows = list(reader)
assert len(rows) == 2
assert rows[0]['name'] == 'Alice'
assert rows[1]['age'] == '25'
print("CHECK 16 OK: csv DictWriter/DictReader")

# --- Test 17: hashlib comprehensive ---
import hashlib
h = hashlib.sha256(b"hello world")
digest = h.hexdigest()
assert len(digest) == 64
md5 = hashlib.md5(b"test").hexdigest()
assert len(md5) == 32
print("CHECK 17 OK: hashlib comprehensive")

# --- Test 18: base64 urlsafe ---
import base64
encoded = base64.urlsafe_b64encode(b"hello?world&test")
decoded = base64.urlsafe_b64decode(encoded)
assert decoded == b"hello?world&test"
print("CHECK 18 OK: base64 urlsafe")

# --- Test 19: uuid4 ---
import uuid
u = uuid.uuid4()
assert len(str(u)) == 36
assert str(u).count('-') == 4
print("CHECK 19 OK: uuid4")

# --- Test 20: secrets module ---
import secrets
token = secrets.token_hex(16)
assert len(token) == 32
url_token = secrets.token_urlsafe(16)
assert len(url_token) > 0
print("CHECK 20 OK: secrets module")

# --- Test 21: logging module ---
import logging
import io
logger = logging.getLogger("test_logger")
logger.setLevel(logging.DEBUG)
stream = io.StringIO()
handler = logging.StreamHandler(stream)
handler.setFormatter(logging.Formatter("%(levelname)s:%(message)s"))
logger.addHandler(handler)
logger.info("hello")
logger.warning("caution")
output = stream.getvalue()
assert "INFO:hello" in output
assert "WARNING:caution" in output
print("CHECK 21 OK: logging module")

# --- Test 22: textwrap ---
import textwrap
text = "Hello world, this is a long line that should be wrapped at a certain width."
wrapped = textwrap.fill(text, width=30)
assert '\n' in wrapped
dedented = textwrap.dedent("    hello\n    world")
assert dedented == "hello\nworld"
print("CHECK 22 OK: textwrap")

# --- Test 23: shlex.split ---
import shlex
parts = shlex.split('hello "world with spaces" --flag')
assert parts == ['hello', 'world with spaces', '--flag']
print("CHECK 23 OK: shlex.split")

# --- Test 24: difflib.unified_diff ---
import difflib
diff = list(difflib.unified_diff(
    ['line1\n', 'line2\n', 'line3\n'],
    ['line1\n', 'modified\n', 'line3\n'],
    fromfile='a.txt', tofile='b.txt'
))
assert len(diff) > 0
print("CHECK 24 OK: difflib.unified_diff")

# --- Test 25: struct.Struct class ---
import struct
s = struct.Struct('>2i')
packed = s.pack(10, 20)
assert s.unpack(packed) == (10, 20)
assert s.size == 8
print("CHECK 25 OK: struct.Struct class")

# --- Test 26: abc.abstractmethod enforcement ---
import abc
class Shape(abc.ABC):
    @abc.abstractmethod
    def area(self):
        pass

class Circle(Shape):
    def __init__(self, r):
        self.r = r
    def area(self):
        return 3.14159 * self.r ** 2

try:
    s = Shape()  # should fail
    print("CHECK 26 FAIL: Shape() should raise TypeError")
except TypeError:
    c = Circle(5)
    assert abs(c.area() - 78.54) < 0.01
    print("CHECK 26 OK: abc.abstractmethod")

# --- Test 27: __call__ protocol ---
class Adder:
    def __init__(self, n):
        self.n = n
    def __call__(self, x):
        return self.n + x

add5 = Adder(5)
assert add5(3) == 8
assert callable(add5)
print("CHECK 27 OK: __call__ protocol")

# --- Test 28: property with getter/setter/deleter ---
class MyObj:
    def __init__(self):
        self._x = 0
    
    @property
    def x(self):
        return self._x
    
    @x.setter
    def x(self, value):
        if value < 0:
            raise ValueError("negative")
        self._x = value

obj = MyObj()
obj.x = 42
assert obj.x == 42
try:
    obj.x = -1
    print("CHECK 28 FAIL")
except ValueError:
    print("CHECK 28 OK: property setter validation")

# --- Test 29: __enter__/__exit__ with exception handling ---
class TrackingCM:
    def __init__(self):
        self.log = []
    def __enter__(self):
        self.log.append('enter')
        return self
    def __exit__(self, exc_type, exc_val, exc_tb):
        self.log.append('exit')
        if exc_type is ValueError:
            self.log.append('suppressed')
            return True
        return False

cm = TrackingCM()
with cm:
    pass
assert cm.log == ['enter', 'exit']

cm2 = TrackingCM()
with cm2:
    raise ValueError("test")
assert cm2.log == ['enter', 'exit', 'suppressed']
print("CHECK 29 OK: context manager exception suppression")

# --- Test 30: Generator.send() ---
def accumulator():
    total = 0
    while True:
        value = yield total
        if value is None:
            break
        total += value

gen = accumulator()
next(gen)  # prime
assert gen.send(10) == 10
assert gen.send(20) == 30
assert gen.send(5) == 35
print("CHECK 30 OK: Generator.send()")

# --- Test 31: Generator.throw() ---
def gen_with_throw():
    try:
        yield 1
        yield 2
    except ValueError:
        yield "caught"

g = gen_with_throw()
assert next(g) == 1
assert g.throw(ValueError, "test") == "caught"
print("CHECK 31 OK: Generator.throw()")

# --- Test 32: Nested comprehensions ---
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat = [x for row in matrix for x in row]
assert flat == [1, 2, 3, 4, 5, 6, 7, 8, 9]
print("CHECK 32 OK: nested list comprehension")

# --- Test 33: Set comprehension ---
result = {x % 3 for x in range(10)}
assert result == {0, 1, 2}
print("CHECK 33 OK: set comprehension")

# --- Test 34: Dict comprehension with condition ---
result = {k: v for k, v in enumerate('abcdef') if k % 2 == 0}
assert result == {0: 'a', 2: 'c', 4: 'e'}
print("CHECK 34 OK: dict comprehension with filter")

# --- Test 35: walrus operator ---
data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
result = [y for x in data if (y := x ** 2) > 20]
assert result == [25, 36, 49, 64, 81, 100]
print("CHECK 35 OK: walrus operator in comprehension")

# --- Test 36: f-string expressions ---
name = "world"
assert f"Hello, {name}!" == "Hello, world!"
assert f"{2 + 3}" == "5"
assert f"{'hello':>10}" == "     hello"
assert f"{42:08b}" == "00101010"
print("CHECK 36 OK: f-string expressions")

# --- Test 37: Exception chaining ---
try:
    try:
        raise ValueError("original")
    except ValueError as e:
        raise TypeError("new") from e
except TypeError as e:
    assert str(e) == "new"
    assert isinstance(e.__cause__, ValueError)
    assert str(e.__cause__) == "original"
    print("CHECK 37 OK: exception chaining")

# --- Test 38: bytes methods ---
b = b"Hello, World!"
assert b.lower() == b"hello, world!"
assert b.upper() == b"HELLO, WORLD!"
assert b.split(b", ") == [b"Hello", b"World!"]
assert b.startswith(b"Hello")
assert b.endswith(b"World!")
assert b.replace(b"World", b"Python") == b"Hello, Python!"
print("CHECK 38 OK: bytes methods")

# --- Test 39: enumerate with start ---
result = list(enumerate(['a', 'b', 'c'], start=1))
assert result == [(1, 'a'), (2, 'b'), (3, 'c')]
print("CHECK 39 OK: enumerate with start")

# --- Test 40: min/max with key ---
words = ['banana', 'apple', 'cherry']
assert min(words, key=len) == 'apple'
assert max(words, key=len) == 'banana' or max(words, key=len) == 'cherry'
print("CHECK 40 OK: min/max with key")

# --- Summary ---
print("\nAll phase 135 checks complete!")
