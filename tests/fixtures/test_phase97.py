# Phase 97: Gap audit fixes — cell_contents, length_hint, VM callback mechanism
import sys

# ── cell_contents on closure cells ──
def make_adder(n):
    def add(x): return x + n
    return add

f = make_adder(10)
assert f(5) == 15, "closure call"
c = f.__closure__
assert c is not None, "__closure__ should not be None"
assert len(c) >= 1, "__closure__ should have at least 1 cell"
cell = c[0]
assert hasattr(cell, 'cell_contents'), "cell should have cell_contents"
assert cell.cell_contents == 10, f"cell_contents should be 10, got {cell.cell_contents}"
print("CHECK 1 PASS: cell_contents works")

# ── nested closure cell_contents ──
def outer(a):
    def middle(b):
        def inner(c):
            return a + b + c
        return inner
    return middle

fn = outer(1)(2)
assert fn(3) == 6, "nested closure"
cells = fn.__closure__
assert cells is not None
for cell in cells:
    assert hasattr(cell, 'cell_contents')
print("CHECK 2 PASS: nested closure cell_contents")

# ── operator.length_hint with Python __length_hint__ ──
import operator

class MyIter:
    def __init__(self, n):
        self.n = n
    def __iter__(self):
        return self
    def __next__(self):
        if self.n <= 0:
            raise StopIteration
        self.n -= 1
        return self.n
    def __length_hint__(self):
        return self.n

h = operator.length_hint(MyIter(5))
assert h == 5, f"length_hint should be 5, got {h}"
print("CHECK 3 PASS: operator.length_hint with __length_hint__")

# ── operator.length_hint with __len__ fallback ──
class Sized:
    def __len__(self):
        return 42

h2 = operator.length_hint(Sized())
assert h2 == 42, f"length_hint __len__ fallback should be 42, got {h2}"
print("CHECK 4 PASS: operator.length_hint __len__ fallback")

# ── operator.length_hint default ──
class NoHint:
    pass
h3 = operator.length_hint(NoHint(), 99)
assert h3 == 99, f"length_hint default should be 99, got {h3}"
print("CHECK 5 PASS: operator.length_hint default value")

# ── sys.exc_info inside except block ──
try:
    raise ValueError("test123")
except ValueError:
    t, v, tb = sys.exc_info()
    assert t is not None, "exc_info type should not be None"
    assert "ValueError" in str(t), f"exc_info type should be ValueError, got {t}"
print("CHECK 6 PASS: sys.exc_info inside except")

# ── finally return override ──
def try_finally():
    try:
        return 1
    finally:
        return 2

assert try_finally() == 2, "finally should override try return"
print("CHECK 7 PASS: finally return override")

# ── print(file=...) with StringIO ──
import io
buf = io.StringIO()
print("hello", "world", sep="-", end="!\n", file=buf)
val = buf.getvalue()
assert val == "hello-world!\n", f"print(file=) got {repr(val)}"
print("CHECK 8 PASS: print(file=, sep=, end=)")

# ── sys.stdout redirect ──
old = sys.stdout
buf2 = io.StringIO()
sys.stdout = buf2
print("captured_output")
sys.stdout = old
val2 = buf2.getvalue()
assert "captured_output" in val2, f"stdout redirect got {repr(val2)}"
print("CHECK 9 PASS: sys.stdout redirect")

# ── VM callback mechanism (request_vm_call) ──
class Counter:
    def __init__(self):
        self.count = 0
    def __length_hint__(self):
        self.count += 1
        return self.count

c = Counter()
h = operator.length_hint(c)
assert h == 1, f"first call should return 1, got {h}"
print("CHECK 10 PASS: VM callback stateful")
