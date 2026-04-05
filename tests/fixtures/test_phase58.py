# Phase 58: Test new features — subclasses, fstring, eval/compile, redirect, inspect, array
# expected: All phase 58 tests passed

# ── type.__subclasses__() ──
class Base:
    pass

class Child1(Base):
    pass

class Child2(Base):
    pass

class GrandChild(Child1):
    pass

subs = Base.__subclasses__()
sub_names = sorted([s.__name__ for s in subs])
assert sub_names == ["Child1", "Child2"], f"subclasses: {sub_names}"
assert len(Child1.__subclasses__()) == 1
assert Child2.__subclasses__() == []

# ── type.mro() ──
mro = GrandChild.mro()
assert mro[0].__name__ == "GrandChild"
assert any(c.__name__ == "Child1" for c in mro)
assert any(c.__name__ == "Base" for c in mro)

# ── f-string nested quotes ──
result = f"{'hello':>10}"
assert len(result) == 10
assert result.strip() == "hello"

val = f"{'yes' if True else 'no'}"
assert val == "yes"

n = 2
pi_str = f"{3.14159:.{n}f}"
assert pi_str == "3.14"

d = {"key": "value"}
assert f"{d['key']}" == "value"

# ── eval with code objects ──
code = compile("x + y", "<test>", "exec")
assert code is not None

result = eval("2 ** 10")
assert result == 1024

result = eval("a + b", {"a": 10, "b": 20})
assert result == 30

# ── compile + exec ──
ns = {}
exec(compile("result = [x**2 for x in range(5)]", "<test>", "exec"), ns)
assert ns["result"] == [0, 1, 4, 9, 16]

# ── redirect_stdout ──
import io
from contextlib import redirect_stdout

buf = io.StringIO()
with redirect_stdout(buf):
    print("captured")

assert "captured" in buf.getvalue()

# ── inspect module ──
import inspect

def example_func(a, b, c=10):
    """Example docstring"""
    return a + b + c

assert inspect.isfunction(example_func)
assert not inspect.isclass(example_func)
assert inspect.isclass(Base)
assert inspect.isroutine(example_func)

sig = inspect.signature(example_func)
assert "a" in sig and "b" in sig and "c" in sig

spec = inspect.getfullargspec(example_func)
assert spec["args"] == ["a", "b", "c"]

members = inspect.getmembers(Base)
member_names = [m[0] for m in members]
assert "__qualname__" in member_names

# ── array module ──
import array

a = array.array('i', [5, 3, 1, 4, 2])
assert len(a) == 5
a.append(6)
assert len(a) == 6
assert a.pop() == 6
a.reverse()
assert a.tolist() == [2, 4, 1, 3, 5]
assert a.count(4) == 1
assert a.index(1) == 2
assert 3 in a
assert a.itemsize == 4

print("All phase 58 tests passed")
