# test_cpython_compat98.py - Advanced string formatting and textwrap
passed98 = 0
total98 = 0

def check98(desc, got, expected):
    global passed98, total98
    total98 += 1
    if got == expected:
        passed98 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- str.format with positional index ---
check98("format index {0} {1}", "{0} {1}".format("a", "b"), "a b")
check98("format index reverse", "{1} {0}".format("a", "b"), "b a")
check98("format index repeat", "{0} {0} {0}".format("x"), "x x x")
check98("format index mixed types", "{0} is {1}".format(42, True), "42 is True")
check98("format auto numbering", "{} {}".format("hello", "world"), "hello world")

# --- str.format with named args ---
check98("format named", "{name} is {age}".format(name="Alice", age=30), "Alice is 30")
check98("format named and positional", "{0} is {adjective}".format("Python", adjective="great"), "Python is great")

# --- str.format with format spec ---
check98("format int padding", "{:05d}".format(42), "00042")
check98("format float precision", "{:.2f}".format(3.14159), "3.14")
check98("format left align", "{:<10}".format("left"), "left      ")
check98("format right align", "{:>10}".format("right"), "     right")
check98("format center", "{:^10}".format("mid"), "   mid    ")
check98("format fill char", "{:*^10}".format("mid"), "***mid****")

# --- str.format_map ---
check98("format_map basic", "{name} {age}".format_map({"name": "Bob", "age": 25}), "Bob 25")
check98("format_map single", "{x}".format_map({"x": 42}), "42")

# --- str.expandtabs ---
check98("expandtabs default", "\thello".expandtabs(), "        hello")
check98("expandtabs 4", "\thello".expandtabs(4), "    hello")
check98("expandtabs 0", "\thello".expandtabs(0), "hello")
check98("expandtabs mid", "01\t012\t0123\t01234".expandtabs(8), "01      012     0123    01234")
check98("expandtabs no tabs", "hello".expandtabs(), "hello")

# --- str.zfill ---
check98("zfill positive", "42".zfill(5), "00042")
check98("zfill negative", "-42".zfill(5), "-0042")
check98("zfill already long", "12345".zfill(3), "12345")
check98("zfill zero width", "abc".zfill(0), "abc")
check98("zfill plus sign", "+42".zfill(6), "+00042")
check98("zfill empty", "".zfill(3), "000")

# --- str.translate / str.maketrans ---
table98 = str.maketrans("aeiou", "12345")
check98("translate vowels", "hello world".translate(table98), "h2ll4 w4rld")

table98_del = str.maketrans("", "", "aeiou")
check98("translate delete vowels", "hello world".translate(table98_del), "hll wrld")

table98_map = str.maketrans({"a": "A", "b": "B"})
check98("translate dict", "abc".translate(table98_map), "ABc")

check98("maketrans returns dict", isinstance(str.maketrans("ab", "12"), dict), True)

# --- str.encode / bytes.decode ---
check98("encode utf-8", "hello".encode("utf-8"), b"hello")
check98("encode ascii", "hello".encode("ascii"), b"hello")
check98("decode utf-8", b"hello".decode("utf-8"), "hello")
check98("decode ascii", b"hello".decode("ascii"), "hello")
check98("encode decode roundtrip", "test string".encode("utf-8").decode("utf-8"), "test string")
check98("encode type is bytes", isinstance("hello".encode(), bytes), True)
check98("decode type is str", isinstance(b"hello".decode(), str), True)

# --- repr() of various types ---
check98("repr int", repr(42), "42")
check98("repr float", repr(1.0), "1.0")
check98("repr str", repr("hello"), "'hello'")
check98("repr list", repr([1, 2, 3]), "[1, 2, 3]")
check98("repr tuple", repr((1, 2)), "(1, 2)")
check98("repr dict", repr({}), "{}")
check98("repr bool True", repr(True), "True")
check98("repr bool False", repr(False), "False")
check98("repr None", repr(None), "None")
check98("repr empty str", repr(""), "''")

# --- ascii() ---
check98("ascii simple", ascii("hello"), "'hello'")
check98("ascii with escape", ascii("hello\n"), "'hello\\n'")
check98("ascii non-ascii", ascii("\u00e9"), "'\\xe9'")

# --- format() builtin ---
check98("format int 05d", format(42, "05d"), "00042")
check98("format float .1f", format(3.14, ".1f"), "3.1")
check98("format hex", format(255, "x"), "ff")
check98("format oct", format(255, "o"), "377")
check98("format binary", format(255, "b"), "11111111")
check98("format 08b", format(255, "08b"), "11111111")
check98("format 08b small", format(10, "08b"), "00001010")
check98("format 05d negative", format(-42, "05d"), "-0042")
check98("format empty spec", format(42, ""), "42")
check98("format str spec", format("hello", ">10"), "     hello")
check98("format float e", format(1234.5, ".2e"), "1.23e+03")
check98("format percentage", format(0.75, ".0%"), "75%")

print(f"Tests: {total98} | Passed: {passed98} | Failed: {total98 - passed98}")
