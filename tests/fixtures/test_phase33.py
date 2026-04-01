# Phase 33: JSON, regex, enum, and more stdlib tests
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── json module ──
import json

# json.dumps
test("json.dumps string", json.dumps("hello") == '"hello"')
test("json.dumps int", json.dumps(42) == '42')
test("json.dumps float", json.dumps(3.14) == '3.14')
test("json.dumps bool true", json.dumps(True) == 'true')
test("json.dumps bool false", json.dumps(False) == 'false')
test("json.dumps null", json.dumps(None) == 'null')
test("json.dumps list", json.dumps([1, 2, 3]) == '[1, 2, 3]')
test("json.dumps dict", json.dumps({"a": 1}) == '{"a": 1}')

# json.loads
test("json.loads string", json.loads('"hello"') == "hello")
test("json.loads int", json.loads('42') == 42)
test("json.loads float", json.loads('3.14') == 3.14)
test("json.loads bool", json.loads('true') == True)
test("json.loads null", json.loads('null') == None)
test("json.loads array", json.loads('[1, 2, 3]') == [1, 2, 3])
test("json.loads object", json.loads('{"a": 1}') == {"a": 1})
test("json.loads nested", json.loads('{"a": [1, {"b": 2}]}')['a'][1]['b'] == 2)

# Round-trip
data = {"name": "test", "values": [1, 2.5, True, None], "nested": {"x": 10}}
test("json round-trip", json.loads(json.dumps(data)) == data)

# ── re module ──
import re

# re.search
m = re.search(r'\d+', 'abc 123 def')
test("re.search match", m is not None)
test("re.search group", m.group() == '123')

# re.match
m = re.match(r'\d+', '123 abc')
test("re.match match", m is not None)
test("re.match group", m.group() == '123')

m = re.match(r'\d+', 'abc 123')
test("re.match no match", m is None)

# re.findall
result = re.findall(r'\d+', 'a1 b22 c333')
test("re.findall", result == ['1', '22', '333'])

# re.sub
result = re.sub(r'\d+', 'X', 'a1 b22 c333')
test("re.sub", result == 'aX bX cX')

# re.split
result = re.split(r'[,;]', 'a,b;c,d')
test("re.split", result == ['a', 'b', 'c', 'd'])

# ── math module ──
import math

test("math.pi", abs(math.pi - 3.14159265) < 0.001)
test("math.e", abs(math.e - 2.71828) < 0.001)
test("math.sqrt", math.sqrt(16) == 4.0)
test("math.floor", math.floor(3.7) == 3)
test("math.ceil", math.ceil(3.2) == 4)
test("math.abs/fabs", math.fabs(-5) == 5.0)
test("math.pow", math.pow(2, 10) == 1024.0)
test("math.log", abs(math.log(math.e) - 1.0) < 0.001)
test("math.log10", abs(math.log10(100) - 2.0) < 0.001)
test("math.sin", abs(math.sin(0) - 0.0) < 0.001)
test("math.cos", abs(math.cos(0) - 1.0) < 0.001)
test("math.gcd", math.gcd(12, 8) == 4)
test("math.factorial", math.factorial(5) == 120)
test("math.isnan", math.isnan(float('nan')))
test("math.isinf", math.isinf(float('inf')))

# ── os module ──
import os

test("os.getcwd", len(os.getcwd()) > 0)
test("os.sep", os.sep == '/')
test("os.path.exists", os.path.exists('.'))
test("os.path.isdir", os.path.isdir('.'))
test("os.path.join", os.path.join('/tmp', 'test') == '/tmp/test')
test("os.path.basename", os.path.basename('/tmp/test.py') == 'test.py')
test("os.path.dirname", os.path.dirname('/tmp/test.py') == '/tmp')
test("os.path.splitext", os.path.splitext('test.py') == ('test', '.py'))

# ── string module ──
import string

test("string.ascii_lowercase", string.ascii_lowercase == 'abcdefghijklmnopqrstuvwxyz')
test("string.ascii_uppercase", string.ascii_uppercase == 'ABCDEFGHIJKLMNOPQRSTUVWXYZ')
test("string.digits", string.digits == '0123456789')
test("string.punctuation length", len(string.punctuation) > 0)

# ── functools ──
from functools import reduce, partial

test("reduce sum", reduce(lambda a, b: a + b, [1, 2, 3, 4]) == 10)
test("reduce with initial", reduce(lambda a, b: a + b, [1, 2, 3], 10) == 16)

add5 = partial(lambda x, y: x + y, 5)
test("partial", add5(3) == 8)

# ── copy ──
import copy

original = [1, [2, 3], 4]
shallow = copy.copy(original)
test("copy.copy", shallow == original)
test("copy.copy is different", shallow is not original)

deep = copy.deepcopy(original)
test("copy.deepcopy", deep == original)

# ── hashlib ──
import hashlib

h = hashlib.md5(b"hello")
test("hashlib.md5 type", h is not None)
digest = h.hexdigest()
test("hashlib.md5 hexdigest", len(digest) == 32)
test("hashlib.md5 value", digest == "5d41402abc4b2a76b9719d911017c592")

h2 = hashlib.sha256(b"hello")
digest2 = h2.hexdigest()
test("hashlib.sha256 hexdigest", len(digest2) == 64)

# ── operator module ──
import operator

test("operator.add", operator.add(3, 4) == 7)
test("operator.mul", operator.mul(3, 4) == 12)
test("operator.sub", operator.sub(10, 3) == 7)
test("operator.eq", operator.eq(3, 3) == True)
test("operator.ne", operator.ne(3, 4) == True)
test("operator.lt", operator.lt(3, 4) == True)
test("operator.gt", operator.gt(4, 3) == True)

# ── typing module ──
import typing

test("typing.List", hasattr(typing, 'List'))
test("typing.Dict", hasattr(typing, 'Dict'))
test("typing.Optional", hasattr(typing, 'Optional'))

# ── sys module ──
import sys

test("sys.maxsize", sys.maxsize > 0)
test("sys.version", len(sys.version) > 0)
test("sys.platform", len(sys.platform) > 0)
test("sys.path", isinstance(sys.path, list))
test("sys.argv", isinstance(sys.argv, list))

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
assert failed == 0, f"{failed} tests failed!"
print("ALL PHASE 33 TESTS PASSED")
