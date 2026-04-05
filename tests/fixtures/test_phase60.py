"""Phase 60: Finally override, super().__getattribute__, dis module, Enum unpacking, decimal ops."""

results = []
def test(name, condition):
    results.append((name, condition))

# ── Finally return override ──
def f_finally_override():
    try:
        return 1
    finally:
        return 2

test("finally_override", f_finally_override() == 2)

def f_nested_finally():
    try:
        try:
            return 1
        finally:
            return 2
    finally:
        return 3

test("nested_finally", f_nested_finally() == 3)

def f_finally_exc():
    try:
        raise ValueError("boom")
    finally:
        return 42

test("finally_suppress_exc", f_finally_exc() == 42)

def f_finally_pass():
    try:
        return "try"
    except:
        return "except"
    finally:
        pass

test("finally_pass_through", f_finally_pass() == "try")

# ── super().__getattribute__ ──
class AttrBase:
    x = 10
    y = 20

class AttrChild(AttrBase):
    x = 99
    def get_base_x(self):
        return super().__getattribute__("x")
    def get_base_y(self):
        return super().__getattribute__("y")

ac = AttrChild()
test("super_getattribute", ac.get_base_x() == 10)
test("super_getattribute_y", ac.get_base_y() == 20)

# super().__class__
class M:
    pass
class N(M):
    def get_super_class(self):
        return super().__class__
n = N()
# In CPython, super().__class__ is <class 'super'>
test("super_class", str(n.get_super_class()) in ("super", "<class 'super'>"))

# super() MRO chain
class A:
    def method(self): return "A"
class B(A):
    def method(self): return "B+" + super().method()
class C(B):
    def method(self): return "C+" + super().method()
test("super_mro_chain", C().method() == "C+B+A")

# super() with __init__
class InitBase:
    def __init__(self, x):
        self.x = x
class InitChild(InitBase):
    def __init__(self, x, y):
        super().__init__(x)
        self.y = y
ic = InitChild(10, 20)
test("super_init", ic.x == 10 and ic.y == 20)

# super() property access
class PropBase:
    @property
    def val(self):
        return 42
class PropChild(PropBase):
    @property
    def val(self):
        return super().val * 2
test("super_property", PropChild().val == 84)

# ── dis module ──
import dis
def simple_fn(a, b):
    return a + b

# dis.dis outputs via Rust println!, not Python sys.stdout
# So just verify it runs without error
try:
    dis.dis(simple_fn)
    test("dis_runs", True)
except Exception:
    test("dis_runs", False)

# dis.disassemble also works
try:
    dis.disassemble(simple_fn)
    test("dis_disassemble", True)
except Exception:
    test("dis_disassemble", False)

# ── Enum tuple unpacking ──
from enum import Enum

class Planet(Enum):
    MERCURY = (3.3e23, 2.44e6)
    EARTH = (5.97e24, 6.37e6)
    def __init__(self, mass, radius):
        self.mass = mass
        self.radius = radius

test("enum_tuple_unpack", Planet.EARTH.mass == 5.97e24)
test("enum_tuple_radius", Planet.EARTH.radius == 6.37e6)
test("enum_tuple_value", Planet.EARTH.value == (5.97e24, 6.37e6))

class Color(Enum):
    RED = 1
    GREEN = 2
    BLUE = 3

test("enum_basic", Color.RED.value == 1)
test("enum_name", Color.GREEN.name == "GREEN")

# ── Decimal arithmetic ──
from decimal import Decimal

a = Decimal("1.5")
b = Decimal("2.3")
test("decimal_add", str(a + b) == "3.8")
test("decimal_sub", str(b - a) == "0.8")
test("decimal_mul", str(a * b) == "3.45")

c = Decimal("10")
d = Decimal("3")
div_result = c / d
test("decimal_div", "3.333" in str(div_result))

test("decimal_neg", str(-a) == "-1.5")
test("decimal_eq", Decimal("1.0") == Decimal("1"))
test("decimal_lt", Decimal("1.5") < Decimal("2.0"))

# ── cmath module ──
import cmath
sqrt_neg = cmath.sqrt(-1)
test("cmath_sqrt_neg", abs(sqrt_neg.imag - 1.0) < 1e-10)
test("cmath_sqrt_pos", abs(cmath.sqrt(4) - 2.0) < 1e-10 if isinstance(cmath.sqrt(4), (int, float)) else abs(cmath.sqrt(4).real - 2.0) < 1e-10)

test("cmath_pi", abs(cmath.pi - 3.14159265) < 1e-6)
test("cmath_e", abs(cmath.e - 2.71828182) < 1e-6)

# ── Recursion limit ──
import sys
test("recursion_limit", sys.getrecursionlimit() >= 100)

# Note: actual infinite recursion test omitted from fixture as it can
# overflow the Rust stack in the test harness (limited thread stack size)

# ── timeit module ──
import timeit
t = timeit.default_timer()
test("timeit_timer", isinstance(t, float) and t > 0)
timer = timeit.Timer()
test("timeit_timer_class", timer is not None)

# ── Report ──
passed = sum(1 for _, v in results if v)
failed = [(n, v) for n, v in results if not v]
print(f"test_phase60: {passed}/{len(results)} passed")
if failed:
    for name, _ in failed:
        print(f"  FAIL: {name}")
assert not failed, f"{len(failed)} tests failed"
