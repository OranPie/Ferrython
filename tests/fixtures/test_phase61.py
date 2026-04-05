"""Phase 61: Expanded pathlib.Path methods, dataclasses __post_init__, enum auto() & IntEnum."""

import os
import pathlib
from pathlib import Path
from dataclasses import dataclass
from enum import Enum, IntEnum, auto

results = []
def test(name, condition):
    results.append((name, condition))

# ── pathlib.Path methods ──

# exists / is_dir / is_file on known paths
p_cwd = Path(".")
test("path_exists_dot", p_cwd.exists())
test("path_is_dir_dot", p_cwd.is_dir())
test("path_is_file_dot_false", not p_cwd.is_file())

p_no = Path("/nonexistent_path_abc123")
test("path_not_exists", not p_no.exists())

# __truediv__ (/ operator) builds joined path
joined = Path("/tmp") / "sub" / "file.txt"
test("path_truediv_str", str(joined) == "/tmp/sub/file.txt")

# __str__ and __repr__
test("path_str", str(Path("/a/b")) == "/a/b")
test("path_repr", repr(Path("/a/b")) == "PosixPath('/a/b')")

# __eq__
test("path_eq_same", Path("/a/b") == Path("/a/b"))
test("path_neq_diff", not (Path("/a") == Path("/b")))

# __fspath__
p = Path("/some/path")
test("path_fspath", os.fspath(p) == "/some/path")

# write_text / read_text round-trip
test_dir = "/tmp/_ferrython_phase61_test"
try:
    os.makedirs(test_dir)
except:
    pass
test_file = Path(test_dir) / "hello.txt"
test_file.write_text("hello world")
content = test_file.read_text()
test("path_write_read_text", content == "hello world")

# read_bytes
data = test_file.read_bytes()
test("path_read_bytes", data == b"hello world")

# is_file on written file
test("path_is_file_written", test_file.is_file())

# unlink
test_file.unlink()
test("path_unlink", not test_file.exists())

# mkdir and iterdir
sub_dir = Path(test_dir) / "subdir"
sub_dir.mkdir()
test("path_mkdir_exists", sub_dir.is_dir())

# Write a file inside subdir for iterdir test
inner = sub_dir / "inner.txt"
inner.write_text("inner")
entries = sub_dir.iterdir()
test("path_iterdir_nonempty", len(entries) >= 1)

# glob
matches = sub_dir.glob("*.txt")
test("path_glob", len(matches) >= 1)

# resolve returns a Path with absolute path
resolved = Path(".").resolve()
test("path_resolve_absolute", str(resolved).startswith("/"))

# Cleanup
inner.unlink()
os.rmdir(str(sub_dir))
os.rmdir(test_dir)

# ── dataclasses __post_init__ ──

@dataclass
class Point:
    x: int
    y: int
    magnitude: float = 0.0

    def __post_init__(self):
        self.magnitude = (self.x ** 2 + self.y ** 2) ** 0.5

pt = Point(3, 4)
test("dc_post_init_magnitude", pt.magnitude == 5.0)

@dataclass
class Greeting:
    first: str
    last: str
    full: str = ""

    def __post_init__(self):
        self.full = self.first + " " + self.last

g = Greeting("John", "Doe")
test("dc_post_init_full", g.full == "John Doe")

# ── enum auto() ──

class Color(Enum):
    RED = auto()
    GREEN = auto()
    BLUE = auto()

# auto() should assign incrementing int values
test("enum_auto_red", Color.RED.value >= 1)
test("enum_auto_green", Color.GREEN.value == Color.RED.value + 1)
test("enum_auto_blue", Color.BLUE.value == Color.GREEN.value + 1)

# ── IntEnum ──

class Prio(IntEnum):
    LOW = 1
    MED = 2
    HIGH = 3

# IntEnum members compare with ints
test("intenum_eq_int", Prio.LOW == 1)
test("intenum_lt", Prio.LOW < Prio.HIGH)
test("intenum_gt", Prio.HIGH > Prio.MED)
test("intenum_le", Prio.LOW <= 1)
test("intenum_ge", Prio.HIGH >= 3)

# IntEnum arithmetic
test("intenum_add", Prio.LOW + 10 == 11)
test("intenum_sub", Prio.HIGH - 1 == 2)

# IntEnum retains name and value
test("intenum_name", Prio.HIGH.name == "HIGH")
test("intenum_value", Prio.HIGH.value == 3)

# ── Summary ──

failed = [(name, ok) for name, ok in results if not ok]
if failed:
    for name, _ in failed:
        print("FAIL:", name)
    raise AssertionError(f"{len(failed)} test(s) failed")
print(f"All {len(results)} phase-61 tests passed")
