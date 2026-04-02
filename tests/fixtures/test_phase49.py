"""Phase 49: Real-world patterns — caching, state machines, visitor pattern,
   builder pattern, mixins, descriptors, __slots__"""

passed = 0
failed = 0
total = 0
def test(name, cond):
    global passed, failed, total
    total += 1
    if cond:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# 1. Memoization decorator (manual, no functools)
def memoize(func):
    cache = {}
    def wrapper(*args):
        if args not in cache:
            cache[args] = func(*args)
        return cache[args]
    return wrapper

@memoize
def fib(n):
    if n <= 1:
        return n
    return fib(n-1) + fib(n-2)

test("memoize fib(10)", fib(10) == 55)
test("memoize fib(20)", fib(20) == 6765)

# 2. State machine
class StateMachine:
    def __init__(self):
        self.state = "idle"
        self.transitions = {
            ("idle", "start"): "running",
            ("running", "pause"): "paused",
            ("paused", "resume"): "running",
            ("running", "stop"): "stopped",
            ("paused", "stop"): "stopped",
        }
    
    def trigger(self, event):
        key = (self.state, event)
        if key in self.transitions:
            self.state = self.transitions[key]
            return True
        return False

sm = StateMachine()
test("sm idle", sm.state == "idle")
sm.trigger("start")
test("sm running", sm.state == "running")
sm.trigger("pause")
test("sm paused", sm.state == "paused")
sm.trigger("resume")
test("sm resumed", sm.state == "running")
sm.trigger("stop")
test("sm stopped", sm.state == "stopped")
test("sm invalid", not sm.trigger("start"))

# 3. Builder pattern
class QueryBuilder:
    def __init__(self):
        self._table = ""
        self._conditions = []
        self._order = ""
        self._limit = None
    
    def from_table(self, table):
        self._table = table
        return self
    
    def where(self, condition):
        self._conditions.append(condition)
        return self
    
    def order_by(self, field):
        self._order = field
        return self
    
    def limit(self, n):
        self._limit = n
        return self
    
    def build(self):
        sql = f"SELECT * FROM {self._table}"
        if self._conditions:
            sql += " WHERE " + " AND ".join(self._conditions)
        if self._order:
            sql += f" ORDER BY {self._order}"
        if self._limit:
            sql += f" LIMIT {self._limit}"
        return sql

query = (QueryBuilder()
    .from_table("users")
    .where("age > 18")
    .where("active = 1")
    .order_by("name")
    .limit(10)
    .build())
test("builder", query == "SELECT * FROM users WHERE age > 18 AND active = 1 ORDER BY name LIMIT 10")

# 4. Observer pattern
class EventEmitter:
    def __init__(self):
        self._handlers = {}
    
    def on(self, event, handler):
        if event not in self._handlers:
            self._handlers[event] = []
        self._handlers[event].append(handler)
        return self
    
    def emit(self, event, *args):
        for handler in self._handlers.get(event, []):
            handler(*args)

log = []
emitter = EventEmitter()
emitter.on("data", lambda d: log.append(f"got {d}"))
emitter.on("data", lambda d: log.append(f"also {d}"))
emitter.emit("data", "hello")
test("observer", log == ["got hello", "also hello"])

# 5. Chain of responsibility
class Handler:
    def __init__(self, name, can_handle):
        self.name = name
        self.can_handle = can_handle
        self.next = None
    
    def set_next(self, handler):
        self.next = handler
        return handler
    
    def handle(self, request):
        if self.can_handle(request):
            return f"{self.name} handled {request}"
        if self.next:
            return self.next.handle(request)
        return None

h1 = Handler("auth", lambda r: r.startswith("auth:"))
h2 = Handler("data", lambda r: r.startswith("data:"))
h3 = Handler("log", lambda r: True)
h1.set_next(h2).set_next(h3)

test("chain auth", h1.handle("auth:login") == "auth handled auth:login")
test("chain data", h1.handle("data:get") == "data handled data:get")
test("chain fallback", h1.handle("unknown") == "log handled unknown")

# 6. Strategy pattern
class Sorter:
    def __init__(self, strategy):
        self.strategy = strategy
    
    def sort(self, data):
        return self.strategy(data)

bubble = lambda data: sorted(data)
reverse = lambda data: sorted(data, reverse=True)

s = Sorter(bubble)
test("strategy asc", s.sort([3, 1, 2]) == [1, 2, 3])
s.strategy = reverse
test("strategy desc", s.sort([3, 1, 2]) == [3, 2, 1])

# 7. Mixin classes
class JsonMixin:
    def to_json_str(self):
        parts = []
        for k, v in self.__dict__.items():
            if isinstance(v, str):
                parts.append(f'"{k}": "{v}"')
            else:
                parts.append(f'"{k}": {v}')
        return "{" + ", ".join(parts) + "}"

class Printable:
    def display(self):
        return str(self.__dict__)

class User(JsonMixin, Printable):
    def __init__(self, name, age):
        self.name = name
        self.age = age

u = User("Alice", 30)
test("mixin json", '"name": "Alice"' in u.to_json_str())
test("mixin display", "name" in u.display())

