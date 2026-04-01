# Phase 37: Real-world patterns - design patterns, data processing, algorithms
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── Singleton pattern ──
class Singleton:
    _instance = None
    
    def __new__(cls):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance

s1 = Singleton()
s2 = Singleton()
test("singleton same instance", s1 is s2)

# ── Observer pattern ──
class EventEmitter:
    def __init__(self):
        self._handlers = {}
    
    def on(self, event, handler):
        if event not in self._handlers:
            self._handlers[event] = []
        self._handlers[event].append(handler)
    
    def emit(self, event, *args):
        results = []
        if event in self._handlers:
            for handler in self._handlers[event]:
                results.append(handler(*args))
        return results

emitter = EventEmitter()
log = []
emitter.on("data", lambda x: log.append(x))
emitter.on("data", lambda x: x * 2)

results = emitter.emit("data", 42)
test("observer log", log == [42])
test("observer results", results == [None, 84])

# ── Chain of responsibility ──
class Handler:
    def __init__(self):
        self.next_handler = None
    
    def set_next(self, handler):
        self.next_handler = handler
        return handler
    
    def handle(self, request):
        if self.next_handler:
            return self.next_handler.handle(request)
        return None

class PositiveHandler(Handler):
    def handle(self, request):
        if request > 0:
            return "positive"
        return super().handle(request)

class ZeroHandler(Handler):
    def handle(self, request):
        if request == 0:
            return "zero"
        return super().handle(request)

class NegativeHandler(Handler):
    def handle(self, request):
        if request < 0:
            return "negative"
        return super().handle(request)

h1 = PositiveHandler()
h2 = ZeroHandler()
h3 = NegativeHandler()
h1.set_next(h2).set_next(h3)

test("chain positive", h1.handle(5) == "positive")
test("chain zero", h1.handle(0) == "zero")
test("chain negative", h1.handle(-3) == "negative")

# ── Data pipeline ──
data = [
    {"name": "Alice", "age": 30, "salary": 70000},
    {"name": "Bob", "age": 25, "salary": 55000},
    {"name": "Charlie", "age": 35, "salary": 90000},
    {"name": "Diana", "age": 28, "salary": 65000},
    {"name": "Eve", "age": 32, "salary": 80000},
]

# Filter, map, reduce pipeline
seniors = [p for p in data if p["age"] >= 30]
test("filter seniors", len(seniors) == 3)

names = [p["name"] for p in seniors]
test("map names", names == ["Alice", "Charlie", "Eve"])

from functools import reduce
total_salary = reduce(lambda acc, p: acc + p["salary"], data, 0)
test("reduce total", total_salary == 360000)

avg_salary = total_salary / len(data)
test("average salary", avg_salary == 72000.0)

above_avg = [p["name"] for p in data if p["salary"] > avg_salary]
test("above average", above_avg == ["Charlie", "Eve"])

# Sort by salary descending
sorted_data = sorted(data, key=lambda p: p["salary"], reverse=True)
test("sorted by salary", sorted_data[0]["name"] == "Charlie")
test("sorted by salary last", sorted_data[-1]["name"] == "Bob")

# Group by age bracket
def age_bracket(age):
    if age < 30:
        return "20s"
    elif age < 40:
        return "30s"
    return "40+"

groups = {}
for p in data:
    bracket = age_bracket(p["age"])
    if bracket not in groups:
        groups[bracket] = []
    groups[bracket].append(p["name"])

test("group 20s", sorted(groups["20s"]) == ["Bob", "Diana"])
test("group 30s", sorted(groups["30s"]) == ["Alice", "Charlie", "Eve"])

# ── Binary search ──
def binary_search(arr, target):
    lo, hi = 0, len(arr) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        if arr[mid] == target:
            return mid
        elif arr[mid] < target:
            lo = mid + 1
        else:
            hi = mid - 1
    return -1

arr = [1, 3, 5, 7, 9, 11, 13, 15]
test("binary search found", binary_search(arr, 7) == 3)
test("binary search not found", binary_search(arr, 6) == -1)
test("binary search first", binary_search(arr, 1) == 0)
test("binary search last", binary_search(arr, 15) == 7)

