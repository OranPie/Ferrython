"""Phase 46: Design patterns, dataclass-like, descriptors, complex generators,
   recursive data structures, functional programming patterns"""

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

# 1. Singleton pattern
class Singleton:
    _instance = None
    
    def __new__(cls, *args, **kwargs):
        if cls._instance is None:
            cls._instance = super().__new__(cls)
        return cls._instance
    
    def __init__(self, value=None):
        if value is not None:
            self.value = value

s1 = Singleton(42)
s2 = Singleton(100)
test("singleton same", s1 is s2)
test("singleton value", s1.value == 100)

# 2. Observer pattern
class EventEmitter:
    def __init__(self):
        self._handlers = {}
    
    def on(self, event, handler):
        if event not in self._handlers:
            self._handlers[event] = []
        self._handlers[event].append(handler)
    
    def emit(self, event, *args):
        for handler in self._handlers.get(event, []):
            handler(*args)

log = []
emitter = EventEmitter()
emitter.on("data", lambda x: log.append(f"received: {x}"))
emitter.on("data", lambda x: log.append(f"logged: {x}"))
emitter.emit("data", 42)
test("observer", log == ["received: 42", "logged: 42"])

# 3. Chain of responsibility
class Handler:
    def __init__(self, successor=None):
        self.successor = successor
    
    def handle(self, request):
        if self.successor:
            return self.successor.handle(request)
        return None

class ConcreteHandlerA(Handler):
    def handle(self, request):
        if request < 10:
            return f"A handled {request}"
        return super().handle(request)

class ConcreteHandlerB(Handler):
    def handle(self, request):
        if request < 20:
            return f"B handled {request}"
        return super().handle(request)

chain = ConcreteHandlerA(ConcreteHandlerB())
test("chain A", chain.handle(5) == "A handled 5")
test("chain B", chain.handle(15) == "B handled 15")
test("chain none", chain.handle(25) is None)

# 4. Builder pattern
class QueryBuilder:
    def __init__(self):
        self._table = ""
        self._conditions = []
        self._limit = None
    
    def table(self, name):
        self._table = name
        return self
    
    def where(self, condition):
        self._conditions.append(condition)
        return self
    
    def limit(self, n):
        self._limit = n
        return self
    
    def build(self):
        q = f"SELECT * FROM {self._table}"
        if self._conditions:
            q += " WHERE " + " AND ".join(self._conditions)
        if self._limit is not None:
            q += f" LIMIT {self._limit}"
        return q

query = QueryBuilder().table("users").where("age > 18").where("active = 1").limit(10).build()
test("builder", query == "SELECT * FROM users WHERE age > 18 AND active = 1 LIMIT 10")

# 5. Mixin pattern
class JsonMixin:
    def to_dict(self):
        return {k: v for k, v in self.__dict__.items() if not k.startswith("_")}

class ValidateMixin:
    def validate(self):
        for k, v in self.__dict__.items():
            if not k.startswith("_") and v is None:
                return False
        return True

class User(JsonMixin, ValidateMixin):
    def __init__(self, name, email):
        self.name = name
        self.email = email

u = User("Alice", "alice@example.com")
test("mixin to_dict", u.to_dict() == {"name": "Alice", "email": "alice@example.com"})
test("mixin validate", u.validate())

u2 = User("Bob", None)
test("mixin invalid", not u2.validate())

# 6. Recursive data structures
class TreeNode:
    def __init__(self, val, left=None, right=None):
        self.val = val
        self.left = left
        self.right = right

def tree_sum(node):
    if node is None:
        return 0
    return node.val + tree_sum(node.left) + tree_sum(node.right)

def tree_depth(node):
    if node is None:
        return 0
    return 1 + max(tree_depth(node.left), tree_depth(node.right))

tree = TreeNode(1, TreeNode(2, TreeNode(4), TreeNode(5)), TreeNode(3, None, TreeNode(6)))
test("tree sum", tree_sum(tree) == 21)
test("tree depth", tree_depth(tree) == 3)

# 7. Linked list
class ListNode:
    def __init__(self, val, next=None):
        self.val = val
        self.next = next

def list_to_array(head):
    result = []
    while head is not None:
        result.append(head.val)
        head = head.next
    return result

def reverse_list(head):
    prev = None
    current = head
    while current is not None:
        next_node = current.next
        current.next = prev
        prev = current
        current = next_node
    return prev

ll = ListNode(1, ListNode(2, ListNode(3, ListNode(4))))
test("linked list", list_to_array(ll) == [1, 2, 3, 4])
rev = reverse_list(ll)
test("reverse list", list_to_array(rev) == [4, 3, 2, 1])

