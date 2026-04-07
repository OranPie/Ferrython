# Phase 146: raise-from-None __suppress_context__, object identity, eval locals,
#             setattr on modules, compile(ast), generator context manager protocol

# 1. raise X from None sets __suppress_context__
try:
    raise ValueError("oops") from None
except ValueError as e:
    assert e.__suppress_context__ == True, f"expected True got {e.__suppress_context__}"
    assert e.__cause__ is None
print("PASS raise-from-None suppress_context")

# 2. raise bare type from None
try:
    raise ValueError from None
except ValueError as e:
    assert e.__suppress_context__ == True
print("PASS raise bare type from None")

# 3. raise X from Y sets __cause__ and __suppress_context__
try:
    raise RuntimeError("new") from TypeError("orig")
except RuntimeError as e:
    assert e.__suppress_context__ == True
    assert type(e.__cause__).__name__ == "TypeError"
print("PASS raise-from cause")

# 4. object() identity
assert type(object()) is object, "type(object()) is object"
o = object()
assert isinstance(o, object)
print("PASS object() identity")

# 5. eval with locals dict
result = eval('x + y', {}, {'x': 10, 'y': 32})
assert result == 42, f"expected 42 got {result}"
print("PASS eval with locals")

# 6. setattr on modules
import sys
setattr(sys, '_test_marker_146', 'hello')
assert sys._test_marker_146 == 'hello'
print("PASS setattr on modules")

# 7. compile(ast.parse(...))
import ast
tree = ast.parse('x = 1 + 2')
code = compile(tree, '<test>', 'exec')
ns = {}
exec(code, ns)
assert ns['x'] == 3, f"expected 3 got {ns['x']}"
print("PASS compile(ast) exec")

# 8. Generator context manager protocol
from contextlib import contextmanager

@contextmanager
def my_ctx(val):
    yield val

with my_ctx(42) as v:
    assert v == 42
print("PASS generator context manager")

# 9. ExitStack with contextmanager
from contextlib import ExitStack

results = []
@contextmanager
def track(name):
    results.append(f"enter-{name}")
    yield name
    results.append(f"exit-{name}")

with ExitStack() as stack:
    a = stack.enter_context(track("a"))
    b = stack.enter_context(track("b"))
    assert a == "a"
    assert b == "b"
assert results == ["enter-a", "enter-b", "exit-b", "exit-a"]
print("PASS ExitStack with contextmanager")

print("All phase 146 tests passed")
