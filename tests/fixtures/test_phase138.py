"""Phase 138: re.sub backrefs, mkstemp fd, logger default level, format specs, protocols."""
import sys
passed = 0
failed = 0
errors = []

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        errors.append(f"  FAIL {name}: got {got!r}, expected {expected!r}")

# re.sub backref correctness
import re
check("re.sub backref", re.sub(r'(\w+)@(\w+)', r'\2_\1', 'user@host'), 'host_user')
check("re.sub groups", re.sub(r'(a)(b)', r'\2\1', 'ab'), 'ba')

# tempfile.mkstemp real fd
import tempfile, os
fd, path = tempfile.mkstemp()
os.write(fd, b"test_mkstemp")
os.close(fd)
check("mkstemp fd", os.path.getsize(path) >= 12, True)
with open(path) as f:
    check("mkstemp content", f.read(), "test_mkstemp")
os.unlink(path)

# logging.getLogger default level
import logging
logger = logging.getLogger("phase138")
check("logger default level", logger.level, 0)  # NOTSET
logger.setLevel(logging.DEBUG)
check("logger set level", logger.level, 10)

# format spec edge cases
check("format fill align", f"{'hi':*>10}", "********hi")
check("format +sign", f"{42:+d}", "+42")
check("format hex", f"{255:x}", "ff")
check("format oct", f"{255:o}", "377")
check("format bin", f"{10:b}", "1010")

# Custom protocols
class Box:
    def __init__(self, items):
        self.items = items
    def __len__(self):
        return len(self.items)
    def __contains__(self, item):
        return item in self.items
    def __iter__(self):
        return iter(self.items)

b = Box([1, 2, 3])
check("custom len", len(b), 3)
check("custom contains", 2 in b, True)
check("custom iter", list(b), [1, 2, 3])

# Generator with return value
def gen():
    yield 1
    return "done"
g = gen()
next(g)
try:
    next(g)
except StopIteration as e:
    check("gen return", e.value, "done")

# yield from
def inner():
    yield 1
    yield 2
    return "inner_done"
def outer():
    result = yield from inner()
    yield result
check("yield from", list(outer()), [1, 2, "inner_done"])

# Decorator with args
def repeat(n):
    def dec(fn):
        def wrapper(*a):
            return [fn(*a) for _ in range(n)]
        return wrapper
    return dec

@repeat(3)
def greet():
    return "hi"
check("decorator args", greet(), ["hi", "hi", "hi"])

print(f"phase138: {passed} checks passed")
for e in errors:
    print(e)
if failed:
    sys.exit(1)
