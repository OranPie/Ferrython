# Phase 143: Binary I/O, shutil dir-dst, cProfile stream, patch.object, InstanceDict del

import sys

passed = 0
failed = 0

def check(name, cond):
    global passed, failed
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# 1. Binary file I/O preserves raw bytes
import tempfile, os
f = tempfile.mktemp()
with open(f, 'wb') as fh:
    fh.write(b'\x00\x01\x02\xff\xfe\x80')
with open(f, 'rb') as fh:
    data = fh.read()
check("binary write/read raw bytes", list(data) == [0, 1, 2, 255, 254, 128])
os.unlink(f)

# 2. Binary append mode
f = tempfile.mktemp()
with open(f, 'wb') as fh:
    fh.write(b'hello')
with open(f, 'ab') as fh:
    fh.write(b' world')
with open(f, 'rb') as fh:
    data = fh.read()
check("binary append mode", data == b'hello world')
os.unlink(f)

# 3. shutil.copy2 with directory destination
import shutil
d1 = tempfile.mkdtemp()
d2 = tempfile.mkdtemp()
with open(os.path.join(d1, 'test.txt'), 'w') as fh:
    fh.write('hello')
shutil.copy2(os.path.join(d1, 'test.txt'), d2)
check("shutil.copy2 dir dst", os.path.exists(os.path.join(d2, 'test.txt')))
shutil.rmtree(d1)
shutil.rmtree(d2)

# 4. shutil.copy with directory destination
d1 = tempfile.mkdtemp()
d2 = tempfile.mkdtemp()
with open(os.path.join(d1, 'a.txt'), 'w') as fh:
    fh.write('data')
shutil.copy(os.path.join(d1, 'a.txt'), d2)
check("shutil.copy dir dst", os.path.exists(os.path.join(d2, 'a.txt')))
shutil.rmtree(d1)
shutil.rmtree(d2)

# 5. cProfile.Profile print_stats to stream
import io, cProfile
pr = cProfile.Profile()
pr.enable()
sum(range(100))
pr.disable()
buf = io.StringIO()
pr.print_stats(buf)
out = buf.getvalue()
check("cProfile stream output", 'function calls' in out)

# 6. cProfile.Profile getstats
stats = pr.getstats()
check("cProfile getstats", isinstance(stats, list) and len(stats) > 0)

# 7. mock.patch.object
from unittest.mock import patch, MagicMock
class API:
    def fetch(self):
        return 'real'
api = API()
check("patch.object before", api.fetch() == 'real')
with patch.object(api, 'fetch', return_value='mocked') as m:
    check("patch.object during", api.fetch() == 'mocked')
check("patch.object after", api.fetch() == 'real')

# 8. del obj.__dict__[key]
class Obj:
    pass
o = Obj()
o.x = 1
o.y = 2
d = o.__dict__
del d['x']
check("del __dict__[key]", 'x' not in o.__dict__)
check("del __dict__ preserves other", o.y == 2)
try:
    _ = o.x
    check("del __dict__ removes attr", False)
except AttributeError:
    check("del __dict__ removes attr", True)

# 9. obj.__dict__[key] = value (store)
o2 = Obj()
o2.__dict__['z'] = 99
check("__dict__ store", o2.z == 99)

# 10. Descriptor protocol with __delete__
class Validator:
    def __set_name__(self, owner, name):
        self.name = name
    def __get__(self, obj, objtype=None):
        if obj is None:
            return self
        return obj.__dict__.get(self.name, 0)
    def __set__(self, obj, value):
        if not isinstance(value, int):
            raise TypeError('must be int')
        obj.__dict__[self.name] = value
    def __delete__(self, obj):
        del obj.__dict__[self.name]

class Model:
    x = Validator()

m = Model()
m.x = 42
check("descriptor set", m.x == 42)
del m.x
check("descriptor delete", m.x == 0)

# 11. File iteration
f = tempfile.mktemp()
with open(f, 'w') as fh:
    fh.write('line1\nline2\nline3\n')
lines = []
with open(f, 'r') as fh:
    for line in fh:
        lines.append(line)
check("file iteration", lines == ['line1\n', 'line2\n', 'line3\n'])
os.unlink(f)

# 12. writelines
f = tempfile.mktemp()
with open(f, 'w') as fh:
    fh.writelines(['a\n', 'b\n', 'c\n'])
with open(f, 'r') as fh:
    check("writelines", fh.read() == 'a\nb\nc\n')
os.unlink(f)

# 13. print to file
f = tempfile.mktemp()
with open(f, 'w') as fh:
    print('hello', 'world', file=fh, sep=', ')
with open(f, 'r') as fh:
    check("print to file", fh.read().strip() == 'hello, world')
os.unlink(f)

print(f"test_phase143: {passed}/{passed+failed} passed")
