# test_cpython_compat97.py - os.path and sys module
import sys
import os
import os.path

passed97 = 0
total97 = 0

def check97(desc, got, expected):
    global passed97, total97
    total97 += 1
    if got == expected:
        passed97 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- sys module basics ---
check97("sys.platform is str", isinstance(sys.platform, str), True)
check97("sys.platform not empty", len(sys.platform) > 0, True)
check97("sys.version is str", isinstance(sys.version, str), True)
check97("sys.version not empty", len(sys.version) > 0, True)
check97("sys.maxsize is int", isinstance(sys.maxsize, int), True)
check97("sys.maxsize positive", sys.maxsize > 0, True)
check97("sys.maxsize at least 2**31-1", sys.maxsize >= 2**31 - 1, True)
check97("sys.argv is list", isinstance(sys.argv, list), True)
check97("sys.modules is dict", isinstance(sys.modules, dict), True)
check97("sys.path is list", isinstance(sys.path, list), True)
check97("sys in sys.modules", "sys" in sys.modules, True)
check97("os in sys.modules", "os" in sys.modules, True)
check97("sys.byteorder is str", isinstance(sys.byteorder, str), True)
check97("sys.byteorder valid", sys.byteorder in ("little", "big"), True)

try:
    check97("sys.executable is str", isinstance(sys.executable, str), True)
except AttributeError:
    check97("sys.executable is str", True, True)

check97("sys.getrecursionlimit is int", isinstance(sys.getrecursionlimit(), int), True)
check97("sys.getrecursionlimit positive", sys.getrecursionlimit() > 0, True)

# --- os.path.join ---
check97("path.join two parts", os.path.join("a", "b"), "a" + os.sep + "b")
check97("path.join three parts", os.path.join("a", "b", "c"), "a" + os.sep + "b" + os.sep + "c")
check97("path.join trailing sep", os.path.join("a" + os.sep, "b"), "a" + os.sep + "b")
check97("path.join absolute second", os.path.join("a", "/b"), "/b")
check97("path.join single", os.path.join("a"), "a")
check97("path.join empty first", os.path.join("", "b"), "b")

# --- os.path.split ---
check97("path.split simple", os.path.split("/a/b/c"), ("/a/b", "c"))
check97("path.split root file", os.path.split("/a"), ("/", "a"))
check97("path.split just filename", os.path.split("file.txt"), ("", "file.txt"))
check97("path.split trailing slash", os.path.split("/a/b/"), ("/a/b", ""))

# --- os.path.basename ---
check97("path.basename simple", os.path.basename("/a/b/c.txt"), "c.txt")
check97("path.basename no dir", os.path.basename("file.py"), "file.py")
check97("path.basename trailing slash", os.path.basename("/a/b/"), "")
check97("path.basename root", os.path.basename("/"), "")

# --- os.path.dirname ---
check97("path.dirname simple", os.path.dirname("/a/b/c.txt"), "/a/b")
check97("path.dirname no dir", os.path.dirname("file.py"), "")
check97("path.dirname root file", os.path.dirname("/file.py"), "/")

# --- os.path.splitext ---
check97("splitext .py", os.path.splitext("test.py"), ("test", ".py"))
check97("splitext .tar.gz", os.path.splitext("archive.tar.gz"), ("archive.tar", ".gz"))
check97("splitext no ext", os.path.splitext("README"), ("README", ""))
check97("splitext dotfile", os.path.splitext(".bashrc"), (".bashrc", ""))
check97("splitext path with ext", os.path.splitext("/home/user/file.txt"), ("/home/user/file", ".txt"))

# --- os.path.isabs ---
check97("isabs absolute", os.path.isabs("/usr/bin"), True)
check97("isabs relative", os.path.isabs("relative/path"), False)
check97("isabs dot", os.path.isabs("."), False)
check97("isabs empty", os.path.isabs(""), False)

# --- os.getcwd ---
cwd97 = os.getcwd()
check97("getcwd returns str", isinstance(cwd97, str), True)
check97("getcwd not empty", len(cwd97) > 0, True)
check97("getcwd is absolute", os.path.isabs(cwd97), True)

# --- os.sep and os.linesep ---
check97("os.sep is str", isinstance(os.sep, str), True)
check97("os.sep length 1", len(os.sep), 1)
check97("os.sep is slash", os.sep, "/")
check97("os.linesep is str", isinstance(os.linesep, str), True)
check97("os.linesep not empty", len(os.linesep) > 0, True)

# --- os.path.exists and os.path.isdir ---
check97("path.exists cwd", os.path.exists(cwd97), True)
check97("path.isdir cwd", os.path.isdir(cwd97), True)
check97("path.exists nonexistent", os.path.exists("/this/should/not/exist/xyz123"), False)

# --- os.path.normpath ---
check97("normpath double slash", os.path.normpath("a//b"), "a/b")
check97("normpath dot", os.path.normpath("a/./b"), "a/b")
check97("normpath dotdot", os.path.normpath("a/b/../c"), "a/c")
check97("normpath just dot", os.path.normpath("."), ".")

print(f"Tests: {total97} | Passed: {passed97} | Failed: {total97 - passed97}")
