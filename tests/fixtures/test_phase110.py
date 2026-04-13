# test_phase110.py — Socket constants, Decimal deepening, Pickle classes

# ── Socket constants ──
import socket

assert socket.TCP_NODELAY == 1
assert socket.SO_KEEPALIVE == 9
assert socket.SO_RCVBUF == 8
assert socket.SO_SNDBUF == 7
assert socket.SO_REUSEPORT == 15
assert socket.SOCK_RAW == 3
assert socket.SOL_TCP == 6
assert socket.IPPROTO_IP == 0
assert socket.has_ipv6 == True
assert socket.SOMAXCONN >= 128  # system-dependent (128 or 4096 on Linux)
assert socket.INADDR_ANY == 0
assert socket.INADDR_LOOPBACK == 0x7F000001
assert socket.MSG_PEEK == 2
assert socket.MSG_WAITALL == 256

# inet_aton / inet_ntoa
packed = socket.inet_aton("127.0.0.1")
assert len(packed) == 4
assert packed[0] == 127 and packed[3] == 1
back = socket.inet_ntoa(packed)
assert back == "127.0.0.1"

# htons / ntohs round-trip
v = socket.htons(80)
assert socket.ntohs(v) == 80
v2 = socket.htonl(8080)
assert socket.ntohl(v2) == 8080

# ── Decimal deepening ──
from decimal import Decimal

# as_tuple
d = Decimal("123.45")
t = d.as_tuple()
assert t[0] == 0  # sign
assert list(t[1]) == [1, 2, 3, 4, 5]  # digits
assert t[2] == -2  # exponent

d2 = Decimal("-42")
t2 = d2.as_tuple()
assert t2[0] == 1  # negative

# copy_sign
d3 = Decimal("10")
d4 = Decimal("-5")
r = d3.copy_sign(d4)
assert str(r).startswith("-")

# __pow__
d5 = Decimal("2")
d6 = Decimal("10")
r2 = d5 ** d6
assert float(r2) == 1024.0

# __mod__
d7 = Decimal("10")
d8 = Decimal("3")
r3 = d7 % d8
assert float(r3) == 1.0

# __floordiv__
r4 = d7 // d8
assert float(r4) == 3.0

# __bool__
assert bool(Decimal("1")) == True
assert bool(Decimal("0")) == False

# max/min
assert Decimal("10").max(Decimal("5")) == Decimal("10")
assert Decimal("10").min(Decimal("5")) == Decimal("5")

# ── Pickle classes ──
import pickle

# Pickler/Unpickler exist as classes
assert hasattr(pickle, 'Pickler')
assert hasattr(pickle, 'Unpickler')

# PicklingError/UnpicklingError are classes, not strings
assert hasattr(pickle, 'PicklingError')
assert hasattr(pickle, 'UnpicklingError')
assert hasattr(pickle, 'PickleError')

# dumps/loads still work
data = pickle.dumps([1, 2, "hello", True, None])
result = pickle.loads(data)
assert result == [1, 2, "hello", True, None]

# Nested structures
nested = {"a": [1, 2], "b": (3, 4), "c": {"d": 5}}
data2 = pickle.dumps(nested)
result2 = pickle.loads(data2)
assert result2["a"] == [1, 2]
assert result2["c"]["d"] == 5

# ── curses (verify from phase109) ──
import curses
assert curses.COLOR_BLACK == 0

print("phase110: all tests passed")
