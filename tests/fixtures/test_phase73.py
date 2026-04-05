# Phase 73 – __contains__, __missing__, __len__ / __bool__ protocols

# ── Task 1: __contains__ protocol ─────────────────────────────────────

class Range:
    def __init__(self, n):
        self.n = n
    def __contains__(self, item):
        return 0 <= item < self.n

r = Range(10)
assert 5 in r, "5 should be in Range(10)"
assert 0 in r, "0 should be in Range(10)"
assert 9 in r, "9 should be in Range(10)"
assert 15 not in r, "15 should not be in Range(10)"
assert -1 not in r, "-1 should not be in Range(10)"
assert 10 not in r, "10 should not be in Range(10)"

# __contains__ returning truthy/falsy (non-bool)
class Bag:
    def __init__(self, *items):
        self.items = list(items)
    def __contains__(self, item):
        for x in self.items:
            if x == item:
                return 1  # truthy non-bool
        return 0  # falsy non-bool

b = Bag(1, 2, 3)
assert 2 in b
assert 4 not in b

# __contains__ with string items
class WordSet:
    def __init__(self, words):
        self.words = words
    def __contains__(self, word):
        return word in self.words

ws = WordSet(["hello", "world"])
assert "hello" in ws
assert "missing" not in ws

print("Task 1 (__contains__): OK")

# ── Task 2: __missing__ protocol ──────────────────────────────────────

class DefaultDict(dict):
    def __missing__(self, key):
        self[key] = 0
        return 0

d = DefaultDict()
assert d["a"] == 0, "missing key should return default 0"
assert d["a"] == 0, "second access should also return 0"
assert "a" in d, "key should now exist"
d["b"] = 5
assert d["b"] == 5

# __missing__ with custom default
class CountingDict(dict):
    def __init__(self):
        super().__init__()
        self.miss_count = 0
    def __missing__(self, key):
        self.miss_count += 1
        self[key] = self.miss_count
        return self.miss_count

cd = CountingDict()
assert cd["x"] == 1
assert cd["y"] == 2
assert cd["x"] == 1  # already set, no new miss
assert cd.miss_count == 2

# __missing__ with list default
class ListDict(dict):
    def __missing__(self, key):
        self[key] = []
        return self[key]

ld = ListDict()
ld["items"].append(1)
ld["items"].append(2)
assert ld["items"] == [1, 2]

print("Task 2 (__missing__): OK")

# ── Task 3: __len__ and __bool__ ──────────────────────────────────────

# __len__ with len()
class MyList:
    def __init__(self, *items):
        self.data = list(items)
    def __len__(self):
        return len(self.data)

ml = MyList(1, 2, 3, 4, 5)
assert len(ml) == 5
assert len(MyList()) == 0

# __bool__ explicit
class AlwaysTrue:
    def __bool__(self):
        return True

class AlwaysFalse:
    def __bool__(self):
        return False

assert bool(AlwaysTrue()) == True
assert bool(AlwaysFalse()) == False

# __bool__ takes priority over __len__
class WeirdContainer:
    def __len__(self):
        return 0
    def __bool__(self):
        return True  # bool wins even though len is 0

wc = WeirdContainer()
assert bool(wc) == True, "__bool__ should take priority over __len__"

# __len__ as bool fallback (no __bool__)
class EmptyThing:
    def __len__(self):
        return 0

class NonEmptyThing:
    def __len__(self):
        return 42

assert bool(EmptyThing()) == False, "len()==0 should be falsy"
assert bool(NonEmptyThing()) == True, "len()!=0 should be truthy"

# if-statement with __bool__
af = AlwaysFalse()
if af:
    assert False, "AlwaysFalse should be falsy in if"

at = AlwaysTrue()
if not at:
    assert False, "AlwaysTrue should be truthy in if"

# if-statement with __len__
et = EmptyThing()
if et:
    assert False, "EmptyThing should be falsy in if"

net = NonEmptyThing()
passed = False
if net:
    passed = True
assert passed, "NonEmptyThing should be truthy in if"

# not operator with __bool__
assert not af
assert not (not at)

print("Task 3 (__len__/__bool__): OK")

# ── Truthiness of empty built-in collections ──────────────────────────

assert bool([]) == False
assert bool([1]) == True
assert bool(()) == False
assert bool((1,)) == True
assert bool({}) == False
assert bool({"a": 1}) == True
assert bool(set()) == False
assert bool({1}) == True
assert bool("") == False
assert bool("x") == True
assert bool(0) == False
assert bool(1) == True
assert bool(0.0) == False
assert bool(1.0) == True

# Empty collections in if-statements
if []:
    assert False, "empty list should be falsy"
if {}:
    assert False, "empty dict should be falsy"
if "":
    assert False, "empty string should be falsy"
if 0:
    assert False, "zero should be falsy"

print("Task 3 (builtin truthiness): OK")
print("All phase 73 tests passed.")
