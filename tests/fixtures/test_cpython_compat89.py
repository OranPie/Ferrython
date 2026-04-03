## test_cpython_compat89.py - String formatting advanced (~40 tests)

passed89 = 0
total89 = 0

def check89(desc, got, expected):
    global passed89, total89
    total89 += 1
    if got == expected:
        passed89 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- Right-align ---
r1 = format("hi", ">10")
check89("right align str", r1, "        hi")

r2 = f"{'hi':>10}"
check89("right align f-string", r2, "        hi")

# --- Left-align ---
r3 = format("hi", "<10")
check89("left align str", r3, "hi        ")

# --- Center-align ---
r4 = format("hi", "^10")
check89("center align str", r4, "    hi    ")

# --- Fill and align ---
r5 = format(42, "0>5")
check89("zero-fill right align", r5, "00042")

r6 = format("x", "*^10")
check89("star-fill center", r6, "****x*****")

r7 = format("ab", "-<10")
check89("dash-fill left", r7, "ab--------")

# --- Zero-padding for numbers ---
r8 = format(42, "05d")
check89("zero pad int", r8, "00042")

r9 = format(3.14, "010.2f")
check89("zero pad float", r9, "0000003.14")

# --- Float precision ---
r10 = format(3.14159, ".2f")
check89("float 2 decimal", r10, "3.14")

r11 = format(3.14159, ".4f")
check89("float 4 decimal", r11, "3.1416")

r12 = format(1.0, ".0f")
check89("float 0 decimal", r12, "1")

# --- Thousands separator ---
r13 = format(1234567, ",")
check89("comma separator int", r13, "1,234,567")

r14 = format(1234567.89, ",.2f")
check89("comma separator float", r14, "1,234,567.89")

r15 = format(1234567, "_")
check89("underscore separator", r15, "1_234_567")

# --- Sign formatting ---
r16 = format(42, "+d")
check89("positive sign +", r16, "+42")

r17 = format(-42, "+d")
check89("negative sign +", r17, "-42")

r18 = format(42, "-d")
check89("negative sign only pos", r18, "42")

r19 = format(-42, "-d")
check89("negative sign only neg", r19, "-42")

r20 = format(42, " d")
check89("space sign pos", r20, " 42")

r21 = format(-42, " d")
check89("space sign neg", r21, "-42")

# --- Binary, octal, hex ---
r22 = format(255, "b")
check89("binary", r22, "11111111")

r23 = format(255, "o")
check89("octal", r23, "377")

r24 = format(255, "x")
check89("hex lower", r24, "ff")

r25 = format(255, "X")
check89("hex upper", r25, "FF")

# --- Alternate form ---
r26 = format(255, "#b")
check89("binary alt form", r26, "0b11111111")

r27 = format(255, "#o")
check89("octal alt form", r27, "0o377")

r28 = format(255, "#x")
check89("hex alt form lower", r28, "0xff")

r29 = format(255, "#X")
check89("hex alt form upper", r29, "0XFF")

# --- Percentage ---
r30 = format(0.75, ".0%")
check89("percentage no decimal", r30, "75%")

r31 = format(0.756, ".1%")
check89("percentage 1 decimal", r31, "75.6%")

# --- Scientific notation ---
r32 = format(12345.6789, ".2e")
check89("scientific lower", r32, "1.23e+04")

r33 = format(12345.6789, ".2E")
check89("scientific upper", r33, "1.23E+04")

# --- format() builtin with int ---
r34 = format(42)
check89("format int no spec", r34, "42")

r35 = format("hello")
check89("format str no spec", r35, "hello")

# --- Custom __format__ ---
class Fmt1:
    def __init__(self, val):
        self.val = val
    def __format__(self, spec):
        if spec == "upper":
            return str(self.val).upper()
        if spec == "repeat":
            return str(self.val) * 3
        return str(self.val)

f1 = Fmt1("hello")
check89("custom __format__ upper", format(f1, "upper"), "HELLO")
check89("custom __format__ repeat", format(f1, "repeat"), "hellohellohello")
check89("custom __format__ default", format(f1, ""), "hello")

# --- Custom __format__ in f-string ---
r36 = f"{Fmt1('abc'):upper}"
check89("custom __format__ in f-string", r36, "ABC")

# --- str.format method ---
r37 = "{0} and {1}".format("a", "b")
check89("str.format positional", r37, "a and b")

r38 = "{x} and {y}".format(x=10, y=20)
check89("str.format keyword", r38, "10 and 20")

r39 = "{} and {}".format("p", "q")
check89("str.format auto numbering", r39, "p and q")

# --- Nested attribute access in format ---
class Obj1:
    def __init__(self, val):
        self.val = val

r40 = "{0.val}".format(Obj1("nested"))
check89("str.format attribute access", r40, "nested")

# --- Format with width and type combined ---
r41 = format(42, ">+10d")
check89("right align with sign", r41, "       +42")

r42 = format(-42, ">10d")
check89("right align negative", r42, "       -42")

# --- format_map ---
r43 = "{name} is {age}".format_map({"name": "Alice", "age": 30})
check89("format_map basic", r43, "Alice is 30")

print(f"Tests: {total89} | Passed: {passed89} | Failed: {total89 - passed89}")
