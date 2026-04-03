# test_cpython_compat94.py - Numeric edge cases
import math

passed94 = 0
total94 = 0

def check94(desc, got, expected):
    global passed94, total94
    total94 += 1
    if got == expected:
        passed94 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

def check94_approx(desc, got, expected, tol=1e-9):
    global passed94, total94
    total94 += 1
    if abs(got - expected) < tol:
        passed94 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# divmod
check94("divmod positive", divmod(17, 5), (3, 2))
check94("divmod negative dividend", divmod(-17, 5), (-4, 3))
check94("divmod negative divisor", divmod(17, -5), (-4, -3))
check94("divmod both negative", divmod(-17, -5), (3, -2))
check94("divmod exact", divmod(10, 5), (2, 0))
check94("divmod float", divmod(7.5, 2.5), (3.0, 0.0))

# pow with mod (three-argument pow)
check94("pow with mod basic", pow(2, 10, 1000), 24)
check94("pow with mod 1", pow(3, 4, 5), 1)
check94("pow with mod large", pow(7, 256, 13), pow(7, 256) % 13)
check94("pow basic", pow(2, 10), 1024)
check94("pow zero exponent", pow(5, 0), 1)
check94("pow negative exponent float", pow(2, -1), 0.5)

# complex arithmetic
c1 = complex(3, 4)
c2 = complex(1, -2)
check94("complex addition", c1 + c2, complex(4, 2))
check94("complex subtraction", c1 - c2, complex(2, 6))
check94("complex multiplication", c1 * c2, complex(11, -2))
check94("complex conjugate", c1.conjugate(), complex(3, -4))
check94("complex real", c1.real, 3.0)
check94("complex imag", c1.imag, 4.0)
check94_approx("complex abs", abs(c1), 5.0)

# int.bit_length
check94("bit_length 0", (0).bit_length(), 0)
check94("bit_length 1", (1).bit_length(), 1)
check94("bit_length 7", (7).bit_length(), 3)
check94("bit_length 8", (8).bit_length(), 4)
check94("bit_length 255", (255).bit_length(), 8)
check94("bit_length 256", (256).bit_length(), 9)
check94("bit_length negative", (-1).bit_length(), 1)
check94("bit_length negative large", (-128).bit_length(), 8)

# int.to_bytes
check94("to_bytes basic", (1024).to_bytes(2, byteorder="big"), b"\x04\x00")
check94("to_bytes little", (1024).to_bytes(2, byteorder="little"), b"\x00\x04")
check94("to_bytes zero", (0).to_bytes(1, byteorder="big"), b"\x00")
check94("to_bytes signed negative", (-1).to_bytes(1, byteorder="big", signed=True), b"\xff")

# int.from_bytes
check94("from_bytes big", int.from_bytes(b"\x04\x00", byteorder="big"), 1024)
check94("from_bytes little", int.from_bytes(b"\x00\x04", byteorder="little"), 1024)
check94("from_bytes signed", int.from_bytes(b"\xff", byteorder="big", signed=True), -1)

# float.is_integer
check94("float is_integer true", (3.0).is_integer(), True)
check94("float is_integer false", (3.5).is_integer(), False)
check94("float is_integer zero", (0.0).is_integer(), True)
check94("float is_integer negative", (-4.0).is_integer(), True)

# float.hex and fromhex roundtrip
f1 = 1.5
check94("float hex roundtrip", float.fromhex(f1.hex()), f1)
f2 = -0.25
check94("float hex roundtrip negative", float.fromhex(f2.hex()), f2)

# round with ndigits
check94("round basic", round(3.14159, 2), 3.14)
check94("round to int", round(3.7), 4)
check94("round negative ndigits", round(12345, -2), 12300)
check94("round bankers half even", round(2.5), 2)
check94("round bankers half even 2", round(3.5), 4)
check94("round zero ndigits", round(3.14, 0), 3.0)
check94("round negative", round(-2.5), -2)

# math.isclose
check94("isclose true", math.isclose(1.0000000001, 1.0, rel_tol=1e-9), True)
check94("isclose false", math.isclose(1.1, 1.0, rel_tol=1e-9), False)
check94("isclose abs_tol", math.isclose(0.0, 0.0001, abs_tol=0.001), True)
check94("isclose exact", math.isclose(1.0, 1.0), True)

# math.gcd
check94("gcd basic", math.gcd(12, 8), 4)
check94("gcd coprime", math.gcd(7, 13), 1)
check94("gcd with zero", math.gcd(0, 5), 5)
check94("gcd both zero", math.gcd(0, 0), 0)
check94("gcd negative", math.gcd(-12, 8), 4)

# abs with various types
check94("abs int positive", abs(5), 5)
check94("abs int negative", abs(-5), 5)
check94("abs float", abs(-3.14), 3.14)
check94("abs zero", abs(0), 0)
check94("abs bool", abs(True), 1)

# math.factorial
check94("factorial 0", math.factorial(0), 1)
check94("factorial 1", math.factorial(1), 1)
check94("factorial 5", math.factorial(5), 120)
check94("factorial 10", math.factorial(10), 3628800)

# math.ceil and math.floor
check94("ceil positive", math.ceil(3.2), 4)
check94("ceil negative", math.ceil(-3.2), -3)
check94("floor positive", math.floor(3.8), 3)
check94("floor negative", math.floor(-3.2), -4)
check94("ceil int", math.ceil(5), 5)
check94("floor int", math.floor(5), 5)

# math.trunc
check94("trunc positive", math.trunc(3.7), 3)
check94("trunc negative", math.trunc(-3.7), -3)

# math.copysign
check94("copysign positive", math.copysign(5, -1), -5.0)
check94("copysign negative", math.copysign(-5, 1), 5.0)

# math.fabs
check94("fabs negative", math.fabs(-3.14), 3.14)
check94("fabs positive", math.fabs(3.14), 3.14)

print(f"Tests: {total94} | Passed: {passed94} | Failed: {total94 - passed94}")
