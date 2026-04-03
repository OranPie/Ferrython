# Phase 30: namedtuple, logging, subprocess, pathlib, threading, csv, base64, etc.
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── namedtuple ──
from collections import namedtuple

Point = namedtuple('Point', ['x', 'y'])
p = Point(3, 4)
test("namedtuple field access", p.x == 3 and p.y == 4)
test("namedtuple indexing", p[0] == 3 and p[1] == 4)

# Test repr
r = repr(p)
test("namedtuple repr", "Point" in r and "x=3" in r and "y=4" in r)

# _asdict
d = p._asdict()
test("namedtuple _asdict", d['x'] == 3 and d['y'] == 4)

# String field names
Color = namedtuple('Color', 'r g b')
c = Color(255, 128, 0)
test("namedtuple string fields", c.r == 255 and c.g == 128 and c.b == 0)

# _fields
test("namedtuple _fields", p._fields == ('x', 'y'))

# ── logging ──
import logging

test("logging DEBUG level", logging.DEBUG == 10)
test("logging INFO level", logging.INFO == 20)
test("logging WARNING level", logging.WARNING == 30)
test("logging ERROR level", logging.ERROR == 40)
test("logging CRITICAL level", logging.CRITICAL == 50)

logger = logging.getLogger("test_logger")
test("logger has name", logger.name == "test_logger")
test("logger has level", logger.level == 30)

# basicConfig should not error
logging.basicConfig()
test("basicConfig no error", True)

# ── threading ──
import threading

test("threading active_count", threading.active_count() == 1)
ct = threading.current_thread()
test("current_thread name", ct.name == "MainThread")
test("current_thread is_alive", ct.is_alive())

# ── csv ──
import csv

lines = ["a,b,c", "1,2,3", "4,5,6"]
reader = csv.reader(lines)
test("csv reader length", len(reader) == 3)
test("csv reader first row", reader[0] == ['a', 'b', 'c'])
test("csv reader data row", reader[1] == ['1', '2', '3'])
test("csv QUOTE_ALL", csv.QUOTE_ALL == 1)

# ── base64 ──
import base64

# Test b64encode
encoded = base64.b64encode(b"Hello")
test("b64encode", encoded == b"SGVsbG8=")

# Test b64decode
decoded = base64.b64decode(b"SGVsbG8=")
test("b64decode", decoded == b"Hello")

# b16encode
hex_encoded = base64.b16encode(b"AB")
test("b16encode", hex_encoded == b"4142")

# ── pathlib ──
import pathlib

p = pathlib.Path("/tmp/test.txt")
test("pathlib name", p.name == "test.txt")
test("pathlib stem", p.stem == "test")
test("pathlib suffix", p.suffix == ".txt")
test("pathlib parent", str(p.parent) == "/tmp")

# ── tempfile ──
import tempfile

tmpdir = tempfile.gettempdir()
test("tempfile gettempdir", len(tmpdir) > 0)

# ── shutil ──
import shutil
test("shutil module exists", True)

# ── glob ──
import glob
test("glob module exists", True)

# ── fnmatch ──
import fnmatch
test("fnmatch basic", fnmatch.fnmatch("test.py", "*.py"))
test("fnmatch no match", not fnmatch.fnmatch("test.py", "*.txt"))
test("fnmatch exact", fnmatch.fnmatch("hello", "hello"))

# ── pprint ──
import pprint
test("pprint module exists", True)

# ── argparse ──
import argparse
test("argparse module exists", True)

# ── unittest ──
import unittest
test("unittest module exists", True)

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
assert failed == 0, f"{failed} tests failed!"
print("ALL PHASE 30 TESTS PASSED")