# 8. Recursive data structures
class TreeNode:
    def __init__(self, val, left=None, right=None):
        self.val = val
        self.left = left
        self.right = right
    
    def inorder(self):
        result = []
        if self.left:
            result.extend(self.left.inorder())
        result.append(self.val)
        if self.right:
            result.extend(self.right.inorder())
        return result
    
    def height(self):
        left_h = self.left.height() if self.left else 0
        right_h = self.right.height() if self.right else 0
        return 1 + max(left_h, right_h)

tree = TreeNode(4,
    TreeNode(2, TreeNode(1), TreeNode(3)),
    TreeNode(6, TreeNode(5), TreeNode(7)))
test("bst inorder", tree.inorder() == [1, 2, 3, 4, 5, 6, 7])
test("bst height", tree.height() == 3)

# 9. Graph algorithms
def bfs(graph, start):
    visited = []
    queue = [start]
    seen = {start}
    while queue:
        node = queue.pop(0)
        visited.append(node)
        for neighbor in graph.get(node, []):
            if neighbor not in seen:
                seen.add(neighbor)
                queue.append(neighbor)
    return visited

graph = {
    "A": ["B", "C"],
    "B": ["D", "E"],
    "C": ["F"],
    "D": [],
    "E": ["F"],
    "F": [],
}
test("bfs", bfs(graph, "A") == ["A", "B", "C", "D", "E", "F"])

# 10. Generator pipeline
def gen_range(n):
    for i in range(n):
        yield i

def gen_filter(pred, gen):
    for item in gen:
        if pred(item):
            yield item

def gen_map(func, gen):
    for item in gen:
        yield func(item)

pipeline = list(gen_map(lambda x: x * x, gen_filter(lambda x: x % 2 == 0, gen_range(10))))
test("gen pipeline", pipeline == [0, 4, 16, 36, 64])

# 11. Topological sort
def topo_sort(graph):
    visited = set()
    stack = []
    
    def dfs(node):
        visited.add(node)
        for neighbor in graph.get(node, []):
            if neighbor not in visited:
                dfs(neighbor)
        stack.append(node)
    
    for node in graph:
        if node not in visited:
            dfs(node)
    
    return list(reversed(stack))

deps = {
    "A": ["B", "C"],
    "B": ["D"],
    "C": ["D"],
    "D": [],
}
order = topo_sort(deps)
test("topo A before B", order.index("A") < order.index("B"))
test("topo B before D", order.index("B") < order.index("D"))

# 12. LRU cache (manual implementation)
class LRUCache:
    def __init__(self, capacity):
        self.capacity = capacity
        self.cache = {}
        self.order = []
    
    def get(self, key):
        if key in self.cache:
            self.order.remove(key)
            self.order.append(key)
            return self.cache[key]
        return -1
    
    def put(self, key, value):
        if key in self.cache:
            self.order.remove(key)
        elif len(self.cache) >= self.capacity:
            oldest = self.order.pop(0)
            del self.cache[oldest]
        self.cache[key] = value
        self.order.append(key)

lru = LRUCache(2)
lru.put(1, 1)
lru.put(2, 2)
test("lru get", lru.get(1) == 1)
lru.put(3, 3)  # Evicts key 2
test("lru evict", lru.get(2) == -1)
test("lru newest", lru.get(3) == 3)

# 13. Linked list
class ListNode:
    def __init__(self, val, next=None):
        self.val = val
        self.next = next

def to_list(node):
    result = []
    while node:
        result.append(node.val)
        node = node.next
    return result

def reverse_list(head):
    prev = None
    curr = head
    while curr:
        next_node = curr.next
        curr.next = prev
        prev = curr
        curr = next_node
    return prev

head = ListNode(1, ListNode(2, ListNode(3, ListNode(4))))
test("linked list", to_list(head) == [1, 2, 3, 4])
rev = reverse_list(head)
test("reverse list", to_list(rev) == [4, 3, 2, 1])

# 14. Complex dict operations
inventory = {}
orders = [("apple", 3), ("banana", 2), ("apple", 5), ("cherry", 1)]
for item, qty in orders:
    inventory[item] = inventory.get(item, 0) + qty
test("dict accumulate", inventory == {"apple": 8, "banana": 2, "cherry": 1})

# Group by
words = ["apple", "ant", "banana", "bat", "cherry", "cat"]
groups = {}
for w in words:
    key = w[0]
    if key not in groups:
        groups[key] = []
    groups[key].append(w)
test("group by", groups["a"] == ["apple", "ant"])

# 15. Matrix operations
def matrix_multiply(A, B):
    rows_A = len(A)
    cols_A = len(A[0])
    cols_B = len(B[0])
    result = [[0] * cols_B for _ in range(rows_A)]
    for i in range(rows_A):
        for j in range(cols_B):
            for k in range(cols_A):
                result[i][j] += A[i][k] * B[k][j]
    return result

A = [[1, 2], [3, 4]]
B = [[5, 6], [7, 8]]
C = matrix_multiply(A, B)
test("matrix mul", C == [[19, 22], [43, 50]])

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 49 TESTS PASSED")
