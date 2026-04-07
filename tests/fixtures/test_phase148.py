# Phase 148: __index__ protocol, memoryview mutable, protocol probes

# 1. __index__ with bin/oct/hex
class MyIndex:
    def __index__(self):
        return 5

x = MyIndex()
assert bin(x) == '0b101'
assert oct(x) == '0o5'
assert hex(x) == '0x5'
print("PASS __index__ bin/oct/hex")

# 2. __index__ with list indexing
data = [10, 20, 30, 40, 50, 60]
assert data[x] == 60  # index 5
print("PASS __index__ list index")

# 3. memoryview on bytearray supports mutation
b = bytearray(b'hello')
m = memoryview(b)
assert m[0] == ord('h')
m[0] = ord('H')
assert bytes(m) == b'Hello'
print("PASS memoryview mutable bytearray")

# 4. memoryview on bytes is read-only (just reads)
b2 = b'world'
m2 = memoryview(b2)
assert m2[0] == ord('w')
assert bytes(m2[1:3]) == b'or'
print("PASS memoryview bytes read")

# 5. bytearray.fromhex classmethod
ba = bytearray.fromhex('deadbeef')
assert len(ba) == 4
assert ba[0] == 0xde
print("PASS bytearray.fromhex")

# 6. delattr on module (both builtin and opcode)
import sys
sys._test148 = 'temp'
assert hasattr(sys, '_test148')
delattr(sys, '_test148')
assert not hasattr(sys, '_test148')
sys._test148b = 'temp2'
del sys._test148b
assert not hasattr(sys, '_test148b')
print("PASS delattr on module")

# 7. csv.reader as iterator
import csv, io
buf = io.StringIO("a,b,c\n1,2,3\n4,5,6\n")
r = csv.reader(buf)
assert next(r) == ['a', 'b', 'c']
rows = [next(r), next(r)]
assert len(rows) == 2
try:
    next(r)
    assert False
except StopIteration:
    pass
print("PASS csv.reader iterator")

print("All phase 148 tests passed")
