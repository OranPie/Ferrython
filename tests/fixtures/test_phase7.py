# Phase 7: Mutable sets, set operations, more patterns
# Run: ferrython tests/fixtures/test_phase7.py

passed = 0
failed = 0

def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"FAIL: {name}")

# === Mutable set operations ===
s = set()
s.add(1)
s.add(2)
s.add(3)
test("set_add", s == {1, 2, 3})

s.add(2)  # duplicate
test("set_add_dup", len(s) == 3)

s.discard(2)
test("set_discard", s == {1, 3})

s.discard(99)  # no error
test("set_discard_missing", s == {1, 3})

s.remove(3)
test("set_remove", s == {1})

try:
    s.remove(99)
    test("set_remove_missing_raises", False)
except KeyError:
    test("set_remove_missing_raises", True)

s.add(10)
s.add(20)
s.add(30)
val = s.pop()
test("set_pop", val in {1, 10, 20, 30} and len(s) == 3)

s.clear()
test("set_clear", len(s) == 0 and s == set())

# set.update
s = {1, 2}
s.update([3, 4, 5])
test("set_update", s == {1, 2, 3, 4, 5})

# set.copy
s = {1, 2, 3}
s2 = s.copy()
s2.add(4)
test("set_copy", s == {1, 2, 3} and s2 == {1, 2, 3, 4})

# set.union
test("set_union", {1, 2}.union({2, 3}) == {1, 2, 3})

# set.intersection
test("set_intersection", {1, 2, 3}.intersection({2, 3, 4}) == {2, 3})

# set.difference
test("set_difference", {1, 2, 3}.difference({2, 3, 4}) == {1})

# set.symmetric_difference
test("set_symmetric_diff", {1, 2, 3}.symmetric_difference({2, 3, 4}) == {1, 4})

# set.issubset
test("set_issubset_true", {1, 2}.issubset({1, 2, 3}))
test("set_issubset_false", not {1, 4}.issubset({1, 2, 3}))

# set.issuperset
test("set_issuperset_true", {1, 2, 3}.issuperset({1, 2}))
test("set_issuperset_false", not {1, 2}.issuperset({1, 2, 3}))

# set.isdisjoint
test("set_isdisjoint_true", {1, 2}.isdisjoint({3, 4}))
test("set_isdisjoint_false", not {1, 2}.isdisjoint({2, 3}))

# === Set comprehension with mutation ===
evens = {x for x in range(10) if x % 2 == 0}
test("set_comprehension", evens == {0, 2, 4, 6, 8})
evens.add(10)
test("set_comp_then_add", len(evens) == 6)

# === Set as de-duplicate ===
nums = [1, 2, 2, 3, 3, 3, 4, 4, 4, 4]
unique = sorted(list(set(nums)))
test("set_dedup", unique == [1, 2, 3, 4])

# === Set iteration ===
s = {10, 20, 30}
total = 0
for x in s:
    total += x
test("set_iteration", total == 60)

# === Set in check ===
s = {1, 2, 3}
test("set_in", 2 in s and 4 not in s)

# === Set len ===
test("set_len", len({1, 2, 3}) == 3 and len(set()) == 0)

# === Abstract class pattern ===
class Shape:
    def area(self):
        raise NotImplementedError
    def __str__(self):
        return f"{type(self).__name__}(area={self.area()})"

class Circle(Shape):
    def __init__(self, r):
        self.r = r
    def area(self):
        return 3.14159 * self.r * self.r

class Rect(Shape):
    def __init__(self, w, h):
        self.w = w
        self.h = h
    def area(self):
        return self.w * self.h

shapes = [Circle(5), Rect(3, 4)]
test("shape_circle_area", abs(shapes[0].area() - 78.53975) < 0.001)
test("shape_rect_area", shapes[1].area() == 12)
test("shape_str", "Circle" in str(shapes[0]))

# === Builder pattern ===
class HTML:
    def __init__(self):
        self.parts = []
    def tag(self, name, content):
        self.parts.append(f"<{name}>{content}</{name}>")
        return self
    def build(self):
        return "".join(self.parts)

html = HTML().tag("h1", "Title").tag("p", "Hello").build()
test("builder_html", html == "<h1>Title</h1><p>Hello</p>")

# === Graph algorithms ===
# DFS
def dfs(graph, start):
    visited = []
    stack = [start]
    seen = {start}
    while stack:
        node = stack[-1]
        stack = stack[:-1]
        visited.append(node)
        for neighbor in reversed(graph.get(node, [])):
            if neighbor not in seen:
                seen.add(neighbor)
                stack.append(neighbor)
    return visited

graph = {"A": ["B", "C"], "B": ["D"], "C": ["D", "E"], "D": [], "E": []}
test("dfs", dfs(graph, "A") == ["A", "B", "D", "C", "E"])

