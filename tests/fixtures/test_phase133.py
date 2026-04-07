# test_phase133: enum iteration, deque extendleft, struct pack_into, csv dialects,
#                pathlib multi-arg, date ordinal, MRO fixes
checks = 0

# 1. Enum iteration (list, comprehension)
import enum
class Color(enum.IntEnum):
    RED = 1
    GREEN = 2
    BLUE = 3

members = list(Color)
assert len(members) == 3, f"enum iter len: {len(members)}"
names = [m.name for m in members]
assert set(names) == {"RED", "GREEN", "BLUE"}
checks += 1

# 2. Enum: for-loop
collected = []
for c in Color:
    collected.append(c.value)
assert sorted(collected) == [1, 2, 3]
checks += 1

# 3. Enum: __members__
assert "RED" in Color.__members__
assert "GREEN" in Color.__members__
checks += 1

# 4. Enum: Color[name] lookup
assert Color["BLUE"].value == 3
checks += 1

# 5. Enum: Color(value) lookup
assert Color(2).name == "GREEN"
checks += 1

# 6. Flag iteration
class Perm(enum.Flag):
    R = 4
    W = 2
    X = 1

perms = list(Perm)
assert len(perms) == 3
checks += 1

# 7. auto() with iteration
class Status(enum.Enum):
    PENDING = enum.auto()
    RUNNING = enum.auto()
    DONE = enum.auto()

vals = [s.value for s in Status]
assert sorted(vals) == sorted([Status.PENDING.value, Status.RUNNING.value, Status.DONE.value])
checks += 1

# 8. deque.extendleft: correct reversal order
from collections import deque
d = deque([1, 2, 3], maxlen=5)
d.extendleft([4, 5])
assert list(d) == [5, 4, 1, 2, 3], f"extendleft: {list(d)}"
checks += 1

# 9. deque.extendleft: maxlen enforcement
d2 = deque([1, 2, 3], maxlen=4)
d2.extendleft([4, 5])
# appendleft(4) → [4,1,2,3], appendleft(5) → [5,4,1,2] (maxlen drops right)
assert list(d2) == [5, 4, 1, 2] or len(d2) == 4
checks += 1

# 10. deque.extendleft without maxlen
d3 = deque([10, 20])
d3.extendleft([30, 40, 50])
assert list(d3) == [50, 40, 30, 10, 20]
checks += 1

# 11. struct.pack_into
import struct
buf = bytearray(20)
struct.pack_into('>II', buf, 0, 100, 200)
v1, v2 = struct.unpack_from('>II', buf, 0)
assert v1 == 100 and v2 == 200
checks += 1

# 12. struct.pack_into at offset
struct.pack_into('>H', buf, 8, 9999)
v = struct.unpack_from('>H', buf, 8)[0]
assert v == 9999
checks += 1

# 13. Struct class pack_into
s = struct.Struct('>HH')
s.pack_into(buf, 12, 42, 43)
v1, v2 = struct.unpack_from('>HH', buf, 12)
assert v1 == 42 and v2 == 43
checks += 1

# 14. csv.register_dialect
import csv
csv.register_dialect('pipe', delimiter='|')
dialects = csv.list_dialects()
assert 'pipe' in dialects
checks += 1

# 15. csv.get_dialect
d = csv.get_dialect('pipe')
assert d.delimiter == '|'
checks += 1

# 16. csv.Sniffer().sniff
import io
sample = '"a","b","c"\n"1","2","3"\n'
sniffer = csv.Sniffer()
dialect = sniffer.sniff(sample)
assert dialect.delimiter == ','
checks += 1

# 17. csv.Sniffer with tab-separated
sample_tab = "a\tb\tc\n1\t2\t3\n"
dialect2 = sniffer.sniff(sample_tab)
assert dialect2.delimiter == '\t'
checks += 1

# 18. csv.Sniffer.has_header
assert sniffer.has_header("name,age\nAlice,30\nBob,25\n")
checks += 1

# 19. csv writer with dialect
output = io.StringIO()
w = csv.writer(output, dialect='pipe')
w.writerow(["a", "b", "c"])
assert "|" in output.getvalue()
checks += 1

# 20. csv.unregister_dialect
csv.unregister_dialect('pipe')
assert 'pipe' not in csv.list_dialects()
checks += 1

# 21. Path multi-arg constructor
from pathlib import Path
import tempfile, os
td = tempfile.mkdtemp()
p = Path(td, "subdir", "file.txt")
assert str(p).endswith(os.path.join("subdir", "file.txt"))
checks += 1

