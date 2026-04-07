# test_phase156.py — dis.dis string, bound method eq, advanced probes

# dis.dis with string input
import dis, io
out = io.StringIO()
dis.dis("x = 1 + 2", file=out)
assert len(out.getvalue()) > 0, "dis.dis should produce output for string input"

# sys.settrace works for function calls
import sys
log = []
def tracer(frame, event, arg):
    log.append(event)
    return tracer
sys.settrace(tracer)
def traced_fn(): x = 1 + 2; return x
traced_fn()
sys.settrace(None)
assert 'call' in log, f"expected 'call' in trace log, got {log}"
assert 'return' in log, f"expected 'return' in trace log, got {log}"

# Bound method equality
class B:
    def m(self): pass
b = B()
assert b.m == b.m, "bound methods from same instance should be equal"
b2 = B()
assert b.m != b2.m, "bound methods from different instances should not be equal"

# functools.singledispatch
from functools import singledispatch
@singledispatch
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
assert process(3.14) == "default: 3.14"

# typing.get_type_hints
from typing import get_type_hints
def annotated_fn(x: int, y: str) -> bool: pass
hints = get_type_hints(annotated_fn)
assert hints.get('x') is int
assert hints.get('y') is str

# dict.keys/items set operations
d1 = {'a': 1, 'b': 2, 'c': 3}
d2 = {'b': 20, 'c': 30, 'd': 40}
assert d1.keys() & d2.keys() == {'b', 'c'}
assert ('b', 2) in (d1.items() & {('b', 2), ('d', 4)})

# recursive repr
lst = []
lst.append(lst)
assert repr(lst) == '[[...]]'

# max/min with default
assert max([], default=42) == 42

# bool arithmetic
assert True + True == 2 and True * 5 == 5

# complex from string
assert complex("3+4j") == complex(3, 4)

# any short-circuit
counter = [0]
def check(x):
    counter[0] += 1
    return x > 5
any(check(x) for x in [1, 2, 6, 3, 4])
assert counter[0] == 3, f"expected 3 checks, got {counter[0]}"

print("test_phase156 passed")
