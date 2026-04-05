# Test bz2, lzma, tarfile, and csv.DictWriter improvements

# ── bz2 ──
import bz2
data = b"Ferrython compression test! " * 50
c = bz2.compress(data)
assert bz2.decompress(c) == data
comp = bz2.BZ2Compressor()
chunk = comp.compress(data)
decomp = bz2.BZ2Decompressor()
assert decomp.decompress(chunk) == data
print("bz2: PASS")

# ── lzma ──
import lzma
c = lzma.compress(data)
assert lzma.decompress(c) == data
assert lzma.FORMAT_XZ == 1
comp = lzma.LZMACompressor()
chunk = comp.compress(data)
decomp = lzma.LZMADecompressor()
assert decomp.decompress(chunk) == data
print("lzma: PASS")

# ── tarfile ──
import tarfile, os, tempfile, shutil
tmpdir = tempfile.mkdtemp()
test_file = os.path.join(tmpdir, "hello.txt")
with open(test_file, "w") as f:
    f.write("hello world")
tar_path = os.path.join(tmpdir, "test.tar")
tf = tarfile.open(tar_path, "w")
tf.add(test_file, "hello.txt")
tf.close()
tf = tarfile.open(tar_path, "r")
assert "hello.txt" in tf.getnames()
members = tf.getmembers()
assert len(members) == 1
assert members[0].name == "hello.txt"
assert members[0].isfile()
assert not members[0].isdir()
tf.close()
shutil.rmtree(tmpdir)
print("tarfile: PASS")

# ── csv.DictWriter ──
import csv, io
buf = io.StringIO()
w = csv.DictWriter(buf, ["a", "b"])
w.writeheader()
w.writerow({"a": "1", "b": "2"})
w.writerows([{"a": "3", "b": "4"}])
result = buf.getvalue()
lines = result.strip().split("\r\n")
assert lines[0] == "a,b"
assert lines[1] == "1,2"
assert lines[2] == "3,4"
print("csv.DictWriter: PASS")

print("ALL PASS")