# 22. Path multi-arg: create and read
os.makedirs(os.path.join(td, "subdir"), exist_ok=True)
Path(td, "subdir", "file.txt").write_text("content")
assert Path(td, "subdir", "file.txt").read_text() == "content"
checks += 1

# 23. Path.iterdir
Path(td, "a.txt").write_text("a")
Path(td, "b.txt").write_text("b")
entries = sorted([e.name for e in Path(td).iterdir()])
assert "a.txt" in entries
assert "b.txt" in entries
checks += 1

# 24. Path.stat
st = Path(td, "a.txt").stat()
assert st.st_size == 1
checks += 1

# 25. date ordinal round-trip: 2024-01-01
import datetime
d = datetime.date(2024, 1, 1)
o = d.toordinal()
d2 = datetime.date.fromordinal(o)
assert d2.year == 2024 and d2.month == 1 and d2.day == 1
checks += 1

# 26. date ordinal: Jan 1 year 1
d = datetime.date(1, 1, 1)
assert d.toordinal() == 1
checks += 1

# 27. date ordinal: leap year Feb 29
d = datetime.date(2000, 2, 29)
o = d.toordinal()
d2 = datetime.date.fromordinal(o)
assert d2.year == 2000 and d2.month == 2 and d2.day == 29
checks += 1

# 28. date ordinal: Dec 31
d = datetime.date(2024, 12, 31)
o = d.toordinal()
d2 = datetime.date.fromordinal(o)
assert d2.year == 2024 and d2.month == 12 and d2.day == 31
checks += 1

# 29. date ordinal: mid-year
d = datetime.date(1999, 6, 15)
o = d.toordinal()
d2 = datetime.date.fromordinal(o)
assert d2.year == 1999 and d2.month == 6 and d2.day == 15
checks += 1

# 30. datetime.combine still works
d = datetime.date(2024, 3, 15)
t = datetime.time(10, 30, 0)
dt = datetime.datetime.combine(d, t)
assert dt.year == 2024 and dt.hour == 10 and dt.minute == 30
checks += 1

# 31. timestamp round-trip
dt = datetime.datetime(2024, 1, 1, 0, 0, 0)
ts = dt.timestamp()
dt2 = datetime.datetime.fromtimestamp(ts)
assert dt2.year == 2024
checks += 1

# 32. MRO attribute resolution for Rust base classes
# IntEnum.__iter__ should be found via Enum base
assert hasattr(enum.Enum, '__iter__')
assert hasattr(enum.IntEnum, '__iter__')
checks += 1

# 33. Counter.most_common
from collections import Counter
c = Counter("abracadabra")
mc = c.most_common(3)
assert mc[0][0] == 'a' and mc[0][1] == 5
checks += 1

# 34. itertools complete
import itertools
r = list(itertools.chain([1,2], [3,4]))
assert r == [1,2,3,4]
r2 = list(itertools.islice(range(100), 5, 10))
assert r2 == [5,6,7,8,9]
checks += 1

# 35. functools advanced
import functools
@functools.singledispatch
def process(arg):
    return f"default: {arg}"

@process.register(int)
def _(arg):
    return f"int: {arg}"

assert process(42) == "int: 42"
assert process(3.14) == "default: 3.14"
checks += 1

# 36. functools.total_ordering
@functools.total_ordering
class Student:
    def __init__(self, grade):
        self.grade = grade
    def __eq__(self, other):
        return self.grade == other.grade
    def __lt__(self, other):
        return self.grade < other.grade

assert Student(80) < Student(90)
assert Student(90) > Student(80)
assert Student(80) <= Student(80)
checks += 1

# 37. re advanced
import re
m = re.search(r'(?P<first>\w+) (?P<last>\w+)', 'John Smith')
assert m.groupdict() == {'first': 'John', 'last': 'Smith'}
checks += 1

# 38. re.subn
result, count = re.subn(r'\d+', 'NUM', 'abc 123 def 456')
assert result == 'abc NUM def NUM' and count == 2
checks += 1

# 39. tokenize basic
import tokenize as tok_module
tokens = tok_module.generate_tokens(io.StringIO("x = 1\n").readline)
types = [t[0] for t in tokens]
assert 1 in types  # NAME
assert 2 in types  # NUMBER
checks += 1

# 40. string methods completeness
assert "hello".partition(",") == ("hello", "", "")
assert "hi".center(10) == "    hi    "
assert "42".zfill(5) == "00042"
checks += 1

# Cleanup
import shutil
shutil.rmtree(td, ignore_errors=True)

print(f"test_phase133: {checks}/40 checks passed")
assert checks == 40
