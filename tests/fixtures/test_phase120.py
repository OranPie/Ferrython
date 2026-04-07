# Phase 120: HMAC update/copy, pickle file I/O, operator indexOf/countOf, deep module tests

# --- 1. HMAC with update and copy ---
import hmac

h1 = hmac.new(b"key", b"hello", "sha256")
h2 = hmac.new(b"key", digestmod="sha256")
h2.update(b"hel")
h2.update(b"lo")
assert h1.hexdigest() == h2.hexdigest(), "HMAC incremental update mismatch"

h3 = h1.copy()
h1.update(b" world")
assert h1.hexdigest() != h3.hexdigest(), "HMAC copy should be independent"
assert h3.hexdigest() == h2.hexdigest(), "HMAC copy should match original"
assert h1.digest_size == 32
assert h1.block_size == 64
print("HMAC update/copy: OK")

# --- 2. Pickle with file objects ---
import pickle
import tempfile
import os

path = os.path.join(tempfile.gettempdir(), "test_phase120.pickle")
try:
    data = {"name": "test", "values": [1, 2, 3], "flag": True}
    with open(path, "wb") as f:
        pickle.dump(data, f)
    with open(path, "rb") as f:
        loaded = pickle.load(f)
    assert loaded == data
    print("Pickle file I/O: OK")
finally:
    try: os.remove(path)
    except: pass

# --- 3. operator.indexOf and countOf ---
import operator

assert operator.indexOf([10, 20, 30], 20) == 1
assert operator.countOf([1, 2, 2, 3, 2], 2) == 3
assert operator.countOf("hello", "l") == 2
print("operator indexOf/countOf: OK")

# --- 4. Deep itertools ---
import itertools

# accumulate with function
result = list(itertools.accumulate([1, 2, 3, 4], lambda a, b: a * b))
assert result == [1, 2, 6, 24]

# chain.from_iterable
result = list(itertools.chain.from_iterable(["abc", "def"]))
assert result == ["a", "b", "c", "d", "e", "f"]

# tee
a, b = itertools.tee(range(5))
assert list(a) == [0, 1, 2, 3, 4]
assert list(b) == [0, 1, 2, 3, 4]
print("Deep itertools: OK")

# --- 5. functools completeness ---
import functools

@functools.lru_cache(maxsize=None)
def factorial(n):
    return n * factorial(n-1) if n else 1

assert factorial(10) == 3628800
info = factorial.cache_info()
print(f"Cache info: {info}")

@functools.total_ordering
class Temperature:
    def __init__(self, value):
        self.value = value
    def __eq__(self, other):
        return self.value == other.value
    def __lt__(self, other):
        return self.value < other.value

assert Temperature(30) < Temperature(40)
assert Temperature(40) > Temperature(30)
assert Temperature(30) <= Temperature(30)
print("functools total_ordering: OK")

# --- 6. Deep json ---
import json

class CustomEncoder(json.JSONEncoder):
    def default(self, obj):
        if isinstance(obj, set):
            return sorted(list(obj))
        return super().default(obj)

result = json.dumps({"items": {1, 2, 3}}, cls=CustomEncoder)
parsed = json.loads(result)
assert parsed["items"] == [1, 2, 3]
print("json custom encoder: OK")

# --- 7. configparser ---
import configparser

cfg = configparser.ConfigParser()
cfg.read_string("[section]\nkey = value\nnum = 42")
assert cfg.getint("section", "num") == 42
cfg.set("section", "new_key", "new_value")
assert cfg.get("section", "new_key") == "new_value"
print("configparser: OK")

# --- 8. csv round-trip ---
import csv
import io

output = io.StringIO()
writer = csv.DictWriter(output, fieldnames=["name", "score"])
writer.writeheader()
writer.writerow({"name": "Alice", "score": "95"})
writer.writerow({"name": "Bob", "score": "87"})
text = output.getvalue()

reader = csv.DictReader(io.StringIO(text))
rows = list(reader)
assert rows[0]["name"] == "Alice"
assert rows[1]["score"] == "87"
print("csv round-trip: OK")

print("All phase 120 tests passed!")