# Topological sort (Kahn's algorithm)
def topo_sort(graph):
    in_degree = {}
    for node in graph:
        if node not in in_degree:
            in_degree[node] = 0
        for dep in graph[node]:
            if dep not in in_degree:
                in_degree[dep] = 0
            in_degree[dep] = in_degree[dep] + 1
    
    queue = []
    for node in in_degree:
        if in_degree[node] == 0:
            queue.append(node)
    
    result = []
    while queue:
        node = queue[0]
        queue = queue[1:]
        result.append(node)
        for dep in graph.get(node, []):
            in_degree[dep] = in_degree[dep] - 1
            if in_degree[dep] == 0:
                queue.append(dep)
    return result

deps = {"A": ["B", "C"], "B": ["D"], "C": ["D"], "D": []}
order = topo_sort(deps)
test("topo_sort_valid", order.index("A") < order.index("B"))
test("topo_sort_valid2", order.index("A") < order.index("C"))
test("topo_sort_valid3", order.index("B") < order.index("D"))

# === Priority Queue (heap-like) ===
class MinHeap:
    def __init__(self):
        self.data = []
    
    def push(self, val):
        self.data.append(val)
        self._sift_up(len(self.data) - 1)
    
    def pop(self):
        if not self.data:
            raise IndexError("pop from empty heap")
        self._swap(0, len(self.data) - 1)
        val = self.data[-1]
        self.data = self.data[:-1]
        if self.data:
            self._sift_down(0)
        return val
    
    def _swap(self, i, j):
        tmp = self.data[i]
        self.data[i] = self.data[j]
        self.data[j] = tmp
    
    def _sift_up(self, i):
        while i > 0:
            parent = (i - 1) // 2
            if self.data[i] < self.data[parent]:
                self._swap(i, parent)
                i = parent
            else:
                break
    
    def _sift_down(self, i):
        n = len(self.data)
        while True:
            left = 2 * i + 1
            right = 2 * i + 2
            smallest = i
            if left < n and self.data[left] < self.data[smallest]:
                smallest = left
            if right < n and self.data[right] < self.data[smallest]:
                smallest = right
            if smallest != i:
                self._swap(i, smallest)
                i = smallest
            else:
                break
    
    def __len__(self):
        return len(self.data)

heap = MinHeap()
heap.push(5)
heap.push(3)
heap.push(7)
heap.push(1)
heap.push(4)
result = []
while len(heap) > 0:
    result.append(heap.pop())
test("min_heap", result == [1, 3, 4, 5, 7])

# === LRU Cache (manual implementation) ===
class LRUCache:
    def __init__(self, capacity):
        self.capacity = capacity
        self.cache = {}
        self.order = []
    
    def get(self, key):
        if key not in self.cache:
            return -1
        self.order.remove(key)
        self.order.append(key)
        return self.cache[key]
    
    def put(self, key, value):
        if key in self.cache:
            self.order.remove(key)
        elif len(self.cache) >= self.capacity:
            oldest = self.order[0]
            self.order = self.order[1:]
            del self.cache[oldest]
        self.cache[key] = value
        self.order.append(key)

lru = LRUCache(2)
lru.put("a", 1)
lru.put("b", 2)
test("lru_get_a", lru.get("a") == 1)
lru.put("c", 3)  # evicts "b"
test("lru_evict_b", lru.get("b") == -1)
test("lru_get_c", lru.get("c") == 3)

# === Matrix transpose and multiply ===
def mat_mul(a, b):
    rows_a = len(a)
    cols_a = len(a[0])
    cols_b = len(b[0])
    result = []
    for i in range(rows_a):
        row = []
        for j in range(cols_b):
            s = 0
            for k in range(cols_a):
                s += a[i][k] * b[k][j]
            row.append(s)
        result.append(row)
    return result

a = [[1, 2], [3, 4]]
b = [[5, 6], [7, 8]]
c = mat_mul(a, b)
test("mat_mul", c == [[19, 22], [43, 50]])

# === String algorithms ===
def is_palindrome(s):
    return s == s[::-1]

test("palindrome_yes", is_palindrome("racecar"))
test("palindrome_no", not is_palindrome("hello"))
test("palindrome_empty", is_palindrome(""))

def caesar_cipher(text, shift):
    result = ""
    for c in text:
        if c.isalpha():
            base = ord("a") if c.islower() else ord("A")
            result += chr((ord(c) - base + shift) % 26 + base)
        else:
            result += c
    return result

test("caesar", caesar_cipher("Hello", 3) == "Khoor")
test("caesar_wrap", caesar_cipher("xyz", 3) == "abc")

# === Functional patterns ===
# reduce
def reduce(func, iterable, initial=None):
    it = iter(iterable)
    if initial is None:
        acc = next(it)
    else:
        acc = initial
    for item in it:
        acc = func(acc, item)
    return acc

test("reduce_sum", reduce(lambda a, b: a + b, [1, 2, 3, 4]) == 10)
test("reduce_product", reduce(lambda a, b: a * b, [1, 2, 3, 4]) == 24)
test("reduce_initial", reduce(lambda a, b: a + b, [1, 2, 3], 10) == 16)

