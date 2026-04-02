"""Test suite 30: Complex OOP patterns, decorator stacking, introspection"""
passed = 0
failed = 0
def test(name, condition):
    global passed, failed
    if condition:
        passed += 1
    else:
        failed += 1
        print(f"  FAIL: {name}")

# ── Decorator stacking ──
def bold(func):
    def wrapper(*args, **kwargs):
        return f"<b>{func(*args, **kwargs)}</b>"
    return wrapper

def italic(func):
    def wrapper(*args, **kwargs):
        return f"<i>{func(*args, **kwargs)}</i>"
    return wrapper

@bold
@italic
def greet(name):
    return f"Hello, {name}"

test("stacked decorators", greet("World") == "<b><i>Hello, World</i></b>")

# ── Class decorator ──
def singleton(cls):
    instances = {}
    def get_instance(*args, **kwargs):
        if cls not in instances:
            instances[cls] = cls(*args, **kwargs)
        return instances[cls]
    return get_instance

@singleton
class Database:
    def __init__(self, url):
        self.url = url

db1 = Database("localhost")
db2 = Database("remote")
test("singleton", db1 is db2)
test("singleton url", db1.url == "localhost")

# ── Mixin pattern ──
class JsonMixin:
    def to_json(self):
        import json
        return json.dumps(vars(self))

class ReprMixin:
    def __repr__(self):
        attrs = ", ".join(f"{k}={v!r}" for k, v in vars(self).items())
        return f"{type(self).__name__}({attrs})"

class Config(JsonMixin, ReprMixin):
    def __init__(self, host, port):
        self.host = host
        self.port = port

c = Config("localhost", 8080)
test("mixin json", '"host"' in c.to_json() and '"localhost"' in c.to_json())
test("mixin repr", "Config(host='localhost'" in repr(c))

# ── Abstract pattern ──
class Shape:
    def area(self):
        raise NotImplementedError("Subclass must implement area()")
    
    def __str__(self):
        return f"{type(self).__name__}(area={self.area():.2f})"

class Circle(Shape):
    def __init__(self, r):
        self.r = r
    def area(self):
        return 3.14159 * self.r ** 2

class Square(Shape):
    def __init__(self, side):
        self.side = side
    def area(self):
        return self.side ** 2

shapes = [Circle(5), Square(4)]
test("polymorphism", abs(shapes[0].area() - 78.54) < 0.1)
test("polymorphism2", shapes[1].area() == 16)
test("polymorphism str", "Circle" in str(shapes[0]))

# ── Observer pattern ──
class EventEmitter:
    def __init__(self):
        self._listeners = {}
    
    def on(self, event, callback):
        if event not in self._listeners:
            self._listeners[event] = []
        self._listeners[event].append(callback)
        return self
    
    def emit(self, event, *args):
        for cb in self._listeners.get(event, []):
            cb(*args)

log = []
em = EventEmitter()
em.on("data", lambda x: log.append(f"got {x}"))
em.on("data", lambda x: log.append(f"also {x}"))
em.emit("data", 42)
test("observer", log == ["got 42", "also 42"])

# ── Chain of responsibility ──
class Handler:
    def __init__(self):
        self.next = None
    
    def set_next(self, handler):
        self.next = handler
        return handler
    
    def handle(self, request):
        if self.next:
            return self.next.handle(request)
        return None

class AuthHandler(Handler):
    def handle(self, request):
        if request.get("auth"):
            return super().handle(request)
        return "auth_failed"

class RateHandler(Handler):
    def handle(self, request):
        if request.get("rate_ok"):
            return super().handle(request)
        return "rate_limited"

class DataHandler(Handler):
    def handle(self, request):
        return "success"

auth = AuthHandler()
rate = RateHandler()
data = DataHandler()
auth.set_next(rate)
rate.set_next(data)

test("chain success", auth.handle({"auth": True, "rate_ok": True}) == "success")
test("chain auth fail", auth.handle({"auth": False}) == "auth_failed")
test("chain rate fail", auth.handle({"auth": True, "rate_ok": False}) == "rate_limited")

# ── Strategy pattern ──
class Sorter:
    def __init__(self, strategy):
        self.strategy = strategy
    
    def sort(self, data):
        return self.strategy(data)

bubble = Sorter(lambda data: sorted(data))
reverse_sort = Sorter(lambda data: sorted(data, reverse=True))

