"""Phase 9 tests: list.sort kwargs, str.join generators, advanced OOP,
format specs, comprehensions, edge cases."""

passed = 0
failed = 0
failures = []

def check(name, got, expected):
    global passed, failed
    if got == expected:
        passed += 1
    else:
        failed += 1
        failures.append(name)
        print(f"  FAIL: {name} got: {got!r} expected: {expected!r}")

# ── list.sort with key/reverse kwargs ──
nums = [3, -1, 4, -1, 5, -9]
nums.sort(key=abs)
check("sort_key_abs", nums, [-1, -1, 3, 4, 5, -9])

words = ["banana", "apple", "cherry", "date"]
words.sort(key=len)
check("sort_key_len", words, ["date", "apple", "banana", "cherry"])

nums2 = [3, 1, 4, 1, 5]
nums2.sort(reverse=True)
check("sort_reverse", nums2, [5, 4, 3, 1, 1])

# sort with both key and reverse
items = ["bb", "a", "ccc", "dddd"]
items.sort(key=len, reverse=True)
check("sort_key_reverse", items, ["dddd", "ccc", "bb", "a"])

# ── str.join with generators ──
check("join_gen", ",".join(str(x) for x in range(5)), "0,1,2,3,4")
check("join_genexpr", " ".join(str(x*x) for x in range(4)), "0 1 4 9")
check("join_map", "-".join(map(str, [1, 2, 3])), "1-2-3")

# ── str.join with list ──
check("join_list", ", ".join(["a", "b", "c"]), "a, b, c")
check("join_empty", ",".join([]), "")
check("join_single", ",".join(["only"]), "only")

# ── Advanced format specs ──
check("fmt_right_align", f"{'hello':>10}", "     hello")
check("fmt_left_align", f"{'hello':<10}", "hello     ")
check("fmt_center", f"{'hi':^10}", "    hi    ")
check("fmt_fill_align", f"{'hi':*^10}", "****hi****")
check("fmt_int_pad", f"{42:05d}", "00042")
check("fmt_float_prec", f"{3.14159:.2f}", "3.14")

# ── Dict comprehension with filter ──
d = {k: v for k, v in [("a", 1), ("b", 2), ("c", 3)] if v > 1}
check("dict_comp_filter", d, {"b": 2, "c": 3})
d2 = {x: x**2 for x in range(5) if x % 2 == 0}
check("dict_comp_even", d2, {0: 0, 2: 4, 4: 16})

# ── Set comprehension ──
s = {x*x for x in range(5)}
check("set_comp", sorted(list(s)), [0, 1, 4, 9, 16])

# ── Nested list comprehension ──
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
flat = [x for row in matrix for x in row]
check("nested_comp", flat, [1, 2, 3, 4, 5, 6, 7, 8, 9])

# ── Generator in sum ──
check("sum_genexpr", sum(x*x for x in range(5)), 30)
check("sum_genexpr2", sum(1 for x in range(10)), 10)
check("sum_gen_filter", sum(x for x in range(10) if x % 2 == 0), 20)

# ── Multiple return values ──
def divmod2(a, b):
    return a // b, a % b
q, r = divmod2(17, 5)
check("multi_return_q", q, 3)
check("multi_return_r", r, 2)

# ── Chained methods ──
check("chain_upper_split", "hello world".upper().split(), ["HELLO", "WORLD"])
check("chain_strip_split", "  a b c  ".strip().split(), ["a", "b", "c"])
check("chain_lower_replace", "HELLO".lower().replace("l", "r"), "herro")

# ── String methods ──
check("zfill", "42".zfill(5), "00042")
check("count", "hello".count("l"), 2)
check("index", "hello".index("l"), 2)
check("find_miss", "hello".find("world"), -1)
check("startswith", "hello".startswith("hel"), True)
check("endswith", "hello".endswith("llo"), True)
check("title", "hello world".title(), "Hello World")
check("swapcase", "Hello World".swapcase(), "hELLO wORLD")
check("center", "hi".center(10, "-"), "----hi----")
check("ljust", "hi".ljust(10, "."), "hi........")
check("rjust", "hi".rjust(10, "."), "........hi")

# ── Boolean operators with non-bool values ──
check("or_val", 0 or "default", "default")
check("and_val", "hello" and "world", "world")
check("or_chain", "" or 0 or None or "found", "found")
check("and_short", 0 and "never", 0)
check("or_first_true", 1 or "never", 1)

