# Phase 128: bytes/bytearray removeprefix/removesuffix, bytes.maketrans/translate, int dunders, structseq

# bytes.removeprefix / removesuffix
b = b"TestHook"
assert b.removeprefix(b"Test") == b"Hook"
assert b.removesuffix(b"Hook") == b"Test"
assert b.removeprefix(b"xxx") == b"TestHook"
assert b.removesuffix(b"xxx") == b"TestHook"

# bytearray removeprefix / removesuffix
ba = bytearray(b"hello.py")
assert ba.removeprefix(bytearray(b"hello")) == bytearray(b".py")
assert ba.removesuffix(bytearray(b".py")) == bytearray(b"hello")

# bytes.maketrans
t = bytes.maketrans(b"abc", b"xyz")
assert isinstance(t, bytes)
assert len(t) == 256
assert t[ord('a')] == ord('x')

# bytes.translate
assert b"abc".translate(t) == b"xyz"
assert b"aabbcc".translate(t) == b"xxyyzz"
assert b"hello".translate(t) == b"hello"

# int.__index__ and related dunders
assert (5).__index__() == 5
assert (0).__index__() == 0
assert (-3).__index__() == -3
assert (7).__abs__() == 7
assert (-7).__abs__() == 7
assert (5).__neg__() == -5
assert (5).__pos__() == 5

# sys structseq named attributes
import sys
assert sys.float_info.max > 0
assert sys.float_info.epsilon > 0
assert sys.float_info.dig > 0
assert sys.int_info.bits_per_digit > 0
assert sys.hash_info.width > 0

print("phase128 ok")
