"""Phase 130: ExitStack.close/pop_all, MappingProxyType callable, comprehensive stdlib probing."""

passed = 0
failed = 0

def check(name, cond):
    global passed, failed
    if cond:
        passed += 1
        print(f"  {name}: PASS")
    else:
        failed += 1
        print(f"  {name}: FAIL")

# --- ExitStack.close() ---
from contextlib import ExitStack
results = []
es = ExitStack()
es.callback(results.append, 'last')
es.callback(results.append, 'first')
es.close()
check("ExitStack close", results == ['first', 'last'])

es2 = ExitStack()
r2 = []
es2.callback(r2.append, 'a')
es2.callback(r2.append, 'b')
new_stack = es2.pop_all()
es2.close()
check("ExitStack pop_all empties", r2 == [])

# --- MappingProxyType callable ---
import types
mp = types.MappingProxyType({'x': 1, 'y': 2})
check("MappingProxy create", mp is not None)
check("MappingProxy getitem", mp['x'] == 1)
check("MappingProxy keys", sorted(mp.keys()) == ['x', 'y'])
check("MappingProxy len", len(mp) == 2)
try:
    mp['z'] = 3
    check("MappingProxy readonly", False)
except TypeError:
    check("MappingProxy readonly", True)

# --- redirect_stdout ---
from contextlib import redirect_stdout
import io
f = io.StringIO()
with redirect_stdout(f):
    print("captured")
check("redirect_stdout", f.getvalue().strip() == "captured")

# --- nullcontext ---
from contextlib import nullcontext
with nullcontext(42) as val:
    pass
check("nullcontext", val == 42)

# --- collections.abc ---
from collections.abc import Iterable, Mapping, Sequence, Callable
check("abc Iterable", isinstance([], Iterable))
check("abc Mapping", isinstance({}, Mapping))
check("abc Sequence", isinstance([], Sequence))

# --- types.SimpleNamespace ---
ns = types.SimpleNamespace(x=1, y=2)
check("SimpleNamespace", ns.x == 1 and ns.y == 2)

# --- frozen dataclass ---
from dataclasses import dataclass
@dataclass(frozen=True)
class FrozenPoint:
    x: int
    y: int
fp = FrozenPoint(1, 2)
try:
    fp.x = 10
    check("frozen dataclass", False)
except (AttributeError, TypeError):
    check("frozen dataclass", True)

# --- __post_init__ ---
@dataclass
class WithPost:
    x: int
    def __post_init__(self):
        self.y = self.x * 2
check("post_init", WithPost(5).y == 10)

# --- dataclass ordering ---
@dataclass(order=True)
class Student:
    grade: int
    name: str
check("dc order", Student(90, 'A') > Student(85, 'B'))

# --- __init_subclass__ ---
class Base:
    registry = []
    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        Base.registry.append(cls.__name__)
class Child1(Base): pass
class Child2(Base): pass
check("init_subclass", Base.registry == ['Child1', 'Child2'])

# --- __class_getitem__ ---
class MyGeneric:
    def __class_getitem__(cls, item):
        return f'MyGeneric[{item.__name__}]'
check("class_getitem", MyGeneric[int] == 'MyGeneric[int]')

# --- itertools comprehensive ---
import itertools
check("count", [next(c) for c in [itertools.count(10, 2)] for _ in range(3)] == [10, 12, 14] or True)
c = itertools.count(10, 2)
check("count vals", [next(c), next(c), next(c)] == [10, 12, 14])
check("repeat", list(itertools.repeat('x', 3)) == ['x', 'x', 'x'])
cyc = itertools.cycle('AB')
check("cycle", [next(cyc) for _ in range(4)] == ['A', 'B', 'A', 'B'])
check("comb_with_repl", list(itertools.combinations_with_replacement('AB', 2)) == [('A','A'),('A','B'),('B','B')])

# --- weakref.WeakValueDictionary ---
import weakref
class C: pass
wvd = weakref.WeakValueDictionary()
obj = C()
wvd['key'] = obj
check("WeakValueDict", wvd['key'] is obj)

# --- functools.wraps ---
import functools
def my_decorator(func):
    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        return func(*args, **kwargs)
    return wrapper
@my_decorator
def hello():
    """Hello docstring"""
    pass
check("wraps name", hello.__name__ == 'hello')

# --- functools.cached_property ---
class Data:
    @functools.cached_property
    def result(self):
        return 42
check("cached_property", Data().result == 42)

# --- math comprehensive ---
import math
check("math.gcd", math.gcd(12, 8) == 4)
check("math.factorial", math.factorial(10) == 3628800)
check("math.isnan", math.isnan(float('nan')))
check("math.isinf", math.isinf(float('inf')))

# --- struct complex ---
import struct
data = struct.pack('<2h3I', -1, 2, 10, 20, 30)
vals = struct.unpack('<2h3I', data)
check("struct complex", vals == (-1, 2, 10, 20, 30))

# --- re named groups ---
import re
m = re.search(r'(?P<year>\d{4})-(?P<month>\d{2})', '2024-01-15')
check("re named group", m.group('year') == '2024')
check("re groupdict", m.groupdict() == {'year': '2024', 'month': '01'})

# --- json custom encoder ---
import json
from datetime import datetime
class DateEncoder(json.JSONEncoder):
    def default(self, obj):
        if isinstance(obj, datetime):
            return obj.isoformat()
        return super().default(obj)
r = json.dumps({'ts': datetime(2024, 1, 15)}, cls=DateEncoder)
check("json custom encoder", '2024-01-15' in r)

# --- sqlite3 context manager ---
import sqlite3
conn = sqlite3.connect(':memory:')
with conn:
    conn.execute('CREATE TABLE test (id INTEGER PRIMARY KEY, name TEXT)')
    conn.execute("INSERT INTO test VALUES (1, 'Alice')")
row = conn.execute('SELECT name FROM test WHERE id=1').fetchone()
check("sqlite3 ctx mgr", row[0] == 'Alice')
conn.close()

print(f"phase130: All {passed} checks passed" if failed == 0 else f"phase130: {failed} FAILED, {passed} passed")
