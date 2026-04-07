# Phase 123: New stdlib modules - shutil, glob, tempfile, logging, cmd, locale, sched, pydoc, mock

import os
import tempfile

# --- shutil ---
import shutil

# shutil.which
ls_path = shutil.which("ls")
assert ls_path is not None, "shutil.which('ls') should find ls"
assert "ls" in ls_path

# shutil.copy + rmtree
with tempfile.TemporaryDirectory() as tmpdir:
    src = os.path.join(tmpdir, "src.txt")
    dst = os.path.join(tmpdir, "dst.txt")
    with open(src, "w") as f:
        f.write("hello shutil")
    shutil.copy(src, dst)
    with open(dst) as f:
        assert f.read() == "hello shutil"
    
    # copytree
    srcdir = os.path.join(tmpdir, "srcdir")
    os.makedirs(srcdir)
    with open(os.path.join(srcdir, "a.txt"), "w") as f:
        f.write("aaa")
    dstdir = os.path.join(tmpdir, "dstdir")
    shutil.copytree(srcdir, dstdir)
    assert os.path.exists(os.path.join(dstdir, "a.txt"))

    # ignore_patterns
    ignore = shutil.ignore_patterns("*.pyc", "__pycache__")
    ignored = ignore("/some/path", ["a.py", "b.pyc", "__pycache__", "c.txt"])
    assert "b.pyc" in ignored
    assert "__pycache__" in ignored
    assert "a.py" not in ignored

print("shutil: OK")

# --- glob ---
import glob

assert glob.has_magic("*.py")
assert not glob.has_magic("hello.py")

with tempfile.TemporaryDirectory() as tmpdir:
    for name in ["a.py", "b.py", "c.txt"]:
        with open(os.path.join(tmpdir, name), "w") as f:
            f.write("")
    py_files = glob.glob(os.path.join(tmpdir, "*.py"))
    assert len(py_files) == 2, f"Expected 2 .py files, got {len(py_files)}"
    txt_files = glob.glob(os.path.join(tmpdir, "*.txt"))
    assert len(txt_files) == 1

print("glob: OK")

# --- tempfile ---
with tempfile.TemporaryDirectory() as d:
    assert os.path.isdir(d)
    p = os.path.join(d, "test.txt")
    with open(p, "w") as f:
        f.write("temp data")
    assert os.path.exists(p)
assert not os.path.exists(d), "TemporaryDirectory cleanup failed"

td = tempfile.mkdtemp(prefix="ftest_")
assert os.path.isdir(td)
os.rmdir(td)

assert tempfile.gettempdir() == "/tmp"

print("tempfile: OK")

# --- logging ---
import logging
import io

logger = logging.getLogger("test123")
logger.setLevel(logging.DEBUG)
stream = io.StringIO()
handler = logging.StreamHandler(stream)
handler.setFormatter(logging.Formatter('%(levelname)s:%(name)s:%(message)s'))
logger.addHandler(handler)

logger.debug("debug %d", 1)
logger.info("info %s", "msg")
logger.warning("warn")
logger.error("err")

output = stream.getvalue()
assert "DEBUG:test123:debug 1" in output
assert "INFO:test123:info msg" in output
assert "WARNING:test123:warn" in output
assert "ERROR:test123:err" in output

# Level filtering
logger2 = logging.getLogger("filtered")
logger2.setLevel(logging.ERROR)
s2 = io.StringIO()
h2 = logging.StreamHandler(s2)
logger2.addHandler(h2)
logger2.warning("nope")
logger2.error("yes")
assert "nope" not in s2.getvalue()
assert "yes" in s2.getvalue()

print("logging: OK")

# --- cmd ---
import cmd

class CalcCmd(cmd.Cmd):
    prompt = 'calc> '
    def do_add(self, arg):
        """Add two numbers"""
        parts = arg.split()
        return int(parts[0]) + int(parts[1])
    def do_quit(self, arg):
        return True

c = CalcCmd()
assert c.prompt == 'calc> '
result = c.onecmd("add 3 4")
assert result == 7, f"Expected 7, got {result}"
assert c.onecmd("quit") == True

print("cmd: OK")

# --- locale ---
import locale

assert locale.LC_ALL == 6
conv = locale.localeconv()
assert conv['decimal_point'] == '.'
assert locale.getpreferredencoding() == 'UTF-8'

print("locale: OK")

# --- sched ---
import sched
import time

events = []
s = sched.scheduler(time.monotonic, time.sleep)
t = time.monotonic()
s.enterabs(t, 2, lambda: events.append("low"))
s.enterabs(t, 1, lambda: events.append("high"))
assert not s.empty()
s.run()
assert s.empty()
assert events == ["high", "low"], f"Priority order wrong: {events}"

print("sched: OK")

# --- pydoc ---
import pydoc

desc = pydoc.describe(os)
assert "module" in desc.lower()
doc = pydoc.getdoc(list)
# May or may not have docstring, just ensure no crash

print("pydoc: OK")

# --- unittest.mock ---
from unittest.mock import Mock, MagicMock, ANY

m = Mock(return_value=42)
assert m() == 42
assert m.called
assert m.call_count == 1
m.assert_called_once()

m2 = Mock()
m2("a", "b")
m2("c")
assert m2.call_count == 2

mm = MagicMock()
assert len(mm) == 0
assert bool(mm) == True

# side_effect
m3 = Mock(side_effect=ValueError("test error"))
try:
    m3()
    assert False, "Should have raised"
except Exception:
    pass  # Good

print("unittest.mock: OK")

# --- tarfile ---
import tarfile
assert hasattr(tarfile, 'TarFile')
assert hasattr(tarfile, 'TarInfo')
assert hasattr(tarfile, 'is_tarfile')
assert tarfile.GNU_FORMAT == 1
assert tarfile.USTAR_FORMAT == 0

print("tarfile: OK")

print("ALL PHASE 123 TESTS PASSED")
