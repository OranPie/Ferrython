# ═══════════════════════════════════════════
# Phase 70 Tests — CPython alignment: C3 MRO, property decorator,
# numbers module isinstance checks
# ═══════════════════════════════════════════

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed = passed + 1
    else:
        failed = failed + 1
        print("FAIL: " + name)

# ── Task 1: C3 Linearization MRO ──

# Diamond inheritance
class O: pass
class A(O): pass
class B(O): pass
class C(A, B): pass

# C3 MRO for C should be: C -> A -> B -> O
mro_c = C.__mro__
mro_names = []
for cls in mro_c:
    n = cls.__name__
    mro_names.append(n)

# MRO should be C, A, B, O, object
test("c3_diamond_mro_C_first", mro_names[0] == "C")
test("c3_diamond_mro_A_second", mro_names[1] == "A")
test("c3_diamond_mro_B_third", mro_names[2] == "B")
test("c3_diamond_mro_O_fourth", mro_names[3] == "O")
test("c3_diamond_mro_object_last", mro_names[-1] == "object")

# Larger diamond: D(B, C) where B(A) and C(A)
class Base: pass
class Left(Base): pass
class Right(Base): pass
class Diamond(Left, Right): pass

d_mro = Diamond.__mro__
d_names = []
for cls in d_mro:
    d_names.append(cls.__name__)

test("c3_diamond2_first", d_names[0] == "Diamond")
test("c3_diamond2_Left_before_Right", d_names[1] == "Left")
test("c3_diamond2_Right_third", d_names[2] == "Right")
test("c3_diamond2_Base_fourth", d_names[3] == "Base")
test("c3_diamond2_object_last", d_names[-1] == "object")

# Method resolution follows MRO
class M1:
    def who(self):
        return "M1"

class M2(M1):
    def who(self):
        return "M2"

class M3(M1):
    def who(self):
        return "M3"

class M4(M2, M3):
    pass

test("mro_method_resolution", M4().who() == "M2")

# Inconsistent MRO should raise TypeError
mro_error = False
try:
    class X(A, B): pass
    class Y(B, A): pass
    class Z(X, Y): pass
except TypeError:
    mro_error = True
test("inconsistent_mro_raises_typeerror", mro_error)


# ── Task 2: Property getter/setter/deleter chain ──

class PropTest:
    def __init__(self):
        self._x = 0

    @property
    def x(self):
        return self._x

    @x.setter
    def x(self, value):
        self._x = value

    @x.deleter
    def x(self):
        self._x = -1  # sentinel value instead of actual del

pt = PropTest()
test("property_getter_initial", pt.x == 0)
pt.x = 42
test("property_setter", pt.x == 42)
del pt.x
test("property_deleter", pt.x == -1)

# Property with all three defined inline
class PropTest2:
    def __init__(self):
        self._val = "init"

    @property
    def val(self):
        return self._val

    @val.setter
    def val(self, v):
        self._val = "set:" + str(v)

    @val.deleter
    def val(self):
        self._val = "deleted"

p2 = PropTest2()
test("prop2_getter", p2.val == "init")
p2.val = 99
test("prop2_setter", p2.val == "set:99")
del p2.val
test("prop2_deleter", p2.val == "deleted")


# ── Task 3: numbers module isinstance checks ──

from numbers import Number, Complex, Real, Rational, Integral

test("isinstance_int_Integral", isinstance(42, Integral))
test("isinstance_int_Rational", isinstance(42, Rational))
test("isinstance_int_Real", isinstance(42, Real))
test("isinstance_int_Complex", isinstance(42, Complex))
test("isinstance_int_Number", isinstance(42, Number))

test("isinstance_float_Real", isinstance(3.14, Real))
test("isinstance_float_Complex", isinstance(3.14, Complex))
test("isinstance_float_Number", isinstance(3.14, Number))
test("isinstance_float_not_Integral", not isinstance(3.14, Integral))

test("isinstance_bool_Integral", isinstance(True, Integral))
test("isinstance_bool_Number", isinstance(True, Number))

# Strings should not be numbers
test("isinstance_str_not_Number", not isinstance("hello", Number))
test("isinstance_str_not_Integral", not isinstance("hello", Integral))

# ── Summary ──
print("phase70: " + str(passed) + " passed, " + str(failed) + " failed")
if failed > 0:
    raise Exception("phase70 FAILED")
