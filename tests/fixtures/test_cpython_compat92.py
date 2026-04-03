# test_cpython_compat92.py - More dict operations
from collections import OrderedDict, defaultdict

passed92 = 0
total92 = 0

def check92(desc, got, expected):
    global passed92, total92
    total92 += 1
    if got == expected:
        passed92 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# dict.update with another dict
d1 = {"a": 1, "b": 2}
d1.update({"b": 3, "c": 4})
check92("dict.update with dict", d1, {"a": 1, "b": 3, "c": 4})

# dict.update with kwargs
d2 = {"a": 1}
d2.update(b=2, c=3)
check92("dict.update with kwargs", d2, {"a": 1, "b": 2, "c": 3})

# dict.update with list of tuples
d3 = {}
d3.update([("x", 10), ("y", 20)])
check92("dict.update with list of tuples", d3, {"x": 10, "y": 20})

# dict.update overwrites existing
d4 = {"a": 1}
d4.update(a=99)
check92("dict.update overwrites existing via kwargs", d4, {"a": 99})

# dict.update with empty
d5 = {"a": 1}
d5.update({})
check92("dict.update with empty dict", d5, {"a": 1})

# dict comprehension basic
r6 = {k: v for k, v in [("a", 1), ("b", 2)]}
check92("dict comprehension from list of tuples", r6, {"a": 1, "b": 2})

# dict comprehension with condition
r7 = {k: v for k, v in {"a": 1, "b": 2, "c": 3}.items() if v > 1}
check92("dict comprehension with filter", r7, {"b": 2, "c": 3})

# dict comprehension squaring
r8 = {x: x * x for x in range(5)}
check92("dict comprehension squaring", r8, {0: 0, 1: 1, 2: 4, 3: 9, 4: 16})

# dict comprehension key transform
r9 = {k.upper(): v for k, v in {"hello": 1, "world": 2}.items()}
check92("dict comprehension key transform", r9, {"HELLO": 1, "WORLD": 2})

# nested dict comprehension
r10 = {i: {j: i * j for j in range(3)} for i in range(2)}
check92("nested dict comprehension", r10, {0: {0: 0, 1: 0, 2: 0}, 1: {0: 0, 1: 1, 2: 2}})

# dict.pop with default
d11 = {"a": 1, "b": 2}
v11 = d11.pop("c", 42)
check92("dict.pop missing key with default", v11, 42)
check92("dict.pop missing key dict unchanged", d11, {"a": 1, "b": 2})

# dict.pop existing key
d12 = {"a": 1, "b": 2}
v12 = d12.pop("a")
check92("dict.pop existing key returns value", v12, 1)
check92("dict.pop existing key removes it", d12, {"b": 2})

# dict.pop with None default
d13 = {"a": 1}
v13 = d13.pop("z", None)
check92("dict.pop with None default", v13, None)

# dict.setdefault existing key
d14 = {"a": 1}
v14 = d14.setdefault("a", 99)
check92("dict.setdefault existing key", v14, 1)
check92("dict.setdefault existing key dict unchanged", d14, {"a": 1})

# dict.setdefault missing key
d15 = {"a": 1}
v15 = d15.setdefault("b", 99)
check92("dict.setdefault missing key returns default", v15, 99)
check92("dict.setdefault missing key inserts", d15, {"a": 1, "b": 99})

# dict.setdefault missing key no default
d16 = {"a": 1}
v16 = d16.setdefault("b")
check92("dict.setdefault missing key no default returns None", v16, None)
check92("dict.setdefault missing key no default inserts None", d16, {"a": 1, "b": None})

# dict.get
d17 = {"a": 1}
check92("dict.get existing", d17.get("a"), 1)
check92("dict.get missing", d17.get("b"), None)
check92("dict.get missing with default", d17.get("b", 42), 42)

# dict.keys, values, items as lists
d18 = {"a": 1, "b": 2}
check92("dict.keys as sorted list", sorted(d18.keys()), ["a", "b"])
check92("dict.values as sorted list", sorted(d18.values()), [1, 2])
check92("dict.items as sorted list", sorted(d18.items()), [("a", 1), ("b", 2)])

# dict fromkeys
r19 = dict.fromkeys(["a", "b", "c"], 0)
check92("dict.fromkeys with value", r19, {"a": 0, "b": 0, "c": 0})

# dict fromkeys default None
r20 = dict.fromkeys(["x", "y"])
check92("dict.fromkeys default None", r20, {"x": None, "y": None})