test("strategy 1", bubble.sort([3, 1, 2]) == [1, 2, 3])
test("strategy 2", reverse_sort.sort([3, 1, 2]) == [3, 2, 1])

# ── State machine ──
class FSM:
    def __init__(self):
        self.state = "idle"
        self.transitions = {}
    
    def add_transition(self, from_state, event, to_state, action=None):
        self.transitions[(from_state, event)] = (to_state, action)
    
    def trigger(self, event):
        key = (self.state, event)
        if key in self.transitions:
            to_state, action = self.transitions[key]
            self.state = to_state
            if action:
                action()
            return True
        return False

log2 = []
fsm = FSM()
fsm.add_transition("idle", "start", "running", lambda: log2.append("started"))
fsm.add_transition("running", "pause", "paused", lambda: log2.append("paused"))
fsm.add_transition("paused", "resume", "running", lambda: log2.append("resumed"))
fsm.add_transition("running", "stop", "idle", lambda: log2.append("stopped"))

fsm.trigger("start")
test("fsm state", fsm.state == "running")
fsm.trigger("pause")
fsm.trigger("resume")
fsm.trigger("stop")
test("fsm log", log2 == ["started", "paused", "resumed", "stopped"])
test("fsm final", fsm.state == "idle")

# ── Iterator tools ──
from itertools import chain, islice, count, repeat

# zip_longest equivalent using generators
def zip_longest_(*iterables, fillvalue=None):
    iters = [iter(it) for it in iterables]
    sentinel = object()
    while True:
        row = []
        active = False
        for it in iters:
            try:
                val = next(it)
                active = True
                row.append(val)
            except StopIteration:
                row.append(fillvalue)
        if not active:
            break
        yield tuple(row)

test("zip longest", list(zip_longest_([1,2,3], [4,5], fillvalue=0)) == [(1,4), (2,5), (3,0)])

# ── functools.reduce patterns ──
from functools import reduce

test("reduce sum", reduce(lambda a, b: a + b, range(1, 11)) == 55)
test("reduce max", reduce(lambda a, b: a if a > b else b, [3, 7, 2, 9, 4]) == 9)

# flatten with reduce
nested = [[1, 2], [3, 4], [5, 6]]
test("reduce flatten", reduce(lambda a, b: a + b, nested) == [1, 2, 3, 4, 5, 6])

# ── Data pipeline ──
data = [
    {"name": "Alice", "age": 30, "score": 85},
    {"name": "Bob", "age": 25, "score": 92},
    {"name": "Charlie", "age": 35, "score": 78},
    {"name": "Diana", "age": 28, "score": 95},
]

# Pipeline: filter age > 27, extract names, sort
pipeline = sorted(
    [d["name"] for d in data if d["age"] > 27]
)
test("pipeline", pipeline == ["Alice", "Charlie", "Diana"])

# Average score
avg = sum(d["score"] for d in data) / len(data)
test("avg score", avg == 87.5)

# Group by predicate
young = [d for d in data if d["age"] < 30]
senior = [d for d in data if d["age"] >= 30]
test("partition", len(young) == 2 and len(senior) == 2)

# ── Recursive algorithms ──
def quicksort(lst):
    if len(lst) <= 1:
        return lst
    pivot = lst[len(lst) // 2]
    left = [x for x in lst if x < pivot]
    middle = [x for x in lst if x == pivot]
    right = [x for x in lst if x > pivot]
    return quicksort(left) + middle + quicksort(right)

test("quicksort", quicksort([3, 6, 8, 10, 1, 2, 1]) == [1, 1, 2, 3, 6, 8, 10])

# ── GCD/LCM ──
def gcd(a, b):
    while b:
        a, b = b, a % b
    return a

def lcm(a, b):
    return a * b // gcd(a, b)

test("gcd", gcd(48, 18) == 6)
test("lcm", lcm(4, 6) == 12)

# ── Fibonacci (non-generator version for list test) ──
def fib_list(n):
    result = []
    a, b = 0, 1
    for _ in range(n):
        result.append(a)
        a, b = b, a + b
    return result

test("fib", fib_list(10) == [0, 1, 1, 2, 3, 5, 8, 13, 21, 34])

print(f"\nTests: {passed + failed} | Passed: {passed} | Failed: {failed}")
