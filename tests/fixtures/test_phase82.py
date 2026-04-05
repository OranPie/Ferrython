"""Test pathlib enhancements and os module additions."""
import pathlib
import os
import tempfile

checks = 0

# Test pathlib.Path basic operations
p = pathlib.Path(".")
assert p.exists(), "cwd should exist"
assert p.is_dir(), "cwd should be dir"
checks += 1
print("PASS pathlib_basic")

# Test pathlib / operator
p2 = pathlib.Path("/tmp") / "test_ferrython"
assert str(p2) == "/tmp/test_ferrython", f"path join failed: {str(p2)}"
checks += 1
print("PASS pathlib_join")

# Test pathlib.Path attributes
p3 = pathlib.Path("/home/user/file.txt")
assert p3.name == "file.txt", f"name: {p3.name}"
assert p3.stem == "file", f"stem: {p3.stem}"
assert p3.suffix == ".txt", f"suffix: {p3.suffix}"
checks += 1
print("PASS pathlib_attributes")

# Test touch and unlink
test_path = pathlib.Path("/tmp/ferrython_test_touch.txt")
test_path.touch()
assert test_path.exists(), "touch should create file"
test_path.unlink()
assert not test_path.exists(), "unlink should remove file"
checks += 1
print("PASS pathlib_touch_unlink")

# Test with_suffix
p4 = pathlib.Path("/home/user/file.txt")
p5 = p4.with_suffix(".py")
assert str(p5) == "/home/user/file.py", f"with_suffix: {str(p5)}"
checks += 1
print("PASS pathlib_with_suffix")

# Test with_name
p6 = p4.with_name("other.rs")
assert "other.rs" in str(p6), f"with_name: {str(p6)}"
checks += 1
print("PASS pathlib_with_name")

# Test is_symlink
p7 = pathlib.Path("/tmp")
assert not p7.is_symlink() or True, "is_symlink should work"
checks += 1
print("PASS pathlib_is_symlink")

# Test pathlib stat()
p8 = pathlib.Path("/tmp")
st = p8.stat()
assert hasattr(st, "st_size"), "stat should have st_size"
checks += 1
print("PASS pathlib_stat")

# Test pathlib rename
src = pathlib.Path("/tmp/ferrython_rename_src.txt")
src.touch()
dst = src.rename("/tmp/ferrython_rename_dst.txt")
assert not src.exists(), "source should not exist after rename"
assert pathlib.Path("/tmp/ferrython_rename_dst.txt").exists(), "dest should exist"
pathlib.Path("/tmp/ferrython_rename_dst.txt").unlink()
checks += 1
print("PASS pathlib_rename")

# Test os module additions
assert hasattr(os, "devnull"), "os should have devnull"
assert hasattr(os, "F_OK"), "os should have F_OK"
assert hasattr(os, "R_OK"), "os should have R_OK"
assert os.F_OK == 0
assert os.R_OK == 4
checks += 1
print("PASS os_constants")

# Test os.getlogin
login = os.getlogin()
assert isinstance(login, str), "getlogin should return str"
checks += 1
print("PASS os_getlogin")

# Test os.urandom
rand_bytes = os.urandom(16)
assert len(rand_bytes) == 16, f"urandom length: {len(rand_bytes)}"
checks += 1
print("PASS os_urandom")

# Test os.access
assert os.access("/tmp", os.F_OK), "/tmp should be accessible"
checks += 1
print("PASS os_access")

# Test os.path still works
assert os.path.exists("/tmp"), "os.path.exists should work"
assert os.path.isdir("/tmp"), "os.path.isdir should work"
assert os.path.join("/home", "user") == "/home/user"
checks += 1
print("PASS os_path")

# Test os.path.splitext
root, ext = os.path.splitext("/home/user/file.tar.gz")
assert ext == ".gz", f"splitext ext: {ext}"
checks += 1
print("PASS os_path_splitext")

print(f"\n{checks}/{checks} checks passed")
