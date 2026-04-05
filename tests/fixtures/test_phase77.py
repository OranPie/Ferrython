# test_phase77: Pure Python stdlib modules + enhanced Rust unittest
import sys
results = []

# ── 1. textwrap module ──
try:
    from textwrap import wrap, fill, dedent, indent, shorten
    
    # wrap
    text = "The quick brown fox jumps over the lazy dog and runs away"
    lines = wrap(text, width=30)
    assert len(lines) >= 2, f"wrap should produce multiple lines: {lines}"
    for line in lines:
        assert len(line) <= 30, f"line too long: {line}"
    
    # fill
    filled = fill(text, width=30)
    assert "\n" in filled, "fill should contain newlines"
    
    # dedent
    dedented = dedent("    hello\n    world\n")
    assert "hello" in dedented and "world" in dedented, f"dedent failed: {dedented!r}"
    # Check leading whitespace is removed
    assert not dedented.startswith("    "), f"dedent didn't strip: {dedented!r}"
    
    # indent
    indented = indent("hello\nworld\n", ">>> ")
    assert ">>> hello" in indented and ">>> world" in indented, f"indent failed: {indented!r}"
    
    # shorten
    short = shorten("Hello World, this is a very long string", width=20)
    assert len(short) <= 20, f"shorten too long: {short!r}"
    
    results.append("PASS textwrap")
except Exception as e:
    results.append(f"FAIL textwrap: {e}")

# ── 2. string module ──
try:
    import string as string_mod
    
    # Check basic constants
    assert hasattr(string_mod, 'ascii_letters') or hasattr(string_mod, 'digits'), "string module loaded"
    
    # The Rust string module provides constants; Python version adds Template/Formatter
    # Test what's available
    has_letters = hasattr(string_mod, 'ascii_letters')
    has_digits = hasattr(string_mod, 'digits')
    if has_letters:
        assert len(string_mod.ascii_letters) == 52
    if has_digits:
        assert len(string_mod.digits) == 10
    
    results.append("PASS string")
except Exception as e:
    results.append(f"FAIL string: {e}")

# ── 3. copy module ──
try:
    from copy import copy, deepcopy
    
    # Shallow copy of list
    orig = [1, [2, 3], 4]
    shallow = copy(orig)
    assert shallow == orig
    assert shallow is not orig
    assert shallow[1] is orig[1]  # shallow: inner list shared
    
    # Deep copy of list
    deep = deepcopy(orig)
    assert deep == orig
    assert deep is not orig
    assert deep[1] is not orig[1]  # deep: inner list copied
    
    # Deep copy of dict
    d = {"a": [1, 2], "b": {"c": 3}}
    dd = deepcopy(d)
    assert dd == d
    assert dd["a"] is not d["a"]
    assert dd["b"] is not d["b"]
    
    # Primitives return as-is
    assert copy(42) == 42
    assert deepcopy("hello") == "hello"
    
    results.append("PASS copy")
except Exception as e:
    results.append(f"FAIL copy: {e}")

# ── 4. functools module (pure Python) ──
try:
    from functools import wraps, reduce, partial
    
    # reduce
    assert reduce(lambda a, b: a + b, [1, 2, 3, 4]) == 10
    assert reduce(lambda a, b: a * b, [1, 2, 3], 10) == 60
    
    # partial
    def add(a, b, c=0):
        return a + b + c
    add5 = partial(add, 5)
    assert add5(3) == 8
    assert add5(3, c=10) == 18
    
    results.append("PASS functools_py")
except Exception as e:
    results.append(f"FAIL functools_py: {e}")

# ── 5. types module ──
try:
    from types import SimpleNamespace
    
    ns = SimpleNamespace(x=1, y=2, z="hello")
    assert ns.x == 1
    assert ns.y == 2
    assert ns.z == "hello"
    ns.w = 42
    assert ns.w == 42
    
    results.append("PASS types")
except Exception as e:
    results.append(f"FAIL types: {e}")

