# test_cpython_compat99.py - copy module and advanced data structures
import copy
import collections

passed99 = 0
total99 = 0

def check99(desc, got, expected):
    global passed99, total99
    total99 += 1
    if got == expected:
        passed99 += 1
    else:
        print(f"FAIL: {desc}: got {got!r}, expected {expected!r}")

# --- copy.copy of basic types ---
lst99 = [1, 2, 3]
lst99_c = copy.copy(lst99)
check99("copy list equal", lst99_c, [1, 2, 3])
check99("copy list is different obj", lst99_c is not lst99, True)

dict99 = {"a": 1, "b": 2}
dict99_c = copy.copy(dict99)
check99("copy dict equal", dict99_c, {"a": 1, "b": 2})
check99("copy dict is different obj", dict99_c is not dict99, True)

set99 = {1, 2, 3}
set99_c = copy.copy(set99)
check99("copy set equal", set99_c, {1, 2, 3})
check99("copy set is different obj", set99_c is not set99, True)

# --- copy.copy independence ---
lst99_c.append(4)
check99("copy list independence original", lst99, [1, 2, 3])
check99("copy list independence copy", lst99_c, [1, 2, 3, 4])

dict99_c["c"] = 3
check99("copy dict independence original", dict99, {"a": 1, "b": 2})
check99("copy dict independence copy", dict99_c, {"a": 1, "b": 2, "c": 3})

# --- copy.copy is shallow ---
nested99 = [[1, 2], [3, 4]]
nested99_c = copy.copy(nested99)
check99("shallow copy equal", nested99_c, [[1, 2], [3, 4]])
nested99_c[0].append(99)
check99("shallow copy shares inner", nested99[0], [1, 2, 99])

# --- copy.deepcopy ---
nested99b = [[1, 2], [3, 4]]
nested99b_d = copy.deepcopy(nested99b)
check99("deepcopy equal", nested99b_d, [[1, 2], [3, 4]])
nested99b_d[0].append(99)
check99("deepcopy inner independence original", nested99b[0], [1, 2])
check99("deepcopy inner independence copy", nested99b_d[0], [1, 2, 99])

# --- deepcopy of nested dicts ---
d99_nested = {"a": {"x": 1}, "b": [1, 2]}
d99_deep = copy.deepcopy(d99_nested)
check99("deepcopy nested dict equal", d99_deep, {"a": {"x": 1}, "b": [1, 2]})
d99_deep["a"]["x"] = 99
d99_deep["b"].append(3)
check99("deepcopy nested dict independence inner dict", d99_nested["a"]["x"], 1)
check99("deepcopy nested dict independence inner list", d99_nested["b"], [1, 2])

# --- deepcopy of tuples with mutable contents ---
t99 = ([1, 2], [3, 4])
t99_d = copy.deepcopy(t99)
check99("deepcopy tuple equal", t99_d, ([1, 2], [3, 4]))
t99_d[0].append(99)
check99("deepcopy tuple independence", t99[0], [1, 2])

# --- collections.ChainMap ---
base99 = {"a": 1, "b": 2}
override99 = {"b": 3, "c": 4}
cm99 = collections.ChainMap(override99, base99)

check99("ChainMap lookup override", cm99["b"], 3)
check99("ChainMap lookup base", cm99["a"], 1)
check99("ChainMap lookup new", cm99["c"], 4)
check99("ChainMap len", len(cm99), 3)
check99("ChainMap contains", "a" in cm99, True)
check99("ChainMap not contains", "z" in cm99, False)
check99("ChainMap keys sorted", sorted(cm99.keys()), ["a", "b", "c"])

# --- ChainMap new_child / parents ---
child99 = cm99.new_child({"d": 5})
check99("ChainMap new_child lookup", child99["d"], 5)
check99("ChainMap new_child inherits", child99["a"], 1)
check99("ChainMap parents type", isinstance(cm99.parents, collections.ChainMap), True)

# --- frozenset operations ---
fs99_a = frozenset([1, 2, 3, 4])
fs99_b = frozenset([3, 4, 5, 6])

check99("frozenset union", fs99_a | fs99_b, frozenset([1, 2, 3, 4, 5, 6]))
check99("frozenset intersection", fs99_a & fs99_b, frozenset([3, 4]))
check99("frozenset difference", fs99_a - fs99_b, frozenset([1, 2]))
check99("frozenset symmetric_difference", fs99_a ^ fs99_b, frozenset([1, 2, 5, 6]))
check99("frozenset issubset", frozenset([3, 4]).issubset(fs99_a), True)
check99("frozenset issuperset", fs99_a.issuperset(frozenset([1, 2])), True)
check99("frozenset isdisjoint", fs99_a.isdisjoint(frozenset([7, 8])), True)

# --- frozenset is hashable ---
check99("frozenset is hashable", isinstance(hash(fs99_a), int), True)

# --- frozenset as dict key ---
fsd99 = {frozenset([1, 2]): "ab", frozenset([3, 4]): "cd"}
check99("frozenset as dict key", fsd99[frozenset([1, 2])], "ab")
check99("frozenset as dict key 2", fsd99[frozenset([3, 4])], "cd")

# --- frozenset from various sources ---
check99("frozenset from range", frozenset(range(5)), frozenset([0, 1, 2, 3, 4]))
check99("frozenset from str", frozenset("abc"), frozenset(["a", "b", "c"]))
check99("frozenset empty", frozenset(), frozenset())
check99("frozenset len", len(frozenset([1, 2, 3])), 3)
check99("frozenset contains", 2 in frozenset([1, 2, 3]), True)
check99("frozenset not contains", 5 in frozenset([1, 2, 3]), False)

# --- frozenset and set interop ---
check99("frozenset == set", frozenset([1, 2, 3]) == {1, 2, 3}, True)
check99("frozenset union with set", frozenset([1, 2]) | {3, 4}, frozenset([1, 2, 3, 4]))

print(f"Tests: {total99} | Passed: {passed99} | Failed: {total99 - passed99}")