# defaultdict basic
dd21 = defaultdict(int)
dd21["a"] += 1
dd21["a"] += 1
dd21["b"] += 5
check92("defaultdict int", dict(dd21), {"a": 2, "b": 5})

# defaultdict with list
dd22 = defaultdict(list)
dd22["a"].append(1)
dd22["a"].append(2)
dd22["b"].append(3)
check92("defaultdict list", dict(dd22), {"a": [1, 2], "b": [3]})

# defaultdict missing key creates default
dd23 = defaultdict(lambda: "missing")
v23 = dd23["nonexistent"]
check92("defaultdict lambda default", v23, "missing")

# defaultdict with set
dd24 = defaultdict(set)
dd24["a"].add(1)
dd24["a"].add(1)
dd24["a"].add(2)
check92("defaultdict set deduplicates", dict(dd24), {"a": {1, 2}})

# defaultdict default_factory None
dd25 = defaultdict(None, {"a": 1})
check92("defaultdict None factory get existing", dd25["a"], 1)
try:
    _ = dd25["b"]
    check92("defaultdict None factory missing key should raise", False, True)
except KeyError:
    check92("defaultdict None factory raises KeyError", True, True)

# OrderedDict basic
od26 = OrderedDict()
od26["b"] = 2
od26["a"] = 1
od26["c"] = 3
check92("OrderedDict preserves insertion order", list(od26.keys()), ["b", "a", "c"])

# OrderedDict move_to_end
od27 = OrderedDict([("a", 1), ("b", 2), ("c", 3)])
od27.move_to_end("a")
check92("OrderedDict move_to_end last", list(od27.keys()), ["b", "c", "a"])

# OrderedDict move_to_end first
od28 = OrderedDict([("a", 1), ("b", 2), ("c", 3)])
od28.move_to_end("c", last=False)
check92("OrderedDict move_to_end first", list(od28.keys()), ["c", "a", "b"])

# OrderedDict popitem LIFO
od29 = OrderedDict([("a", 1), ("b", 2), ("c", 3)])
v29 = od29.popitem()
check92("OrderedDict popitem LIFO", v29, ("c", 3))

# OrderedDict popitem FIFO
od30 = OrderedDict([("a", 1), ("b", 2), ("c", 3)])
v30 = od30.popitem(last=False)
check92("OrderedDict popitem FIFO", v30, ("a", 1))

# dict copy
d31 = {"a": 1, "b": [2, 3]}
d31c = d31.copy()
check92("dict copy equals original", d31c, d31)
d31c["a"] = 99
check92("dict copy is shallow - scalar changed", d31["a"], 1)

# dict clear
d32 = {"a": 1, "b": 2}
d32.clear()
check92("dict clear empties dict", d32, {})

# dict equality
check92("dict equality ignores order", {"a": 1, "b": 2}, {"b": 2, "a": 1})
check92("dict inequality different values", {"a": 1} == {"a": 2}, False)

# dict unpacking
d33a = {"a": 1, "b": 2}
d33b = {"c": 3, "d": 4}
d33m = {**d33a, **d33b}
check92("dict unpacking merge", d33m, {"a": 1, "b": 2, "c": 3, "d": 4})

# dict unpacking override
d34a = {"a": 1, "b": 2}
d34b = {"b": 99, "c": 3}
d34m = {**d34a, **d34b}
check92("dict unpacking override", d34m, {"a": 1, "b": 99, "c": 3})

# dict subclass
class MyDict92(dict):
    def doubled_values(self):
        return {k: v * 2 for k, v in self.items()}

md35 = MyDict92(a=1, b=2)
check92("dict subclass basic access", md35["a"], 1)
check92("dict subclass custom method", md35.doubled_values(), {"a": 2, "b": 4})
check92("dict subclass isinstance", isinstance(md35, dict), True)

# dict with tuple keys
d36 = {(1, 2): "a", (3, 4): "b"}
check92("dict with tuple keys", d36[(1, 2)], "a")

# dict with mixed key types
d37 = {1: "int", "1": "str", (1,): "tuple"}
check92("dict mixed key types len", len(d37), 3)
check92("dict mixed key int lookup", d37[1], "int")
check92("dict mixed key str lookup", d37["1"], "str")

# dict in operator
d38 = {"a": 1, "b": 2}
check92("dict in operator existing", "a" in d38, True)
check92("dict not in operator missing", "c" not in d38, True)

# dict len
check92("dict len empty", len({}), 0)
check92("dict len nonempty", len({"a": 1, "b": 2, "c": 3}), 3)

print(f"Tests: {total92} | Passed: {passed92} | Failed: {total92 - passed92}")