# 8. Functional patterns — map/filter/reduce-like
def compose(*fns):
    def composed(x):
        result = x
        for f in reversed(fns):
            result = f(result)
        return result
    return composed

add1 = lambda x: x + 1
double = lambda x: x * 2
square = lambda x: x ** 2

transform = compose(square, double, add1)
test("compose", transform(3) == 64)  # (3+1)*2 = 8, 8^2 = 64

# 9. Pipeline pattern
def pipeline(data, *fns):
    result = data
    for f in fns:
        result = f(result)
    return result

result = pipeline(
    [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    lambda lst: [x for x in lst if x % 2 == 0],
    lambda lst: [x * x for x in lst],
    sum
)
test("pipeline", result == 220)  # 4+16+36+64+100 = 220

# 10. Complex generators
def fibonacci_gen():
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b

def take(n, gen):
    result = []
    for i, val in enumerate(gen):
        if i >= n:
            break
        result.append(val)
    return result

test("fib gen", take(10, fibonacci_gen()) == [0, 1, 1, 2, 3, 5, 8, 13, 21, 34])

# 11. Generator chaining
def gen_chain(*iterables):
    for it in iterables:
        yield from it

result = list(gen_chain([1, 2], [3, 4], [5, 6]))
test("gen chain", result == [1, 2, 3, 4, 5, 6])

# 12. Flatten nested lists
def flatten(lst):
    for item in lst:
        if isinstance(item, list):
            yield from flatten(item)
        else:
            yield item

nested = [1, [2, [3, 4]], [5, [6, [7]]]]
test("flatten", list(flatten(nested)) == [1, 2, 3, 4, 5, 6, 7])

# 13. Dynamic dispatch with dict
def handle_add(a, b):
    return a + b

def handle_mul(a, b):
    return a * b

def handle_sub(a, b):
    return a - b

operations = {
    "+": handle_add,
    "*": handle_mul,
    "-": handle_sub,
}

def calculate(op, a, b):
    return operations[op](a, b)

test("dispatch add", calculate("+", 3, 4) == 7)
test("dispatch mul", calculate("*", 3, 4) == 12)
test("dispatch sub", calculate("-", 10, 4) == 6)

# 14. Data transformation
records = [
    {"name": "Alice", "dept": "Engineering", "salary": 100000},
    {"name": "Bob", "dept": "Engineering", "salary": 95000},
    {"name": "Charlie", "dept": "Marketing", "salary": 85000},
    {"name": "Diana", "dept": "Marketing", "salary": 90000},
    {"name": "Eve", "dept": "Engineering", "salary": 110000},
]

# Group by department
by_dept = {}
for r in records:
    dept = r["dept"]
    if dept not in by_dept:
        by_dept[dept] = []
    by_dept[dept].append(r)

# Average salary per department
avg_salary = {}
for dept, members in by_dept.items():
    avg_salary[dept] = sum(m["salary"] for m in members) / len(members)

test("group by eng", len(by_dept["Engineering"]) == 3)
test("avg salary eng", abs(avg_salary["Engineering"] - 101666.67) < 1)
test("avg salary mkt", avg_salary["Marketing"] == 87500.0)

# 15. Memoization with dict
def memoize(func):
    cache = {}
    def wrapper(*args):
        if args not in cache:
            cache[args] = func(*args)
        return cache[args]
    return wrapper

@memoize
def expensive_fib(n):
    if n <= 1:
        return n
    return expensive_fib(n - 1) + expensive_fib(n - 2)

test("memo fib 30", expensive_fib(30) == 832040)
test("memo fib 40", expensive_fib(40) == 102334155)

# 16. State machine
class StateMachine:
    def __init__(self):
        self.state = "idle"
        self.transitions = {
            ("idle", "start"): "running",
            ("running", "pause"): "paused",
            ("paused", "resume"): "running",
            ("running", "stop"): "idle",
            ("paused", "stop"): "idle",
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
test("sm stopped", sm.state == "idle")
test("sm invalid", not sm.trigger("pause"))

# 17. Complex number operations
z1 = complex(3, 4)
z2 = complex(1, -2)
test("complex add", z1 + z2 == complex(4, 2))
test("complex mul", z1 * z2 == complex(11, -2))
test("complex abs", abs(z1) == 5.0)

print(f"\nTests: {total} | Passed: {passed} | Failed: {failed}")
if failed == 0:
    print("ALL PHASE 46 TESTS PASSED")
