# Test phase 66: gzip, zipfile, and itertools enhancements

import os

# ── gzip compress / decompress round-trip ──
import gzip

original = b"hello world! " * 100
compressed = gzip.compress(original)
assert isinstance(compressed, bytes), "gzip.compress should return bytes"
assert len(compressed) < len(original), "compressed data should be smaller"

decompressed = gzip.decompress(compressed)
assert decompressed == original, f"round-trip failed: got {len(decompressed)} bytes, expected {len(original)}"
print("PASS: gzip compress/decompress round-trip")

# compress with level
compressed_fast = gzip.compress(b"data data data data", 1)
decompressed_fast = gzip.decompress(compressed_fast)
assert decompressed_fast == b"data data data data"
print("PASS: gzip compress with compresslevel")

# empty data
empty_compressed = gzip.compress(b"")
empty_decompressed = gzip.decompress(empty_compressed)
assert empty_decompressed == b""
print("PASS: gzip empty data round-trip")

# ── gzip.open write/read ──
gz_path = "/tmp/_test_phase66.gz"
f = gzip.open(gz_path, "wb")
f.write(b"hello from gzip.open")
f.close()
assert os.path.exists(gz_path), "gzip file should exist after close"

f2 = gzip.open(gz_path, "rb")
data = f2.read()
f2.close()
assert data == b"hello from gzip.open", f"gzip.open read got: {data}"
print("PASS: gzip.open write/read")

# cleanup
os.remove(gz_path)

# ── zipfile create / read ──
import zipfile

zip_path = "/tmp/_test_phase66.zip"
zf = zipfile.ZipFile(zip_path, "w")
zf.writestr("hello.txt", "hello world")
zf.writestr("data.bin", b"binary data here")
zf.close()
assert os.path.exists(zip_path), "zip file should exist after close"

zf2 = zipfile.ZipFile(zip_path, "r")
names = zf2.namelist()
assert "hello.txt" in names, f"namelist should contain hello.txt, got {names}"
assert "data.bin" in names, f"namelist should contain data.bin, got {names}"

content = zf2.read("hello.txt")
assert content == b"hello world", f"read hello.txt got: {content}"

bin_content = zf2.read("data.bin")
assert bin_content == b"binary data here", f"read data.bin got: {bin_content}"
zf2.close()
print("PASS: zipfile create/read round-trip")

# cleanup
os.remove(zip_path)

# ── itertools.product ──
import itertools

result = list(itertools.product([1, 2], ["a", "b"]))
assert len(result) == 4, f"product length should be 4, got {len(result)}"
assert result[0] == (1, "a"), f"product[0] should be (1, 'a'), got {result[0]}"
assert result[3] == (2, "b"), f"product[3] should be (2, 'b'), got {result[3]}"
print("PASS: itertools.product")

# ── itertools.combinations ──
result = list(itertools.combinations([1, 2, 3, 4], 2))
assert len(result) == 6, f"C(4,2) should be 6, got {len(result)}"
assert (1, 2) in result
assert (3, 4) in result
print("PASS: itertools.combinations")

# ── itertools.permutations ──
result = list(itertools.permutations([1, 2, 3], 2))
assert len(result) == 6, f"P(3,2) should be 6, got {len(result)}"
assert (1, 2) in result
assert (2, 1) in result
print("PASS: itertools.permutations")

# ── itertools.groupby ──
data = [1, 1, 2, 2, 2, 3]
groups = list(itertools.groupby(data))
assert len(groups) == 3, f"groupby should have 3 groups, got {len(groups)}"
print("PASS: itertools.groupby")

# ── itertools.tee ──
original_list = [1, 2, 3, 4, 5]
a, b = itertools.tee(original_list, 2)
assert list(a) == [1, 2, 3, 4, 5]
assert list(b) == [1, 2, 3, 4, 5]
print("PASS: itertools.tee")

# ── itertools.chain ──
result = list(itertools.chain([1, 2], [3, 4]))
assert result == [1, 2, 3, 4], f"chain got: {result}"
print("PASS: itertools.chain")

print("ALL PHASE 66 TESTS PASSED")
