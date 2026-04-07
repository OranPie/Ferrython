# test_phase127.py — typing.NamedTuple callable, comprehensive stdlib depth validation

import typing

# ── typing.NamedTuple as function call ──
# List of tuples form
Point = typing.NamedTuple('Point', [('x', int), ('y', int)])
p = Point(1, 2)
assert p.x == 1 and p.y == 2
assert p[0] == 1 and p[1] == 2
assert p._fields == ('x', 'y')

# kwargs form
Color = typing.NamedTuple('Color', r=int, g=int, b=int)
c = Color(255, 128, 0)
assert c.r == 255 and c.g == 128 and c.b == 0
assert c[0] == 255

# class-style (still works)
class Person(typing.NamedTuple):
    name: str
    age: int

person = Person('Alice', 30)
assert person.name == 'Alice' and person.age == 30
assert person[0] == 'Alice'

# ── email.mime multipart serialization ──
from email.mime.text import MIMEText
from email.mime.multipart import MIMEMultipart

msg = MIMEMultipart()
msg['Subject'] = 'Test'
msg['From'] = 'a@b.com'

part = MIMEText('Hello', 'plain')
msg.attach(part)

s = str(msg)
assert 'boundary=' in s
assert 'Hello' in s
assert 'Content-Type: multipart/mixed' in s

# ── sys module attributes ──
import sys
assert isinstance(sys.hexversion, int)
assert sys.hexversion >= 0x030800f0
assert isinstance(sys.warnoptions, list)
assert isinstance(sys.path_importer_cache, dict)
assert callable(sys.displayhook)
assert callable(sys.breakpointhook)

# ── os waitpid / W* macros ──
import os
assert os.WNOHANG == 1
assert callable(os.WIFEXITED)
assert callable(os.WEXITSTATUS)
assert callable(os.WIFSIGNALED)
assert callable(os.WTERMSIG)
assert os.WIFEXITED(0) == True
assert os.WEXITSTATUS(0) == 0
assert os.WIFEXITED(256) == True
assert os.WEXITSTATUS(256) == 1

# ── datetime tzinfo default ──
import datetime
dt = datetime.datetime.now()
assert dt.tzinfo is None

# ── comprehensive stdlib depth checks ──
# These verify the deep functional correctness across many modules

# collections
from collections import OrderedDict, Counter, deque, namedtuple
od = OrderedDict([('a', 1), ('b', 2)])
od.move_to_end('a')
assert list(od.keys()) == ['b', 'a']

c = Counter('abracadabra')
assert c['a'] == 5
assert c.most_common(1)[0] == ('a', 5)

d = deque([1, 2, 3])
d.appendleft(0)
assert list(d) == [0, 1, 2, 3]

# itertools
import itertools
assert list(itertools.chain.from_iterable([[1, 2], [3, 4]])) == [1, 2, 3, 4]
assert list(itertools.repeat(42, 3)) == [42, 42, 42]
assert list(itertools.takewhile(lambda x: x < 5, [1, 3, 5, 2])) == [1, 3]

# functools
import functools
@functools.lru_cache(maxsize=16)
def fib(n):
    if n < 2: return n
    return fib(n-1) + fib(n-2)
assert fib(20) == 6765

# contextlib
import contextlib
@contextlib.contextmanager
def my_ctx():
    yield 42
with my_ctx() as v:
    assert v == 42

# operator
import operator
assert operator.itemgetter(1)([10, 20, 30]) == 20
assert operator.attrgetter('real')(complex(3, 4)) == 3.0

# struct
import struct
packed = struct.pack('!I', 0x01020304)
assert packed == b'\x01\x02\x03\x04'

# json
import json
assert json.loads(json.dumps({'a': [1, 2.5, True, None]})) == {'a': [1, 2.5, True, None]}

# re
import re
m = re.match(r'(?P<name>\w+)', 'hello')
assert m.group('name') == 'hello'

# pathlib
from pathlib import Path
p = Path('/usr/local/bin')
assert p.parts == ('/', 'usr', 'local', 'bin')
assert p.name == 'bin'

print("test_phase127 passed")
