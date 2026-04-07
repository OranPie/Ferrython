# Phase 147: csv.reader iterator, bytearray.fromhex, delattr on modules

# 1. csv.reader as iterator with next()
import csv, io
buf = io.StringIO()
w = csv.writer(buf)
w.writerow(['a', 'b', 'c'])
w.writerow(['d', 'e', 'f'])
buf.seek(0)
r = csv.reader(buf)
assert next(r) == ['a', 'b', 'c']
assert next(r) == ['d', 'e', 'f']
try:
    next(r)
    assert False, "should raise StopIteration"
except StopIteration:
    pass
print("PASS csv.reader next()")

# 2. csv.reader for-loop iteration
buf2 = io.StringIO("x,y,z\n1,2,3\n")
rows = list(csv.reader(buf2))
assert len(rows) == 2
assert rows[0] == ['x', 'y', 'z']
print("PASS csv.reader for-loop")

# 3. csv.reader backward compat (len, index)
buf3 = io.StringIO("a,b\nc,d\ne,f\n")
r3 = csv.reader(buf3)
assert len(r3) == 3
assert r3[0] == ['a', 'b']
assert r3[-1] == ['e', 'f']
print("PASS csv.reader len/index")

# 4. csv.reader line_num
buf4 = io.StringIO("a,b\n")
r4 = csv.reader(buf4)
assert hasattr(r4, 'line_num')
print("PASS csv.reader line_num")

# 5. bytearray.fromhex
ba = bytearray.fromhex('48656c6c6f')
assert ba == bytearray(b'Hello')
print("PASS bytearray.fromhex")

# 6. bytes.fromhex still works
b = bytes.fromhex('48656c6c6f')
assert b == b'Hello'
print("PASS bytes.fromhex")

# 7. fromhex round-trip
original = bytearray(b'\xde\xad\xbe\xef')
assert bytearray.fromhex(original.hex()) == original
print("PASS fromhex round-trip")

# 8. delattr on modules
import sys
sys._phase147_test = 'hello'
assert sys._phase147_test == 'hello'
delattr(sys, '_phase147_test')
assert not hasattr(sys, '_phase147_test')
print("PASS delattr on module")

# 9. del module.attr (opcode path)
sys._phase147_test2 = 42
del sys._phase147_test2
assert not hasattr(sys, '_phase147_test2')
print("PASS del module.attr")

print("All phase 147 tests passed")