# compose
def compose(f, g):
    def h(x):
        return f(g(x))
    return h

double = lambda x: x * 2
inc = lambda x: x + 1
double_then_inc = compose(inc, double)
test("compose", double_then_inc(5) == 11)

# === Generator pipelines ===
def integers():
    n = 1
    while n <= 20:
        yield n
        n += 1

def evens(gen):
    for x in gen:
        if x % 2 == 0:
            yield x

def squares(gen):
    for x in gen:
        yield x * x

pipeline = list(squares(evens(integers())))
test("gen_pipeline", pipeline == [4, 16, 36, 64, 100, 144, 196, 256, 324, 400])

# === Decorator with state (using closure) ===
def call_counter(func):
    count = [0]
    def wrapper(*args, **kwargs):
        count[0] += 1
        return func(*args, **kwargs)
    return wrapper, count

def greet(name):
    return f"Hello, {name}"

counted_greet, counter = call_counter(greet)
counted_greet("Alice")
counted_greet("Bob")
counted_greet("Charlie")
test("call_counter", counter[0] == 3)

# === Multiple inheritance with cooperative super ===
class Base:
    def __init__(self):
        self.log = []
    def do(self):
        self.log.append("Base")

class A(Base):
    def do(self):
        self.log.append("A")
        super().do()

class B(Base):
    def do(self):
        self.log.append("B")
        super().do()

class C(A, B):
    def do(self):
        self.log.append("C")
        super().do()

c = C()
c.do()
test("cooperative_super", c.log == ["C", "A", "B", "Base"])

# === Exception handling patterns ===
def safe_div(a, b):
    try:
        return a / b
    except ZeroDivisionError:
        return None

test("safe_div_ok", safe_div(10, 2) == 5.0)
test("safe_div_zero", safe_div(10, 0) is None)

# Nested try
def nested_try():
    results = []
    try:
        try:
            x = 1 / 0
        except ZeroDivisionError:
            results.append("caught_inner")
            raise ValueError("from inner")
    except ValueError as e:
        results.append("caught_outer")
    return results

test("nested_try", nested_try() == ["caught_inner", "caught_outer"])

# Finally always runs
def with_finally():
    results = []
    try:
        results.append("try")
        return results
    finally:
        results.append("finally")

r = with_finally()
test("finally_always", r == ["try", "finally"])

# === dict operations ===
d = {"a": 1, "b": 2}
d["c"] = 3
test("dict_assign", d == {"a": 1, "b": 2, "c": 3})

keys = sorted(list(d.keys()))
test("dict_keys_sorted", keys == ["a", "b", "c"])

vals = sorted(list(d.values()))
test("dict_values_sorted", vals == [1, 2, 3])

items = sorted(list(d.items()))
test("dict_items_sorted", items == [("a", 1), ("b", 2), ("c", 3)])

test("dict_get_default", d.get("z", 0) == 0)
test("dict_get_exists", d.get("a", 0) == 1)

d.update({"b": 20, "d": 4})
test("dict_update", d["b"] == 20 and d["d"] == 4)

popped = d.pop("d")
test("dict_pop", popped == 4 and "d" not in d)

# === Nested closures ===
def make_adder(x):
    def add(y):
        def add_more(z):
            return x + y + z
        return add_more
    return add

test("nested_closure", make_adder(1)(2)(3) == 6)

# === Walrus in while ===
data = [1, 2, 3, 0, 4, 5]
results = []
i = 0
while i < len(data) and (val := data[i]) != 0:
    results.append(val)
    i += 1
test("walrus_while", results == [1, 2, 3])

# === Complex string operations ===
test("str_replace", "hello world".replace("world", "python") == "hello python")
test("str_split", "a,b,c".split(",") == ["a", "b", "c"])
test("str_join", "-".join(["a", "b", "c"]) == "a-b-c")
test("str_strip", "  hello  ".strip() == "hello")
test("str_upper_lower", "Hello".upper() == "HELLO" and "Hello".lower() == "hello")
test("str_startswith", "hello".startswith("hel"))
test("str_endswith", "hello".endswith("llo"))
test("str_find", "hello world".find("world") == 6)
test("str_count", "banana".count("a") == 3)
test("str_zfill", "42".zfill(5) == "00042")
test("str_center", "hi".center(6) == "  hi  ")
test("str_ljust_rjust", "hi".ljust(5) == "hi   " and "hi".rjust(5) == "   hi")

# === Type checking patterns ===
test("isinstance_int", isinstance(42, int))
test("isinstance_str", isinstance("hello", str))
test("isinstance_list", isinstance([1, 2], list))
test("isinstance_dict", isinstance({"a": 1}, dict))
test("isinstance_bool_int", isinstance(True, int))

# === Summary ===
print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL TESTS PASSED!")
else:
    print(f"FAILURES: {failed}")
