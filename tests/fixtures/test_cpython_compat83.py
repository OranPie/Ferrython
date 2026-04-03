# Test 83: Bytes and bytearray operations

passed83 = 0
total83 = 0

def check83(desc, got, expected):
    global passed83, total83
    total83 += 1
    if got == expected:
        passed83 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- bytes constructor ---
val83_1 = bytes(5)
check83("bytes(5) creates 5 zero bytes", val83_1, b"\x00\x00\x00\x00\x00")
check83("bytes(5) length", len(val83_1), 5)

val83_2 = bytes([65, 66, 67])
check83("bytes from list", val83_2, b"ABC")

val83_3 = bytes(b"abc")
check83("bytes from bytes", val83_3, b"abc")

val83_4 = bytes("hello", "utf-8")
check83("bytes from str with encoding", val83_4, b"hello")

val83_5 = bytes()
check83("bytes() empty", val83_5, b"")
check83("bytes() empty length", len(val83_5), 0)

# --- bytes indexing returns int ---
val83_6 = b"ABC"
check83("bytes indexing [0]", val83_6[0], 65)
check83("bytes indexing [1]", val83_6[1], 66)
check83("bytes indexing [-1]", val83_6[-1], 67)
check83("bytes index type is int", type(val83_6[0]), int)

# --- bytes slicing returns bytes ---
val83_7 = b"ABCDEF"
check83("bytes slice [1:3]", val83_7[1:3], b"BC")
check83("bytes slice [:2]", val83_7[:2], b"AB")
check83("bytes slice type", type(val83_7[1:3]), bytes)

# --- bytes.hex() ---
val83_8 = b"AB"
check83("bytes.hex()", val83_8.hex(), "4142")
check83("empty bytes hex", b"".hex(), "")

# --- bytes.fromhex ---
val83_9 = bytes.fromhex("4142")
check83("bytes.fromhex('4142')", val83_9, b"AB")
check83("bytes.fromhex('68656c6c6f')", bytes.fromhex("68656c6c6f"), b"hello")

# --- bytes.decode / str.encode ---
val83_10 = b"hello"
check83("bytes.decode utf-8", val83_10.decode("utf-8"), "hello")
check83("str.encode utf-8", "hello".encode("utf-8"), b"hello")
check83("roundtrip encode/decode", b"test".decode("utf-8").encode("utf-8"), b"test")

# --- bytes methods: upper, lower ---
val83_11 = b"Hello World"
check83("bytes.upper()", val83_11.upper(), b"HELLO WORLD")
check83("bytes.lower()", val83_11.lower(), b"hello world")

# --- bytes.strip, split, join ---
val83_12 = b"  hello  "
check83("bytes.strip()", val83_12.strip(), b"hello")
check83("bytes.lstrip()", val83_12.lstrip(), b"hello  ")
check83("bytes.rstrip()", val83_12.rstrip(), b"  hello")

val83_13 = b"a,b,c"
check83("bytes.split(b',')", val83_13.split(b","), [b"a", b"b", b"c"])

val83_14 = b",".join([b"x", b"y", b"z"])
check83("bytes join", val83_14, b"x,y,z")

# --- bytes.find, replace ---
val83_15 = b"hello world"
check83("bytes.find(b'world')", val83_15.find(b"world"), 6)
check83("bytes.find not found", val83_15.find(b"xyz"), -1)

val83_16 = b"hello world"
check83("bytes.replace", val83_16.replace(b"world", b"python"), b"hello python")

# --- bytes.startswith, endswith ---
val83_17 = b"hello world"
check83("bytes.startswith", val83_17.startswith(b"hello"), True)
check83("bytes.startswith false", val83_17.startswith(b"world"), False)
check83("bytes.endswith", val83_17.endswith(b"world"), True)
check83("bytes.endswith false", val83_17.endswith(b"hello"), False)

# --- bytes concatenation ---
val83_18 = b"hello" + b" " + b"world"
check83("bytes concat", val83_18, b"hello world")

# --- bytes repetition ---
val83_19 = b"ab" * 3
check83("bytes repetition", val83_19, b"ababab")
check83("bytes repetition zero", b"ab" * 0, b"")

# --- bytes comparison ---
check83("bytes equal", b"abc" == b"abc", True)
check83("bytes not equal", b"abc" == b"abd", False)
check83("bytes less than", b"abc" < b"abd", True)
check83("bytes greater than", b"abd" > b"abc", True)

# --- bytearray (mutable) ---
val83_20 = bytearray(b"hello")
check83("bytearray from bytes", val83_20, bytearray(b"hello"))
val83_20[0] = 72
check83("bytearray mutation", val83_20, bytearray(b"Hello"))
check83("bytearray type", type(val83_20), bytearray)

# --- bytearray.append, extend ---
val83_21 = bytearray(b"ab")
val83_21.append(99)
check83("bytearray.append", val83_21, bytearray(b"abc"))

val83_22 = bytearray(b"ab")
val83_22.extend(b"cd")
check83("bytearray.extend", val83_22, bytearray(b"abcd"))

# --- b"hello" literal ---
val83_23 = b"hello"
check83("b-string literal type", type(val83_23), bytes)
check83("b-string literal value", val83_23, b"hello")

# --- len(bytes) ---
check83("len(bytes)", len(b"abcde"), 5)
check83("len(empty bytes)", len(b""), 0)

# --- bytes in bytes containment ---
check83("bytes in bytes (found)", b"ll" in b"hello", True)
check83("bytes in bytes (not found)", b"xyz" in b"hello", False)
check83("single byte in bytes", b"h" in b"hello", True)

print(f"Tests: {total83} | Passed: {passed83} | Failed: {total83 - passed83}")
