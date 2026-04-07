# Test os.replace, logging improvements, subprocess attrs, Counter ops, new modules

# --- os.replace ---
import os
import tempfile

tmpdir = tempfile.mkdtemp()
src = os.path.join(tmpdir, "src.txt")
dst = os.path.join(tmpdir, "dst.txt")

# Create src file
with open(src, "w") as f:
    f.write("hello")

# Create dst file (should be overwritten)
with open(dst, "w") as f:
    f.write("old")

os.replace(src, dst)
assert not os.path.exists(src), "source should be gone after replace"
with open(dst, "r") as f:
    assert f.read() == "hello", "dst should have src content"

# Cleanup
os.remove(dst)
os.rmdir(tmpdir)
print("os.replace: OK")

# --- Counter copy/clear ---
from collections import Counter
c = Counter(["a", "a", "b", "c", "c", "c"])
assert c["a"] == 2
assert c["c"] == 3

# Test counter_copy via collections module
import collections
c2 = collections.counter_copy(c)
assert c2["a"] == 2
assert c2["c"] == 3

# Test counter_clear
collections.counter_clear(c2)
# After clear, counter keys should be gone (only internal keys remain)
has_user_keys = False
for k in c2:
    s = str(k)
    if s not in ("__defaultdict_factory__", "__counter__", "True"):
        has_user_keys = True
assert not has_user_keys, "counter should be empty after clear"
print("Counter copy/clear: OK")

# --- logging improvements ---
import logging

# Test exception() method exists
logger = logging.getLogger("test_exc")
logger.setLevel(logging.ERROR)
assert hasattr(logger, "exception"), "Logger should have exception() method"

# Test log() method
logger2 = logging.getLogger("test_log")
logger2.setLevel(logging.DEBUG)
assert hasattr(logger2, "log"), "Logger should have log() method"

# Test removeHandler actually removes
handler = logging.StreamHandler()
logger3 = logging.getLogger("test_rm")
logger3.addHandler(handler)
assert logger3.hasHandlers(), "should have handlers after addHandler"
logger3.removeHandler(handler)
assert not logger3.hasHandlers(), "should have no handlers after removeHandler"
print("logging improvements: OK")

# --- subprocess Popen attributes ---
import subprocess
p = subprocess.Popen(["echo", "hello"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
assert hasattr(p, "args"), "Popen should have args attribute"
assert hasattr(p, "pid"), "Popen should have pid attribute"
assert hasattr(p, "stdout"), "Popen should have stdout attribute"
assert hasattr(p, "stderr"), "Popen should have stderr attribute"
assert hasattr(p, "stdin"), "Popen should have stdin attribute"
assert p.stdout is not None, "stdout should not be None when PIPE"
assert p.stderr is not None, "stderr should not be None when PIPE"
out, err = p.communicate()
assert b"hello" in out or "hello" in str(out), "should capture stdout"
print("Popen attributes: OK")

# --- urllib.robotparser ---
from urllib.robotparser import RobotFileParser
rp = RobotFileParser()
rp.parse([
    "User-agent: *",
    "Disallow: /private/",
    "Allow: /public/",
    "Sitemap: https://example.com/sitemap.xml",
])
assert rp.can_fetch("Googlebot", "https://example.com/public/page")
assert not rp.can_fetch("Googlebot", "https://example.com/private/secret")
assert rp.can_fetch("Googlebot", "https://example.com/other")
assert rp.site_maps() == ["https://example.com/sitemap.xml"]
print("urllib.robotparser: OK")

# --- mailbox ---
from mailbox import mbox, Maildir
mb = mbox("/tmp/test_mbox")
key1 = mb.add("From: test@example.com\nSubject: Hello\n\nBody")
key2 = mb.add("From: other@example.com\nSubject: World\n\nBody2")
assert len(mb) == 2
assert key1 in mb
msg = mb[key1]
assert "Hello" in msg
mb.remove(key1)
assert len(mb) == 1
assert key1 not in mb
mb.close()

md = Maildir("/tmp/test_maildir")
k = md.add("test message")
assert len(md) == 1
assert k in md
md.close()
print("mailbox: OK")

print("All phase 116 tests passed!")