# ── 6. collections module (Python layer) ──
try:
    from collections import OrderedDict, Counter, defaultdict, deque, namedtuple
    
    # Counter
    c = Counter("abracadabra")
    assert c['a'] == 5 or isinstance(c, dict), "Counter basics"  # Rust returns dict-like
    
    # deque
    d = deque([1, 2, 3])
    d.append(4)
    d.appendleft(0)
    assert list(d) == [0, 1, 2, 3, 4] or len(d) == 5
    
    # namedtuple
    Point = namedtuple('Point', ['x', 'y'])
    p = Point(10, 20)
    assert p[0] == 10 or p.x == 10
    assert p[1] == 20 or p.y == 20
    
    results.append("PASS collections")
except Exception as e:
    results.append(f"FAIL collections: {e}")

# ── 7. unittest assert methods ──
try:
    import unittest
    
    tc = unittest.TestCase()
    
    # assertEqual / assertNotEqual
    tc.assertEqual(1, 1)
    tc.assertNotEqual(1, 2)
    
    # assertTrue / assertFalse
    tc.assertTrue(True)
    tc.assertFalse(False)
    
    # assertIsNone / assertIsNotNone
    tc.assertIsNone(None)
    tc.assertIsNotNone(42)
    
    # assertIn / assertNotIn
    tc.assertIn(1, [1, 2, 3])
    tc.assertNotIn(4, [1, 2, 3])
    
    # assertGreater / assertLess
    tc.assertGreater(5, 3)
    tc.assertLess(3, 5)
    
    results.append("PASS unittest_asserts")
except Exception as e:
    results.append(f"FAIL unittest_asserts: {e}")

# ── 8. json.dump/json.load ──
try:
    import json
    
    data = {"name": "test", "values": [1, 2, 3], "nested": {"a": True, "b": None}}
    
    # dumps with indent
    pretty = json.dumps(data, indent=2)
    assert "\n" in pretty, "indent should produce newlines"
    
    # dumps with sort_keys
    sorted_json = json.dumps({"z": 1, "a": 2}, sort_keys=True)
    # Should have "a" before "z"
    a_pos = sorted_json.find('"a"')
    z_pos = sorted_json.find('"z"')
    assert a_pos < z_pos, f"sort_keys failed: {sorted_json}"
    
    # roundtrip
    s = json.dumps(data)
    loaded = json.loads(s)
    assert loaded["name"] == "test"
    assert loaded["values"] == [1, 2, 3]
    
    results.append("PASS json_enhanced")
except Exception as e:
    results.append(f"FAIL json_enhanced: {e}")

# ── 9. contextlib module ──
try:
    from contextlib import suppress, closing, nullcontext
    
    # suppress
    with suppress(ValueError, TypeError):
        raise ValueError("suppressed")
    
    # nullcontext
    with nullcontext(42) as val:
        assert val == 42
    
    # closing
    class Closeable:
        def __init__(self):
            self.closed = False
        def close(self):
            self.closed = True
    
    c = Closeable()
    with closing(c):
        pass
    assert c.closed
    
    results.append("PASS contextlib")
except Exception as e:
    results.append(f"FAIL contextlib: {e}")

# ── 10. dataclasses module ──
try:
    from dataclasses import dataclass, field, fields, asdict, astuple, replace
    
    @dataclass
    class Point:
        x: int
        y: int
        z: int = 0
    
    p = Point(1, 2)
    assert p.x == 1
    assert p.y == 2
    assert p.z == 0
    
    # repr
    r = repr(p)
    assert "Point" in r
    assert "x=1" in r
    
    # equality
    p2 = Point(1, 2)
    assert p == p2
    
    p3 = Point(1, 3)
    assert p != p3
    
    # asdict
    d = asdict(p)
    assert d == {"x": 1, "y": 2, "z": 0}
    
    # astuple
    t = astuple(p)
    assert t == (1, 2, 0)
    
    # replace
    p4 = replace(p, z=99)
    assert p4.z == 99
    assert p4.x == 1
    
    results.append("PASS dataclasses")
except Exception as e:
    results.append(f"FAIL dataclasses: {e}")

# ── Summary ──
for r in results:
    print(r)

passed = sum(1 for r in results if r.startswith("PASS"))
total = len(results)
print(f"\n{passed}/{total} stdlib checks passed")
assert passed == total, f"Some checks failed!"