# ── Tuple comparison ──
check("tuple_lt", (1, 2) < (1, 3), True)
check("tuple_eq", (1, 2) == (1, 2), True)
check("tuple_gt", (1, 2, 3) > (1, 2), True)
check("tuple_le", (1, 2) <= (1, 2), True)
check("tuple_ne", (1, 2) != (1, 3), True)

# ── Walrus operator ──
data = [1, 2, 3, 4, 5]
results = []
idx = 0
while idx < len(data) and (val := data[idx]) < 4:
    results.append(val)
    idx += 1
check("walrus_while", results, [1, 2, 3])

# ── Nested f-string expressions ──
name = "World"
check("fstr_concat", f"{'Hello, ' + name + '!'}", "Hello, World!")
check("fstr_method", f"{'hello world'.upper()}", "HELLO WORLD")
check("fstr_ternary", f"{'yes' if True else 'no'}", "yes")

# ── Class __eq__ and __hash__ ──
class Pt:
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def __eq__(self, other):
        return self.x == other.x and self.y == other.y
    def __hash__(self):
        return hash((self.x, self.y))
    def __repr__(self):
        return f"Pt({self.x}, {self.y})"

p1 = Pt(1, 2)
p2 = Pt(1, 2)
p3 = Pt(3, 4)
check("custom_eq", p1 == p2, True)
check("custom_ne", p1 == p3, False)
check("custom_hash_eq", hash(p1) == hash(p2), True)

# ── Classmethod via instance and class ──
class Counter:
    count = 0
    @classmethod
    def increment(cls):
        cls.count += 1
        return cls.count
    @staticmethod
    def reset():
        Counter.count = 0

check("classmethod_1", Counter.increment(), 1)
check("classmethod_2", Counter.increment(), 2)
c = Counter()
check("classmethod_inst", c.increment(), 3)

# ── setdefault pattern ──
d = {}
d.setdefault("a", []).append(1)
d.setdefault("a", []).append(2)
check("setdefault_pattern", d, {"a": [1, 2]})

# ── dict.update ──
d = {"a": 1}
d.update({"b": 2, "c": 3})
check("dict_update", d, {"a": 1, "b": 2, "c": 3})

# ── dict.pop ──
d = {"a": 1, "b": 2}
val = d.pop("a")
check("dict_pop_val", val, 1)
check("dict_pop_remaining", d, {"b": 2})

# ── Exception __class__ ──
try:
    raise ValueError("test")
except ValueError as e:
    check("exc_class_name", type(e).__name__, "ValueError")

# ── Multiple inheritance MRO ──
class A:
    def method(self):
        return "A"
class B(A):
    def method(self):
        return "B"
class C(A):
    def method(self):
        return "C"
class D(B, C):
    pass
check("mi_mro", D().method(), "B")

# ── Property descriptor ──
class Circle:
    def __init__(self, radius):
        self._radius = radius
    @property
    def radius(self):
        return self._radius
    @property
    def area(self):
        return 3.14159 * self._radius ** 2

c = Circle(5)
check("property_get", c.radius, 5)
check("property_computed", round(c.area, 2), 78.54)

# ── iter() and next() ──
it = iter([10, 20, 30])
check("iter_next_1", next(it), 10)
check("iter_next_2", next(it), 20)
check("iter_next_3", next(it), 30)

# ── all() and any() ──
check("all_true", all([1, True, "yes"]), True)
check("all_false", all([1, 0, "yes"]), False)
check("any_true", any([0, False, 3]), True)
check("any_false", any([0, False, ""]), False)

# ── enumerate with start ──
result = list(enumerate(["a", "b", "c"], 1))
check("enumerate_start", result, [(1, "a"), (2, "b"), (3, "c")])

# ── zip ──
result = list(zip([1, 2, 3], ["a", "b", "c"]))
check("zip_basic", result, [(1, "a"), (2, "b"), (3, "c")])

# ── map and filter ──
check("map_result", list(map(lambda x: x*2, [1, 2, 3])), [2, 4, 6])
check("filter_result", list(filter(lambda x: x > 2, [1, 2, 3, 4])), [3, 4])

# ── sorted with key ──
check("sorted_key", sorted(["banana", "apple", "cherry"], key=len), ["apple", "banana", "cherry"])
check("sorted_reverse", sorted([3, 1, 4], reverse=True), [4, 3, 1])

# ── Summary ──
print("=" * 40)
print(f"Tests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failures:
    print(f"FAILURES: {len(failures)}")
    for f in failures:
        print(f"  - {f}")
else:
    print("ALL TESTS PASSED!")
print("=" * 40)