# ── Quicksort ──
def quicksort(arr):
    if len(arr) <= 1:
        return arr
    pivot = arr[len(arr) // 2]
    left = [x for x in arr if x < pivot]
    middle = [x for x in arr if x == pivot]
    right = [x for x in arr if x > pivot]
    return quicksort(left) + middle + quicksort(right)

test("quicksort", quicksort([3, 6, 8, 10, 1, 2, 1]) == [1, 1, 2, 3, 6, 8, 10])
test("quicksort empty", quicksort([]) == [])
test("quicksort single", quicksort([42]) == [42])

# ── Memoization ──
call_count = 0
def memoize(func):
    cache = {}
    def wrapper(n):
        if n not in cache:
            cache[n] = func(n)
        return cache[n]
    return wrapper

@memoize
def fib(n):
    global call_count
    call_count += 1
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

result = fib(30)
test("memoized fib(30)", result == 832040)
test("memoized efficiency", call_count == 31)  # Only 31 unique calls

# ── Linked list ──
class Node:
    def __init__(self, val, next=None):
        self.val = val
        self.next = next
    
    def __repr__(self):
        vals = []
        curr = self
        while curr:
            vals.append(str(curr.val))
            curr = curr.next
        return " -> ".join(vals)

def from_list(lst):
    head = None
    for val in reversed(lst):
        head = Node(val, head)
    return head

def to_list(node):
    result = []
    while node:
        result.append(node.val)
        node = node.next
    return result

def reverse_list(node):
    prev = None
    curr = node
    while curr:
        next_node = curr.next
        curr.next = prev
        prev = curr
        curr = next_node
    return prev

ll = from_list([1, 2, 3, 4, 5])
test("linked list to_list", to_list(ll) == [1, 2, 3, 4, 5])
test("linked list repr", repr(ll) == "1 -> 2 -> 3 -> 4 -> 5")

rev = reverse_list(ll)
test("linked list reverse", to_list(rev) == [5, 4, 3, 2, 1])

# ── Stack using list ──
class Stack:
    def __init__(self):
        self._items = []
    
    def push(self, item):
        self._items.append(item)
        return self
    
    def pop(self):
        if not self._items:
            raise IndexError("pop from empty stack")
        return self._items.pop()
    
    def peek(self):
        if not self._items:
            raise IndexError("peek from empty stack")
        return self._items[-1]
    
    def __len__(self):
        return len(self._items)
    
    def __bool__(self):
        return len(self._items) > 0

# Balanced parentheses
def is_balanced(s):
    stack = Stack()
    matching = {')': '(', ']': '[', '}': '{'}
    for ch in s:
        if ch in '([{':
            stack.push(ch)
        elif ch in ')]}':
            if not stack or stack.pop() != matching[ch]:
                return False
    return not stack

test("balanced parens", is_balanced("({[]})"))
test("balanced empty", is_balanced(""))
test("unbalanced", not is_balanced("({[})"))
test("unbalanced open", not is_balanced("(("))

# ── Dictionary defaultdict pattern ──
from collections import defaultdict

word_count = defaultdict(int)
words = "the quick brown fox jumps over the lazy dog the fox".split()
for word in words:
    word_count[word] += 1

test("word count 'the'", word_count["the"] == 3)
test("word count 'fox'", word_count["fox"] == 2)
test("word count 'quick'", word_count["quick"] == 1)

# ── collections.Counter ──
from collections import Counter

counter = Counter(words)
test("counter 'the'", counter["the"] == 3)
most_common = counter.most_common(2)
test("most common", most_common[0][0] == "the" and most_common[0][1] == 3)

# ── String algorithms ──
def is_palindrome(s):
    s = s.lower().replace(" ", "")
    return s == s[::-1]

test("palindrome", is_palindrome("racecar"))
test("palindrome spaces", is_palindrome("race car"))  # Note: with spaces removed
test("not palindrome", not is_palindrome("hello"))

# ── Matrix operations ──
def matrix_mult(a, b):
    rows_a, cols_a = len(a), len(a[0])
    rows_b, cols_b = len(b), len(b[0])
    result = [[0] * cols_b for _ in range(rows_a)]
    for i in range(rows_a):
        for j in range(cols_b):
            for k in range(cols_a):
                result[i][j] += a[i][k] * b[k][j]
    return result

a = [[1, 2], [3, 4]]
b = [[5, 6], [7, 8]]
c = matrix_mult(a, b)
test("matrix mult", c == [[19, 22], [43, 50]])

# Identity matrix
identity = [[1, 0], [0, 1]]
test("matrix identity", matrix_mult(a, identity) == a)

# ── Recursive data structures ──
def flatten(lst):
    result = []
    for item in lst:
        if isinstance(item, list):
            result.extend(flatten(item))
        else:
            result.append(item)
    return result

test("flatten nested", flatten([1, [2, [3, 4], 5], [6, 7]]) == [1, 2, 3, 4, 5, 6, 7])
test("flatten flat", flatten([1, 2, 3]) == [1, 2, 3])
test("flatten empty", flatten([]) == [])

# ── Complex f-string expressions ──
items = [("apple", 3), ("banana", 5), ("cherry", 2)]
formatted = [f"{name}: {count}" for name, count in items]
test("f-string in list comp", formatted == ["apple: 3", "banana: 5", "cherry: 2"])

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
assert failed == 0, f"{failed} tests failed!"
print("ALL PHASE 37 TESTS PASSED")
