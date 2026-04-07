# Test sys.version_info named attrs, sys.implementation, platform.uname

import sys

# --- sys.version_info ---
vi = sys.version_info
assert vi.major == 3, f"Expected major=3, got {vi.major}"
assert vi.minor == 8, f"Expected minor=8, got {vi.minor}"
assert vi.micro == 0
assert vi.releaselevel == "final"
assert vi.serial == 0
# Test comparison with tuple
assert vi >= (3, 7), "version_info should be >= (3, 7)"
assert vi >= (3, 8), "version_info should be >= (3, 8)"
assert not (vi >= (3, 9)), "version_info should not be >= (3, 9)"
assert vi < (3, 9), "version_info should be < (3, 9)"
assert not (vi < (3, 8)), "version_info should not be < (3, 8)"
# Test indexing
assert vi[0] == 3
assert vi[1] == 8
print("sys.version_info: OK")

# --- sys.implementation ---
impl = sys.implementation
assert impl.name == "ferrython"
assert impl.cache_tag == "ferrython-38"
print("sys.implementation: OK")

# --- platform module ---
import platform

v = platform.python_version()
assert v == "3.8.0", f"Expected '3.8.0', got '{v}'"

vt = platform.python_version_tuple()
assert vt[0] == "3" and vt[1] == "8" and vt[2] == "0"

s = platform.system()
assert s in ("Linux", "Darwin", "Windows", "FreeBSD"), f"Unexpected system: {s}"

m = platform.machine()
assert len(m) > 0

un = platform.uname()
assert un.system in ("Linux", "Darwin", "Windows", "FreeBSD")
assert len(un.machine) > 0

comp = platform.python_compiler()
assert "Ferrython" in comp or "Rust" in comp

impl_name = platform.python_implementation()
assert impl_name == "Ferrython"

arch = platform.architecture()
assert arch[0] in ("32bit", "64bit")

print("platform: OK")

# --- sys module additional checks ---
assert sys.maxsize > 0
assert sys.maxunicode == 0x10FFFF
assert sys.byteorder in ("little", "big")
assert sys.getdefaultencoding() == "utf-8"
assert sys.getfilesystemencoding() == "utf-8"
assert sys.getrecursionlimit() > 0
assert sys.prefix is not None
assert sys.exec_prefix is not None
assert sys.base_prefix is not None

# sys.flags should exist
assert sys.flags is not None

# sys.float_info should exist
assert sys.float_info is not None

# sys.intern should return the same string
s = sys.intern("hello")
assert s == "hello"

print("sys extras: OK")

print("All phase 118 tests passed!")
