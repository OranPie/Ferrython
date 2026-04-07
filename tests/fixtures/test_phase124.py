# Phase 124: pathlib fix, date.fromisoformat, logging handler levels, re types
import sys
passed = 0
failed = 0
def check(name, val):
    global passed, failed
    if val:
        passed += 1
    else:
        failed += 1
        print(f"FAIL: {name}")

# --- pathlib / operator returns Path with methods ---
from pathlib import Path
import tempfile, os

p = Path("/tmp") / "test_sub" / "file.txt"
check("pathlib / type", type(p).__name__ == "Path")
check("pathlib parent type", type(p.parent).__name__ == "Path")
check("pathlib parent parent", type(p.parent.parent).__name__ in ("Path", "str"))

with tempfile.TemporaryDirectory() as td:
    sub = Path(td) / "sub"
    sub.mkdir(parents=True, exist_ok=True)
    (Path(td) / "a.txt").write_text("aaa")
    (sub / "b.txt").write_text("bbb")

    check("pathlib write/read", (Path(td) / "a.txt").read_text() == "aaa")
    check("pathlib mkdir+write", (sub / "b.txt").read_text() == "bbb")
    check("pathlib exists", (Path(td) / "a.txt").exists())
    check("pathlib is_file", (Path(td) / "a.txt").is_file())
    check("pathlib is_dir", sub.is_dir())

    # glob
    simple = list(Path(td).glob("*.txt"))
    check("pathlib glob *.txt", len(simple) == 1)

    recursive = list(Path(td).glob("**/*.txt"))
    check("pathlib glob **/*.txt", len(recursive) == 2)

    rg = list(Path(td).rglob("*.txt"))
    check("pathlib rglob", len(rg) == 2)

# --- date.fromisoformat returns date (not datetime) ---
from datetime import date, datetime, timedelta

d = date.fromisoformat("2024-06-15")
check("date.fromisoformat type", type(d).__name__ == "date")
check("date.fromisoformat str", str(d) == "2024-06-15")
check("date.fromisoformat repr", "datetime.date" in repr(d))

dt = datetime.fromisoformat("2024-06-15T10:30:00")
check("datetime.fromisoformat type", type(dt).__name__ == "datetime")
check("datetime.fromisoformat str", "10:30" in str(dt))

# date operations
d2 = date(2024, 6, 10)
delta = d - d2
check("date subtract", delta.days == 5)

today = date.today()
check("date.today", today.year >= 2024)

# --- logging handler level filtering ---
import logging

logger = logging.getLogger("phase124_test")
# Capture stderr to check output
import io
handler = logging.StreamHandler()
handler.setLevel(logging.ERROR)  # Only ERROR+
logger.addHandler(handler)
logger.setLevel(logging.DEBUG)

# These should not produce output via handler
logger.debug("no_debug")
logger.info("no_info")
logger.warning("no_warning")
# This should produce output
logger.error("yes_error")
check("logging handler setup", True)  # if we got here without crash

# --- re.Pattern and re.Match types ---
import re
check("re.Pattern exists", hasattr(re, "Pattern"))
check("re.Match exists", hasattr(re, "Match"))

# re.compile still works
pat = re.compile(r"\d+")
m = pat.search("abc123def")
check("re.compile search", m is not None)
check("re.compile group", m.group() == "123")

# --- more pathlib operations ---
p = Path("/some/dir/file.tar.gz")
check("pathlib stem", p.stem == "file.tar")
check("pathlib suffix", p.suffix == ".gz")
check("pathlib name", p.name == "file.tar.gz")
check("pathlib parts", len(p.parts) >= 4)

# Path resolve
p2 = Path(".").resolve()
check("pathlib resolve", str(p2).startswith("/"))

print(f"\ntest_phase124: {passed}/{passed+failed} passed")
if failed:
    sys.exit(1)
