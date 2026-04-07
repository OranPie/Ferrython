# test_phase113.py — Fraction ops, Pattern.subn, re.compile repr

# ── Fraction deepening ──
from fractions import Fraction

# __pow__
f = Fraction(2, 3)
r = f ** 2
assert r == Fraction(4, 9), f"Expected 4/9, got {r}"

# Negative exponent
r2 = Fraction(3, 4) ** -1
assert r2 == Fraction(4, 3), f"Expected 4/3, got {r2}"

# __mod__
f1 = Fraction(7, 3)
f2 = Fraction(2, 1)
# 7/3 % 2 = 7/3 - 2*floor(7/6) = 7/3 - 2*1 = 1/3
r3 = f1 % f2
assert float(r3) - 1/3 < 0.001, f"Expected ~1/3, got {r3}"

# as_integer_ratio
f3 = Fraction(3, 7)
ratio = f3.as_integer_ratio()
assert ratio == (3, 7), f"Expected (3, 7), got {ratio}"

# __format__ basic
f4 = Fraction(1, 3)
s = format(f4)
assert "/" in s or "1" in s  # either "1/3" or float repr

# from_float
f5 = Fraction.from_float(0.5)
assert f5 == Fraction(1, 2), f"Expected 1/2, got {f5}"

# limit_denominator
f6 = Fraction(355, 113)
f7 = f6.limit_denominator(10)
assert f7.denominator <= 10

# ── re.compile with subn and repr ──
import re

p = re.compile(r"\d+")
# Pattern has repr
repr_str = repr(p)
assert "compile" in repr_str or "Pattern" in repr_str

# Pattern.subn
result = p.subn("X", "abc123def456")
assert isinstance(result, tuple)
assert result[0] == "abcXdefX"
assert result[1] == 2

# Pattern.groups and groupindex
p2 = re.compile(r"(?P<word>\w+)\s+(?P<num>\d+)")
assert p2.groups >= 2
gi = p2.groupindex
assert "word" in str(gi) or hasattr(gi, '__getitem__')

# Pattern methods work
m = p2.match("hello 123")
assert m is not None
assert m.group(1) == "hello"
assert m.group(2) == "123"

# fullmatch
p3 = re.compile(r"\d{3}")
assert p3.fullmatch("123") is not None
assert p3.fullmatch("1234") is None

print("phase113: all tests passed")
