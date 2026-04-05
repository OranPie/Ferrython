# Phase 71: CPython alignment — extended unpacking, vars(), globals(), locals()

# ── Task 2: Extended unpacking (UNPACK_EX) ──

# first, *rest
first, *rest = [1, 2, 3, 4, 5]
assert first == 1, f"first should be 1, got {first}"
assert rest == [2, 3, 4, 5], f"rest should be [2, 3, 4, 5], got {rest}"

# *init, last
*init, last = [1, 2, 3, 4, 5]
assert init == [1, 2, 3, 4], f"init should be [1, 2, 3, 4], got {init}"
assert last == 5, f"last should be 5, got {last}"

# a, *b, c
a, *b, c = [1, 2, 3, 4, 5]
assert a == 1, f"a should be 1, got {a}"
assert b == [2, 3, 4], f"b should be [2, 3, 4], got {b}"
assert c == 5, f"c should be 5, got {c}"

# Edge: starred gets empty list when exact match
x, *y, z = [10, 20]
assert x == 10
assert y == []
assert z == 20

# Unpacking from tuple
p, *q = (100, 200, 300)
assert p == 100
assert q == [200, 300]

print("extended unpacking: OK")

# ── Task 3: vars(obj) ──

class Obj:
    def __init__(self):
        self.x = 10
        self.y = 20

o = Obj()
d = vars(o)
assert isinstance(d, dict), f"vars(o) should return dict, got {type(d)}"
assert d["x"] == 10, f"vars(o)['x'] should be 10, got {d.get('x')}"
assert d["y"] == 20, f"vars(o)['y'] should be 20, got {d.get('y')}"

print("vars(obj): OK")

# ── Task 3: globals() ──

GLOBAL_VAR = 42
g = globals()
assert isinstance(g, dict), f"globals() should return dict, got {type(g)}"
assert g["GLOBAL_VAR"] == 42, f"globals()['GLOBAL_VAR'] should be 42, got {g.get('GLOBAL_VAR')}"

print("globals(): OK")

# ── Task 3: locals() inside a function ──

def test_locals():
    a_local = 99
    b_local = "hello"
    loc = locals()
    assert isinstance(loc, dict), f"locals() should return dict, got {type(loc)}"
    assert loc["a_local"] == 99, f"locals()['a_local'] should be 99, got {loc.get('a_local')}"
    assert loc["b_local"] == "hello", f"locals()['b_local'] should be 'hello', got {loc.get('b_local')}"

test_locals()

print("locals(): OK")

# ── Task 3: vars() with no args (== locals()) ──

def test_vars_no_arg():
    v1 = 111
    v2 = 222
    d = vars()
    assert isinstance(d, dict)
    assert d["v1"] == 111, f"vars()['v1'] should be 111, got {d.get('v1')}"
    assert d["v2"] == 222, f"vars()['v2'] should be 222, got {d.get('v2')}"

test_vars_no_arg()

print("vars() no-arg: OK")

print("phase71: all tests passed")
