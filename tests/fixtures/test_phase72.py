# Phase 72: stdlib improvements — struct, hashlib, re

passed = 0
failed = 0

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        print("FAIL:", name, "got:", repr(got), "expected:", repr(expected))

# ── struct module ──

import struct

# calcsize
check("calcsize_b", struct.calcsize("b"), 1)
check("calcsize_B", struct.calcsize("B"), 1)
check("calcsize_h", struct.calcsize("h"), 2)
check("calcsize_H", struct.calcsize("H"), 2)
check("calcsize_i", struct.calcsize("i"), 4)
check("calcsize_I", struct.calcsize("I"), 4)
check("calcsize_q", struct.calcsize("q"), 8)
check("calcsize_Q", struct.calcsize("Q"), 8)
check("calcsize_f", struct.calcsize("f"), 4)
check("calcsize_d", struct.calcsize("d"), 8)
check("calcsize_10s", struct.calcsize("10s"), 10)
check("calcsize_combo", struct.calcsize(">2ih"), 10)

# pack/unpack little-endian integers
data = struct.pack("<i", 1)
check("pack_le_i_len", len(data), 4)
val = struct.unpack("<i", data)
check("unpack_le_i", val, (1,))

# pack/unpack big-endian
data_be = struct.pack(">i", 256)
val_be = struct.unpack(">i", data_be)
check("unpack_be_i", val_be, (256,))

# network byte order (big-endian)
data_net = struct.pack("!H", 8080)
val_net = struct.unpack("!H", data_net)
check("unpack_net_H", val_net, (8080,))

# bytes/signed
data_b = struct.pack("<bB", -1, 255)
val_b = struct.unpack("<bB", data_b)
check("unpack_bB", val_b, (-1, 255))

# short signed/unsigned
data_h = struct.pack("<hH", -100, 60000)
val_h = struct.unpack("<hH", data_h)
check("unpack_hH", val_h, (-100, 60000))

# long long
data_q = struct.pack("<qQ", -1, 2**63)
val_q = struct.unpack("<qQ", data_q)
check("unpack_q", val_q[0], -1)

# float
data_f = struct.pack("<f", 3.14)
val_f = struct.unpack("<f", data_f)
check("unpack_f_close", abs(val_f[0] - 3.14) < 0.001, True)

# double
data_d = struct.pack("<d", 2.718281828)
val_d = struct.unpack("<d", data_d)
check("unpack_d_close", abs(val_d[0] - 2.718281828) < 1e-6, True)

# string format
data_s = struct.pack("5s", b"hello")
val_s = struct.unpack("5s", data_s)
check("unpack_5s", val_s[0], b"hello")

# multiple values
data_multi = struct.pack("<2i", 10, 20)
val_multi = struct.unpack("<2i", data_multi)
check("unpack_2i", val_multi, (10, 20))

# Struct object
s = struct.Struct("<ih")
check("Struct_size", s.size, 6)
packed = s.pack(42, 7)
unpacked = s.unpack(packed)
check("Struct_roundtrip", unpacked, (42, 7))

print("struct:", passed, "passed")

# ── hashlib module ──

import hashlib

# sha256 of empty string
h = hashlib.sha256(b"")
check("sha256_empty", h.hexdigest(), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")

# sha256 of "hello"
h2 = hashlib.sha256(b"hello")
check("sha256_hello", h2.hexdigest(), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")

# md5
h3 = hashlib.md5(b"hello")
check("md5_hello", h3.hexdigest(), "5d41402abc4b2a76b9719d911017c592")

# digest returns bytes
d = hashlib.sha256(b"test").digest()
check("digest_type", type(d).__name__, "bytes")
check("digest_len", len(d), 32)

# update()
h4 = hashlib.sha256(b"hel")
h4.update(b"lo")
check("sha256_update", h4.hexdigest(), "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")

# hash object attributes
h5 = hashlib.sha256(b"x")
check("hash_name", h5.name, "sha256")
check("hash_digest_size", h5.digest_size, 32)
check("hash_block_size", h5.block_size, 64)

# hashlib.new()
h6 = hashlib.new("md5", b"hello")
check("new_md5", h6.hexdigest(), "5d41402abc4b2a76b9719d911017c592")

print("hashlib:", passed, "passed (cumulative)")

# ── re module ──

import re

# match at start
m = re.match(r"hello", "hello world")
check("re_match_found", m is not None, True)
check("re_match_group", m.group(), "hello")

# match fails
m2 = re.match(r"world", "hello world")
check("re_match_none", m2 is None, True)

# search anywhere
m3 = re.search(r"world", "hello world")
check("re_search_found", m3 is not None, True)
check("re_search_group", m3.group(), "world")
check("re_search_start", m3.start(), 6)
check("re_search_end", m3.end(), 11)
check("re_search_span", m3.span(), (6, 11))

# findall
results = re.findall(r"\d+", "abc 123 def 456 ghi 789")
check("re_findall", results, ["123", "456", "789"])

# findall with groups
results2 = re.findall(r"(\w+)=(\w+)", "a=1 b=2 c=3")
check("re_findall_groups", results2, [("a", "1"), ("b", "2"), ("c", "3")])

# sub
result = re.sub(r"\d+", "NUM", "abc 123 def 456")
check("re_sub", result, "abc NUM def NUM")

# split
parts = re.split(r"\s+", "hello  world   foo")
check("re_split", parts, ["hello", "world", "foo"])

# compile
pat = re.compile(r"\d+")
m4 = pat.search("abc 42 def")
check("compiled_search", m4.group(), "42")

found = pat.findall("1 22 333")
check("compiled_findall", found, ["1", "22", "333"])

replaced = pat.sub("X", "a1b2c3")
check("compiled_sub", replaced, "aXbXcX")

split_result = pat.split("a1b22c333d")
check("compiled_split", split_result, ["a", "b", "c", "d"])

# compiled match
pat2 = re.compile(r"(\w+)@(\w+)")
m5 = pat2.match("user@host rest")
check("compiled_match", m5 is not None, True)
check("compiled_match_group0", m5.group(0), "user@host")
check("compiled_match_group1", m5.group(1), "user")
check("compiled_match_group2", m5.group(2), "host")
check("compiled_match_groups", m5.groups(), ("user", "host"))

# IGNORECASE flag
m6 = re.search(r"hello", "HELLO WORLD", re.IGNORECASE)
check("re_ignorecase", m6 is not None, True)
check("re_ignorecase_group", m6.group(), "HELLO")

# MULTILINE flag
m7 = re.search(r"^world", "hello\nworld", re.MULTILINE)
check("re_multiline", m7 is not None, True)
check("re_multiline_group", m7.group(), "world")

# match object group(0) and groups
m8 = re.search(r"(\d+)-(\d+)", "call 555-1234 now")
check("groups_tuple", m8.groups(), ("555", "1234"))
check("group_0", m8.group(0), "555-1234")
check("group_1", m8.group(1), "555")
check("group_2", m8.group(2), "1234")

print("re:", passed, "passed (cumulative)")

# ── Summary ──
print("Tests:", passed + failed, "| Passed:", passed, "| Failed:", failed)
if failed > 0:
    raise Exception("test_phase72 FAILED: " + str(failed) + " failures")
print("phase72: all tests passed")
